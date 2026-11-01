//! Merging the two tracks into one timeline, with the D9 echo safety net.
//!
//! Without headphones the remote speakers leak into the mic. Echo
//! cancellation (`meetings::native::aec`) removes most of it; what survives shows up as a mic
//! segment that overlaps a system segment in time and says the same words.
//! That mic copy is dropped, so the speech is attributed to the remote speaker
//! rather than the user. One mic segment can hold both bleed and the user's
//! own speech, so the decision is made word by word (see [`echo_cuts`]).

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

/// The same token, allowing for a misrecognition ("colour"/"color").
fn same_token(mic: &str, system: &str) -> bool {
    mic == system || (mic.len() > 3 && strsim::normalized_levenshtein(system, mic) >= 0.75)
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
        let hit = sys
            .iter()
            .enumerate()
            .position(|(i, s)| !used[i] && same_token(m, s));
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
    /// Similarity at or above which a live mic line is hidden as an echo
    /// (see [`echo_similarity`]). The stored transcript goes by [`echo_cuts`].
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

/// Runs of matching mic words shorter than this are never called echoes: a
/// short reply ("yes", "right") too easily matches words said nearby.
pub const ECHO_MIN_TOKENS: usize = 3;

/// Ideographs and kana: each character counts as a word.
fn written_by_character(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF // Kana
        | 0x3400..=0x9FFF // CJK ideographs
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF66..=0xFF9D // Halfwidth kana
        | 0x20000..=0x3FFFF // Supplementary CJK ideographs
    )
}

/// How many words `text` holds, for [`ECHO_MIN_TOKENS`]: what spaces separate,
/// and joining punctuation outside numbers ("10:30" is one), not [`tokens`],
/// which can cut one word of some scripts into several; with
/// each ideograph or kana counted as one and Thai at about four characters a
/// word, as those scripts aren't written with spaces between words.
fn word_count(text: &str) -> usize {
    let thai = |c: char| matches!(c as u32, 0x0E00..=0x0E7F);
    let joiner = |c: char| matches!(c, ',' | '-' | ';' | ':' | '/' | '–' | '—');
    text.split_whitespace()
        .flat_map(|w| {
            let number = w.chars().any(|c| c.is_ascii_digit());
            w.split(move |c: char| !number && joiner(c))
        })
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| {
            let chars = w
                .chars()
                .filter(|c| written_by_character(*c) && c.is_alphanumeric())
                .count();
            let thai_chars = w.chars().filter(|c| thai(*c)).count();
            (chars + thai_chars / 4).max(1)
        })
        .sum()
}

/// [`tokens`], with each ideograph or kana on its own: engines cut those
/// scripts into words differently on each track.
fn atoms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokens(text) {
        let mut run = String::new();
        for c in token.chars() {
            if written_by_character(c) {
                if !run.is_empty() {
                    out.push(std::mem::take(&mut run));
                }
                out.push(c.to_string());
            } else {
                run.push(c);
            }
        }
        if !run.is_empty() {
            out.push(run);
        }
    }
    out
}

/// One [`atoms`] entry and when it was said.
struct Timed {
    t0_ms: i64,
    t1_ms: i64,
    atom: String,
}

fn timed_atoms(text: &str, t0_ms: i64, t1_ms: i64) -> impl Iterator<Item = Timed> {
    atoms(text)
        .into_iter()
        .map(move |atom| Timed { t0_ms, t1_ms, atom })
}

/// What the system track said in `[t0, t1]`, in order: its words there, or
/// the whole text of a segment without word times.
fn heard_near(system: &[RawSegment], t0: i64, t1: i64) -> Vec<Timed> {
    let mut near: Vec<&RawSegment> = system
        .iter()
        .filter(|s| s.t0_ms <= t1 && t0 <= s.t1_ms)
        .collect();
    near.sort_by_key(|s| s.t0_ms);
    let mut out = Vec::new();
    for s in near {
        if s.words.is_empty() {
            out.extend(timed_atoms(&s.text, s.t0_ms, s.t1_ms));
        } else {
            for w in s.words.iter().filter(|w| w.t0_ms <= t1 && t0 <= w.t1_ms) {
                out.extend(timed_atoms(&w.text, w.t0_ms, w.t1_ms));
            }
        }
    }
    out
}

/// Per entry of `mic`, whether it is in the longest in-order match with
/// `heard`, each heard entry used once and only within `slack_ms` of the mic
/// one: an echo repeats what was said, in the same order, at the same time.
fn matched_in_order(mic: &[Timed], heard: &[Timed], slack_ms: i64) -> Vec<bool> {
    let fits = |i: usize, j: usize| {
        let (m, h) = (&mic[i], &heard[j]);
        m.t0_ms - slack_ms <= h.t1_ms
            && h.t0_ms <= m.t1_ms + slack_ms
            && same_token(&m.atom, &h.atom)
    };
    let (rows, cols) = (mic.len(), heard.len());
    let at = |i: usize, j: usize| i * (cols + 1) + j;
    let mut best = vec![0u32; (rows + 1) * (cols + 1)];
    for i in (0..rows).rev() {
        for j in (0..cols).rev() {
            let skip = best[at(i + 1, j)].max(best[at(i, j + 1)]);
            best[at(i, j)] = if fits(i, j) {
                skip.max(best[at(i + 1, j + 1)] + 1)
            } else {
                skip
            };
        }
    }
    let mut out = vec![false; rows];
    let (mut i, mut j) = (0, 0);
    while i < rows && j < cols {
        if fits(i, j) && best[at(i, j)] == best[at(i + 1, j + 1)] + 1 {
            out[i] = true;
            i += 1;
            j += 1;
        } else if best[at(i + 1, j)] == best[at(i, j)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Whether the segment's words can decide for its text: in time order, with
/// real times, and saying all of it. A word list that leaves text out could
/// not rebuild what's kept.
fn has_word_times(m: &RawSegment) -> bool {
    let spelled = |atoms: Vec<String>| atoms.concat();
    !m.words.is_empty()
        && m.words.windows(2).all(|w| w[0].t0_ms <= w[1].t0_ms)
        && m.words.iter().any(|w| w.t1_ms > w.t0_ms)
        && m.words
            .iter()
            .map(|w| spelled(atoms(&w.text)))
            .collect::<String>()
            == spelled(atoms(&m.text))
}

/// Per word of `m`, whether it goes: it is in a contiguous run of matching
/// words holding at least [`ECHO_MIN_TOKENS`].
fn echo_words(m: &RawSegment, heard: &[Timed], slack_ms: i64) -> Vec<bool> {
    let mut said = Vec::new();
    let mut owner = Vec::new();
    for (k, w) in m.words.iter().enumerate() {
        for t in timed_atoms(&w.text, w.t0_ms, w.t1_ms) {
            said.push(t);
            owner.push(k);
        }
    }
    let hits = matched_in_order(&said, heard, slack_ms);
    // `None`: nothing to match (punctuation), so it follows the word before.
    let mut matched: Vec<Option<bool>> = vec![None; m.words.len()];
    for (&k, hit) in owner.iter().zip(hits) {
        matched[k] = Some(matched[k].unwrap_or(true) && hit);
    }
    let first = matched.iter().flatten().next().copied().unwrap_or(false);
    let mut last = first;
    let matched: Vec<bool> = matched
        .into_iter()
        .map(|state| {
            last = state.unwrap_or(last);
            last
        })
        .collect();
    let mut out = vec![false; m.words.len()];
    let mut k = 0;
    while k < matched.len() {
        let end = (k..matched.len())
            .find(|&e| matched[e] != matched[k])
            .unwrap_or(matched.len());
        let count: usize = m.words[k..end].iter().map(|w| word_count(&w.text)).sum();
        if matched[k] && count >= ECHO_MIN_TOKENS {
            out[k..end].fill(true);
        }
        k = end;
    }
    out
}

/// The words `m.words[range]` as a segment of their own, inside `m`.
fn piece_of(m: &RawSegment, range: std::ops::Range<usize>) -> RawSegment {
    // The text's own tokens keep their punctuation, when they line up.
    let spoken: Vec<&str> = m.text.split_whitespace().collect();
    let aligned = spoken.len() == m.words.len();
    let separator = if spoken.len() > 1 { " " } else { "" };
    let text: Vec<&str> = range
        .clone()
        .map(|k| {
            if aligned {
                spoken[k]
            } else {
                m.words[k].text.as_str()
            }
        })
        .collect();
    let words: Vec<Word> = m.words[range]
        .iter()
        .map(|w| {
            let t0_ms = w.t0_ms.clamp(m.t0_ms, m.t1_ms);
            Word {
                t0_ms,
                t1_ms: w.t1_ms.clamp(t0_ms, m.t1_ms),
                text: w.text.clone(),
            }
        })
        .collect();
    RawSegment {
        track: m.track,
        t0_ms: words.first().map_or(m.t0_ms, |w| w.t0_ms),
        t1_ms: words.iter().map(|w| w.t1_ms).max().unwrap_or(m.t1_ms),
        text: text.join(separator),
        words,
    }
}

/// What echo removal leaves of one mic segment.
#[derive(Debug, Clone, PartialEq)]
pub enum EchoCut {
    /// No echo: the segment stands as it is.
    Whole,
    /// Echo removed: what remains, in order. Empty when all of it was echo.
    Trimmed(Vec<RawSegment>),
}

/// For each mic segment, what's left of it without the system track's echo.
/// With word times, only contiguous runs of words matching nearby system words
/// go, and only runs of at least [`ECHO_MIN_TOKENS`]. Without them, the segment
/// goes only if it has that many tokens and every one matches. Anything unsure
/// stays. Both clustering and the merge decide with this.
pub fn echo_cuts(mic: &[RawSegment], system: &[RawSegment], p: &EchoParams) -> Vec<EchoCut> {
    mic.iter().map(|m| echo_cut(m, system, p)).collect()
}

fn echo_cut(m: &RawSegment, system: &[RawSegment], p: &EchoParams) -> EchoCut {
    if word_count(&m.text) < ECHO_MIN_TOKENS {
        return EchoCut::Whole;
    }
    let heard = heard_near(system, m.t0_ms - p.slack_ms, m.t1_ms + p.slack_ms);
    if heard.is_empty() {
        return EchoCut::Whole;
    }
    if !has_word_times(m) {
        let said: Vec<Timed> = timed_atoms(&m.text, m.t0_ms, m.t1_ms).collect();
        let all = !said.is_empty()
            && matched_in_order(&said, &heard, p.slack_ms)
                .into_iter()
                .all(|hit| hit);
        return if all {
            EchoCut::Trimmed(Vec::new())
        } else {
            EchoCut::Whole
        };
    }
    let gone = echo_words(m, &heard, p.slack_ms);
    if !gone.contains(&true) {
        return EchoCut::Whole;
    }
    let mut pieces = Vec::new();
    let mut k = 0;
    while k < gone.len() {
        let end = (k..gone.len())
            .find(|&e| gone[e] != gone[k])
            .unwrap_or(gone.len());
        if !gone[k] {
            pieces.push(piece_of(m, k..end));
        }
        k = end;
    }
    EchoCut::Trimmed(pieces)
}

/// For each mic segment, whether all of it is echo.
#[cfg(test)]
fn echo_flags(mic: &[RawSegment], system: &[RawSegment], p: &EchoParams) -> Vec<bool> {
    echo_cuts(mic, system, p)
        .iter()
        .map(|cut| matches!(cut, EchoCut::Trimmed(left) if left.is_empty()))
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
/// Mic segment `i` goes to `mic_speakers[i]` (someone else in the room, when
/// the mic track was diarized), else to `user_speaker`, less its echo (see
/// [`echo_cuts`]); system segment `i` goes to `system_speakers[i]`.
/// Empty-text segments are dropped.
pub fn merge_timeline(
    mic: &[RawSegment],
    system: &[RawSegment],
    mic_speakers: &[Option<usize>],
    system_speakers: &[Option<usize>],
    user_speaker: usize,
    echo: &EchoParams,
) -> Vec<TimelineEntry> {
    let cuts = echo_cuts(mic, system, echo);
    let mut out: Vec<TimelineEntry> = mic
        .iter()
        .zip(cuts)
        .enumerate()
        .flat_map(|(i, (s, cut))| {
            let speaker = Some(
                mic_speakers
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or(user_speaker),
            );
            let kept = match cut {
                EchoCut::Whole => vec![s.clone()],
                EchoCut::Trimmed(pieces) => pieces,
            };
            kept.into_iter()
                .map(move |segment| TimelineEntry { segment, speaker })
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
        let tl = merge_timeline(&mic, &system, &[], &[Some(0)], 9, &EchoParams::default());
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0].segment.track, Track::System);
        assert_eq!(tl[0].speaker, Some(0));
        assert_eq!(tl[1].segment.text, "Sounds good to me.");
        assert_eq!(tl[1].speaker, Some(9));
    }

    #[test]
    fn a_short_reply_is_kept_and_a_three_word_echo_dropped() {
        let system = vec![seg(Track::System, 1000, 4000, "yes ship it now")];
        let p = EchoParams::default();
        for reply in ["Yes.", "yes ship"] {
            let mic = vec![seg(Track::Mic, 1200, 2000, reply)];
            assert_eq!(echo_flags(&mic, &system, &p), vec![false], "{reply}");
        }
        let mic = vec![seg(Track::Mic, 1200, 3000, "ship it now")];
        assert_eq!(echo_flags(&mic, &system, &p), vec![true]);
    }

    #[test]
    fn a_script_without_spaces_counts_each_character() {
        assert_eq!(word_count("ship it"), 2);
        assert_eq!(word_count("明日リリース"), 6);
        // Five kana and a voicing mark.
        assert_eq!(word_count("ｱﾘｶﾞﾄｳ"), 5);
        assert_eq!(word_count("𠀀𠀁𠀂"), 3);
        // One word, however a tokenizer would cut it.
        assert_eq!(word_count("स्वास्थ्य"), 1);
        assert_eq!(word_count("1.5"), 1);
        assert_eq!(word_count("1,000,000"), 1);
        assert_eq!(word_count("10:30 AM"), 2);
        // Marks in a kana block aren't words; words joined by punctuation are.
        assert_eq!(word_count("はい・・・"), 2);
        assert_eq!(word_count("ship,it,now"), 3);
        assert_eq!(word_count("let's-ship it"), 3);
        assert_eq!(word_count("ครับ"), 1);
        assert_eq!(word_count("... -- !"), 0);
        let system = vec![seg(Track::System, 1000, 4000, "明日リリースします")];
        let mic = vec![seg(Track::Mic, 1200, 3000, "明日リリースします")];
        assert_eq!(
            echo_flags(&mic, &system, &EchoParams::default()),
            vec![true]
        );
        let mic = vec![seg(Track::Mic, 1200, 1500, "明日")];
        assert_eq!(
            echo_flags(&mic, &system, &EchoParams::default()),
            vec![false]
        );
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

    /// `words` spread evenly over `[t0, t1]`.
    fn over(track: Track, t0: i64, t1: i64, words: &[&str]) -> RawSegment {
        let step = (t1 - t0) / words.len() as i64;
        let words: Vec<Word> = words
            .iter()
            .enumerate()
            .map(|(i, w)| Word {
                t0_ms: t0 + i as i64 * step,
                t1_ms: if i + 1 == words.len() {
                    t1
                } else {
                    t0 + (i as i64 + 1) * step
                },
                text: w.to_string(),
            })
            .collect();
        RawSegment {
            track,
            t0_ms: t0,
            t1_ms: t1,
            text: words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            words,
        }
    }

    /// The segment made of the words of `parts`, one after the other.
    fn joined(parts: &[RawSegment]) -> RawSegment {
        let first = parts.first().unwrap();
        RawSegment {
            track: first.track,
            t0_ms: first.t0_ms,
            t1_ms: parts.last().unwrap().t1_ms,
            text: parts
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            words: parts.iter().flat_map(|p| p.words.clone()).collect(),
        }
    }

    fn left_of(mic: RawSegment, system: &[RawSegment]) -> Vec<(i64, i64, String)> {
        match echo_cuts(&[mic], system, &EchoParams::default()).remove(0) {
            EchoCut::Whole => panic!("nothing was cut"),
            EchoCut::Trimmed(pieces) => pieces
                .into_iter()
                .map(|s| (s.t0_ms, s.t1_ms, s.text))
                .collect(),
        }
    }

    const ONE: [&str; 5] = ["the", "numbers", "look", "great", "today"];
    const TWO: [&str; 5] = ["we", "shipped", "the", "billing", "fix"];
    const THREE: [&str; 5] = ["please", "review", "the", "launch", "plan"];
    const OWN: [&str; 4] = ["sorry", "my", "connection", "dropped"];

    #[test]
    fn a_unit_of_bleed_and_local_speech_keeps_the_local_turn() {
        // As measured: one mic unit [864, 28896] holding three remote turns
        // and the user's turn [14881, 21383]. The whole unit used to go.
        let (a, b, c) = (
            over(Track::System, 864, 7_000, &ONE),
            over(Track::System, 7_500, 14_500, &TWO),
            over(Track::System, 21_800, 28_896, &THREE),
        );
        let mine = over(Track::Mic, 14_881, 21_383, &OWN);
        let copy = |s: &RawSegment| RawSegment {
            track: Track::Mic,
            ..s.clone()
        };
        let mic = joined(&[copy(&a), copy(&b), mine, copy(&c)]);
        assert_eq!((mic.t0_ms, mic.t1_ms), (864, 28_896));
        let system = [a, b, c];
        assert_eq!(
            left_of(mic.clone(), &system),
            vec![(14_881, 21_383, "sorry my connection dropped".to_string())]
        );
        // And the merged transcript has it, as the user's.
        let tl = merge_timeline(
            &[mic],
            &system,
            &[],
            &[Some(0), Some(1), Some(0)],
            9,
            &EchoParams::default(),
        );
        let theirs: Vec<&TimelineEntry> = tl.iter().filter(|e| e.speaker == Some(9)).collect();
        assert_eq!(theirs.len(), 1);
        assert_eq!(theirs[0].segment.track, Track::Mic);
        assert_eq!(
            (theirs[0].segment.t0_ms, theirs[0].segment.t1_ms),
            (14_881, 21_383)
        );
        assert_eq!(theirs[0].segment.words.len(), 4);
        assert_eq!(tl.len(), 4);
    }

    #[test]
    fn leading_and_trailing_echo_go() {
        let echo = over(Track::System, 0, 3_000, &ONE);
        let copy = RawSegment {
            track: Track::Mic,
            ..echo.clone()
        };
        // Past the slack of the echo, so it can't match by time.
        let mine = over(Track::Mic, 5_000, 8_000, &OWN);
        assert_eq!(
            left_of(joined(&[copy, mine.clone()]), &[echo]),
            vec![(5_000, 8_000, "sorry my connection dropped".to_string())]
        );
        let echo = over(Track::System, 10_000, 13_000, &ONE);
        let copy = RawSegment {
            track: Track::Mic,
            ..echo.clone()
        };
        assert_eq!(
            left_of(joined(&[mine, copy]), &[echo]),
            vec![(5_000, 8_000, "sorry my connection dropped".to_string())]
        );
    }

    #[test]
    fn echo_around_local_speech_and_local_speech_around_echo() {
        let (a, b) = (
            over(Track::System, 0, 3_000, &ONE),
            over(Track::System, 8_000, 11_000, &TWO),
        );
        let mic_of = |s: &RawSegment| RawSegment {
            track: Track::Mic,
            ..s.clone()
        };
        let mine = over(Track::Mic, 4_000, 7_000, &OWN);
        assert_eq!(
            left_of(
                joined(&[mic_of(&a), mine.clone(), mic_of(&b)]),
                &[a.clone(), b.clone()]
            ),
            vec![(4_000, 7_000, "sorry my connection dropped".to_string())]
        );
        // The other way round leaves two segments, each with its own times.
        let echo = over(Track::System, 4_000, 7_000, &ONE);
        let before = over(Track::Mic, 0, 2_000, &OWN);
        let after = over(Track::Mic, 9_000, 11_000, &["back", "again", "now"]);
        assert_eq!(
            left_of(joined(&[before, mic_of(&echo), after]), &[echo]),
            vec![
                (0, 2_000, "sorry my connection dropped".to_string()),
                (9_000, 11_000, "back again now".to_string()),
            ]
        );
    }

    #[test]
    fn without_word_times_every_token_must_match() {
        let system = vec![seg(Track::System, 0, 4_000, "ship the release on friday")];
        let p = EchoParams::default();
        let all = seg(Track::Mic, 0, 4_000, "ship the release");
        assert_eq!(
            echo_cuts(&[all], &system, &p),
            vec![EchoCut::Trimmed(vec![])]
        );
        // Most of it matches, which used to be enough; one word is the user's.
        let most = seg(Track::Mic, 0, 4_000, "ship the release tomorrow");
        assert_eq!(echo_cuts(&[most], &system, &p), vec![EchoCut::Whole]);
        // Word times on the mic only count if they can stand for the text.
        let mut partial = over(Track::Mic, 0, 4_000, &["ship", "the", "release"]);
        partial.text = "ship the release and my own words".into();
        assert_eq!(echo_cuts(&[partial], &system, &p), vec![EchoCut::Whole]);
        // A system segment without word times still counts as heard.
        let timed = over(Track::Mic, 0, 4_000, &["ship", "the", "release"]);
        assert_eq!(
            echo_cuts(&[timed], &system, &p),
            vec![EchoCut::Trimmed(vec![])]
        );
    }

    #[test]
    fn a_one_word_reply_stays_with_or_without_word_times() {
        let system = [over(Track::System, 0, 3_000, &["yes", "ship", "it", "now"])];
        let p = EchoParams::default();
        let reply = over(Track::Mic, 100, 500, &["yes"]);
        assert_eq!(echo_cuts(&[reply], &system, &p), vec![EchoCut::Whole]);
        // A two-word match inside the user's sentence isn't enough either.
        let mine = over(Track::Mic, 0, 3_000, &["yes", "ship", "wait", "stop"]);
        assert_eq!(echo_cuts(&[mine], &system, &p), vec![EchoCut::Whole]);
    }

    #[test]
    fn matching_words_said_out_of_order_or_twice_stay() {
        let system = [over(Track::System, 0, 3_000, &["ship", "it", "now"])];
        let p = EchoParams::default();
        let backwards = over(Track::Mic, 0, 3_000, &["now", "it", "ship"]);
        assert_eq!(echo_cuts(&[backwards], &system, &p), vec![EchoCut::Whole]);
        let repeated = over(Track::Mic, 0, 3_000, &["ship", "ship", "ship"]);
        assert_eq!(echo_cuts(&[repeated], &system, &p), vec![EchoCut::Whole]);
    }

    #[test]
    fn cjk_counts_by_character_however_the_words_are_cut() {
        let system = [over(
            Track::System,
            0,
            2_000,
            &["明日", "リリース", "します"],
        )];
        let p = EchoParams::default();
        // Cut differently on the mic, and written without spaces.
        let mut echo = over(Track::Mic, 0, 2_000, &["明", "日リリー", "スします"]);
        echo.text = "明日リリースします".into();
        assert_eq!(
            echo_cuts(&[echo], &system, &p),
            vec![EchoCut::Trimmed(vec![])]
        );
        // Two characters are under the minimum.
        let reply = over(Track::Mic, 0, 500, &["明日"]);
        assert_eq!(echo_cuts(&[reply], &system, &p), vec![EchoCut::Whole]);
        // A long enough run goes; what the user said after it stays, unspaced.
        let mut mixed = over(Track::Mic, 0, 6_000, &["明日", "リリース", "はい", "了解"]);
        mixed.words[2].t0_ms = 3_500;
        mixed.words[2].t1_ms = 4_500;
        mixed.words[3].t0_ms = 4_500;
        mixed.text = "明日リリースはい了解".into();
        assert_eq!(
            left_of(mixed, &system),
            vec![(3_500, 6_000, "はい了解".to_string())]
        );
    }

    #[test]
    fn punctuation_stays_on_what_is_kept() {
        let echo = over(Track::System, 0, 3_000, &["ship", "it", "now"]);
        let mut mic = joined(&[
            RawSegment {
                track: Track::Mic,
                ..echo.clone()
            },
            over(Track::Mic, 5_000, 7_000, &["sounds", "good"]),
        ]);
        mic.text = "Ship it, now. Sounds good!".into();
        assert_eq!(
            left_of(mic, &[echo]),
            vec![(5_000, 7_000, "Sounds good!".to_string())]
        );
    }

    #[test]
    fn a_kept_piece_finds_no_more_echo_when_processed_again() {
        let echo = over(Track::System, 0, 3_000, &ONE);
        let mic = joined(&[
            RawSegment {
                track: Track::Mic,
                ..echo.clone()
            },
            over(Track::Mic, 5_000, 8_000, &OWN),
        ]);
        let system = [echo];
        let p = EchoParams::default();
        let EchoCut::Trimmed(pieces) = echo_cuts(&[mic], &system, &p).remove(0) else {
            panic!("nothing was cut");
        };
        assert_eq!(echo_cuts(&pieces, &system, &p), vec![EchoCut::Whole]);
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
        let tl = merge_timeline(
            &mic,
            &system,
            &[],
            &[None, Some(1)],
            0,
            &EchoParams::default(),
        );
        let texts: Vec<&str> = tl.iter().map(|e| e.segment.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
        assert_eq!(tl[1].speaker, None);
    }
}
