//! Merging the two tracks into one timeline, with the D9 echo safety net.
//!
//! Without headphones the remote speakers leak into the mic. Voice
//! Processing I/O removes most of it; what survives shows up as a mic
//! segment that overlaps a system segment in time and says (nearly) the same
//! words. Such a mic copy is dropped, so the speech is attributed to the
//! remote speaker rather than the user.

use serde::{Deserialize, Serialize};

use crate::meetings::types::{Track, Word};

/// One transcribed VAD segment, in meeting time.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RawSegment {
    pub track: Track,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub text: String,
    pub words: Vec<Word>,
}

impl RawSegment {
    pub fn overlaps(&self, t0_ms: i64, t1_ms: i64) -> bool {
        self.t0_ms < t1_ms && t0_ms < self.t1_ms
    }
}

/// Lowercased alphanumeric tokens (apostrophes kept inside words).
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|t| t.trim_matches('\'').to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// How much of `mic` is also said in `system`, in [0, 1]: the larger of
/// (a) the share of the mic's tokens found in the system tokens (each
///     system token used once; fuzzy, so "colour"/"color" still count), and
/// (b) the normalized Levenshtein similarity of the token strings.
pub fn echo_similarity(mic: &str, system: &str) -> f32 {
    let mic = tokens(mic);
    let sys = tokens(system);
    if mic.is_empty() || sys.is_empty() {
        return 0.0;
    }
    let mut used = vec![false; sys.len()];
    let mut found = 0usize;
    for m in &mic {
        let hit = sys.iter().enumerate().position(|(i, s)| {
            !used[i] && (s == m || (m.len() > 3 && strsim::normalized_levenshtein(s, m) >= 0.75))
        });
        if let Some(i) = hit {
            used[i] = true;
            found += 1;
        }
    }
    let containment = found as f32 / mic.len() as f32;
    let leven = strsim::normalized_levenshtein(&mic.join(" "), &sys.join(" ")) as f32;
    containment.max(leven)
}

#[derive(Debug, Clone, Copy)]
pub struct EchoParams {
    /// Mic and system speech this close (ms) count as overlapping.
    pub slack_ms: i64,
    /// Similarity at or above which the mic segment is an echo.
    pub threshold: f32,
}

impl Default for EchoParams {
    fn default() -> Self {
        Self {
            slack_ms: 750,
            threshold: 0.6,
        }
    }
}

/// The system-track text heard around `[t0, t1]`: the words inside the
/// window when word timestamps exist, else the overlapping segments' text.
fn system_text_near(system: &[RawSegment], t0: i64, t1: i64) -> String {
    let mut out: Vec<&str> = Vec::new();
    for s in system.iter().filter(|s| s.overlaps(t0, t1)) {
        if s.words.is_empty() {
            out.push(&s.text);
        } else {
            out.extend(
                s.words
                    .iter()
                    .filter(|w| w.t0_ms < t1 && t0 < w.t1_ms)
                    .map(|w| w.text.as_str()),
            );
        }
    }
    out.join(" ")
}

/// For each mic segment, whether it is an echo of the system track.
pub fn echo_flags(mic: &[RawSegment], system: &[RawSegment], p: &EchoParams) -> Vec<bool> {
    mic.iter()
        .map(|m| {
            let (t0, t1) = (m.t0_ms - p.slack_ms, m.t1_ms + p.slack_ms);
            if !system.iter().any(|s| s.overlaps(t0, t1)) {
                return false;
            }
            let near = system_text_near(system, t0, t1);
            echo_similarity(&m.text, &near) >= p.threshold
        })
        .collect()
}

/// A segment on the merged timeline, with the local speaker index it belongs
/// to (`None` = unattributed system speech).
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    pub segment: RawSegment,
    pub speaker: Option<usize>,
}

/// Merge mic and system segments into one timeline sorted by start time.
/// Mic segments go to `user_speaker`, minus the ones flagged as echoes;
/// system segment `i` goes to `system_speakers[i]`. Empty-text segments are
/// dropped.
pub fn merge_timeline(
    mic: &[RawSegment],
    system: &[RawSegment],
    system_speakers: &[Option<usize>],
    user_speaker: usize,
    echo: &EchoParams,
) -> Vec<TimelineEntry> {
    let echoes = echo_flags(mic, system, echo);
    let mut out: Vec<TimelineEntry> = mic
        .iter()
        .zip(&echoes)
        .filter(|(_, &is_echo)| !is_echo)
        .map(|(s, _)| TimelineEntry {
            segment: s.clone(),
            speaker: Some(user_speaker),
        })
        .chain(system.iter().enumerate().map(|(i, s)| TimelineEntry {
            segment: s.clone(),
            speaker: system_speakers.get(i).copied().flatten(),
        }))
        .filter(|e| !e.segment.text.trim().is_empty())
        .collect();
    out.sort_by_key(|e| {
        (
            e.segment.t0_ms,
            e.segment.track == Track::Mic,
            e.segment.t1_ms,
        )
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(track: Track, t0: i64, t1: i64, text: &str) -> RawSegment {
        RawSegment {
            track,
            t0_ms: t0,
            t1_ms: t1,
            text: text.into(),
            words: Vec::new(),
        }
    }

    fn worded(track: Track, t0: i64, words: &[&str]) -> RawSegment {
        let words: Vec<Word> = words
            .iter()
            .enumerate()
            .map(|(i, w)| Word {
                t0_ms: t0 + i as i64 * 400,
                t1_ms: t0 + i as i64 * 400 + 350,
                text: w.to_string(),
            })
            .collect();
        RawSegment {
            track,
            t0_ms: t0,
            t1_ms: words.last().map_or(t0, |w| w.t1_ms),
            text: words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            words,
        }
    }

    #[test]
    fn tokenizes_and_scores_similarity() {
        assert_eq!(
            tokens("Hello, world! It's ME."),
            vec!["hello", "world", "it's", "me"]
        );
        assert!(echo_similarity("let's ship it on Friday", "Let's ship it on Friday.") > 0.99);
        // A partial, slightly misrecognized echo still scores high.
        assert!(
            echo_similarity(
                "we should ship on friday",
                "I think we should ship it on Friday"
            ) >= 0.6
        );
        assert!(echo_similarity("I disagree completely", "let's ship it on Friday") < 0.3);
        assert_eq!(echo_similarity("", "anything"), 0.0);
    }

    #[test]
    fn drops_the_mic_copy_of_overlapping_matching_speech() {
        let system = vec![seg(
            Track::System,
            1000,
            4000,
            "Let's ship the release on Friday.",
        )];
        let mic = vec![
            // Echo: same words, same time.
            seg(Track::Mic, 1200, 4100, "let's ship the release on friday"),
            // The user's own reply right after.
            seg(Track::Mic, 4500, 6000, "Sounds good to me."),
        ];
        let tl = merge_timeline(&mic, &system, &[Some(0)], 9, &EchoParams::default());
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0].segment.track, Track::System);
        assert_eq!(tl[0].speaker, Some(0));
        assert_eq!(tl[1].segment.text, "Sounds good to me.");
        assert_eq!(tl[1].speaker, Some(9));
    }

    #[test]
    fn keeps_overlapping_speech_that_differs() {
        // Crosstalk: both talk at once, different words. Both kept.
        let system = vec![seg(
            Track::System,
            0,
            3000,
            "the numbers look great this quarter",
        )];
        let mic = vec![seg(Track::Mic, 500, 2500, "sorry can you repeat that")];
        assert_eq!(
            echo_flags(&mic, &system, &EchoParams::default()),
            vec![false]
        );
        // Same words but far apart in time: not an echo (the user repeating).
        let mic = vec![seg(
            Track::Mic,
            10_000,
            13_000,
            "the numbers look great this quarter",
        )];
        assert_eq!(
            echo_flags(&mic, &system, &EchoParams::default()),
            vec![false]
        );
    }

    #[test]
    fn uses_only_the_overlapping_system_words() {
        // A long system segment: only its words near the mic segment count.
        let system = vec![worded(
            Track::System,
            0,
            &[
                "alpha", "beta", "gamma", "delta", "ship", "it", "now", "epsilon", "zeta",
            ],
        )];
        let mic_echo = vec![worded(Track::Mic, 1600, &["ship", "it", "now"])];
        assert_eq!(
            echo_flags(
                &mic_echo,
                &system,
                &EchoParams {
                    slack_ms: 0,
                    threshold: 0.6
                }
            ),
            vec![true]
        );
        // Words said by the user that appear elsewhere in the system segment,
        // but not at this time, are kept.
        let mic_far = vec![worded(Track::Mic, 0, &["epsilon", "zeta", "eta"])];
        assert_eq!(
            echo_flags(
                &mic_far,
                &system,
                &EchoParams {
                    slack_ms: 0,
                    threshold: 0.6
                }
            ),
            vec![false]
        );
    }

    #[test]
    fn timeline_is_sorted_and_skips_empty_text() {
        let system = vec![
            seg(Track::System, 5000, 6000, "second"),
            seg(Track::System, 7000, 7500, "  "),
        ];
        let mic = vec![
            seg(Track::Mic, 1000, 2000, "first"),
            seg(Track::Mic, 8000, 9000, "third"),
        ];
        let tl = merge_timeline(&mic, &system, &[None, Some(1)], 0, &EchoParams::default());
        let texts: Vec<&str> = tl.iter().map(|e| e.segment.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
        assert_eq!(tl[1].speaker, None);
    }
}
