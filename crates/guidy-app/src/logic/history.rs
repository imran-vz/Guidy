//! History list behavior (Handy's `HistorySettings`): cursor pages, live
//! updates, optimistic toggles/deletes.

use guidy_core::managers::history::{HistoryEntry, HistoryUpdatePayload};
use std::collections::HashSet;

pub const PAGE_SIZE: usize = 30;

#[derive(Clone, Debug, Default)]
pub struct HistoryList {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
    pub loading: bool,
    pub loading_more: bool,
}

impl HistoryList {
    /// Merge a fetched page (first page replaces; later pages append new ids).
    pub fn apply_page(&mut self, first: bool, entries: Vec<HistoryEntry>, has_more: bool) {
        if first {
            self.entries = entries;
        } else {
            let existing: HashSet<i64> = self.entries.iter().map(|e| e.id).collect();
            self.entries
                .extend(entries.into_iter().filter(|e| !existing.contains(&e.id)));
        }
        self.has_more = has_more;
        self.loading = false;
        self.loading_more = false;
    }

    /// Cursor for the next page (the last entry's id).
    pub fn next_cursor(&self) -> Option<i64> {
        self.entries.last().map(|e| e.id)
    }

    /// `added` prepends (deduped), `updated` replaces in place; `deleted` /
    /// `toggled` are already applied optimistically and ignored here.
    pub fn apply_event(&mut self, payload: &HistoryUpdatePayload) {
        match payload {
            HistoryUpdatePayload::Added { entry } => {
                self.entries.retain(|e| e.id != entry.id);
                self.entries.insert(0, entry.clone());
            }
            HistoryUpdatePayload::Updated { entry } => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == entry.id) {
                    *e = entry.clone();
                }
            }
            HistoryUpdatePayload::Deleted { .. } | HistoryUpdatePayload::Toggled { .. } => {}
        }
    }

    pub fn toggle_saved(&mut self, id: i64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.saved = !e.saved;
        }
    }

    pub fn remove(&mut self, id: i64) {
        self.entries.retain(|e| e.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, text: &str) -> HistoryEntry {
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

    #[test]
    fn pages_append_without_duplicates() {
        let mut l = HistoryList::default();
        l.apply_page(true, vec![entry(3, "c"), entry(2, "b")], true);
        assert_eq!(l.next_cursor(), Some(2));
        l.apply_page(false, vec![entry(2, "b"), entry(1, "a")], false);
        assert_eq!(
            l.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        assert!(!l.has_more);
        l.apply_page(true, vec![entry(9, "z")], false);
        assert_eq!(l.entries.len(), 1, "first page replaces");
    }

    #[test]
    fn live_events_prepend_and_replace() {
        let mut l = HistoryList::default();
        l.apply_page(true, vec![entry(1, "a")], false);
        l.apply_event(&HistoryUpdatePayload::Added {
            entry: entry(2, "b"),
        });
        l.apply_event(&HistoryUpdatePayload::Added {
            entry: entry(2, "b"),
        });
        assert_eq!(l.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [2, 1]);
        l.apply_event(&HistoryUpdatePayload::Updated {
            entry: entry(1, "fixed"),
        });
        assert_eq!(l.entries[1].transcription_text, "fixed");
        l.apply_event(&HistoryUpdatePayload::Deleted { id: 1 });
        assert_eq!(l.entries.len(), 2, "deletes are optimistic only");
    }

    #[test]
    fn optimistic_toggle_and_remove() {
        let mut l = HistoryList::default();
        l.apply_page(true, vec![entry(1, "a")], false);
        l.toggle_saved(1);
        assert!(l.entries[0].saved);
        l.toggle_saved(1);
        assert!(!l.entries[0].saved, "revert by toggling back");
        l.remove(1);
        assert!(l.entries.is_empty());
    }
}
