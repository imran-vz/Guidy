//! The recording floater: what it shows. Pure: driven by recorder states,
//! live-transcript updates, polled input levels and a clock. How it moves
//! is [`crate::logic::floater`].
//!
//! Collapsed it is a rounded rectangle (or wings around the notch) with a waveform
//! and a stop button; expanded it is a panel with the meeting being
//! recorded, which sources are picking up sound, the live transcript and
//! the controls.
//!
//! While nothing records but a call is on in Zoom, Google Meet or WhatsApp
//! ([`guidy_core::meetings::calls`]), it shows stopped, with a hint to take
//! notes, until the call ends or the hint is dismissed.

use guidy_core::meetings::calls::Call;
use guidy_core::meetings::types::{
    format_elapsed, LiveLine, LiveStatus, LiveTranscript, LiveUpdate, RecorderState, Track,
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
    pub fn hint_call(&self) -> Option<Call> {
        self.call.filter(|c| Some(c.since) != self.dismissed)
    }

    pub fn set_call(&mut self, call: Option<Call>) {
        self.call = call;
    }

    /// "Not now": stop offering notes for this call.
    pub fn dismiss_call(&mut self) {
        if let Some(c) = self.call {
            self.dismissed = Some(c.since);
        }
    }

    /// Starting a recording from the hint, until it reports in.
    pub fn starting(&self) -> bool {
        self.starting && !self.recorder.is_recording()
    }

    /// "Take notes" clicked: show it starting. False when it already is
    /// (a second click starts nothing).
    pub fn begin_start(&mut self) -> bool {
        !std::mem::replace(&mut self.starting, true)
    }

    /// The start finished (the recording reports in through
    /// [`Self::set_recorder`]) or failed: back to offering notes.
    pub fn end_start(&mut self) {
        self.starting = false;
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
                call: self.call,
                dismissed: self.dismissed,
                ..Self::default()
            };
        }
        // Stopped during a call: don't offer notes for it again.
        if self.recorder.is_recording() && !state.is_recording() {
            self.dismiss_call();
        }
        self.recorder = state;
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

    fn recording(id: i64) -> RecorderState {
        RecorderState::Recording {
            meeting_id: id,
            title: "Weekly sync".into(),
            started_at: 0,
            scheduled_end_at: None,
            paused_at: None,
            paused_ms: 0,
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
            app: guidy_core::meetings::calls::CallApp::Zoom,
            since,
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
        m.end_start();
        m.set_recorder(RecorderState::Idle);
        m.set_call(Some(call(30)));
        assert!(!m.starting());
    }

    #[test]
    fn a_failed_start_offers_notes_again() {
        let mut m = WidgetModel::default();
        m.set_call(Some(call(10)));
        assert!(m.begin_start());
        m.end_start();
        assert!(!m.starting() && m.hinting());
        assert!(m.begin_start());
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
        });
        m.push_levels([0.9, 0.9]);
        assert!(m.wave().all(|v| v == 0.0));
        assert_eq!(m.source_level(Track::System), 0.0);
    }
}
