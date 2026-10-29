//! Meeting-notes UI logic with no GPUI dependency: core event decoding,
//! sidebar visibility, transcript grouping, labels and formatting, deep
//! links, external-URL checks and settings patches.

use guidy_core::downloads::{DownloadItem, DOWNLOADS_CHANGED_EVENT};
use guidy_core::meetings::processing::SpeakerModelStatus;
use guidy_core::meetings::settings::{MeetingSettings, UserEmail};
use guidy_core::meetings::types::{
    events, ActionItem, CloudTranscriptionOption, Meeting, MeetingId, MeetingSpeaker,
    MeetingStatus, Person, ProcessingProgress, ProvisionalLine, RecorderState, Segment, SpeakerId,
    StoredSummary, TranscriptionEngine,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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
    /// A transcript line from a run that has not merged yet.
    ProvisionalLine(ProvisionalLine),
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
    /// The shared download tracker changed (the full list).
    DownloadsChanged(Vec<DownloadItem>),
}

/// Whether [`decode_event`] handles events with this name.
pub fn is_meetings_event(name: &str) -> bool {
    name.starts_with("meetings://")
        || name.starts_with("llm-models://")
        || name == DOWNLOADS_CHANGED_EVENT
}

pub fn decode_event(name: &str, payload: &Value) -> Option<MeetingsEvent> {
    match name {
        events::RECORDER_STATE => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::RecorderState),
        events::PROCESSING_PROGRESS => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::Progress),
        events::PROVISIONAL_LINE => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::ProvisionalLine),
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
        DOWNLOADS_CHANGED_EVENT => serde_json::from_value(payload.clone())
            .ok()
            .map(MeetingsEvent::DownloadsChanged),
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

/// Where to create an API key, per LLM provider id (the "Get a key" link).
pub const PROVIDER_KEY_URLS: [(&str, &str); 7] = [
    ("anthropic", "https://console.anthropic.com/settings/keys"),
    ("openai", "https://platform.openai.com/api-keys"),
    ("openrouter", "https://openrouter.ai/settings/keys"),
    ("groq", "https://console.groq.com/keys"),
    ("cerebras", "https://cloud.cerebras.ai/platform"),
    ("zai", "https://z.ai/manage-apikey/apikey-list"),
    (
        "vercel_ai_gateway",
        "https://vercel.com/dashboard/ai-gateway/api-keys",
    ),
];

/// URLs the meetings UI may open: Linear pages, the settings panes and the
/// LLM providers' API key pages.
pub fn is_allowed_external_url(url: &str) -> bool {
    is_linear_url(url)
        || PROVIDER_KEY_URLS.iter().any(|(_, key_url)| *key_url == url)
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

/// How one speaker's label moved when a transcript or the speakers were redone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpeakerShift {
    Renamed {
        from: String,
        to: String,
    },
    Merged {
        from: String,
        into: String,
    },
    /// `count` lines changed speaker. Used when the source label is still
    /// present afterwards, so "merged" / "joined" would contradict that.
    Moved {
        count: usize,
        from: String,
        to: String,
    },
    Added {
        label: String,
    },
    Removed {
        label: String,
    },
}

/// One speaker's share of the talking, before and after a redo.
#[derive(Clone, Debug, PartialEq)]
pub struct ShareShift {
    pub label: String,
    pub before: f32,
    pub after: f32,
}

/// What a redo changed: talk-time share, speaker labels, and lines whose
/// speaker label differs on the same span.
#[derive(Clone, Debug, PartialEq)]
pub struct RedoDiff {
    pub shares: Vec<ShareShift>,
    pub speakers: Vec<SpeakerShift>,
    pub lines_changed: usize,
    /// Segment ids in `after` whose speaker label differs from the line on
    /// the same `(track, t0, t1)` before.
    pub changed_lines: Vec<i64>,
}

impl RedoDiff {
    pub fn is_empty(&self) -> bool {
        self.lines_changed == 0
            && self.speakers.is_empty()
            && self
                .shares
                .iter()
                .all(|s| (s.before - s.after).abs() < 0.005)
    }
}

fn speaker_label(
    id: Option<SpeakerId>,
    speakers: &[MeetingSpeaker],
    people: &[Person],
    you: &str,
) -> String {
    let Some(id) = id else {
        return String::new();
    };
    speakers
        .iter()
        .find(|s| s.id == id)
        .map(|s| speaker_name(s, people, None, you))
        .unwrap_or_default()
}

fn share_by_label(
    segments: &[Segment],
    speakers: &[MeetingSpeaker],
    people: &[Person],
    you: &str,
    duration_ms: Option<i64>,
) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = Vec::new();
    for talk in talk_time(segments, duration_ms).speakers {
        let label = speaker_label(talk.speaker_id, speakers, people, you);
        if label.is_empty() {
            continue;
        }
        if let Some(row) = out.iter_mut().find(|(l, _)| l == &label) {
            row.1 += talk.share;
        } else {
            out.push((label, talk.share));
        }
    }
    out
}

fn overlap_ms(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

/// Diff a transcript before and after a redo.
///
/// Each after-line maps to the before-line on the same track with the
/// largest overlap (a transcript redo re-runs VAD, so the spans rarely
/// match exactly). Overlap milliseconds are also split across every
/// before-line they touch, and that flow decides renames and merges.
///
/// "Joined" is only a label that did not exist before, "left" only one
/// that does not exist after. A label present on both sides is never a
/// merge or a join; its lines are described as moving.
pub fn redo_diff(
    before_segments: &[Segment],
    after_segments: &[Segment],
    before_speakers: &[MeetingSpeaker],
    after_speakers: &[MeetingSpeaker],
    people: &[Person],
    you: &str,
    duration_ms: Option<i64>,
) -> RedoDiff {
    let label_before = |id| speaker_label(id, before_speakers, people, you);
    let label_after = |id| speaker_label(id, after_speakers, people, you);
    let mut ms_votes: HashMap<(String, String), i64> = HashMap::new();
    let mut line_moves: HashMap<(String, String), usize> = HashMap::new();
    let mut changed_lines = Vec::new();
    for after in after_segments {
        let to = label_after(after.speaker_id);
        if to.is_empty() {
            continue;
        }
        let mut best: Option<(&Segment, i64)> = None;
        for before in before_segments {
            if before.track != after.track {
                continue;
            }
            let overlap = overlap_ms(before.t0_ms, before.t1_ms, after.t0_ms, after.t1_ms);
            if overlap <= 0 {
                continue;
            }
            let from = label_before(before.speaker_id);
            if from.is_empty() {
                continue;
            }
            *ms_votes.entry((from, to.clone())).or_default() += overlap;
            let take = best.is_none_or(|(winner, won)| {
                overlap > won
                    || (overlap == won && (before.t0_ms, before.id) < (winner.t0_ms, winner.id))
            });
            if take {
                best = Some((before, overlap));
            }
        }
        if let Some((before, _)) = best {
            let from = label_before(before.speaker_id);
            if !from.is_empty() && from != to {
                changed_lines.push(after.id);
                *line_moves.entry((from, to)).or_default() += 1;
            }
        }
    }

    let before_rows = share_by_label(before_segments, before_speakers, people, you, duration_ms);
    let after_rows = share_by_label(after_segments, after_speakers, people, you, duration_ms);
    let before_labels: HashSet<String> = before_rows.iter().map(|(l, _)| l.clone()).collect();
    let after_labels: HashSet<String> = after_rows.iter().map(|(l, _)| l.clone()).collect();

    let mut dominant_to: HashMap<String, (String, i64)> = HashMap::new();
    let mut dominant_from: HashMap<String, (String, i64)> = HashMap::new();
    for ((from, to), ms) in &ms_votes {
        let slot = dominant_to.entry(from.clone()).or_insert((to.clone(), *ms));
        if *ms > slot.1 || (*ms == slot.1 && to < &slot.0) {
            *slot = (to.clone(), *ms);
        }
        let slot = dominant_from
            .entry(to.clone())
            .or_insert((from.clone(), *ms));
        if *ms > slot.1 || (*ms == slot.1 && from < &slot.0) {
            *slot = (from.clone(), *ms);
        }
    }

    let mut speakers = Vec::new();
    let mut renamed_from = HashSet::new();
    let mut renamed_to = HashSet::new();
    let mut merged_from = HashSet::new();
    let mut explained_to = HashSet::new();

    // A pure rename: a label that vanished and a new label that received
    // its speech, each the other's dominant partner.
    let mut rename_pairs: Vec<(String, String, i64)> = Vec::new();
    for (from, (to, ms)) in &dominant_to {
        if before_labels.contains(from)
            && !after_labels.contains(from)
            && !before_labels.contains(to)
            && after_labels.contains(to)
            && dominant_from.get(to).is_some_and(|(src, _)| src == from)
        {
            rename_pairs.push((from.clone(), to.clone(), *ms));
        }
    }
    rename_pairs.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    for (from, to, _) in rename_pairs {
        if !renamed_from.insert(from.clone()) || !renamed_to.insert(to.clone()) {
            continue;
        }
        explained_to.insert(to.clone());
        speakers.push(SpeakerShift::Renamed { from, to });
    }

    // A merge: a label that vanished, absorbed by one that already existed.
    let mut merge_pairs: Vec<(String, String, i64)> = dominant_to
        .iter()
        .filter(|(from, (to, _))| {
            !renamed_from.contains(*from)
                && before_labels.contains(*from)
                && !after_labels.contains(*from)
                && before_labels.contains(to)
                && after_labels.contains(to)
        })
        .map(|(from, (to, ms))| (from.clone(), to.clone(), *ms))
        .collect();
    merge_pairs.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    for (from, to, _) in merge_pairs {
        if !merged_from.insert(from.clone()) {
            continue;
        }
        explained_to.insert(to.clone());
        speakers.push(SpeakerShift::Merged { from, into: to });
    }

    // Lines that changed speaker while the source label is still around
    // (a swap, or a split that only moved some of a speaker's lines).
    let mut moves: Vec<(String, String, usize)> = line_moves
        .into_iter()
        .filter(|((from, to), _)| from != to)
        .map(|((from, to), n)| (from, to, n))
        .filter(|(from, to, _)| !(renamed_from.contains(from) && renamed_to.contains(to)))
        .filter(|(from, _, _)| !merged_from.contains(from))
        .filter(|(from, _, _)| after_labels.contains(from))
        .collect();
    moves.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    for (from, to, count) in moves {
        explained_to.insert(to.clone());
        speakers.push(SpeakerShift::Moved { count, from, to });
    }

    let mut added: Vec<String> = after_labels
        .iter()
        .filter(|l| {
            !before_labels.contains(*l) && !renamed_to.contains(*l) && !explained_to.contains(*l)
        })
        .cloned()
        .collect();
    added.sort();
    for label in added {
        speakers.push(SpeakerShift::Added { label });
    }
    let mut removed: Vec<String> = before_labels
        .iter()
        .filter(|l| {
            !after_labels.contains(*l) && !renamed_from.contains(*l) && !merged_from.contains(*l)
        })
        .cloned()
        .collect();
    removed.sort();
    for label in removed {
        speakers.push(SpeakerShift::Removed { label });
    }

    let mut shares = Vec::new();
    for (label, after) in &after_rows {
        let before = before_rows
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, s)| *s)
            .unwrap_or(0.0);
        shares.push(ShareShift {
            label: label.clone(),
            before,
            after: *after,
        });
    }
    for (label, before) in &before_rows {
        if shares.iter().any(|s| s.label == *label) {
            continue;
        }
        shares.push(ShareShift {
            label: label.clone(),
            before: *before,
            after: 0.0,
        });
    }
    RedoDiff {
        shares,
        speakers,
        lines_changed: changed_lines.len(),
        changed_lines,
    }
}

/// Insert a streamed line in time order. The same span updates in place.
pub fn insert_provisional(lines: &mut Vec<ProvisionalLine>, line: ProvisionalLine) {
    if let Some(existing) = lines
        .iter_mut()
        .find(|l| l.track == line.track && l.t0_ms == line.t0_ms && l.t1_ms == line.t1_ms)
    {
        existing.text = line.text;
        return;
    }
    let at = lines.partition_point(|l| (l.t0_ms, l.track as u8) <= (line.t0_ms, line.track as u8));
    lines.insert(at, line);
}

/// The speaker's line closest to `t_ms` (a click on their talk-time lane),
/// if one is within `slack_ms` of it.
pub fn line_near(
    segments: &[Segment],
    speaker_id: Option<SpeakerId>,
    t_ms: i64,
    slack_ms: i64,
) -> Option<i64> {
    segments
        .iter()
        .filter(|s| s.speaker_id == speaker_id)
        .map(|s| {
            let gap = (s.t0_ms - t_ms).max(t_ms - s.t1_ms.max(s.t0_ms)).max(0);
            (gap, s.t0_ms, s.id)
        })
        .filter(|(gap, ..)| *gap <= slack_ms)
        .min()
        .map(|(.., id)| id)
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

/// The day of `at_ms` as people say it, relative to `now_ms` (local time):
/// "Today", "Tomorrow", "Yesterday", else a short date ("Mon, Oct 20",
/// with the year when it isn't this year's).
pub fn day_label(at_ms: i64, now_ms: i64, locale: &str) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let (Some(at), Some(now)) = (
        Local.timestamp_millis_opt(at_ms).single(),
        Local.timestamp_millis_opt(now_ms).single(),
    ) else {
        return String::new();
    };
    match (at.date_naive() - now.date_naive()).num_days() {
        0 => guidy_i18n::t("meetings.day.today"),
        1 => guidy_i18n::t("meetings.day.tomorrow"),
        -1 => guidy_i18n::t("meetings.day.yesterday"),
        _ => {
            let loc = crate::logic::flows::chrono_locale(locale);
            let fmt = if at.year() == now.year() {
                "%a, %b %-d"
            } else {
                "%a, %b %-d, %Y"
            };
            at.format_localized(fmt, loc).to_string()
        }
    }
}

/// Calendar events still worth starting notes for: not ended more than
/// [`UPCOMING_GRACE_MS`] ago, soonest first.
pub fn upcoming_to_show(
    events: &[guidy_core::meetings::types::CalendarEvent],
    now_ms: i64,
) -> Vec<guidy_core::meetings::types::CalendarEvent> {
    let mut out: Vec<_> = events
        .iter()
        .filter(|e| e.end_at >= now_ms - UPCOMING_GRACE_MS)
        .cloned()
        .collect();
    out.sort_by_key(|e| e.start_at);
    out
}

/// How long after its end a calendar event still shows as upcoming.
pub const UPCOMING_GRACE_MS: i64 = 60 * 60 * 1000;

/// The next calendar event to offer in the recorder: the first of
/// [`upcoming_to_show`] that hasn't ended.
pub fn next_event(
    events: &[guidy_core::meetings::types::CalendarEvent],
    now_ms: i64,
) -> Option<guidy_core::meetings::types::CalendarEvent> {
    upcoming_to_show(events, now_ms)
        .into_iter()
        .find(|e| e.end_at >= now_ms)
}

/// The day groups of the meeting list, newest first (as in Notes and Mail).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DayGroup {
    Today,
    Yesterday,
    Previous7Days,
    Previous30Days,
    Earlier,
}

impl DayGroup {
    /// The i18n key of the group header.
    pub fn key(self) -> &'static str {
        match self {
            Self::Today => "meetings.groups.today",
            Self::Yesterday => "meetings.groups.yesterday",
            Self::Previous7Days => "meetings.groups.previous7",
            Self::Previous30Days => "meetings.groups.previous30",
            Self::Earlier => "meetings.groups.earlier",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::Previous7Days => "previous7",
            Self::Previous30Days => "previous30",
            Self::Earlier => "earlier",
        }
    }
}

/// The group of a meeting started at `at_ms`, by local calendar day
/// relative to `now_ms`. Anything later than today counts as today.
pub fn day_group(at_ms: i64, now_ms: i64) -> DayGroup {
    use chrono::{Local, TimeZone};
    let (Some(at), Some(now)) = (
        Local.timestamp_millis_opt(at_ms).single(),
        Local.timestamp_millis_opt(now_ms).single(),
    ) else {
        return DayGroup::Earlier;
    };
    match (now.date_naive() - at.date_naive()).num_days() {
        ..=0 => DayGroup::Today,
        1 => DayGroup::Yesterday,
        2..=7 => DayGroup::Previous7Days,
        8..=30 => DayGroup::Previous30Days,
        _ => DayGroup::Earlier,
    }
}

/// One row of the meeting list: a day header or a meeting (its index in
/// the list's meetings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListRow {
    Header(DayGroup),
    Meeting(usize),
}

/// The list's rows for meetings sorted newest first: each day group's
/// header, then its meetings.
pub fn list_rows(meetings: &[Meeting], now_ms: i64) -> Vec<ListRow> {
    let mut rows = Vec::with_capacity(meetings.len() + 5);
    let mut last = None;
    for (i, m) in meetings.iter().enumerate() {
        let group = day_group(m.started_at, now_ms);
        if last != Some(group) {
            rows.push(ListRow::Header(group));
            last = Some(group);
        }
        rows.push(ListRow::Meeting(i));
    }
    rows
}

/// The meeting `step` places after `current` in `order` (clamped to the
/// ends); the first one when nothing is selected.
pub fn step_selection(
    order: &[MeetingId],
    current: Option<MeetingId>,
    step: isize,
) -> Option<MeetingId> {
    if order.is_empty() {
        return None;
    }
    let Some(at) = current.and_then(|id| order.iter().position(|m| *m == id)) else {
        return order.first().copied();
    };
    let next = (at as isize + step).clamp(0, order.len() as isize - 1) as usize;
    Some(order[next])
}

/// The meeting to select after `deleted` goes: the next one down, else the
/// one above.
pub fn after_delete(order: &[MeetingId], deleted: MeetingId) -> Option<MeetingId> {
    let at = order.iter().position(|m| *m == deleted)?;
    order
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| order.get(i)))
        .copied()
}

/// Where `query` occurs in `text`, ignoring case. Byte ranges into `text`.
pub fn find_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let (hay, needle) = (text.to_lowercase(), query.to_lowercase());
    // Lowercasing kept the byte offsets: search case-insensitively.
    // Otherwise (rare scripts) fall back to an exact search.
    let (hay, needle) = if hay.len() == text.len() {
        (hay, needle)
    } else {
        (text.to_string(), query.to_string())
    };
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            out.push(start..end);
        }
        from = end.max(start + 1);
        while from < hay.len() && !hay.is_char_boundary(from) {
            from += 1;
        }
    }
    out
}

/// Find in a transcript: each matching line's id with its ranges, in
/// transcript order.
pub fn find_in_segments(
    segments: &[Segment],
    query: &str,
) -> Vec<(i64, Vec<std::ops::Range<usize>>)> {
    segments
        .iter()
        .filter_map(|s| {
            let ranges = find_ranges(&s.text, query);
            (!ranges.is_empty()).then_some((s.id, ranges))
        })
        .collect()
}

/// A file name for an exported meeting: the title without characters that
/// file systems refuse, and `ext`.
pub fn export_file_name(title: &str, ext: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let base = if cleaned.is_empty() {
        "Meeting"
    } else {
        cleaned
    };
    format!("{base}.{ext}")
}

/// Notes and transcript in one Markdown document: the notes as they copy,
/// then the transcript lines under a "Transcript" heading (its own title
/// and date dropped, the notes already have them).
pub fn export_markdown(notes: Option<&str>, transcript: Option<&str>) -> String {
    let mut out = notes.map(|n| n.trim_end().to_string()).unwrap_or_default();
    if let Some(transcript) = transcript {
        let lines: Vec<&str> = transcript
            .lines()
            .skip_while(|l| !l.starts_with('['))
            .collect();
        if notes.is_none() {
            return transcript.to_string();
        }
        if !lines.is_empty() {
            out.push_str("\n\n## Transcript\n\n");
            for l in lines {
                out.push_str(l);
                out.push_str("  \n");
            }
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The stretch of meeting audio loaded at a time for playback.
pub const PLAYER_CHUNK_MS: i64 = 5 * 60 * 1000;

/// The chunk of audio to load to play from `at_ms` in a meeting
/// `duration_ms` long: `[at, at + PLAYER_CHUNK_MS)`, clipped to the end.
pub fn player_chunk(at_ms: i64, duration_ms: i64) -> (i64, i64) {
    let t0 = at_ms.clamp(0, duration_ms.max(0));
    (t0, (t0 + PLAYER_CHUNK_MS).min(duration_ms.max(t0)))
}

/// Mix two mono tracks into one, padding the shorter one with silence and
/// keeping the sum in range.
pub fn mix_tracks(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| {
            let s = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
            s.clamp(-1.0, 1.0)
        })
        .collect()
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

/// Why a transcript looks unreliable (see [`transcript_issue`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptIssue {
    /// The model got stuck repeating itself ("thank you thank you ...").
    Repetition,
    /// Far fewer words than that much talking holds: speech was missed or
    /// came out as noise.
    TooFewWords,
}

/// Talking time before the word rate means anything.
const MIN_SPEECH_MS_FOR_RATE: i64 = 90_000;
/// Conversational speech runs at 120-160 words a minute; well under half
/// of the lowest is missing text, not slow talking.
const MIN_WORDS_PER_MINUTE: f64 = 45.0;
/// The same 1-3 words this many times in a row is a decoding loop.
const LOOP_REPEATS: usize = 4;
/// Segments stuck in a loop before the whole transcript is suspect.
const MIN_LOOPED_SEGMENTS: usize = 2;

/// Whether a stored transcript looks like the model struggled, so the UI
/// can offer to transcribe it again (e.g. with a cloud model). Conservative:
/// it flags decoding loops and transcripts with far too few words for the
/// time people spent talking, never mere accents or jargon.
pub fn transcript_issue(segments: &[Segment]) -> Option<TranscriptIssue> {
    let looped = segments.iter().filter(|s| has_loop(&s.text)).count();
    if looped >= MIN_LOOPED_SEGMENTS {
        return Some(TranscriptIssue::Repetition);
    }
    let speech_ms: i64 = segments.iter().map(|s| (s.t1_ms - s.t0_ms).max(0)).sum();
    if speech_ms >= MIN_SPEECH_MS_FOR_RATE {
        let words: usize = segments
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum();
        let per_minute = words as f64 / (speech_ms as f64 / 60_000.0);
        if per_minute < MIN_WORDS_PER_MINUTE {
            return Some(TranscriptIssue::TooFewWords);
        }
    }
    None
}

/// Whether `text` repeats a run of 1-3 words [`LOOP_REPEATS`] times in a row
/// (case and punctuation ignored).
fn has_loop(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    (1..=3).any(|n| {
        let span = n * LOOP_REPEATS;
        words.len() >= span
            && (0..=words.len() - span).any(|start| {
                let unit = &words[start..start + n];
                (1..LOOP_REPEATS).all(|k| &words[start + k * n..start + (k + 1) * n] == unit)
            })
    })
}

/// A downloaded model a meeting can be transcribed with on this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelChoice {
    pub id: String,
    pub name: String,
    /// Meetings use it now (the meeting model, or its fallback).
    pub current: bool,
}

/// One entry of the "Redo transcript" menu.
#[derive(Debug, Clone, PartialEq)]
pub struct RedoChoice {
    pub label: String,
    /// `None`: shown but can't be picked (a cloud provider without a key).
    pub engine: Option<TranscriptionEngine>,
    pub current: bool,
    /// Uploads the audio: ask first.
    pub cloud: bool,
    /// For the confirmation: provider, model and estimated cost.
    pub provider: String,
    pub model: String,
    pub cost: Option<String>,
}

/// The "Redo transcript" menu: the downloaded models (the current one
/// first), then the cloud providers, priced for a meeting of `minutes`.
/// `no_key` is the label suffix for a provider without an API key.
pub fn redo_choices(
    local: &[LocalModelChoice],
    cloud: &[CloudTranscriptionOption],
    minutes: Option<f64>,
    no_key: &str,
) -> Vec<RedoChoice> {
    let mut out: Vec<RedoChoice> = local
        .iter()
        .map(|m| RedoChoice {
            label: m.name.clone(),
            engine: Some(TranscriptionEngine::LocalModel {
                model_id: m.id.clone(),
            }),
            current: m.current,
            cloud: false,
            provider: String::new(),
            model: m.name.clone(),
            cost: None,
        })
        .collect();
    for o in cloud {
        let cost = guidy_core::llm::usage::transcription_usd_per_min(&o.provider_id, &o.model)
            .zip(minutes)
            .map(|(rate, minutes)| format!("${:.2}", (rate * minutes).max(0.01)));
        let label = if !o.has_key {
            format!("{} · {no_key}", o.label)
        } else {
            match &cost {
                Some(cost) => format!("{} · {} · ~{cost}", o.label, o.model),
                None => format!("{} · {}", o.label, o.model),
            }
        };
        out.push(RedoChoice {
            label,
            engine: o.has_key.then(|| TranscriptionEngine::Cloud {
                provider_id: o.provider_id.clone(),
                model: o.model.clone(),
            }),
            current: false,
            cloud: true,
            provider: o.label.clone(),
            model: o.model.clone(),
            cost,
        });
    }
    out
}

/// How a transcript line was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineClick {
    /// A plain click (plays it): no selection, the next range starts here.
    Plain,
    /// Shift: everything from the anchor to here.
    Range,
    /// Command: this line in or out of the selection.
    Toggle,
}

/// The lines from `a` to `b` (either way round), in transcript order
/// (`order`: every line's id, by time). Empty if either isn't there.
pub fn line_range(order: &[i64], a: i64, b: i64) -> Vec<i64> {
    let (Some(i), Some(j)) = (
        order.iter().position(|&x| x == a),
        order.iter().position(|&x| x == b),
    ) else {
        return Vec::new();
    };
    order[i.min(j)..=i.max(j)].to_vec()
}

/// The selection and anchor after `click` on line `clicked`, as in a
/// file list: Shift extends from the anchor, Command adds or removes one
/// line (and moves the anchor), a plain click clears the selection and
/// moves the anchor. The selection is kept in transcript order.
pub fn select_lines(
    order: &[i64],
    selected: &[i64],
    anchor: Option<i64>,
    clicked: i64,
    click: LineClick,
) -> (Vec<i64>, Option<i64>) {
    match click {
        LineClick::Plain => (Vec::new(), Some(clicked)),
        LineClick::Range => {
            let from = anchor.filter(|a| order.contains(a)).unwrap_or(clicked);
            (line_range(order, from, clicked), Some(from))
        }
        LineClick::Toggle => {
            let on = !selected.contains(&clicked);
            let picked: Vec<i64> = order
                .iter()
                .copied()
                .filter(|id| {
                    if *id == clicked {
                        on
                    } else {
                        selected.contains(id)
                    }
                })
                .collect();
            (picked, Some(clicked))
        }
    }
}

/// One entry of the "Redo speakers" menu.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerRedoChoice {
    pub engine: guidy_core::meetings::types::DiarizationEngine,
    pub label: String,
    /// `false`: shown, can't be picked (Deepgram without a key).
    pub enabled: bool,
    /// Uploads the audio: ask first.
    pub cloud: bool,
    pub cost: Option<String>,
    /// The model meetings use (Meetings settings).
    pub current: bool,
}

/// The "Redo speakers" menu for a meeting of `minutes`: first what one
/// click does (the model chosen in settings, or the most accurate one when
/// that is the standard one: redoing with the same model changes little),
/// then the other models smallest first, then Deepgram. Labels come
/// translated: `standard` names the model when none is listed, `download`
/// contains `{{size}}`, `no_key` is the suffix for Deepgram without a key.
pub fn speaker_redo_choices(
    options: &guidy_core::meetings::types::DiarizationOptions,
    minutes: Option<f64>,
    standard: &str,
    download: &str,
    no_key: &str,
) -> Vec<SpeakerRedoChoice> {
    use guidy_core::meetings::types::{DiarizationEngine, SpeakerModelId};
    let mut out: Vec<SpeakerRedoChoice> = options
        .models
        .iter()
        .map(|m| SpeakerRedoChoice {
            engine: DiarizationEngine::Model(m.id),
            label: if m.installed {
                m.name.clone()
            } else {
                format!(
                    "{} · {}",
                    m.name,
                    download.replace("{{size}}", &format!("{} MB", m.size_mb))
                )
            },
            enabled: true,
            cloud: false,
            cost: None,
            current: m.id == options.default_model,
        })
        .collect();
    if out.is_empty() {
        out.push(SpeakerRedoChoice {
            engine: DiarizationEngine::default(),
            label: standard.to_string(),
            enabled: true,
            cloud: false,
            cost: None,
            current: true,
        });
    }
    let main = if options.default_model.is_standard() {
        SpeakerModelId::Resnet293
    } else {
        options.default_model
    };
    if let Some(i) = out
        .iter()
        .position(|c| c.engine == DiarizationEngine::Model(main))
    {
        let first = out.remove(i);
        out.insert(0, first);
    }
    let cost = options
        .deepgram_usd_per_min
        .zip(minutes)
        .map(|(rate, minutes)| format!("${:.2}", (rate * minutes).max(0.01)));
    let deepgram = match (&cost, options.deepgram_has_key) {
        (_, false) => format!("Deepgram · {no_key}"),
        (Some(cost), true) => format!("Deepgram · ~{cost}"),
        (None, true) => "Deepgram".to_string(),
    };
    out.push(SpeakerRedoChoice {
        engine: DiarizationEngine::Deepgram,
        label: deepgram,
        enabled: options.deepgram_has_key,
        cloud: true,
        cost,
        current: false,
    });
    out
}

/// A meeting's summary as Markdown, for pasting into notes or a doc: the
/// title, `date`, the summary, decisions, topics and action items (with
/// owner, due date and Linear issue). Empty sections are left out.
pub fn summary_markdown(
    summary: &StoredSummary,
    items: &[ActionItem],
    people: &[Person],
    user_display_name: &str,
    date: &str,
) -> String {
    summary_markdown_parts(summary, items, people, user_display_name, date, true)
}

/// `minutes` includes the minutes and open questions. Copy summary only
/// leaves them out.
pub fn summary_markdown_parts(
    summary: &StoredSummary,
    items: &[ActionItem],
    people: &[Person],
    user_display_name: &str,
    date: &str,
    minutes: bool,
) -> String {
    let mut out = format!(
        "# {}
",
        summary.title.trim()
    );
    if !date.is_empty() {
        out.push_str(&format!("\n{date}\n"));
    }
    let short = summary.short_summary.trim();
    if !short.is_empty() {
        out.push_str(&format!("\n## Summary\n\n{short}\n"));
    }
    let mut list = |heading: &str, lines: Vec<String>| {
        let lines: Vec<String> = lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if !lines.is_empty() {
            out.push_str(&format!("\n## {heading}\n\n"));
            for l in lines {
                out.push_str(&format!("- {}\n", l.trim()));
            }
        }
    };
    list("Decisions", summary.decisions.clone());
    list("Topics", summary.topics.clone());
    let user = guidy_core::meetings::summarize::user_name(user_display_name);
    list(
        "Action items",
        items
            .iter()
            .filter(|i| i.ticket_state != guidy_core::meetings::types::TicketState::Discarded)
            .map(|i| {
                let owner = if i.owner_is_user {
                    Some(user.clone())
                } else {
                    i.owner_person_id
                        .and_then(|id| people.iter().find(|p| p.id == id))
                        .map(|p| p.display_name.clone())
                        .or_else(|| i.owner_name.clone())
                };
                let mut line = format!("[ ] {}", i.text.trim());
                let details: Vec<String> = owner
                    .into_iter()
                    .chain(i.due.clone().map(|d| format!("due {d}")))
                    .filter(|d| !d.trim().is_empty())
                    .collect();
                if !details.is_empty() {
                    line.push_str(&format!(" ({})", details.join(", ")));
                }
                if let Some(issue) = &i.linear_issue_identifier {
                    line.push_str(&format!(" [{issue}]"));
                }
                line
            })
            .collect(),
    );
    drop(list);
    if minutes {
        if !summary.notes.is_empty() {
            out.push_str("\n## Minutes\n");
            for section in &summary.notes {
                let when = if section.start.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", section.start)
                };
                out.push_str(&format!("\n### {}{when}\n", section.heading));
                for point in &section.points {
                    out.push_str(&format!("- {point}\n"));
                }
                if let Some(outcome) = &section.outcome {
                    out.push_str(&format!("\nOutcome: {outcome}\n"));
                }
            }
        }
        let questions: Vec<String> = summary
            .open_questions
            .iter()
            .filter(|q| !q.trim().is_empty())
            .cloned()
            .collect();
        if !questions.is_empty() {
            out.push_str("\n## Open questions\n\n");
            for q in questions {
                out.push_str(&format!("- {}\n", q.trim()));
            }
        }
    }
    out
}

/// A meeting's transcript as text: the title and `date`, then one
/// `[mm:ss] Speaker: text` line per segment, with people's names (the same
/// lines the MCP server gives other apps).
pub fn transcript_text(
    title: &str,
    date: &str,
    segments: &[Segment],
    speakers: &[MeetingSpeaker],
    people: &[Person],
    user_display_name: &str,
) -> String {
    use guidy_core::meetings::summarize;
    let user = summarize::user_name(user_display_name);
    let names = summarize::speaker_names(speakers, people, &user);
    let mut out = format!("# {}\n", title.trim());
    if !date.is_empty() {
        out.push_str(&format!("\n{date}\n"));
    }
    out.push('\n');
    for line in summarize::transcript_lines(segments, &names, &user) {
        out.push_str(&line.render());
        out.push('\n');
    }
    out
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

/// Why an email address can't be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailError {
    Invalid,
    Duplicate,
}

/// A plausible email address: one `@`, something before it, a dotted domain
/// after it, no whitespace. Deliverability isn't checked.
pub fn is_valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && !email.chars().any(char::is_whitespace)
        && domain
            .split_once('.')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && !b.ends_with('.'))
}

/// `emails` with `email` (trimmed) appended under `label` (trimmed).
pub fn add_user_email(
    emails: &[UserEmail],
    label: &str,
    email: &str,
) -> Result<Vec<UserEmail>, EmailError> {
    let email = email.trim();
    if !is_valid_email(email) {
        return Err(EmailError::Invalid);
    }
    if emails.iter().any(|u| u.matches(email)) {
        return Err(EmailError::Duplicate);
    }
    let mut out = emails.to_vec();
    out.push(UserEmail {
        label: label.trim().to_string(),
        email: email.to_string(),
    });
    Ok(out)
}

/// People a person can be merged into (everyone else).
pub fn merge_targets(people: &[Person], from: i64) -> Vec<&Person> {
    people.iter().filter(|p| p.id != from).collect()
}

/// A processing run's status line: "Transcribing · 45/158 · 32%" (the
/// stage's items when it counts them, then how far the whole run is).
pub fn progress_label(stage: &str, p: Option<&ProcessingProgress>) -> String {
    let fraction = p.map(|p| p.fraction).unwrap_or(0.0).clamp(0.0, 1.0);
    match p.and_then(|p| p.done.zip(p.total)).filter(|(_, n)| *n > 0) {
        Some((done, total)) => format!("{stage} · {done}/{total} · {:.0}%", fraction * 100.0),
        None => format!("{stage} · {:.0}%", fraction * 100.0),
    }
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

    fn timed(t0_ms: i64, len_ms: i64, text: &str) -> Segment {
        Segment {
            id: 0,
            meeting_id: 1,
            track: Track::System,
            t0_ms,
            t1_ms: t0_ms + len_ms,
            text: text.into(),
            words: Vec::new(),
            speaker_id: None,
        }
    }

    #[test]
    fn meetings_group_by_day() {
        use chrono::{Local, TimeZone};
        let noon = Local
            .with_ymd_and_hms(2026, 10, 23, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let d = 24 * 3_600_000;
        assert_eq!(day_group(noon + 3_600_000, noon), DayGroup::Today);
        assert_eq!(day_group(noon - d, noon), DayGroup::Yesterday);
        assert_eq!(day_group(noon - 3 * d, noon), DayGroup::Previous7Days);
        assert_eq!(day_group(noon - 20 * d, noon), DayGroup::Previous30Days);
        assert_eq!(day_group(noon - 90 * d, noon), DayGroup::Earlier);
        let m = |id, at| {
            let mut m = crate::testing::meeting_fixture(id, "m", MeetingStatus::Processed);
            m.started_at = at;
            m
        };
        let meetings = [m(1, noon), m(2, noon - 60_000), m(3, noon - 3 * d)];
        assert_eq!(
            list_rows(&meetings, noon),
            vec![
                ListRow::Header(DayGroup::Today),
                ListRow::Meeting(0),
                ListRow::Meeting(1),
                ListRow::Header(DayGroup::Previous7Days),
                ListRow::Meeting(2),
            ]
        );
    }

    #[test]
    fn selection_steps_and_survives_a_delete() {
        let order = [5, 4, 3];
        assert_eq!(step_selection(&order, None, 1), Some(5));
        assert_eq!(step_selection(&order, Some(5), 1), Some(4));
        assert_eq!(step_selection(&order, Some(3), 1), Some(3));
        assert_eq!(step_selection(&order, Some(5), -1), Some(5));
        assert_eq!(step_selection(&[], Some(5), 1), None);
        assert_eq!(after_delete(&order, 4), Some(3));
        assert_eq!(after_delete(&order, 3), Some(4));
        assert_eq!(after_delete(&[3], 3), None);
    }

    #[test]
    fn find_ignores_case_and_returns_byte_ranges() {
        assert_eq!(find_ranges("Hello hello", "HELLO"), vec![0..5, 6..11]);
        assert_eq!(find_ranges("aaa", "aa"), vec![0..2]);
        assert!(find_ranges("abc", "  ").is_empty());
        assert_eq!(find_ranges("Café au lait", "au"), vec![6..8]);
        let mut a = timed(0, 1, "Ship it Friday");
        a.id = 1;
        let mut b = timed(1, 1, "No");
        b.id = 2;
        let found = find_in_segments(&[a, b], "friday");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].0, found[0].1[0].clone()), (1, 8..14));
    }

    #[test]
    fn exports_name_files_and_join_notes_and_transcript() {
        assert_eq!(
            export_file_name("Q3: plan/review?", "md"),
            "Q3- plan-review-.md"
        );
        assert_eq!(export_file_name("  ", "txt"), "Meeting.txt");
        let transcript = "# Sync\n\nMon\n\n[00:01] Ann: Hi\n[00:02] Bob: Hey\n";
        let md = export_markdown(Some("# Sync\n\n## Summary\n\nShort.\n"), Some(transcript));
        assert!(md.starts_with("# Sync"));
        assert!(md.contains("## Transcript\n\n[00:01] Ann: Hi  \n[00:02] Bob: Hey  \n"));
        assert_eq!(export_markdown(None, Some(transcript)), transcript);
        assert_eq!(export_markdown(Some("# A"), None), "# A\n");
    }

    #[test]
    fn player_chunks_and_mixing() {
        assert_eq!(player_chunk(0, 10_000), (0, 10_000));
        assert_eq!(
            player_chunk(1_000, 1_000_000),
            (1_000, 1_000 + PLAYER_CHUNK_MS)
        );
        assert_eq!(player_chunk(20_000, 10_000), (10_000, 10_000));
        assert_eq!(mix_tracks(&[0.5, 0.9], &[0.25]), vec![0.75, 0.9]);
        assert_eq!(mix_tracks(&[0.8], &[0.8]), vec![1.0]);
    }

    #[test]
    fn days_read_relative_to_today() {
        use chrono::{Local, TimeZone};
        guidy_i18n::set_language("en");
        let noon = Local
            .with_ymd_and_hms(2026, 10, 23, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let h = 3_600_000;
        assert_eq!(day_label(noon + 5 * h, noon, "en"), "Today");
        assert_eq!(day_label(noon + 20 * h, noon, "en"), "Tomorrow");
        assert_eq!(day_label(noon - 13 * h, noon, "en"), "Yesterday");
        assert_eq!(day_label(noon - 72 * h, noon, "en"), "Tue, Oct 20");
        assert_eq!(
            day_label(noon - 365 * 24 * h, noon, "en"),
            "Thu, Oct 23, 2025"
        );
    }

    #[test]
    fn upcoming_drops_events_over_an_hour_past_and_sorts() {
        let ev = |id: &str, start: i64, end: i64| guidy_core::meetings::types::CalendarEvent {
            id: id.into(),
            title: id.into(),
            start_at: start,
            end_at: end,
            meeting_url: None,
            attendees: Vec::new(),
        };
        let now = 10 * 3_600_000;
        let shown = upcoming_to_show(
            &[
                ev("tomorrow", now + 18 * 3_600_000, now + 19 * 3_600_000),
                ev("ended-long-ago", now - 3 * 3_600_000, now - 2 * 3_600_000),
                ev("just-ended", now - 3_600_000, now - 30 * 60_000),
            ],
            now,
        );
        let ids: Vec<_> = shown.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["just-ended", "tomorrow"]);
    }

    #[test]
    fn lines_are_selected_like_files_in_a_list() {
        let order = [10, 11, 12, 13, 14, 15];
        // A plain click only sets where a range starts.
        let (sel, anchor) = select_lines(&order, &[11], None, 12, LineClick::Plain);
        assert_eq!((sel, anchor), (vec![], Some(12)));
        // Shift: from there to here, either direction.
        let (sel, anchor) = select_lines(&order, &[], Some(12), 14, LineClick::Range);
        assert_eq!(sel, [12, 13, 14]);
        let (sel, _) = select_lines(&order, &sel, anchor, 10, LineClick::Range);
        assert_eq!(sel, [10, 11, 12]);
        // Without an anchor, shift selects just that line.
        let (sel, anchor) = select_lines(&order, &[], None, 13, LineClick::Range);
        assert_eq!((sel, anchor), (vec![13], Some(13)));
        // Command adds and removes single lines, kept in order.
        let (sel, anchor) = select_lines(&order, &[12, 13], Some(12), 15, LineClick::Toggle);
        assert_eq!((sel.clone(), anchor), (vec![12, 13, 15], Some(15)));
        let (sel, _) = select_lines(&order, &sel, anchor, 10, LineClick::Toggle);
        assert_eq!(sel, [10, 12, 13, 15]);
        let (sel, _) = select_lines(&order, &sel, anchor, 13, LineClick::Toggle);
        assert_eq!(sel, [10, 12, 15]);
        // A line that is gone (a reload) ranges from the clicked one.
        let (sel, _) = select_lines(&order, &[], Some(99), 11, LineClick::Range);
        assert_eq!(sel, [11]);
        assert!(line_range(&order, 10, 99).is_empty());
    }

    #[test]
    fn the_speaker_redo_menu_offers_the_models_then_deepgram() {
        use guidy_core::meetings::types::{
            DiarizationEngine, DiarizationOptions, SpeakerModelId, SpeakerModelOption,
        };
        let model = |id, name: &str, size_mb, installed| SpeakerModelOption {
            id,
            name: name.into(),
            size_mb,
            installed,
        };
        let mut options = DiarizationOptions {
            models: vec![
                model(SpeakerModelId::Resnet34, "ResNet34", 26, true),
                model(SpeakerModelId::TitanetLarge, "TitaNet", 97, false),
                model(SpeakerModelId::Resnet293, "ResNet293", 110, false),
            ],
            default_model: SpeakerModelId::Resnet34,
            deepgram_has_key: false,
            deepgram_usd_per_min: Some(0.0063),
        };
        let menu = |o: &DiarizationOptions| {
            speaker_redo_choices(
                o,
                Some(25.0),
                "Standard",
                "{{size}} download",
                "needs a key",
            )
        };
        let c = menu(&options);
        let labels: Vec<_> = c.iter().map(|c| c.label.as_str()).collect();
        // The standard model is the default: one click is the most accurate.
        assert_eq!(
            labels,
            [
                "ResNet293 · 110 MB download",
                "ResNet34",
                "TitaNet · 97 MB download",
                "Deepgram · needs a key"
            ]
        );
        assert_eq!(
            c[0].engine,
            DiarizationEngine::Model(SpeakerModelId::Resnet293)
        );
        assert!(c[1].current && !c[0].current);
        assert!(!c[3].enabled);
        // A model chosen in settings is what one click uses.
        options.default_model = SpeakerModelId::TitanetLarge;
        options.models[1].installed = true;
        options.deepgram_has_key = true;
        let c = menu(&options);
        assert_eq!(c[0].label, "TitaNet");
        assert!(c[0].current);
        assert_eq!(c[3].label, "Deepgram · ~$0.16");
        assert!(c[3].enabled && c[3].cloud);
        // Nothing listed (no meeting notes): still one choice.
        let c = menu(&DiarizationOptions::default());
        assert_eq!(c[0].label, "Standard");
    }

    #[test]
    fn the_redo_menu_lists_local_models_then_priced_cloud_providers() {
        let local = vec![
            LocalModelChoice {
                id: "qwen".into(),
                name: "Qwen3-ASR".into(),
                current: true,
            },
            LocalModelChoice {
                id: "nemo".into(),
                name: "Nemotron".into(),
                current: false,
            },
        ];
        let cloud = vec![
            CloudTranscriptionOption {
                provider_id: "groq".into(),
                label: "Groq".into(),
                model: "whisper-large-v3-turbo".into(),
                has_key: true,
            },
            CloudTranscriptionOption {
                provider_id: "openai".into(),
                label: "OpenAI".into(),
                model: "whisper-1".into(),
                has_key: false,
            },
        ];
        let c = redo_choices(&local, &cloud, Some(30.0), "needs an API key");
        let labels: Vec<_> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Qwen3-ASR",
                "Nemotron",
                "Groq · whisper-large-v3-turbo · ~$0.02",
                "OpenAI · needs an API key",
            ]
        );
        assert!(c[0].current && !c[0].cloud);
        assert_eq!(
            c[1].engine,
            Some(TranscriptionEngine::LocalModel {
                model_id: "nemo".into()
            })
        );
        assert!(c[2].cloud && c[2].engine.is_some());
        assert!(c[3].engine.is_none(), "no key: not pickable");
    }

    #[test]
    fn the_summary_copies_as_markdown_without_empty_sections() {
        use crate::testing::{action_item_fixture, person_fixture};
        let summary = StoredSummary {
            meeting_id: 1,
            title: "Weekly sync".into(),
            short_summary: "We agreed to ship.".into(),
            decisions: vec!["Ship on Friday".into(), " ".into()],
            topics: Vec::new(),
            generated_by: "groq/x".into(),
            generated_at: 0,
            usage: None,

            notes: vec![],
            open_questions: vec![],
        };
        let mut mine = action_item_fixture(1, 1, "Write the release notes");
        mine.owner_is_user = true;
        mine.due = Some("Friday".into());
        let mut ada = action_item_fixture(2, 1, "Review the PR");
        ada.owner_person_id = Some(7);
        ada.linear_issue_identifier = Some("ENG-12".into());
        let mut dropped = action_item_fixture(3, 1, "Discarded idea");
        dropped.ticket_state = guidy_core::meetings::types::TicketState::Discarded;
        let md = summary_markdown(
            &summary,
            &[mine, ada, dropped],
            &[person_fixture(7, "Ada", None)],
            "Imran",
            "October 23, 2026 14:19",
        );
        assert_eq!(
            md,
            "# Weekly sync\n\nOctober 23, 2026 14:19\n\n## Summary\n\nWe agreed to ship.\n\n\
             ## Decisions\n\n- Ship on Friday\n\n## Action items\n\n\
             - [ ] Write the release notes (Imran, due Friday)\n\
             - [ ] Review the PR (Ada) [ENG-12]\n"
        );
    }

    #[test]
    fn the_transcript_copies_with_times_and_names() {
        use crate::testing::{person_fixture, segment_fixture, speaker_fixture};
        let speakers = vec![
            speaker_fixture(5, 1, "Speaker 1", Some(7), false),
            speaker_fixture(6, 1, "me", None, true),
        ];
        let segments = vec![
            segment_fixture(1, 1, 6, 5_000, "Let's ship it."),
            segment_fixture(2, 1, 5, 65_000, "Agreed."),
        ];
        let text = transcript_text(
            "Weekly sync",
            "",
            &segments,
            &speakers,
            &[person_fixture(7, "Ada", None)],
            "Imran",
        );
        assert_eq!(
            text,
            "# Weekly sync\n\n[00:05] Imran: Let's ship it.\n[01:05] Ada: Agreed.\n"
        );
    }

    #[test]
    fn decoding_loops_are_flagged_but_one_emphatic_repeat_is_not() {
        assert!(has_loop("Thank you. Thank you. Thank you. Thank you."));
        assert!(has_loop("we we we we need to"));
        assert!(has_loop(
            "so the plan is the plan is the plan is the plan is"
        ));
        assert!(!has_loop("no no no, that's fine"));
        assert!(!has_loop("very very good, very very nice"));
        let ok = vec![
            timed(0, 4_000, "Thank you. Thank you. Thank you. Thank you."),
            timed(5_000, 3_000, "Let's get started with the roadmap."),
        ];
        assert_eq!(transcript_issue(&ok), None, "one loop is not enough");
        let looping = vec![
            timed(0, 4_000, "yeah yeah yeah yeah yeah"),
            timed(5_000, 4_000, "the the the the the"),
        ];
        assert_eq!(
            transcript_issue(&looping),
            Some(TranscriptIssue::Repetition)
        );
    }

    #[test]
    fn too_few_words_for_the_talking_time_is_flagged() {
        // Two minutes of talking with a handful of words.
        let sparse: Vec<Segment> = (0..12)
            .map(|i| timed(i * 10_000, 10_000, "uh hm"))
            .collect();
        assert_eq!(
            transcript_issue(&sparse),
            Some(TranscriptIssue::TooFewWords)
        );
        // The same time at a normal rate.
        let sentence = "we should ship the new onboarding flow next week after review";
        let normal: Vec<Segment> = (0..12)
            .map(|i| timed(i * 10_000, 10_000, &format!("{sentence} {sentence}")))
            .collect();
        assert_eq!(transcript_issue(&normal), None);
        // A short meeting says too little to judge.
        assert_eq!(transcript_issue(&[timed(0, 30_000, "ok")]), None);
    }

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
    fn progress_labels_show_counts_when_there_are_some() {
        let mut p = ProcessingProgress {
            meeting_id: 1,
            stage: ProcessingStage::Transcribe,
            fraction: 0.316,
            message: None,
            done: Some(45),
            total: Some(158),
        };
        assert_eq!(
            progress_label("Transcribing", Some(&p)),
            "Transcribing · 45/158 · 32%"
        );
        (p.done, p.total) = (None, None);
        assert_eq!(
            progress_label("Transcribing", Some(&p)),
            "Transcribing · 32%"
        );
        assert_eq!(progress_label("Processing", None), "Processing · 0%");
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
                done: None,
                total: None,
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
            description: None,
            title_source: Default::default(),
            has_summary: false,
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
    fn only_known_urls_are_opened() {
        assert!(is_linear_url("https://linear.app/acme/issue/ENG-1/fix-it"));
        assert!(is_allowed_external_url("https://linear.app/settings/api"));
        assert!(is_allowed_external_url(PRIVACY_MICROPHONE_URL));
        assert!(is_allowed_external_url(PRIVACY_CALENDARS_URL));
        assert!(is_allowed_external_url(PRIVACY_AUDIO_CAPTURE_URL));
        assert!(is_allowed_external_url(NOTIFICATIONS_SETTINGS_URL));
        for (_, url) in PROVIDER_KEY_URLS {
            assert!(is_allowed_external_url(url), "{url}");
        }
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
    fn email_validation_accepts_plain_addresses_only() {
        for ok in ["a@b.co", "first.last+tag@work.example.com"] {
            assert!(is_valid_email(ok), "{ok}");
        }
        for bad in [
            "", "a", "a@", "@b.co", "a@b", "a@b.", "a@.co", "a b@c.co", "a@b@c.co",
        ] {
            assert!(!is_valid_email(bad), "{bad}");
        }
    }

    #[test]
    fn adding_an_email_trims_and_rejects_duplicates() {
        let list = add_user_email(&[], " Work ", " me@work.example ").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "Work");
        assert_eq!(list[0].email, "me@work.example");
        assert_eq!(
            add_user_email(&list, "Other", "ME@work.example"),
            Err(EmailError::Duplicate)
        );
        assert_eq!(add_user_email(&list, "", "nope"), Err(EmailError::Invalid));
        let list = add_user_email(&list, "", "me@home.example").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].label, "");
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
    fn a_redo_reports_renames_merges_and_lines_that_moved() {
        let spk = |id, label: &str| MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        };
        let line = |id, t0, t1, spk| Segment {
            t1_ms: t1,
            ..seg(id, t0, Some(spk))
        };
        let before_speakers = vec![
            spk(1, "Speaker 1"),
            spk(2, "Speaker 2"),
            spk(3, "Speaker 3"),
        ];
        let after_speakers = vec![spk(1, "Speaker 1"), spk(2, "Alice")];
        // Speaker 2's line is now Alice. Speaker 3's line joined Speaker 1.
        let before = vec![
            line(1, 0, 4_000, 1),
            line(2, 5_000, 8_000, 2),
            line(3, 9_000, 10_000, 3),
        ];
        let after = vec![
            line(1, 0, 4_000, 1),
            line(2, 5_000, 8_000, 2),
            line(3, 9_000, 10_000, 1),
        ];
        let diff = redo_diff(
            &before,
            &after,
            &before_speakers,
            &after_speakers,
            &[],
            "You",
            None,
        );
        assert_eq!(diff.lines_changed, 2);
        assert_eq!(diff.changed_lines, vec![2, 3]);
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Renamed { from, to } if from == "Speaker 2" && to == "Alice"
        )));
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Merged { from, into } if from == "Speaker 3" && into == "Speaker 1"
        )));
        let alice = diff.shares.iter().find(|s| s.label == "Alice").unwrap();
        assert_eq!(alice.before, 0.0);
        assert!(alice.after > 0.0);
        assert!(!diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Added { label } if label == "Alice" || label == "Speaker 1"
        )));
        assert!(!diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Removed { label } if label == "Speaker 2" || label == "Speaker 3"
        )));
    }

    fn shift_line(id: i64, track: Track, t0: i64, t1: i64, spk: i64) -> Segment {
        Segment {
            id,
            meeting_id: 1,
            track,
            t0_ms: t0,
            t1_ms: t1,
            text: String::new(),
            words: vec![],
            speaker_id: Some(spk),
        }
    }

    #[test]
    fn a_speaker_swap_reports_lines_moving_not_a_join_and_a_merge() {
        let spk = |id, label: &str| MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        };
        // Speaker 3's line moves to Speaker 1. Soumyo's line moves to Speaker 3,
        // who is still in the meeting, so Speaker 3 neither left nor joined.
        let before_speakers = vec![spk(3, "Speaker 3"), spk(4, "Soumyo")];
        let after_speakers = vec![spk(1, "Speaker 1"), spk(3, "Speaker 3")];
        let before = vec![
            shift_line(1, Track::System, 0, 4_000, 3),
            shift_line(2, Track::System, 5_000, 9_000, 4),
        ];
        let after = vec![
            shift_line(11, Track::System, 0, 4_000, 1),
            shift_line(12, Track::System, 5_000, 9_000, 3),
        ];
        let diff = redo_diff(
            &before,
            &after,
            &before_speakers,
            &after_speakers,
            &[],
            "You",
            None,
        );
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Moved { count: 1, from, to }
                if from == "Speaker 3" && to == "Speaker 1"
        )));
        assert!(
            !diff.speakers.iter().any(|s| matches!(
                s,
                SpeakerShift::Merged { from, .. } if from == "Speaker 3"
            )),
            "Speaker 3 is still present: {diff:?}"
        );
        assert!(
            !diff.speakers.iter().any(|s| matches!(
                s,
                SpeakerShift::Added { label } | SpeakerShift::Removed { label } if label == "Speaker 3"
            )),
            "Speaker 3 neither joined nor left: {diff:?}"
        );
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Merged { from, into } if from == "Soumyo" && into == "Speaker 3"
        )));
        assert!(!diff
            .speakers
            .iter()
            .any(|s| matches!(s, SpeakerShift::Added { label } if label == "Speaker 1")));
    }

    #[test]
    fn a_transcript_redo_with_shifted_spans_does_not_invent_joins() {
        let spk = |id, label: &str| MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        };
        let speakers = vec![spk(1, "Speaker 1"), spk(2, "Speaker 2")];
        let before = vec![
            shift_line(1, Track::Mic, 0, 4_000, 1),
            shift_line(2, Track::System, 5_000, 9_000, 2),
        ];
        // VAD shifted the mic line and split the system line. Same speakers.
        let after = vec![
            shift_line(10, Track::Mic, 200, 3_800, 1),
            shift_line(11, Track::System, 5_100, 7_000, 2),
            shift_line(12, Track::System, 7_000, 9_200, 2),
        ];
        let diff = redo_diff(&before, &after, &speakers, &speakers, &[], "You", None);
        assert!(diff.speakers.is_empty(), "{diff:?}");
        assert_eq!(diff.lines_changed, 0, "{diff:?}");
        assert!(diff.changed_lines.is_empty());
    }

    #[test]
    fn a_pure_rename_is_not_a_join_and_a_leave() {
        let spk = |id, label: &str| MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        };
        let before_speakers = vec![spk(1, "Speaker 1"), spk(2, "Speaker 2")];
        let after_speakers = vec![spk(1, "Speaker 1"), spk(2, "Alice")];
        let before = vec![
            shift_line(1, Track::System, 0, 3_000, 1),
            shift_line(2, Track::System, 4_000, 8_000, 2),
        ];
        let after = vec![
            shift_line(1, Track::System, 0, 3_000, 1),
            shift_line(2, Track::System, 4_000, 8_000, 2),
        ];
        let diff = redo_diff(
            &before,
            &after,
            &before_speakers,
            &after_speakers,
            &[],
            "You",
            None,
        );
        assert_eq!(
            diff.speakers,
            vec![SpeakerShift::Renamed {
                from: "Speaker 2".into(),
                to: "Alice".into(),
            }]
        );
    }

    #[test]
    fn two_speakers_merging_into_one_is_not_a_join() {
        let spk = |id, label: &str| MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: None,
            match_confidence: None,
            confirmed: false,
            is_user: false,
        };
        let before_speakers = vec![
            spk(1, "Speaker 1"),
            spk(2, "Speaker 2"),
            spk(3, "Speaker 3"),
        ];
        let after_speakers = vec![spk(1, "Speaker 1")];
        let before = vec![
            shift_line(1, Track::System, 0, 4_000, 1),
            shift_line(2, Track::System, 5_000, 8_000, 2),
            shift_line(3, Track::System, 9_000, 12_000, 3),
        ];
        let after = vec![
            shift_line(1, Track::System, 0, 4_000, 1),
            shift_line(2, Track::System, 5_000, 8_000, 1),
            shift_line(3, Track::System, 9_000, 12_000, 1),
        ];
        let diff = redo_diff(
            &before,
            &after,
            &before_speakers,
            &after_speakers,
            &[],
            "You",
            None,
        );
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Merged { from, into } if from == "Speaker 2" && into == "Speaker 1"
        )));
        assert!(diff.speakers.iter().any(|s| matches!(
            s,
            SpeakerShift::Merged { from, into } if from == "Speaker 3" && into == "Speaker 1"
        )));
        assert!(!diff
            .speakers
            .iter()
            .any(|s| matches!(s, SpeakerShift::Added { .. } | SpeakerShift::Removed { .. })));
        assert_eq!(diff.lines_changed, 2);
    }

    #[test]
    fn provisional_lines_insert_in_time_order_and_update_the_same_span() {
        let line = |track, t0, t1, text: &str| ProvisionalLine {
            meeting_id: 1,
            track,
            t0_ms: t0,
            t1_ms: t1,
            text: text.into(),
        };
        let mut lines = Vec::new();
        // Cloud batches finish a track before the other, so system can arrive first.
        insert_provisional(&mut lines, line(Track::System, 3_000, 5_000, "system"));
        insert_provisional(&mut lines, line(Track::Mic, 0, 2_000, "mic"));
        insert_provisional(&mut lines, line(Track::Mic, 8_000, 10_000, "mic later"));
        insert_provisional(
            &mut lines,
            line(Track::System, 3_000, 5_000, "system revised"),
        );
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["mic", "system revised", "mic later"]);
        let starts: Vec<_> = lines.iter().map(|l| l.t0_ms).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]));
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

    #[test]
    fn a_lane_click_finds_the_speakers_nearest_line() {
        let segments = vec![
            spoken(1, 0, 4_000, Some(1), "a"),
            spoken(2, 4_000, 9_000, Some(2), "b"),
            spoken(3, 10_000, 12_000, Some(1), "c"),
            spoken(4, 20_000, 21_000, None, "d"),
        ];
        // Inside a line, or just past one.
        assert_eq!(line_near(&segments, Some(1), 11_000, 0), Some(3));
        assert_eq!(line_near(&segments, Some(1), 5_000, 2_000), Some(1));
        // Another speaker's line is skipped.
        assert_eq!(line_near(&segments, Some(1), 6_000, 5_000), Some(1));
        assert_eq!(line_near(&segments, None, 19_500, 1_000), Some(4));
        // Nothing close enough.
        assert_eq!(line_near(&segments, Some(2), 15_000, 1_000), None);
    }
}
