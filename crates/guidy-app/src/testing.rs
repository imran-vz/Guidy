//! Test support: an in-memory backend that records every command, plus
//! helpers to stand up the app state in a GPUI test context.

use crate::backend::{Backend, BackendResult};
use guidy_core::commands::audio::{
    AudioDevice, CustomSounds, PermissionAccess, WindowsMicrophonePermissionStatus,
};
use guidy_core::managers::history::{HistoryEntry, PaginatedHistory};
use guidy_core::managers::model::{ModelInfo, RetiredModel};
use guidy_core::managers::transcription::AvailableAccelerators;
use guidy_core::secure_input::{KeyboardDiagnosticReport, SecureInputStatus};
use guidy_core::settings::{AppSettings, LLMPrompt, PostProcessApiFormat};
use guidy_core::shortcut::{BindingResponse, ImplementationChangeResult};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;

#[derive(Default)]
pub struct FakeData {
    pub settings: Option<AppSettings>,
    pub models: Vec<ModelInfo>,
    pub current_model: String,
    pub history: Vec<HistoryEntry>,
    pub retired: Vec<RetiredModel>,
    pub microphones: Vec<AudioDevice>,
    pub channels: u16,
    pub fail_settings: Vec<String>,
    pub fail_binding: bool,
    pub accessibility: bool,
    pub microphone: bool,
    pub pp_models: Vec<String>,
    pub fail_download: bool,
    pub fail_devices: bool,
    pub loaded_model: Option<String>,
}

/// Records calls as `"name arg1 arg2"` strings.
pub struct FakeBackend {
    pub calls: Mutex<Vec<String>>,
    pub data: Mutex<FakeData>,
}

impl FakeBackend {
    pub fn new() -> Arc<Self> {
        let mut settings = guidy_core::settings::get_default_settings();
        settings.onboarding_completed = true;
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            data: Mutex::new(FakeData {
                settings: Some(settings),
                microphones: vec![AudioDevice {
                    index: "0".into(),
                    name: "Studio Mic".into(),
                    is_default: true,
                }],
                channels: 1,
                accessibility: true,
                microphone: true,
                ..Default::default()
            }),
        })
    }

    fn rec(&self, call: impl Into<String>) {
        self.calls.lock().push(call.into());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    pub fn called(&self, prefix: &str) -> bool {
        self.calls.lock().iter().any(|c| c.starts_with(prefix))
    }

    pub fn with_settings(&self, f: impl FnOnce(&mut AppSettings)) {
        let mut d = self.data.lock();
        f(d.settings.as_mut().unwrap());
    }

    pub fn settings(&self) -> AppSettings {
        self.data.lock().settings.clone().unwrap()
    }
}

impl Backend for FakeBackend {
    fn get_app_settings(&self) -> BackendResult<AppSettings> {
        self.data
            .lock()
            .settings
            .clone()
            .ok_or("no settings".into())
    }
    fn get_default_settings(&self) -> BackendResult<AppSettings> {
        Ok(guidy_core::settings::get_default_settings())
    }
    fn update_setting(&self, key: &str, value: Value) -> BackendResult<()> {
        self.rec(format!("update_setting {key} {value}"));
        let mut d = self.data.lock();
        if d.fail_settings.iter().any(|k| k == key) {
            return Err(format!("{key} failed"));
        }
        let s = d.settings.as_mut().unwrap();
        let mut json = serde_json::to_value(&*s).unwrap();
        json[key] = value;
        *s = serde_json::from_value(json).map_err(|e| e.to_string())?;
        Ok(())
    }
    fn set_model_unload_timeout(&self, timeout: Value) -> BackendResult<()> {
        self.rec(format!("set_model_unload_timeout {timeout}"));
        Ok(())
    }
    fn change_binding(&self, id: &str, binding: &str) -> BackendResult<BindingResponse> {
        self.rec(format!("change_binding {id} {binding}"));
        if self.data.lock().fail_binding {
            return Ok(BindingResponse {
                success: false,
                binding: None,
                error: Some("taken".into()),
            });
        }
        let mut d = self.data.lock();
        if let Some(b) = d.settings.as_mut().unwrap().bindings.get_mut(id) {
            b.current_binding = binding.into();
        }
        Ok(BindingResponse {
            success: true,
            binding: None,
            error: None,
        })
    }
    fn reset_binding(&self, id: &str) -> BackendResult<BindingResponse> {
        self.rec(format!("reset_binding {id}"));
        Ok(BindingResponse {
            success: true,
            binding: None,
            error: None,
        })
    }
    fn suspend_all_bindings(&self) -> BackendResult<()> {
        self.rec("suspend_all_bindings");
        Ok(())
    }
    fn resume_all_bindings(&self) -> BackendResult<()> {
        self.rec("resume_all_bindings");
        Ok(())
    }
    fn change_keyboard_implementation(
        &self,
        implementation: &str,
    ) -> BackendResult<ImplementationChangeResult> {
        self.rec(format!("change_keyboard_implementation {implementation}"));
        Ok(ImplementationChangeResult {
            success: true,
            reset_bindings: vec!["cancel".into()],
        })
    }
    fn start_native_keys_recording(&self, binding_id: &str) -> BackendResult<()> {
        self.rec(format!("start_native_keys_recording {binding_id}"));
        Ok(())
    }
    fn stop_native_keys_recording(&self) -> BackendResult<()> {
        self.rec("stop_native_keys_recording");
        Ok(())
    }
    fn set_post_process_provider(&self, provider_id: &str) -> BackendResult<()> {
        self.rec(format!("set_post_process_provider {provider_id}"));
        self.with_settings(|s| s.post_process_provider_id = provider_id.into());
        Ok(())
    }
    fn change_post_process_base_url(&self, provider_id: &str, base_url: &str) -> BackendResult<()> {
        self.rec(format!(
            "change_post_process_base_url {provider_id} {base_url}"
        ));
        Ok(())
    }
    fn change_post_process_api_format(
        &self,
        provider_id: &str,
        format: PostProcessApiFormat,
    ) -> BackendResult<()> {
        self.rec(format!(
            "change_post_process_api_format {provider_id} {format:?}"
        ));
        Ok(())
    }
    fn change_post_process_api_key(&self, provider_id: &str, api_key: &str) -> BackendResult<()> {
        self.rec(format!(
            "change_post_process_api_key {provider_id} {api_key}"
        ));
        Ok(())
    }
    fn change_post_process_model(&self, provider_id: &str, model: &str) -> BackendResult<()> {
        self.rec(format!("change_post_process_model {provider_id} {model}"));
        Ok(())
    }
    fn fetch_post_process_models(&self, provider_id: &str) -> BackendResult<Vec<String>> {
        self.rec(format!("fetch_post_process_models {provider_id}"));
        Ok(self.data.lock().pp_models.clone())
    }
    fn add_post_process_prompt(&self, name: &str, prompt: &str) -> BackendResult<LLMPrompt> {
        self.rec(format!("add_post_process_prompt {name}"));
        let p = LLMPrompt {
            id: format!("prompt_{name}"),
            name: name.into(),
            prompt: prompt.into(),
        };
        self.with_settings(|s| s.post_process_prompts.push(p.clone()));
        Ok(p)
    }
    fn update_post_process_prompt(&self, id: &str, name: &str, _prompt: &str) -> BackendResult<()> {
        self.rec(format!("update_post_process_prompt {id} {name}"));
        Ok(())
    }
    fn delete_post_process_prompt(&self, id: &str) -> BackendResult<()> {
        self.rec(format!("delete_post_process_prompt {id}"));
        Ok(())
    }
    fn check_apple_intelligence_available(&self) -> bool {
        false
    }
    fn get_available_accelerators(&self) -> AvailableAccelerators {
        AvailableAccelerators {
            transcribe: vec!["auto".into(), "cpu".into()],
            gpu_devices: vec![],
        }
    }
    fn get_available_typing_tools(&self) -> Vec<String> {
        vec!["auto".into()]
    }
    fn is_portable(&self) -> bool {
        false
    }
    fn is_update_checks_locked(&self) -> bool {
        false
    }
    fn get_app_dir_path(&self) -> BackendResult<String> {
        Ok("/tmp/guidy".into())
    }
    fn get_log_dir_path(&self) -> BackendResult<String> {
        Ok("/tmp/guidy/logs".into())
    }
    fn open_recordings_folder(&self) -> BackendResult<()> {
        self.rec("open_recordings_folder");
        Ok(())
    }
    fn open_log_dir(&self) -> BackendResult<()> {
        self.rec("open_log_dir");
        Ok(())
    }
    fn open_app_data_dir(&self) -> BackendResult<()> {
        self.rec("open_app_data_dir");
        Ok(())
    }
    fn initialize_enigo(&self) -> BackendResult<()> {
        self.rec("initialize_enigo");
        Ok(())
    }
    fn initialize_shortcuts(&self) -> BackendResult<()> {
        self.rec("initialize_shortcuts");
        Ok(())
    }
    fn cancel_operation(&self) {
        self.rec("cancel_operation");
    }
    fn get_secure_input_status(&self) -> SecureInputStatus {
        SecureInputStatus {
            enabled: false,
            sustained: false,
            culprit_pid: None,
            culprit_name: None,
            fallback_active: false,
            covered_bindings: vec![],
            degraded_bindings: vec![],
            uncovered_bindings: vec![],
            recorder_blocked: false,
        }
    }
    fn run_keyboard_diagnostic(&self, secs: u32) -> BackendResult<KeyboardDiagnosticReport> {
        self.rec(format!("run_keyboard_diagnostic {secs}"));
        Ok(KeyboardDiagnosticReport {
            secure_input_enabled: false,
            culprit_pid: None,
            culprit_name: None,
            key_down: 3,
            key_up: 3,
            flags_changed: 1,
            mouse: 0,
            duration_ms: 10_000,
        })
    }
    fn is_laptop(&self) -> BackendResult<bool> {
        Ok(false)
    }
    fn show_main_window(&self) {
        self.rec("show_main_window");
    }
    fn trigger_update_check(&self) -> BackendResult<()> {
        self.rec("trigger_update_check");
        Ok(())
    }
    fn check_accessibility_permission(&self) -> bool {
        self.data.lock().accessibility
    }
    fn request_accessibility_permission(&self) -> bool {
        self.rec("request_accessibility_permission");
        self.data.lock().accessibility
    }
    fn check_microphone_permission(&self) -> bool {
        self.data.lock().microphone
    }
    fn request_microphone_permission(&self) {
        self.rec("request_microphone_permission");
    }
    fn get_windows_microphone_permission_status(&self) -> WindowsMicrophonePermissionStatus {
        WindowsMicrophonePermissionStatus {
            supported: false,
            overall_access: PermissionAccess::Unknown,
            device_access: PermissionAccess::Unknown,
            app_access: PermissionAccess::Unknown,
            desktop_app_access: PermissionAccess::Unknown,
        }
    }
    fn open_microphone_privacy_settings(&self) -> BackendResult<()> {
        self.rec("open_microphone_privacy_settings");
        Ok(())
    }
    fn get_available_microphones(&self) -> BackendResult<Vec<AudioDevice>> {
        self.rec("get_available_microphones");
        if self.data.lock().fail_devices {
            return Err("no audio host".into());
        }
        Ok(self.data.lock().microphones.clone())
    }
    fn get_available_output_devices(&self) -> BackendResult<Vec<AudioDevice>> {
        self.rec("get_available_output_devices");
        Ok(vec![])
    }
    fn get_microphone_channels(&self, _device_name: &str) -> BackendResult<u16> {
        Ok(self.data.lock().channels)
    }
    fn check_custom_sounds(&self) -> CustomSounds {
        CustomSounds {
            start: false,
            stop: false,
        }
    }
    fn play_test_sound(&self, sound_type: &str) {
        self.rec(format!("play_test_sound {sound_type}"));
    }
    fn is_recording(&self) -> bool {
        false
    }
    fn get_available_models(&self) -> BackendResult<Vec<ModelInfo>> {
        Ok(self.data.lock().models.clone())
    }
    fn get_current_model(&self) -> BackendResult<String> {
        Ok(self.data.lock().current_model.clone())
    }
    fn rescan_local_models(&self) -> BackendResult<()> {
        self.rec("rescan_local_models");
        Ok(())
    }
    fn download_model(&self, model_id: &str) -> BackendResult<()> {
        self.rec(format!("download_model {model_id}"));
        if self.data.lock().fail_download {
            return Err("network down".into());
        }
        Ok(())
    }
    fn cancel_download(&self, model_id: &str) -> BackendResult<()> {
        self.rec(format!("cancel_download {model_id}"));
        Ok(())
    }
    fn delete_model(&self, model_id: &str) -> BackendResult<()> {
        self.rec(format!("delete_model {model_id}"));
        Ok(())
    }
    fn set_active_model(&self, model_id: &str) -> BackendResult<()> {
        self.rec(format!("set_active_model {model_id}"));
        self.data.lock().current_model = model_id.into();
        Ok(())
    }
    fn get_transcription_model_status(&self) -> BackendResult<Option<String>> {
        Ok(self.data.lock().loaded_model.clone())
    }
    fn get_retired_models(&self) -> Vec<RetiredModel> {
        self.data.lock().retired.clone()
    }
    fn delete_retired_model(&self, model_id: &str) -> BackendResult<()> {
        self.rec(format!("delete_retired_model {model_id}"));
        self.data.lock().retired.retain(|m| m.id != model_id);
        Ok(())
    }
    fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: usize,
    ) -> BackendResult<PaginatedHistory> {
        self.rec(format!("get_history_entries {cursor:?} {limit}"));
        let all = self.data.lock().history.clone();
        let start = match cursor {
            Some(c) => all
                .iter()
                .position(|e| e.id == c)
                .map(|i| i + 1)
                .unwrap_or(all.len()),
            None => 0,
        };
        let entries: Vec<_> = all.iter().skip(start).take(limit).cloned().collect();
        let has_more = start + entries.len() < all.len();
        Ok(PaginatedHistory { entries, has_more })
    }
    fn toggle_history_entry_saved(&self, id: i64) -> BackendResult<()> {
        self.rec(format!("toggle_history_entry_saved {id}"));
        Ok(())
    }
    fn get_audio_file_path(&self, file_name: &str) -> BackendResult<String> {
        Ok(format!("/tmp/{file_name}"))
    }
    fn delete_history_entry(&self, id: i64) -> BackendResult<()> {
        self.rec(format!("delete_history_entry {id}"));
        Ok(())
    }
    fn retry_history_entry_transcription(&self, id: i64) -> BackendResult<()> {
        self.rec(format!("retry_history_entry_transcription {id}"));
        Ok(())
    }
}

pub fn history_entry(id: i64, text: &str) -> HistoryEntry {
    HistoryEntry {
        id,
        file_name: format!("guidy-{id}.wav"),
        timestamp: 1_700_000_000 + id,
        saved: false,
        title: "Recording".into(),
        transcription_text: text.into(),
        post_processed_text: None,
        post_process_prompt: None,
        post_process_requested: false,
    }
}
