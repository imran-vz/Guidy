//! The recorder's ports: the small interfaces it needs from the store (A),
//! the macOS bridges (B) and the app. Real adapters live in
//! [`super::adapters`]; tests use the fakes in [`fakes`].

use std::sync::Arc;

use anyhow::Result;

#[cfg(test)]
use crate::meetings::types::MeetingStatus;
use crate::meetings::types::{CalendarEvent, Meeting, MeetingId, Track};

/// What the recorder and retention need from `meetings.db`.
pub trait MeetingRepo: Send + Sync {
    /// Insert a meeting row (`meeting.id` is ignored) and return its id.
    fn insert_meeting(&self, meeting: &Meeting) -> Result<MeetingId>;
    fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>>;
    fn list_meetings(&self) -> Result<Vec<Meeting>>;
    fn delete_meeting(&self, id: MeetingId) -> Result<()>;
    /// Set both track paths (nothing else: a concurrent rename survives).
    fn set_audio_paths(&self, id: MeetingId, mic: Option<&str>, system: Option<&str>)
        -> Result<()>;
    /// End a recording: set `ended_at`, status `Recorded` and `error`, only
    /// while the row is still `Recording`. Returns false when it wasn't.
    fn finish_recording(&self, id: MeetingId, ended_at: i64, error: Option<&str>) -> Result<bool>;
    /// Retention: clear both track paths, only when the meeting is fully
    /// processed (`processing_stage = done`) and not recording/processing.
    /// Returns false (nothing changed) otherwise.
    fn clear_audio_paths_if_processed(&self, id: MeetingId) -> Result<bool>;
}

/// Called from the capture (audio) threads with 16 kHz mono f32 samples.
/// Must not block.
pub type CaptureSink = Arc<dyn Fn(Track, &[f32]) + Send + Sync>;

/// Mic (echo-cancelled) + system-audio capture.
pub trait CaptureBackend: Send + Sync {
    fn start(&self, sink: CaptureSink) -> Result<Box<dyn CaptureSession>>;
}

/// A running capture. Dropping it must also stop it.
pub trait CaptureSession: Send {
    fn stop(self: Box<Self>) -> Result<()>;
}

pub trait CalendarSource: Send + Sync {
    /// Events overlapping `[start_ms, end_ms)` (unix ms).
    fn events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>>;
    /// Every timed event overlapping the range, with or without a meeting
    /// link (manual starts link to whatever is happening now).
    fn timed_events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
        self.events_between(start_ms, end_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAction {
    pub id: String,
    pub title: String,
}

pub trait Notifier: Send + Sync {
    fn post(&self, id: &str, title: &str, body: &str, actions: &[NotificationAction])
        -> Result<()>;
}

#[cfg(test)]
pub mod fakes {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    pub struct MemRepo {
        pub rows: Mutex<Vec<Meeting>>,
        next: Mutex<MeetingId>,
        /// Make `set_audio_paths` fail.
        pub fail_set_audio_paths: Mutex<bool>,
    }

    impl MeetingRepo for MemRepo {
        fn insert_meeting(&self, meeting: &Meeting) -> Result<MeetingId> {
            let mut next = self.next.lock();
            *next += 1;
            let mut m = meeting.clone();
            m.id = *next;
            self.rows.lock().push(m);
            Ok(*next)
        }
        fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
            Ok(self.rows.lock().iter().find(|m| m.id == id).cloned())
        }
        fn list_meetings(&self) -> Result<Vec<Meeting>> {
            Ok(self.rows.lock().clone())
        }
        fn delete_meeting(&self, id: MeetingId) -> Result<()> {
            self.rows.lock().retain(|m| m.id != id);
            Ok(())
        }
        fn set_audio_paths(
            &self,
            id: MeetingId,
            mic: Option<&str>,
            system: Option<&str>,
        ) -> Result<()> {
            if *self.fail_set_audio_paths.lock() {
                anyhow::bail!("database is locked");
            }
            let mut rows = self.rows.lock();
            let row = rows
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or_else(|| anyhow::anyhow!("no meeting {id}"))?;
            row.mic_audio_path = mic.map(Into::into);
            row.system_audio_path = system.map(Into::into);
            Ok(())
        }
        fn finish_recording(
            &self,
            id: MeetingId,
            ended_at: i64,
            error: Option<&str>,
        ) -> Result<bool> {
            let mut rows = self.rows.lock();
            match rows
                .iter_mut()
                .find(|m| m.id == id && m.status == MeetingStatus::Recording)
            {
                Some(row) => {
                    row.ended_at = Some(ended_at);
                    row.status = MeetingStatus::Recorded;
                    row.error = error.map(Into::into);
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        fn clear_audio_paths_if_processed(&self, id: MeetingId) -> Result<bool> {
            let mut rows = self.rows.lock();
            match rows.iter_mut().find(|m| {
                m.id == id
                    && !matches!(
                        m.status,
                        MeetingStatus::Recording | MeetingStatus::Processing
                    )
                    && m.processing_stage.as_deref() == Some("done")
            }) {
                Some(row) => {
                    row.mic_audio_path = None;
                    row.system_audio_path = None;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }

    /// Capture that hands its sink to the test, which then plays audio in.
    #[derive(Default)]
    pub struct FakeCapture {
        pub sink: Mutex<Option<CaptureSink>>,
        pub fail: Mutex<bool>,
        pub stops: Arc<Mutex<usize>>,
    }

    struct FakeSession(Arc<Mutex<usize>>);

    impl CaptureSession for FakeSession {
        fn stop(self: Box<Self>) -> Result<()> {
            *self.0.lock() += 1;
            Ok(())
        }
    }

    impl FakeCapture {
        pub fn feed(&self, track: Track, samples: &[f32]) {
            let sink = self.sink.lock().clone().expect("capture not started");
            sink(track, samples);
        }
    }

    impl CaptureBackend for FakeCapture {
        fn start(&self, sink: CaptureSink) -> Result<Box<dyn CaptureSession>> {
            if *self.fail.lock() {
                anyhow::bail!("no permission");
            }
            *self.sink.lock() = Some(sink);
            Ok(Box::new(FakeSession(self.stops.clone())))
        }
    }

    #[derive(Default)]
    pub struct FakeCalendar {
        pub events: Mutex<Vec<CalendarEvent>>,
        pub queries: Mutex<usize>,
    }

    impl CalendarSource for FakeCalendar {
        /// Like the native source: only events with a meeting link.
        fn events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
            Ok(self
                .timed_events_between(start_ms, end_ms)?
                .into_iter()
                .filter(|e| e.meeting_url.is_some())
                .collect())
        }
        fn timed_events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
            *self.queries.lock() += 1;
            Ok(self
                .events
                .lock()
                .iter()
                .filter(|e| e.start_at < end_ms && e.end_at > start_ms)
                .cloned()
                .collect())
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Posted {
        pub id: String,
        pub title: String,
        pub body: String,
        pub actions: Vec<String>,
    }

    #[derive(Default)]
    pub struct FakeNotifier {
        pub posted: Mutex<Vec<Posted>>,
    }

    impl Notifier for FakeNotifier {
        fn post(
            &self,
            id: &str,
            title: &str,
            body: &str,
            actions: &[NotificationAction],
        ) -> Result<()> {
            self.posted.lock().push(Posted {
                id: id.into(),
                title: title.into(),
                body: body.into(),
                actions: actions.iter().map(|a| a.id.clone()).collect(),
            });
            Ok(())
        }
    }
}
