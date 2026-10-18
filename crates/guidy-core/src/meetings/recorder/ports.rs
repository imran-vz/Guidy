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
    /// End a recording that never got going (a failed start): set `ended_at`,
    /// status `Failed` and `error`, only while the row is still `Recording`.
    /// Returns false when it wasn't.
    fn fail_recording(&self, id: MeetingId, ended_at: i64, error: &str) -> Result<bool>;
    /// Retention: clear both track paths, only when the meeting is fully
    /// processed (`processing_stage = done`) and not recording/processing.
    /// Returns false (nothing changed) otherwise.
    fn clear_audio_paths_if_processed(&self, id: MeetingId) -> Result<bool>;
    /// Retention candidates: meetings that still have audio (either track),
    /// aren't recording, and ended (or, without an end, started) before
    /// `ts` (unix ms).
    fn meetings_with_audio_older_than(&self, ts: i64) -> Result<Vec<Meeting>>;
    /// Every track path any meeting references (the orphan sweep).
    fn audio_paths(&self) -> Result<Vec<String>>;
}

/// Called from the capture threads with 16 kHz mono f32 samples (owned, so
/// they move on to the writer without another copy). Must not block.
pub type CaptureSink = Arc<dyn Fn(Track, Vec<f32>) + Send + Sync>;

/// Called from the capture threads when a stream fails mid-recording (e.g.
/// the mic or the system-audio tap stopped delivering audio). Must not block.
pub type CaptureErrorSink = Arc<dyn Fn(Track, String) + Send + Sync>;

/// Mic (echo-cancelled) + system-audio capture.
pub trait CaptureBackend: Send + Sync {
    fn start(
        &self,
        sink: CaptureSink,
        on_error: CaptureErrorSink,
    ) -> Result<Box<dyn CaptureSession>>;
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
        /// Make `delete_meeting` fail.
        pub fail_delete: Mutex<bool>,
        /// Make this many of the next `finish_recording` calls fail.
        pub fail_finish: Mutex<usize>,
    }

    impl MeetingRepo for MemRepo {
        fn meetings_with_audio_older_than(&self, ts: i64) -> Result<Vec<Meeting>> {
            Ok(self
                .rows
                .lock()
                .iter()
                .filter(|m| {
                    (m.mic_audio_path.is_some() || m.system_audio_path.is_some())
                        && m.status != MeetingStatus::Recording
                        && m.ended_at.unwrap_or(m.started_at) < ts
                })
                .cloned()
                .collect())
        }
        fn audio_paths(&self) -> Result<Vec<String>> {
            Ok(self
                .rows
                .lock()
                .iter()
                .flat_map(|m| [m.mic_audio_path.clone(), m.system_audio_path.clone()])
                .flatten()
                .collect())
        }
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
            if *self.fail_delete.lock() {
                anyhow::bail!("database is locked");
            }
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
            {
                let mut fail = self.fail_finish.lock();
                if *fail > 0 {
                    *fail -= 1;
                    anyhow::bail!("database is locked");
                }
            }
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
        fn fail_recording(&self, id: MeetingId, ended_at: i64, error: &str) -> Result<bool> {
            let mut rows = self.rows.lock();
            match rows
                .iter_mut()
                .find(|m| m.id == id && m.status == MeetingStatus::Recording)
            {
                Some(row) => {
                    row.ended_at = Some(ended_at);
                    row.status = MeetingStatus::Failed;
                    row.error = Some(error.into());
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
        pub on_error: Mutex<Option<CaptureErrorSink>>,
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
            sink(track, samples.to_vec());
        }

        /// A stream dying mid-recording.
        pub fn fail_stream(&self, track: Track, message: &str) {
            let on_error = self.on_error.lock().clone().expect("capture not started");
            on_error(track, message.into());
        }
    }

    impl CaptureBackend for FakeCapture {
        fn start(
            &self,
            sink: CaptureSink,
            on_error: CaptureErrorSink,
        ) -> Result<Box<dyn CaptureSession>> {
            if *self.fail.lock() {
                anyhow::bail!("no permission");
            }
            *self.sink.lock() = Some(sink);
            *self.on_error.lock() = Some(on_error);
            Ok(Box::new(FakeSession(self.stops.clone())))
        }
    }

    /// Runs once, inside the next call (to interleave another operation).
    pub type Hook = Mutex<Option<Box<dyn FnOnce() + Send>>>;

    fn run_hook(hook: &Hook) {
        let hook = hook.lock().take();
        if let Some(f) = hook {
            f();
        }
    }

    #[derive(Default)]
    pub struct FakeCalendar {
        pub events: Mutex<Vec<CalendarEvent>>,
        pub queries: Mutex<usize>,
        /// Runs during the next query, before it answers.
        pub during_query: Hook,
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
            run_hook(&self.during_query);
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
        /// Runs during the next post, before it is recorded.
        pub during_post: Hook,
    }

    impl Notifier for FakeNotifier {
        fn post(
            &self,
            id: &str,
            title: &str,
            body: &str,
            actions: &[NotificationAction],
        ) -> Result<()> {
            run_hook(&self.during_post);
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
