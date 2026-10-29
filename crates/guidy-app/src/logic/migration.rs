//! Retired-model migration: replace retired ONNX
//! models with their catalog GGUF successors. Pure state machine; the view
//! runs the resulting actions.

use super::models_store::ModelsStore;
use guidy_core::managers::model::{ModelInfo, ModelSource, RetiredModel};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Pair {
    pub retired: RetiredModel,
    pub replacement: ModelInfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Choose,
    Migrating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowState {
    Idle,
    Ready,
    Failed,
    Progress(f64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Switch to `replacement` first when `select`, then delete `retired`.
    Finalize {
        retired: String,
        replacement: String,
        select: bool,
    },
    /// All picks settled: delete unticked retired models, then finish.
    WrapUp { delete: Vec<String> },
}

fn repo_of(m: &ModelInfo) -> Option<&str> {
    match &m.source {
        ModelSource::HuggingFace { repo_id, .. } => Some(repo_id),
        _ => None,
    }
}

/// Catalog ids carry the filename, so replacements match on repo id.
pub fn find_replacement<'a>(
    models: &'a [ModelInfo],
    retired: &RetiredModel,
) -> Option<&'a ModelInfo> {
    models
        .iter()
        .find(|m| repo_of(m) == Some(retired.replacement_repo.as_str()))
}

#[derive(Clone, Debug)]
pub struct Migration {
    pub in_use: String,
    snapshot: Vec<RetiredModel>,
    pub picked: HashSet<String>,
    pub phase: Phase,
    pub outcomes: HashMap<String, Outcome>,
    finalizing: HashSet<String>,
    wrapping_up: bool,
    /// Download-tracker failures already counted, by replacement id (the
    /// failed item's start time): a retry starts a new item.
    failures_seen: HashMap<String, i64>,
}

impl Migration {
    /// The retired list is read once so rows never vanish mid-flow; the model
    /// in use leads and starts ticked.
    pub fn new(mut snapshot: Vec<RetiredModel>, in_use: &str) -> Self {
        snapshot.sort_by_key(|m| m.id != in_use);
        Self {
            in_use: in_use.to_string(),
            snapshot,
            picked: [in_use.to_string()].into_iter().collect(),
            phase: Phase::Choose,
            outcomes: HashMap::new(),
            finalizing: HashSet::new(),
            wrapping_up: false,
            failures_seen: HashMap::new(),
        }
    }

    pub fn rows(&self, models: &[ModelInfo]) -> Vec<Pair> {
        self.snapshot
            .iter()
            .filter_map(|r| {
                find_replacement(models, r).map(|rep| Pair {
                    retired: r.clone(),
                    replacement: rep.clone(),
                })
            })
            .collect()
    }

    pub fn chosen(&self, models: &[ModelInfo]) -> Vec<Pair> {
        self.rows(models)
            .into_iter()
            .filter(|p| self.picked.contains(&p.retired.id))
            .collect()
    }

    /// The replacement for the model in use if ticked, else the first pick.
    pub fn target_id(&self, models: &[ModelInfo]) -> Option<String> {
        let chosen = self.chosen(models);
        chosen
            .iter()
            .find(|p| p.retired.id == self.in_use)
            .or(chosen.first())
            .map(|p| p.retired.id.clone())
    }

    pub fn to_download(&self, models: &[ModelInfo]) -> Vec<Pair> {
        self.chosen(models)
            .into_iter()
            .filter(|p| !p.replacement.is_downloaded)
            .collect()
    }

    pub fn download_mb(&self, models: &[ModelInfo]) -> f64 {
        self.to_download(models)
            .iter()
            .map(|p| p.replacement.size_mb as f64)
            .sum()
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.picked.remove(id) {
            self.picked.insert(id.to_string());
        }
    }

    fn ready(p: &Pair, store: &ModelsStore) -> bool {
        let id = &p.replacement.id;
        store
            .model(id)
            .map(|m| m.is_downloaded)
            .unwrap_or(p.replacement.is_downloaded)
            && !store.downloading.contains(id)
            && !store.verifying.contains(id)
    }

    pub fn row_state(&self, p: &Pair, store: &ModelsStore) -> RowState {
        if !self.picked.contains(&p.retired.id) || self.phase == Phase::Choose {
            return RowState::Idle;
        }
        match self.outcomes.get(&p.retired.id) {
            Some(Outcome::Done) => return RowState::Ready,
            Some(Outcome::Failed) => return RowState::Failed,
            None => {}
        }
        let id = &p.replacement.id;
        if store.downloading.contains(id) {
            return RowState::Progress(store.progress.get(id).map(|p| p.percentage).unwrap_or(0.0));
        }
        RowState::Progress(100.0)
    }

    /// Start: returns replacement ids to download (all at once). Failures
    /// the tracker shows from before don't count.
    pub fn start(&mut self, models: &[ModelInfo], store: &ModelsStore) -> Vec<String> {
        self.phase = Phase::Migrating;
        let ids: Vec<String> = self
            .to_download(models)
            .into_iter()
            .map(|p| p.replacement.id)
            .collect();
        for id in &ids {
            if let Some(item) = Self::failed_item(id, store) {
                self.failures_seen.insert(id.clone(), item.started_at_ms);
            }
        }
        ids
    }

    fn failed_item<'a>(
        replacement_id: &str,
        store: &'a ModelsStore,
    ) -> Option<&'a guidy_core::downloads::DownloadItem> {
        store
            .stt_download(replacement_id)
            .filter(|d| d.state == guidy_core::downloads::DownloadState::Failed)
    }

    /// Mark rows whose replacement download failed (the download tracker
    /// reports it; queuing it succeeded). Call after any store change.
    pub fn note_failures(&mut self, models: &[ModelInfo], store: &ModelsStore) {
        if self.phase != Phase::Migrating {
            return;
        }
        for p in self.chosen(models) {
            let id = &p.replacement.id;
            let Some(item) = Self::failed_item(id, store) else {
                continue;
            };
            if self.failures_seen.get(id) == Some(&item.started_at_ms) {
                continue;
            }
            self.failures_seen.insert(id.clone(), item.started_at_ms);
            if !self.outcomes.contains_key(&p.retired.id) {
                self.outcomes.insert(p.retired.id.clone(), Outcome::Failed);
            }
        }
    }

    /// A download could not be queued for this replacement.
    pub fn download_failed(&mut self, replacement_id: &str, models: &[ModelInfo]) {
        if let Some(p) = self
            .rows(models)
            .into_iter()
            .find(|p| p.replacement.id == replacement_id)
        {
            self.outcomes.insert(p.retired.id, Outcome::Failed);
        }
    }

    /// Retry a failed row; returns a replacement id to download if needed.
    pub fn retry(
        &mut self,
        retired_id: &str,
        models: &[ModelInfo],
        store: &ModelsStore,
    ) -> Option<String> {
        self.outcomes.remove(retired_id);
        self.finalizing.remove(retired_id);
        let p = self
            .rows(models)
            .into_iter()
            .find(|p| p.retired.id == retired_id)?;
        (!Self::ready(&p, store)).then_some(p.replacement.id)
    }

    /// Next things to do given the store state (call after any change).
    pub fn next_actions(&mut self, models: &[ModelInfo], store: &ModelsStore) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.phase != Phase::Migrating {
            return actions;
        }
        let target = self.target_id(models);
        let chosen = self.chosen(models);
        for p in &chosen {
            let rid = &p.retired.id;
            if self.finalizing.contains(rid)
                || self.outcomes.get(rid) == Some(&Outcome::Failed)
                || !Self::ready(p, store)
            {
                continue;
            }
            self.finalizing.insert(rid.clone());
            actions.push(Action::Finalize {
                retired: rid.clone(),
                replacement: p.replacement.id.clone(),
                select: target.as_deref() == Some(rid.as_str()),
            });
        }
        let target_done = target
            .as_ref()
            .is_none_or(|t| self.outcomes.get(t) == Some(&Outcome::Done));
        let all_settled = chosen
            .iter()
            .all(|p| self.outcomes.contains_key(&p.retired.id));
        if !self.wrapping_up && target_done && all_settled {
            self.wrapping_up = true;
            let delete = self
                .rows(models)
                .into_iter()
                .filter(|p| !self.picked.contains(&p.retired.id))
                .map(|p| p.retired.id)
                .collect();
            actions.push(Action::WrapUp { delete });
        }
        actions
    }

    /// A finalize finished (`ok` = selected (if target) and old deleted).
    pub fn finalized(&mut self, retired_id: &str, ok: bool) {
        if !ok {
            self.finalizing.remove(retired_id);
        }
        self.outcomes.insert(
            retired_id.to_string(),
            if ok { Outcome::Done } else { Outcome::Failed },
        );
    }

    pub fn target_failed(&self, models: &[ModelInfo]) -> bool {
        self.target_id(models)
            .is_some_and(|t| self.outcomes.get(&t) == Some(&Outcome::Failed))
    }

    pub fn downloading(&self, models: &[ModelInfo], store: &ModelsStore) -> bool {
        self.chosen(models).iter().any(|p| {
            matches!(self.row_state(p, store), RowState::Progress(v) if v < 100.0)
                || store.downloading.contains(&p.replacement.id)
        })
    }

    /// (combined MB/s, seconds remaining).
    pub fn eta(&self, models: &[ModelInfo], store: &ModelsStore) -> (f64, f64) {
        let mut speed = 0.0;
        let mut remaining_mb = 0.0;
        for p in self.chosen(models) {
            let RowState::Progress(pct) = self.row_state(&p, store) else {
                continue;
            };
            if pct >= 100.0 {
                continue;
            }
            let id = &p.replacement.id;
            speed += store.stats.get(id).map(|s| s.speed).unwrap_or(0.0);
            remaining_mb += match store.progress.get(id) {
                Some(pr) if pr.total > 0 => (pr.total - pr.downloaded) as f64 / (1024.0 * 1024.0),
                _ => p.replacement.size_mb as f64 * (1.0 - pct / 100.0),
            };
        }
        let secs = if speed > 0.0 {
            remaining_mb / speed
        } else {
            0.0
        };
        (speed, secs)
    }
}

/// "45 sec" / "3 min".
pub fn format_time_left(secs: f64) -> Option<String> {
    if secs <= 0.0 {
        return None;
    }
    Some(if secs < 60.0 {
        format!("{} sec", secs.ceil() as u64)
    } else {
        format!("{} min", (secs / 60.0).ceil() as u64)
    })
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::logic::models::test_model;

    fn retired(id: &str, repo: &str) -> RetiredModel {
        RetiredModel {
            id: id.into(),
            name: id.to_uppercase(),
            size_mb: 400,
            replacement_repo: repo.into(),
        }
    }

    fn setup() -> (Vec<ModelInfo>, Migration) {
        let models = vec![test_model("new-a"), test_model("new-b")];
        // test_model uses repo "org/<id>"
        let m = Migration::new(
            vec![retired("old-b", "org/new-b"), retired("old-a", "org/new-a")],
            "old-a",
        );
        (models, m)
    }

    #[test]
    fn in_use_leads_and_is_ticked() {
        let (models, m) = setup();
        let rows = m.rows(&models);
        assert_eq!(rows[0].retired.id, "old-a");
        assert_eq!(rows[0].replacement.id, "new-a");
        assert_eq!(m.chosen(&models).len(), 1);
        assert_eq!(m.target_id(&models).as_deref(), Some("old-a"));
        assert_eq!(m.download_mb(&models), 500.0);
    }

    #[test]
    fn target_falls_back_to_first_pick() {
        let (models, mut m) = setup();
        m.toggle("old-a");
        assert!(m.target_id(&models).is_none());
        m.toggle("old-b");
        assert_eq!(m.target_id(&models).as_deref(), Some("old-b"));
    }

    #[test]
    fn full_flow_selects_target_before_deleting_and_wraps_up() {
        let (mut models, mut m) = setup();
        let mut store = ModelsStore::default();
        assert_eq!(m.start(&models, &store), vec!["new-a".to_string()]);
        store.begin_download("new-a");
        assert!(m.next_actions(&models, &store).is_empty());
        assert!(matches!(
            m.row_state(&m.rows(&models)[0], &store),
            RowState::Progress(_)
        ));
        // download completes
        store.downloading.clear();
        store.progress.clear();
        models[0].is_downloaded = true;
        store.models = models.clone();
        let actions = m.next_actions(&models, &store);
        assert_eq!(
            actions,
            vec![Action::Finalize {
                retired: "old-a".into(),
                replacement: "new-a".into(),
                select: true
            }]
        );
        assert!(
            m.next_actions(&models, &store).is_empty(),
            "finalize issued once"
        );
        m.finalized("old-a", true);
        let actions = m.next_actions(&models, &store);
        assert_eq!(
            actions,
            vec![Action::WrapUp {
                delete: vec!["old-b".into()]
            }]
        );
        assert!(m.next_actions(&models, &store).is_empty(), "wrap up once");
    }

    #[test]
    fn a_failed_tracker_download_fails_its_row_once() {
        use guidy_core::downloads::{stt_id, DownloadItem, DownloadState};
        let (models, mut m) = setup();
        let mut store = ModelsStore::default();
        // A failure from before the start doesn't count.
        let mut old = DownloadItem::new(stt_id("new-a"), "New A", 10, DownloadState::Failed);
        old.started_at_ms = 1;
        store.downloads = vec![old];
        m.start(&models, &store);
        m.note_failures(&models, &store);
        assert!(!m.target_failed(&models));
        // The new attempt fails.
        let mut failed = DownloadItem::new(stt_id("new-a"), "New A", 10, DownloadState::Failed);
        failed.started_at_ms = 2;
        store.downloads = vec![failed];
        m.note_failures(&models, &store);
        assert!(m.target_failed(&models));
        // Retry clears it; the same failed item isn't counted again.
        m.retry("old-a", &models, &store);
        m.note_failures(&models, &store);
        assert!(!m.target_failed(&models));
    }

    #[test]
    fn failed_target_can_retry() {
        let (mut models, mut m) = setup();
        models[0].is_downloaded = true;
        let mut store = ModelsStore::default();
        store.models = models.clone();
        m.start(&models, &store);
        m.next_actions(&models, &store);
        m.finalized("old-a", false);
        assert!(m.target_failed(&models));
        assert_eq!(m.row_state(&m.rows(&models)[0], &store), RowState::Failed);
        // ready replacement: retry only re-finalizes
        assert_eq!(m.retry("old-a", &models, &store), None);
        assert_eq!(m.next_actions(&models, &store).len(), 1);
    }

    #[test]
    fn eta_and_time_format() {
        let (models, mut m) = setup();
        let mut store = ModelsStore::default();
        m.start(&models, &store);
        store.begin_download("new-a");
        store.progress.get_mut("new-a").unwrap().percentage = 50.0;
        store.stats.insert(
            "new-a".into(),
            crate::logic::models::DownloadStats {
                start_ms: 0,
                last_update_ms: 0,
                total_downloaded: 0,
                speed: 5.0,
            },
        );
        let (speed, secs) = m.eta(&models, &store);
        assert_eq!(speed, 5.0);
        assert!((secs - 50.0).abs() < 1e-9, "250 MB left at 5 MB/s");
        assert_eq!(format_time_left(50.0).as_deref(), Some("50 sec"));
        assert_eq!(format_time_left(130.0).as_deref(), Some("3 min"));
        assert_eq!(format_time_left(0.0), None);
    }
}
