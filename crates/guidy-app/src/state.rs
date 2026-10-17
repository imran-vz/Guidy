//! Application state shared by every view (the zustand stores of Handy's UI):
//! settings, models, devices, secure input, live logs. Backend calls run on
//! the background executor; results land back on the entity.

use crate::backend::SharedBackend;
use crate::logic::flows::{
    after_permissions, append_log_lines, initial_step, LogLine, OnboardingStep, StartupFacts,
    LOG_FLUSH_MS,
};
use crate::logic::keyboard::OsType;
use crate::logic::models::{initial_selector_status, selector_status_after_event, SelectorStatus};
use crate::logic::models_store::{ModelsEffect, ModelsStore};
use crate::logic::settings_store::{toast_for_event, SettingsStore, ToastKind, ToastSpec};
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global};
use guidy_core::commands::audio::{AudioDevice, CustomSounds};
use guidy_core::host::Event as CoreEvent;
use guidy_core::managers::history::HistoryUpdatePayload;
use guidy_core::secure_input::SecureInputStatus;
use guidy_core::settings::AppSettings;
use guidy_core::shortcut::handy_keys::FrontendKeyEvent;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// UI-level notifications other views react to.
#[derive(Clone, Debug)]
pub enum AppEvent {
    Toast(ToastSpec),
    History(HistoryUpdatePayload),
    HandyKeys(FrontendKeyEvent),
    CheckForUpdates,
    /// Auto-select candidate (ModelSelector), already delayed 500 ms.
    ModelDownloadCompleted(String),
    LanguageChanged,
}

/// The startup step machine (Handy's `App.tsx` onboarding state). Lives on
/// the app, not the window: it must run when Guidy starts hidden, and survive
/// the settings window being closed and reopened.
#[derive(Default)]
pub struct Startup {
    /// `None` while the startup check runs.
    pub step: Option<OnboardingStep>,
    pub returning_user: bool,
    pub migration_model_id: Option<String>,
    checked: bool,
    initialized_after_onboarding: bool,
}

pub struct AppState {
    pub backend: SharedBackend,
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

fn device_with_default(mut devices: Vec<AudioDevice>) -> Vec<AudioDevice> {
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
            startup: Startup::default(),
            settings: SettingsStore::default(),
            models: ModelsStore::default(),
            selector_status: SelectorStatus::None,
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
        let entity = cx.new(|cx| {
            let mut s = AppState::new(backend);
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
    /// model). Runs once per session.
    pub fn check_onboarding(&mut self, cx: &mut Context<Self>) {
        if self.startup.checked {
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
            // permission prompts), then load devices.
            self.call(
                cx,
                |b| (b.initialize_enigo(), b.initialize_shortcuts()),
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

    /// Apply writes one after another (Handy awaited each), stopping at the
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
                    // Like Handy: fall back to "Default" so the select stays usable.
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
                    // Like Handy: fall back to "Default" so the select stays usable.
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
                match r {
                    // A new endpoint makes the stored model meaningless
                    // (settingsStore.updatePostProcessBaseUrl).
                    Ok(()) if what == "base_url" => {
                        s.settings.updating.remove(&key);
                        s.update_post_process("model", &pid_done, String::new(), cx);
                        return;
                    }
                    Ok(()) => {}
                    Err(e) => log::error!("Failed to update post-process {what}: {e}"),
                }
                s.settings.updating.remove(&key);
                s.refresh_settings(cx);
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
            move |s, r, cx| {
                s.settings.updating.remove(&key);
                match r {
                    Ok(models) => {
                        s.pp_model_options.insert(pid2, models);
                    }
                    Err(e) => {
                        log::error!("Failed to fetch models: {e}");
                        s.toast_error(e, cx);
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
            ModelsEffect::ToastError(e) => self.toast_error(e, cx),
            // ModelSelector (mounted only once onboarding is done): during
            // onboarding and migration those screens pick the model.
            ModelsEffect::DownloadCompleted(_) if !self.onboarding_done() => {}
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
            "handy-keys-event" => {
                if let Some(k) = event.decode::<FrontendKeyEvent>() {
                    cx.emit(AppEvent::HandyKeys(k));
                }
            }
            "check-for-updates" => cx.emit(AppEvent::CheckForUpdates),
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
