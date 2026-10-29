//! Application state shared by every view:
//! settings, models, devices, secure input, live logs. Backend calls run on
//! the background executor; results land back on the entity.

use crate::backend::SharedBackend;
use crate::logic::flows::{
    after_permissions, append_log_lines, initial_step, LogLine, OnboardingStep, StartupFacts,
    LOG_FLUSH_MS,
};
use crate::logic::keyboard::OsType;
use crate::logic::meetings::{MeetingsEvent, MeetingsInit};
use crate::logic::models::{initial_selector_status, selector_status_after_event, SelectorStatus};
use crate::logic::models_store::{ModelsEffect, ModelsStore};
use crate::logic::settings_store::{toast_for_event, SettingsStore, ToastKind, ToastSpec};
use crate::meetings_backend::{NoMeetings, SharedMeetingsBackend};
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global};
use guidy_core::commands::audio::{AudioDevice, CustomSounds};
use guidy_core::host::Event as CoreEvent;
use guidy_core::managers::history::HistoryUpdatePayload;
use guidy_core::secure_input::SecureInputStatus;
use guidy_core::settings::AppSettings;
use guidy_core::shortcut::native_keys::FrontendKeyEvent;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// UI-level notifications other views react to.
#[derive(Clone, Debug)]
pub enum AppEvent {
    Toast(ToastSpec),
    History(HistoryUpdatePayload),
    NativeKeys(FrontendKeyEvent),
    CheckForUpdates,
    /// Auto-select candidate (ModelSelector), already delayed 500 ms.
    ModelDownloadCompleted(String),
    /// A speech model finished downloading and wasn't picked for
    /// dictation: the window offers "Use for Dictation" (model id, name).
    ModelReady {
        id: String,
        name: String,
    },
    LanguageChanged,
    /// A meeting-notes core event (recorder state, progress, changes).
    Meetings(MeetingsEvent),
}

/// Whether the core (model registry, history, recorder) has started. It
/// starts off the main thread while the window shows a skeleton.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CoreStatus {
    #[default]
    Ready,
    Starting,
    /// It couldn't start; the window explains why (Show Logs, Quit).
    Failed(String),
}

/// The startup step machine (onboarding state). Lives on
/// the app, not the window: it must run when Guidy starts hidden, and survive
/// the settings window being closed and reopened.
#[derive(Default)]
pub struct Startup {
    /// `None` while the startup check runs.
    pub step: Option<OnboardingStep>,
    pub returning_user: bool,
    pub migration_model_id: Option<String>,
    pub core: CoreStatus,
    /// The last session finished onboarding: while the startup check runs
    /// the window draws the app's shell, not the setup assistant.
    pub expect_app: bool,
    /// What the user picked during onboarding (kept across Back and window
    /// reopening).
    pub choices: crate::logic::onboarding::Choices,
    checked: bool,
    initialized_after_onboarding: bool,
}

pub struct AppState {
    pub backend: SharedBackend,
    /// Meeting notes ([`NoMeetings`] when unavailable).
    pub meetings: SharedMeetingsBackend,
    /// Cached `meetings.is_supported()`: shows the Meetings section.
    pub meetings_supported: bool,
    /// Meeting-notes startup (the manager opens its store off-thread).
    pub meetings_init: MeetingsInit,
    /// Meeting-model downloads started from the meetings UI: they must not
    /// switch the dictation model when they finish.
    pub meeting_model_downloads: std::collections::HashSet<String>,
    /// Speech models picked for dictation before they finished downloading
    /// (Models page "Use for Dictation", a setup): selected when done.
    pub select_on_download: std::collections::HashSet<String>,
    pub startup: Startup,
    pub settings: SettingsStore,
    pub models: ModelsStore,
    pub selector_status: SelectorStatus,
    /// The load error shown in the footer when `selector_status` is Error.
    pub selector_error: Option<String>,
    /// Optimistic footer selection while a switch is in flight.
    pub pending_model: Option<String>,
    pub audio_devices: Vec<AudioDevice>,
    pub output_devices: Vec<AudioDevice>,
    pub custom_sounds: CustomSounds,
    pub update_checks_locked: Option<bool>,
    /// Fetched post-processing model lists per provider.
    pub pp_model_options: HashMap<String, Vec<String>>,
    pub secure_input: Option<SecureInputStatus>,
    pub log_lines: Vec<LogLine>,
    log_pending: Vec<LogLine>,
    log_flush_scheduled: bool,
    next_log_id: u64,
    pub is_portable: bool,
    pub settings_loaded: bool,
}

impl EventEmitter<AppEvent> for AppState {}

pub struct GlobalAppState(pub Entity<AppState>);
impl Global for GlobalAppState {}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Ensures the list starts with the "Default" entry. The core commands
/// already prepend it, so only add it when missing (e.g. on load failure).
fn device_with_default(mut devices: Vec<AudioDevice>) -> Vec<AudioDevice> {
    if devices.iter().any(|d| d.index == "default") {
        return devices;
    }
    devices.insert(
        0,
        AudioDevice {
            index: "default".into(),
            name: "Default".into(),
            is_default: true,
        },
    );
    devices
}

impl AppState {
    pub fn new(backend: SharedBackend) -> Self {
        Self {
            backend,
            meetings: std::sync::Arc::new(NoMeetings),
            meetings_supported: false,
            meetings_init: MeetingsInit::Ready,
            meeting_model_downloads: Default::default(),
            select_on_download: Default::default(),
            startup: Startup::default(),
            settings: SettingsStore::default(),
            models: ModelsStore::default(),
            selector_status: SelectorStatus::Unknown,
            selector_error: None,
            pending_model: None,
            audio_devices: Vec::new(),
            output_devices: Vec::new(),
            custom_sounds: CustomSounds {
                start: false,
                stop: false,
            },
            update_checks_locked: None,
            pp_model_options: HashMap::new(),
            secure_input: None,
            log_lines: Vec::new(),
            log_pending: Vec::new(),
            log_flush_scheduled: false,
            next_log_id: 0,
            is_portable: false,
            settings_loaded: false,
        }
    }

    pub fn global(cx: &App) -> Entity<AppState> {
        cx.global::<GlobalAppState>().0.clone()
    }

    pub fn install(backend: SharedBackend, cx: &mut App) -> Entity<AppState> {
        Self::install_with_meetings(backend, std::sync::Arc::new(NoMeetings), cx)
    }

    /// Install the state while the core is still starting (off the main
    /// thread): nothing calls the backend until [`Self::core_ready`].
    /// `expect_app` (the last session finished onboarding) picks the
    /// skeleton the window draws meanwhile.
    pub fn install_starting(
        backend: SharedBackend,
        meetings: SharedMeetingsBackend,
        expect_app: bool,
        cx: &mut App,
    ) -> Entity<AppState> {
        let entity = cx.new(|_| {
            let mut s = AppState::new(backend);
            s.meetings_supported = meetings.is_supported();
            s.meetings = meetings;
            s.startup.core = CoreStatus::Starting;
            s.startup.expect_app = expect_app;
            s
        });
        cx.set_global(GlobalAppState(entity.clone()));
        entity
    }

    /// The core started: load everything and run the startup check.
    pub fn core_ready(&mut self, cx: &mut Context<Self>) {
        self.startup.core = CoreStatus::Ready;
        self.meetings_init = if self.meetings_supported {
            self.meetings.init_status()
        } else {
            MeetingsInit::Ready
        };
        self.initialize(cx);
        self.check_onboarding(cx);
        cx.notify();
    }

    /// The core couldn't start: the window shows `error`.
    pub fn core_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.startup.core = CoreStatus::Failed(error);
        cx.notify();
    }

    pub fn install_with_meetings(
        backend: SharedBackend,
        meetings: SharedMeetingsBackend,
        cx: &mut App,
    ) -> Entity<AppState> {
        let entity = cx.new(|cx| {
            let mut s = AppState::new(backend);
            s.meetings_supported = meetings.is_supported();
            s.meetings_init = if s.meetings_supported {
                meetings.init_status()
            } else {
                MeetingsInit::Ready
            };
            s.meetings = meetings;
            s.initialize(cx);
            s.check_onboarding(cx);
            s
        });
        cx.set_global(GlobalAppState(entity.clone()));
        entity
    }

    pub fn app_settings(&self) -> Option<&AppSettings> {
        self.settings.settings.as_ref()
    }

    pub fn toast(&mut self, spec: ToastSpec, cx: &mut Context<Self>) {
        cx.emit(AppEvent::Toast(spec));
    }

    pub fn toast_error(&mut self, title: impl Into<String>, cx: &mut Context<Self>) {
        self.toast(ToastSpec::error(title, None), cx);
    }

    pub fn toast_kind(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.toast(
            ToastSpec {
                kind,
                title: title.into(),
                description: None,
            },
            cx,
        );
    }

    /// Run `f` against the backend off the main thread, then `done` here.
    pub fn call<T, F, D>(&self, cx: &mut Context<Self>, f: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(&dyn crate::backend::Backend) -> T + Send + 'static,
        D: FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { f(&*backend) }).await;
            this.update(cx, |s, cx| {
                done(s, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn initialize(&mut self, cx: &mut Context<Self>) {
        // Settings store init (settingsStore.initialize): defaults, settings,
        // custom sounds, updater lock. Devices load after onboarding.
        self.call(
            cx,
            |b| {
                (
                    b.get_default_settings(),
                    b.get_app_settings(),
                    b.check_custom_sounds(),
                    b.is_update_checks_locked(),
                    b.is_portable(),
                )
            },
            |s, (defaults, settings, sounds, locked, portable), cx| {
                s.settings.defaults = defaults.ok();
                match settings {
                    Ok(settings) => s.apply_loaded_settings(settings, cx),
                    Err(e) => log::error!("Failed to load settings: {e}"),
                }
                s.custom_sounds = sounds;
                s.update_checks_locked = Some(locked);
                s.is_portable = portable;
                s.settings_loaded = true;
            },
        );
        self.load_models(cx);
        self.load_library(cx);
        self.load_disk_usage(cx);
        self.load_current_model(cx);
        self.refresh_selector_status(cx);
        self.call(
            cx,
            |b| b.get_secure_input_status(),
            |s, status, _| {
                s.secure_input = Some(status);
            },
        );
    }

    fn apply_loaded_settings(&mut self, settings: AppSettings, cx: &mut Context<Self>) {
        let language_changed = self
            .settings
            .settings
            .as_ref()
            .is_none_or(|old| old.app_language != settings.app_language);
        let theme = settings.theme;
        self.settings.settings = Some(settings.clone());
        if language_changed {
            apply_app_language(&settings.app_language);
            cx.emit(AppEvent::LanguageChanged);
        }
        crate::theme::apply_setting(theme, cx);
    }

    // -- startup ------------------------------------------------------------

    pub fn onboarding_done(&self) -> bool {
        self.startup.step == Some(OnboardingStep::Done)
    }

    /// Decide the first step (returning vs new user, permissions, retired
    /// model). Runs once per session, once the core has started.
    pub fn check_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.startup.checked || self.startup.core != CoreStatus::Ready {
            return;
        }
        self.startup.checked = true;
        let os = OsType::current();
        self.call(
            cx,
            move |backend| {
                let settings = backend.get_app_settings().ok();
                let mut facts = StartupFacts {
                    onboarding_completed: settings.as_ref().map(|s| s.onboarding_completed),
                    ..Default::default()
                };
                if facts.onboarding_completed == Some(true) {
                    let selected = settings.map(|s| s.selected_model).unwrap_or_default();
                    facts.migration_model_id = backend
                        .get_retired_models()
                        .into_iter()
                        .any(|m| m.id == selected)
                        .then_some(selected);
                    let dev_skip = cfg!(debug_assertions)
                        && std::env::var_os("GUIDY_DEV_SKIP_PERMISSIONS").is_some();
                    match os {
                        _ if dev_skip => facts.mac_permissions_ok = Some(true),
                        OsType::MacOs => {
                            facts.mac_permissions_ok = Some(
                                backend.check_accessibility_permission()
                                    && backend.check_microphone_permission(),
                            )
                        }
                        OsType::Windows => {
                            facts.windows_mic =
                                Some(backend.get_windows_microphone_permission_status())
                        }
                        _ => {}
                    }
                }
                facts
            },
            move |s, facts, cx| {
                let outcome = initial_step(&facts, os);
                s.startup.returning_user = outcome.returning_user;
                s.startup.migration_model_id = facts.migration_model_id.clone();
                if outcome.reveal_window {
                    s.backend.show_main_window();
                }
                s.set_step(outcome.step, cx);
            },
        );
    }

    pub fn set_step(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        self.startup.step = Some(step);
        if step == OnboardingStep::Done && !self.startup.initialized_after_onboarding {
            self.startup.initialized_after_onboarding = true;
            // Enigo + shortcuts only after onboarding (avoids early macOS
            // permission prompts), then load devices. The selected model
            // loads now, so the first dictation doesn't wait for it.
            self.call(
                cx,
                |b| {
                    let input = (b.initialize_enigo(), b.initialize_shortcuts());
                    b.warm_up_model();
                    input
                },
                |s, (enigo, shortcuts), cx| {
                    if let Err(e) = enigo {
                        log::warn!("Failed to initialize input: {e}");
                    }
                    if let Err(e) = shortcuts {
                        log::warn!("Failed to initialize shortcuts: {e}");
                    }
                    s.refresh_audio_devices(cx);
                    s.refresh_output_devices(cx);
                },
            );
        }
        cx.notify();
    }

    pub fn on_permissions_complete(&mut self, cx: &mut Context<Self>) {
        let o = after_permissions(
            self.startup.returning_user,
            self.startup.migration_model_id.as_deref(),
        );
        self.set_step(o.step, cx);
    }

    /// Back from an onboarding step (no-op on the first one).
    pub fn onboarding_back(&mut self, cx: &mut Context<Self>) {
        if let Some(step) = self
            .startup
            .step
            .and_then(|step| crate::logic::flows::step_back(step, self.startup.returning_user))
        {
            self.set_step(step, cx);
        }
    }

    /// Change what the user is setting up (Welcome and setup steps).
    pub fn update_onboarding_choices(
        &mut self,
        f: impl FnOnce(&mut crate::logic::onboarding::Choices),
        cx: &mut Context<Self>,
    ) {
        f(&mut self.startup.choices);
        cx.notify();
    }

    /// What the onboarding choices need (`None` until settings load).
    pub fn onboarding_plan(&self) -> Option<crate::logic::onboarding::Plan> {
        use crate::logic::onboarding::{plan, plan_language};
        use crate::logic::requirements::Inputs;
        let settings = self.app_settings()?;
        let language = plan_language(settings, &guidy_i18n::language());
        let m = &self.models;
        let inputs = Inputs {
            stt_models: &m.models,
            other_models: &m.other,
            settings,
            providers_with_keys: &m.providers_with_keys,
            apple_intelligence: m.apple_intelligence,
            language: &language,
            ram_gb: m.ram_gb,
        };
        Some(plan(&inputs, &self.startup.choices))
    }

    /// Finish onboarding without waiting for anything: write the settings
    /// the choices need (meetings on, summaries provider), select the
    /// speech model now or when its download finishes, queue the downloads
    /// (they continue in the background, shown in the status bar), mark
    /// onboarding complete and show the app. `skip` finishes with nothing
    /// set up ("Set Up Later").
    pub fn finish_onboarding(&mut self, skip: bool, cx: &mut Context<Self>) {
        let choices = self.startup.choices.clone();
        if let Some(plan) = self.onboarding_plan().filter(|_| !skip) {
            self.update_settings_in_order(
                crate::logic::onboarding::setting_writes(&choices, &plan),
                cx,
            );
            if let Some(id) = plan.speech_model.clone() {
                let downloaded = self.models.model(&id).is_some_and(|m| m.is_downloaded);
                if choices.dictation && downloaded {
                    self.select_model(&id, cx, |_, _, _| {});
                } else if choices.dictation {
                    self.select_on_download.insert(id);
                } else if choices.meetings && !downloaded {
                    self.meeting_model_downloads.insert(id);
                }
            }
            let ids = plan.download_ids();
            if !ids.is_empty() {
                self.request_downloads(ids, cx);
            }
        }
        self.call(
            cx,
            |b| b.complete_onboarding(),
            |_, r, _| {
                if let Err(e) = r {
                    log::warn!("Failed to save onboarding as complete: {e}");
                }
            },
        );
        self.set_step(OnboardingStep::Done, cx);
    }

    /// Migration "Later": use the app now, ask again next launch.
    pub fn postpone_migration(&mut self, cx: &mut Context<Self>) {
        self.set_step(OnboardingStep::Done, cx);
    }

    /// Migration "Choose Another…": use `id` for dictation (now, or when
    /// its download finishes), remove the retired models, open the app.
    pub fn migrate_to_model(&mut self, id: &str, cx: &mut Context<Self>) {
        self.use_for_dictation(id, cx);
        self.on_model_selected(cx);
    }

    pub fn on_model_selected(&mut self, cx: &mut Context<Self>) {
        if self.startup.migration_model_id.is_some() {
            // Picked a different model instead of updating: retired models
            // left on disk can't run anymore.
            self.call(
                cx,
                |b| {
                    for m in b.get_retired_models() {
                        let _ = b.delete_retired_model(&m.id);
                    }
                },
                |_, _, _| {},
            );
        }
        self.set_step(OnboardingStep::Done, cx);
    }

    // -- settings -----------------------------------------------------------

    pub fn refresh_settings(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.get_app_settings(),
            |s, r, cx| match r {
                Ok(settings) => s.apply_loaded_settings(settings, cx),
                Err(e) => log::error!("Failed to refresh settings: {e}"),
            },
        );
    }

    /// Optimistic update + backend write + rollback on error.
    pub fn update_setting(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        self.update_setting_then(key, value, cx, |_, _, _| {});
    }

    /// `update_setting`, then `done(ok)` once the write has finished. Chain
    /// writes through this when one must land before the next (the backend
    /// setters read-modify-write the whole settings object).
    pub fn update_setting_then(
        &mut self,
        key: &str,
        value: Value,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, bool, &mut Context<Self>) + 'static,
    ) {
        let pending = self.settings.begin_update(key, value.clone());
        // Side effects the React store applied immediately.
        match key {
            "theme" => {
                if let Some(s) = self.app_settings() {
                    crate::theme::apply_setting(s.theme, cx);
                }
            }
            "app_language" => {
                if let Some(lang) = value.as_str() {
                    apply_app_language(lang);
                    cx.emit(AppEvent::LanguageChanged);
                }
            }
            _ => {}
        }
        cx.notify();
        let key_owned = key.to_string();
        self.call(
            cx,
            move |b| {
                if key_owned == "model_unload_timeout" {
                    b.set_model_unload_timeout(value)
                } else {
                    b.update_setting(&key_owned, value)
                }
            },
            move |s, result, cx| {
                let key = pending.key.clone();
                s.settings.finish_update(pending, &result);
                if let Err(e) = &result {
                    if key == "vad_backend" {
                        s.toast_error(e.clone(), cx);
                    }
                }
                done(s, result.is_ok(), cx);
            },
        );
    }

    /// Apply writes one after another, awaiting each, stopping at the
    /// first failure. Pairs like gpu device + accelerator, or auto-submit key
    /// + flag, would otherwise race and one write could undo the other.
    pub fn update_settings_in_order(
        &mut self,
        mut writes: Vec<(&'static str, Value)>,
        cx: &mut Context<Self>,
    ) {
        if writes.is_empty() {
            return;
        }
        let (key, value) = writes.remove(0);
        self.update_setting_then(key, value, cx, move |s, ok, cx| {
            if ok {
                s.update_settings_in_order(writes, cx);
            }
        });
    }

    pub fn reset_setting(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(default) = self.settings.default_of(key) {
            self.update_setting(key, default, cx);
        }
    }

    /// Change a shortcut binding; `done(ok, error)` reports the outcome.
    pub fn update_binding(
        &mut self,
        id: &str,
        binding: &str,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        let original = self.settings.begin_binding(id, binding);
        cx.notify();
        let (id_a, binding_a) = (id.to_string(), binding.to_string());
        let id_b = id.to_string();
        self.call(
            cx,
            move |b| b.change_binding(&id_a, &binding_a),
            move |s, r, cx| {
                let outcome = match r {
                    Ok(resp) if resp.success => Ok(()),
                    Ok(resp) => Err(resp
                        .error
                        .unwrap_or_else(|| "Failed to update binding".into())),
                    Err(e) => Err(e),
                };
                s.settings.finish_binding(&id_b, original, outcome.is_ok());
                done(s, outcome, cx);
            },
        );
    }

    pub fn reset_binding(&mut self, id: &str, cx: &mut Context<Self>) {
        let key = format!("binding_{id}");
        self.settings.updating.insert(key.clone());
        let id = id.to_string();
        self.call(
            cx,
            move |b| b.reset_binding(&id),
            move |s, r, cx| {
                if let Err(e) = r {
                    log::error!("Failed to reset binding: {e}");
                }
                s.settings.updating.remove(&key);
                s.refresh_settings(cx);
            },
        );
    }

    pub fn refresh_audio_devices(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.get_available_microphones(),
            |s, r, _| match r {
                Ok(d) => s.audio_devices = device_with_default(d),
                Err(e) => {
                    // Fall back to "Default" so the select stays usable.
                    log::error!("Failed to load microphones: {e}");
                    s.audio_devices = device_with_default(Vec::new());
                }
            },
        );
    }

    pub fn refresh_output_devices(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.get_available_output_devices(),
            |s, r, _| match r {
                Ok(d) => s.output_devices = device_with_default(d),
                Err(e) => {
                    // Fall back to "Default" so the select stays usable.
                    log::error!("Failed to load output devices: {e}");
                    s.output_devices = device_with_default(Vec::new());
                }
            },
        );
    }

    pub fn check_custom_sounds(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.check_custom_sounds(),
            |s, r, _| s.custom_sounds = r,
        );
    }

    // -- post-processing ------------------------------------------------------

    pub fn set_post_process_provider(&mut self, provider_id: &str, cx: &mut Context<Self>) {
        let key = "post_process_provider_id".to_string();
        self.settings.updating.insert(key.clone());
        let previous = self
            .app_settings()
            .map(|s| s.post_process_provider_id.clone());
        if let Some(s) = self.settings.settings.as_mut() {
            s.post_process_provider_id = provider_id.to_string();
        }
        self.pp_model_options
            .insert(provider_id.to_string(), Vec::new());
        cx.notify();
        let id = provider_id.to_string();
        self.call(
            cx,
            move |b| b.set_post_process_provider(&id),
            move |s, r, cx| {
                if let Err(e) = r {
                    log::error!("Failed to set post-process provider: {e}");
                    if let (Some(prev), Some(settings)) = (previous, s.settings.settings.as_mut()) {
                        settings.post_process_provider_id = prev;
                    }
                    s.settings.set_error(&key, e);
                } else {
                    s.settings.clear_error(&key);
                }
                s.settings.updating.remove(&key);
                s.refresh_settings(cx);
            },
        );
    }

    /// base_url / api_key / model / api_format writes (refresh afterwards).
    pub fn update_post_process(
        &mut self,
        what: &'static str,
        provider_id: &str,
        value: String,
        cx: &mut Context<Self>,
    ) {
        self.update_post_process_then(what, provider_id, value, cx, |_, _, _| {});
    }

    /// [`Self::update_post_process`], then `done(result)` once the backend
    /// call has finished.
    pub fn update_post_process_then(
        &mut self,
        what: &'static str,
        provider_id: &str,
        value: String,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, &Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        let key = format!("post_process_{what}:{provider_id}");
        self.settings.updating.insert(key.clone());
        if matches!(what, "base_url" | "api_key" | "api_format") {
            // A different endpoint/credential invalidates the fetched list.
            self.pp_model_options
                .insert(provider_id.to_string(), Vec::new());
        }
        cx.notify();
        let pid = provider_id.to_string();
        let pid_done = pid.clone();
        self.call(
            cx,
            move |b| match what {
                "base_url" => b.change_post_process_base_url(&pid, &value),
                "api_key" => b.change_post_process_api_key(&pid, &value),
                "model" => b.change_post_process_model(&pid, &value),
                "api_format" => match serde_json::from_value(Value::String(value)) {
                    Ok(f) => b.change_post_process_api_format(&pid, f),
                    Err(e) => Err(e.to_string()),
                },
                _ => Ok(()),
            },
            move |s, r, cx| {
                match &r {
                    // A new endpoint makes the stored model meaningless
                    // (settingsStore.updatePostProcessBaseUrl).
                    Ok(()) if what == "base_url" => {
                        s.settings.updating.remove(&key);
                        s.update_post_process("model", &pid_done, String::new(), cx);
                        done(s, &r, cx);
                        return;
                    }
                    Ok(()) => s.settings.clear_error(&key),
                    Err(e) => {
                        log::error!("Failed to update post-process {what}: {e}");
                        s.settings.set_error(&key, e.clone());
                    }
                }
                s.settings.updating.remove(&key);
                s.refresh_settings(cx);
                done(s, &r, cx);
            },
        );
    }

    pub fn fetch_post_process_models(&mut self, provider_id: &str, cx: &mut Context<Self>) {
        let key = format!("post_process_models_fetch:{provider_id}");
        self.settings.updating.insert(key.clone());
        let pid = provider_id.to_string();
        let pid2 = pid.clone();
        self.call(
            cx,
            move |b| b.fetch_post_process_models(&pid),
            move |s, r, _| {
                s.settings.updating.remove(&key);
                match r {
                    Ok(models) => {
                        s.settings.clear_error(&key);
                        s.pp_model_options.insert(pid2, models);
                    }
                    Err(e) => {
                        // Shown under the Model row (Post Process page).
                        log::error!("Failed to fetch models: {e}");
                        s.settings.set_error(&key, e);
                    }
                }
            },
        );
    }

    // -- models ---------------------------------------------------------------

    pub fn load_models(&mut self, cx: &mut Context<Self>) {
        self.models.loading = !self.models.initialized;
        self.call(
            cx,
            |b| b.get_available_models(),
            |s, r, _| {
                match r {
                    Ok(models) => s.models.set_models(models),
                    Err(e) => s.models.error = Some(format!("Failed to load models: {e}")),
                }
                s.models.loading = false;
                s.models.initialized = true;
            },
        );
    }

    pub fn load_current_model(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.get_current_model(),
            |s, r, cx| {
                if let Ok(id) = r {
                    s.set_current_model(id, cx);
                }
            },
        );
    }

    /// ModelSelector re-checks its status whenever the current model changes
    /// (e.g. deleting the active model must show "download required").
    fn set_current_model(&mut self, id: String, cx: &mut Context<Self>) {
        if self.models.current_model != id {
            self.models.current_model = id;
            self.refresh_selector_status(cx);
        }
    }

    fn refresh_selector_status(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| (b.get_transcription_model_status(), b.get_current_model()),
            |s, (loaded, current), _| {
                let current = current.unwrap_or_default();
                s.selector_status =
                    initial_selector_status(loaded.ok().flatten().as_deref(), &current);
            },
        );
    }

    pub fn rescan_models(&mut self, cx: &mut Context<Self>) {
        self.models.rescanning = true;
        cx.notify();
        self.call(
            cx,
            |b| b.rescan_local_models(),
            |s, r, _| {
                if let Err(e) = r {
                    s.models.error = Some(format!("Failed to rescan models: {e}"));
                }
                s.models.rescanning = false;
            },
        );
    }

    /// `done(success)`.
    pub fn select_model(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, bool, &mut Context<Self>) + 'static,
    ) {
        self.models.error = None;
        let id_a = id.to_string();
        let id_b = id.to_string();
        self.call(
            cx,
            move |b| b.set_active_model(&id_a),
            move |s, r, cx| {
                let ok = match r {
                    Ok(()) => {
                        s.set_current_model(id_b, cx);
                        true
                    }
                    Err(e) => {
                        s.models.error = Some(format!("Failed to switch to model: {e}"));
                        false
                    }
                };
                done(s, ok, cx);
            },
        );
    }

    pub fn download_model(&mut self, id: &str, cx: &mut Context<Self>) {
        self.download_model_then(id, cx, |_, _, _| {});
    }

    /// Start a download; `done` gets the command's final result (the backend
    /// command returns once the download has finished or failed).
    pub fn download_model_then(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<(), String>, &mut Context<Self>) + 'static,
    ) {
        self.models.begin_download(id);
        cx.notify();
        let id_a = id.to_string();
        let id_b = id.to_string();
        self.call(
            cx,
            move |b| b.download_model(&id_a),
            move |s, r, cx| {
                if r.is_err() {
                    // The failed event may not arrive (e.g. listener not ready).
                    s.models.download_command_failed(&id_b);
                }
                done(s, r, cx);
            },
        );
    }

    // -- downloads: every model kind ---------------------------------------

    /// Reload the LLM and speaker models, the download list and what the
    /// requirements plan needs about this Mac (memory, Apple Intelligence,
    /// saved API keys).
    pub fn load_library(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| {
                (
                    b.other_models(),
                    b.downloads(),
                    b.system_ram_gb(),
                    b.check_apple_intelligence_available(),
                    b.providers_with_api_keys(),
                )
            },
            |s, (other, downloads, ram, ai, keys), cx| {
                s.models.other = other;
                s.models.other_loaded = true;
                for fx in s.models.set_downloads(downloads) {
                    s.run_models_effect(fx, cx);
                }
                s.models.ram_gb = ram;
                s.models.apple_intelligence = ai;
                s.models.providers_with_keys = keys;
            },
        );
    }

    /// Re-measure the disk use of models (walks the model folders).
    pub fn load_disk_usage(&mut self, cx: &mut Context<Self>) {
        self.call(
            cx,
            |b| b.disk_usage(),
            |s, disk, _| s.models.disk = Some(disk),
        );
    }

    /// Queue a download of any kind by tracker id (`stt:…`, `llm:…`,
    /// `speaker:resnet34`, …). Errors (e.g. not enough disk space) land in
    /// `models.error` and a toast.
    pub fn request_download(&mut self, id: &str, cx: &mut Context<Self>) {
        self.request_downloads(vec![id.to_string()], cx);
    }

    /// Queue several downloads in order (a setup). Stops at the first that
    /// can't be queued.
    pub fn request_downloads(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.models.error = None;
        cx.notify();
        self.call(
            cx,
            move |b| {
                for id in &ids {
                    b.request_download(id)?;
                }
                Ok::<(), String>(())
            },
            |s, r, cx| {
                if let Err(e) = r {
                    s.models.error = Some(e.clone());
                    s.toast(
                        ToastSpec::error(guidy_i18n::t("models.downloadNotStarted"), Some(e)),
                        cx,
                    );
                }
            },
        );
    }

    pub fn cancel_tracked_download(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.call(cx, move |b| b.cancel_tracked_download(&id), |_, _, _| {});
    }

    /// Start a failed or cancelled download again, resuming its partial.
    pub fn retry_download(&mut self, id: &str, cx: &mut Context<Self>) {
        self.models.error = None;
        let id = id.to_string();
        self.call(
            cx,
            move |b| b.retry_download(&id),
            |s, r, cx| {
                if let Err(e) = r {
                    s.models.error = Some(e.clone());
                    s.toast(
                        ToastSpec::error(guidy_i18n::t("models.downloadNotStarted"), Some(e)),
                        cx,
                    );
                }
            },
        );
    }

    /// Delete a downloaded LLM or speaker model.
    pub fn delete_tracked_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.call(
            cx,
            move |b| b.delete_tracked_model(&id),
            |s, r, cx| {
                match r {
                    Ok(()) => {}
                    Err(e) => s.models.error = Some(format!("Failed to delete model: {e}")),
                }
                s.load_library(cx);
                s.load_disk_usage(cx);
            },
        );
    }

    /// Make `id` the dictation model: now when it is downloaded, else
    /// queue it and select it when it finishes.
    pub fn use_for_dictation(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.models.model(id).is_some_and(|m| m.is_downloaded) {
            self.select_model(id, cx, |_, _, _| {});
            return;
        }
        self.select_on_download.insert(id.to_string());
        self.request_download(&guidy_core::downloads::stt_id(id), cx);
    }

    /// Transcribe meetings with `id` (queued when it isn't downloaded yet;
    /// the dictation model stays as it is).
    pub fn use_for_meetings(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_setting("meetings", serde_json::json!({ "meeting_model": id }), cx);
        if !self.models.model(id).is_some_and(|m| m.is_downloaded) {
            self.meeting_model_downloads.insert(id.to_string());
            self.request_download(&guidy_core::downloads::stt_id(id), cx);
        }
    }

    /// Download the meeting transcription model (the Models page download
    /// path) without making it the dictation model afterwards.
    pub fn download_meeting_model(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.models.downloading.contains(id) {
            return;
        }
        self.meeting_model_downloads.insert(id.to_string());
        let id_done = id.to_string();
        self.download_model_then(id, cx, move |s, r, cx| {
            if let Err(e) = r {
                s.meeting_model_downloads.remove(&id_done);
                let spec = ToastSpec::error(guidy_i18n::t("meetings.model.downloadError"), Some(e));
                s.toast(spec, cx);
            }
        });
    }

    // -- meetings -------------------------------------------------------------

    /// Emit a UI-side meetings event (e.g. the Linear key changed).
    pub fn emit_meetings(&mut self, ev: MeetingsEvent, cx: &mut Context<Self>) {
        cx.emit(AppEvent::Meetings(ev));
    }

    /// Start the meetings manager again after a failed start.
    pub fn retry_meetings_init(&mut self, cx: &mut Context<Self>) {
        self.meetings_init = MeetingsInit::Starting;
        cx.notify();
        let meetings = self.meetings.clone();
        cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move {
                    meetings.retry_init();
                    meetings.init_status()
                })
                .await;
            this.update(cx, |s, cx| {
                // The ready / init-failed event may already have updated it.
                if s.meetings_init == MeetingsInit::Starting && status != MeetingsInit::Starting {
                    s.set_meetings_init(status, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Move unreadable meeting data aside, make a new data key and start the
    /// meetings manager again (after a missing key or a failed start).
    pub fn reset_meetings_data(&mut self, cx: &mut Context<Self>) {
        self.meetings_init = MeetingsInit::Starting;
        cx.notify();
        let meetings = self.meetings.clone();
        cx.spawn(async move |this, cx| {
            let (result, status) = cx
                .background_spawn(async move {
                    let result = meetings.reset_data();
                    (result, meetings.init_status())
                })
                .await;
            this.update(cx, |s, cx| {
                if s.meetings_init != MeetingsInit::Starting {
                    return; // the ready / failure event got here first
                }
                match result {
                    Err(e) => s.set_meetings_init(MeetingsInit::after_failed_reset(status, e), cx),
                    Ok(()) if status != MeetingsInit::Starting => s.set_meetings_init(status, cx),
                    Ok(()) => {}
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_meetings_init(&mut self, status: MeetingsInit, cx: &mut Context<Self>) {
        let ready = status == MeetingsInit::Ready && self.meetings_init != MeetingsInit::Ready;
        self.meetings_init = status;
        if ready {
            cx.emit(AppEvent::Meetings(MeetingsEvent::Ready));
        }
        cx.notify();
    }

    pub fn cancel_download(&mut self, id: &str, cx: &mut Context<Self>) {
        let id_a = id.to_string();
        let id_b = id.to_string();
        self.call(
            cx,
            move |b| b.cancel_download(&id_a),
            move |s, r, cx| match r {
                Ok(()) => {
                    for fx in s.models.cancelled_by_user(&id_b) {
                        s.run_models_effect(fx, cx);
                    }
                }
                Err(e) => s.models.error = Some(format!("Failed to cancel download: {e}")),
            },
        );
    }

    pub fn delete_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.call(
            cx,
            move |b| b.delete_model(&id),
            |s, r, cx| match r {
                Ok(()) => {
                    s.load_models(cx);
                    s.load_current_model(cx);
                }
                Err(e) => s.models.error = Some(format!("Failed to delete model: {e}")),
            },
        );
    }

    fn run_models_effect(&mut self, fx: ModelsEffect, cx: &mut Context<Self>) {
        match fx {
            ModelsEffect::ReloadModels => self.load_models(cx),
            ModelsEffect::ReloadCurrentModel => self.load_current_model(cx),
            ModelsEffect::ReloadOther => self.load_library(cx),
            ModelsEffect::ReloadDisk => self.load_disk_usage(cx),
            ModelsEffect::ToastError(e) => self.toast_error(e, cx),
            ModelsEffect::DownloadDone { name } => self.toast_kind(
                ToastKind::Success,
                guidy_i18n::t_args("meetings.downloads.done", &[("name", &name)]),
                cx,
            ),
            ModelsEffect::DownloadFailed { name, error } => self.toast(
                ToastSpec::error(
                    guidy_i18n::t_args("meetings.downloads.failed", &[("name", &name)]),
                    error,
                ),
                cx,
            ),
            // A meeting model: keep the dictation model as it is. Checked
            // first so the set is cleaned up during onboarding too.
            ModelsEffect::DownloadCompleted(id) if self.meeting_model_downloads.remove(&id) => {
                self.select_on_download.remove(&id);
            }
            // ModelSelector (mounted only once onboarding is done): during
            // onboarding and migration those screens pick the model.
            ModelsEffect::DownloadCompleted(_) if !self.onboarding_done() => {}
            // Not picked for dictation while there is a dictation model:
            // offer it instead of switching on its own.
            ModelsEffect::DownloadCompleted(id)
                if !self.select_on_download.remove(&id)
                    && !self.models.current_model.is_empty() =>
            {
                let name = self
                    .models
                    .model(&id)
                    .map(crate::ui::kit::model_name)
                    .unwrap_or_else(|| id.clone());
                cx.emit(AppEvent::ModelReady { id, name });
            }
            ModelsEffect::DownloadCompleted(id) => {
                // ModelSelector: auto-select the finished download after
                // 500 ms unless a recording is running.
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    this.update(cx, |s, cx| {
                        let id = id.clone();
                        s.call(
                            cx,
                            |b| b.is_recording(),
                            move |s, recording, cx| {
                                if !recording {
                                    cx.emit(AppEvent::ModelDownloadCompleted(id.clone()));
                                    s.pending_model = Some(id.clone());
                                    s.select_model(&id, cx, |s, _, _| s.pending_model = None);
                                }
                            },
                        );
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    // -- logs ---------------------------------------------------------------

    pub fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.log_lines.clear();
        cx.notify();
    }

    fn push_log(&mut self, level: u8, message: String, cx: &mut Context<Self>) {
        self.next_log_id += 1;
        self.log_pending.push(LogLine {
            id: self.next_log_id,
            level,
            message,
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
        if !self.log_flush_scheduled {
            self.log_flush_scheduled = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(LOG_FLUSH_MS))
                    .await;
                this.update(cx, |s, cx| {
                    let batch = std::mem::take(&mut s.log_pending);
                    append_log_lines(&mut s.log_lines, batch);
                    s.log_flush_scheduled = false;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    // -- backend events -----------------------------------------------------

    pub fn handle_core_event(&mut self, event: &CoreEvent, cx: &mut Context<Self>) {
        let payload = &event.payload;
        if let Some(spec) = toast_for_event(&event.name, payload) {
            self.toast(spec, cx);
        }
        for fx in self.models.apply_event(&event.name, payload, now_ms()) {
            self.run_models_effect(fx, cx);
        }
        match event.name.as_str() {
            "settings-changed" => {
                self.refresh_settings(cx);
                if payload["setting"] == "selected_microphone" {
                    self.refresh_audio_devices(cx);
                }
            }
            "model-state-changed" => {
                self.refresh_settings(cx);
                if let Some(t) = payload["event_type"].as_str() {
                    self.selector_status = selector_status_after_event(self.selector_status, t);
                    if matches!(
                        t,
                        "loading_started" | "loading_completed" | "loading_failed" | "unloaded"
                    ) {
                        self.selector_error = (t == "loading_failed").then(|| {
                            payload["error"]
                                .as_str()
                                .filter(|e| !e.is_empty())
                                .unwrap_or("Failed to load model")
                                .to_string()
                        });
                    }
                    if t == "loading_completed" || t == "loading_failed" {
                        self.pending_model = None;
                    }
                }
            }
            "history-update-payload" => {
                if let Some(p) = event.decode::<HistoryUpdatePayload>() {
                    cx.emit(AppEvent::History(p));
                }
            }
            "secure-input-changed" => {
                if let Some(s) = event.decode::<SecureInputStatus>() {
                    self.secure_input = Some(s);
                }
            }
            "native-keys-event" => {
                if let Some(k) = event.decode::<FrontendKeyEvent>() {
                    cx.emit(AppEvent::NativeKeys(k));
                }
            }
            "check-for-updates" => cx.emit(AppEvent::CheckForUpdates),
            name if crate::logic::meetings::is_meetings_event(name) => {
                match crate::logic::meetings::decode_event(name, payload) {
                    Some(MeetingsEvent::Ready) => self.set_meetings_init(MeetingsInit::Ready, cx),
                    Some(MeetingsEvent::InitFailed(e)) => {
                        self.set_meetings_init(MeetingsInit::Failed(e.clone()), cx);
                        cx.emit(AppEvent::Meetings(MeetingsEvent::InitFailed(e)));
                    }
                    Some(MeetingsEvent::DataKeyMissing(e)) => {
                        self.set_meetings_init(MeetingsInit::DataKeyMissing(e.clone()), cx);
                        cx.emit(AppEvent::Meetings(MeetingsEvent::DataKeyMissing(e)));
                    }
                    Some(ev) => cx.emit(AppEvent::Meetings(ev)),
                    None => {}
                }
            }
            "theme-changed" => {
                if let Some(t) = event.decode::<guidy_core::settings::Theme>() {
                    crate::theme::apply_setting(t, cx);
                }
            }
            guidy_core::logging::LOG_EVENT => {
                if let Some(p) = event.decode::<guidy_core::logging::LogRecordPayload>() {
                    self.push_log(p.level, p.message, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

/// Switch the UI language (falls back to the OS locale when unset).
pub fn apply_app_language(setting: &str) {
    let tag = if setting.is_empty() {
        sys_locale_tag()
    } else {
        setting.to_string()
    };
    guidy_i18n::set_language(&tag);
}

fn sys_locale_tag() -> String {
    std::env::var("LANG")
        .ok()
        .map(|l| l.split('.').next().unwrap_or("en").to_string())
        .unwrap_or_else(|| "en".into())
}
