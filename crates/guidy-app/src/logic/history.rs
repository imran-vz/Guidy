//! History list behavior: cursor pages, live
//! updates, optimistic toggles/deletes.

use guidy_core::managers::history::{HistoryEntry, HistoryUpdatePayload};
use std::collections::HashSet;

pub const PAGE_SIZE: usize = 30;

/// The largest history limit the field accepts.
pub const HISTORY_LIMIT_MAX: usize = 10_000;

/// The history limit typed in the field: a whole number from 1 to
/// [`HISTORY_LIMIT_MAX`], or `None`.
pub fn parse_history_limit(text: &str) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=HISTORY_LIMIT_MAX).contains(n))
}

/// How many transcriptions lowering the limit to `limit` deletes, given how
/// many unsaved ones there are (saved ones are never pruned).
pub fn pruned_by_limit(unsaved: usize, limit: usize) -> usize {
    unsaved.saturating_sub(limit)
}

#[derive(Clone, Debug, Default)]
pub struct HistoryList {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
    pub loading: bool,
    pub loading_more: bool,
    /// Why the last page failed to load (shown with Try Again).
    pub error: Option<String>,
}

/// The first non-empty line of a transcription, for list rows.
pub fn preview_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// Whether `entry` shows under the search `query` (case-insensitive, in the
/// transcription or its post-processed text) and the Saved filter.
pub fn matches_filter(entry: &HistoryEntry, query: &str, saved_only: bool) -> bool {
    if saved_only && !entry.saved {
        return false;
    }
    let q = query.trim().to_lowercase();
    q.is_empty()
        || entry.transcription_text.to_lowercase().contains(&q)
        || entry
            .post_processed_text
            .as_ref()
            .is_some_and(|t| t.to_lowercase().contains(&q))
}

/// The splice that turns `old` into `new` (ids in display order): the range
/// of `old` to replace and how many items replace it. Common prefixes and
/// suffixes are kept, so appending a page or deleting one row keeps the
/// scroll position.
pub fn list_splice(old: &[i64], new: &[i64]) -> (std::ops::Range<usize>, usize) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (prefix..old.len() - suffix, new.len() - prefix - suffix)
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
        self.error = None;
    }

    /// A page failed to load.
    pub fn fail(&mut self, error: String) {
        self.loading = false;
        self.loading_more = false;
        self.error = Some(error);
    }

    /// Put back an entry removed by [`HistoryList::remove`] (undo), in
    /// newest-first order.
    pub fn restore(&mut self, entry: HistoryEntry) {
        if self.entries.iter().any(|e| e.id == entry.id) {
            return;
        }
        let at = self
            .entries
            .iter()
            .position(|e| (e.timestamp, e.id) < (entry.timestamp, entry.id))
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
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
    fn history_limit_accepts_whole_numbers_in_range() {
        assert_eq!(parse_history_limit(" 100 "), Some(100));
        assert_eq!(parse_history_limit("1"), Some(1));
        assert_eq!(parse_history_limit("0"), None);
        assert_eq!(parse_history_limit(""), None);
        assert_eq!(parse_history_limit("-3"), None);
        assert_eq!(parse_history_limit("1.5"), None);
        assert_eq!(parse_history_limit("10001"), None);
        assert_eq!(pruned_by_limit(50, 2), 48);
        assert_eq!(pruned_by_limit(3, 10), 0);
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
    fn filter_preview_and_restore() {
        let mut e = entry(1, "\n  Hello there\nsecond");
        assert_eq!(preview_line(&e.transcription_text), "Hello there");
        assert!(matches_filter(&e, "HELLO", false));
        assert!(!matches_filter(&e, "nope", false));
        assert!(!matches_filter(&e, "", true));
        e.saved = true;
        assert!(matches_filter(&e, " ", true));
        let mut l = HistoryList::default();
        l.apply_page(
            true,
            vec![entry(3, "c"), entry(2, "b"), entry(1, "a")],
            false,
        );
        let two = l.entries[1].clone();
        l.remove(2);
        l.restore(two.clone());
        l.restore(two);
        assert_eq!(
            l.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        l.fail("disk".into());
        assert_eq!(l.error.as_deref(), Some("disk"));
    }

    #[test]
    fn splices_keep_common_ends() {
        assert_eq!(list_splice(&[3, 2], &[3, 2, 1]), (2..2, 1));
        assert_eq!(list_splice(&[3, 2, 1], &[3, 1]), (1..2, 0));
        assert_eq!(list_splice(&[3, 2], &[4, 3, 2]), (0..0, 1));
        assert_eq!(list_splice(&[3, 2], &[3, 2]), (2..2, 0));
        assert_eq!(list_splice(&[1, 2], &[5]), (0..2, 1));
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
