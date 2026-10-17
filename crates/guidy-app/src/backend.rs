//! The UI's view of the backend.
//!
//! Every Tauri command Handy's React UI invoked is a method here. Production
//! uses [`CoreBackend`] (calls into `guidy-core` with the live app handle);
//! UI tests use a recording fake. Methods are synchronous and may block, so
//! views call them from background tasks.

use guidy_core::commands::audio::{AudioDevice, CustomSounds, WindowsMicrophonePermissionStatus};
use guidy_core::managers::history::PaginatedHistory;
use guidy_core::managers::model::{ModelInfo, RetiredModel};
use guidy_core::managers::transcription::AvailableAccelerators;
use guidy_core::secure_input::{KeyboardDiagnosticReport, SecureInputStatus};
use guidy_core::settings::{AppSettings, LLMPrompt, PostProcessApiFormat};
use guidy_core::shortcut::{BindingResponse, ImplementationChangeResult};
use guidy_core::AppHandle;
use serde_json::Value;
use std::sync::Arc;

pub type BackendResult<T> = Result<T, String>;

pub trait Backend: Send + Sync + 'static {
    // -- settings ---------------------------------------------------------
    fn get_app_settings(&self) -> BackendResult<AppSettings>;
    fn get_default_settings(&self) -> BackendResult<AppSettings>;
    /// Persist one setting through its dedicated command (Handy's
    /// `settingsStore` updater table). `value` is the setting's JSON value.
    fn update_setting(&self, key: &str, value: Value) -> BackendResult<()>;
    fn set_model_unload_timeout(&self, timeout: Value) -> BackendResult<()>;

    // -- shortcuts --------------------------------------------------------
    fn change_binding(&self, id: &str, binding: &str) -> BackendResult<BindingResponse>;
    fn reset_binding(&self, id: &str) -> BackendResult<BindingResponse>;
    fn suspend_all_bindings(&self) -> BackendResult<()>;
    fn resume_all_bindings(&self) -> BackendResult<()>;
    fn change_keyboard_implementation(
        &self,
        implementation: &str,
    ) -> BackendResult<ImplementationChangeResult>;
    fn start_handy_keys_recording(&self, binding_id: &str) -> BackendResult<()>;
    fn stop_handy_keys_recording(&self) -> BackendResult<()>;

    // -- post-processing --------------------------------------------------
    fn set_post_process_provider(&self, provider_id: &str) -> BackendResult<()>;
    fn change_post_process_base_url(&self, provider_id: &str, base_url: &str) -> BackendResult<()>;
    fn change_post_process_api_format(
        &self,
        provider_id: &str,
        format: PostProcessApiFormat,
    ) -> BackendResult<()>;
    fn change_post_process_api_key(&self, provider_id: &str, api_key: &str) -> BackendResult<()>;
    fn change_post_process_model(&self, provider_id: &str, model: &str) -> BackendResult<()>;
    fn fetch_post_process_models(&self, provider_id: &str) -> BackendResult<Vec<String>>;
    fn add_post_process_prompt(&self, name: &str, prompt: &str) -> BackendResult<LLMPrompt>;
    fn update_post_process_prompt(&self, id: &str, name: &str, prompt: &str) -> BackendResult<()>;
    fn delete_post_process_prompt(&self, id: &str) -> BackendResult<()>;
    fn check_apple_intelligence_available(&self) -> bool;

    // -- system -----------------------------------------------------------
    fn get_available_accelerators(&self) -> AvailableAccelerators;
    fn get_available_typing_tools(&self) -> Vec<String>;
    fn is_portable(&self) -> bool;
    fn is_update_checks_locked(&self) -> bool;
    fn get_app_dir_path(&self) -> BackendResult<String>;
    fn get_log_dir_path(&self) -> BackendResult<String>;
    fn open_recordings_folder(&self) -> BackendResult<()>;
    fn open_log_dir(&self) -> BackendResult<()>;
    fn open_app_data_dir(&self) -> BackendResult<()>;
    fn initialize_enigo(&self) -> BackendResult<()>;
    fn initialize_shortcuts(&self) -> BackendResult<()>;
    fn cancel_operation(&self);
    fn get_secure_input_status(&self) -> SecureInputStatus;
    fn run_keyboard_diagnostic(&self, secs: u32) -> BackendResult<KeyboardDiagnosticReport>;
    fn is_laptop(&self) -> BackendResult<bool>;
    fn show_main_window(&self);
    fn trigger_update_check(&self) -> BackendResult<()>;

    // -- permissions ------------------------------------------------------
    fn check_accessibility_permission(&self) -> bool;
    fn request_accessibility_permission(&self) -> bool;
    fn check_microphone_permission(&self) -> bool;
    fn request_microphone_permission(&self);
    fn get_windows_microphone_permission_status(&self) -> WindowsMicrophonePermissionStatus;
    fn open_microphone_privacy_settings(&self) -> BackendResult<()>;

    // -- audio ------------------------------------------------------------
    fn get_available_microphones(&self) -> BackendResult<Vec<AudioDevice>>;
    fn get_available_output_devices(&self) -> BackendResult<Vec<AudioDevice>>;
    fn get_microphone_channels(&self, device_name: &str) -> BackendResult<u16>;
    fn check_custom_sounds(&self) -> CustomSounds;
    fn play_test_sound(&self, sound_type: &str);
    fn is_recording(&self) -> bool;

    // -- models -----------------------------------------------------------
    fn get_available_models(&self) -> BackendResult<Vec<ModelInfo>>;
    fn get_current_model(&self) -> BackendResult<String>;
    fn rescan_local_models(&self) -> BackendResult<()>;
    fn download_model(&self, model_id: &str) -> BackendResult<()>;
    fn cancel_download(&self, model_id: &str) -> BackendResult<()>;
    fn delete_model(&self, model_id: &str) -> BackendResult<()>;
    fn set_active_model(&self, model_id: &str) -> BackendResult<()>;
    fn get_transcription_model_status(&self) -> BackendResult<Option<String>>;
    fn get_retired_models(&self) -> Vec<RetiredModel>;
    fn delete_retired_model(&self, model_id: &str) -> BackendResult<()>;

    // -- history ----------------------------------------------------------
    fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: usize,
    ) -> BackendResult<PaginatedHistory>;
    fn toggle_history_entry_saved(&self, id: i64) -> BackendResult<()>;
    fn get_audio_file_path(&self, file_name: &str) -> BackendResult<String>;
    fn delete_history_entry(&self, id: i64) -> BackendResult<()>;
    fn retry_history_entry_transcription(&self, id: i64) -> BackendResult<()>;
}

pub type SharedBackend = Arc<dyn Backend>;

fn from_json<T: serde::de::DeserializeOwned>(key: &str, value: Value) -> BackendResult<T> {
    serde_json::from_value(value).map_err(|e| format!("Invalid value for {key}: {e}"))
}

fn json_string(key: &str, value: &Value) -> BackendResult<String> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Invalid value for {key}: expected a string"))
}

/// A device setting value: `null` (the reset default) and `"Default"` both
/// mean the system default device, stored as `"default"`.
fn device_name(key: &str, value: &Value) -> BackendResult<String> {
    if value.is_null() {
        return Ok("default".into());
    }
    let name = json_string(key, value)?;
    Ok(if name == "Default" {
        "default".into()
    } else {
        name
    })
}

/// The live backend.
pub struct CoreBackend {
    app: AppHandle,
}

impl CoreBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn a(&self) -> AppHandle {
        self.app.clone()
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    guidy_core::host::async_runtime::block_on(f)
}

impl Backend for CoreBackend {
    fn get_app_settings(&self) -> BackendResult<AppSettings> {
        guidy_core::commands::get_app_settings(self.a())
    }

    fn get_default_settings(&self) -> BackendResult<AppSettings> {
        guidy_core::commands::get_default_settings()
    }

    fn update_setting(&self, key: &str, value: Value) -> BackendResult<()> {
        use guidy_core::commands::audio;
        use guidy_core::commands::history;
        use guidy_core::host::State;
        use guidy_core::shortcut as s;
        let app = self.a();
        let b = |v: &Value| {
            v.as_bool()
                .ok_or_else(|| format!("Invalid value for {key}: expected a boolean"))
        };
        let u = |v: &Value| {
            v.as_u64()
                .ok_or_else(|| format!("Invalid value for {key}: expected an integer"))
        };
        let f = |v: &Value| {
            v.as_f64()
                .ok_or_else(|| format!("Invalid value for {key}: expected a number"))
        };
        match key {
            "always_on_microphone" => block_on(audio::update_microphone_mode(app, b(&value)?)),
            "audio_feedback" => s::change_audio_feedback_setting(app, b(&value)?),
            "audio_feedback_volume" => {
                s::change_audio_feedback_volume_setting(app, f(&value)? as f32)
            }
            "sound_theme" => s::change_sound_theme_setting(app, json_string(key, &value)?),
            "start_hidden" => s::change_start_hidden_setting(app, b(&value)?),
            "autostart_enabled" => s::change_autostart_setting(app, b(&value)?),
            "update_checks_enabled" => s::change_update_checks_setting(app, b(&value)?),
            "show_whats_new_on_update" => {
                s::change_show_whats_new_on_update_setting(app, b(&value)?)
            }
            "whats_new_last_seen_version" => {
                s::change_whats_new_last_seen_version_setting(app, json_string(key, &value)?)
            }
            "shortcut_activation" => {
                s::change_shortcut_activation_setting(app, from_json(key, value)?)
            }
            "hold_threshold_ms" => s::change_hold_threshold_ms_setting(app, u(&value)?),
            "selected_microphone" => {
                let name = device_name(key, &value)?;
                block_on(audio::set_selected_microphone(app, name))
            }
            "selected_channel" => {
                block_on(audio::set_selected_channel(app, from_json(key, value)?))
            }
            "clamshell_microphone" => {
                let name = device_name(key, &value)?;
                audio::set_clamshell_microphone(app, name)
            }
            "selected_output_device" => {
                let name = device_name(key, &value)?;
                audio::set_selected_output_device(app, name)
            }
            "recording_retention_period" => {
                let period = json_string(key, &value)?;
                let hm = app
                    .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
                    .ok_or("History manager not ready")?;
                let state: State<'_, Arc<_>> = hm;
                block_on(history::update_recording_retention_period(
                    app.clone(),
                    state,
                    period,
                ))
            }
            "history_limit" => {
                let limit = u(&value)? as usize;
                let hm = app
                    .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
                    .ok_or("History manager not ready")?;
                block_on(history::update_history_limit(app.clone(), hm, limit))
            }
            "translate_to_english" => s::change_translate_to_english_setting(app, b(&value)?),
            "selected_language" => {
                s::change_selected_language_setting(app, json_string(key, &value)?)
            }
            "overlay_position" => {
                s::change_overlay_position_setting(app, json_string(key, &value)?)
            }
            "overlay_style" => s::change_overlay_style_setting(app, json_string(key, &value)?),
            "debug_mode" => s::change_debug_mode_setting(app, b(&value)?),
            "custom_words" => s::update_custom_words(app, from_json(key, value)?),
            "word_correction_threshold" => {
                s::change_word_correction_threshold_setting(app, f(&value)?)
            }
            "paste_delay_ms" => s::change_paste_delay_ms_setting(app, u(&value)?),
            "paste_delay_after_ms" => s::change_paste_delay_after_ms_setting(app, u(&value)?),
            "reliable_paste" => s::change_reliable_paste_setting(app, b(&value)?),
            "paste_method" => s::change_paste_method_setting(app, json_string(key, &value)?),
            "typing_tool" => s::change_typing_tool_setting(app, json_string(key, &value)?),
            "external_script_path" => {
                s::change_external_script_path_setting(app, from_json(key, value)?)
            }
            "clipboard_handling" => {
                s::change_clipboard_handling_setting(app, json_string(key, &value)?)
            }
            "auto_submit" => s::change_auto_submit_setting(app, b(&value)?),
            "auto_submit_key" => s::change_auto_submit_key_setting(app, json_string(key, &value)?),
            "post_process_enabled" => s::change_post_process_enabled_setting(app, b(&value)?),
            "post_process_selected_prompt_id" => {
                s::set_post_process_selected_prompt(app, json_string(key, &value)?)
            }
            "mute_while_recording" => s::change_mute_while_recording_setting(app, b(&value)?),
            "append_trailing_space" => s::change_append_trailing_space_setting(app, b(&value)?),
            "log_level" => guidy_core::commands::set_log_level(app, from_json(key, value)?),
            "app_language" => s::change_app_language_setting(app, json_string(key, &value)?),
            "theme" => s::change_theme_setting(app, json_string(key, &value)?),
            "experimental_enabled" => s::change_experimental_enabled_setting(app, b(&value)?),
            "lazy_stream_close" => s::change_lazy_stream_close_setting(app, b(&value)?),
            "vad_enabled" => s::change_vad_enabled_setting(app, b(&value)?),
            "long_form_chunking" => s::change_long_form_chunking_setting(app, b(&value)?),
            "vad_backend" => block_on(s::change_vad_backend_setting(app, from_json(key, value)?)),
            "filler_word_removal_enabled" => {
                s::change_filler_word_removal_enabled_setting(app, b(&value)?)
            }
            "chinese_script" => s::change_chinese_script_setting(app, from_json(key, value)?),
            "show_tray_icon" => s::change_show_tray_icon_setting(app, b(&value)?),
            "transcribe_accelerator" => {
                s::change_transcribe_accelerator_setting(app, from_json(key, value)?)
            }
            "transcribe_gpu_device" => s::change_transcribe_gpu_device(app, from_json(key, value)?),
            "extra_recording_buffer_ms" => {
                s::change_extra_recording_buffer_setting(app, u(&value)?)
            }
            other => {
                log::warn!("No handler for setting: {other}");
                Ok(())
            }
        }
    }

    fn set_model_unload_timeout(&self, timeout: Value) -> BackendResult<()> {
        let timeout = from_json("model_unload_timeout", timeout)?;
        guidy_core::commands::transcription::set_model_unload_timeout(self.a(), timeout);
        Ok(())
    }

    fn change_binding(&self, id: &str, binding: &str) -> BackendResult<BindingResponse> {
        guidy_core::shortcut::change_binding(self.a(), id.into(), binding.into())
    }

    fn reset_binding(&self, id: &str) -> BackendResult<BindingResponse> {
        guidy_core::shortcut::reset_binding(self.a(), id.into())
    }

    fn suspend_all_bindings(&self) -> BackendResult<()> {
        guidy_core::shortcut::suspend_all_bindings(self.a())
    }

    fn resume_all_bindings(&self) -> BackendResult<()> {
        guidy_core::shortcut::resume_all_bindings(self.a())
    }

    fn change_keyboard_implementation(
        &self,
        implementation: &str,
    ) -> BackendResult<ImplementationChangeResult> {
        guidy_core::shortcut::change_keyboard_implementation_setting(
            self.a(),
            implementation.into(),
        )
    }

    fn start_handy_keys_recording(&self, binding_id: &str) -> BackendResult<()> {
        guidy_core::shortcut::handy_keys::start_handy_keys_recording(self.a(), binding_id.into())
    }

    fn stop_handy_keys_recording(&self) -> BackendResult<()> {
        guidy_core::shortcut::handy_keys::stop_handy_keys_recording(self.a())
    }

    fn set_post_process_provider(&self, provider_id: &str) -> BackendResult<()> {
        guidy_core::shortcut::set_post_process_provider(self.a(), provider_id.into())
    }

    fn change_post_process_base_url(&self, provider_id: &str, base_url: &str) -> BackendResult<()> {
        guidy_core::shortcut::change_post_process_base_url_setting(
            self.a(),
            provider_id.into(),
            base_url.into(),
        )
    }

    fn change_post_process_api_format(
        &self,
        provider_id: &str,
        format: PostProcessApiFormat,
    ) -> BackendResult<()> {
        guidy_core::shortcut::change_post_process_api_format_setting(
            self.a(),
            provider_id.into(),
            format,
        )
    }

    fn change_post_process_api_key(&self, provider_id: &str, api_key: &str) -> BackendResult<()> {
        guidy_core::shortcut::change_post_process_api_key_setting(
            self.a(),
            provider_id.into(),
            api_key.into(),
        )
    }

    fn change_post_process_model(&self, provider_id: &str, model: &str) -> BackendResult<()> {
        guidy_core::shortcut::change_post_process_model_setting(
            self.a(),
            provider_id.into(),
            model.into(),
        )
    }

    fn fetch_post_process_models(&self, provider_id: &str) -> BackendResult<Vec<String>> {
        block_on(guidy_core::shortcut::fetch_post_process_models(
            self.a(),
            provider_id.into(),
        ))
    }

    fn add_post_process_prompt(&self, name: &str, prompt: &str) -> BackendResult<LLMPrompt> {
        guidy_core::shortcut::add_post_process_prompt(self.a(), name.into(), prompt.into())
    }

    fn update_post_process_prompt(&self, id: &str, name: &str, prompt: &str) -> BackendResult<()> {
        guidy_core::shortcut::update_post_process_prompt(
            self.a(),
            id.into(),
            name.into(),
            prompt.into(),
        )
    }

    fn delete_post_process_prompt(&self, id: &str) -> BackendResult<()> {
        guidy_core::shortcut::delete_post_process_prompt(self.a(), id.into())
    }

    fn check_apple_intelligence_available(&self) -> bool {
        guidy_core::commands::check_apple_intelligence_available()
    }

    fn get_available_accelerators(&self) -> AvailableAccelerators {
        block_on(guidy_core::shortcut::get_available_accelerators(self.a()))
    }

    fn get_available_typing_tools(&self) -> Vec<String> {
        guidy_core::shortcut::get_available_typing_tools()
    }

    fn is_portable(&self) -> bool {
        guidy_core::commands::is_portable()
    }

    fn is_update_checks_locked(&self) -> bool {
        // A build without an update endpoint can't check: show it as locked
        // rather than reporting "up to date".
        guidy_core::commands::is_update_checks_locked() || !crate::updater::configured()
    }

    fn get_app_dir_path(&self) -> BackendResult<String> {
        guidy_core::commands::get_app_dir_path(self.a())
    }

    fn get_log_dir_path(&self) -> BackendResult<String> {
        guidy_core::commands::get_log_dir_path(self.a())
    }

    fn open_recordings_folder(&self) -> BackendResult<()> {
        guidy_core::commands::open_recordings_folder(self.a())
    }

    fn open_log_dir(&self) -> BackendResult<()> {
        guidy_core::commands::open_log_dir(self.a())
    }

    fn open_app_data_dir(&self) -> BackendResult<()> {
        guidy_core::commands::open_app_data_dir(self.a())
    }

    fn initialize_enigo(&self) -> BackendResult<()> {
        guidy_core::commands::initialize_enigo(self.a())
    }

    fn initialize_shortcuts(&self) -> BackendResult<()> {
        guidy_core::commands::initialize_shortcuts(self.a())
    }

    fn cancel_operation(&self) {
        guidy_core::commands::cancel_operation(self.a())
    }

    fn get_secure_input_status(&self) -> SecureInputStatus {
        guidy_core::secure_input::get_secure_input_status(self.a())
    }

    fn run_keyboard_diagnostic(&self, secs: u32) -> BackendResult<KeyboardDiagnosticReport> {
        block_on(guidy_core::secure_input::run_keyboard_diagnostic(Some(
            secs,
        )))
    }

    fn is_laptop(&self) -> BackendResult<bool> {
        guidy_core::helpers::clamshell::is_laptop()
    }

    fn show_main_window(&self) {
        let _ = guidy_core::show_main_window_command(self.a());
    }

    fn trigger_update_check(&self) -> BackendResult<()> {
        guidy_core::trigger_update_check(self.a())
    }

    fn check_accessibility_permission(&self) -> bool {
        guidy_core::permissions::check_accessibility_permission()
    }

    fn request_accessibility_permission(&self) -> bool {
        guidy_core::permissions::request_accessibility_permission()
    }

    fn check_microphone_permission(&self) -> bool {
        guidy_core::permissions::check_microphone_permission()
    }

    fn request_microphone_permission(&self) {
        guidy_core::permissions::request_microphone_permission()
    }

    fn get_windows_microphone_permission_status(&self) -> WindowsMicrophonePermissionStatus {
        guidy_core::commands::audio::get_windows_microphone_permission_status()
    }

    fn open_microphone_privacy_settings(&self) -> BackendResult<()> {
        guidy_core::commands::audio::open_microphone_privacy_settings()
    }

    fn get_available_microphones(&self) -> BackendResult<Vec<AudioDevice>> {
        block_on(guidy_core::commands::audio::get_available_microphones())
    }

    fn get_available_output_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        block_on(guidy_core::commands::audio::get_available_output_devices())
    }

    fn get_microphone_channels(&self, device_name: &str) -> BackendResult<u16> {
        block_on(guidy_core::commands::audio::get_microphone_channels(
            device_name.into(),
        ))
    }

    fn check_custom_sounds(&self) -> CustomSounds {
        guidy_core::commands::audio::check_custom_sounds(self.a())
    }

    fn play_test_sound(&self, sound_type: &str) {
        block_on(guidy_core::commands::audio::play_test_sound(
            self.a(),
            sound_type.into(),
        ))
    }

    fn is_recording(&self) -> bool {
        guidy_core::commands::audio::is_recording(self.a())
    }

    fn get_available_models(&self) -> BackendResult<Vec<ModelInfo>> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        block_on(guidy_core::commands::models::get_available_models(mm))
    }

    fn get_current_model(&self) -> BackendResult<String> {
        block_on(guidy_core::commands::models::get_current_model(self.a()))
    }

    fn rescan_local_models(&self) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        block_on(guidy_core::commands::models::rescan_local_models(mm))
    }

    fn download_model(&self, model_id: &str) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        block_on(guidy_core::commands::models::download_model(
            self.a(),
            mm,
            model_id.into(),
        ))
    }

    fn cancel_download(&self, model_id: &str) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        block_on(guidy_core::commands::models::cancel_download(
            mm,
            model_id.into(),
        ))
    }

    fn delete_model(&self, model_id: &str) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        let tm = self
            .app
            .try_state::<Arc<guidy_core::managers::transcription::TranscriptionManager>>()
            .ok_or("Transcription manager not ready")?;
        block_on(guidy_core::commands::models::delete_model(
            self.a(),
            mm,
            tm,
            model_id.into(),
        ))
    }

    fn set_active_model(&self, model_id: &str) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        let tm = self
            .app
            .try_state::<Arc<guidy_core::managers::transcription::TranscriptionManager>>()
            .ok_or("Transcription manager not ready")?;
        block_on(guidy_core::commands::models::set_active_model(
            self.a(),
            mm,
            tm,
            model_id.into(),
        ))
    }

    fn get_transcription_model_status(&self) -> BackendResult<Option<String>> {
        let tm = self
            .app
            .try_state::<Arc<guidy_core::managers::transcription::TranscriptionManager>>()
            .ok_or("Transcription manager not ready")?;
        block_on(guidy_core::commands::models::get_transcription_model_status(tm))
    }

    fn get_retired_models(&self) -> Vec<RetiredModel> {
        match self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
        {
            Some(mm) => guidy_core::commands::models::get_retired_models(mm),
            None => Vec::new(),
        }
    }

    fn delete_retired_model(&self, model_id: &str) -> BackendResult<()> {
        let mm = self
            .app
            .try_state::<Arc<guidy_core::managers::model::ModelManager>>()
            .ok_or("Model manager not ready")?;
        block_on(guidy_core::commands::models::delete_retired_model(
            mm,
            model_id.into(),
        ))
    }

    fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: usize,
    ) -> BackendResult<PaginatedHistory> {
        let hm = self
            .app
            .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
            .ok_or("History manager not ready")?;
        block_on(guidy_core::commands::history::get_history_entries(
            self.a(),
            hm,
            cursor,
            Some(limit),
        ))
    }

    fn toggle_history_entry_saved(&self, id: i64) -> BackendResult<()> {
        let hm = self
            .app
            .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
            .ok_or("History manager not ready")?;
        block_on(guidy_core::commands::history::toggle_history_entry_saved(
            self.a(),
            hm,
            id,
        ))
    }

    fn get_audio_file_path(&self, file_name: &str) -> BackendResult<String> {
        let hm = self
            .app
            .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
            .ok_or("History manager not ready")?;
        block_on(guidy_core::commands::history::get_audio_file_path(
            self.a(),
            hm,
            file_name.into(),
        ))
    }

    fn delete_history_entry(&self, id: i64) -> BackendResult<()> {
        let hm = self
            .app
            .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
            .ok_or("History manager not ready")?;
        block_on(guidy_core::commands::history::delete_history_entry(
            self.a(),
            hm,
            id,
        ))
    }

    fn retry_history_entry_transcription(&self, id: i64) -> BackendResult<()> {
        let hm = self
            .app
            .try_state::<Arc<guidy_core::managers::history::HistoryManager>>()
            .ok_or("History manager not ready")?;
        let tm = self
            .app
            .try_state::<Arc<guidy_core::managers::transcription::TranscriptionManager>>()
            .ok_or("Transcription manager not ready")?;
        block_on(
            guidy_core::commands::history::retry_history_entry_transcription(self.a(), hm, tm, id),
        )
    }
}

/// Every key Handy's settings store could update (its updater table).
pub const UPDATABLE_SETTINGS: &[&str] = &[
    "always_on_microphone",
    "audio_feedback",
    "audio_feedback_volume",
    "sound_theme",
    "start_hidden",
    "autostart_enabled",
    "update_checks_enabled",
    "show_whats_new_on_update",
    "whats_new_last_seen_version",
    "shortcut_activation",
    "hold_threshold_ms",
    "selected_microphone",
    "selected_channel",
    "clamshell_microphone",
    "selected_output_device",
    "recording_retention_period",
    "translate_to_english",
    "selected_language",
    "overlay_position",
    "overlay_style",
    "debug_mode",
    "custom_words",
    "word_correction_threshold",
    "paste_delay_ms",
    "paste_delay_after_ms",
    "reliable_paste",
    "paste_method",
    "typing_tool",
    "external_script_path",
    "clipboard_handling",
    "auto_submit",
    "auto_submit_key",
    "history_limit",
    "post_process_enabled",
    "post_process_selected_prompt_id",
    "mute_while_recording",
    "append_trailing_space",
    "log_level",
    "app_language",
    "theme",
    "experimental_enabled",
    "lazy_stream_close",
    "vad_enabled",
    "long_form_chunking",
    "vad_backend",
    "filler_word_removal_enabled",
    "chinese_script",
    "show_tray_icon",
    "transcribe_accelerator",
    "transcribe_gpu_device",
    "extra_recording_buffer_ms",
];

#[cfg(test)]
mod tests {
    use super::*;
    use guidy_core::host::testing::TestApp;
    use serde_json::json;

    /// Settings whose command only persists (no hardware/manager side
    /// effects) round-trip through the real core.
    #[test]
    fn simple_settings_persist_through_core() {
        let t = TestApp::new();
        let backend = CoreBackend::new(t.app.clone());
        let cases: Vec<(&str, Value)> = vec![
            ("audio_feedback", json!(true)),
            ("audio_feedback_volume", json!(0.25)),
            ("sound_theme", json!("pop")),
            ("start_hidden", json!(true)),
            ("update_checks_enabled", json!(false)),
            ("show_whats_new_on_update", json!(false)),
            ("whats_new_last_seen_version", json!("0.1.0")),
            ("shortcut_activation", json!("toggle")),
            ("hold_threshold_ms", json!(450)),
            ("translate_to_english", json!(true)),
            ("selected_language", json!("de")),
            ("overlay_position", json!("top")),
            ("overlay_style", json!("minimal")),
            ("debug_mode", json!(true)),
            ("custom_words", json!(["Guidy", "GPUI"])),
            ("word_correction_threshold", json!(0.3)),
            ("paste_delay_ms", json!(120)),
            ("paste_delay_after_ms", json!(90)),
            ("reliable_paste", json!(false)),
            ("paste_method", json!("none")),
            ("clipboard_handling", json!("copy_to_clipboard")),
            ("auto_submit", json!(true)),
            ("auto_submit_key", json!("ctrl_enter")),
            ("post_process_enabled", json!(true)),
            ("mute_while_recording", json!(true)),
            ("append_trailing_space", json!(true)),
            ("log_level", json!("trace")),
            ("app_language", json!("fr")),
            ("theme", json!("dark")),
            ("experimental_enabled", json!(true)),
            ("lazy_stream_close", json!(true)),
            ("vad_enabled", json!(false)),
            ("long_form_chunking", json!(false)),
            ("filler_word_removal_enabled", json!(false)),
            ("chinese_script", json!("simplified")),
            ("extra_recording_buffer_ms", json!(300)),
        ];
        for (key, value) in &cases {
            backend
                .update_setting(key, value.clone())
                .unwrap_or_else(|e| panic!("{key}: {e}"));
        }
        let stored = serde_json::to_value(backend.get_app_settings().unwrap()).unwrap();
        for (key, value) in cases {
            assert_eq!(stored[key], value, "{key} did not persist");
        }
    }

    #[test]
    fn invalid_values_are_rejected_not_written() {
        let t = TestApp::new();
        let backend = CoreBackend::new(t.app.clone());
        assert!(backend
            .update_setting("audio_feedback", json!("yes"))
            .is_err());
        assert!(backend
            .update_setting("hold_threshold_ms", json!("x"))
            .is_err());
        assert!(backend
            .update_setting("shortcut_activation", json!("bogus"))
            .is_err());
    }

    #[test]
    fn theme_change_emits_theme_changed() {
        let t = TestApp::new();
        CoreBackend::new(t.app.clone())
            .update_setting("theme", json!("light"))
            .unwrap();
        assert!(t.event_names().contains(&"theme-changed".to_string()));
    }

    #[test]
    fn debug_mode_change_emits_settings_changed() {
        let t = TestApp::new();
        CoreBackend::new(t.app.clone())
            .update_setting("debug_mode", json!(true))
            .unwrap();
        let ev = t.take_events();
        let changed = ev.iter().find(|e| e.name == "settings-changed").unwrap();
        assert_eq!(changed.payload["setting"], "debug_mode");
    }

    #[test]
    fn device_reset_means_the_system_default() {
        assert_eq!(device_name("k", &Value::Null).unwrap(), "default");
        assert_eq!(device_name("k", &json!("Default")).unwrap(), "default");
        assert_eq!(device_name("k", &json!("USB Mic")).unwrap(), "USB Mic");
        assert!(device_name("k", &json!(3)).is_err());
    }

    #[test]
    fn builds_without_an_update_endpoint_are_locked() {
        let t = TestApp::new();
        let backend = CoreBackend::new(t.app.clone());
        if !crate::updater::configured() {
            assert!(backend.is_update_checks_locked());
        }
    }

    #[test]
    fn every_updatable_key_has_a_handler() {
        // Unknown keys fall through to a warning; every listed key must be
        // matched explicitly. Exercising with null values must error (a
        // handler parsed it) rather than silently succeed (no handler).
        let t = TestApp::new();
        let backend = CoreBackend::new(t.app.clone());
        // Device selections take null as "reset to the system default".
        let nullable = [
            "selected_channel",
            "external_script_path",
            "transcribe_gpu_device",
            "selected_microphone",
            "selected_output_device",
            "clamshell_microphone",
        ];
        for key in UPDATABLE_SETTINGS {
            if nullable.contains(key) {
                continue;
            }
            assert!(
                backend.update_setting(key, Value::Null).is_err(),
                "{key} has no handler"
            );
        }
    }
}
