//! Names meeting apps showed for the active speaker
//! ([`crate::meetings::native::speaker_hints`]), read back as a suggestion: each
//! speaker of the other side gets the name that was showing longest while they
//! spoke. Only a suggestion the user can edit: without a hints file nothing
//! changes, and diarization never sees it.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::meetings::audio_store;
use crate::meetings::manager::{NameSuggestion, SuggestionSource};
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::{MeetingId, SpeakerId, Track};

/// A sample stands for this long: they are taken about once a second.
const SAMPLE_MS: i64 = 1_000;
/// Less overlap than this says nothing about who a speaker is.
const MIN_OVERLAP_MS: i64 = 2_000;

/// One sample: the name showing `t` ms into the recording.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Hint {
    pub t: i64,
    pub name: String,
}

/// The hints of a recording. A missing file is none; a torn or foreign line
/// (a crash mid-write) is skipped.
pub fn read_hints(audio_dir: &Path, meeting_id: MeetingId) -> Vec<Hint> {
    let path = audio_dir.join(audio_store::speakers_log_name(meeting_id));
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The name showing longest during `spans` (`[t0, t1)` ms), with
/// [`MIN_OVERLAP_MS`] at least. A tie goes to the alphabetically first, so the
/// answer doesn't change between runs.
pub fn best_name(hints: &[Hint], spans: &[(i64, i64)]) -> Option<String> {
    let mut totals: std::collections::BTreeMap<&str, i64> = Default::default();
    for hint in hints {
        let (a0, a1) = (hint.t, hint.t + SAMPLE_MS);
        let overlap: i64 = spans
            .iter()
            .map(|(t0, t1)| (a1.min(*t1) - a0.max(*t0)).max(0))
            .sum();
        if overlap > 0 {
            *totals.entry(hint.name.as_str()).or_default() += overlap;
        }
    }
    let mut best: Option<(&str, i64)> = None;
    for (name, total) in totals {
        if total >= MIN_OVERLAP_MS && best.is_none_or(|(_, b)| total > b) {
            best = Some((name, total));
        }
    }
    best.map(|(name, _)| name.to_string())
}

/// `suggestions` with the name the meeting app showed for `speaker_id` first,
/// if it showed one. Reading hints never fails a suggestion list.
pub fn with_app_name(
    store: &MeetingsStore,
    audio_dir: &Path,
    speaker_id: SpeakerId,
    mut suggestions: Vec<NameSuggestion>,
) -> Result<Vec<NameSuggestion>> {
    let Some(speaker) = store.get_speaker(speaker_id)? else {
        return Ok(suggestions);
    };
    let hints = read_hints(audio_dir, speaker.meeting_id);
    if hints.is_empty() {
        return Ok(suggestions);
    }
    // The other side's speakers: the mic is the user.
    let spans: Vec<(i64, i64)> = store
        .segments(speaker.meeting_id)?
        .iter()
        .filter(|s| s.speaker_id == Some(speaker_id) && s.track == Track::System)
        .map(|s| (s.t0_ms, s.t1_ms))
        .collect();
    let Some(name) = best_name(&hints, &spans) else {
        return Ok(suggestions);
    };
    let known = suggestions
        .iter()
        .position(|s| s.display_name.trim().eq_ignore_ascii_case(name.trim()));
    let first = match known {
        Some(i) => suggestions.remove(i),
        None => NameSuggestion {
            display_name: name,
            email: None,
            person_id: None,
            source: SuggestionSource::MeetingApp,
            similarity: None,
        },
    };
    suggestions.insert(0, first);
    Ok(suggestions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(t: i64, name: &str) -> Hint {
        Hint {
            t,
            name: name.into(),
        }
    }

    #[test]
    fn the_name_showing_longest_wins() {
        let hints: Vec<Hint> = (0..10)
            .map(|s| hint(s * 1_000, "Ann"))
            .chain((10..13).map(|s| hint(s * 1_000, "Ben")))
            .collect();
        assert_eq!(best_name(&hints, &[(0, 20_000)]).as_deref(), Some("Ann"));
        assert_eq!(
            best_name(&hints, &[(10_000, 13_000)]).as_deref(),
            Some("Ben")
        );
        // Several spans add up.
        assert_eq!(
            best_name(&hints, &[(0, 1_000), (10_000, 13_000)]).as_deref(),
            Some("Ben")
        );
    }

    #[test]
    fn too_little_overlap_or_none_says_nothing() {
        let hints = vec![hint(0, "Ann"), hint(1_000, "Ann")];
        assert_eq!(best_name(&hints, &[(0, 1_999)]), None);
        assert_eq!(best_name(&hints, &[(0, 2_000)]).as_deref(), Some("Ann"));
        assert_eq!(best_name(&hints, &[(50_000, 60_000)]), None);
        assert_eq!(best_name(&hints, &[]), None);
        assert_eq!(best_name(&[], &[(0, 10_000)]), None);
    }

    #[test]
    fn a_tie_is_stable() {
        let hints = vec![
            hint(0, "Ben"),
            hint(1_000, "Ben"),
            hint(2_000, "Ann"),
            hint(3_000, "Ann"),
        ];
        assert_eq!(best_name(&hints, &[(0, 4_000)]).as_deref(), Some("Ann"));
    }

    #[test]
    fn a_torn_line_is_skipped_and_no_file_is_no_hints() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_hints(dir.path(), 1).is_empty());
        std::fs::write(
            dir.path().join(audio_store::speakers_log_name(1)),
            "{\"t\":0,\"name\":\"Ann\"}\nnot json\n{\"t\":1000,\"name\":\"Be",
        )
        .unwrap();
        assert_eq!(read_hints(dir.path(), 1), vec![hint(0, "Ann")]);
    }

    fn suggestion(name: &str) -> NameSuggestion {
        NameSuggestion {
            display_name: name.into(),
            email: None,
            person_id: Some(7),
            source: SuggestionSource::KnownPerson,
            similarity: None,
        }
    }

    #[test]
    fn absent_names_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open_in_memory().unwrap();
        let listed = vec![suggestion("Ann")];
        let out = with_app_name(&store, dir.path(), 999, listed.clone()).unwrap();
        assert_eq!(out, listed);
    }
}
