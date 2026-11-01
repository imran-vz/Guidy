//! The recording floater: what it shows. Pure: driven by recorder states,
//! live-transcript updates, polled input levels and a clock. How it moves
//! is [`crate::logic::floater`].
//!
//! Collapsed it is a rounded rectangle (or wings around the notch) with a waveform
//! and a stop button; expanded it is a panel with the meeting being
//! recorded, which sources are picking up sound, the live transcript and
//! the controls.
//!
//! While nothing records but a call is on in another app or a browser
//! ([`guidy_core::meetings::calls`]), it shows stopped, with a hint to take
//! notes, until the call ends or the hint is dismissed.

use guidy_core::meetings::calls::{
    holds_microphone, Call, CallPresence, Detection, InputApp, PresenceChange,
};
use guidy_core::meetings::types::{
    format_elapsed, LiveLine, LiveStatus, LiveTranscript, LiveUpdate, RecorderState,
    RecordingHealth, SourceHealth, Track,
};
use std::collections::VecDeque;

/// Expanded panel size, in design units.
pub const PANEL: (f64, f64) = (360.0, 500.0);
/// Gap to the screen's edges, in design units.
pub const MARGIN: f64 = 12.0;
/// Where the top edge sits, as a share of the visible height below the
/// menu bar: clear of the top-right corner, where notifications land.
pub const TOP_SHARE: f64 = 0.22;
/// Bars in the waveform (newest on the right).
pub const BARS: usize = 9;
/// Level samples kept per source for its activity meter.
const SOURCE_SAMPLES: usize = 8;
/// A source whose recent peak is under this reads as quiet.
const ACTIVE_LEVEL: f32 = 0.06;
/// Transcript rows kept on screen (older ones scroll out of memory too).
pub const MAX_ROWS: usize = 60;

/// An unanswered offer closes after this long (only the offer: the call goes
/// on, unoffered).
pub const OFFER_TIMEOUT_MS: i64 = 20_000;
/// An unanswered "the call ended" stops the recording after this long.
pub const LEFT_CALL_TIMEOUT_MS: i64 = 60_000;

/// What the floater says about how a recording is doing, the worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthNotice {
    /// No audio for 15 minutes; it stops soon unless kept.
    NoAudio,
    MicDenied,
    SystemDenied,
    MicLost,
    SystemLost,
    LowDisk,
    MicReconnecting,
    SystemReconnecting,
    MicSilent,
    SystemSilent,
    Backlog,
}

impl HealthNotice {
    /// Audio is being lost, or is about to stop being recorded: this can't
    /// wait behind a question. The rest can.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::NoAudio | Self::MicDenied | Self::SystemDenied | Self::MicLost | Self::SystemLost
        )
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::NoAudio => "meetings.widget.health.noAudio",
            Self::MicDenied => "meetings.widget.health.micDenied",
            Self::SystemDenied => "meetings.widget.health.systemDenied",
            Self::MicLost => "meetings.widget.health.micLost",
            Self::SystemLost => "meetings.widget.health.systemLost",
            Self::LowDisk => "meetings.widget.health.lowDisk",
            Self::MicReconnecting => "meetings.widget.health.micReconnecting",
            Self::SystemReconnecting => "meetings.widget.health.systemReconnecting",
            Self::MicSilent => "meetings.widget.health.micSilent",
            Self::SystemSilent => "meetings.widget.health.systemSilent",
            Self::Backlog => "meetings.widget.health.backlog",
        }
    }
}

/// The worst thing to say about `health`, if anything.
pub fn health_notice(health: &RecordingHealth) -> Option<HealthNotice> {
    use HealthNotice::*;
    use SourceHealth as S;
    Some(if health.silence_stop_at.is_some() {
        NoAudio
    } else if health.mic_denied {
        MicDenied
    } else if health.system_denied {
        SystemDenied
    } else if health.mic == S::Lost {
        MicLost
    } else if health.system == S::Lost {
        SystemLost
    } else if health.low_disk {
        LowDisk
    } else if health.mic == S::Reconnecting {
        MicReconnecting
    } else if health.system == S::Reconnecting {
        SystemReconnecting
    } else if health.mic == S::Silent {
        MicSilent
    } else if health.system == S::Silent {
        SystemSilent
    } else if health.backlog {
        Backlog
    } else {
        return None;
    })
}

/// What the call watcher should do after a poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallCommand {
    Start,
    Stop,
}

/// One poll of the call watcher, for [`WidgetModel::apply_call_poll`].
#[derive(Clone, Debug)]
pub struct CallPoll<'a> {
    pub apps: &'a [InputApp],
    /// The call in progress.
    pub current: Option<Call>,
    /// What a call confirmed by this poll leads to.
    pub detection: Option<Detection>,
    pub now: i64,
}

/// Consecutive live lines from the same source, shown as one paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub track: Track,
    pub t0_ms: i64,
    /// Settled text of the row.
    pub text: String,
    /// The last line's text while it may still change.
    pub tentative: String,
}

/// The rows of `lines` (in time order), dropping empty ones; the newest
/// [`MAX_ROWS`].
pub fn rows(lines: &[LiveLine]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for line in lines.iter().filter(|l| !l.text.trim().is_empty()) {
        let text = line.text.trim();
        let row = match rows.last_mut() {
            Some(r) if r.track == line.track && r.tentative.is_empty() => r,
            _ => {
                rows.push(Row {
                    track: line.track,
                    t0_ms: line.t0_ms,
                    text: String::new(),
                    tentative: String::new(),
                });
                rows.last_mut().expect("just pushed")
            }
        };
        let slot = if line.tentative {
            &mut row.tentative
        } else {
            &mut row.text
        };
        if !slot.is_empty() {
            slot.push(' ');
        }
        slot.push_str(text);
    }
    let skip = rows.len().saturating_sub(MAX_ROWS);
    rows.drain(..skip);
    rows
}

#[derive(Clone, Debug)]
pub struct WidgetModel {
    pub recorder: RecorderState,
    pub expanded: bool,
    pub live: LiveTranscript,
    /// A call in another app (offer to take notes while idle).
    pub call: Option<Call>,
    /// The call whose hint was dismissed (or that was recorded), by its
    /// start: it isn't offered again.
    dismissed: Option<i64>,
    /// "Take notes" was clicked and the recording hasn't started yet
    /// (loading the live transcriber takes seconds).
    starting: bool,
    /// The last start from the hint failed (said in the hint until the
    /// next try or the next call).
    start_failed: bool,
    /// When the offer for the current call first showed.
    offer_since: Option<i64>,
    /// The offer's "..." is open: Always and Never instead of Take notes.
    pub more_open: bool,
    /// Apps the call watcher should snooze
    /// ([`guidy_core::meetings::calls::CallDetector::snooze_app`]).
    pending_snoozes: Vec<String>,
    /// The call behind the recording in progress.
    recording_call: Option<Call>,
    presence: CallPresence,
    /// When that call was seen to have ended, while the user is being asked.
    left_at: Option<i64>,
    /// A stop was asked for by the watcher and hasn't reported in.
    stopping: bool,
    /// The call to record once this recording is stopped: its provider is
    /// Always, and it came on while this one's call was ending.
    record_next: Option<Call>,
    /// The waveform (both sources together).
    wave: VecDeque<f32>,
    /// Recent levels per source: mic, system.
    sources: [VecDeque<f32>; 2],
}

impl Default for WidgetModel {
    fn default() -> Self {
        Self {
            recorder: RecorderState::Idle,
            expanded: false,
            live: LiveTranscript::default(),
            call: None,
            dismissed: None,
            starting: false,
            start_failed: false,
            offer_since: None,
            more_open: false,
            pending_snoozes: Vec::new(),
            recording_call: None,
            presence: CallPresence::default(),
            left_at: None,
            stopping: false,
            record_next: None,
            wave: VecDeque::from(vec![0.0; BARS]),
            sources: [
                VecDeque::from(vec![0.0; SOURCE_SAMPLES]),
                VecDeque::from(vec![0.0; SOURCE_SAMPLES]),
            ],
        }
    }
}

impl WidgetModel {
    pub fn visible(&self) -> bool {
        self.recorder.is_recording() || self.hinting()
    }

    /// Stopped, offering to take notes for a call.
    pub fn hinting(&self) -> bool {
        !self.recorder.is_recording() && self.hint_call().is_some()
    }

    /// The call offered notes for, unless dismissed.
    pub fn hint_call(&self) -> Option<&Call> {
        self.call
            .as_ref()
            .filter(|c| Some(c.since) != self.dismissed)
    }

    pub fn set_call(&mut self, call: Option<Call>) {
        if call.as_ref().map(|c| c.since) != self.call.as_ref().map(|c| c.since) {
            self.start_failed = false;
            self.offer_since = None;
            self.more_open = false;
        }
        self.call = call;
    }

    /// "Not now", or the offer timed out: stop offering notes for this call,
    /// and for this app until it has left the microphone. Not once a start is
    /// on its way: that recording is for this call.
    pub fn dismiss_call(&mut self) {
        if self.starting() {
            return;
        }
        if let Some(c) = &self.call {
            self.dismissed = Some(c.since);
            self.pending_snoozes.push(c.app_id.clone());
        }
        self.more_open = false;
    }

    /// How the recording is doing (all healthy when idle).
    pub fn health(&self) -> RecordingHealth {
        match &self.recorder {
            RecorderState::Recording { health, .. } => *health,
            RecorderState::Idle => RecordingHealth::default(),
        }
    }

    /// The source's own state, for its indicator.
    pub fn source_health(&self, track: Track) -> SourceHealth {
        let health = self.health();
        match track {
            Track::Mic => health.mic,
            Track::System => health.system,
        }
    }

    /// What to say beside the floater about the recording, if anything.
    pub fn health_notice(&self) -> Option<HealthNotice> {
        health_notice(&self.health())
    }

    /// A call is going on behind the recording.
    pub fn recording_in_call(&self) -> bool {
        self.recorder.is_recording() && self.recording_call.is_some() && self.left_at.is_none()
    }

    /// The apps to snooze since the last poll.
    pub fn take_snoozes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_snoozes)
    }

    /// The call behind the recording has ended and the user is being asked:
    /// Keep or Stop. Stops by itself at [`LEFT_CALL_TIMEOUT_MS`].
    pub fn left_call(&self) -> Option<&Call> {
        self.left_at.and(self.recording_call.as_ref())
    }

    /// "Keep": the recording goes on, no longer tied to that call.
    pub fn keep_recording(&mut self) {
        self.left_at = None;
        self.recording_call = None;
        self.presence = CallPresence::default();
    }

    /// A poll with the feature off: nothing is followed from here on. A
    /// recording goes on until it's stopped by hand, but one the user is being
    /// asked about still stops if they don't answer.
    pub fn apply_idle_poll(&mut self, now: i64) -> Option<CallCommand> {
        self.keep_silence_warning_in_view();
        self.set_call(None);
        self.record_next = None;
        if self.left_at.is_none() {
            self.keep_recording();
            return None;
        }
        self.stop_if_unanswered(now)
    }

    /// The no-audio warning's Keep shows beside the floater, not over the open
    /// panel: it can't be reopened over it either.
    fn keep_silence_warning_in_view(&mut self) {
        if self.health().silence_stop_at.is_some() {
            self.expanded = false;
        }
    }

    fn stop_if_unanswered(&mut self, now: i64) -> Option<CallCommand> {
        let timed_out = self
            .left_at
            .is_some_and(|at| now - at >= LEFT_CALL_TIMEOUT_MS);
        (timed_out && !std::mem::replace(&mut self.stopping, true)).then_some(CallCommand::Stop)
    }

    /// Take in a poll of the call watcher; returns what it should do.
    pub fn apply_call_poll(&mut self, poll: CallPoll<'_>) -> Option<CallCommand> {
        let CallPoll {
            apps,
            current,
            detection,
            now,
        } = poll;
        let record = matches!(detection, Some(Detection::Record(_)));
        self.keep_silence_warning_in_view();
        self.set_call(current);
        if self.recorder.is_recording() {
            return self.follow_recording_call(apps, record, now);
        }
        let chosen = self.record_next.take();
        let wanted = record || (chosen.is_some() && chosen == self.call);
        if wanted && self.hint_call().is_some() && self.begin_start() {
            return Some(CallCommand::Start);
        }
        if self.hinting() && !self.starting() {
            let since = *self.offer_since.get_or_insert(now);
            if now - since >= OFFER_TIMEOUT_MS {
                self.dismiss_call();
                self.offer_since = None;
            }
        } else {
            self.offer_since = None;
        }
        None
    }

    /// While recording: the call behind it, whether it ended, and whether a
    /// new one means a separate note.
    fn follow_recording_call(
        &mut self,
        apps: &[InputApp],
        record: bool,
        now: i64,
    ) -> Option<CallCommand> {
        if self.recording_call.is_none() {
            // Started by hand or from a calendar prompt: whatever call shows
            // up is what it's for.
            self.recording_call = self.call.clone();
            self.presence = CallPresence::default();
            self.left_at = None;
        }
        // Decided now, while its own call is still ending: it stands for this
        // call and no other.
        if record {
            self.record_next = self.call.clone();
        }
        let tracked = self.recording_call.clone()?;
        match self
            .presence
            .observe(holds_microphone(apps, &tracked.app_id), now)
        {
            Some(PresenceChange::Left) => self.left_at = Some(now),
            Some(PresenceChange::Back) => self.left_at = None,
            None => {}
        }
        // The question shows beside the floater, not over the open panel.
        if self.left_at.is_some() {
            self.expanded = false;
        }
        // Back-to-back: a different call once this one's app left, or another
        // service after a release in the same browser. The same service
        // coming back (a long sleep, a rejoin) is the same note.
        let new_call = self.call.as_ref().is_some_and(|c| {
            c.since != tracked.since
                && (self.presence.has_left()
                    || (c.app_id == tracked.app_id && c.source != tracked.source))
        });
        if new_call {
            return (!std::mem::replace(&mut self.stopping, true)).then_some(CallCommand::Stop);
        }
        self.stop_if_unanswered(now)
    }

    /// Starting a recording from the hint, until it reports in.
    pub fn starting(&self) -> bool {
        self.starting && !self.recorder.is_recording()
    }

    /// "Take notes" clicked: show it starting. False when it already is
    /// (a second click starts nothing).
    pub fn begin_start(&mut self) -> bool {
        self.start_failed = false;
        self.more_open = false;
        !std::mem::replace(&mut self.starting, true)
    }

    /// The start finished (the recording reports in through
    /// [`Self::set_recorder`]) or failed: back to offering notes, saying
    /// it failed.
    pub fn end_start(&mut self, ok: bool) {
        self.starting = false;
        self.start_failed = !ok;
    }

    /// The last start from the hint failed.
    pub fn start_failed(&self) -> bool {
        self.start_failed && !self.starting()
    }

    pub fn paused(&self) -> bool {
        self.recorder.is_paused()
    }

    pub fn meeting_id(&self) -> Option<i64> {
        match &self.recorder {
            RecorderState::Recording { meeting_id, .. } => Some(*meeting_id),
            RecorderState::Idle => None,
        }
    }

    pub fn title(&self) -> &str {
        match &self.recorder {
            RecorderState::Recording { title, .. } => title,
            RecorderState::Idle => "",
        }
    }

    /// A new recording starts collapsed, with flat meters.
    pub fn set_recorder(&mut self, state: RecorderState) {
        let started = match (&self.recorder, &state) {
            (
                RecorderState::Recording { meeting_id: a, .. },
                RecorderState::Recording { meeting_id: b, .. },
            ) => a != b,
            (RecorderState::Idle, RecorderState::Recording { .. }) => true,
            _ => false,
        };
        if started {
            *self = Self {
                recorder: RecorderState::Idle,
                // The live transcriber starts before the recorder reports
                // the recording: keep what it already said.
                live: std::mem::take(&mut self.live),
                call: self.call.clone(),
                dismissed: self.dismissed,
                pending_snoozes: std::mem::take(&mut self.pending_snoozes),
                ..Self::default()
            };
        }
        if self.recorder.is_recording() && !state.is_recording() {
            self.recording_stopped();
        }
        self.recorder = state;
        self.keep_silence_warning_in_view();
    }

    /// Stopped during a call: don't offer notes for it again. One that ended
    /// first needs no such thing, and a new call behind it is still offered.
    fn recording_stopped(&mut self) {
        let tracked = self.recording_call.take();
        let left = self.presence.has_left();
        self.presence = CallPresence::default();
        self.left_at = None;
        self.stopping = false;
        // Whatever comes next offers beside the floater.
        self.expanded = false;
        let Some(tracked) = tracked else {
            self.dismiss_call();
            return;
        };
        let new_call = self.call.as_ref().filter(|c| c.since != tracked.since);
        if new_call.is_none() && self.call.is_some() {
            self.dismiss_call();
        } else if !left && new_call.is_none_or(|c| c.app_id != tracked.app_id) {
            self.pending_snoozes.push(tracked.app_id);
        }
    }

    pub fn apply_live(&mut self, update: &LiveUpdate) {
        self.live.apply(update);
    }

    /// The live transcript of the recording shown, if it is this one's.
    pub fn live(&self) -> Option<&LiveTranscript> {
        let id = self.meeting_id()?;
        (self.live.meeting_id == id).then_some(&self.live)
    }

    pub fn live_status(&self) -> LiveStatus {
        self.live()
            .map(|l| l.status.clone())
            .unwrap_or(LiveStatus::Starting)
    }

    /// Scroll in one sample per source, `[mic, system]` (flat while paused).
    pub fn push_levels(&mut self, levels: [f32; 2]) {
        let levels = if self.paused() {
            [0.0; 2]
        } else {
            levels.map(|l| l.clamp(0.0, 1.0))
        };
        self.wave.pop_front();
        self.wave.push_back(levels[0].max(levels[1]));
        for (history, level) in self.sources.iter_mut().zip(levels) {
            history.pop_front();
            history.push_back(level);
        }
    }

    pub fn wave(&self) -> impl Iterator<Item = f32> + '_ {
        self.wave.iter().copied()
    }

    /// The recent peak of a source, 0.0..=1.0.
    pub fn source_level(&self, track: Track) -> f32 {
        let i = match track {
            Track::Mic => 0,
            Track::System => 1,
        };
        self.sources[i].iter().copied().fold(0.0, f32::max)
    }

    /// Whether a source picked up sound lately.
    pub fn source_active(&self, track: Track) -> bool {
        self.source_level(track) >= ACTIVE_LEVEL
    }

    /// Capture time so far (pauses excluded), e.g. `12:34`.
    pub fn timer(&self, now_ms: i64) -> String {
        format_elapsed(self.recorder.elapsed_ms(now_ms).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guidy_core::meetings::calls::{CallSource, END_GRACE_MS};

    fn recording(id: i64) -> RecorderState {
        RecorderState::Recording {
            meeting_id: id,
            title: "Weekly sync".into(),
            started_at: 0,
            scheduled_end_at: None,
            paused_at: None,
            paused_ms: 0,
            health: Default::default(),
        }
    }

    fn line(id: u64, track: Track, t0_ms: i64, text: &str, tentative: bool) -> LiveLine {
        LiveLine {
            id,
            track,
            t0_ms,
            t1_ms: t0_ms + 1_000,
            text: text.into(),
            tentative,
        }
    }

    #[test]
    fn rows_join_a_speakers_lines_and_keep_the_tentative_tail() {
        let lines = [
            line(0, Track::Mic, 0, "Morning all.", false),
            line(2, Track::Mic, 1_500, "Shall we start?", false),
            line(1, Track::System, 3_000, "Sure,", false),
            line(3, Track::System, 4_000, "  ", false),
            line(5, Track::System, 5_000, "let me share", true),
            line(4, Track::Mic, 6_000, "Great.", false),
        ];
        let r = rows(&lines);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].text, "Morning all. Shall we start?");
        assert_eq!(
            (r[1].text.as_str(), r[1].tentative.as_str()),
            ("Sure,", "let me share")
        );
        assert_eq!(r[2].track, Track::Mic);
    }

    #[test]
    fn a_new_recording_starts_collapsed_and_keeps_its_early_live_lines() {
        let mut m = WidgetModel::default();
        m.set_recorder(recording(1));
        m.expanded = true;
        m.push_levels([0.8, 0.0]);
        assert!(m.source_active(Track::Mic));
        assert!(!m.source_active(Track::System));
        // Recording 2: the transcriber reports before the recorder does.
        m.apply_live(&LiveUpdate {
            meeting_id: 2,
            status: Some(LiveStatus::Live),
            line: Some(line(0, Track::Mic, 0, "hi", false)),
        });
        m.set_recorder(recording(2));
        assert!(!m.expanded);
        assert!(!m.source_active(Track::Mic));
        assert_eq!(m.live().map(|l| l.lines.len()), Some(1));
        assert_eq!(m.live_status(), LiveStatus::Live);
    }

    #[test]
    fn another_meetings_transcript_is_not_shown() {
        let mut m = WidgetModel::default();
        m.set_recorder(recording(3));
        m.apply_live(&LiveUpdate {
            meeting_id: 2,
            status: Some(LiveStatus::Live),
            line: None,
        });
        assert!(m.live().is_none());
        assert_eq!(m.live_status(), LiveStatus::Starting);
    }

    fn call(since: i64) -> Call {
        Call {
            source: guidy_core::meetings::calls::CallSource::Zoom,
            since,
            app_id: "us.zoom.xos".into(),
        }
    }

    #[test]
    fn a_call_shows_the_hint_until_dismissed() {
        let mut m = WidgetModel::default();
        assert!(!m.visible());
        m.set_call(Some(call(10)));
        assert!(m.visible() && m.hinting());
        m.dismiss_call();
        assert!(!m.visible());
        // The next call is offered again.
        m.set_call(Some(call(20)));
        assert!(m.hinting());
        m.set_call(None);
        assert!(!m.visible());
    }

    #[test]
    fn recording_replaces_the_hint_and_stopping_does_not_bring_it_back() {
        let mut m = WidgetModel::default();
        m.set_call(Some(call(10)));
        m.set_recorder(recording(1));
        assert!(m.visible() && !m.hinting());
        m.set_recorder(RecorderState::Idle);
        assert!(!m.visible());
        m.set_call(Some(call(30)));
        assert!(m.hinting());
    }

    #[test]
    fn take_notes_shows_starting_until_the_recording_reports_in() {
        let mut m = WidgetModel::default();
        m.set_call(Some(call(10)));
        assert!(m.begin_start());
        assert!(m.starting() && m.hinting());
        // A second click while starting does nothing.
        assert!(!m.begin_start());
        m.set_recorder(recording(1));
        assert!(!m.starting());
        m.end_start(true);
        m.set_recorder(RecorderState::Idle);
        m.set_call(Some(call(30)));
        assert!(!m.starting());
    }

    #[test]
    fn a_failed_start_offers_notes_again() {
        let mut m = WidgetModel::default();
        m.set_call(Some(call(10)));
        assert!(m.begin_start());
        m.end_start(false);
        assert!(!m.starting() && m.hinting() && m.start_failed());
        // Trying again clears it.
        assert!(m.begin_start());
        assert!(!m.start_failed());
        m.end_start(false);
        // So does the next call.
        m.set_call(Some(call(20)));
        assert!(!m.start_failed());
    }

    fn chrome(since: i64) -> Call {
        Call {
            source: CallSource::Meet,
            since,
            app_id: "com.google.Chrome".into(),
        }
    }

    fn holding(id: &str) -> Vec<InputApp> {
        vec![InputApp {
            bundle_id: id.into(),
            ..Default::default()
        }]
    }

    fn poll(apps: &[InputApp], current: Option<Call>, now: i64) -> CallPoll<'_> {
        CallPoll {
            apps,
            current,
            detection: None,
            now,
        }
    }

    #[test]
    fn an_unanswered_offer_closes_and_snoozes_only_itself() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        assert_eq!(m.apply_call_poll(poll(&zoom, Some(call(1)), 1_000)), None);
        assert!(m.hinting());
        m.apply_call_poll(poll(&zoom, Some(call(1)), 1_000 + OFFER_TIMEOUT_MS - 1));
        assert!(m.hinting());
        m.apply_call_poll(poll(&zoom, Some(call(1)), 1_000 + OFFER_TIMEOUT_MS));
        assert!(!m.hinting());
        assert_eq!(m.take_snoozes(), ["us.zoom.xos"]);
        // Nothing was stopped or started.
        assert!(!m.recorder.is_recording());
        // While it starts, the offer doesn't time out.
        let mut m = WidgetModel::default();
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        assert!(m.begin_start());
        m.apply_call_poll(poll(&zoom, Some(call(1)), OFFER_TIMEOUT_MS));
        assert!(m.hinting() && m.take_snoozes().is_empty());
    }

    #[test]
    fn a_call_set_to_always_starts_recording_once() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        let detected = CallPoll {
            apps: &zoom,
            current: Some(call(1)),
            detection: Some(Detection::Record(call(1))),
            now: 0,
        };
        assert_eq!(
            m.apply_call_poll(detected.clone()),
            Some(CallCommand::Start)
        );
        assert_eq!(m.apply_call_poll(detected), None, "already starting");
    }

    #[test]
    fn a_call_that_ended_asks_then_stops_by_itself() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        assert_eq!(m.apply_call_poll(poll(&zoom, Some(call(1)), 0)), None);
        // Left: asked after the grace, not before.
        assert_eq!(m.apply_call_poll(poll(&[], Some(call(1)), 1_000)), None);
        assert!(m.left_call().is_none());
        let left = 1_000 + END_GRACE_MS;
        assert_eq!(m.apply_call_poll(poll(&[], None, left)), None);
        assert!(m.left_call().is_some());
        assert_eq!(m.apply_call_poll(poll(&[], None, left + 59_000)), None);
        // No answer in a minute: stop, once.
        let late = left + LEFT_CALL_TIMEOUT_MS;
        assert_eq!(
            m.apply_call_poll(poll(&[], None, late)),
            Some(CallCommand::Stop)
        );
        assert_eq!(m.apply_call_poll(poll(&[], None, late + 1_000)), None);
        m.set_recorder(RecorderState::Idle);
        assert!(m.left_call().is_none());
        // It had ended: a new call in the same app is offered.
        assert!(m.take_snoozes().is_empty());
    }

    #[test]
    fn keep_keeps_recording_and_coming_back_cancels_the_question() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        m.apply_call_poll(poll(&[], None, 1_000));
        m.apply_call_poll(poll(&[], None, 1_000 + END_GRACE_MS));
        assert!(m.left_call().is_some());
        // Back in the call before answering.
        m.apply_call_poll(poll(&zoom, None, 20_000));
        assert!(m.left_call().is_none());
        // Left again; this time "Keep".
        m.apply_call_poll(poll(&[], None, 21_000));
        m.apply_call_poll(poll(&[], None, 21_000 + END_GRACE_MS));
        assert!(m.left_call().is_some());
        m.keep_recording();
        for t in 0..10 {
            let now = 30_000 + t * LEFT_CALL_TIMEOUT_MS;
            assert_eq!(m.apply_call_poll(poll(&[], None, now)), None);
        }
        assert!(m.recorder.is_recording());
    }

    #[test]
    fn stopping_while_the_call_goes_on_snoozes_it() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        m.set_recorder(RecorderState::Idle);
        assert!(!m.hinting());
        assert_eq!(m.take_snoozes(), ["us.zoom.xos"]);
    }

    #[test]
    fn a_call_starting_after_this_one_ended_makes_a_separate_note() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        let teams_apps = holding("com.microsoft.teams2");
        let teams = Call {
            source: CallSource::Teams,
            since: 30_000,
            app_id: "com.microsoft.teams2".into(),
        };
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        // Teams appears while Zoom's grace still runs: not yet a new note.
        m.apply_call_poll(poll(&teams_apps, None, 1_000));
        assert_eq!(
            m.apply_call_poll(poll(&teams_apps, Some(teams.clone()), 2_000)),
            None
        );
        // Zoom has left: stop this one.
        assert_eq!(
            m.apply_call_poll(poll(&teams_apps, Some(teams.clone()), 1_000 + END_GRACE_MS)),
            Some(CallCommand::Stop)
        );
        m.set_recorder(RecorderState::Idle);
        // And Teams is offered, not dismissed with it.
        assert!(m.hinting());
        assert_eq!(m.hint_call().map(|c| c.since), Some(30_000));
        assert!(m.take_snoozes().is_empty());
    }

    #[test]
    fn a_new_service_in_the_same_browser_makes_a_separate_note_but_a_rejoin_does_not() {
        let mut m = WidgetModel::default();
        let browser = holding("com.google.Chrome");
        m.set_call(Some(chrome(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&browser, Some(chrome(1)), 0));
        // The same service, a new detection (a long sleep): the same note.
        assert_eq!(
            m.apply_call_poll(poll(&browser, Some(chrome(500_000)), 500_000)),
            None
        );
        // Another service in that browser, after a release.
        let teams = Call {
            source: CallSource::Teams,
            since: 600_000,
            app_id: "com.google.Chrome".into(),
        };
        assert_eq!(
            m.apply_call_poll(poll(&browser, Some(teams), 600_000)),
            Some(CallCommand::Stop)
        );
    }

    #[test]
    fn an_always_call_after_a_split_starts_once_the_stop_reports_in() {
        let mut m = WidgetModel::default();
        let teams_apps = holding("com.microsoft.teams2");
        let teams = Call {
            source: CallSource::Teams,
            since: 30_000,
            app_id: "com.microsoft.teams2".into(),
        };
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&holding("us.zoom.xos"), Some(call(1)), 0));
        m.apply_call_poll(poll(&[], None, 1_000));
        let at = 1_000 + END_GRACE_MS;
        let detected = CallPoll {
            apps: &teams_apps,
            current: Some(teams.clone()),
            detection: Some(Detection::Record(teams.clone())),
            now: at,
        };
        assert_eq!(m.apply_call_poll(detected), Some(CallCommand::Stop));
        m.set_recorder(RecorderState::Idle);
        assert_eq!(
            m.apply_call_poll(poll(&teams_apps, Some(teams), at + 1_000)),
            Some(CallCommand::Start)
        );
    }

    #[test]
    fn an_always_call_decided_during_the_grace_still_records() {
        let mut m = WidgetModel::default();
        let teams_apps = holding("com.microsoft.teams2");
        let teams = Call {
            source: CallSource::Teams,
            since: 2_000,
            app_id: "com.microsoft.teams2".into(),
        };
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&holding("us.zoom.xos"), Some(call(1)), 0));
        m.apply_call_poll(poll(&[], None, 1_000));
        // Confirmed well before Zoom's grace ends.
        let confirmed = CallPoll {
            apps: &teams_apps,
            current: Some(teams.clone()),
            detection: Some(Detection::Record(teams.clone())),
            now: 3_000,
        };
        assert_eq!(m.apply_call_poll(confirmed), None);
        let at = 1_000 + END_GRACE_MS;
        assert_eq!(
            m.apply_call_poll(poll(&teams_apps, Some(teams.clone()), at)),
            Some(CallCommand::Stop)
        );
        m.set_recorder(RecorderState::Idle);
        assert_eq!(
            m.apply_call_poll(poll(&teams_apps, Some(teams), at + 1_000)),
            Some(CallCommand::Start)
        );
    }

    #[test]
    fn an_always_decision_does_not_carry_over_to_another_call() {
        let mut m = WidgetModel::default();
        let teams = Call {
            source: CallSource::Teams,
            since: 2_000,
            app_id: "com.microsoft.teams2".into(),
        };
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        let zoom = holding("us.zoom.xos");
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        let teams_apps = holding("com.microsoft.teams2");
        m.apply_call_poll(poll(&teams_apps, None, 1_000));
        m.apply_call_poll(CallPoll {
            apps: &teams_apps,
            current: Some(teams.clone()),
            detection: Some(Detection::Record(teams)),
            now: 3_000,
        });
        m.set_recorder(RecorderState::Idle);
        // While the stop was in flight that call went and another came.
        let discord = Call {
            source: CallSource::Discord,
            since: 50_000,
            app_id: "com.hnc.Discord".into(),
        };
        let apps = holding("com.hnc.Discord");
        assert_eq!(m.apply_call_poll(poll(&apps, Some(discord), 51_000)), None);
        assert!(m.hinting(), "offered, not recorded");
    }

    #[test]
    fn an_always_decision_stays_with_its_app_when_another_shares_its_start() {
        let mut m = WidgetModel::default();
        let apps = holding("com.microsoft.teams2");
        let (mut discord, teams) = (call(2_000), call(2_000));
        discord.app_id = "com.hnc.Discord".into();
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&holding("us.zoom.xos"), Some(call(1)), 0));
        m.apply_call_poll(CallPoll {
            apps: &[],
            current: Some(discord.clone()),
            detection: Some(Detection::Record(discord)),
            now: 3_000,
        });
        m.set_recorder(RecorderState::Idle);
        assert_eq!(m.apply_call_poll(poll(&apps, Some(teams), 9_000)), None);
    }

    #[test]
    fn starting_closes_the_choices() {
        let mut m = WidgetModel::default();
        m.set_call(Some(call(1)));
        m.more_open = true;
        assert!(m.begin_start());
        assert!(!m.more_open);
    }

    #[test]
    fn a_dismiss_during_a_start_keeps_the_call_followed() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        assert!(m.begin_start());
        m.dismiss_call();
        assert!(m.take_snoozes().is_empty());
        m.set_recorder(recording(1));
        m.end_start(true);
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        m.apply_call_poll(poll(&[], None, 1_000));
        m.apply_call_poll(poll(&[], None, 1_000 + END_GRACE_MS));
        assert!(m.left_call().is_some());
    }

    #[test]
    fn the_question_closes_an_open_panel_so_it_can_be_seen() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.expanded = true;
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        assert!(m.expanded, "open while the call goes on");
        m.apply_call_poll(poll(&[], None, 1_000));
        m.apply_call_poll(poll(&[], None, 1_000 + END_GRACE_MS));
        assert!(m.left_call().is_some() && !m.expanded);
        // Reopened: closed again while the question stands.
        m.expanded = true;
        m.apply_call_poll(poll(&[], None, 2_000 + END_GRACE_MS));
        assert!(!m.expanded);
    }

    #[test]
    fn turning_suggestions_off_lets_an_unanswered_question_stop() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        m.apply_call_poll(poll(&[], None, 1_000));
        let left = 1_000 + END_GRACE_MS;
        m.apply_call_poll(poll(&[], None, left));
        assert_eq!(m.apply_idle_poll(left + 1_000), None);
        assert!(m.left_call().is_some());
        assert_eq!(
            m.apply_idle_poll(left + LEFT_CALL_TIMEOUT_MS),
            Some(CallCommand::Stop)
        );
        // With nothing being asked it goes on until stopped by hand.
        let mut m = WidgetModel::default();
        m.set_call(Some(call(1)));
        m.set_recorder(recording(1));
        m.apply_call_poll(poll(&zoom, Some(call(1)), 0));
        assert_eq!(m.apply_idle_poll(1_000), None);
        assert_eq!(m.apply_idle_poll(10_000_000), None);
        assert!(m.recorder.is_recording() && m.left_call().is_none());
    }

    #[test]
    fn a_recording_started_by_hand_follows_the_call_that_shows_up() {
        let mut m = WidgetModel::default();
        let zoom = holding("us.zoom.xos");
        m.set_recorder(recording(1));
        // No call: nothing to follow, nothing to stop.
        for t in 0..10 {
            assert_eq!(m.apply_call_poll(poll(&[], None, t * 60_000)), None);
        }
        m.apply_call_poll(poll(&zoom, Some(call(600_000)), 600_000));
        m.apply_call_poll(poll(&[], None, 601_000));
        m.apply_call_poll(poll(&[], None, 601_000 + END_GRACE_MS));
        assert!(m.left_call().is_some());
    }

    #[test]
    fn the_worst_health_is_what_is_said() {
        let healthy = RecordingHealth::default();
        assert_eq!(health_notice(&healthy), None);
        let mut health = RecordingHealth {
            backlog: true,
            ..healthy
        };
        assert_eq!(health_notice(&health), Some(HealthNotice::Backlog));
        assert!(!HealthNotice::Backlog.is_failure());
        assert!(!HealthNotice::MicSilent.is_failure());
        assert!(HealthNotice::SystemLost.is_failure());
        health.system = SourceHealth::Silent;
        assert_eq!(health_notice(&health), Some(HealthNotice::SystemSilent));
        health.mic = SourceHealth::Reconnecting;
        assert_eq!(health_notice(&health), Some(HealthNotice::MicReconnecting));
        health.low_disk = true;
        assert_eq!(health_notice(&health), Some(HealthNotice::LowDisk));
        health.system = SourceHealth::Lost;
        assert_eq!(health_notice(&health), Some(HealthNotice::SystemLost));
        health.system_denied = true;
        assert_eq!(health_notice(&health), Some(HealthNotice::SystemDenied));
        health.silence_stop_at = Some(1);
        assert_eq!(health_notice(&health), Some(HealthNotice::NoAudio));
    }

    #[test]
    fn a_new_state_with_only_its_health_changed_is_not_a_new_recording() {
        let mut m = WidgetModel::default();
        m.set_recorder(recording(1));
        m.expanded = true;
        m.push_levels([0.8, 0.0]);
        let RecorderState::Recording {
            meeting_id,
            title,
            started_at,
            scheduled_end_at,
            paused_at,
            paused_ms,
            ..
        } = recording(1)
        else {
            unreachable!()
        };
        let with = |health| RecorderState::Recording {
            meeting_id,
            title: title.clone(),
            started_at,
            scheduled_end_at,
            paused_at,
            paused_ms,
            health,
        };
        m.set_recorder(with(RecordingHealth {
            system: SourceHealth::Lost,
            ..Default::default()
        }));
        assert!(m.expanded && m.source_active(Track::Mic));
        assert_eq!(m.source_health(Track::System), SourceHealth::Lost);
        // Only the warning that stops it closes the panel.
        m.set_recorder(with(RecordingHealth {
            silence_stop_at: Some(1),
            ..Default::default()
        }));
        assert!(!m.expanded);
    }

    #[test]
    fn meters_are_flat_while_paused() {
        let mut m = WidgetModel::default();
        m.set_recorder(RecorderState::Recording {
            meeting_id: 1,
            title: "Weekly sync".into(),
            started_at: 0,
            scheduled_end_at: None,
            paused_at: Some(10),
            paused_ms: 0,
            health: Default::default(),
        });
        m.push_levels([0.9, 0.9]);
        assert!(m.wave().all(|v| v == 0.0));
        assert_eq!(m.source_level(Track::System), 0.0);
    }
}
