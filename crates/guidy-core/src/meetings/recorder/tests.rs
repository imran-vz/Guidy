use super::ports::fakes::*;
use super::schedule::MINUTE_MS as M;
use super::*;
use crate::meetings::audio_store::TrackReader;

const KEY: [u8; 32] = [3u8; 32];

struct Rig {
    rec: MeetingRecorder,
    repo: Arc<MemRepo>,
    capture: Arc<FakeCapture>,
    calendar: Arc<FakeCalendar>,
    notifier: Arc<FakeNotifier>,
    settings: Arc<Mutex<MeetingSettings>>,
    states: Arc<Mutex<Vec<RecorderState>>>,
    changed: Arc<Mutex<Vec<MeetingId>>>,
    /// `on_recording_finished` calls.
    finished: Arc<Mutex<Vec<MeetingId>>>,
    dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let repo = Arc::new(MemRepo::default());
    let capture = Arc::new(FakeCapture::default());
    let calendar = Arc::new(FakeCalendar::default());
    let notifier = Arc::new(FakeNotifier::default());
    let settings = Arc::new(Mutex::new(MeetingSettings {
        enabled: true,
        ..Default::default()
    }));
    let states = Arc::new(Mutex::new(Vec::new()));
    let changed = Arc::new(Mutex::new(Vec::new()));
    let finished = Arc::new(Mutex::new(Vec::new()));
    let (s, st, ch, fin) = (
        settings.clone(),
        states.clone(),
        changed.clone(),
        finished.clone(),
    );
    let rec = MeetingRecorder::new(RecorderDeps {
        repo: repo.clone(),
        capture: capture.clone(),
        calendar: calendar.clone(),
        notifier: notifier.clone(),
        data_key: Arc::new(|| Ok(KEY)),
        audio_dir: dir.path().join("meetings/audio"),
        settings: Arc::new(move || s.lock().clone()),
        strings: Arc::new(NotificationStrings::default),
        on_state: Arc::new(move |state: &RecorderState| st.lock().push(state.clone())),
        on_meeting_changed: Arc::new(move |id| ch.lock().push(id)),
        on_recording_finished: Arc::new(move |id| fin.lock().push(id)),
    });
    Rig {
        rec,
        repo,
        capture,
        calendar,
        notifier,
        settings,
        states,
        changed,
        finished,
        dir,
    }
}

fn ev(id: &str, start: i64, end: i64) -> CalendarEvent {
    CalendarEvent {
        id: id.into(),
        title: format!("Event {id}"),
        start_at: start,
        end_at: end,
        meeting_url: Some("https://meet.google.com/abc-defg-hij".into()),
        attendees: vec![Attendee {
            name: Some("Ann".into()),
            email: Some("ann@example.com".into()),
            is_current_user: false,
        }],
    }
}

fn audio_path(r: &Rig, rel: &str) -> PathBuf {
    r.dir.path().join("meetings/audio").join(rel)
}

const T0: i64 = 1_700_000_000_000;

#[test]
fn start_feed_stop_produces_a_recorded_meeting_with_two_tracks() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    assert!(r.rec.is_recording());
    match r.rec.state() {
        RecorderState::Recording {
            meeting_id,
            started_at,
            scheduled_end_at,
            ref title,
        } => {
            assert_eq!(meeting_id, id);
            assert_eq!(started_at, T0);
            assert_eq!(scheduled_end_at, None);
            assert!(title.starts_with("Meeting "), "{title}");
        }
        s => panic!("unexpected {s:?}"),
    }
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recording);
    assert_eq!(
        m.mic_audio_path.as_deref(),
        Some(&*format!("{id}-mic.gopus"))
    );
    assert_eq!(
        m.system_audio_path.as_deref(),
        Some(&*format!("{id}-system.gopus"))
    );

    // 2 s on the mic, 1.5 s of system audio, in callback-sized pieces.
    let mic: Vec<f32> = (0..32_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    let sys: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.11).sin() * 0.3).collect();
    for c in mic.chunks(512) {
        r.capture.feed(Track::Mic, c);
    }
    for c in sys.chunks(480) {
        r.capture.feed(Track::System, c);
    }

    assert_eq!(r.rec.stop(T0 + 2_500).unwrap(), Some(id));
    assert!(!r.rec.is_recording());
    assert_eq!(*r.capture.stops.lock(), 1);
    assert_eq!(r.rec.state(), RecorderState::Idle);
    let states = r.states.lock().clone();
    assert_eq!(states.len(), 2);
    assert!(matches!(states[0], RecorderState::Recording { .. }));
    assert_eq!(states[1], RecorderState::Idle);
    assert_eq!(*r.changed.lock(), vec![id, id]);

    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 2_500));
    assert_eq!(m.error, None);

    let mut mr =
        TrackReader::open(&audio_path(&r, m.mic_audio_path.as_ref().unwrap()), &KEY).unwrap();
    assert!(mr.is_complete());
    assert_eq!(mr.read_all().unwrap().len(), mic.len());
    let mut sr =
        TrackReader::open(&audio_path(&r, m.system_audio_path.as_ref().unwrap()), &KEY).unwrap();
    assert_eq!(sr.read_all().unwrap().len(), sys.len());

    // Playback goes through the same files.
    assert_eq!(
        r.rec
            .segment_audio(id, Track::Mic, 500, 1000)
            .unwrap()
            .len(),
        8_000
    );

    // Stopping again is a no-op.
    assert_eq!(r.rec.stop(T0 + 3_000).unwrap(), None);
}

#[test]
fn rename_while_recording_survives_stop() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    // The user renames the meeting mid-recording (a targeted title update).
    r.repo.rows.lock()[0].title = "Renamed by user".into();
    r.rec.stop(T0 + 5_000).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.title, "Renamed by user");
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 5_000));
    assert!(m.mic_audio_path.is_some());
}

#[test]
fn manual_start_links_to_a_current_event_without_a_meeting_link() {
    let r = rig();
    let mut office = ev("office", T0 - 5 * M, T0 + 25 * M);
    office.meeting_url = None;
    office.title = "In-person 1:1".into();
    r.calendar.events.lock().push(office);
    let id = r.rec.start(None, T0).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id.as_deref(), Some("office"));
    assert_eq!(m.title, "In-person 1:1");
    r.rec.stop(T0 + 1_000).unwrap();
}

#[test]
fn second_start_is_refused() {
    let r = rig();
    r.rec.start(None, T0).unwrap();
    assert!(r.rec.start(None, T0 + 1).is_err());
    assert_eq!(r.repo.list_meetings().unwrap().len(), 1);
}

#[test]
fn manual_start_links_to_the_event_happening_now() {
    let r = rig();
    *r.calendar.events.lock() = vec![
        ev("past", T0 - 90 * M, T0 - 60 * M),
        ev("now", T0 - 5 * M, T0 + 25 * M),
    ];
    let id = r.rec.start(None, T0).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id.as_deref(), Some("now"));
    assert_eq!(m.title, "Event now");
    assert_eq!(m.calendar_event.as_ref().unwrap().attendees.len(), 1);
    assert!(matches!(
        r.rec.state(),
        RecorderState::Recording { scheduled_end_at: Some(e), .. } if e == T0 + 25 * M
    ));
}

#[test]
fn explicit_event_id_is_linked_and_snapshotted() {
    let r = rig();
    *r.calendar.events.lock() = vec![ev("a", T0 + 10 * M, T0 + 40 * M), ev("b", T0 - M, T0 + M)];
    let id = r.rec.start(Some("a".into()), T0).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id.as_deref(), Some("a"));
    assert_eq!(m.calendar_event.unwrap().end_at, T0 + 40 * M);
    r.rec.stop(T0 + M).unwrap();

    // An id the calendar doesn't know is a plain start (nothing on now).
    let id = r.rec.start(Some("gone".into()), T0 + 2 * M).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id, None);
    assert!(m.calendar_event.is_none());
    assert!(m.title.starts_with("Meeting "), "{}", m.title);
}

#[test]
fn a_late_prompt_start_is_not_linked_to_its_ended_event() {
    let r = rig();
    *r.calendar.events.lock() = vec![ev("sync", T0, T0 + 30 * M)];
    r.rec.tick(T0 + 30_000);
    assert_eq!(r.notifier.posted.lock().len(), 1);
    let end = T0 + 30 * M;

    // Clicked within the grace period after the event: linked.
    let id = r
        .rec
        .start(Some("sync".into()), end + schedule::START_LINK_GRACE_MS)
        .unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id.as_deref(), Some("sync"));
    r.rec.stop(end + schedule::START_LINK_GRACE_MS + M).unwrap();

    // Hours later: a plain start, linked to whatever is on now.
    let later = end + 3 * 60 * M;
    r.calendar
        .events
        .lock()
        .push(ev("now", later - 5 * M, later + 25 * M));
    let id = r.rec.start(Some("sync".into()), later).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.calendar_event_id.as_deref(), Some("now"));
    assert_eq!(m.title, "Event now");
}

#[test]
fn failed_capture_leaves_nothing_behind() {
    let r = rig();
    *r.capture.fail.lock() = true;
    let err = r.rec.start(None, T0).unwrap_err();
    assert!(format!("{err:#}").contains("no permission"));
    assert!(r.repo.list_meetings().unwrap().is_empty());
    assert!(!r.rec.is_recording());
    assert!(r.states.lock().is_empty());
    let leftovers = std::fs::read_dir(r.dir.path().join("meetings/audio"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
}

#[test]
fn failed_audio_path_update_removes_the_created_files() {
    let r = rig();
    *r.repo.fail_set_audio_paths.lock() = true;
    let err = r.rec.start(None, T0).unwrap_err();
    assert!(format!("{err:#}").contains("database is locked"));
    assert!(r.repo.list_meetings().unwrap().is_empty());
    assert!(!r.rec.is_recording());
    // The track files were created before the row update failed.
    let leftovers: Vec<_> = std::fs::read_dir(r.dir.path().join("meetings/audio"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_failed_start_that_cant_be_cleaned_up_is_not_recovered() {
    let r = rig();
    *r.capture.fail.lock() = true;
    *r.repo.fail_delete.lock() = true;
    assert!(r.rec.start(None, T0).is_err());
    // The row survives, failed with the error: not a crash leftover, and not
    // offered for processing (it has no audio).
    let rows = r.repo.list_meetings().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, MeetingStatus::Failed);
    assert_eq!(rows[0].ended_at, Some(T0));
    assert!(rows[0].error.as_deref().unwrap().contains("no permission"));
    assert!(r.rec.recover_interrupted().unwrap().is_empty());
}

#[test]
fn a_stream_dying_mid_recording_is_the_meetings_error() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    r.capture.feed(Track::Mic, &vec![0.1; 16_000]);
    r.capture
        .fail_stream(Track::System, "error: system audio tap stopped");
    // Repeats of the same failure are kept once.
    r.capture
        .fail_stream(Track::System, "error: system audio tap stopped");
    r.capture
        .fail_stream(Track::Mic, "error: microphone restart failed");
    r.capture.feed(Track::Mic, &vec![0.1; 16_000]);
    r.rec.stop(T0 + 2_000).unwrap();

    // Still recorded (the audio up to the failure is there), with the error.
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(
        m.error.as_deref(),
        Some(
            "system capture: error: system audio tap stopped; \
             mic capture: error: microphone restart failed"
        )
    );
}

/// A track output whose `n`th push fails.
struct FailingWriter {
    fail_at: Option<usize>,
    pushes: Arc<Mutex<usize>>,
    finished: Arc<Mutex<bool>>,
}

impl TrackOutput for FailingWriter {
    fn push(&mut self, _samples: &[f32]) -> Result<()> {
        let mut n = self.pushes.lock();
        *n += 1;
        if Some(*n) == self.fail_at {
            anyhow::bail!("disk full");
        }
        Ok(())
    }
    fn finish(self) -> Result<TrackSummary> {
        *self.finished.lock() = true;
        Ok(TrackSummary { samples: 0 })
    }
}

#[test]
fn a_failed_write_stops_that_track_and_is_reported() {
    let writer = |fail_at| FailingWriter {
        fail_at,
        pushes: Arc::default(),
        finished: Arc::default(),
    };
    let (mic, sys) = (writer(Some(2)), writer(None));
    let (mic_pushes, mic_finished) = (mic.pushes.clone(), mic.finished.clone());
    let (sys_pushes, sys_finished) = (sys.pushes.clone(), sys.finished.clone());
    let (tx, rx) = channel();
    let thread = std::thread::spawn(move || writer_loop(rx, mic, sys));
    for _ in 0..4 {
        tx.send(WriterMsg::Samples(Track::Mic, vec![0.0; 320]))
            .unwrap();
        tx.send(WriterMsg::Samples(Track::System, vec![0.0; 320]))
            .unwrap();
    }
    tx.send(WriterMsg::Finish).unwrap();
    let (mic, sys) = thread.join().unwrap();

    // Nothing is appended after the failed (possibly torn) write, and the
    // track isn't finished (no end marker after a torn record).
    assert_eq!(*mic_pushes.lock(), 2);
    assert!(!*mic_finished.lock());
    let err = format!("{:#}", mic.unwrap_err());
    assert!(err.contains("disk full"), "{err}");
    // The other track is unaffected.
    assert_eq!(*sys_pushes.lock(), 4);
    assert!(*sys_finished.lock());
    assert!(sys.is_ok());
}

#[test]
fn missing_key_fails_before_creating_a_meeting() {
    let mut r = rig();
    r.rec.deps.data_key = Arc::new(|| anyhow::bail!("keychain locked"));
    assert!(r.rec.start(None, T0).is_err());
    assert!(r.repo.list_meetings().unwrap().is_empty());
}

#[test]
fn reminders_for_a_calendar_meeting() {
    let r = rig();
    *r.calendar.events.lock() = vec![ev("sync", T0 - M, T0 + 30 * M)];
    let id = r.rec.start(None, T0).unwrap();
    let end = T0 + 30 * M;
    assert_eq!(r.rec.next_reminder_at(), Some(end + 5 * M));

    r.rec.tick(end + 4 * M);
    assert!(r.notifier.posted.lock().is_empty());
    r.rec.tick(end + 5 * M);
    {
        let posted = r.notifier.posted.lock();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].id, reminder_notification_id(id));
        assert_eq!(posted[0].title, "Guidy is still recording");
        assert_eq!(posted[0].body, "Event sync");
        assert_eq!(posted[0].actions, [ACTION_STOP, ACTION_KEEP_GOING]);
    }
    // Not again until the interval (10 min) has passed.
    r.rec.tick(end + 14 * M);
    assert_eq!(r.notifier.posted.lock().len(), 1);
    r.rec.tick(end + 15 * M);
    assert_eq!(r.notifier.posted.lock().len(), 2);

    // Keep going at +17: next at +27.
    r.rec.snooze(id, end + 17 * M);
    r.rec.tick(end + 26 * M);
    assert_eq!(r.notifier.posted.lock().len(), 2);
    r.rec.tick(end + 27 * M);
    assert_eq!(r.notifier.posted.lock().len(), 3);
    // Recording never stops by itself.
    assert!(r.rec.is_recording());
}

#[test]
fn reminders_for_a_manual_session_use_the_interval_setting() {
    let r = rig();
    r.settings.lock().reminder_interval_minutes = 5;
    r.rec.start(None, T0).unwrap();
    r.rec.tick(T0 + 59 * M);
    assert!(r.notifier.posted.lock().is_empty());
    r.rec.tick(T0 + 60 * M);
    r.rec.tick(T0 + 64 * M);
    r.rec.tick(T0 + 65 * M);
    assert_eq!(r.notifier.posted.lock().len(), 2);
}

#[test]
fn calendar_prompts_once_at_start_time() {
    let r = rig();
    *r.calendar.events.lock() = vec![ev("sync", T0, T0 + 30 * M)];
    r.rec.tick(T0 - 30_000);
    assert!(r.notifier.posted.lock().is_empty());
    r.rec.tick(T0 + 20_000); // polled at most once a minute: skipped
    assert!(r.notifier.posted.lock().is_empty());
    r.rec.tick(T0 + 31_000);
    {
        let posted = r.notifier.posted.lock();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].id, prompt_notification_id("sync"));
        assert_eq!(posted[0].title, "Take notes for Event sync?");
        assert_eq!(posted[0].actions, [ACTION_START]);
    }
    r.rec.tick(T0 + 95_000);
    assert_eq!(r.notifier.posted.lock().len(), 1, "prompted once per event");

    // "Start" from the prompt links the event (snapshot from the poll cache,
    // even if the calendar has since changed).
    r.calendar.events.lock().clear();
    let cmd = parse_notification(&prompt_notification_id("sync"), ACTION_START);
    assert_eq!(cmd, NotificationCommand::Start("sync".into()));
    let id = r.rec.start(Some("sync".into()), T0 + 2 * M).unwrap();
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.title, "Event sync");
    assert_eq!(m.calendar_event.unwrap().end_at, T0 + 30 * M);
}

#[test]
fn calendar_prompts_respect_settings_and_recording() {
    let r = rig();
    *r.calendar.events.lock() = vec![ev("a", T0, T0 + 30 * M), ev("b", T0 + 5 * M, T0 + 9 * M)];
    r.settings.lock().calendar_notifications = false;
    r.rec.tick(T0);
    r.settings.lock().calendar_notifications = true;
    r.settings.lock().enabled = false;
    r.rec.tick(T0 + 10);
    assert!(r.notifier.posted.lock().is_empty());
    assert_eq!(*r.calendar.queries.lock(), 0);

    r.settings.lock().enabled = true;
    r.rec.start(None, T0 + M).unwrap();
    r.rec.tick(T0 + 5 * M);
    assert!(
        r.notifier.posted.lock().is_empty(),
        "no prompts while recording"
    );
}

#[test]
fn notification_ids_round_trip() {
    assert_eq!(
        parse_notification(&reminder_notification_id(42), ACTION_STOP),
        NotificationCommand::Stop(42)
    );
    assert_eq!(
        parse_notification(&reminder_notification_id(42), ACTION_KEEP_GOING),
        NotificationCommand::KeepGoing(42)
    );
    assert_eq!(
        parse_notification(&reminder_notification_id(42), "default"),
        NotificationCommand::Ignore
    );
    assert_eq!(
        parse_notification(&prompt_notification_id("x.y@1700"), ACTION_START),
        NotificationCommand::Start("x.y@1700".into())
    );
    assert_eq!(
        parse_notification("other", ACTION_START),
        NotificationCommand::Ignore
    );
    assert_eq!(
        parse_notification("guidy.meetings.reminder.abc", ACTION_STOP),
        NotificationCommand::Ignore
    );
}

#[test]
fn interrupted_recordings_are_recovered() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    r.capture.feed(Track::Mic, &vec![0.1; 40_000]);
    // Simulate a crash: a fresh recorder over the same repo and files.
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    let fresh = MeetingRecorder::new(r.rec.deps.clone());
    // A finished meeting is left alone.
    let done = r
        .repo
        .insert_meeting(&Meeting {
            status: MeetingStatus::Processed,
            ended_at: Some(5),
            ..m.clone()
        })
        .unwrap();

    // The recorder that is actually recording never "recovers" its own meeting.
    assert!(r.rec.recover_interrupted().unwrap().is_empty());

    assert_eq!(fresh.recover_interrupted().unwrap(), vec![id]);
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    // ended_at = the files' last write (now-ish, well after the fake T0).
    let ended = m.ended_at.unwrap();
    assert!(ended > T0 && ended <= now_ms(), "{ended}");
    assert_eq!(r.repo.get_meeting(done).unwrap().unwrap().ended_at, Some(5));
}

#[test]
fn a_transient_database_error_at_stop_is_retried() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = 2;
    assert_eq!(r.rec.stop(T0 + 1_000).unwrap(), Some(id));
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 1_000));
    assert_eq!(r.rec.state(), RecorderState::Idle);
}

#[test]
fn a_failed_finish_at_stop_is_retried_by_the_next_stop() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    r.capture.fail_stream(Track::System, "tap stopped");
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    // Capture is stopped and the UI is idle, but the row is still recording.
    assert!(!r.rec.is_recording());
    assert_eq!(r.rec.state(), RecorderState::Idle);
    assert_eq!(*r.capture.stops.lock(), 1);
    assert_eq!(
        r.repo.get_meeting(id).unwrap().unwrap().status,
        MeetingStatus::Recording
    );
    // Still failing: stays pending.
    assert!(r.rec.stop(T0 + 5_000).is_err());
    *r.repo.fail_finish.lock() = 0;
    assert!(r.finished.lock().is_empty());
    // A later stop (while idle) finishes it, with the original end and error,
    // and reports it so it gets queued for processing.
    assert_eq!(r.rec.stop(T0 + 9_000).unwrap(), None);
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 1_000));
    assert_eq!(m.error.as_deref(), Some("system capture: tap stopped"));
    assert!(r.changed.lock().contains(&id));
    assert_eq!(*r.finished.lock(), vec![id]);
    // Nothing left to retry.
    assert_eq!(r.rec.stop(T0 + 10_000).unwrap(), None);
}

#[test]
fn a_failed_finish_at_stop_is_retried_by_tick_start_and_recovery() {
    // The scheduler tick.
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    *r.repo.fail_finish.lock() = 0;
    r.rec.tick(T0 + 2_000);
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 1_000));
    // Reported, so it is queued for processing like a normal stop.
    assert_eq!(*r.finished.lock(), vec![id]);

    // The next start.
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    *r.repo.fail_finish.lock() = 0;
    let next = r.rec.start(None, T0 + 2_000).unwrap();
    assert_ne!(next, id);
    let m = r.repo.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.status, MeetingStatus::Recorded);
    assert_eq!(m.ended_at, Some(T0 + 1_000));
    assert_eq!(*r.finished.lock(), vec![id]);
    // The new recording's own stop is returned, not reported.
    assert_eq!(r.rec.stop(T0 + 3_000).unwrap(), Some(next));
    assert_eq!(*r.finished.lock(), vec![id]);

    // Recovery: finished with the stop's end time, not the files' mtime,
    // and returned so it gets queued for processing.
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    *r.repo.fail_finish.lock() = 0;
    assert_eq!(r.rec.recover_interrupted().unwrap(), vec![id]);
    assert_eq!(
        r.repo.get_meeting(id).unwrap().unwrap().ended_at,
        Some(T0 + 1_000)
    );
    // Returned to the caller, which queues it: not reported twice.
    assert!(r.finished.lock().is_empty());
}

#[test]
fn a_stop_finished_by_its_trailing_retry_is_returned_not_reported() {
    let r = rig();
    let a = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    let b = r.rec.start(None, T0 + 2_000).unwrap();
    // b's own attempt and both retries fail; the pending retry after them
    // (a, then b) works.
    *r.repo.fail_finish.lock() = 1 + FINISH_RETRY_DELAYS.len();
    assert_eq!(r.rec.stop(T0 + 3_000).unwrap(), Some(b));
    assert!(!r.rec.has_pending_finish());
    for (id, end) in [(a, T0 + 1_000), (b, T0 + 3_000)] {
        let m = r.repo.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Recorded);
        assert_eq!(m.ended_at, Some(end), "meeting {id}");
    }
    // Only the earlier stop is reported; b is returned (queued once).
    assert_eq!(*r.finished.lock(), vec![a]);
}

#[test]
fn every_failed_finish_is_kept_and_retried_on_its_own() {
    let r = rig();
    let a = r.rec.start(None, T0).unwrap();
    *r.repo.fail_finish.lock() = usize::MAX;
    assert!(r.rec.stop(T0 + 1_000).is_err());
    // The user can still record while it keeps failing.
    let b = r.rec.start(None, T0 + 2_000).unwrap();
    assert!(r.rec.stop(T0 + 3_000).is_err());
    let c = r.rec.start(None, T0 + 4_000).unwrap();
    assert!(r.rec.stop(T0 + 5_000).is_err());
    assert!(r.rec.has_pending_finish());

    // Only the first retry fails (meeting a): the others still finish.
    *r.repo.fail_finish.lock() = 1;
    r.rec.tick(T0 + 6_000);
    assert_eq!(*r.finished.lock(), vec![b, c]);
    assert!(r.rec.has_pending_finish());
    r.rec.tick(T0 + 7_000);
    assert_eq!(*r.finished.lock(), vec![b, c, a]);
    assert!(!r.rec.has_pending_finish());
    for (id, end) in [(a, T0 + 1_000), (b, T0 + 3_000), (c, T0 + 5_000)] {
        let m = r.repo.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Recorded);
        assert_eq!(m.ended_at, Some(end), "meeting {id}");
    }
}

#[test]
fn a_stop_during_a_reminder_post_waits_for_it() {
    let r = rig();
    let rec = Arc::new(MeetingRecorder::new(r.rec.deps.clone()));
    let id = rec.start(None, T0).unwrap();
    let stopper = Arc::new(Mutex::new(None));
    let (weak, slot, states) = (Arc::downgrade(&rec), stopper.clone(), r.states.clone());
    *r.notifier.during_post.lock() = Some(Box::new(move || {
        // "Stop" pressed while the reminder is being posted.
        let rec = weak.upgrade().unwrap();
        let stop = std::thread::spawn(move || rec.stop(T0 + 61 * M));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!stop.is_finished(), "the stop must wait for the post");
        // Not idle yet, so the reminders are withdrawn after this one.
        assert!(!states.lock().contains(&RecorderState::Idle));
        *slot.lock() = Some(stop);
    }));
    rec.tick(T0 + 60 * M);
    let stop = stopper.lock().take().expect("reminder posted");
    assert_eq!(stop.join().unwrap().unwrap(), Some(id));
    assert_eq!(r.notifier.posted.lock().len(), 1);
    assert_eq!(r.states.lock().last(), Some(&RecorderState::Idle));
}

#[test]
fn a_recording_started_during_the_calendar_poll_gets_no_prompt() {
    let r = rig();
    let rec = Arc::new(MeetingRecorder::new(r.rec.deps.clone()));
    *r.calendar.events.lock() = vec![ev("sync", T0, T0 + 30 * M)];
    let weak = Arc::downgrade(&rec);
    *r.calendar.during_query.lock() = Some(Box::new(move || {
        // "Start" from the UI while the calendar is being polled.
        weak.upgrade().unwrap().start(None, T0 + 30_000).unwrap();
    }));
    rec.tick(T0 + 30_000);
    assert!(rec.is_recording());
    assert!(r.notifier.posted.lock().is_empty());
}

#[test]
fn a_meeting_deleted_while_recording_is_not_retried() {
    let r = rig();
    let id = r.rec.start(None, T0).unwrap();
    r.repo.rows.lock().retain(|m| m.id != id);
    assert!(r.rec.stop(T0 + 1_000).is_err());
    assert_eq!(r.rec.state(), RecorderState::Idle);
    assert_eq!(r.rec.stop(T0 + 2_000).unwrap(), None);
}

#[test]
fn recovery_waits_for_a_start_or_stop_in_progress() {
    let r = rig();
    let id = r
        .repo
        .insert_meeting(&Meeting {
            id: 0,
            title: "x".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: T0,
            ended_at: None,
            mic_audio_path: None,
            system_audio_path: None,
            status: MeetingStatus::Recording,
            error: None,
            processing_stage: None,
        })
        .unwrap();
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let op = r.rec.op.lock();
        let recovery = scope.spawn(|| {
            let ids = r.rec.recover_interrupted().unwrap();
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            ids
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!done.load(std::sync::atomic::Ordering::SeqCst));
        drop(op);
        assert_eq!(recovery.join().unwrap(), vec![id]);
    });
}

#[test]
fn recovery_without_files_uses_the_start_time() {
    let r = rig();
    let id = r
        .repo
        .insert_meeting(&Meeting {
            id: 0,
            title: "x".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: T0,
            ended_at: None,
            mic_audio_path: Some("9-mic.gopus".into()),
            system_audio_path: None,
            status: MeetingStatus::Recording,
            error: None,
            processing_stage: None,
        })
        .unwrap();
    assert_eq!(r.rec.recover_interrupted().unwrap(), vec![id]);
    assert_eq!(r.repo.get_meeting(id).unwrap().unwrap().ended_at, Some(T0));
}

#[test]
fn scheduler_thread_ticks_and_stops() {
    let r = rig();
    let rec = Arc::new(MeetingRecorder::new(r.rec.deps.clone()));
    // Calendar polled on the first tick (enabled + notifications default on).
    let s = Scheduler::start(rec).unwrap();
    for _ in 0..100 {
        if *r.calendar.queries.lock() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(*r.calendar.queries.lock() > 0);
    drop(s);
}

#[test]
fn dropping_the_scheduler_never_waits_for_its_tick() {
    let r = rig();
    let rec = Arc::new(MeetingRecorder::new(r.rec.deps.clone()));
    // A stop finishing: the tick blocks on `op`.
    let op = rec.op.lock();
    let old = Scheduler::start(rec.clone()).unwrap();
    for _ in 0..200 {
        if rec.ticking.is_locked() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(rec.ticking.is_locked(), "the old scheduler is mid-tick");
    let t = std::time::Instant::now();
    drop(old);
    assert!(t.elapsed() < Duration::from_millis(100), "drop didn't join");

    // A new scheduler waits for the old tick; the old one never ticks again.
    let new = Scheduler::start(rec.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(*r.calendar.queries.lock(), 0, "nothing ticked past `op`");
    drop(op);
    for _ in 0..200 {
        if *r.calendar.queries.lock() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // The old tick polled; the new one is within the poll interval.
    assert_eq!(*r.calendar.queries.lock(), 1);
    drop(new);
    // Both threads exit (each held a recorder reference).
    for _ in 0..200 {
        if Arc::strong_count(&rec) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(Arc::strong_count(&rec), 1);
}

/// The capture section of `MeetingsManager`, against the real (in-memory
/// keyed) store and test secrets backend. Capture itself is not started here:
/// that needs hardware.
mod manager_section {
    use super::*;
    use crate::host::testing::TestApp;
    use crate::meetings::manager::MeetingsManager;

    fn recorded_meeting(mgr: &MeetingsManager, ended_at: i64) -> MeetingId {
        let id = mgr
            .store()
            .insert_meeting(&Meeting {
                id: 0,
                title: "Sync".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: ended_at - 2_000,
                ended_at: Some(ended_at),
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: Some("done".into()),
            })
            .unwrap();
        let key = crate::secrets::meetings_data_key().unwrap();
        let rel = audio_store::track_file_name(id, Track::Mic);
        let mut w = TrackWriter::create(&mgr.audio_dir().join(&rel), &key).unwrap();
        w.push(&vec![0.25; 32_000]).unwrap();
        w.finish().unwrap();
        mgr.store().set_audio_paths(id, Some(&rel), None).unwrap();
        id
    }

    #[test]
    fn idle_manager_basics() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        assert_eq!(mgr.recorder_state(), RecorderState::Idle);
        assert_eq!(mgr.stop_recording().unwrap(), None);
        // Unknown / stale notification responses are ignored.
        mgr.handle_notification("something.else", "stop");
        mgr.handle_notification(&reminder_notification_id(7), ACTION_STOP);
        mgr.handle_notification(&reminder_notification_id(7), ACTION_KEEP_GOING);
        assert_eq!(mgr.recorder_state(), RecorderState::Idle);
    }

    #[test]
    fn segment_audio_and_retention_go_through_the_store() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let fresh = recorded_meeting(&mgr, now_ms());
        let old = recorded_meeting(&mgr, now_ms() - 10 * 24 * 60 * M);

        let audio = mgr.segment_audio(fresh, Track::Mic, 250, 750).unwrap();
        assert_eq!(audio.len(), 8_000);
        assert!(mgr.segment_audio(fresh, Track::System, 0, 100).is_err());

        // Default retention is 3 days: only the old meeting loses its audio.
        assert_eq!(mgr.apply_retention().unwrap(), 1);
        assert!(mgr
            .get_meeting(old)
            .unwrap()
            .unwrap()
            .mic_audio_path
            .is_none());
        assert!(!mgr.audio_dir().join(format!("{old}-mic.gopus")).exists());
        assert!(mgr
            .get_meeting(fresh)
            .unwrap()
            .unwrap()
            .mic_audio_path
            .is_some());
        assert!(mgr.segment_audio(old, Track::Mic, 0, 100).is_err());
        assert!(t
            .events
            .lock()
            .iter()
            .any(|e| e.name == events::MEETING_CHANGED && e.decode::<i64>() == Some(old)));
    }

    #[test]
    fn startup_recovers_interrupted_recordings() {
        let t = TestApp::new();
        let mgr = MeetingsManager::new(&t.app).unwrap();
        let id = recorded_meeting(&mgr, now_ms());
        let mut m = mgr.store().get_meeting(id).unwrap().unwrap();
        m.status = MeetingStatus::Recording;
        m.ended_at = None;
        mgr.store().update_meeting(&m).unwrap();

        assert_eq!(mgr.recorder().recover_interrupted().unwrap(), vec![id]);
        let m = mgr.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Recorded);
        let file_time =
            audio_store::file_mtime_ms(&mgr.audio_dir().join(m.mic_audio_path.unwrap())).unwrap();
        assert_eq!(m.ended_at, Some(file_time.max(m.started_at)));
    }
}
