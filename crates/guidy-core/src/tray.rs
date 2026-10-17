//! System tray icon and menu.
//!
//! The tray is driven by a single *desired state* snapshot ([`TrayDesired`])
//! that callers update through [`set_tray_state`], [`refresh_tray_icon`] and
//! [`update_tray_menu`]. Every such call just records intent and schedules a
//! single applier on the main thread, which diffs the desired snapshot against
//! what is currently displayed and touches the native tray only for the parts
//! that actually changed. Requests that arrive while an apply is pending are
//! coalesced into it, so bursts of state changes never queue up native work.
//!
//! Why: native tray updates are the lever we control for the macOS tray
//! disappearance bug (tauri-apps/tauri#12060). Before this, every
//! recording cycle rebuilt the full menu 3-6 times from several threads, and
//! concurrent rebuilds could interleave and leave a stale menu behind.
//!
//! Exception: [`set_tray_visibility`] and [`recreate_tray_icon`] call the tray
//! directly. Visibility is a separate attribute that never participates in the
//! icon/menu diff, both are rare and user-initiated, and they are marshalled
//! onto the main thread so they serialize with the applier anyway. Re-showing
//! a hidden tray relies on tray-icon recreating it from the last applied
//! icon/menu/tooltip, so those must only ever be set through the applier.

use crate::host::image::Image;
use crate::host::{AppHandle, Theme};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings;
use crate::tray_i18n::get_tray_translations;
use log::{debug, error, info, trace, warn};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

/// Managed marker: present once the native tray has been built. The native
/// `tray_icon::TrayIcon` itself is main-thread-only and lives in [`NATIVE`].
pub struct TrayIcon;

thread_local! {
    static NATIVE: RefCell<Option<tray_icon::TrayIcon>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconState {
    Idle,
    Recording,
    Transcribing,
}

impl TrayIconState {
    /// Recording and Transcribing share the same menu ("Cancel" instead of the
    /// model submenu), so only the idle/busy distinction matters for the menu.
    fn is_busy(self) -> bool {
        self != TrayIconState::Idle
    }
}

/// Everything the tray *menu* (and tooltip) depends on. When two snapshots
/// compare equal the menu is not rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuInputs {
    busy: bool,
    warning: bool,
    model_loaded: bool,
    selected_model: String,
    /// `(id, name)` of downloaded models, sorted by name.
    downloaded_models: Vec<(String, String)>,
    locale: String,
    update_checks_enabled: bool,
    meeting: MeetingMenu,
}

/// The meeting-notes part of the tray menu.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct MeetingMenu {
    /// Show "Start/Stop Meeting Notes" (supported macOS, meetings enabled,
    /// or a recording in progress).
    available: bool,
    recording: bool,
    /// Title of the meeting being recorded (the linked event's title).
    title: Option<String>,
}

fn meeting_menu(app: &AppHandle, settings: &settings::AppSettings) -> MeetingMenu {
    let manager = app.try_state::<Arc<crate::meetings::manager::MeetingsManager>>();
    let state = manager
        .as_ref()
        .map(|m| m.recorder_state())
        .unwrap_or_default();
    match state {
        crate::meetings::types::RecorderState::Recording { title, .. } => MeetingMenu {
            available: true,
            recording: true,
            title: Some(title),
        },
        crate::meetings::types::RecorderState::Idle => MeetingMenu {
            available: manager.is_some()
                && settings.meetings.enabled
                && crate::meetings::is_supported(),
            recording: false,
            title: None,
        },
    }
}

/// A meeting recording shows the recording icon unless dictation is busy
/// (which has its own icons).
fn effective_icon_state(dictation: TrayIconState, meeting_recording: bool) -> TrayIconState {
    if meeting_recording && dictation == TrayIconState::Idle {
        TrayIconState::Recording
    } else {
        dictation
    }
}

/// Complete description of what the tray should look like.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayDesired {
    icon_path: &'static str,
    menu: MenuInputs,
}

struct TrayInner {
    /// Intent set by [`set_tray_state`].
    icon_state: TrayIconState,
    /// Latest computed snapshot, waiting to be (or just) applied.
    desired: Option<TrayDesired>,
    /// Icon the native tray currently shows. Only updated when `set_icon`
    /// succeeds, so a failed update is retried on the next sync.
    applied_icon: Option<&'static str>,
    /// Inputs the native menu was last successfully built from. The tooltip
    /// is derived from the same inputs and set best-effort alongside the menu;
    /// it is not tracked separately.
    applied_menu: Option<MenuInputs>,
    /// An apply is scheduled on the main thread.
    pending: bool,
    /// Decoded icons by resource path so the main thread never touches disk.
    icons: HashMap<&'static str, Image<'static>>,
    /// Handed out to each sync request in trigger order, so a slow request
    /// can't overwrite the snapshot of one that was triggered after it.
    next_seq: u64,
    /// Sequence number of the request that produced `desired`.
    desired_seq: u64,
}

/// Tauri managed state owning the tray's desired/applied snapshots.
pub struct TrayState(Mutex<TrayInner>);

impl TrayState {
    pub fn new() -> Self {
        Self(Mutex::new(TrayInner {
            icon_state: TrayIconState::Idle,
            desired: None,
            applied_icon: None,
            applied_menu: None,
            pending: false,
            icons: HashMap::new(),
            next_seq: 0,
            desired_seq: 0,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, TrayInner> {
        self.0.lock().unwrap_or_else(|poisoned| {
            warn!("Tray state mutex was poisoned, recovering");
            poisoned.into_inner()
        })
    }
}

impl Default for TrayState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppTheme {
    Dark,
    Light,
    Colored, // Full-color icons for Linux
}

/// Gets the current app theme, with Linux defaulting to Colored theme
pub fn get_current_theme(app: &AppHandle) -> AppTheme {
    if cfg!(target_os = "linux") {
        // On Linux, always use the colored theme
        AppTheme::Colored
    } else {
        // On Windows the tray icon sits on the taskbar, which follows the
        // *system* theme (SystemUsesLightTheme), not the app theme. With the
        // "Custom" personalization mode the two can differ (e.g. dark taskbar
        // + light apps), and the window theme would pick an icon that is
        // invisible against the taskbar.
        #[cfg(target_os = "windows")]
        if let Some(theme) = windows_taskbar_theme() {
            return theme;
        }

        // On other platforms, map the system appearance to our app theme
        match app.shell().system_theme().unwrap_or(Theme::Dark) {
            Theme::Light => AppTheme::Light,
            Theme::Dark => AppTheme::Dark,
        }
    }
}

/// Reads the Windows taskbar theme from the registry.
///
/// Returns None if the value is missing (older Windows 10 builds default to a
/// dark taskbar there, but falling back to the window theme is safer than
/// guessing).
#[cfg(target_os = "windows")]
fn windows_taskbar_theme() -> Option<AppTheme> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let personalize = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .ok()?;
    let system_uses_light: u32 = personalize.get_value("SystemUsesLightTheme").ok()?;
    Some(if system_uses_light == 1 {
        AppTheme::Light
    } else {
        AppTheme::Dark
    })
}

/// Gets the appropriate icon path for the given theme and state.
///
/// `warning` overlays a badge on the idle icon while keyboard shortcuts are
/// blocked (macOS Secure Input); recording/transcribing states keep their
/// normal icons so in-flight activity stays recognizable.
pub fn get_icon_path(theme: AppTheme, state: TrayIconState, warning: bool) -> &'static str {
    if warning && state == TrayIconState::Idle {
        return match theme {
            AppTheme::Dark => "resources/tray_idle_warning.png",
            AppTheme::Light => "resources/tray_idle_warning_dark.png",
            // Linux never sets the warning flag (Secure Input is macOS-only),
            // but fall back to the normal icon just in case.
            AppTheme::Colored => "resources/tray_colored.png",
        };
    }
    match (theme, state) {
        // Dark theme uses light icons
        (AppTheme::Dark, TrayIconState::Idle) => "resources/tray_idle.png",
        (AppTheme::Dark, TrayIconState::Recording) => "resources/tray_recording.png",
        (AppTheme::Dark, TrayIconState::Transcribing) => "resources/tray_transcribing.png",
        // Light theme uses dark icons
        (AppTheme::Light, TrayIconState::Idle) => "resources/tray_idle_dark.png",
        (AppTheme::Light, TrayIconState::Recording) => "resources/tray_recording_dark.png",
        (AppTheme::Light, TrayIconState::Transcribing) => "resources/tray_transcribing_dark.png",
        // Colored theme uses full-color icons (for Linux)
        (AppTheme::Colored, TrayIconState::Idle) => "resources/tray_colored.png",
        (AppTheme::Colored, TrayIconState::Recording) => "resources/tray_colored_recording.png",
        (AppTheme::Colored, TrayIconState::Transcribing) => {
            "resources/tray_colored_transcribing.png"
        }
    }
}

/// Sets the recording state shown by the tray (icon + Cancel/model menu).
pub fn set_tray_state(app: &AppHandle, state: TrayIconState) {
    sync_tray_with(app, |inner| inner.icon_state = state);
}

/// Re-syncs the tray after something other than the recording state changed
/// (theme, Secure Input warning). The recording state itself is preserved.
pub fn refresh_tray_icon(app: &AppHandle) {
    sync_tray(app);
}

/// Re-syncs the tray after something the menu depends on changed (model
/// list/selection/loaded state, language, settings).
pub fn update_tray_menu(app: &AppHandle) {
    sync_tray(app);
}

/// Records the current desired tray state and schedules one apply on the main
/// thread (or lets an already-pending apply pick it up). Never blocks on the
/// main thread.
///
/// The snapshot (settings, model list, loaded state) is computed on the
/// *calling* thread on purpose: the main-thread applier must not take manager
/// locks that a worker may hold across slow work (see #1716).
pub fn sync_tray(app: &AppHandle) {
    sync_tray_with(app, |_| {});
}

fn sync_tray_with(app: &AppHandle, update: impl FnOnce(&mut TrayInner)) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };

    // Record intent and claim a sequence number in one critical section, so
    // sequence order == the order in which state changes were requested.
    let (seq, icon_state) = {
        let mut inner = state.lock();
        update(&mut inner);
        inner.next_seq += 1;
        (inner.next_seq, inner.icon_state)
    };

    // Tray not built yet (early secure-input monitor callbacks). The intent
    // is kept and picked up by the first sync after the tray exists.
    if app.try_state::<TrayIcon>().is_none() {
        return;
    }

    let desired = compute_desired(app, icon_state);

    // Decode the icon off the main thread, once per path, outside the lock.
    let needs_icon = !state.lock().icons.contains_key(desired.icon_path);
    let loaded_icon = if needs_icon {
        match load_tray_icon(app.path().resolve(
            desired.icon_path,
            crate::host::path::BaseDirectory::Resource,
        )) {
            Ok(image) => Some(image),
            Err(err) => {
                error!("Failed to load tray icon '{}': {err}", desired.icon_path);
                None
            }
        }
    } else {
        None
    };

    let schedule = {
        let mut inner = state.lock();
        if let Some(image) = loaded_icon {
            inner.icons.insert(desired.icon_path, image);
        }
        if seq < inner.desired_seq {
            // A request triggered after this one already stored its snapshot
            // (and scheduled an apply). Ours is stale; drop it.
            trace!(
                "tray sync: request {seq} superseded by {}",
                inner.desired_seq
            );
            return;
        }
        inner.desired = Some(desired);
        inner.desired_seq = seq;
        // If an apply is already pending it will read the snapshot we just
        // stored; otherwise schedule one.
        !std::mem::replace(&mut inner.pending, true)
    };

    if schedule {
        post_apply(app);
    } else {
        trace!("tray sync: apply already pending");
    }
}

fn compute_desired(app: &AppHandle, icon_state: TrayIconState) -> TrayDesired {
    let settings = settings::get_settings(app);
    let theme = get_current_theme(app);
    let warning = crate::secure_input::tray_warning_active(app);
    let model_loaded = app.state::<Arc<TranscriptionManager>>().is_model_loaded();

    let mut downloaded_models: Vec<(String, String)> = app
        .state::<Arc<ModelManager>>()
        .get_available_models()
        .into_iter()
        .filter(|m| m.is_downloaded)
        .map(|m| (m.id, m.name))
        .collect();
    downloaded_models.sort_by(|a, b| a.1.cmp(&b.1));

    let meeting = meeting_menu(app, &settings);
    let icon_state = effective_icon_state(icon_state, meeting.recording);

    TrayDesired {
        icon_path: get_icon_path(theme, icon_state, warning),
        menu: MenuInputs {
            busy: icon_state.is_busy(),
            warning,
            model_loaded,
            selected_model: settings.selected_model,
            downloaded_models,
            locale: settings.app_language,
            update_checks_enabled: settings.update_checks_enabled,
            meeting,
        },
    }
}

/// Run `f` on the thread that owns the native tray: the app's main thread,
/// or on Linux the tray's own GTK thread (see [`build_tray`]).
fn on_tray_thread(app: &AppHandle, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        gtk::glib::MainContext::default().invoke(f);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        app.run_on_main_thread(f).map_err(|e| e.to_string())
    }
}

fn post_apply(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = on_tray_thread(app, move || apply_on_main(&handle)) {
        // Event loop is gone (shutdown). Clear `pending` so a later call, if
        // any, doesn't wait forever for an apply that will never run.
        error!("Failed to dispatch tray update to the main thread: {err}");
        if let Some(state) = app.try_state::<TrayState>() {
            state.lock().pending = false;
        }
    }
}

/// The single writer to the native tray. Runs on the tray thread.
fn apply_on_main(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    if app.try_state::<TrayIcon>().is_none() {
        return;
    }

    let started = Instant::now();
    let (desired, icon, icon_changed, menu_changed) = {
        let mut inner = state.lock();
        inner.pending = false;
        let Some(desired) = inner.desired.clone() else {
            return;
        };
        let icon_changed = inner.applied_icon != Some(desired.icon_path);
        let menu_changed = inner.applied_menu.as_ref() != Some(&desired.menu);
        if !icon_changed && !menu_changed {
            trace!("tray apply: nothing changed");
            return;
        }
        let icon = inner.icons.get(desired.icon_path).cloned();
        (desired, icon, icon_changed, menu_changed)
    };

    // Each part is recorded as applied only if its native call succeeded, so a
    // transient failure is retried on the next sync instead of being
    // remembered as displayed.
    let (icon_ok, menu_ok) = NATIVE.with(|native| {
        let native = native.borrow();
        let Some(tray) = native.as_ref() else {
            return (false, false);
        };
        let mut icon_ok = false;
        if icon_changed {
            match icon.map(to_native_icon) {
                Some(Ok(image)) => match tray.set_icon_with_as_template(Some(image), true) {
                    Ok(()) => icon_ok = true,
                    Err(err) => error!("Failed to update tray icon '{}': {err}", desired.icon_path),
                },
                Some(Err(err)) => error!("Bad tray icon '{}': {err}", desired.icon_path),
                None => error!("Tray icon '{}' is not loaded", desired.icon_path),
            }
        }

        let mut menu_ok = false;
        if menu_changed {
            let model = build_menu(&desired.menu);
            match to_native_menu(&model) {
                Ok(menu) => {
                    tray.set_menu(Some(Box::new(menu)));
                    menu_ok = true;
                    // Best-effort: the tooltip is cosmetic.
                    if let Err(err) = tray.set_tooltip(Some(model.tooltip.clone())) {
                        error!("Failed to set tray tooltip: {err}");
                    }
                }
                Err(err) => error!("Failed to build tray menu: {err}"),
            }
        }
        (icon_ok, menu_ok)
    });

    {
        let mut inner = state.lock();
        if icon_ok {
            inner.applied_icon = Some(desired.icon_path);
        }
        if menu_ok {
            inner.applied_menu = Some(desired.menu.clone());
        }
    }

    debug!(
        "tray apply: icon={} menu={} busy={} took={:?}",
        if icon_changed {
            desired.icon_path
        } else {
            "unchanged"
        },
        if menu_changed { "rebuilt" } else { "unchanged" },
        desired.menu.busy,
        started.elapsed()
    );
}

fn load_tray_icon(
    resolved_icon_path: crate::host::Result<PathBuf>,
) -> crate::host::Result<Image<'static>> {
    let resolved_icon_path = resolved_icon_path?;
    let decoded = image::open(&resolved_icon_path)
        .map_err(|e| crate::host::Error::Other(format!("{}: {e}", resolved_icon_path.display())))?
        .into_rgba8();
    let (w, h) = decoded.dimensions();
    Ok(Image::new_owned(decoded.into_raw(), w, h))
}

fn to_native_icon(image: Image<'static>) -> Result<tray_icon::Icon, String> {
    tray_icon::Icon::from_rgba(image.rgba().to_vec(), image.width(), image.height())
        .map_err(|e| e.to_string())
}

pub fn tray_tooltip() -> String {
    version_label()
}

fn version_label() -> String {
    if cfg!(debug_assertions) {
        format!(
            "{} v{} (Dev)",
            crate::host::PRODUCT_NAME,
            env!("CARGO_PKG_VERSION")
        )
    } else {
        format!(
            "{} v{}",
            crate::host::PRODUCT_NAME,
            env!("CARGO_PKG_VERSION")
        )
    }
}

/// One tray menu entry, independent of the native menu toolkit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
        accelerator: Option<&'static str>,
    },
    Check {
        id: String,
        label: String,
        checked: bool,
    },
    Submenu {
        id: String,
        label: String,
        items: Vec<MenuEntry>,
    },
    Separator,
}

impl MenuEntry {
    fn item(id: &str, label: &str, enabled: bool, accelerator: Option<&'static str>) -> Self {
        MenuEntry::Item {
            id: id.to_string(),
            label: label.to_string(),
            enabled,
            accelerator,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            MenuEntry::Item { id, .. }
            | MenuEntry::Check { id, .. }
            | MenuEntry::Submenu { id, .. } => Some(id),
            MenuEntry::Separator => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuModel {
    pub entries: Vec<MenuEntry>,
    pub tooltip: String,
}

/// Builds the tray menu and tooltip for the given inputs. Pure: everything it
/// depends on is in `inputs`, plus the process-constant updater-disable env
/// flag behind `update_checks_forced_disabled()`.
fn build_menu(inputs: &MenuInputs) -> MenuModel {
    let strings = get_tray_translations(Some(inputs.locale.clone()));

    // Secure Input warning entry (macOS): clicking opens the settings window
    // where the full warning banner explains the situation. Locales that
    // haven't translated the key yet get the English string rather than a
    // blank menu item (build.rs emits "" for missing keys).
    let secure_input_warning = if inputs.warning {
        let label = if strings.secure_input_warning.is_empty() {
            get_tray_translations(Some("en".to_string())).secure_input_warning
        } else {
            strings.secure_input_warning.clone()
        };
        Some(label)
    } else {
        None
    };

    // Platform-specific accelerators
    #[cfg(target_os = "macos")]
    let (settings_accelerator, quit_accelerator) = (Some("Cmd+,"), Some("Cmd+Q"));
    #[cfg(not(target_os = "macos"))]
    let (settings_accelerator, quit_accelerator) = (Some("Ctrl+,"), Some("Ctrl+Q"));

    let version_label = version_label();
    let version_i = MenuEntry::item("version", &version_label, false, None);
    let settings_i = MenuEntry::item("settings", &strings.settings, true, settings_accelerator);
    // When update checks are forced off the item is dropped rather than shown
    // disabled — it can never do anything in that case. A manually-disabled
    // toggle keeps the greyed-out behavior via the enabled flag.
    let check_updates_i = (!settings::update_checks_forced_disabled()).then(|| {
        MenuEntry::item(
            "check_updates",
            &strings.check_updates,
            inputs.update_checks_enabled,
            None,
        )
    });
    let copy_last_transcript_i = MenuEntry::item(
        "copy_last_transcript",
        &strings.copy_last_transcript,
        true,
        None,
    );
    let quit_i = MenuEntry::item("quit", &strings.quit, true, quit_accelerator);

    let mut entries = vec![version_i, MenuEntry::Separator];
    if inputs.busy {
        entries.push(MenuEntry::item("cancel", &strings.cancel, true, None));
        entries.push(MenuEntry::Separator);
        entries.push(copy_last_transcript_i);
        entries.push(MenuEntry::Separator);
    } else {
        // Build model submenu — label is the active model name
        let submenu_label = inputs
            .downloaded_models
            .iter()
            .find(|(id, _)| *id == inputs.selected_model)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| strings.model.clone());
        let items = inputs
            .downloaded_models
            .iter()
            .map(|(id, name)| MenuEntry::Check {
                id: format!("model_select:{}", id),
                label: name.clone(),
                checked: *id == inputs.selected_model,
            })
            .collect();
        entries.push(copy_last_transcript_i);
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Submenu {
            id: "model_submenu".to_string(),
            label: submenu_label,
            items,
        });
        entries.push(MenuEntry::item(
            "unload_model",
            &strings.unload_model,
            inputs.model_loaded,
            None,
        ));
        entries.push(MenuEntry::Separator);
    }
    entries.push(settings_i);
    if let Some(item) = check_updates_i {
        entries.push(item);
    }
    entries.push(MenuEntry::Separator);
    entries.push(quit_i);

    // Meeting notes go right below the version line.
    if inputs.meeting.available {
        let en = get_tray_translations(Some("en".to_string()));
        let or_en = |s: &str, e: &str| {
            if s.is_empty() {
                e.to_string()
            } else {
                s.to_string()
            }
        };
        let mut items = Vec::new();
        if inputs.meeting.recording {
            if let Some(title) = inputs.meeting.title.as_deref().filter(|t| !t.is_empty()) {
                items.push(MenuEntry::item(
                    "meeting_title",
                    &format!("● {title}"),
                    false,
                    None,
                ));
            }
            items.push(MenuEntry::item(
                "meeting_stop",
                &or_en(&strings.stop_meeting_notes, &en.stop_meeting_notes),
                true,
                None,
            ));
        } else {
            items.push(MenuEntry::item(
                "meeting_start",
                &or_en(&strings.start_meeting_notes, &en.start_meeting_notes),
                true,
                None,
            ));
        }
        items.push(MenuEntry::Separator);
        entries.splice(2..2, items);
    }

    // Both layouts start with [version, separator, ...]; slot the warning in
    // right below the version line so it's the first actionable thing seen.
    let mut tooltip = version_label;
    if let Some(label) = secure_input_warning {
        tooltip = format!("{} — {}", tooltip, label);
        entries.insert(
            2,
            MenuEntry::item("secure_input_warning", &label, true, None),
        );
        entries.insert(3, MenuEntry::Separator);
    }

    MenuModel { entries, tooltip }
}

fn to_native_menu(model: &MenuModel) -> Result<tray_icon::menu::Menu, String> {
    use tray_icon::menu::{
        accelerator::Accelerator, CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem,
        Submenu,
    };

    fn build(entry: &MenuEntry) -> Result<Box<dyn IsMenuItem>, String> {
        Ok(match entry {
            MenuEntry::Item {
                id,
                label,
                enabled,
                accelerator,
            } => {
                let accel = accelerator.and_then(|a| a.parse::<Accelerator>().ok());
                Box::new(MenuItem::with_id(id.clone(), label, *enabled, accel))
            }
            MenuEntry::Check { id, label, checked } => Box::new(CheckMenuItem::with_id(
                id.clone(),
                label,
                true,
                *checked,
                None,
            )),
            MenuEntry::Submenu { id, label, items } => {
                let sub = Submenu::with_id(id.clone(), label, true);
                for item in items {
                    let built = build(item)?;
                    sub.append(built.as_ref()).map_err(|e| e.to_string())?;
                }
                Box::new(sub)
            }
            MenuEntry::Separator => Box::new(PredefinedMenuItem::separator()),
        })
    }

    let menu = Menu::new();
    for entry in &model.entries {
        let built = build(entry)?;
        menu.append(built.as_ref()).map_err(|e| e.to_string())?;
    }
    Ok(menu)
}

fn last_transcript_text(entry: &HistoryEntry) -> &str {
    entry
        .post_processed_text
        .as_deref()
        .unwrap_or(&entry.transcription_text)
}

pub fn set_tray_visibility(app: &AppHandle, visible: bool) {
    let _ = on_tray_thread(app, move || {
        NATIVE.with(|native| {
            if let Some(tray) = native.borrow().as_ref() {
                if let Err(e) = tray.set_visible(visible) {
                    error!("Failed to set tray visibility: {}", e);
                } else {
                    info!("Tray visibility set to: {}", visible);
                }
            }
        })
    });
}

/// Recovery for the macOS tray-disappearance bug (#1948, tauri-apps/tauri#12060):
/// the `NSStatusItem` can silently vanish with no error surfaced to the app.
/// Hiding and re-showing the tray recreates it with its current icon, menu and
/// tooltip. Called when the user "relaunches" Guidy while it is already running
/// (`RunEvent::Reopen` for Spotlight/Finder/Dock, the single-instance callback
/// for a second process) — the natural "where did my icon go?" moment — so a
/// relaunch brings the icon back without a full quit.
#[cfg(target_os = "macos")]
pub fn recreate_tray_icon(app: &AppHandle) {
    let no_tray = app
        .try_state::<crate::cli::CliArgs>()
        .map(|args| args.no_tray)
        .unwrap_or(false);
    if no_tray || !settings::get_settings(app).show_tray_icon {
        return;
    }
    info!("Recreating tray icon on relaunch");
    let _ = app.run_on_main_thread(|| {
        NATIVE.with(|native| {
            if let Some(tray) = native.borrow().as_ref() {
                if let Err(e) = tray.set_visible(false).and_then(|_| tray.set_visible(true)) {
                    error!("Failed to recreate tray icon: {}", e);
                }
            }
        })
    });
}

pub fn copy_last_transcript(app: &AppHandle) {
    let history_manager = app.state::<Arc<HistoryManager>>();
    let entry = match history_manager.get_latest_completed_entry() {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            warn!("No completed transcription history entries available for tray copy.");
            return;
        }
        Err(err) => {
            error!(
                "Failed to fetch last completed transcription entry: {}",
                err
            );
            return;
        }
    };

    let text = last_transcript_text(&entry);
    if text.trim().is_empty() {
        warn!("Last completed transcription is empty; skipping tray copy.");
        return;
    }

    if let Err(err) = app.clipboard().write_text(text.to_string()) {
        error!("Failed to copy last transcript to clipboard: {}", err);
        return;
    }

    info!("Copied last transcript to clipboard via tray.");
}

/// Build the native tray icon. Must run on the main thread, after the
/// managers and [`TrayState`] are managed.
#[cfg(not(target_os = "linux"))]
pub fn build_tray(app: &AppHandle) -> Result<(), String> {
    build_native_tray(app)?;
    app.manage(TrayIcon);
    Ok(())
}

/// Linux: libappindicator needs GTK initialized, and its main loop running,
/// on the thread that owns the tray (Tauri ran GTK on the main thread; GPUI
/// doesn't use GTK). The tray lives on a dedicated GTK thread and every
/// native tray call is sent there ([`on_tray_thread`]).
#[cfg(target_os = "linux")]
pub fn build_tray(app: &AppHandle) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tray_app = app.clone();
    std::thread::Builder::new()
        .name("tray-gtk".into())
        .spawn(move || {
            if let Err(e) = gtk::init() {
                let _ = tx.send(Err(format!("GTK init failed: {e}")));
                return;
            }
            let built = build_native_tray(&tray_app);
            let ok = built.is_ok();
            let _ = tx.send(built);
            if ok {
                gtk::main();
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())??;
    app.manage(TrayIcon);
    Ok(())
}

/// Create the native tray on the current thread (which then owns it).
fn build_native_tray(app: &AppHandle) -> Result<(), String> {
    let initial_theme = get_current_theme(app);
    let initial_icon_path = get_icon_path(initial_theme, TrayIconState::Idle, false);
    let icon = load_tray_icon(app.path().resolve(
        initial_icon_path,
        crate::host::path::BaseDirectory::Resource,
    ))
    .map_err(|e| e.to_string())
    .and_then(to_native_icon)?;

    // Windows notification-area convention: left click opens the app, right
    // click shows the menu. Elsewhere (macOS menu bar, Linux) the menu stays
    // on left click.
    let tray = tray_icon::TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(tray_tooltip())
        .with_icon_as_template(true)
        .with_menu_on_left_click(!cfg!(target_os = "windows"))
        .build()
        .map_err(|e| e.to_string())?;
    NATIVE.with(|native| *native.borrow_mut() = Some(tray));

    let menu_app = app.clone();
    tray_icon::menu::MenuEvent::set_event_handler(Some(
        move |event: tray_icon::menu::MenuEvent| {
            handle_menu_event(&menu_app, event.id.as_ref());
        },
    ));

    #[cfg(target_os = "windows")]
    {
        let click_app = app.clone();
        tray_icon::TrayIconEvent::set_event_handler(Some(
            move |event: tray_icon::TrayIconEvent| {
                use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
                let opens_window = matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                );
                if opens_window {
                    click_app.shell().show_main_window();
                }
            },
        ));
    }

    Ok(())
}

/// Tray menu actions.
pub fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "settings" | "secure_input_warning" => {
            // The secure-input entry opens the window, where the full
            // warning banner explains the situation.
            app.shell().show_main_window();
        }
        "check_updates" => {
            let settings = settings::get_settings(app);
            if settings::update_checks_effectively_enabled(&settings) {
                app.shell().show_main_window();
                let _ = app.emit("check-for-updates", ());
            }
        }
        "copy_last_transcript" => {
            copy_last_transcript(app);
        }
        "unload_model" => {
            let transcription_manager = app.state::<Arc<TranscriptionManager>>();
            if !transcription_manager.is_model_loaded() {
                warn!("No model is currently loaded.");
                return;
            }
            transcription_manager.request_unload();
            info!("Model unloaded via tray.");
        }
        "cancel" => {
            crate::utils::cancel_current_operation(app);
        }
        "meeting_start" | "meeting_stop" => {
            let Some(manager) = app
                .try_state::<Arc<crate::meetings::manager::MeetingsManager>>()
                .map(|m| m.inner().clone())
            else {
                return;
            };
            let start = id == "meeting_start";
            // Capture start/stop and calendar lookups must not block the
            // main thread.
            std::thread::spawn(move || {
                let result = if start {
                    manager.start_recording(None).map(|_| ())
                } else {
                    manager.stop_recording().map(|_| ())
                };
                if let Err(e) = result {
                    error!(
                        "Meeting notes {} from tray failed: {e:#}",
                        if start { "start" } else { "stop" }
                    );
                }
            });
        }
        "quit" => {
            app.exit(0);
        }
        id if id.starts_with("model_select:") => {
            let model_id = id.strip_prefix("model_select:").unwrap().to_string();
            let current_model = settings::get_settings(app).selected_model;
            if model_id == current_model {
                return;
            }
            let app_clone = app.clone();
            std::thread::spawn(move || {
                match crate::commands::models::switch_active_model(&app_clone, &model_id) {
                    Ok(()) => info!("Model switched to {} via tray.", model_id),
                    Err(e) => error!("Failed to switch model via tray: {}", e),
                }
                update_tray_menu(&app_clone);
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_menu, effective_icon_state, last_transcript_text, load_tray_icon, MeetingMenu,
        MenuEntry, MenuInputs, TrayDesired, TrayIconState,
    };
    use crate::managers::history::HistoryEntry;

    fn build_entry(transcription: &str, post_processed: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            file_name: "guidy-1.wav".to_string(),
            timestamp: 0,
            saved: false,
            title: "Recording".to_string(),
            transcription_text: transcription.to_string(),
            post_processed_text: post_processed.map(|text| text.to_string()),
            post_process_prompt: None,
            post_process_requested: false,
        }
    }

    fn inputs(busy: bool) -> MenuInputs {
        MenuInputs {
            busy,
            warning: false,
            model_loaded: true,
            selected_model: "small".to_string(),
            downloaded_models: vec![("small".to_string(), "Small".to_string())],
            locale: "en".to_string(),
            update_checks_enabled: true,
            meeting: MeetingMenu::default(),
        }
    }

    #[test]
    fn uses_post_processed_text_when_available() {
        let entry = build_entry("raw", Some("processed"));
        assert_eq!(last_transcript_text(&entry), "processed");
    }

    #[test]
    fn falls_back_to_raw_transcription() {
        let entry = build_entry("raw", None);
        assert_eq!(last_transcript_text(&entry), "raw");
    }

    #[test]
    fn tray_icon_resolution_failure_is_returned_instead_of_panicking() {
        assert!(load_tray_icon(Err(crate::host::Error::UnknownPath)).is_err());
    }

    #[test]
    fn tray_icon_returns_err_when_file_does_not_exist() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let missing = dir.path().join("does_not_exist.png");
        assert!(load_tray_icon(Ok(missing)).is_err());
    }

    #[test]
    fn recording_and_transcribing_share_a_menu() {
        // The icon differs but the menu inputs are identical, so a
        // Recording -> Transcribing transition must not rebuild the menu.
        let recording = TrayDesired {
            icon_path: "resources/tray_recording.png",
            menu: inputs(TrayIconState::Recording.is_busy()),
        };
        let transcribing = TrayDesired {
            icon_path: "resources/tray_transcribing.png",
            menu: inputs(TrayIconState::Transcribing.is_busy()),
        };
        assert_ne!(recording.icon_path, transcribing.icon_path);
        assert_eq!(recording.menu, transcribing.menu);
    }

    #[test]
    fn idle_and_busy_menus_differ() {
        assert_ne!(inputs(false), inputs(true));
    }

    fn ids(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| e.id().unwrap_or("---").to_string())
            .collect()
    }

    #[test]
    fn idle_menu_has_model_submenu_and_unload() {
        let menu = build_menu(&inputs(false));
        let ids = ids(&menu.entries);
        assert_eq!(ids[0], "version");
        assert!(ids.contains(&"model_submenu".to_string()));
        assert!(ids.contains(&"unload_model".to_string()));
        assert!(!ids.contains(&"cancel".to_string()));
        assert_eq!(ids.last().unwrap(), "quit");
        let sub = menu
            .entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Submenu { label, items, .. } => Some((label.clone(), items.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(sub.0, "Small");
        assert_eq!(
            sub.1,
            vec![MenuEntry::Check {
                id: "model_select:small".into(),
                label: "Small".into(),
                checked: true
            }]
        );
    }

    #[test]
    fn busy_menu_offers_cancel_instead_of_models() {
        let ids = ids(&build_menu(&inputs(true)).entries);
        assert_eq!(ids[2], "cancel");
        assert!(!ids.contains(&"model_submenu".to_string()));
    }

    #[test]
    fn secure_input_warning_is_slotted_below_version() {
        let mut i = inputs(false);
        i.warning = true;
        let menu = build_menu(&i);
        assert_eq!(menu.entries[2].id(), Some("secure_input_warning"));
        assert_eq!(menu.entries[3], MenuEntry::Separator);
        assert!(menu.tooltip.contains(" — "));
    }

    #[test]
    fn unload_disabled_when_no_model_loaded_and_updates_disabled() {
        let mut i = inputs(false);
        i.model_loaded = false;
        i.update_checks_enabled = false;
        let menu = build_menu(&i);
        for e in &menu.entries {
            if let MenuEntry::Item { id, enabled, .. } = e {
                if id == "unload_model" || id == "check_updates" {
                    assert!(!enabled, "{id} should be disabled");
                }
            }
        }
    }

    #[test]
    fn localized_labels_follow_the_locale() {
        let mut i = inputs(false);
        i.locale = "de".to_string();
        let de = build_menu(&i);
        let en = build_menu(&inputs(false));
        assert_ne!(de.entries, en.entries);
    }

    #[test]
    fn meeting_items_hidden_when_unavailable() {
        let ids = ids(&build_menu(&inputs(false)).entries);
        assert!(!ids.iter().any(|id| id.starts_with("meeting_")));
    }

    #[test]
    fn meeting_start_item_when_idle() {
        let mut i = inputs(false);
        i.meeting.available = true;
        let menu = build_menu(&i);
        assert_eq!(menu.entries[2].id(), Some("meeting_start"));
        assert_eq!(menu.entries[3], MenuEntry::Separator);
        assert!(matches!(
            &menu.entries[2],
            MenuEntry::Item { label, enabled: true, .. } if label == "Start Meeting Notes"
        ));
        // The rest of the idle menu is unchanged.
        assert!(ids(&menu.entries).contains(&"model_submenu".to_string()));
    }

    #[test]
    fn meeting_recording_shows_title_and_stop() {
        for busy in [false, true] {
            let mut i = inputs(busy);
            i.meeting = MeetingMenu {
                available: true,
                recording: true,
                title: Some("Weekly sync".into()),
            };
            let menu = build_menu(&i);
            assert_eq!(
                menu.entries[2],
                MenuEntry::Item {
                    id: "meeting_title".into(),
                    label: "● Weekly sync".into(),
                    enabled: false,
                    accelerator: None,
                }
            );
            assert_eq!(menu.entries[3].id(), Some("meeting_stop"));
            assert_eq!(menu.entries[4], MenuEntry::Separator);
            assert!(!ids(&menu.entries).contains(&"meeting_start".to_string()));
        }
    }

    #[test]
    fn untranslated_meeting_labels_fall_back_to_english() {
        let mut i = inputs(false);
        i.locale = "de".to_string();
        i.meeting.available = true;
        let menu = build_menu(&i);
        assert!(matches!(&menu.entries[2], MenuEntry::Item { label, .. } if !label.is_empty()));
    }

    #[test]
    fn meeting_recording_changes_the_menu_and_icon() {
        let mut rec = inputs(false);
        rec.meeting.recording = true;
        assert_ne!(rec, inputs(false));
        assert_eq!(
            effective_icon_state(TrayIconState::Idle, true),
            TrayIconState::Recording
        );
        assert_eq!(
            effective_icon_state(TrayIconState::Transcribing, true),
            TrayIconState::Transcribing
        );
        assert_eq!(
            effective_icon_state(TrayIconState::Idle, false),
            TrayIconState::Idle
        );
    }
}
