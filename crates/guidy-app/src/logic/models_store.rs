//! Model list state machine.
//! Pure: events in, effects out; the GPUI layer runs the effects.

use super::models::{update_download_stats, DownloadStats};
use guidy_core::managers::model::{DownloadProgress, ModelInfo};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelsEffect {
    ReloadModels,
    ReloadCurrentModel,
    ToastError(String),
    /// A download finished; the selector may auto-select it (500 ms later,
    /// when not recording).
    DownloadCompleted(String),
}

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
    pub error: Option<String>,
}

impl ModelsStore {
    pub fn model(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == id)
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
            "model-deleted" | "model-state-changed" | "models-updated" => {
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
        for name in ["model-deleted", "model-state-changed", "models-updated"] {
            assert_eq!(
                s.apply_event(name, &Value::Null, 0),
                vec![ModelsEffect::ReloadModels, ModelsEffect::ReloadCurrentModel]
            );
        }
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
