//! Meeting-notes UI logic with no GPUI dependency: core event decoding,
//! sidebar visibility, transcript grouping, labels and formatting.

use guidy_core::meetings::settings::MeetingSettings;
use guidy_core::meetings::types::{
    events, Meeting, MeetingId, MeetingSpeaker, MeetingStatus, Person, ProcessingProgress,
    RecorderState, Segment, SpeakerId,
};
use serde_json::Value;

/// A meeting-notes core event, decoded for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum MeetingsEvent {
    RecorderState(RecorderState),
    Progress(ProcessingProgress),
    MeetingChanged(MeetingId),
    PeopleChanged,
}

pub fn decode_event(name: &str, payload: &Value) -> Option<MeetingsEvent> {
    match name {
        events::RECORDER_STATE => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::RecorderState),
        events::PROCESSING_PROGRESS => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::Progress),
        events::MEETING_CHANGED => payload.as_i64().map(MeetingsEvent::MeetingChanged),
        events::PEOPLE_CHANGED => Some(MeetingsEvent::PeopleChanged),
        _ => None,
    }
}

/// Insert the Meetings section right after History when supported.
pub fn with_meetings_section(
    mut sections: Vec<&'static str>,
    supported: bool,
) -> Vec<&'static str> {
    if supported {
        let at = sections
            .iter()
            .position(|s| *s == "history")
            .map(|i| i + 1)
            .unwrap_or(sections.len());
        sections.insert(at, "meetings");
    }
    sections
}

/// Consecutive segments by the same speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerBlock {
    pub speaker_id: Option<SpeakerId>,
    pub segments: Vec<Segment>,
}

impl SpeakerBlock {
    pub fn t0_ms(&self) -> i64 {
        self.segments.first().map(|s| s.t0_ms).unwrap_or(0)
    }
}

/// Group a transcript (sorted by time) into runs of the same speaker.
pub fn group_by_speaker(segments: &[Segment]) -> Vec<SpeakerBlock> {
    let mut sorted: Vec<&Segment> = segments.iter().collect();
    sorted.sort_by_key(|s| (s.t0_ms, s.id));
    let mut blocks: Vec<SpeakerBlock> = Vec::new();
    for seg in sorted {
        match blocks.last_mut() {
            Some(b) if b.speaker_id == seg.speaker_id => b.segments.push(seg.clone()),
            _ => blocks.push(SpeakerBlock {
                speaker_id: seg.speaker_id,
                segments: vec![seg.clone()],
            }),
        }
    }
    blocks
}

/// Offset in the recording: `m:ss`, or `h:mm:ss` past an hour.
pub fn format_offset(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Whole minutes, rounded, at least 1 for any non-empty recording.
pub fn duration_minutes(ms: i64) -> i64 {
    if ms <= 0 {
        0
    } else {
        ((ms + 30_000) / 60_000).max(1)
    }
}

/// Localized date of a unix-ms timestamp.
pub fn format_meeting_date(at_ms: i64, locale: &str) -> String {
    crate::logic::flows::format_history_date(at_ms / 1000, locale)
}

/// Uncertain automatic match: shows "Confirm?".
pub fn needs_confirmation(s: &MeetingSpeaker) -> bool {
    !s.confirmed && s.person_id.is_some()
}

/// The label for a speaker: the matched person, the user (mic track), or
/// the cluster label ("Speaker 2").
pub fn speaker_name(
    speaker: &MeetingSpeaker,
    people: &[Person],
    settings: Option<&MeetingSettings>,
    you: &str,
) -> String {
    if let Some(p) = speaker
        .person_id
        .and_then(|id| people.iter().find(|p| p.id == id))
    {
        return p.display_name.clone();
    }
    if speaker.is_user {
        return settings
            .map(|s| s.user_display_name.trim())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| you.to_string());
    }
    speaker.cluster_label.clone()
}

/// Translation key of a meeting's status.
pub fn status_key(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Recording => "meetings.status.recording",
        MeetingStatus::Recorded => "meetings.status.recorded",
        MeetingStatus::Processing => "meetings.status.processing",
        MeetingStatus::Processed => "meetings.status.processed",
        MeetingStatus::Failed => "meetings.status.failed",
    }
}

/// Recorded/failed meetings can be (re)processed; processed ones re-run.
pub fn can_process(m: &Meeting) -> bool {
    matches!(
        m.status,
        MeetingStatus::Recorded | MeetingStatus::Failed | MeetingStatus::Processed
    )
}

/// Retention dropdown value: `"3"`, `"7"`, `"30"` or `"forever"`.
pub fn retention_value(days: Option<u32>) -> String {
    days.map(|d| d.to_string())
        .unwrap_or_else(|| "forever".into())
}

pub fn parse_retention(value: &str) -> Option<u32> {
    value.parse().ok()
}

/// Trimmed optional text: empty means `None`.
pub fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// People a person can be merged into (everyone else).
pub fn merge_targets(people: &[Person], from: i64) -> Vec<&Person> {
    people.iter().filter(|p| p.id != from).collect()
}

/// Apply a progress event to the cached meeting list (status follows it).
pub fn apply_progress(meetings: &mut [Meeting], p: &ProcessingProgress) {
    if let Some(m) = meetings.iter_mut().find(|m| m.id == p.meeting_id) {
        if m.status != MeetingStatus::Processing && p.fraction < 1.0 {
            m.status = MeetingStatus::Processing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guidy_core::meetings::types::{ProcessingStage, Track};

    fn seg(id: i64, t0: i64, spk: Option<i64>) -> Segment {
        Segment {
            id,
            meeting_id: 1,
            track: Track::System,
            t0_ms: t0,
            t1_ms: t0 + 1000,
            text: format!("s{id}"),
            words: vec![],
            speaker_id: spk,
        }
    }

    fn speaker(person: Option<i64>, confirmed: bool, is_user: bool) -> MeetingSpeaker {
        MeetingSpeaker {
            id: 1,
            meeting_id: 1,
            cluster_label: "Speaker 2".into(),
            person_id: person,
            match_confidence: None,
            confirmed,
            is_user,
        }
    }

    #[test]
    fn groups_consecutive_segments_by_speaker() {
        let blocks = group_by_speaker(&[
            seg(3, 3000, Some(2)),
            seg(1, 0, Some(1)),
            seg(2, 1000, Some(1)),
            seg(4, 4000, Some(1)),
        ]);
        let shape: Vec<(Option<i64>, usize)> = blocks
            .iter()
            .map(|b| (b.speaker_id, b.segments.len()))
            .collect();
        assert_eq!(shape, [(Some(1), 2), (Some(2), 1), (Some(1), 1)]);
        assert_eq!(blocks[1].t0_ms(), 3000);
    }

    #[test]
    fn formats_offsets_and_durations() {
        assert_eq!(format_offset(65_000), "1:05");
        assert_eq!(format_offset(3_725_000), "1:02:05");
        assert_eq!(format_offset(-5), "0:00");
        assert_eq!(duration_minutes(0), 0);
        assert_eq!(duration_minutes(10_000), 1);
        assert_eq!(duration_minutes(25 * 60_000 + 10_000), 25);
    }

    #[test]
    fn sidebar_places_meetings_after_history() {
        let base = vec!["general", "history", "models", "about"];
        assert_eq!(
            with_meetings_section(base.clone(), true),
            ["general", "history", "meetings", "models", "about"]
        );
        assert_eq!(with_meetings_section(base.clone(), false), base);
    }

    #[test]
    fn speaker_names_and_confirmation() {
        let people = vec![Person {
            id: 7,
            display_name: "Ada".into(),
            email: None,
            created_at: 0,
            voiceprint_count: 1,
        }];
        let mut settings = MeetingSettings::default();
        assert_eq!(
            speaker_name(&speaker(Some(7), false, false), &people, None, "You"),
            "Ada"
        );
        assert_eq!(
            speaker_name(&speaker(None, false, false), &people, None, "You"),
            "Speaker 2"
        );
        assert_eq!(
            speaker_name(&speaker(None, true, true), &people, Some(&settings), "You"),
            "You"
        );
        settings.user_display_name = "Imran".into();
        assert_eq!(
            speaker_name(&speaker(None, true, true), &people, Some(&settings), "You"),
            "Imran"
        );
        assert!(needs_confirmation(&speaker(Some(7), false, false)));
        assert!(!needs_confirmation(&speaker(Some(7), true, false)));
        assert!(!needs_confirmation(&speaker(None, false, false)));
    }

    #[test]
    fn decodes_core_events() {
        let p = serde_json::json!({
            "meeting_id": 4, "stage": "transcribe", "fraction": 0.5, "message": null
        });
        assert_eq!(
            decode_event(events::PROCESSING_PROGRESS, &p),
            Some(MeetingsEvent::Progress(ProcessingProgress {
                meeting_id: 4,
                stage: ProcessingStage::Transcribe,
                fraction: 0.5,
                message: None,
            }))
        );
        assert_eq!(
            decode_event(events::MEETING_CHANGED, &serde_json::json!(9)),
            Some(MeetingsEvent::MeetingChanged(9))
        );
        let idle = serde_json::to_value(RecorderState::Idle).unwrap();
        assert_eq!(
            decode_event(events::RECORDER_STATE, &idle),
            Some(MeetingsEvent::RecorderState(RecorderState::Idle))
        );
        assert_eq!(decode_event("history-update-payload", &Value::Null), None);
    }

    #[test]
    fn retention_round_trips() {
        for d in guidy_core::meetings::settings::RETENTION_CHOICES {
            assert_eq!(parse_retention(&retention_value(d)), d);
        }
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty(" a "), Some("a".into()));
    }
}
