//! One place every model download reports to.
//!
//! A download has an id, a display name, byte counts and a state. Changes
//! are emitted as [`DOWNLOADS_CHANGED_EVENT`] (the full list) so the status
//! bar can show one indicator for all of them. Cancel is a flag the
//! downloader polls; the tracker does not know how to restart a download.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::host::AppHandle;

/// Emitted with a [`Vec<DownloadItem>`] whenever a download changes.
pub const DOWNLOADS_CHANGED_EVENT: &str = "downloads://changed";

/// At or above this size, a download waits for the user to agree.
/// Smaller ones may start on their own and still show up here.
pub const CONSENT_BYTES: u64 = 10 * 1024 * 1024;

/// How often a running download pushes a progress event.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

impl DownloadState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading)
    }

    /// The status bar stays up for these.
    pub fn needs_attention(self) -> bool {
        self.is_active() || self == Self::Failed
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub state: DownloadState,
    pub error: Option<String>,
    /// Unix milliseconds.
    pub started_at_ms: i64,
    pub cancellable: bool,
}

impl DownloadItem {
    /// 0..=100, or `None` when the size isn't known yet.
    pub fn percent(&self) -> Option<u32> {
        (self.total_bytes > 0).then(|| {
            ((self.downloaded_bytes as f64 / self.total_bytes as f64) * 100.0)
                .clamp(0.0, 100.0)
                .round() as u32
        })
    }
}

struct Slot {
    item: DownloadItem,
    cancel: Arc<AtomicBool>,
    last_emit: Instant,
}

/// Process-wide download list. Production uses [`global`]; tests build
/// their own so they don't share state.
pub struct DownloadTracker {
    slots: Mutex<HashMap<String, Slot>>,
}

impl DownloadTracker {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Vec<DownloadItem> {
        let mut items: Vec<_> = self.slots.lock().values().map(|s| s.item.clone()).collect();
        items.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.id.cmp(&b.id)));
        items
    }

    pub fn get(&self, id: &str) -> Option<DownloadItem> {
        self.slots.lock().get(id).map(|s| s.item.clone())
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.slots.lock().get(id).is_some_and(|s| {
            s.cancel.load(Ordering::SeqCst) || s.item.state == DownloadState::Cancelled
        })
    }

    /// Start (or restart) `id`. A download already running returns its
    /// existing cancel flag and is left alone.
    pub fn begin(
        &self,
        app: &AppHandle,
        id: impl Into<String>,
        name: impl Into<String>,
        total_bytes: u64,
    ) -> Arc<AtomicBool> {
        let id = id.into();
        let mut slots = self.slots.lock();
        if let Some(slot) = slots.get(&id) {
            if slot.item.state.is_active() && !slot.cancel.load(Ordering::SeqCst) {
                return slot.cancel.clone();
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        slots.insert(
            id.clone(),
            Slot {
                item: DownloadItem {
                    id,
                    name: name.into(),
                    total_bytes,
                    downloaded_bytes: 0,
                    state: DownloadState::Downloading,
                    error: None,
                    started_at_ms: now_ms(),
                    cancellable: true,
                },
                cancel: cancel.clone(),
                last_emit: Instant::now(),
            },
        );
        drop(slots);
        self.emit(app);
        cancel
    }

    /// Byte progress. The stored counts update every call; the event is
    /// throttled to [`PROGRESS_EVERY`].
    pub fn progress(&self, app: &AppHandle, id: &str, downloaded: u64, total: u64) {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return;
        };
        if slot.item.state == DownloadState::Cancelled || slot.cancel.load(Ordering::SeqCst) {
            return;
        }
        slot.item.state = DownloadState::Downloading;
        slot.item.downloaded_bytes = downloaded;
        if total > 0 {
            slot.item.total_bytes = total;
        }
        let due = slot.last_emit.elapsed() >= PROGRESS_EVERY;
        if due {
            slot.last_emit = Instant::now();
        }
        drop(slots);
        if due {
            self.emit(app);
        }
    }

    pub fn finish(&self, app: &AppHandle, id: &str) {
        self.settle(app, id, |item| {
            if item.state == DownloadState::Cancelled {
                return false;
            }
            item.downloaded_bytes = item.total_bytes.max(item.downloaded_bytes);
            item.state = DownloadState::Done;
            item.error = None;
            true
        });
    }

    pub fn fail(&self, app: &AppHandle, id: &str, error: impl Into<String>) {
        let error = error.into();
        self.settle(app, id, |item| {
            if item.state == DownloadState::Cancelled {
                return false;
            }
            item.state = DownloadState::Failed;
            item.error = Some(error);
            true
        });
    }

    /// Ask a running download to stop. Returns false when it isn't running.
    pub fn cancel(&self, app: &AppHandle, id: &str) -> bool {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return false;
        };
        if !slot.item.state.is_active() {
            return false;
        }
        slot.cancel.store(true, Ordering::SeqCst);
        slot.item.state = DownloadState::Cancelled;
        slot.item.error = None;
        drop(slots);
        self.emit(app);
        true
    }

    fn settle(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut DownloadItem) -> bool) {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return;
        };
        if !f(&mut slot.item) {
            return;
        }
        drop(slots);
        self.emit(app);
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(DOWNLOADS_CHANGED_EVENT, self.list());
    }
}

impl Default for DownloadTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global() -> &'static DownloadTracker {
    static T: OnceLock<DownloadTracker> = OnceLock::new();
    T.get_or_init(DownloadTracker::new)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The downloader stopped because [`DownloadTracker::cancel`] was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadCancelled;

impl std::fmt::Display for DownloadCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}

impl std::error::Error for DownloadCancelled {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing::TestApp;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn progress_finish_and_fail_are_recorded_and_emitted() {
        let app = TestApp::new();
        let t = DownloadTracker::new();
        let cancel = t.begin(&app.app, "speaker:titanet", "TitaNet", 1000);
        assert!(!cancel.load(Ordering::SeqCst));
        let item = t.get("speaker:titanet").unwrap();
        assert_eq!(item.state, DownloadState::Downloading);
        assert_eq!(item.percent(), Some(0));

        t.progress(&app.app, "speaker:titanet", 450, 1000);
        // The first progress is inside the throttle window: stored, not
        // necessarily emitted again.
        assert_eq!(t.get("speaker:titanet").unwrap().percent(), Some(45));

        t.finish(&app.app, "speaker:titanet");
        let done = t.get("speaker:titanet").unwrap();
        assert_eq!(done.state, DownloadState::Done);
        assert_eq!(done.percent(), Some(100));

        t.begin(&app.app, "segmentation:pyannote", "Segmentation", 100);
        t.fail(&app.app, "segmentation:pyannote", "network");
        let failed = t.get("segmentation:pyannote").unwrap();
        assert_eq!(failed.state, DownloadState::Failed);
        assert_eq!(failed.error.as_deref(), Some("network"));

        let names = app.event_names();
        assert!(names.iter().any(|n| n == DOWNLOADS_CHANGED_EVENT));
        let last = app
            .events
            .lock()
            .iter()
            .rev()
            .find(|e| e.name == DOWNLOADS_CHANGED_EVENT)
            .unwrap()
            .payload
            .clone();
        let items: Vec<DownloadItem> = serde_json::from_value(last).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn cancel_stops_a_running_download_and_ignores_a_later_finish() {
        let app = TestApp::new();
        let t = DownloadTracker::new();
        let flag = t.begin(&app.app, "speaker:titanet", "TitaNet", 100);
        assert!(t.cancel(&app.app, "speaker:titanet"));
        assert!(flag.load(Ordering::SeqCst));
        assert!(t.is_cancelled("speaker:titanet"));
        assert_eq!(
            t.get("speaker:titanet").unwrap().state,
            DownloadState::Cancelled
        );
        // The downloader finishing after the cancel doesn't flip it to done.
        t.finish(&app.app, "speaker:titanet");
        t.fail(&app.app, "speaker:titanet", "late");
        assert_eq!(
            t.get("speaker:titanet").unwrap().state,
            DownloadState::Cancelled
        );
        assert!(!t.cancel(&app.app, "missing"));
    }

    #[test]
    fn a_second_begin_while_running_does_not_reset_progress() {
        let app = TestApp::new();
        let t = DownloadTracker::new();
        let first = t.begin(&app.app, "a", "A", 100);
        t.progress(&app.app, "a", 40, 100);
        let second = t.begin(&app.app, "a", "A", 100);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(t.get("a").unwrap().downloaded_bytes, 40);
    }

    #[test]
    fn a_failed_download_can_be_started_again() {
        let app = TestApp::new();
        let t = DownloadTracker::new();
        t.begin(&app.app, "a", "A", 10);
        t.fail(&app.app, "a", "nope");
        // Started times are millisecond resolution.
        thread::sleep(StdDuration::from_millis(2));
        t.begin(&app.app, "a", "A again", 10);
        let item = t.get("a").unwrap();
        assert_eq!(item.state, DownloadState::Downloading);
        assert_eq!(item.name, "A again");
        assert!(item.error.is_none());
    }
}
