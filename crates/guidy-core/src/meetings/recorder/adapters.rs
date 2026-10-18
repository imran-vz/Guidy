//! Real implementations of the recorder ports: the encrypted store (A), the
//! macOS bridges (B), and the app (events, tray, settings, translations).

use std::sync::{Arc, OnceLock};

use anyhow::Result;

use super::ports::*;
use super::{MeetingRecorder, NotificationStrings, RecorderDeps};
use crate::host::AppHandle;
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;

// ----- Store (A) -------------------------------------------------------------

pub struct StoreRepo(pub Arc<MeetingsStore>);

impl MeetingRepo for StoreRepo {
    fn insert_meeting(&self, meeting: &Meeting) -> Result<MeetingId> {
        self.0.insert_meeting(meeting)
    }
    fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        self.0.get_meeting(id)
    }
    fn list_meetings(&self) -> Result<Vec<Meeting>> {
        self.0.list_meetings()
    }
    fn delete_meeting(&self, id: MeetingId) -> Result<()> {
        self.0.delete_meeting(id).map(|_| ())
    }
    fn set_audio_paths(
        &self,
        id: MeetingId,
        mic: Option<&str>,
        system: Option<&str>,
    ) -> Result<()> {
        self.0.set_audio_paths(id, mic, system)
    }
    fn finish_recording(&self, id: MeetingId, ended_at: i64, error: Option<&str>) -> Result<bool> {
        self.0.finish_recording(id, ended_at, error)
    }
    fn fail_recording(&self, id: MeetingId, ended_at: i64, error: &str) -> Result<bool> {
        // `finish_recording` is the conditional step (only a row still
        // `Recording` changes); the status then moves on to `Failed`.
        if !self.0.finish_recording(id, ended_at, Some(error))? {
            return Ok(false);
        }
        self.0
            .set_meeting_status(id, MeetingStatus::Failed, Some(error))?;
        Ok(true)
    }
    fn clear_audio_paths_if_processed(&self, id: MeetingId) -> Result<bool> {
        self.0.clear_audio_paths_if_processed(id)
    }
    fn meetings_with_audio_older_than(&self, ts: i64) -> Result<Vec<Meeting>> {
        self.0.meetings_with_audio_older_than(ts)
    }
    fn audio_paths(&self) -> Result<Vec<String>> {
        self.0.audio_paths()
    }
}

// ----- Native bridges (B) ----------------------------------------------------

use crate::meetings::native;

/// Mic (VPIO) + system audio (process tap) through [`native::start_capture`].
pub struct NativeCapture;

/// EventKit through [`native::events_between`].
pub struct NativeCalendar;

/// `UNUserNotificationCenter` through [`native::post_notification`].
pub struct NativeNotifier;

struct NativeSession(native::CaptureHandle);

impl CaptureSession for NativeSession {
    fn stop(mut self: Box<Self>) -> Result<()> {
        // Waits until no callback runs any more.
        self.0.stop();
        Ok(())
    }
}

fn forward(sink: &CaptureSink, track: Track, samples: Vec<f32>, rate: u32, warned: &mut bool) {
    if rate != native::CAPTURE_SAMPLE_RATE && !*warned {
        // The bridge contract is 16 kHz; anything else would mistime the track.
        log::error!("meeting capture: {track:?} delivered {rate} Hz, expected 16 kHz");
        *warned = true;
    }
    sink(track, samples);
}

impl CaptureBackend for NativeCapture {
    fn start(
        &self,
        sink: CaptureSink,
        on_error: CaptureErrorSink,
    ) -> Result<Box<dyn CaptureSession>> {
        let mic_sink = sink.clone();
        let (mut mic_warned, mut sys_warned) = (false, false);
        let callbacks = native::CaptureCallbacks::new(
            move |samples, rate| forward(&mic_sink, Track::Mic, samples, rate, &mut mic_warned),
            move |samples, rate| forward(&sink, Track::System, samples, rate, &mut sys_warned),
        )
        .with_events(move |e| {
            if e.is_error() {
                // A stream stopped delivering audio: the recorder keeps it
                // for the meeting's error.
                on_error(e.track, e.message);
            } else {
                log::warn!("meeting capture ({:?}): {}", e.track, e.message);
            }
        });
        Ok(Box::new(NativeSession(native::start_capture(callbacks)?)))
    }
}

impl CalendarSource for NativeCalendar {
    fn events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
        native::events_between(start_ms, end_ms)
    }
    fn timed_events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
        native::timed_events_between(start_ms, end_ms)
    }
}

impl Notifier for NativeNotifier {
    fn post(
        &self,
        id: &str,
        title: &str,
        body: &str,
        actions: &[NotificationAction],
    ) -> Result<()> {
        let pairs: Vec<(&str, &str)> = actions
            .iter()
            .map(|a| (a.id.as_str(), a.title.as_str()))
            .collect();
        if native::post_notification(id, title, body, &pairs) {
            Ok(())
        } else {
            anyhow::bail!("notifications are unavailable")
        }
    }
}

/// The app that notification responses are routed to.
static NOTIFICATION_APP: OnceLock<AppHandle> = OnceLock::new();

/// Route notification responses ("Stop", "Keep going", "Start") to the
/// manager. Idempotent.
pub fn install_notification_handler(app: &AppHandle) {
    if NOTIFICATION_APP.set(app.clone()).is_err() {
        return;
    }
    native::set_notification_handler(on_notification_response);
}

fn on_notification_response(notification_id: &str, action_id: &str) {
    let Some(app) = NOTIFICATION_APP.get().cloned() else {
        return;
    };
    if action_id == native::NOTIFICATION_DEFAULT_ACTION {
        // A click on the notification body opens Guidy.
        app.shell().show_main_window();
        return;
    }
    let (notification_id, action_id) = (notification_id.to_string(), action_id.to_string());
    // Never do the work on the notification center's thread.
    std::thread::spawn(move || {
        if let Some(m) = app.try_state::<Arc<crate::meetings::manager::MeetingsManager>>() {
            m.handle_notification(&notification_id, &action_id);
        }
    });
}

// ----- App wiring -------------------------------------------------------------

/// Notification texts in the app language, falling back to English per
/// string for locales that haven't translated them yet.
pub fn notification_strings(locale: &str) -> NotificationStrings {
    use crate::tray_i18n::get_tray_translations;
    let s = get_tray_translations(Some(locale.to_string()));
    let en = get_tray_translations(Some("en".to_string()));
    let pick = |a: String, b: String| if a.trim().is_empty() { b } else { a };
    NotificationStrings {
        still_recording: pick(s.meeting_still_recording, en.meeting_still_recording),
        stop: pick(s.meeting_stop, en.meeting_stop),
        keep_going: pick(s.meeting_keep_going, en.meeting_keep_going),
        take_notes_for: pick(s.meeting_take_notes_for, en.meeting_take_notes_for),
        start: pick(s.meeting_start, en.meeting_start),
        untitled_meeting: pick(s.meeting_untitled, en.meeting_untitled),
    }
}

/// Build the real recorder for the running app.
pub fn app_recorder(
    app: &AppHandle,
    store: Arc<MeetingsStore>,
    audio_dir: std::path::PathBuf,
) -> MeetingRecorder {
    let settings_app = app.clone();
    let strings_app = app.clone();
    let state_app = app.clone();
    let changed_app = app.clone();
    MeetingRecorder::new(RecorderDeps {
        repo: Arc::new(StoreRepo(store)),
        capture: Arc::new(NativeCapture),
        calendar: Arc::new(NativeCalendar),
        notifier: Arc::new(NativeNotifier),
        data_key: Arc::new(crate::secrets::meetings_data_key),
        audio_dir,
        settings: Arc::new(move || crate::settings::get_settings(&settings_app).meetings),
        strings: Arc::new(move || {
            notification_strings(&crate::settings::get_settings(&strings_app).app_language)
        }),
        on_state: Arc::new(move |state: &RecorderState| {
            if let Err(e) = state_app.emit(events::RECORDER_STATE, state) {
                log::warn!("emit {}: {e}", events::RECORDER_STATE);
            }
        }),
        on_meeting_changed: Arc::new(move |id| {
            if let Err(e) = changed_app.emit(events::MEETING_CHANGED, id) {
                log::warn!("emit {}: {e}", events::MEETING_CHANGED);
            }
        }),
    })
}
