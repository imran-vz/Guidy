//! Model list state machine.
//! Pure: events in, effects out; the GPUI layer runs the effects.

use super::models::{update_download_stats, DownloadStats};
use guidy_core::downloads::{
    DiskUsage, DownloadItem, DownloadKind, DownloadState, OtherModel, DOWNLOADS_CHANGED_EVENT,
};
use guidy_core::managers::model::{DownloadProgress, ModelInfo};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelsEffect {
    ReloadModels,
    ReloadCurrentModel,
    /// Reload the LLM and speaker models (`Backend::other_models`).
    ReloadOther,
    /// Re-measure what models take on disk.
    ReloadDisk,
    ToastError(String),
    /// A speech model download finished: select it if it was picked for
    /// dictation, else offer "Use for Dictation".
    DownloadCompleted(String),
    /// Another kind of model finished downloading.
    DownloadDone {
        name: String,
    },
    /// A download of another kind failed (speech models toast through
    /// `model-download-failed`).
    DownloadFailed {
        name: String,
        error: Option<String>,
    },
}

/// Speech-to-text models (`ModelManager`) plus the unified download list,
/// the LLM and speaker models, and what the requirements plan needs about
/// this Mac.
#[derive(Clone, Debug, Default)]
pub struct ModelsStore {
    pub models: Vec<ModelInfo>,
    pub current_model: String,
    pub downloading: HashSet<String>,
    pub verifying: HashSet<String>,
    pub progress: HashMap<String, DownloadProgress>,
    pub stats: HashMap<String, DownloadStats>,
    pub loading: bool,
    pub initialized: bool,
    pub rescanning: bool,
    /// The last model operation's error (shown on the Models page).
    pub error: Option<String>,
    /// The download tracker's list (every kind).
    pub downloads: Vec<DownloadItem>,
    /// LLMs and speaker models.
    pub other: Vec<OtherModel>,
    pub other_loaded: bool,
    pub disk: Option<DiskUsage>,
    pub ram_gb: Option<u32>,
    pub apple_intelligence: bool,
    pub providers_with_keys: Vec<String>,
}

impl ModelsStore {
    pub fn model(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The tracker's entry for a download id.
    pub fn download(&self, id: &str) -> Option<&DownloadItem> {
        self.downloads.iter().find(|d| d.id == id)
    }

    /// The tracker's entry for a speech model, when active.
    pub fn stt_download(&self, model_id: &str) -> Option<&DownloadItem> {
        self.download(&guidy_core::downloads::stt_id(model_id))
    }

    pub fn other_model(&self, id: &str) -> Option<&OtherModel> {
        self.other.iter().find(|m| m.id == id)
    }

    /// A new download list: effects for the downloads that just settled.
    pub fn set_downloads(&mut self, items: Vec<DownloadItem>) -> Vec<ModelsEffect> {
        let mut fx = Vec::new();
        for item in &items {
            let prev = self
                .downloads
                .iter()
                .find(|d| d.id == item.id)
                .map(|d| d.state);
            if prev == Some(item.state) || !prev.is_some_and(DownloadState::is_active) {
                continue;
            }
            let stt = item.kind == DownloadKind::Stt;
            match item.state {
                DownloadState::Done if !stt => {
                    fx.push(ModelsEffect::DownloadDone {
                        name: item.name.clone(),
                    });
                    fx.push(ModelsEffect::ReloadOther);
                    fx.push(ModelsEffect::ReloadDisk);
                }
                DownloadState::Failed if !stt => {
                    fx.push(ModelsEffect::DownloadFailed {
                        name: item.name.clone(),
                        error: item.error.clone(),
                    });
                    fx.push(ModelsEffect::ReloadOther);
                }
                DownloadState::Cancelled | DownloadState::Failed if stt => {
                    // The partial's size changed.
                    fx.push(ModelsEffect::ReloadModels);
                }
                DownloadState::Cancelled => fx.push(ModelsEffect::ReloadOther),
                _ => {}
            }
        }
        self.downloads = items;
        fx.dedup();
        fx
    }

    fn clear_download(&mut self, id: &str) {
        self.downloading.remove(id);
        self.verifying.remove(id);
        self.progress.remove(id);
        self.stats.remove(id);
    }

    /// `loadModels` result: merge backend downloading flags with ours.
    pub fn set_models(&mut self, models: Vec<ModelInfo>) {
        let backend_downloading: HashSet<String> = models
            .iter()
            .filter(|m| m.is_downloading)
            .map(|m| m.id.clone())
            .collect();
        for id in &backend_downloading {
            self.downloading.insert(id.clone());
        }
        let progress = &self.progress;
        self.downloading
            .retain(|id| backend_downloading.contains(id) || progress.contains_key(id));
        self.models = models;
        self.loading = false;
    }

    /// Optimistic start of a download (before the command returns).
    pub fn begin_download(&mut self, id: &str) {
        self.error = None;
        self.downloading.insert(id.to_string());
        self.progress.insert(
            id.to_string(),
            DownloadProgress {
                model_id: id.to_string(),
                downloaded: 0,
                total: 0,
                percentage: 0.0,
            },
        );
    }

    /// The download command failed (or the failed event never arrived).
    pub fn download_command_failed(&mut self, id: &str) {
        self.downloading.remove(id);
        self.progress.remove(id);
        self.stats.remove(id);
    }

    pub fn cancelled_by_user(&mut self, id: &str) -> Vec<ModelsEffect> {
        self.downloading.remove(id);
        self.progress.remove(id);
        self.stats.remove(id);
        vec![ModelsEffect::ReloadModels]
    }

    pub fn apply_event(&mut self, name: &str, payload: &Value, now_ms: u64) -> Vec<ModelsEffect> {
        let id_payload = || payload.as_str().map(str::to_string);
        match name {
            DOWNLOADS_CHANGED_EVENT => {
                match serde_json::from_value::<Vec<DownloadItem>>(payload.clone()) {
                    Ok(items) => self.set_downloads(items),
                    Err(_) => vec![],
                }
            }
            "llm-models://changed" | "meetings://speaker-model-status" => {
                vec![ModelsEffect::ReloadOther]
            }
            "model-download-progress" => {
                if let Ok(p) = serde_json::from_value::<DownloadProgress>(payload.clone()) {
                    let stats = update_download_stats(
                        self.stats.get(&p.model_id).copied(),
                        now_ms,
                        p.downloaded,
                    );
                    self.stats.insert(p.model_id.clone(), stats);
                    self.progress.insert(p.model_id.clone(), p);
                }
                vec![]
            }
            "model-download-complete" => {
                let Some(id) = id_payload() else {
                    return vec![];
                };
                self.clear_download(&id);
                vec![
                    ModelsEffect::ReloadModels,
                    ModelsEffect::ReloadDisk,
                    ModelsEffect::DownloadCompleted(id),
                ]
            }
            "model-download-failed" => {
                let id = payload["model_id"].as_str().unwrap_or_default().to_string();
                let error = payload["error"].as_str().unwrap_or_default().to_string();
                self.clear_download(&id);
                self.error = Some(error.clone());
                vec![ModelsEffect::ToastError(error)]
            }
            "model-verification-started" => {
                if let Some(id) = id_payload() {
                    self.verifying.insert(id);
                }
                vec![]
            }
            "model-verification-completed" => {
                if let Some(id) = id_payload() {
                    self.verifying.remove(&id);
                }
                vec![]
            }
            "model-download-cancelled" => {
                if let Some(id) = id_payload() {
                    self.clear_download(&id);
                }
                vec![]
            }
            "model-deleted" | "models-updated" => vec![
                ModelsEffect::ReloadModels,
                ModelsEffect::ReloadCurrentModel,
                ModelsEffect::ReloadOther,
                ModelsEffect::ReloadDisk,
            ],
            "model-state-changed" => {
                vec![ModelsEffect::ReloadModels, ModelsEffect::ReloadCurrentModel]
            }
            _ => vec![],
        }
    }

    pub fn downloaded_models(&self) -> Vec<&ModelInfo> {
        self.models.iter().filter(|m| m.is_downloaded).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::models::test_model;
    use serde_json::json;

    #[test]
    fn progress_tracks_percentage_and_speed() {
        let mut s = ModelsStore::default();
        s.begin_download("m");
        let p = |d: u64| json!({"model_id": "m", "downloaded": d, "total": 100 << 20, "percentage": d as f64 / (100u64 << 20) as f64 * 100.0});
        s.apply_event("model-download-progress", &p(0), 0);
        s.apply_event("model-download-progress", &p(10 << 20), 1000);
        assert!((s.stats["m"].speed - 10.0).abs() < 1e-9);
        assert!((s.progress["m"].percentage - 10.0).abs() < 1e-9);
        assert!(s.downloading.contains("m"));
    }

    #[test]
    fn completion_clears_state_and_requests_reload_and_autoselect() {
        let mut s = ModelsStore::default();
        s.begin_download("m");
        s.verifying.insert("m".into());
        let fx = s.apply_event("model-download-complete", &json!("m"), 0);
        assert!(s.downloading.is_empty() && s.verifying.is_empty() && s.progress.is_empty());
        assert_eq!(
            fx,
            vec![
                ModelsEffect::ReloadModels,
                ModelsEffect::ReloadDisk,
                ModelsEffect::DownloadCompleted("m".into())
            ]
        );
    }

    #[test]
    fn failure_toasts_the_error() {
        let mut s = ModelsStore::default();
        s.begin_download("m");
        let fx = s.apply_event(
            "model-download-failed",
            &json!({"model_id": "m", "error": "disk full"}),
            0,
        );
        assert_eq!(fx, vec![ModelsEffect::ToastError("disk full".into())]);
        assert_eq!(s.error.as_deref(), Some("disk full"));
        assert!(!s.downloading.contains("m"));
    }

    #[test]
    fn verification_toggles() {
        let mut s = ModelsStore::default();
        s.apply_event("model-verification-started", &json!("m"), 0);
        assert!(s.verifying.contains("m"));
        s.apply_event("model-verification-completed", &json!("m"), 0);
        assert!(!s.verifying.contains("m"));
    }

    #[test]
    fn reload_events() {
        let mut s = ModelsStore::default();
        assert_eq!(
            s.apply_event("model-state-changed", &Value::Null, 0),
            vec![ModelsEffect::ReloadModels, ModelsEffect::ReloadCurrentModel]
        );
        for name in ["model-deleted", "models-updated"] {
            let fx = s.apply_event(name, &Value::Null, 0);
            assert!(fx.contains(&ModelsEffect::ReloadOther));
            assert!(fx.contains(&ModelsEffect::ReloadDisk));
        }
        assert_eq!(
            s.apply_event("llm-models://changed", &Value::Null, 0),
            vec![ModelsEffect::ReloadOther]
        );
        assert!(s.apply_event("unrelated", &Value::Null, 0).is_empty());
    }

    #[test]
    fn set_models_merges_downloading_flags() {
        let mut s = ModelsStore::default();
        s.downloading.insert("stale".into());
        s.begin_download("ours");
        let mut backend = test_model("backend");
        backend.is_downloading = true;
        s.set_models(vec![backend, test_model("ours"), test_model("stale")]);
        assert!(s.downloading.contains("backend"));
        assert!(
            s.downloading.contains("ours"),
            "kept: we have progress for it"
        );
        assert!(
            !s.downloading.contains("stale"),
            "dropped: backend idle, no progress"
        );
    }

    #[test]
    fn tracker_transitions_toast_other_kinds_once() {
        use guidy_core::downloads::{DownloadItem, DownloadState};
        let mut s = ModelsStore::default();
        let item = |id: &str, state| DownloadItem::new(id, id.to_uppercase(), 10, state);
        let list = |items: Vec<DownloadItem>| serde_json::to_value(items).unwrap();
        let fx = s.apply_event(
            DOWNLOADS_CHANGED_EVENT,
            &list(vec![
                item("llm:q", DownloadState::Downloading),
                item("stt:m", DownloadState::Downloading),
            ]),
            0,
        );
        assert!(fx.is_empty());
        assert!(s.download("llm:q").is_some());
        assert!(s.stt_download("m").is_some());
        let mut failed = item("speaker:resnet34", DownloadState::Failed);
        failed.error = Some("offline".into());
        // Unknown before: settled items arriving first don't toast.
        let fx = s.apply_event(
            DOWNLOADS_CHANGED_EVENT,
            &list(vec![
                item("llm:q", DownloadState::Done),
                item("stt:m", DownloadState::Done),
                failed.clone(),
            ]),
            0,
        );
        assert_eq!(
            fx,
            vec![
                ModelsEffect::DownloadDone {
                    name: "LLM:Q".into()
                },
                ModelsEffect::ReloadOther,
                ModelsEffect::ReloadDisk,
            ]
        );
        // Same state again: nothing.
        let fx = s.apply_event(
            DOWNLOADS_CHANGED_EVENT,
            &list(vec![item("llm:q", DownloadState::Done)]),
            0,
        );
        assert!(fx.is_empty());
        let mut running = failed.clone();
        running.state = DownloadState::Downloading;
        s.set_downloads(vec![running]);
        let fx = s.set_downloads(vec![failed]);
        assert_eq!(
            fx[0],
            ModelsEffect::DownloadFailed {
                name: "SPEAKER:RESNET34".into(),
                error: Some("offline".into())
            }
        );
    }

    #[test]
    fn cancel_and_command_failure_cleanup() {
        let mut s = ModelsStore::default();
        s.begin_download("m");
        assert_eq!(s.cancelled_by_user("m"), vec![ModelsEffect::ReloadModels]);
        assert!(s.downloading.is_empty());
        s.begin_download("n");
        s.download_command_failed("n");
        assert!(s.progress.is_empty());
    }
}
