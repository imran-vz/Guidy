//! Batch transcription with word timestamps for meeting notes.
//!
//! Dictation keeps its own [`EngineSupervisor`] (and so its own worker
//! process and loaded model). [`MeetingTranscriber`] owns a *second*
//! supervisor, so processing a meeting never evicts the dictation model and a
//! dictation started mid-processing still works. The trade-off: while a
//! meeting is being processed both models are resident (the default Parakeet
//! v3 Q8_0 is ~0.7 GB). The meeting worker is unloaded as soon as the
//! processing queue drains (see `meetings::processing::jobs`).
//!
//! The worker protocol already carries the full [`Transcript`] (segments,
//! words, tokens), so word timestamps only need
//! [`RunOptions::timestamps`] set to [`TimestampKind::Word`].

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use log::info;
use transcribe_cpp::{Backend, RunOptions, TimestampKind, Transcript};

use super::{resolve_gpu_device, select_transcribe_backend, transcribe_gpu_disabled_for_host};
use crate::engine_supervisor::{
    DeviceSelector, EngineError, EngineSupervisor, LoadSpec, LoadedInfo,
};
use crate::host::AppHandle;
use crate::managers::model::{ModelInfo, ModelManager};
use crate::settings::get_settings;

/// Catalog repo of the default meeting model (Parakeet TDT 0.6B v3).
pub const DEFAULT_MEETING_MODEL_REPO: &str = "handy-computer/parakeet-tdt-0.6b-v3-gguf";
/// The default meeting model id (the catalog's default quant).
pub const DEFAULT_MEETING_MODEL_ID: &str =
    "handy-computer/parakeet-tdt-0.6b-v3-gguf/parakeet-tdt-0.6b-v3-Q8_0.gguf";
pub const DEFAULT_MEETING_MODEL_NAME: &str = "Parakeet TDT 0.6B v3";

/// The meeting model is not installed or not downloaded. Downcast an
/// `anyhow::Error` to this to show a "Download <model>" action in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingModelMissing {
    /// The model id to download (`ModelManager::download_model`).
    pub model_id: String,
    pub model_name: String,
    /// False when the id isn't even in the model list (e.g. a removed custom
    /// model).
    pub known: bool,
}

impl fmt::Display for MeetingModelMissing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.known {
            write!(
                f,
                "The meeting transcription model \"{}\" is not downloaded. Download it in \
                 Settings > Models, then process the meeting again.",
                self.model_name
            )
        } else {
            write!(
                f,
                "The meeting transcription model \"{}\" is not installed. Choose another \
                 meeting model in the meeting settings.",
                self.model_id
            )
        }
    }
}

impl std::error::Error for MeetingModelMissing {}

/// Pick the meeting model id from the model list: the model chosen for
/// meetings (`configured`) when downloaded, else the dictation model
/// (`dictation`), else any downloaded Parakeet v3 quant (preferring the
/// catalog default). Fails only when none of them is downloaded, naming the
/// model to download: the chosen one, else the dictation model, else
/// Parakeet v3.
pub fn resolve_meeting_model_id(
    models: &[ModelInfo],
    configured: Option<&str>,
    dictation: Option<&str>,
) -> std::result::Result<String, MeetingModelMissing> {
    let configured = non_blank(configured);
    let dictation = non_blank(dictation);
    let find = |id: &str| models.iter().find(|m| m.id == id);
    for id in configured.into_iter().chain(dictation) {
        if let Some(m) = find(id).filter(|m| m.is_downloaded) {
            return Ok(m.id.clone());
        }
    }
    let prefix = format!("{DEFAULT_MEETING_MODEL_REPO}/");
    let mut candidates: Vec<&ModelInfo> = models
        .iter()
        .filter(|m| m.is_downloaded && m.id.starts_with(&prefix))
        .collect();
    candidates.sort_by_key(|m| m.id != DEFAULT_MEETING_MODEL_ID);
    if let Some(m) = candidates.first() {
        return Ok(m.id.clone());
    }
    let missing = |id: &str| match find(id) {
        Some(m) => MeetingModelMissing {
            model_id: m.id.clone(),
            model_name: m.name.clone(),
            known: true,
        },
        None => MeetingModelMissing {
            model_id: id.to_string(),
            model_name: id.to_string(),
            known: false,
        },
    };
    Err(match (configured, dictation) {
        (Some(id), _) => missing(id),
        (None, Some(id)) if find(id).is_some() => missing(id),
        _ => MeetingModelMissing {
            model_id: DEFAULT_MEETING_MODEL_ID.to_string(),
            model_name: DEFAULT_MEETING_MODEL_NAME.to_string(),
            known: true,
        },
    })
}

fn non_blank(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

/// Whether the model chosen for meetings is downloaded. `None` when no
/// separate meetings model is chosen (meetings use the dictation model).
/// When it isn't, [`resolve_meeting_model_id`] falls back meanwhile.
pub fn configured_meeting_model_missing(
    models: &[ModelInfo],
    configured: Option<&str>,
) -> Option<MeetingModelMissing> {
    let id = non_blank(configured)?;
    match models.iter().find(|m| m.id == id) {
        Some(m) if m.is_downloaded => None,
        Some(m) => Some(MeetingModelMissing {
            model_id: m.id.clone(),
            model_name: m.name.clone(),
            known: true,
        }),
        None => Some(MeetingModelMissing {
            model_id: id.to_string(),
            model_name: id.to_string(),
            known: false,
        }),
    }
}

/// Resolve the meeting model to `(id, path)`, failing with
/// [`MeetingModelMissing`] when it isn't downloaded.
pub fn resolve_meeting_model(
    model_manager: &ModelManager,
    configured: Option<&str>,
    dictation: Option<&str>,
) -> Result<(String, PathBuf)> {
    let id =
        resolve_meeting_model_id(&model_manager.get_available_models(), configured, dictation)?;
    let path = model_manager.get_model_path(&id)?;
    Ok((id, path))
}

/// Run options for a meeting segment: word timestamps, no vocabulary or
/// translation. `language: None` autodetects.
pub fn word_run_options(language: Option<String>) -> RunOptions {
    RunOptions {
        timestamps: TimestampKind::Word,
        language,
        ..Default::default()
    }
}

/// A transcribe.cpp worker dedicated to meeting processing.
pub struct MeetingTranscriber {
    engine: EngineSupervisor,
    loaded: Option<(PathBuf, LoadedInfo)>,
}

impl MeetingTranscriber {
    /// A transcriber over its own engine supervisor (no worker runs until
    /// [`load`](Self::load)).
    pub fn new() -> Self {
        Self::with_engine(EngineSupervisor::new(!transcribe_gpu_disabled_for_host()))
    }

    /// A transcriber over `engine` (a clone of it can [`EngineSupervisor::cancel`]
    /// from another thread).
    pub fn with_engine(engine: EngineSupervisor) -> Self {
        Self {
            engine,
            loaded: None,
        }
    }

    pub fn engine(&self) -> &EngineSupervisor {
        &self.engine
    }

    /// Load `path` unless it is already loaded. Uses the user's transcribe
    /// accelerator / GPU device settings, like dictation.
    pub fn load(&mut self, app: &AppHandle, path: &Path) -> Result<LoadedInfo> {
        if let Some((loaded_path, info)) = &self.loaded {
            if loaded_path == path && self.engine.loaded().is_some() {
                return Ok(info.clone());
            }
        }
        let settings = get_settings(app);
        let accelerator = settings.transcribe_accelerator;
        let (backend, device) =
            match resolve_gpu_device(accelerator, settings.transcribe_gpu_device.as_deref()) {
                Some(key) => (Backend::Auto, DeviceSelector::Key(key)),
                None => (select_transcribe_backend(accelerator), DeviceSelector::Auto),
            };
        let info = self
            .engine
            .load(LoadSpec {
                path: path.to_path_buf(),
                backend,
                device,
            })
            .map_err(|e| anyhow::anyhow!("Failed to load the meeting model: {e}"))?;
        info!(
            "Meeting model loaded: {} ({} on {})",
            path.display(),
            info.backend,
            info.device
        );
        self.loaded = Some((path.to_path_buf(), info.clone()));
        Ok(info)
    }

    pub fn loaded_info(&self) -> Option<&LoadedInfo> {
        self.loaded.as_ref().map(|(_, info)| info)
    }

    /// Transcribe 16 kHz mono PCM with word timestamps (relative to the
    /// start of `pcm`). A hint `language` the model doesn't list is dropped
    /// (autodetect instead).
    pub fn transcribe_words(
        &self,
        pcm: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<Transcript, EngineError> {
        let language = language.filter(|lang| {
            self.loaded_info().is_some_and(|info| {
                info.capabilities
                    .languages
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(lang))
            })
        });
        self.engine.transcribe(pcm, word_run_options(language))
    }

    /// Stop the run in progress (kills the worker; the next run reloads).
    pub fn cancel(&self) {
        self.engine.cancel();
    }

    /// Unload the model and wait for the worker to exit.
    pub fn unload(&mut self) {
        if self.loaded.take().is_some() {
            self.engine.unload().wait();
        }
    }
}

impl Default for MeetingTranscriber {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::ModelSource;

    fn model(id: &str, downloaded: bool) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: format!("name of {id}"),
            description: String::new(),
            filename: String::new(),
            source: ModelSource::Local,
            size_mb: 0,
            is_downloaded: downloaded,
            is_downloading: false,
            partial_size: 0,
            accuracy_score: 0.0,
            speed_score: 0.0,
            supports_translation: false,
            is_recommended: false,
            supported_languages: Vec::new(),
            supports_language_selection: false,
            is_custom: false,
            supports_streaming: false,
            supports_language_detection: false,
        }
    }

    #[test]
    fn parakeet_is_the_last_resort_preferring_the_catalog_quant() {
        let q4 = format!("{DEFAULT_MEETING_MODEL_REPO}/parakeet-tdt-0.6b-v3-Q4_K_M.gguf");
        let models = vec![model(&q4, true), model(DEFAULT_MEETING_MODEL_ID, true)];
        assert_eq!(
            resolve_meeting_model_id(&models, None, None).unwrap(),
            DEFAULT_MEETING_MODEL_ID
        );
        let models = vec![model(&q4, true), model(DEFAULT_MEETING_MODEL_ID, false)];
        assert_eq!(resolve_meeting_model_id(&models, None, None).unwrap(), q4);
    }

    #[test]
    fn meetings_use_the_dictation_model_by_default() {
        let models = vec![
            model(DEFAULT_MEETING_MODEL_ID, true),
            model("whisper", true),
        ];
        assert_eq!(
            resolve_meeting_model_id(&models, None, Some("whisper")).unwrap(),
            "whisper"
        );
        // A blank meetings model is no choice.
        assert_eq!(
            resolve_meeting_model_id(&models, Some(" "), Some("whisper")).unwrap(),
            "whisper"
        );
    }

    #[test]
    fn a_chosen_meetings_model_wins_once_downloaded() {
        let models = vec![model("parakeet", false), model("whisper", true)];
        // Not downloaded yet: the dictation model meanwhile.
        assert_eq!(
            resolve_meeting_model_id(&models, Some("parakeet"), Some("whisper")).unwrap(),
            "whisper"
        );
        let missing = configured_meeting_model_missing(&models, Some("parakeet")).unwrap();
        assert_eq!(missing.model_id, "parakeet");
        assert!(missing.known);
        let models = vec![model("parakeet", true), model("whisper", true)];
        assert_eq!(
            resolve_meeting_model_id(&models, Some("parakeet"), Some("whisper")).unwrap(),
            "parakeet"
        );
        assert!(configured_meeting_model_missing(&models, Some("parakeet")).is_none());
        assert!(configured_meeting_model_missing(&models, None).is_none());
        assert!(
            !configured_meeting_model_missing(&models, Some("gone"))
                .unwrap()
                .known
        );
    }

    #[test]
    fn nothing_downloaded_names_the_model_to_download() {
        let models = vec![
            model(DEFAULT_MEETING_MODEL_ID, false),
            model("whisper", false),
        ];
        let err = resolve_meeting_model_id(&models, Some("whisper"), None).unwrap_err();
        assert_eq!(err.model_id, "whisper");
        let err = resolve_meeting_model_id(&models, None, Some("whisper")).unwrap_err();
        assert_eq!(err.model_id, "whisper");
        // No dictation model either: Parakeet v3.
        let err = resolve_meeting_model_id(&models, None, None).unwrap_err();
        assert_eq!(err.model_id, DEFAULT_MEETING_MODEL_ID);
        assert!(err.known);
        assert!(err.to_string().contains("Parakeet TDT 0.6B v3"));
        // A chosen model missing from the list can't be downloaded.
        assert!(
            !resolve_meeting_model_id(&models, Some("gone"), None)
                .unwrap_err()
                .known
        );
    }

    #[test]
    fn run_options_ask_for_words() {
        let o = word_run_options(Some("en".into()));
        assert_eq!(o.timestamps, TimestampKind::Word);
        assert_eq!(o.language.as_deref(), Some("en"));
        assert!(o.vocabulary.is_empty());
    }
}
