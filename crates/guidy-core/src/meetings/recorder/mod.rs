//! Recording sessions (plan task 1.3, D16).
//!
//! [`MeetingRecorder`] is the session state machine:
//!
//! - **start**: link a calendar event (the given one, else whichever is on
//!   now), create the meeting row (`Recording`), open the two encrypted
//!   [`TrackWriter`]s and start capture. Capture callbacks only push samples
//!   into a channel; a writer thread encodes and writes them.
//! - **stop**: stop capture, finish both files, set `ended_at` and status
//!   `Recorded`. (The manager then queues processing; a finish that only a
//!   later retry manages is reported through `on_recording_finished`.)
//! - **tick** (driven by the [`Scheduler`] thread): "still recording"
//!   reminders and "Take notes for …?" calendar prompts.
//!
//! Everything external is behind the [`ports`] traits so the whole state
//! machine is tested with fakes; [`adapters`] wires the real app.

pub mod adapters;
pub mod ports;
pub mod schedule;

pub use ports::*;

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::{Condvar, Mutex};

use crate::meetings::audio_store::{self, TrackSummary, TrackWriter};
use crate::meetings::settings::MeetingSettings;
use crate::meetings::types::*;
use schedule::{ReminderState, MINUTE_MS};

/// Localized notification texts (from the tray translations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationStrings {
    pub still_recording: String,
    pub stop: String,
    pub keep_going: String,
    /// Contains `{{title}}`.
    pub take_notes_for: String,
    pub start: String,
    pub untitled_meeting: String,
}

impl Default for NotificationStrings {
    fn default() -> Self {
        Self {
            still_recording: "Guidy is still recording".into(),
            stop: "Stop".into(),
            keep_going: "Keep going".into(),
            take_notes_for: "Take notes for {{title}}?".into(),
            start: "Start".into(),
            untitled_meeting: "Meeting".into(),
        }
    }
}

type Shared<F> = Arc<F>;

/// Everything the recorder talks to.
#[derive(Clone)]
pub struct RecorderDeps {
    pub repo: Arc<dyn MeetingRepo>,
    pub capture: Arc<dyn CaptureBackend>,
    pub calendar: Arc<dyn CalendarSource>,
    pub notifier: Arc<dyn Notifier>,
    pub data_key: Shared<dyn Fn() -> Result<[u8; 32]> + Send + Sync>,
    pub audio_dir: PathBuf,
    pub settings: Shared<dyn Fn() -> MeetingSettings + Send + Sync>,
    pub strings: Shared<dyn Fn() -> NotificationStrings + Send + Sync>,
    /// Called after every recorder state change (emit + tray refresh).
    pub on_state: Shared<dyn Fn(&RecorderState) + Send + Sync>,
    /// Called when a meeting row changed (emit `MEETING_CHANGED`).
    pub on_meeting_changed: Shared<dyn Fn(MeetingId) + Send + Sync>,
    /// Called when a stopped recording whose row update failed at stop is
    /// finished by a later retry (start, tick, idle stop), so it gets queued
    /// for processing like a normal stop. May be called with `op` held: it
    /// must not call back into the recorder synchronously.
    pub on_recording_finished: Shared<dyn Fn(MeetingId) + Send + Sync>,
}

enum WriterMsg {
    Samples(Track, Vec<f32>),
    Finish,
}

type WriterOutcome = (Result<TrackSummary>, Result<TrackSummary>);

struct Session {
    meeting_id: MeetingId,
    title: String,
    started_at: i64,
    scheduled_end_at: Option<i64>,
    last_reminder_at: Option<i64>,
    capture: Option<Box<dyn CaptureSession>>,
    tx: Sender<WriterMsg>,
    writer: Option<JoinHandle<WriterOutcome>>,
    /// Streams that failed mid-recording; written as the meeting's error.
    capture_errors: Arc<Mutex<Vec<String>>>,
}

/// A stopped recording whose row is still `Recording` because the database
/// update failed at stop. Retried by the next stop, start, tick and
/// [`MeetingRecorder::recover_interrupted`], so it never waits for a restart.
/// Each one is kept (and retried) on its own: recording goes on meanwhile.
#[derive(Debug, Clone)]
struct PendingFinish {
    meeting_id: MeetingId,
    ended_at: i64,
    error: Option<String>,
}

/// Waits between the attempts to finalize the row at stop (transient
/// errors such as a busy database).
const FINISH_RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(50), Duration::from_millis(250)];

#[derive(Default)]
struct Prompts {
    /// Event id -> start time, for every event already prompted.
    prompted: std::collections::HashMap<String, i64>,
    /// Events seen by the last poll, so "Start" can snapshot them.
    seen: Vec<CalendarEvent>,
    last_poll: Option<i64>,
}

/// Parsed notification response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationCommand {
    /// Reminder "Stop" for this meeting.
    Stop(MeetingId),
    /// Reminder "Keep going".
    KeepGoing(MeetingId),
    /// Calendar prompt "Start" for this event id.
    Start(String),
    Ignore,
}

const REMINDER_PREFIX: &str = "guidy.meetings.reminder.";
const PROMPT_PREFIX: &str = "guidy.meetings.prompt.";
pub const ACTION_STOP: &str = "stop";
pub const ACTION_KEEP_GOING: &str = "keep_going";
pub const ACTION_START: &str = "start";

pub fn reminder_notification_id(id: MeetingId) -> String {
    format!("{REMINDER_PREFIX}{id}")
}

pub fn prompt_notification_id(event_id: &str) -> String {
    format!("{PROMPT_PREFIX}{event_id}")
}

pub fn parse_notification(notification_id: &str, action_id: &str) -> NotificationCommand {
    if let Some(id) = notification_id.strip_prefix(REMINDER_PREFIX) {
        let Ok(id) = id.parse() else {
            return NotificationCommand::Ignore;
        };
        return match action_id {
            ACTION_STOP => NotificationCommand::Stop(id),
            ACTION_KEEP_GOING => NotificationCommand::KeepGoing(id),
            _ => NotificationCommand::Ignore,
        };
    }
    if let Some(event_id) = notification_id.strip_prefix(PROMPT_PREFIX) {
        if action_id == ACTION_START && !event_id.is_empty() {
            return NotificationCommand::Start(event_id.to_string());
        }
    }
    NotificationCommand::Ignore
}

pub struct MeetingRecorder {
    deps: RecorderDeps,
    /// Serializes start/stop/crash recovery.
    op: Mutex<()>,
    /// Held by a [`Scheduler`] thread for a whole tick: a scheduler started
    /// while a stopped one is still finishing its tick waits for it, so two
    /// never tick at once (or post twice).
    ticking: Mutex<()>,
    session: Mutex<Option<Session>>,
    pending: Mutex<Vec<PendingFinish>>,
    state: Mutex<RecorderState>,
    prompts: Mutex<Prompts>,
}

fn local_time(ms: i64, fmt: &str) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format(fmt).to_string())
        .unwrap_or_default()
}

impl MeetingRecorder {
    pub fn new(deps: RecorderDeps) -> Self {
        Self {
            deps,
            op: Mutex::new(()),
            ticking: Mutex::new(()),
            session: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            state: Mutex::new(RecorderState::Idle),
            prompts: Mutex::new(Prompts::default()),
        }
    }

    pub fn deps(&self) -> &RecorderDeps {
        &self.deps
    }

    /// Cheap: never waits on a start/stop in progress.
    pub fn state(&self) -> RecorderState {
        self.state.lock().clone()
    }

    fn set_state(&self, state: RecorderState) {
        *self.state.lock() = state.clone();
        (self.deps.on_state)(&state);
    }

    /// The event a start for `event_id` links to: from the last poll, else
    /// the calendar. `None` when it can't be found or ended more than
    /// [`schedule::START_LINK_GRACE_MS`] ago (a calendar prompt's "Start" may
    /// be clicked hours later).
    fn find_event(&self, event_id: &str, now: i64) -> Option<CalendarEvent> {
        let cached = self
            .prompts
            .lock()
            .seen
            .iter()
            .find(|e| e.id == event_id)
            .cloned();
        let event = cached.or_else(|| {
            match self
                .deps
                .calendar
                .events_between(now - 24 * 60 * MINUTE_MS, now + 24 * 60 * MINUTE_MS)
            {
                Ok(events) => events.into_iter().find(|e| e.id == event_id),
                Err(e) => {
                    log::warn!("meetings: calendar lookup failed: {e:#}");
                    None
                }
            }
        });
        event.filter(|e| schedule::links_to_event(e, now))
    }

    fn current_event(&self, now: i64) -> Option<CalendarEvent> {
        // Any timed event, with or without a meeting link (plan 1.3);
        // `schedule::current_event` still prefers one with a link.
        match self.deps.calendar.timed_events_between(now, now + 1) {
            Ok(events) => schedule::current_event(&events, now).cloned(),
            Err(e) => {
                log::debug!("meetings: no calendar for manual start: {e:#}");
                None
            }
        }
    }

    /// Start recording at `now`, linked to the calendar event
    /// `calendar_event_id`. When that is `None`, or the event has ended or
    /// can't be found ([`Self::find_event`]), it links to the event happening
    /// now, if any.
    pub fn start(&self, calendar_event_id: Option<String>, now: i64) -> Result<MeetingId> {
        let _op = self.op.lock();
        if self.session.lock().is_some() {
            bail!("a meeting is already being recorded");
        }
        // Failing ones stay pending: recording a new meeting isn't blocked.
        if let Err(e) = self.retry_pending() {
            log::warn!("meetings: finishing a previous recording failed again: {e:#}");
        }
        let key = (self.deps.data_key)().context("meetings data key")?;

        let event = calendar_event_id
            .as_deref()
            .and_then(|id| {
                let event = self.find_event(id, now);
                if event.is_none() {
                    log::info!(
                        "meetings: calendar event {id} has ended or can't be found: \
                         starting without linking to it"
                    );
                }
                event
            })
            .or_else(|| self.current_event(now));
        let title = match &event {
            Some(e) if !e.title.trim().is_empty() => e.title.clone(),
            _ => format!(
                "{} {}",
                (self.deps.strings)().untitled_meeting,
                local_time(now, "%b %-d, %H:%M")
            ),
        };
        let meeting = Meeting {
            id: 0,
            title: title.clone(),
            calendar_event_id: event.as_ref().map(|e| e.id.clone()),
            calendar_event: event.clone(),
            started_at: now,
            ended_at: None,
            mic_audio_path: None,
            system_audio_path: None,
            status: MeetingStatus::Recording,
            error: None,
            processing_stage: None,
        };
        let id = self.deps.repo.insert_meeting(&meeting)?;

        let session = match self.open_session(meeting, id, &key) {
            Ok(s) => s,
            Err(e) => {
                // Don't leave an empty meeting behind for a start that never
                // recorded anything (`open_session` removed its files). If it
                // can't be deleted, mark it failed with the error so crash
                // recovery doesn't take it for an interrupted recording (and
                // the UI doesn't offer to process audio that doesn't exist).
                if let Err(e2) = self.deps.repo.delete_meeting(id) {
                    log::warn!("meetings: cleanup of meeting {id} failed: {e2:#}");
                    let msg = format!("{e:#}");
                    if let Err(e3) = self.deps.repo.fail_recording(id, now, &msg) {
                        log::warn!("meetings: ending meeting {id} failed: {e3:#}");
                    }
                }
                return Err(e);
            }
        };
        let state = RecorderState::Recording {
            meeting_id: id,
            title,
            started_at: now,
            scheduled_end_at: session.scheduled_end_at,
        };
        *self.session.lock() = Some(session);
        log::info!("meetings: recording meeting {id}");
        self.set_state(state);
        (self.deps.on_meeting_changed)(id);
        Ok(id)
    }

    /// Create the track files and start capture. On failure, every file it
    /// created is deleted (tracked here, not read back from the row: the
    /// failure may be the row update itself).
    fn open_session(&self, meeting: Meeting, id: MeetingId, key: &[u8; 32]) -> Result<Session> {
        let mut created = Vec::new();
        let result = self.open_session_inner(meeting, id, key, &mut created);
        if result.is_err() {
            // The writers are dropped (closed) by now.
            for path in &created {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => log::warn!(
                        "meetings: couldn't remove {} after a failed start: {e}",
                        path.display()
                    ),
                }
            }
        }
        result
    }

    fn open_session_inner(
        &self,
        mut meeting: Meeting,
        id: MeetingId,
        key: &[u8; 32],
        created: &mut Vec<PathBuf>,
    ) -> Result<Session> {
        let mic_rel = audio_store::track_file_name(id, Track::Mic);
        let sys_rel = audio_store::track_file_name(id, Track::System);
        let mic_path = self.deps.audio_dir.join(&mic_rel);
        let sys_path = self.deps.audio_dir.join(&sys_rel);
        // Tracked before `create`, which may fail after opening (truncating)
        // the file: whatever is at these fresh-id paths is ours.
        created.push(mic_path.clone());
        let mic = TrackWriter::create(&mic_path, key)?;
        created.push(sys_path.clone());
        let sys = TrackWriter::create(&sys_path, key)?;
        // Targeted: never overwrite the whole row (a rename may race this).
        self.deps
            .repo
            .set_audio_paths(id, Some(&mic_rel), Some(&sys_rel))?;
        meeting.id = id;
        meeting.mic_audio_path = Some(mic_rel);
        meeting.system_audio_path = Some(sys_rel);

        let (tx, rx) = channel();
        let writer = std::thread::Builder::new()
            .name(format!("meeting-writer-{id}"))
            .spawn(move || writer_loop(rx, mic, sys))
            .context("spawn meeting writer")?;

        let sink_tx = Mutex::new(tx.clone());
        let sink: CaptureSink = Arc::new(move |track, samples: Vec<f32>| {
            if !samples.is_empty() {
                // Unbounded channel: never blocks the audio thread.
                let _ = sink_tx.lock().send(WriterMsg::Samples(track, samples));
            }
        });
        let capture_errors = Arc::new(Mutex::new(Vec::new()));
        let errors = capture_errors.clone();
        let on_error: CaptureErrorSink = Arc::new(move |track, message: String| {
            log::error!("meetings: {track:?} capture of meeting {id} failed: {message}");
            let msg = format!("{} capture: {message}", track.as_str());
            let mut errors = errors.lock();
            if !errors.contains(&msg) {
                errors.push(msg);
            }
        });
        let capture = match self.deps.capture.start(sink, on_error) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(WriterMsg::Finish);
                let _ = writer.join();
                return Err(e.context("start meeting capture"));
            }
        };
        Ok(Session {
            meeting_id: id,
            title: meeting.title.clone(),
            started_at: meeting.started_at,
            scheduled_end_at: meeting.calendar_event.as_ref().map(|e| e.end_at),
            last_reminder_at: None,
            capture: Some(capture),
            tx,
            writer: Some(writer),
            capture_errors,
        })
    }

    /// Stop recording at `now`. Returns the meeting id, or `None` when idle.
    ///
    /// The row is finished (with retries) before the state goes `Idle`. If it
    /// still can't be, capture stays stopped and the finish is kept pending:
    /// the next `stop` (even when idle), `start`, tick or recovery retries
    /// it, so the meeting isn't stuck `Recording` until a restart. A finish
    /// done by such a retry is reported through
    /// [`RecorderDeps::on_recording_finished`], not returned.
    pub fn stop(&self, now: i64) -> Result<Option<MeetingId>> {
        let _op = self.op.lock();
        let Some(mut session) = self.session.lock().take() else {
            return self.retry_pending().map(|()| None);
        };
        let id = session.meeting_id;
        if let Some(capture) = session.capture.take() {
            if let Err(e) = capture.stop() {
                log::warn!("meetings: stopping capture failed: {e:#}");
            }
        }
        let _ = session.tx.send(WriterMsg::Finish);
        let mut errors = std::mem::take(&mut *session.capture_errors.lock());
        match session.writer.take().map(|w| w.join()) {
            Some(Ok((mic, sys))) => {
                for (track, r) in [(Track::Mic, mic), (Track::System, sys)] {
                    let track = track.as_str();
                    match r {
                        Ok(s) => log::info!(
                            "meetings: {track} track of meeting {id}: {} ms",
                            s.duration_ms()
                        ),
                        Err(e) => errors.push(format!("{track} track: {e:#}")),
                    }
                }
            }
            Some(Err(_)) => errors.push("audio writer panicked".into()),
            None => {}
        }
        let error = (!errors.is_empty()).then(|| errors.join("; "));
        if let Some(e) = &error {
            log::error!("meetings: meeting {id}: {e}");
        }
        let pending = PendingFinish {
            meeting_id: id,
            ended_at: now,
            error,
        };
        self.pending.lock().push(pending.clone());
        let mut result = self.finish_one(&pending);
        for delay in FINISH_RETRY_DELAYS {
            // Stop once it worked, or there's nothing left to retry.
            let Err(e) = &result else { break };
            if !self.is_pending(id) {
                break;
            }
            log::warn!("meetings: finishing meeting {id} failed, retrying: {e:#}");
            std::thread::sleep(delay);
            result = self.finish_one(&pending);
        }
        // Earlier stops whose finish failed. This one is still pending if
        // its own retries failed; a finish here is this stop's result (so
        // it's returned, not also reported through `on_recording_finished`).
        let (finished, error) = self.finish_pending();
        for fid in finished {
            if fid == id {
                result = Ok(());
            } else {
                log::info!("meetings: finished stopped recording {fid} on retry");
                (self.deps.on_recording_finished)(fid);
            }
        }
        if let Some(e) = error {
            log::warn!("meetings: finishing a stopped recording failed again: {e:#}");
        }
        // Capture is stopped either way.
        self.set_state(RecorderState::Idle);
        if result.is_ok() {
            log::info!("meetings: stopped recording meeting {id}");
        }
        result
            .map(|()| Some(id))
            .with_context(|| format!("finish recording meeting {id}"))
    }

    fn is_pending(&self, id: MeetingId) -> bool {
        self.pending.lock().iter().any(|p| p.meeting_id == id)
    }

    fn forget_pending(&self, id: MeetingId) {
        self.pending.lock().retain(|p| p.meeting_id != id);
    }

    /// Finish the row of one stopped recording ([`PendingFinish`]). On error
    /// it stays pending (unless the meeting is gone). Callers hold `op`.
    fn finish_one(&self, p: &PendingFinish) -> Result<()> {
        let id = p.meeting_id;
        let Some(meeting) = self.deps.repo.get_meeting(id)? else {
            self.forget_pending(id);
            bail!("meeting {id} disappeared while recording");
        };
        // Targeted update: the title (renamed while recording) is untouched.
        if !self.deps.repo.finish_recording(
            id,
            p.ended_at.max(meeting.started_at),
            p.error.as_deref(),
        )? {
            log::warn!("meetings: meeting {id} was no longer recording at stop");
        }
        self.forget_pending(id);
        (self.deps.on_meeting_changed)(id);
        Ok(())
    }

    /// Retry every pending finish, each on its own. Returns the ids finished
    /// now and the first error (the others are logged). Callers hold `op`.
    fn finish_pending(&self) -> (Vec<MeetingId>, Option<anyhow::Error>) {
        let pending = self.pending.lock().clone();
        let mut finished = Vec::new();
        let mut first_error = None;
        for p in pending {
            match self.finish_one(&p) {
                Ok(()) => finished.push(p.meeting_id),
                Err(e) => {
                    let e = e.context(format!("finish recording meeting {}", p.meeting_id));
                    if first_error.is_none() {
                        first_error = Some(e);
                    } else {
                        log::warn!("meetings: {e:#}");
                    }
                }
            }
        }
        (finished, first_error)
    }

    /// [`Self::finish_pending`], reporting each meeting finished through
    /// [`RecorderDeps::on_recording_finished`] (queued for processing like
    /// a normal stop). Callers hold `op`.
    fn retry_pending(&self) -> Result<()> {
        let (finished, error) = self.finish_pending();
        for id in finished {
            log::info!("meetings: finished stopped recording {id} on retry");
            (self.deps.on_recording_finished)(id);
        }
        error.map_or(Ok(()), Err)
    }

    /// Whether a stopped recording still waits for its row to be finished
    /// (the scheduler keeps ticking to retry it).
    pub fn has_pending_finish(&self) -> bool {
        !self.pending.lock().is_empty()
    }

    pub fn is_recording(&self) -> bool {
        self.session.lock().is_some()
    }

    pub fn current_meeting_id(&self) -> Option<MeetingId> {
        self.session.lock().as_ref().map(|s| s.meeting_id)
    }

    /// "Keep going": the next reminder comes one interval from `now`.
    pub fn snooze(&self, meeting_id: MeetingId, now: i64) {
        if let Some(s) = self.session.lock().as_mut() {
            if s.meeting_id == meeting_id {
                s.last_reminder_at = Some(now);
            }
        }
    }

    /// When the next reminder is due, if recording.
    pub fn next_reminder_at(&self) -> Option<i64> {
        let interval = (self.deps.settings)().reminder_interval_minutes;
        self.session
            .lock()
            .as_ref()
            .map(|s| schedule::next_reminder_at(&reminder_state(s, interval)))
    }

    /// One scheduler step: retry pending finishes, post a due reminder, and
    /// (at most once a minute) poll the calendar for events starting now.
    pub fn tick(&self, now: i64) {
        let settings = (self.deps.settings)();
        let strings = (self.deps.strings)();
        {
            let _op = self.op.lock();
            // Retry stops whose row update failed.
            if let Err(e) = self.retry_pending() {
                log::warn!("meetings: finishing a stopped recording failed: {e:#}");
            }
            // Decided and posted under `op`: a stop can't withdraw the
            // reminders before a stale one is posted.
            self.post_due_reminder(now, &settings, &strings);
        }

        if !(settings.enabled && settings.calendar_notifications) || self.is_recording() {
            return;
        }
        {
            let mut p = self.prompts.lock();
            if p.last_poll.is_some_and(|t| now - t < MINUTE_MS && now >= t) {
                return;
            }
            p.last_poll = Some(now);
        }
        // The (slow) lookup runs without `op`, so it never holds up a start
        // or stop.
        let events = match self
            .deps
            .calendar
            .events_between(now - schedule::PROMPT_WINDOW_MS, now + MINUTE_MS)
        {
            Ok(e) => e,
            Err(e) => {
                log::debug!("meetings: calendar poll failed: {e:#}");
                return;
            }
        };
        // Re-checked, decided and posted under `op`: a recording started
        // meanwhile gets no prompt (and one starting now withdraws it after).
        let _op = self.op.lock();
        if self.is_recording() {
            return;
        }
        let to_post: Vec<CalendarEvent> = {
            let mut p = self.prompts.lock();
            let list: Vec<CalendarEvent> =
                schedule::events_to_prompt(&events, now, |id| p.prompted.contains_key(id))
                    .into_iter()
                    .cloned()
                    .collect();
            for e in &list {
                p.prompted.insert(e.id.clone(), e.start_at);
            }
            p.prompted
                .retain(|_, start| now - *start < 24 * 60 * MINUTE_MS);
            p.seen = events.clone();
            list
        };
        for e in to_post {
            let title = strings.take_notes_for.replace("{{title}}", &e.title);
            let body = format!(
                "{}–{}",
                local_time(e.start_at, "%H:%M"),
                local_time(e.end_at, "%H:%M")
            );
            let actions = [NotificationAction {
                id: ACTION_START.into(),
                title: strings.start.clone(),
            }];
            if let Err(err) =
                self.deps
                    .notifier
                    .post(&prompt_notification_id(&e.id), &title, &body, &actions)
            {
                log::warn!("meetings: calendar prompt failed: {err:#}");
            }
        }
    }

    /// Post the "still recording" reminder if one is due. Callers hold `op`.
    fn post_due_reminder(
        &self,
        now: i64,
        settings: &MeetingSettings,
        strings: &NotificationStrings,
    ) {
        let due = {
            let mut session = self.session.lock();
            session.as_mut().and_then(|s| {
                let st = reminder_state(s, settings.reminder_interval_minutes);
                schedule::reminder_due(now, &st).then(|| {
                    s.last_reminder_at = Some(now);
                    (s.meeting_id, s.title.clone())
                })
            })
        };
        if let Some((id, title)) = due {
            let actions = [
                NotificationAction {
                    id: ACTION_STOP.into(),
                    title: strings.stop.clone(),
                },
                NotificationAction {
                    id: ACTION_KEEP_GOING.into(),
                    title: strings.keep_going.clone(),
                },
            ];
            if let Err(e) = self.deps.notifier.post(
                &reminder_notification_id(id),
                &strings.still_recording,
                &title,
                &actions,
            ) {
                log::warn!("meetings: reminder notification failed: {e:#}");
            }
        }
    }

    /// Meetings left in `Recording` by a crash: mark them `Recorded`, with
    /// `ended_at` = the newest track file's last-write time. A stop whose row
    /// update failed is finished with its own end time and error instead.
    /// Returns their ids.
    /// Runs under the start/stop lock, so a recording that is just starting
    /// (already in the table, not yet the active session) is never taken for
    /// one.
    ///
    /// The caller ([`crate::meetings::manager::MeetingsManager::start_background`])
    /// then queues each one for processing, like a normal stop; if queueing
    /// fails (e.g. the meeting model is missing) the meeting stays `Recorded`
    /// and the UI offers "Process".
    pub fn recover_interrupted(&self) -> Result<Vec<MeetingId>> {
        let _op = self.op.lock();
        let active = self.current_meeting_id();
        let mut ids = Vec::new();
        // Returned (not reported through `on_recording_finished`): the
        // caller queues them with the rest.
        let (finished, error) = self.finish_pending();
        ids.extend(finished);
        if let Some(e) = error {
            log::warn!("meetings: finishing a stopped recording failed: {e:#}");
        }
        for m in self.deps.repo.list_meetings()? {
            if m.status != MeetingStatus::Recording || Some(m.id) == active || self.is_pending(m.id)
            {
                continue;
            }
            let last_write = [&m.mic_audio_path, &m.system_audio_path]
                .into_iter()
                .flatten()
                .filter_map(|rel| audio_store::resolve_track_path(&self.deps.audio_dir, rel).ok())
                .filter_map(|p| audio_store::file_mtime_ms(&p))
                .max();
            let ended_at = last_write.unwrap_or(m.started_at).max(m.started_at);
            if !self.deps.repo.finish_recording(m.id, ended_at, None)? {
                continue;
            }
            log::warn!("meetings: recovered interrupted recording {}", m.id);
            (self.deps.on_meeting_changed)(m.id);
            ids.push(m.id);
        }
        Ok(ids)
    }

    /// Decode `[t0_ms, t1_ms)` of one track.
    pub fn segment_audio(
        &self,
        meeting_id: MeetingId,
        track: Track,
        t0_ms: i64,
        t1_ms: i64,
    ) -> Result<Vec<f32>> {
        let meeting = self
            .deps
            .repo
            .get_meeting(meeting_id)?
            .ok_or_else(|| anyhow!("no meeting {meeting_id}"))?;
        let rel = match track {
            Track::Mic => meeting.mic_audio_path,
            Track::System => meeting.system_audio_path,
        }
        .ok_or_else(|| anyhow!("the audio of this meeting was deleted"))?;
        let path = audio_store::resolve_track_path(&self.deps.audio_dir, &rel)?;
        let key = (self.deps.data_key)()?;
        audio_store::TrackReader::open(&path, &key)?.read_range(t0_ms, t1_ms)
    }

    /// Apply the retention setting at `now`. `is_busy` says whether the
    /// processor has a meeting queued or running (those are skipped).
    /// Returns files deleted.
    pub fn apply_retention(&self, now: i64, is_busy: &dyn Fn(MeetingId) -> bool) -> Result<usize> {
        let days = (self.deps.settings)().audio_retention_days;
        let outcome = audio_store::apply_retention(
            self.deps.repo.as_ref(),
            &self.deps.audio_dir,
            days,
            now,
            is_busy,
        )?;
        for id in &outcome.meetings {
            (self.deps.on_meeting_changed)(*id);
        }
        Ok(outcome.files_deleted)
    }
}

fn reminder_state(s: &Session, interval_minutes: u32) -> ReminderState {
    ReminderState {
        started_at: s.started_at,
        scheduled_end_at: s.scheduled_end_at,
        last_reminder_at: s.last_reminder_at,
        interval_minutes,
    }
}

/// What the writer thread needs from a track file ([`TrackWriter`]; tests
/// use a failing fake).
trait TrackOutput: Send {
    fn push(&mut self, samples: &[f32]) -> Result<()>;
    fn finish(self) -> Result<TrackSummary>;
}

impl TrackOutput for TrackWriter {
    fn push(&mut self, samples: &[f32]) -> Result<()> {
        TrackWriter::push(self, samples)
    }
    fn finish(self) -> Result<TrackSummary> {
        TrackWriter::finish(self)
    }
}

/// Writes both tracks until `Finish`. A track whose write fails is closed
/// right there and never written again: appending after a possibly torn
/// record would misalign every later one, while a torn *final* record just
/// reads as the end of the file. Its outcome is the write error.
fn writer_loop<W: TrackOutput>(rx: Receiver<WriterMsg>, mic: W, sys: W) -> WriterOutcome {
    let mut tracks: [Result<W>; 2] = [Ok(mic), Ok(sys)];
    for msg in rx {
        match msg {
            WriterMsg::Samples(track, samples) => {
                let slot = match track {
                    Track::Mic => &mut tracks[0],
                    Track::System => &mut tracks[1],
                };
                let Ok(w) = slot else { continue };
                if let Err(e) = w.push(&samples) {
                    log::error!(
                        "meetings: writing the {} track failed, stopping it: {e:#}",
                        track.as_str()
                    );
                    // Drops (closes) the writer without its end marker.
                    *slot = Err(e.context("recording stopped early"));
                }
            }
            WriterMsg::Finish => break,
        }
    }
    let [mic, sys] = tracks;
    (mic.and_then(W::finish), sys.and_then(W::finish))
}

pub fn now_ms() -> i64 {
    audio_store::system_time_ms(std::time::SystemTime::now())
}

// ---------------------------------------------------------------------------
// Scheduler thread
// ---------------------------------------------------------------------------

/// Background thread calling [`MeetingRecorder::tick`] every 15 s.
///
/// Dropping it signals the thread to stop and returns without joining: a
/// tick can wait on the recorder's op lock (a stop finishing) or a calendar
/// lookup, and the drop may run on the UI thread. The thread exits after its
/// current tick, never ticks again, and a scheduler started meanwhile waits
/// for that tick to end (the recorder's `ticking` lock).
pub struct Scheduler {
    stop: Arc<(Mutex<bool>, Condvar)>,
}

pub const TICK: Duration = Duration::from_secs(15);

impl Scheduler {
    pub fn start(recorder: Arc<MeetingRecorder>) -> Result<Self> {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("meetings-scheduler".into())
            .spawn(move || {
                let (lock, cv) = &*thread_stop;
                loop {
                    {
                        let _ticking = recorder.ticking.lock();
                        // Checked under `ticking`: once dropped, no tick
                        // starts after the next scheduler's.
                        if *lock.lock() {
                            break;
                        }
                        recorder.tick(now_ms());
                    }
                    let mut stopped = lock.lock();
                    if !*stopped {
                        cv.wait_for(&mut stopped, TICK);
                    }
                    if *stopped {
                        break;
                    }
                }
            })?;
        Ok(Self { stop })
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        *self.stop.0.lock() = true;
        self.stop.1.notify_all();
    }
}

/// The capture workstream's state inside `MeetingsManager`.
#[derive(Default)]
pub struct CaptureState {
    pub recorder: OnceLock<Arc<MeetingRecorder>>,
    pub scheduler: Mutex<Option<Scheduler>>,
}

#[cfg(test)]
mod tests;
