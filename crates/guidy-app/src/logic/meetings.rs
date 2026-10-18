//! Meeting-notes UI logic with no GPUI dependency: core event decoding,
//! sidebar visibility, transcript grouping, labels and formatting, deep
//! links, external-URL checks and settings patches.

use guidy_core::meetings::processing::SpeakerModelStatus;
use guidy_core::meetings::settings::MeetingSettings;
use guidy_core::meetings::types::{
    events, Meeting, MeetingId, MeetingSpeaker, MeetingStatus, Person, ProcessingProgress,
    RecorderState, Segment, SpeakerId,
};
use serde_json::Value;

/// `meetings://summary-progress` (core `meetings::summarize::events`).
pub const SUMMARY_PROGRESS_EVENT: &str = guidy_core::meetings::summarize::events::SUMMARY_PROGRESS;
/// `meetings://speaker-model-status`.
pub const SPEAKER_MODEL_STATUS_EVENT: &str =
    guidy_core::meetings::processing::SPEAKER_MODEL_STATUS_EVENT;
/// Built-in LLM model downloads (`llm::local_models::events`).
pub const LLM_DOWNLOAD_PROGRESS_EVENT: &str =
    guidy_core::llm::local_models::events::DOWNLOAD_PROGRESS;
pub const LLM_MODELS_CHANGED_EVENT: &str = guidy_core::llm::local_models::events::CHANGED;

/// A meeting-notes core event, decoded for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum MeetingsEvent {
    RecorderState(RecorderState),
    Progress(ProcessingProgress),
    MeetingChanged(MeetingId),
    PeopleChanged,
    /// The meetings manager is registered (startup or retry finished).
    Ready,
    /// The meetings manager failed to start (the error text).
    InitFailed(String),
    /// The meetings manager can't start because of the data key: it is gone
    /// but meeting data exists, or it doesn't decrypt it (the error text).
    /// Offers a reset.
    DataKeyMissing(String),
    /// LLM calls of a running summary: `done` of `total`.
    SummaryProgress {
        meeting_id: MeetingId,
        done: usize,
        total: usize,
    },
    SpeakerModelStatus(SpeakerModelStatus),
    /// A built-in LLM model download advanced (0.0..=1.0).
    LocalModelProgress {
        model_id: String,
        fraction: f32,
    },
    /// A built-in LLM model download started/finished/failed, or a model was
    /// deleted.
    LocalModelsChanged,
    /// UI-only: the Linear API key was saved or removed.
    LinearKeyChanged,
}

/// Whether [`decode_event`] handles events with this name.
pub fn is_meetings_event(name: &str) -> bool {
    name.starts_with("meetings://") || name.starts_with("llm-models://")
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
        guidy_core::MEETINGS_READY_EVENT => Some(MeetingsEvent::Ready),
        guidy_core::MEETINGS_INIT_FAILED_EVENT => {
            Some(MeetingsEvent::InitFailed(payload_text(payload)))
        }
        guidy_core::MEETINGS_DATA_KEY_MISSING_EVENT => {
            Some(MeetingsEvent::DataKeyMissing(payload_text(payload)))
        }
        SUMMARY_PROGRESS_EVENT => {
            let p: guidy_core::meetings::summarize::SummaryProgress =
                serde_json::from_value(payload.clone()).ok()?;
            Some(MeetingsEvent::SummaryProgress {
                meeting_id: p.meeting_id,
                done: p.done,
                total: p.total,
            })
        }
        SPEAKER_MODEL_STATUS_EVENT => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::SpeakerModelStatus),
        LLM_DOWNLOAD_PROGRESS_EVENT => {
            let p: guidy_core::managers::model::DownloadProgress =
                serde_json::from_value(payload.clone()).ok()?;
            let fraction = if p.total > 0 {
                (p.downloaded as f64 / p.total as f64).clamp(0.0, 1.0) as f32
            } else {
                (p.percentage / 100.0).clamp(0.0, 1.0) as f32
            };
            Some(MeetingsEvent::LocalModelProgress {
                model_id: p.model_id,
                fraction,
            })
        }
        LLM_MODELS_CHANGED_EVENT => Some(MeetingsEvent::LocalModelsChanged),
        _ => None,
    }
}

fn payload_text(payload: &Value) -> String {
    payload
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| payload.to_string())
}

/// Where meeting-notes startup is, as the UI tracks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeetingsInit {
    /// Never used: core starts it when meeting notes are turned on.
    NotStarted,
    Starting,
    Ready,
    /// Any other start failure: Retry only (the notes may be readable).
    Failed(String),
    /// The data key is gone (or doesn't decrypt the meeting data) but
    /// meeting data exists: Retry or reset.
    DataKeyMissing(String),
}

impl MeetingsInit {
    pub fn from_core(s: guidy_core::MeetingsInitStatus) -> Self {
        match s {
            guidy_core::MeetingsInitStatus::Ready => Self::Ready,
            guidy_core::MeetingsInitStatus::Failed(e) => Self::Failed(e),
            // Unsupported systems hide the section; treat as ready.
            guidy_core::MeetingsInitStatus::Unsupported => Self::Ready,
            guidy_core::MeetingsInitStatus::Starting => Self::Starting,
            guidy_core::MeetingsInitStatus::Off => Self::NotStarted,
            guidy_core::MeetingsInitStatus::DataKeyMissing(e) => Self::DataKeyMissing(e),
        }
    }

    /// The status to show after a failed reset: core's (`status`), with the
    /// reset's error as the reason. Core keeps `DataKeyMissing` when nothing
    /// changed (Reset is offered again) and is `Failed` when the reset
    /// partly happened (Retry only).
    pub fn after_failed_reset(status: Self, error: String) -> Self {
        match status {
            Self::DataKeyMissing(_) => Self::DataKeyMissing(error),
            Self::Failed(_) => Self::Failed(error),
            other => other,
        }
    }
}

/// Summary progress fraction (0..=1), `None` before the plan is known.
pub fn summary_fraction(done: usize, total: usize) -> Option<f32> {
    (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
}

// -- deep links -----------------------------------------------------------------

/// `guidy://meetings/<id>?t=<ms>`: open a meeting at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeetingLink {
    pub meeting_id: MeetingId,
    pub t_ms: Option<i64>,
}

fn parse_digits(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 18 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parse a `guidy://` URL strictly. Only `guidy://meetings/<id>` with an
/// optional `?t=<ms>` is accepted: ids and times are plain decimal
/// integers, and anything else (other paths, extra query parameters,
/// fragments, signs, whitespace) is rejected. A link only ever navigates.
pub fn parse_deep_link(url: &str) -> Option<MeetingLink> {
    let rest = url.strip_prefix("guidy://meetings/")?;
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let meeting_id = parse_digits(path).filter(|id| *id > 0)?;
    let t_ms = match query {
        None => None,
        Some(q) => Some(parse_digits(q.strip_prefix("t=")?)?),
    };
    Some(MeetingLink { meeting_id, t_ms })
}

// -- external URLs ----------------------------------------------------------------

/// System Settings privacy panes for the meeting permissions.
pub const PRIVACY_CALENDARS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars";
pub const PRIVACY_MICROPHONE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
pub const PRIVACY_AUDIO_CAPTURE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture";
/// Notifications aren't a privacy pane; once the prompt has been answered,
/// this pane is the only place to change the answer.
pub const NOTIFICATIONS_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

/// An `https://linear.app/...` URL (issue links come from Linear's API but
/// are still checked before being opened).
pub fn is_linear_url(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://linear.app/") else {
        return false;
    };
    !path.is_empty()
        && !path.starts_with('/')
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '\\' && c != '@')
}

/// URLs the meetings UI may open: Linear pages and the settings panes.
pub fn is_allowed_external_url(url: &str) -> bool {
    is_linear_url(url)
        || [
            PRIVACY_CALENDARS_URL,
            PRIVACY_MICROPHONE_URL,
            PRIVACY_AUDIO_CAPTURE_URL,
            NOTIFICATIONS_SETTINGS_URL,
        ]
        .contains(&url)
}

// -- settings patches ---------------------------------------------------------------

/// The top-level `MeetingSettings` fields that differ between `before` and
/// `after`, as a JSON object (`update_setting("meetings", patch)` merges it
/// into the stored object, so two views editing different fields never
/// overwrite each other).
pub fn meetings_patch(before: &MeetingSettings, after: &MeetingSettings) -> Value {
    let (Ok(Value::Object(b)), Ok(Value::Object(a))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Value::Object(Default::default());
    };
    Value::Object(a.into_iter().filter(|(k, v)| b.get(k) != Some(v)).collect())
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

/// Who spoke when, for the meeting's talk-time chart.
#[derive(Clone, Debug, PartialEq)]
pub struct TalkTime {
    /// The timeline's length: the recording, else the last segment's end.
    pub total_ms: i64,
    pub words: usize,
    /// Longest talker first; unattributed speech last.
    pub speakers: Vec<SpeakerTalk>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerTalk {
    pub speaker_id: Option<SpeakerId>,
    /// Speaking spans `(t0_ms, t1_ms)`, with short pauses merged.
    pub spans: Vec<(i64, i64)>,
    pub spoken_ms: i64,
    /// Fraction of all speech in the meeting.
    pub share: f32,
}

/// Pauses shorter than this join two spans, so the chart stays readable
/// (it grows with long meetings to bound the number of bars).
fn span_merge_gap(total_ms: i64) -> i64 {
    (total_ms / 300).max(1_500)
}

pub fn talk_time(segments: &[Segment], duration_ms: Option<i64>) -> TalkTime {
    let end = segments.iter().map(|s| s.t1_ms).max().unwrap_or(0);
    let total_ms = duration_ms.unwrap_or(end).max(end);
    let gap = span_merge_gap(total_ms);
    let mut sorted: Vec<&Segment> = segments.iter().collect();
    sorted.sort_by_key(|s| (s.t0_ms, s.id));
    let mut speakers: Vec<SpeakerTalk> = Vec::new();
    for seg in sorted {
        let (t0, t1) = (seg.t0_ms, seg.t1_ms.max(seg.t0_ms));
        let i = match speakers.iter().position(|s| s.speaker_id == seg.speaker_id) {
            Some(i) => i,
            None => {
                speakers.push(SpeakerTalk {
                    speaker_id: seg.speaker_id,
                    spans: Vec::new(),
                    spoken_ms: 0,
                    share: 0.0,
                });
                speakers.len() - 1
            }
        };
        let talk = &mut speakers[i];
        talk.spoken_ms += t1 - t0;
        match talk.spans.last_mut() {
            Some(last) if t0 - last.1 <= gap => last.1 = last.1.max(t1),
            _ => talk.spans.push((t0, t1)),
        }
    }
    let spoken: i64 = speakers.iter().map(|s| s.spoken_ms).sum();
    for s in &mut speakers {
        s.share = if spoken > 0 {
            s.spoken_ms as f32 / spoken as f32
        } else {
            0.0
        };
    }
    speakers.sort_by_key(|s| (s.speaker_id.is_none(), std::cmp::Reverse(s.spoken_ms)));
    TalkTime {
        total_ms,
        words: segments
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum(),
        speakers,
    }
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

/// A speaker's cluster label for display: "Speaker N" is translated
/// (`meetings.speaker.unnamed`), any other label is shown as is.
pub fn cluster_label(speaker: &MeetingSpeaker) -> String {
    match speaker.speaker_number() {
        Some(n) => guidy_i18n::t_args("meetings.speaker.unnamed", &[("n", &n.to_string())]),
        None => speaker.cluster_label.clone(),
    }
}

/// The label for a speaker: the matched person, the user (mic track), or
/// the (translated) cluster label ("Speaker 2").
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
    cluster_label(speaker)
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

/// Recorded/failed meetings can be (re)processed; processed ones re-run
/// only while their audio exists (retention deletes it, and re-running
/// without it would fail and mark a good meeting failed).
pub fn can_process(m: &Meeting) -> bool {
    match m.status {
        MeetingStatus::Recorded | MeetingStatus::Failed => true,
        MeetingStatus::Processed => m.mic_audio_path.is_some() || m.system_audio_path.is_some(),
        MeetingStatus::Recording | MeetingStatus::Processing => false,
    }
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

    #[test]
    fn a_failed_reset_shows_cores_status_with_the_error() {
        let after = |s| MeetingsInit::after_failed_reset(s, "keychain locked".into());
        // Nothing changed: Reset is still offered.
        assert_eq!(
            after(MeetingsInit::DataKeyMissing("gone".into())),
            MeetingsInit::DataKeyMissing("keychain locked".into())
        );
        // Partly done: Retry only.
        assert_eq!(
            after(MeetingsInit::Failed("x".into())),
            MeetingsInit::Failed("keychain locked".into())
        );
        assert_eq!(after(MeetingsInit::Starting), MeetingsInit::Starting);
        assert_eq!(after(MeetingsInit::Ready), MeetingsInit::Ready);
    }

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
    fn processed_meetings_without_audio_cannot_be_reprocessed() {
        let mut m = Meeting {
            id: 1,
            title: "Sync".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: 0,
            ended_at: Some(1),
            mic_audio_path: Some("1-mic.gopus".into()),
            system_audio_path: Some("1-system.gopus".into()),
            status: MeetingStatus::Processed,
            error: None,
            processing_stage: Some("done".into()),
        };
        assert!(can_process(&m));
        // Retention deleted the audio.
        m.mic_audio_path = None;
        m.system_audio_path = None;
        assert!(!can_process(&m));
        for status in [MeetingStatus::Recorded, MeetingStatus::Failed] {
            m.status = status;
            assert!(can_process(&m));
        }
        for status in [MeetingStatus::Recording, MeetingStatus::Processing] {
            m.status = status;
            assert!(!can_process(&m));
        }
    }

    #[test]
    fn retention_round_trips() {
        for d in guidy_core::meetings::settings::RETENTION_CHOICES {
            assert_eq!(parse_retention(&retention_value(d)), d);
        }
        assert_eq!(non_empty("  "), None);
        assert_eq!(non_empty(" a "), Some("a".into()));
    }

    #[test]
    fn deep_links_parse_strictly() {
        let ok = |url: &str, id: i64, t: Option<i64>| {
            assert_eq!(
                parse_deep_link(url),
                Some(MeetingLink {
                    meeting_id: id,
                    t_ms: t
                }),
                "{url}"
            );
        };
        ok("guidy://meetings/42", 42, None);
        ok("guidy://meetings/42?t=65000", 42, Some(65_000));
        ok("guidy://meetings/7?t=0", 7, Some(0));
        for bad in [
            "",
            "guidy://",
            "guidy://meetings",
            "guidy://meetings/",
            "guidy://meetings/0",
            "guidy://meetings/-3",
            "guidy://meetings/+3",
            "guidy://meetings/3.5",
            "guidy://meetings/3/",
            "guidy://meetings/3/delete",
            "guidy://meetings/3?t=",
            "guidy://meetings/3?t=-1",
            "guidy://meetings/3?t=1e3",
            "guidy://meetings/3?t=10&record=1",
            "guidy://meetings/3?x=1",
            "guidy://meetings/3#t=1",
            "guidy://meetings/3?t=10#x",
            "guidy://meetings/ 3",
            "guidy://meetings/٣",
            "guidy://meetings/99999999999999999999",
            "guidy://record/3",
            "guidy://settings",
            "GUIDY://meetings/3",
            "https://meetings/3",
            "guidy:meetings/3",
        ] {
            assert_eq!(parse_deep_link(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_linear_and_privacy_urls_are_opened() {
        assert!(is_linear_url("https://linear.app/acme/issue/ENG-1/fix-it"));
        assert!(is_allowed_external_url("https://linear.app/settings/api"));
        assert!(is_allowed_external_url(PRIVACY_MICROPHONE_URL));
        assert!(is_allowed_external_url(PRIVACY_CALENDARS_URL));
        assert!(is_allowed_external_url(PRIVACY_AUDIO_CAPTURE_URL));
        assert!(is_allowed_external_url(NOTIFICATIONS_SETTINGS_URL));
        for bad in [
            "http://linear.app/acme",
            "https://linear.app/",
            "https://linear.app",
            "https://linear.app.evil.com/x",
            "https://linear.app@evil.com/x",
            "https://evil.com/https://linear.app/x",
            "https://linear.app//evil.com",
            "https://linear.app/x\\@evil.com",
            "https://linear.app/x y",
            "https://LINEAR.app/x",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
        ] {
            assert!(!is_allowed_external_url(bad), "{bad}");
        }
    }

    #[test]
    fn meetings_patch_holds_only_changed_fields() {
        let before = MeetingSettings::default();
        let mut after = before.clone();
        assert_eq!(meetings_patch(&before, &after), serde_json::json!({}));
        after.enabled = !before.enabled;
        after.audio_retention_days = if before.audio_retention_days == Some(3) {
            None
        } else {
            Some(3)
        };
        let patch = meetings_patch(&before, &after);
        let keys: Vec<&String> = patch.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 2, "{patch}");
        assert_eq!(patch["enabled"], after.enabled);
        assert_eq!(
            patch["audio_retention_days"],
            serde_json::to_value(after.audio_retention_days).unwrap()
        );
    }

    #[test]
    fn decodes_ready_summary_speaker_and_llm_events() {
        assert!(is_meetings_event("meetings://ready"));
        assert!(is_meetings_event("llm-models://changed"));
        assert!(!is_meetings_event("settings-changed"));
        assert_eq!(
            decode_event("meetings://ready", &Value::Null),
            Some(MeetingsEvent::Ready)
        );
        assert_eq!(
            decode_event(
                "meetings://init-failed",
                &serde_json::json!("keychain denied")
            ),
            Some(MeetingsEvent::InitFailed("keychain denied".into()))
        );
        assert_eq!(
            decode_event(
                guidy_core::MEETINGS_DATA_KEY_MISSING_EVENT,
                &serde_json::json!("key gone")
            ),
            Some(MeetingsEvent::DataKeyMissing("key gone".into()))
        );
        use guidy_core::MeetingsInitStatus as S;
        assert_eq!(MeetingsInit::from_core(S::Off), MeetingsInit::NotStarted);
        assert_eq!(
            MeetingsInit::from_core(S::DataKeyMissing("k".into())),
            MeetingsInit::DataKeyMissing("k".into())
        );
        assert_eq!(
            decode_event(
                SUMMARY_PROGRESS_EVENT,
                &serde_json::json!({"meeting_id": 3, "done": 2, "total": 5})
            ),
            Some(MeetingsEvent::SummaryProgress {
                meeting_id: 3,
                done: 2,
                total: 5
            })
        );
        let st = SpeakerModelStatus::Downloading {
            downloaded: 5,
            total: 10,
        };
        assert_eq!(
            decode_event(
                SPEAKER_MODEL_STATUS_EVENT,
                &serde_json::to_value(&st).unwrap()
            ),
            Some(MeetingsEvent::SpeakerModelStatus(st))
        );
        assert_eq!(
            decode_event(
                LLM_DOWNLOAD_PROGRESS_EVENT,
                &serde_json::json!({"model_id": "m", "downloaded": 25, "total": 100, "percentage": 25.0})
            ),
            Some(MeetingsEvent::LocalModelProgress {
                model_id: "m".into(),
                fraction: 0.25
            })
        );
        assert_eq!(
            decode_event(
                LLM_MODELS_CHANGED_EVENT,
                &serde_json::json!({"model_id": "m", "state": "completed", "error": null})
            ),
            Some(MeetingsEvent::LocalModelsChanged)
        );
        assert_eq!(summary_fraction(0, 0), None);
        assert_eq!(summary_fraction(1, 4), Some(0.25));
    }

    #[test]
    fn unnamed_speakers_are_translated() {
        guidy_i18n::set_language("en");
        let mut s = speaker(None, false, false);
        s.cluster_label = "Speaker 3".into();
        assert_eq!(cluster_label(&s), "Speaker 3");
        assert_eq!(speaker_name(&s, &[], None, "You"), "Speaker 3");
        s.cluster_label = "Guest".into();
        assert_eq!(cluster_label(&s), "Guest");
    }

    fn spoken(id: i64, t0: i64, t1: i64, spk: Option<i64>, text: &str) -> Segment {
        Segment {
            t1_ms: t1,
            text: text.into(),
            ..seg(id, t0, spk)
        }
    }

    #[test]
    fn talk_time_merges_close_spans_and_ranks_speakers() {
        let segments = vec![
            spoken(1, 0, 4_000, Some(1), "one two three"),
            // 0.5 s pause: same span.
            spoken(2, 4_500, 6_000, Some(1), "four"),
            spoken(3, 6_000, 16_000, Some(2), "five six"),
            // Long pause: a new span.
            spoken(4, 30_000, 32_000, Some(1), "seven"),
            spoken(5, 33_000, 34_000, None, "eight"),
        ];
        let tt = talk_time(&segments, Some(40_000));
        assert_eq!(tt.total_ms, 40_000);
        assert_eq!(tt.words, 8);
        // Speaker 2 talked longest (10 s vs 7.5 s); unattributed comes last.
        let ids: Vec<_> = tt.speakers.iter().map(|s| s.speaker_id).collect();
        assert_eq!(ids, vec![Some(2), Some(1), None]);
        let one = &tt.speakers[1];
        assert_eq!(one.spans, vec![(0, 6_000), (30_000, 32_000)]);
        assert_eq!(one.spoken_ms, 7_500);
        let shares: f32 = tt.speakers.iter().map(|s| s.share).sum();
        assert!((shares - 1.0).abs() < 1e-4);
    }

    #[test]
    fn talk_time_without_a_duration_uses_the_last_segment() {
        let tt = talk_time(&[spoken(1, 1_000, 9_000, Some(1), "hi")], None);
        assert_eq!(tt.total_ms, 9_000);
        assert_eq!(tt.speakers[0].share, 1.0);
        assert_eq!(talk_time(&[], None).speakers.len(), 0);
    }
}
