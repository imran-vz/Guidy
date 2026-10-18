use crate::host::{AppHandle, State};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{update_settings, ModelUnloadTimeout};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, serde::Deserialize, Clone, Debug)]
pub struct ModelLoadStatus {
    pub is_loaded: bool,
    pub current_model: Option<String>,
}
pub fn set_model_unload_timeout(app: AppHandle, timeout: ModelUnloadTimeout) {
    update_settings(&app, |s| s.model_unload_timeout = timeout);
}
pub fn get_model_load_status(
    transcription_manager: State<Arc<TranscriptionManager>>,
) -> Result<ModelLoadStatus, String> {
    Ok(ModelLoadStatus {
        is_loaded: transcription_manager.is_model_loaded(),
        current_model: transcription_manager.get_current_model(),
    })
}
pub fn unload_model_manually(
    transcription_manager: State<Arc<TranscriptionManager>>,
) -> Result<(), String> {
    transcription_manager.request_unload();
    Ok(())
}
