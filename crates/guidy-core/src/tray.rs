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
use crate::meetings::types::RecorderState;
use crate::settings;
use crate::tray_i18n::{get_tray_translations, TrayStrings};
use log::{error, info, trace, warn};
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
    /// The meeting status item of the menu last built, retitled in place
    /// every second by the meeting clock (rebuilding the menu would close it).
    static MEETING_ITEM: RefCell<Option<tray_icon::menu::MenuItem>> = const { RefCell::new(None) };
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
    /// `(history id, menu label)` of the newest completed transcripts,
    /// newest first.
    recent_transcripts: Vec<(i64, String)>,
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
    /// The recording's clock inputs (see [`RecorderState::elapsed_ms`]);
    /// stable while it runs, so the clock never rebuilds the menu.
    started_at: i64,
    paused_at: Option<i64>,
    paused_ms: i64,
}

impl MeetingMenu {
    fn recorder_state(&self) -> RecorderState {
        RecorderState::Recording {
            meeting_id: 0,
            title: String::new(),
            started_at: self.started_at,
            scheduled_end_at: None,
            paused_at: self.paused_at,
            paused_ms: self.paused_ms,
        }
    }

    /// "● Weekly sync · 12:34" (or "Paused · 12:34").
    fn status_label(&self, strings: &TrayStrings, now: i64) -> String {
        let elapsed = crate::meetings::types::format_elapsed(
            self.recorder_state().elapsed_ms(now).unwrap_or(0),
        );
        if self.paused_at.is_some() {
            let en = get_tray_translations(Some("en".to_string()));
            return format!(
                "❚❚ {} · {elapsed}",
                or_en(&strings.meeting_paused, &en.meeting_paused)
            );
        }
        match self.title.as_deref().filter(|t| !t.is_empty()) {
            Some(title) => format!("● {title} · {elapsed}"),
            None => format!("● {elapsed}"),
        }
    }
}

/// A translation, or the English one where the locale lacks the key
/// (build.rs emits "" for missing keys).
fn or_en(s: &str, en: &str) -> String {
    if s.is_empty() {
        en.to_string()
    } else {
        s.to_string()
    }
}

fn meeting_menu(app: &AppHandle, settings: &settings::AppSettings) -> MeetingMenu {
    let manager = app.try_state::<Arc<crate::meetings::manager::MeetingsManager>>();
    let state = manager
        .as_ref()
        .map(|m| m.recorder_state())
        .unwrap_or_default();
    match state {
        RecorderState::Recording {
            title,
            started_at,
            paused_at,
            paused_ms,
            ..
        } => MeetingMenu {
            available: true,
            recording: true,
            title: Some(title),
            started_at,
            paused_at,
            paused_ms,
        },
        RecorderState::Idle => MeetingMenu {
            available: manager.is_some()
                && settings.meetings.enabled
                && crate::meetings::is_supported(),
            ..Default::default()
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

/// Number of frames in the dictation recording animation (frame 0 is the
/// static icon, animation frames are `1..=ANIMATION_FRAMES`).
const ANIMATION_FRAMES: u8 = 12;
/// Time between animation frames.
const ANIMATION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// A displayable icon: a resource path plus an animation frame (0 = the
/// image as shipped, other frames are derived from it).
type IconKey = (&'static str, u8);

/// Complete description of what the tray should look like.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayDesired {
    icon_path: &'static str,
    /// Animation frame of `icon_path`; always 0 unless `animated`.
    frame: u8,
    /// Dictation is recording, so the animation ticker may advance `frame`.
    animated: bool,
    menu: MenuInputs,
}

impl TrayDesired {
    fn icon_key(&self) -> IconKey {
        (self.icon_path, self.frame)
    }
}

struct TrayInner {
    /// Intent set by [`set_tray_state`].
    icon_state: TrayIconState,
    /// Current recording animation frame, advanced by the animation ticker.
    anim_frame: u8,
    /// The animation ticker thread is running.
    animating: bool,
    /// The meeting clock thread is running.
    meeting_clock: bool,
    /// Latest computed snapshot, waiting to be (or just) applied.
    desired: Option<TrayDesired>,
    /// Icon the native tray currently shows. Only updated when `set_icon`
    /// succeeds, so a failed update is retried on the next sync.
    applied_icon: Option<IconKey>,
    /// Inputs the native menu was last successfully built from. The tooltip
    /// is derived from the same inputs and set best-effort alongside the menu;
    /// it is not tracked separately.
    applied_menu: Option<MenuInputs>,
    /// An apply is scheduled on the main thread.
    pending: bool,
    /// Decoded icons (and derived animation frames) so the main thread never
    /// touches disk or renders frames.
    icons: HashMap<IconKey, Image<'static>>,
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
            anim_frame: 0,
            animating: false,
            meeting_clock: false,
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
/// While dictation is recording the icon is animated.
pub fn set_tray_state(app: &AppHandle, state: TrayIconState) {
    let mut start_animation = false;
    sync_tray_with(app, |inner| {
        inner.icon_state = state;
        if state == TrayIconState::Recording {
            start_animation = !std::mem::replace(&mut inner.animating, true);
        } else {
            // The next recording starts from the first frame.
            inner.anim_frame = 0;
        }
    });
    if start_animation {
        spawn_animation(app);
    }
}

/// Advances the recording animation until dictation leaves the Recording
/// state. Only touches the frame of the latest snapshot, so it never
/// recomputes (or clobbers) the menu inputs, and it goes through the same
/// coalescing applier as every other tray update.
fn spawn_animation(app: &AppHandle) {
    let ticker_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("tray-animation".into())
        .spawn(move || loop {
            let app = &ticker_app;
            std::thread::sleep(ANIMATION_INTERVAL);
            let Some(state) = app.try_state::<TrayState>() else {
                return;
            };
            let target = {
                let mut inner = state.lock();
                // No native tray (`--no-tray`): nothing to animate.
                if inner.icon_state != TrayIconState::Recording
                    || app.try_state::<TrayIcon>().is_none()
                {
                    inner.animating = false;
                    return;
                }
                inner.anim_frame = inner.anim_frame % ANIMATION_FRAMES + 1;
                inner
                    .desired
                    .as_ref()
                    .filter(|d| d.animated)
                    .map(|d| (d.icon_path, inner.anim_frame))
            };
            let Some(key) = target else {
                continue;
            };
            ensure_icon(app, &state, key);
            let schedule = {
                let mut inner = state.lock();
                match inner.desired.as_mut() {
                    // Re-check: a state change may have replaced the snapshot
                    // while the frame was being rendered.
                    Some(d) if d.animated && d.icon_path == key.0 => {
                        d.frame = key.1;
                        !std::mem::replace(&mut inner.pending, true)
                    }
                    _ => false,
                }
            };
            if schedule {
                post_apply(app);
            }
        });
    if let Err(err) = spawned {
        error!("Failed to start the tray animation: {err}");
        if let Some(state) = app.try_state::<TrayState>() {
            state.lock().animating = false;
        }
    }
}

/// Retitles the meeting status item every second while a meeting records,
/// so the menu shows how long it has been going (also while it is open).
fn spawn_meeting_clock(app: &AppHandle) {
    let clock_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("tray-meeting-clock".into())
        .spawn(move || loop {
            let app = &clock_app;
            let Some(state) = app.try_state::<TrayState>() else {
                return;
            };
            let label = {
                let mut inner = state.lock();
                match inner
                    .applied_menu
                    .as_ref()
                    .or(inner.desired.as_ref().map(|d| &d.menu))
                {
                    Some(menu) if menu.meeting.recording => {
                        let strings = get_tray_translations(Some(menu.locale.clone()));
                        menu.meeting
                            .status_label(&strings, crate::meetings::recorder::now_ms())
                    }
                    _ => {
                        inner.meeting_clock = false;
                        return;
                    }
                }
            };
            let _ = on_tray_thread(app, move || {
                MEETING_ITEM.with(|item| {
                    if let Some(item) = item.borrow().as_ref() {
                        item.set_text(&label);
                    }
                })
            });
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    if let Err(err) = spawned {
        error!("Failed to start the tray meeting clock: {err}");
        if let Some(state) = app.try_state::<TrayState>() {
            state.lock().meeting_clock = false;
        }
    }
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
    let (seq, icon_state, anim_frame) = {
        let mut inner = state.lock();
        update(&mut inner);
        inner.next_seq += 1;
        (inner.next_seq, inner.icon_state, inner.anim_frame)
    };

    // Tray not built yet (early secure-input monitor callbacks). The intent
    // is kept and picked up by the first sync after the tray exists.
    if app.try_state::<TrayIcon>().is_none() {
        return;
    }

    let desired = compute_desired(app, icon_state, anim_frame);
    let start_clock =
        desired.menu.meeting.recording && !std::mem::replace(&mut state.lock().meeting_clock, true);
    if start_clock {
        spawn_meeting_clock(app);
    }

    ensure_icon(app, &state, desired.icon_key());

    let schedule = {
        let mut inner = state.lock();
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

/// Decodes (and for animation frames, renders) the icon off the main thread,
/// once per key, outside the lock.
fn ensure_icon(app: &AppHandle, state: &TrayState, key: IconKey) {
    let (path, frame) = key;
    let (has_frame, base) = {
        let inner = state.lock();
        (
            inner.icons.contains_key(&key),
            inner.icons.get(&(path, 0)).cloned(),
        )
    };
    if has_frame {
        return;
    }
    let base = match base {
        Some(base) => base,
        None => match load_tray_icon(
            app.path()
                .resolve(path, crate::host::path::BaseDirectory::Resource),
        ) {
            Ok(image) => {
                state.lock().icons.insert((path, 0), image.clone());
                image
            }
            Err(err) => {
                error!("Failed to load tray icon '{path}': {err}");
                return;
            }
        },
    };
    if frame != 0 {
        let image = waveform_frame(&base, frame);
        state.lock().icons.insert(key, image);
    }
}

/// Renders animation frame `frame` (1..=[`ANIMATION_FRAMES`]) of a waveform
/// icon: every narrow vertical bar gets a new height (top and bottom caps are
/// kept, the straight middle is stretched or shrunk) so the bars bounce like a
/// level meter. Wide shapes, like the recording dot, are left untouched.
fn waveform_frame(base: &Image<'static>, frame: u8) -> Image<'static> {
    let (w, h) = (base.width() as usize, base.height() as usize);
    let src = base.rgba();
    let alpha = |x: usize, y: usize| src[(y * w + x) * 4 + 3];

    // Columns that are solidly part of a shape. Antialiasing bleeds faint
    // alpha between bars, so a low threshold would merge them.
    let solid: Vec<bool> = (0..w).map(|x| (0..h).any(|y| alpha(x, y) >= 128)).collect();
    let mut bars = Vec::new();
    let mut x = 0;
    while x < w {
        if !solid[x] {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && solid[x] {
            x += 1;
        }
        // Bars are narrow; anything wider (the dot) stays static. Rows are
        // measured on the solid core columns only, since the edge columns
        // carry faint full-height bleed from the neighbouring bars.
        let core = start..x;
        let rows: Vec<usize> = (0..h)
            .filter(|&y| core.clone().any(|x| alpha(x, y) >= 16))
            .collect();
        if let (true, Some(&top), Some(&bottom)) = (core.len() <= w / 8, rows.first(), rows.last())
        {
            bars.push(Bar {
                // Redraw the antialiased edge columns along with the bar.
                x0: start.saturating_sub(1),
                x1: (x + 1).min(w),
                width: core.len(),
                top,
                bottom,
            });
        }
    }

    let mut out = src.to_vec();
    let tallest = bars.iter().map(|b| b.bottom - b.top + 1).max().unwrap_or(0);
    let phase = f32::from(frame - 1) / f32::from(ANIMATION_FRAMES) * std::f32::consts::TAU;

    for (i, bar) in bars.iter().enumerate() {
        let &Bar {
            x0,
            x1,
            width,
            top,
            bottom,
        } = bar;
        let height = bottom - top + 1;
        // Rounded caps are about half the bar's width tall.
        let cap = width.div_ceil(2).min(height / 2);
        let min_height = (2 * cap + 1).max(tallest * 2 / 5);
        let max_height = tallest.max(min_height);
        // Each bar runs its own phase and a second harmonic, so the bars
        // move independently instead of as one wave.
        let t = phase + i as f32 * 2.1;
        let level = (0.5 + 0.35 * t.sin() + 0.15 * (2.0 * t + i as f32).sin()).clamp(0.0, 1.0);
        let new_height = min_height + ((max_height - min_height) as f32 * level).round() as usize;
        let center = (top + bottom) / 2;
        let new_top = center.saturating_sub(new_height / 2);
        let middle = center;

        for x in x0..x1 {
            for y in 0..h {
                let src_y = if y < new_top || y >= new_top + new_height {
                    None
                } else {
                    let d = y - new_top;
                    Some(if d < cap {
                        top + d
                    } else if d >= new_height - cap {
                        bottom - (new_height - 1 - d)
                    } else {
                        middle
                    })
                };
                let o = (y * w + x) * 4;
                match src_y {
                    Some(sy) => {
                        let s = (sy * w + x) * 4;
                        out[o..o + 4].copy_from_slice(&src[s..s + 4]);
                    }
                    None => out[o..o + 4].copy_from_slice(&[0, 0, 0, 0]),
                }
            }
        }
    }
    Image::new_owned(out, w as u32, h as u32)
}

/// One waveform bar found by [`waveform_frame`].
struct Bar {
    /// Columns to redraw, including the antialiased edges.
    x0: usize,
    x1: usize,
    /// Width of the solid core.
    width: usize,
    /// Rows the bar covers.
    top: usize,
    bottom: usize,
}

fn compute_desired(app: &AppHandle, icon_state: TrayIconState, anim_frame: u8) -> TrayDesired {
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
    let recent_transcripts = recent_transcripts(app)
        .iter()
        .map(|entry| (entry.id, transcript_label(last_transcript_text(entry))))
        .collect();

    desired_from(
        icon_state,
        anim_frame,
        theme,
        MenuInputs {
            busy: false, // set from the dictation state by `desired_from`
            warning,
            model_loaded,
            selected_model: settings.selected_model,
            downloaded_models,
            locale: settings.app_language,
            recent_transcripts,
            meeting,
        },
    )
}

/// The pure part of [`compute_desired`]. `busy` (the dictation "Cancel" item
/// instead of the model submenu and "Unload model") follows the *dictation*
/// state only; a meeting recording changes the icon and the meeting items,
/// never the dictation items.
///
/// Only dictation recording animates; a meeting recording (which can run for
/// hours) keeps the static recording icon.
fn desired_from(
    dictation: TrayIconState,
    anim_frame: u8,
    theme: AppTheme,
    mut menu: MenuInputs,
) -> TrayDesired {
    menu.busy = dictation.is_busy();
    let icon_state = effective_icon_state(dictation, menu.meeting.recording);
    let animated = dictation == TrayIconState::Recording;
    TrayDesired {
        icon_path: get_icon_path(theme, icon_state, menu.warning),
        frame: if animated { anim_frame } else { 0 },
        animated,
        menu,
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
        let icon_changed = inner.applied_icon != Some(desired.icon_key());
        let menu_changed = inner.applied_menu.as_ref() != Some(&desired.menu);
        if !icon_changed && !menu_changed {
            trace!("tray apply: nothing changed");
            return;
        }
        let icon = inner.icons.get(&desired.icon_key()).cloned();
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
            inner.applied_icon = Some(desired.icon_key());
        }
        if menu_ok {
            inner.applied_menu = Some(desired.menu.clone());
        }
    }

    // Animation frames apply ten times a second; keep them out of debug logs.
    let level = if menu_changed || desired.frame == 0 {
        log::Level::Debug
    } else {
        log::Level::Trace
    };
    log::log!(
        level,
        "tray apply: icon={} frame={} menu={} busy={} took={:?}",
        if icon_changed {
            desired.icon_path
        } else {
            "unchanged"
        },
        desired.frame,
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
/// depends on is in `inputs`.
fn build_menu(inputs: &MenuInputs) -> MenuModel {
    build_menu_at(inputs, crate::meetings::recorder::now_ms())
}

/// [`build_menu`] with the meeting clock read at `now` (unix ms).
fn build_menu_at(inputs: &MenuInputs, now: i64) -> MenuModel {
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

    let settings_i = MenuEntry::item("settings", &strings.settings, true, settings_accelerator);
    let en = get_tray_translations(Some("en".to_string()));
    let recent_items = if inputs.recent_transcripts.is_empty() {
        vec![MenuEntry::item(
            "no_transcripts",
            &or_en(&strings.no_transcripts, &en.no_transcripts),
            false,
            None,
        )]
    } else {
        inputs
            .recent_transcripts
            .iter()
            .map(|(id, label)| MenuEntry::item(&format!("copy_transcript:{id}"), label, true, None))
            .collect()
    };
    let recent_transcripts_i = MenuEntry::Submenu {
        id: "recent_transcripts".to_string(),
        label: or_en(&strings.recent_transcripts, &en.recent_transcripts),
        items: recent_items,
    };
    let quit_i = MenuEntry::item("quit", &strings.quit, true, quit_accelerator);

    let mut entries = Vec::new();
    if inputs.busy {
        entries.push(MenuEntry::item("cancel", &strings.cancel, true, None));
        entries.push(MenuEntry::Separator);
        entries.push(recent_transcripts_i);
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
        entries.push(recent_transcripts_i);
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
    entries.push(quit_i);

    // Meeting notes precede transcripts and model controls.
    if inputs.meeting.available {
        let mut items = Vec::new();
        if inputs.meeting.recording {
            items.push(MenuEntry::item(
                "meeting_title",
                &inputs.meeting.status_label(&strings, now),
                false,
                None,
            ));
            let (id, label, label_en) = if inputs.meeting.paused_at.is_some() {
                (
                    "meeting_resume",
                    &strings.resume_meeting_notes,
                    &en.resume_meeting_notes,
                )
            } else {
                (
                    "meeting_pause",
                    &strings.pause_meeting_notes,
                    &en.pause_meeting_notes,
                )
            };
            items.push(MenuEntry::item(id, &or_en(label, label_en), true, None));
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
        entries.splice(0..0, items);
    }

    entries.insert(0, MenuEntry::Separator);
    entries.insert(0, settings_i);

    // The warning goes on top so it's the first actionable thing seen.
    let mut tooltip = version_label();
    if let Some(label) = secure_input_warning {
        tooltip = format!("{} — {}", tooltip, label);
        entries.insert(
            0,
            MenuEntry::item("secure_input_warning", &label, true, None),
        );
        entries.insert(1, MenuEntry::Separator);
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
                let item = MenuItem::with_id(id.clone(), label, *enabled, accel);
                if id == "meeting_title" {
                    MEETING_ITEM.with(|m| *m.borrow_mut() = Some(item.clone()));
                }
                Box::new(item)
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

    MEETING_ITEM.with(|m| *m.borrow_mut() = None);
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

/// How many recent transcripts the tray offers to copy.
const RECENT_TRANSCRIPTS: usize = 3;
/// Longest transcript preview shown in the tray, in characters.
const TRANSCRIPT_LABEL_CHARS: usize = 40;

fn recent_transcripts(app: &AppHandle) -> Vec<HistoryEntry> {
    let Some(history) = app.try_state::<Arc<HistoryManager>>() else {
        return Vec::new();
    };
    match history.get_recent_completed_entries(RECENT_TRANSCRIPTS) {
        Ok(entries) => entries
            .into_iter()
            .filter(|entry| !last_transcript_text(entry).trim().is_empty())
            .collect(),
        Err(err) => {
            error!("Failed to fetch recent transcripts for the tray: {}", err);
            Vec::new()
        }
    }
}

/// A one-line preview of a transcript for its tray item.
fn transcript_label(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= TRANSCRIPT_LABEL_CHARS {
        return line;
    }
    let cut: String = line.chars().take(TRANSCRIPT_LABEL_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
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

/// Copies the recent transcript with history id `id` to the clipboard.
pub fn copy_transcript(app: &AppHandle, id: i64) {
    let Some(entry) = recent_transcripts(app).into_iter().find(|e| e.id == id) else {
        warn!("Transcript {} is no longer available for tray copy.", id);
        return;
    };

    let text = last_transcript_text(&entry);
    if let Err(err) = app.clipboard().write_text(text.to_string()) {
        error!("Failed to copy transcript to clipboard: {}", err);
        return;
    }

    info!("Copied transcript {} to clipboard via tray.", id);
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
        "meeting_pause" | "meeting_resume" => {
            if let Some(manager) = app.try_state::<Arc<crate::meetings::manager::MeetingsManager>>()
            {
                manager.set_recording_paused(id == "meeting_pause");
            }
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
        id if id.starts_with("copy_transcript:") => {
            if let Ok(entry_id) = id["copy_transcript:".len()..].parse::<i64>() {
                copy_transcript(app, entry_id);
            }
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
        build_menu, build_menu_at, desired_from, effective_icon_state, get_icon_path,
        last_transcript_text, load_tray_icon, transcript_label, waveform_frame, AppTheme,
        MeetingMenu, MenuEntry, MenuInputs, TrayDesired, TrayIconState, ANIMATION_FRAMES,
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
            recent_transcripts: vec![(7, "Hello world".to_string())],
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
            frame: 0,
            animated: true,
            menu: inputs(TrayIconState::Recording.is_busy()),
        };
        let transcribing = TrayDesired {
            icon_path: "resources/tray_transcribing.png",
            frame: 0,
            animated: false,
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
        assert_eq!(ids[0], "settings");
        assert!(!ids.contains(&"check_updates".to_string()));
        assert!(!ids.contains(&"version".to_string()));
        assert!(ids.contains(&"model_submenu".to_string()));
        assert!(ids.contains(&"unload_model".to_string()));
        assert!(!ids.contains(&"cancel".to_string()));
        assert_eq!(ids.last().unwrap(), "quit");
        let sub = menu
            .entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Submenu {
                    id, label, items, ..
                } if id == "model_submenu" => Some((label.clone(), items.clone())),
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
        assert_eq!(ids[0], "settings");
        assert_eq!(ids[2], "cancel");
        assert!(!ids.contains(&"model_submenu".to_string()));
    }

    #[test]
    fn secure_input_warning_is_slotted_below_version() {
        let mut i = inputs(false);
        i.warning = true;
        let menu = build_menu(&i);
        assert_eq!(menu.entries[0].id(), Some("secure_input_warning"));
        assert_eq!(menu.entries[1], MenuEntry::Separator);
        assert!(menu.tooltip.contains(" — "));
    }

    #[test]
    fn unload_disabled_when_no_model_loaded() {
        let mut i = inputs(false);
        i.model_loaded = false;
        let menu = build_menu(&i);
        assert!(menu.entries.iter().any(|e| matches!(
            e,
            MenuEntry::Item { id, enabled: false, .. } if id == "unload_model"
        )));
    }

    fn recent_submenu(menu: &super::MenuModel) -> Vec<MenuEntry> {
        menu.entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Submenu { id, items, .. } if id == "recent_transcripts" => {
                    Some(items.clone())
                }
                _ => None,
            })
            .expect("recent transcripts submenu")
    }

    #[test]
    fn recent_transcripts_submenu_lists_copyable_items() {
        for busy in [false, true] {
            let mut i = inputs(busy);
            i.recent_transcripts = vec![(9, "newest".into()), (4, "older".into())];
            let items = recent_submenu(&build_menu(&i));
            assert_eq!(
                items,
                vec![
                    MenuEntry::Item {
                        id: "copy_transcript:9".into(),
                        label: "newest".into(),
                        enabled: true,
                        accelerator: None,
                    },
                    MenuEntry::Item {
                        id: "copy_transcript:4".into(),
                        label: "older".into(),
                        enabled: true,
                        accelerator: None,
                    },
                ]
            );
        }
    }

    #[test]
    fn recent_transcripts_submenu_says_when_empty() {
        let mut i = inputs(false);
        i.recent_transcripts.clear();
        let items = recent_submenu(&build_menu(&i));
        assert!(matches!(
            items.as_slice(),
            [MenuEntry::Item { id, enabled: false, .. }] if id == "no_transcripts"
        ));
    }

    #[test]
    fn transcript_labels_are_one_short_line() {
        assert_eq!(transcript_label("  hello\n  world "), "hello world");
        let long = "word ".repeat(20);
        let label = transcript_label(&long);
        assert_eq!(label.chars().count(), 40);
        assert!(label.ends_with('…'));
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
        assert_eq!(
            ids(&menu.entries),
            [
                "settings",
                "---",
                "meeting_start",
                "---",
                "recent_transcripts",
                "---",
                "model_submenu",
                "unload_model",
                "---",
                "quit",
            ]
        );
    }

    #[test]
    fn meeting_recording_shows_title_elapsed_pause_and_stop() {
        for busy in [false, true] {
            let mut i = inputs(busy);
            i.meeting = MeetingMenu {
                available: true,
                recording: true,
                title: Some("Weekly sync".into()),
                started_at: 1_000,
                ..Default::default()
            };
            let menu = build_menu_at(&i, 1_000 + 754_000);
            assert_eq!(
                menu.entries[2],
                MenuEntry::Item {
                    id: "meeting_title".into(),
                    label: "● Weekly sync · 12:34".into(),
                    enabled: false,
                    accelerator: None,
                }
            );
            assert_eq!(menu.entries[3].id(), Some("meeting_pause"));
            assert_eq!(menu.entries[4].id(), Some("meeting_stop"));
            assert_eq!(menu.entries[5], MenuEntry::Separator);
            assert!(!ids(&menu.entries).contains(&"meeting_start".to_string()));
        }
    }

    #[test]
    fn paused_meeting_offers_resume_and_freezes_the_clock() {
        let mut i = inputs(false);
        i.meeting = MeetingMenu {
            available: true,
            recording: true,
            title: Some("Weekly sync".into()),
            started_at: 0,
            paused_at: Some(65_000),
            paused_ms: 0,
        };
        let menu = build_menu_at(&i, 500_000);
        assert!(matches!(
            &menu.entries[2],
            MenuEntry::Item { label, .. } if label == "❚❚ Paused · 1:05"
        ));
        assert_eq!(menu.entries[3].id(), Some("meeting_resume"));
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

    #[test]
    fn meeting_recording_keeps_the_idle_dictation_menu() {
        // Regression: the meeting's Recording icon state used to make the
        // menu "busy" (dictation Cancel, no model submenu / Unload model).
        let mut i = inputs(true);
        i.meeting = MeetingMenu {
            available: true,
            recording: true,
            title: Some("Weekly sync".into()),
            ..Default::default()
        };
        let desired = desired_from(TrayIconState::Idle, 3, AppTheme::Dark, i);
        assert!(!desired.menu.busy);
        // Meetings can run for hours; only dictation animates.
        assert!(!desired.animated);
        assert_eq!(desired.frame, 0);
        assert_eq!(
            desired.icon_path,
            get_icon_path(AppTheme::Dark, TrayIconState::Recording, false)
        );
        let ids = ids(&build_menu(&desired.menu).entries);
        assert!(ids.contains(&"meeting_stop".to_string()));
        assert!(ids.contains(&"model_submenu".to_string()));
        assert!(ids.contains(&"unload_model".to_string()));
        assert!(!ids.contains(&"cancel".to_string()));

        // Dictation busy during a meeting: Cancel shows, dictation's icon wins.
        let mut i = inputs(false);
        i.meeting.recording = true;
        let desired = desired_from(TrayIconState::Transcribing, 3, AppTheme::Dark, i);
        assert!(desired.menu.busy);
        assert_eq!(
            desired.icon_path,
            get_icon_path(AppTheme::Dark, TrayIconState::Transcribing, false)
        );
    }

    #[test]
    fn dictation_recording_is_animated() {
        let desired = desired_from(TrayIconState::Recording, 3, AppTheme::Dark, inputs(false));
        assert!(desired.animated);
        assert_eq!(desired.frame, 3);
        assert_eq!(desired.icon_key(), (desired.icon_path, 3));
    }

    fn recording_icon() -> crate::host::image::Image<'static> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/resources/tray_recording.png");
        load_tray_icon(Ok(path)).expect("recording icon loads")
    }

    #[test]
    fn waveform_frames_move_the_bars_and_keep_the_dot() {
        let base = recording_icon();
        let (w, h) = (base.width() as usize, base.height() as usize);
        let frames: Vec<_> = (1..=ANIMATION_FRAMES)
            .map(|f| waveform_frame(&base, f))
            .collect();
        for frame in &frames {
            assert_eq!(
                (frame.width(), frame.height()),
                (base.width(), base.height())
            );
            // The recording dot (right of the bars) is untouched.
            for y in 0..h {
                for x in 44..w {
                    let o = (y * w + x) * 4;
                    assert_eq!(frame.rgba()[o..o + 4], base.rgba()[o..o + 4]);
                }
            }
        }
        assert_ne!(frames[0].rgba(), base.rgba());
        assert_ne!(
            frames[0].rgba(),
            frames[ANIMATION_FRAMES as usize / 2].rgba()
        );
    }
}
