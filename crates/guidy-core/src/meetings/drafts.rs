//! Autosaved drafts of typed meeting notes (scorecard R18): unsaved edits to
//! the summary, the action items and the new-item field survive a crash.
//!
//! A draft is one JSON file per meeting in `<meetings_dir>/drafts/`, separate
//! from `meetings.db` (no migration). Save keeps its meaning: a draft is never
//! part of the meeting's notes until the user presses Save.
//!
//! [`DraftDebouncer`] decides when to write and is pure (the caller passes the
//! time). [`DraftStore`] does the file work and must be called off the UI
//! thread. Each file is replaced by a rename, so `kill -9` leaves either the
//! previous draft or the new one, never half of one.

use crate::meetings::types::{ActionItemId, MeetingId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Write this long after the last keystroke.
pub const DRAFT_QUIET: Duration = Duration::from_millis(400);
/// During continuous typing, write this long after the first unsaved
/// keystroke: a trailing debounce alone would never fire. Below 1 s so that
/// the write itself fits inside the R18 bound.
pub const DRAFT_MAX_WAIT: Duration = Duration::from_millis(800);

/// The summary editor's fields, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryDraft {
    pub title: String,
    pub short_summary: String,
    pub decisions: String,
    pub topics: String,
    /// `generated_at` of the summary these edits started from: a draft of an
    /// older generation doesn't get to overwrite a newer one.
    pub based_on_generated_at: i64,
}

impl SummaryDraft {
    /// The fields hold what `current` already stores, once Save's trimming
    /// is applied: it was saved, and only the clearing didn't happen.
    pub fn matches_saved(&self, current: &SummaryDraft) -> bool {
        fn lines(text: &str) -> Vec<&str> {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect()
        }
        self.title.trim() == current.title.trim()
            && self.short_summary.trim() == current.short_summary.trim()
            && lines(&self.decisions) == lines(&current.decisions)
            && lines(&self.topics) == lines(&current.topics)
    }
}

/// An action item's fields, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemDraft {
    pub text: String,
    pub due: String,
}

/// Everything typed in a meeting's notes and not yet saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingDraft {
    pub summary: Option<SummaryDraft>,
    pub items: BTreeMap<ActionItemId, ItemDraft>,
    pub new_item: String,
}

impl MeetingDraft {
    pub fn is_empty(&self) -> bool {
        self.summary.is_none() && self.items.is_empty() && self.new_item.is_empty()
    }
}

/// When to write the draft: [`DRAFT_QUIET`] after the last change, or
/// [`DRAFT_MAX_WAIT`] after the first unwritten one, whichever is sooner.
#[derive(Clone, Copy, Debug, Default)]
pub struct DraftDebouncer {
    first_change: Option<Instant>,
    last_change: Option<Instant>,
}

impl DraftDebouncer {
    pub fn changed(&mut self, now: Instant) {
        self.first_change.get_or_insert(now);
        self.last_change = Some(now);
    }

    /// There are changes not yet written.
    pub fn is_dirty(&self) -> bool {
        self.first_change.is_some()
    }

    /// When the pending changes are due, if there are any.
    pub fn deadline(&self) -> Option<Instant> {
        let first = self.first_change?;
        let last = self.last_change?;
        Some((last + DRAFT_QUIET).min(first + DRAFT_MAX_WAIT))
    }

    /// True once when the pending changes are due: write them now.
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.deadline().is_some_and(|d| d <= now) {
            self.reset();
            true
        } else {
            false
        }
    }

    /// Forget the pending changes (the caller is writing them now).
    pub fn reset(&mut self) {
        self.first_change = None;
        self.last_change = None;
    }
}

/// The next write's order. Writes run on a thread pool and can finish out of
/// order; [`DraftStore`] drops one older than what it already applied, so a
/// slow autosave can't bring back a draft that Save or Cancel just cleared.
pub fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Draft files under `<meetings_dir>/drafts/`.
pub struct DraftStore {
    dir: PathBuf,
    /// The newest revision applied per meeting. Holding this lock serializes
    /// the file work. Only this process writes, so it isn't persisted.
    applied: parking_lot::Mutex<HashMap<MeetingId, u64>>,
    /// Drafts announced by [`Self::hold`] and not yet written. Never held
    /// across file work, so the UI thread may take it.
    held: parking_lot::Mutex<HashMap<MeetingId, (u64, MeetingDraft)>>,
}

impl DraftStore {
    pub fn new(meetings_dir: &Path) -> Self {
        Self {
            dir: meetings_dir.join("drafts"),
            applied: Default::default(),
            held: Default::default(),
        }
    }

    fn path(&self, id: MeetingId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Announce a write about to start, for a view closing or switching away
    /// with text not yet written: [`Self::load`] sees it at once, so reopening
    /// the meeting can't read the file before the write lands. No file work:
    /// safe on the UI thread.
    pub fn hold(&self, id: MeetingId, revision: u64, draft: MeetingDraft) {
        self.held.lock().insert(id, (revision, draft));
    }

    /// Replace `id`'s draft (an empty draft removes it). Returns false if a
    /// newer revision was already applied.
    pub fn save(&self, id: MeetingId, revision: u64, draft: &MeetingDraft) -> anyhow::Result<bool> {
        let mut applied = self.applied.lock();
        if applied.get(&id).is_some_and(|newest| *newest > revision) {
            return Ok(false);
        }
        let tmp = self.dir.join(format!("{id}.json.tmp"));
        if draft.is_empty() {
            // A kill before the rename can leave typed text in the temp file.
            remove_if_present(&tmp)?;
            remove_if_present(&self.path(id))?;
        } else {
            std::fs::create_dir_all(&self.dir)?;
            std::fs::write(&tmp, serde_json::to_vec(draft)?)?;
            std::fs::rename(&tmp, self.path(id))?;
        }
        applied.insert(id, revision);
        let mut held = self.held.lock();
        if held.get(&id).is_some_and(|(r, _)| *r <= revision) {
            held.remove(&id);
        }
        Ok(true)
    }

    /// `id`'s draft. One that can't be read or parsed is treated as absent
    /// and logged: the next save replaces it.
    pub fn load(&self, id: MeetingId) -> Option<MeetingDraft> {
        if let Some((_, draft)) = self.held.lock().get(&id) {
            return (!draft.is_empty()).then(|| draft.clone());
        }
        let _serialized = self.applied.lock();
        let bytes = match std::fs::read(self.path(id)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                log::warn!("meetings: reading the draft of {id} failed: {e}");
                return None;
            }
        };
        match serde_json::from_slice::<MeetingDraft>(&bytes) {
            Ok(draft) => (!draft.is_empty()).then_some(draft),
            Err(e) => {
                log::warn!("meetings: the draft of {id} is unreadable: {e}");
                None
            }
        }
    }
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn writes_after_a_quiet_pause() {
        let t = Instant::now();
        let mut d = DraftDebouncer::default();
        assert_eq!(d.deadline(), None);
        assert!(!d.take_due(t));
        d.changed(t);
        assert!(d.is_dirty());
        assert!(!d.take_due(at(t, 399)));
        assert!(d.take_due(at(t, 400)));
        // Once per batch of changes.
        assert!(!d.is_dirty());
        assert!(!d.take_due(at(t, 5000)));
    }

    #[test]
    fn continuous_typing_still_writes_within_a_second() {
        let t = Instant::now();
        let mut d = DraftDebouncer::default();
        let mut wrote_at = Vec::new();
        // A keystroke every 100 ms for 5 s, polled every 10 ms.
        for ms in (0..5000u64).step_by(10) {
            if ms % 100 == 0 {
                d.changed(at(t, ms));
            }
            if d.take_due(at(t, ms)) {
                wrote_at.push(ms);
            }
        }
        assert!(wrote_at.len() >= 5, "{wrote_at:?}");
        assert!(wrote_at[0] <= 1000);
        for pair in wrote_at.windows(2) {
            assert!(pair[1] - pair[0] <= 1000, "{wrote_at:?}");
        }
    }

    #[test]
    fn a_new_keystroke_after_the_write_starts_a_new_window() {
        let t = Instant::now();
        let mut d = DraftDebouncer::default();
        d.changed(t);
        assert!(d.take_due(at(t, 400)));
        d.changed(at(t, 900));
        assert_eq!(d.deadline(), Some(at(t, 1300)));
    }

    #[test]
    fn reset_drops_the_pending_write() {
        let t = Instant::now();
        let mut d = DraftDebouncer::default();
        d.changed(t);
        d.reset();
        assert!(!d.take_due(at(t, 5000)));
    }

    fn draft() -> MeetingDraft {
        MeetingDraft {
            summary: Some(SummaryDraft {
                title: "T\u{e9}st \"title\"".into(),
                short_summary: "Line one\n\n  two ".into(),
                decisions: "A\nB".into(),
                topics: String::new(),
                based_on_generated_at: 3,
            }),
            items: BTreeMap::from([(
                7,
                ItemDraft {
                    text: "send".into(),
                    due: "Friday".into(),
                },
            )]),
            new_item: "call Sam".into(),
        }
    }

    #[test]
    fn a_draft_round_trips_through_a_new_store() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(DraftStore::new(dir.path()).load(1), None);
        assert!(DraftStore::new(dir.path()).save(1, 1, &draft()).unwrap());
        // A new store is the relaunch after a crash.
        assert_eq!(DraftStore::new(dir.path()).load(1), Some(draft()));
        assert_eq!(DraftStore::new(dir.path()).load(2), None);
    }

    #[test]
    fn an_empty_draft_clears_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        // Nothing to remove yet: not an error.
        assert!(store.save(1, 1, &MeetingDraft::default()).unwrap());
        store.save(1, 2, &draft()).unwrap();
        store.save(1, 3, &MeetingDraft::default()).unwrap();
        assert_eq!(store.load(1), None);
        assert!(!dir.path().join("drafts").join("1.json").exists());
    }

    #[test]
    fn a_late_older_write_cannot_undo_a_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        store.save(1, 5, &draft()).unwrap();
        store.save(1, 7, &MeetingDraft::default()).unwrap();
        assert!(!store.save(1, 6, &draft()).unwrap());
        assert_eq!(store.load(1), None);
        // Another meeting's order is its own.
        assert!(store.save(2, 1, &draft()).unwrap());
    }

    #[test]
    fn a_torn_or_foreign_file_reads_as_no_draft_and_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        store.save(1, 1, &draft()).unwrap();
        std::fs::write(dir.path().join("drafts").join("1.json"), b"{\"summ").unwrap();
        assert_eq!(store.load(1), None);
        store.save(1, 2, &draft()).unwrap();
        assert_eq!(store.load(1), Some(draft()));
    }

    #[test]
    fn a_leftover_temp_file_does_not_hide_the_last_draft() {
        // A kill between the write and the rename.
        let dir = tempfile::tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        store.save(1, 1, &draft()).unwrap();
        std::fs::write(dir.path().join("drafts").join("1.json.tmp"), b"{\"su").unwrap();
        let relaunched = DraftStore::new(dir.path());
        assert_eq!(relaunched.load(1), Some(draft()));
        relaunched.save(1, 1, &MeetingDraft::default()).unwrap();
        assert_eq!(relaunched.load(1), None);
        // Discarding also removes the typed text the temp file held.
        assert!(!dir.path().join("drafts").join("1.json.tmp").exists());
    }

    #[test]
    fn a_held_draft_is_visible_before_its_write_lands() {
        let dir = tempfile::tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        store.hold(1, 4, draft());
        assert_eq!(store.load(1), Some(draft()));
        // An older write finishing doesn't release a newer hold.
        store.save(1, 3, &MeetingDraft::default()).unwrap();
        assert_eq!(store.load(1), Some(draft()));
        store.save(1, 4, &draft()).unwrap();
        assert!(store.held.lock().is_empty());
        assert_eq!(store.load(1), Some(draft()));
    }

    #[test]
    fn matches_saved_applies_saves_trimming() {
        let saved = SummaryDraft {
            title: "T".into(),
            short_summary: "S".into(),
            decisions: "A\nB".into(),
            topics: "C".into(),
            based_on_generated_at: 3,
        };
        let typed = SummaryDraft {
            title: " T ".into(),
            short_summary: "S\n".into(),
            decisions: "A\n\n B \n".into(),
            ..saved.clone()
        };
        assert!(typed.matches_saved(&saved));
        let changed = SummaryDraft {
            title: "Other".into(),
            ..saved.clone()
        };
        assert!(!changed.matches_saved(&saved));
    }
}
