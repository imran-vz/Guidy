//! Data types shared by the meeting-notes layers and the UI.
//!
//! Times inside a meeting are milliseconds from the start of the recording
//! (`t0_ms`/`t1_ms`). Wall-clock times are unix milliseconds (`*_at`).

use serde::{Deserialize, Serialize};

pub type MeetingId = i64;
pub type PersonId = i64;
pub type SpeakerId = i64;
pub type ActionItemId = i64;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    /// Capture in progress.
    Recording,
    /// Recorded, waiting to be processed (or processing was cancelled).
    Recorded,
    /// The processing pipeline is running.
    Processing,
    /// Transcript and speakers are ready.
    Processed,
    /// Processing failed; `Meeting::error` says why. Can be retried.
    Failed,
}

impl MeetingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Recorded => "recorded",
            Self::Processing => "processing",
            Self::Processed => "processed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "recording" => Self::Recording,
            "recorded" => Self::Recorded,
            "processing" => Self::Processing,
            "processed" => Self::Processed,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Track {
    /// The user's microphone (echo-cancelled). Always the user.
    Mic,
    /// Everyone else: system audio from the process tap.
    System,
}

impl Track {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mic" => Some(Self::Mic),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: Option<String>,
    /// EventKit says this attendee is the Mac's user. Excluded from speaker
    /// suggestions and from owner matching (the user is "me").
    #[serde(default)]
    pub is_current_user: bool,
}

/// A calendar event, as read from EventKit.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    /// EventKit `calendarItemExternalIdentifier` (stable across devices) or
    /// `eventIdentifier` as a fallback, suffixed with the start time so
    /// recurring occurrences differ.
    pub id: String,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    /// The Meet / Zoom / Teams URL found in the event, if any.
    pub meeting_url: Option<String>,
    pub attendees: Vec<Attendee>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Meeting {
    pub id: MeetingId,
    pub title: String,
    pub calendar_event_id: Option<String>,
    /// The linked event's snapshot (title, times, attendees) at link time.
    pub calendar_event: Option<CalendarEvent>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// Encrypted Opus files, relative to `<app_data_dir>/meetings/audio/`.
    /// `None` once deleted by retention.
    pub mic_audio_path: Option<String>,
    pub system_audio_path: Option<String>,
    pub status: MeetingStatus,
    pub error: Option<String>,
    /// Last completed processing stage, so processing can resume.
    pub processing_stage: Option<String>,
}

impl Meeting {
    pub fn duration_ms(&self) -> Option<i64> {
        self.ended_at.map(|e| (e - self.started_at).max(0))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Word {
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub text: String,
}

/// One transcribed speech segment on the merged timeline.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: i64,
    pub meeting_id: MeetingId,
    pub track: Track,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub text: String,
    pub words: Vec<Word>,
    /// The meeting speaker this segment is attributed to.
    pub speaker_id: Option<SpeakerId>,
}

/// A speaker within one meeting (a voiceprint cluster, or the mic user).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MeetingSpeaker {
    pub id: SpeakerId,
    pub meeting_id: MeetingId,
    /// "Speaker 1", "Speaker 2", ... or "me" for the mic track.
    pub cluster_label: String,
    pub person_id: Option<PersonId>,
    /// Cosine similarity of the automatic match, if one was made.
    pub match_confidence: Option<f32>,
    /// True when the user named/confirmed it, or the match was above the
    /// confident threshold. False shows "Confirm?" in the UI.
    pub confirmed: bool,
    /// True for the mic track's speaker (the user).
    pub is_user: bool,
}

impl MeetingSpeaker {
    /// The number in a generated "Speaker {n}" label, so the UI can show a
    /// translated label. `None` for the user's label and anything else.
    pub fn speaker_number(&self) -> Option<u32> {
        self.cluster_label
            .trim()
            .strip_prefix("Speaker ")?
            .trim()
            .parse()
            .ok()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: PersonId,
    pub display_name: String,
    pub email: Option<String>,
    pub created_at: i64,
    /// Number of stored voiceprints (filled by list queries).
    pub voiceprint_count: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Voiceprint {
    pub id: i64,
    pub person_id: PersonId,
    pub embedding: Vec<f32>,
    pub source_meeting_id: Option<MeetingId>,
    pub created_at: i64,
}

/// Fixed JSON structure produced by the summarizer (plan §2.3).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct MeetingSummary {
    pub title: String,
    pub short_summary: String,
    pub decisions: Vec<String>,
    pub topics: Vec<String>,
    pub action_items: Vec<ActionItemDraft>,
}

/// An action item as the model returns it (before it is stored).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ActionItemDraft {
    pub text: String,
    /// Owner name as said in the meeting ("me" for the user).
    pub owner: Option<String>,
    /// Due date if one was mentioned, ISO 8601 date or free text.
    pub due: Option<String>,
    /// Verbatim quote from the transcript. Items without one are dropped.
    pub source_quote: String,
    /// Timestamp of the quote in the transcript.
    pub source_t_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketState {
    /// No ticket drafted yet / draft not reviewed.
    Draft,
    Approved,
    Discarded,
    /// The Linear issue exists (`linear_issue_id` is set).
    Created,
}

impl TicketState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::Discarded => "discarded",
            Self::Created => "created",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "draft" => Self::Draft,
            "approved" => Self::Approved,
            "discarded" => Self::Discarded,
            "created" => Self::Created,
            _ => return None,
        })
    }
}

/// Draft Linear ticket fields, edited by the user before approval.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct TicketDraft {
    pub title: String,
    pub description: String,
    pub team_id: Option<String>,
    pub project_id: Option<String>,
    /// Linear priority: 0 none, 1 urgent, 2 high, 3 medium, 4 low.
    pub priority: Option<u8>,
    pub label_ids: Vec<String>,
    pub assignee_id: Option<String>,
    /// Linear `dueDate` (ISO `YYYY-MM-DD`), when the item's due date parses.
    #[serde(default)]
    pub due_date: Option<String>,
}

/// A stored action item (one row of `action_items`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ActionItem {
    pub id: ActionItemId,
    pub meeting_id: MeetingId,
    pub text: String,
    pub owner_person_id: Option<PersonId>,
    pub owner_name: Option<String>,
    pub owner_is_user: bool,
    pub due: Option<String>,
    pub source_quote: String,
    pub source_t_ms: Option<i64>,
    pub ticket: TicketDraft,
    pub ticket_state: TicketState,
    pub linear_issue_id: Option<String>,
    pub linear_issue_identifier: Option<String>,
    pub linear_issue_url: Option<String>,
}

/// A stored summary for a meeting (latest generation wins).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct StoredSummary {
    pub meeting_id: MeetingId,
    pub title: String,
    pub short_summary: String,
    pub decisions: Vec<String>,
    pub topics: Vec<String>,
    /// "provider_id/model" that produced it.
    pub generated_by: String,
    pub generated_at: i64,
    /// Tokens and cost of the cloud calls that produced it; `None` for local
    /// models and summaries made before costs were recorded.
    #[serde(default)]
    pub usage: Option<crate::llm::usage::MeterTotals>,
}

/// Processing stages, in order. Stored as `Meeting::processing_stage` after
/// each one completes so a cancelled/crashed run resumes from the next.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStage {
    Decode,
    Vad,
    Transcribe,
    Voiceprints,
    Cluster,
    Merge,
    Done,
}

impl ProcessingStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Vad => "vad",
            Self::Transcribe => "transcribe",
            Self::Voiceprints => "voiceprints",
            Self::Cluster => "cluster",
            Self::Merge => "merge",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "decode" => Self::Decode,
            "vad" => Self::Vad,
            "transcribe" => Self::Transcribe,
            "voiceprints" => Self::Voiceprints,
            "cluster" => Self::Cluster,
            "merge" => Self::Merge,
            "done" => Self::Done,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProcessingProgress {
    pub meeting_id: MeetingId,
    pub stage: ProcessingStage,
    /// 0.0..=1.0 within the whole pipeline.
    pub fraction: f32,
    pub message: Option<String>,
}

/// What the recorder is doing right now (drives the tray and the UI).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum RecorderState {
    #[default]
    Idle,
    Recording {
        meeting_id: MeetingId,
        title: String,
        started_at: i64,
        /// Scheduled end of the linked event, if any.
        scheduled_end_at: Option<i64>,
        /// When the current pause began (unix ms); `None` while capturing.
        #[serde(default)]
        paused_at: Option<i64>,
        /// Total length of the pauses that already ended.
        #[serde(default)]
        paused_ms: i64,
    },
}

impl RecorderState {
    pub fn is_recording(&self) -> bool {
        matches!(self, RecorderState::Recording { .. })
    }

    pub fn is_paused(&self) -> bool {
        matches!(
            self,
            RecorderState::Recording {
                paused_at: Some(_),
                ..
            }
        )
    }

    /// Time captured so far at `now` (unix ms), pauses excluded. `None` when
    /// idle.
    pub fn elapsed_ms(&self, now: i64) -> Option<i64> {
        match self {
            RecorderState::Recording {
                started_at,
                paused_at,
                paused_ms,
                ..
            } => {
                let end = paused_at.unwrap_or(now);
                Some((end - started_at - paused_ms).max(0))
            }
            RecorderState::Idle => None,
        }
    }
}

/// `m:ss`, or `h:mm:ss` from an hour on.
pub fn format_elapsed(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One utterance of a recording's live transcript. Live lines are only for
/// watching the meeting as it happens: the stored transcript (speakers,
/// words) still comes from processing after the stop.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LiveLine {
    /// Stable while the utterance is still being spoken: a later update with
    /// the same id replaces the line.
    pub id: u64,
    /// Mic: the user. System: everyone else.
    pub track: Track,
    /// Recording time of the utterance.
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub text: String,
    /// Decoded while the utterance is still going: it may still change.
    pub tentative: bool,
}

/// Whether live transcription is running for the recording.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveStatus {
    /// Not recording, or live transcription is turned off.
    #[default]
    Off,
    /// Loading the transcription model.
    Starting,
    Live,
    /// It couldn't start (e.g. no model downloaded). The recording itself is
    /// unaffected.
    Unavailable {
        reason: String,
    },
}

/// The live transcript of the meeting being recorded.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct LiveTranscript {
    pub meeting_id: MeetingId,
    pub status: LiveStatus,
    /// In the order the utterances started.
    pub lines: Vec<LiveLine>,
}

impl LiveTranscript {
    /// Apply an update for this meeting: a new status, and a line added or
    /// replacing the one with its id. Updates for another meeting start a
    /// new transcript.
    pub fn apply(&mut self, update: &LiveUpdate) {
        if update.meeting_id != self.meeting_id {
            *self = LiveTranscript {
                meeting_id: update.meeting_id,
                ..Default::default()
            };
        }
        if let Some(status) = &update.status {
            self.status = status.clone();
        }
        if let Some(line) = &update.line {
            match self.lines.iter_mut().rev().find(|l| l.id == line.id) {
                Some(existing) => *existing = line.clone(),
                None => {
                    let at = self.lines.partition_point(|l| l.t0_ms <= line.t0_ms);
                    self.lines.insert(at, line.clone());
                }
            }
        }
    }
}

/// Payload of [`events::LIVE_TRANSCRIPT`]: what changed in a live transcript.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LiveUpdate {
    pub meeting_id: MeetingId,
    pub status: Option<LiveStatus>,
    pub line: Option<LiveLine>,
}

/// Event names emitted on the app event bus (`AppHandle::emit`).
pub mod events {
    /// Payload: [`super::LiveUpdate`].
    pub const LIVE_TRANSCRIPT: &str = "meetings://live-transcript";
    /// Payload: [`super::RecorderState`].
    pub const RECORDER_STATE: &str = "meetings://recorder-state";
    /// Payload: [`super::ProcessingProgress`].
    pub const PROCESSING_PROGRESS: &str = "meetings://processing-progress";
    /// Payload: meeting id. A meeting row changed (status, title, ...).
    pub const MEETING_CHANGED: &str = "meetings://meeting-changed";
    /// Payload: (). The People list changed.
    pub const PEOPLE_CHANGED: &str = "meetings://people-changed";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker(label: &str) -> MeetingSpeaker {
        MeetingSpeaker {
            id: 1,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        }
    }

    #[test]
    fn elapsed_excludes_pauses() {
        let mut st = RecorderState::Recording {
            meeting_id: 1,
            title: "x".into(),
            started_at: 1_000,
            scheduled_end_at: None,
            paused_at: None,
            paused_ms: 0,
        };
        assert_eq!(st.elapsed_ms(61_000), Some(60_000));
        if let RecorderState::Recording {
            paused_at,
            paused_ms,
            ..
        } = &mut st
        {
            *paused_at = Some(31_000);
            *paused_ms = 10_000;
        }
        assert!(st.is_paused());
        // Frozen at the pause.
        assert_eq!(st.elapsed_ms(99_000), Some(20_000));
        assert_eq!(RecorderState::Idle.elapsed_ms(5), None);
        assert_eq!(format_elapsed(65_400), "1:05");
        assert_eq!(format_elapsed(3_725_000), "1:02:05");
    }

    #[test]
    fn recorder_state_without_pause_fields_still_parses() {
        let v = serde_json::json!({"Recording": {
            "meeting_id": 3, "title": "t", "started_at": 5, "scheduled_end_at": null
        }});
        let st: RecorderState = serde_json::from_value(v).unwrap();
        assert!(st.is_recording() && !st.is_paused());
    }

    #[test]
    fn speaker_number_parses_generated_labels_only() {
        assert_eq!(speaker("Speaker 1").speaker_number(), Some(1));
        assert_eq!(speaker("Speaker 12").speaker_number(), Some(12));
        assert_eq!(speaker("Imran").speaker_number(), None);
        assert_eq!(speaker("Speaker").speaker_number(), None);
        assert_eq!(speaker("Speaker x").speaker_number(), None);
    }

    #[test]
    fn attendee_is_current_user_defaults_to_false() {
        let a: Attendee = serde_json::from_str(r#"{"name":"Ann","email":null}"#).unwrap();
        assert!(!a.is_current_user);
    }

    fn live_line(id: u64, t0_ms: i64, text: &str, tentative: bool) -> LiveLine {
        LiveLine {
            id,
            track: Track::Mic,
            t0_ms,
            t1_ms: t0_ms + 500,
            text: text.into(),
            tentative,
        }
    }

    fn line_update(meeting_id: MeetingId, line: LiveLine) -> LiveUpdate {
        LiveUpdate {
            meeting_id,
            status: None,
            line: Some(line),
        }
    }

    #[test]
    fn live_lines_are_replaced_by_id_and_kept_in_time_order() {
        let mut t = LiveTranscript {
            meeting_id: 1,
            ..Default::default()
        };
        t.apply(&line_update(1, live_line(1, 1_000, "hel", true)));
        t.apply(&line_update(1, live_line(2, 3_000, "later", false)));
        // A system-audio line that started earlier lands before it.
        t.apply(&line_update(1, live_line(3, 2_000, "between", false)));
        t.apply(&line_update(1, live_line(1, 1_000, "hello", false)));
        let texts: Vec<_> = t.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["hello", "between", "later"]);
        assert!(!t.lines[0].tentative);

        t.apply(&LiveUpdate {
            meeting_id: 1,
            status: Some(LiveStatus::Live),
            line: None,
        });
        assert_eq!(t.status, LiveStatus::Live);
        assert_eq!(t.lines.len(), 3);

        // The next recording starts over.
        t.apply(&line_update(2, live_line(1, 0, "new", true)));
        assert_eq!(t.meeting_id, 2);
        assert_eq!(t.status, LiveStatus::Off);
        assert_eq!(t.lines.len(), 1);
    }
}
