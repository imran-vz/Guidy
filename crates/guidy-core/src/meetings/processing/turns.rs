//! Speaker turns inside one speech span: where one voice hands over to
//! another without a pause long enough for the VAD to split there.
//!
//! Two sources propose changes: a segmentation model (pyannote
//! segmentation-3.0 through sherpa-onnx, when installed), and a scan of short
//! sliding voiceprints. A proposal is kept only when the voiceprints on its
//! two sides clearly differ ([`confirm`]), so neither source alone splits a
//! line. Each kept cut moves to the quietest VAD frame nearby, so it falls
//! between words; the pieces are then transcribed and diarized as lines of
//! their own.

use anyhow::Result;

use super::cluster;
use super::pipeline::Embedder;
use super::vad::Span;
use super::SAMPLE_RATE;

/// One speaker's stretch, as a segmentation model reports it: ms from the
/// start of the audio it was given, with a speaker label local to that call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub speaker: i32,
}

/// Finds who speaks when inside a stretch of speech.
pub trait TurnSegmenter {
    /// The turns in 16 kHz mono `pcm`, in any order.
    fn turns(&mut self, pcm: &[f32]) -> Result<Vec<Turn>>;
}

#[derive(Debug, Clone, Copy)]
pub struct TurnParams {
    /// Spans shorter than this are never split.
    pub min_span_ms: i64,
    /// Model turns shorter than this propose no change (a "yeah" over
    /// someone else stays in their line).
    pub min_turn_ms: i64,
    /// Sliding voiceprint length and step for the scan.
    pub scan_window_ms: i64,
    pub scan_hop_ms: i64,
    /// The scan proposes a change where the voiceprints just before and
    /// just after are at most this similar.
    pub scan_threshold: f32,
    /// Audio each side of a proposed change that [`confirm`] compares...
    pub side_ms: i64,
    /// ...and the least it needs on each side (also the shortest piece).
    pub min_side_ms: i64,
    /// A model's change is kept when its sides are less similar than
    /// this...
    pub confirm_threshold: f32,
    /// ...and a change only the scan found, than this (short windows dip
    /// inside one voice too).
    pub scan_confirm_threshold: f32,
    /// Proposals closer than this are one change.
    pub merge_ms: i64,
    /// How far a cut may move to reach a quieter frame.
    pub snap_ms: i64,
}

impl Default for TurnParams {
    /// Calibrated on call audio with TitaNet voiceprints: across a model's
    /// change the 3 s sides score 0.04-0.33, inside one voice 0.5-0.85;
    /// 1.5 s windows inside one voice dip to ~0.15.
    fn default() -> Self {
        Self {
            min_span_ms: 3_000,
            min_turn_ms: 1_000,
            scan_window_ms: 1_500,
            scan_hop_ms: 500,
            scan_threshold: 0.1,
            side_ms: 3_000,
            min_side_ms: 1_500,
            confirm_threshold: 0.4,
            scan_confirm_threshold: 0.25,
            merge_ms: 1_000,
            snap_ms: 300,
        }
    }
}

/// A proposed change of voice, ms from the start of the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub t_ms: i64,
    /// Proposed by the segmentation model (else by the scan only).
    pub from_model: bool,
}

/// The changes a model's turns propose (ms): between consecutive turns of
/// different speakers, at the middle of the gap (or overlap) between them.
pub fn model_candidates(turns: &[Turn], p: &TurnParams) -> Vec<i64> {
    let mut long: Vec<Turn> = turns
        .iter()
        .copied()
        .filter(|t| t.t1_ms - t.t0_ms >= p.min_turn_ms)
        .collect();
    long.sort_by_key(|t| (t.t0_ms, t.t1_ms));
    // One speaker's consecutive turns are one turn.
    let mut merged: Vec<Turn> = Vec::new();
    for t in long {
        match merged.last_mut() {
            Some(last) if last.speaker == t.speaker => last.t1_ms = last.t1_ms.max(t.t1_ms),
            _ => merged.push(t),
        }
    }
    merged
        .windows(2)
        .map(|w| (w[0].t1_ms + w[1].t0_ms) / 2)
        .collect()
}

/// The changes a scan of sliding voiceprints proposes (ms) in `len_ms` of
/// audio: dips where the window before and the window after are at most
/// `scan_threshold` similar, the deepest first, `merge_ms` apart.
/// `embed(t0, t1)` is the voiceprint of `[t0, t1)`.
pub fn scan_candidates(
    len_ms: i64,
    embed: &mut dyn FnMut(i64, i64) -> Result<Option<Vec<f32>>>,
    p: &TurnParams,
) -> Result<Vec<i64>> {
    let (w, hop) = (p.scan_window_ms, p.scan_hop_ms.max(1));
    if len_ms < 2 * w {
        return Ok(Vec::new());
    }
    let mut windows: Vec<Option<Vec<f32>>> = Vec::new();
    let mut t = 0;
    while t + w <= len_ms {
        windows.push(embed(t, t + w)?);
        t += hop;
    }
    // The change at `k * hop` compares the window ending there with the one
    // starting there; that takes `w` to be a whole number of hops.
    let lag = (w / hop).max(1) as usize;
    let mut dips: Vec<(f32, i64)> = (lag..windows.len())
        .filter_map(|k| {
            let (a, b) = (windows[k - lag].as_ref()?, windows[k].as_ref()?);
            let sim = cluster::cosine(a, b);
            (sim <= p.scan_threshold).then_some((sim, k as i64 * hop))
        })
        .collect();
    dips.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<i64> = Vec::new();
    for (_, t) in dips {
        if out.iter().all(|&o| (o - t).abs() >= p.merge_ms) {
            out.push(t);
        }
    }
    out.sort_unstable();
    Ok(out)
}

/// The model's and the scan's proposals as one sorted list. A scan
/// proposal within `merge_ms` of a model one is the same change (the
/// model's time wins); scan proposals are already `merge_ms` apart.
pub fn merge_candidates(model: &[i64], scan: &[i64], merge_ms: i64) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = model
        .iter()
        .map(|&t_ms| Candidate {
            t_ms,
            from_model: true,
        })
        .chain(
            scan.iter()
                .filter(|&&t| model.iter().all(|&m| (m - t).abs() >= merge_ms))
                .map(|&t_ms| Candidate {
                    t_ms,
                    from_model: false,
                }),
        )
        .collect();
    out.sort_by_key(|c| c.t_ms);
    out
}

/// The proposed changes (sorted, in `len_ms` of audio) whose two sides
/// sound like different people, left to right: each side is up to
/// `side_ms`, bounded by the last kept change and the next proposal, and at
/// least `min_side_ms`.
pub fn confirm(
    len_ms: i64,
    candidates: &[Candidate],
    embed: &mut dyn FnMut(i64, i64) -> Result<Option<Vec<f32>>>,
    p: &TurnParams,
) -> Result<Vec<i64>> {
    let mut kept: Vec<i64> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        let t = c.t_ms;
        let left = (t - p.side_ms).max(kept.last().copied().unwrap_or(0));
        let mut right = (t + p.side_ms).min(len_ms);
        if let Some(next) = candidates.get(i + 1) {
            if next.t_ms - t >= p.min_side_ms {
                right = right.min(next.t_ms);
            }
        }
        if t - left < p.min_side_ms || right - t < p.min_side_ms {
            continue;
        }
        let (Some(a), Some(b)) = (embed(left, t)?, embed(t, right)?) else {
            continue;
        };
        let threshold = if c.from_model {
            p.confirm_threshold
        } else {
            p.scan_confirm_threshold
        };
        if cluster::cosine(&a, &b) < threshold {
            kept.push(t);
        }
    }
    Ok(kept)
}

/// Where the voice changes in `pcm` (ms from its start): the segmenter's
/// and the scan's proposals, confirmed by `embedder`'s voiceprints.
pub fn find_changes(
    pcm: &[f32],
    segmenter: Option<&mut (dyn TurnSegmenter + '_)>,
    embedder: &mut dyn Embedder,
    p: &TurnParams,
) -> Result<Vec<i64>> {
    let len_ms = (pcm.len() * 1000 / SAMPLE_RATE) as i64;
    if len_ms < p.min_span_ms {
        return Ok(Vec::new());
    }
    let at = |ms: i64| ((ms.max(0) as usize) * SAMPLE_RATE / 1000).min(pcm.len());
    let mut embed =
        |t0: i64, t1: i64| -> Result<Option<Vec<f32>>> { embedder.embed(&pcm[at(t0)..at(t1)]) };
    let from_model = match segmenter {
        Some(s) => model_candidates(&s.turns(pcm)?, p),
        None => Vec::new(),
    };
    let from_scan = scan_candidates(len_ms, &mut embed, p)?;
    let candidates = merge_candidates(&from_model, &from_scan, p.merge_ms);
    confirm(len_ms, &candidates, &mut embed, p)
}

/// `t_ms` moved to the quietest frame of `probs` (one per `frame_ms` from
/// 0 ms) within `radius_ms`, the nearest on ties.
pub fn snap(t_ms: i64, probs: &[f32], frame_ms: f64, radius_ms: i64) -> i64 {
    if probs.is_empty() || frame_ms <= 0.0 {
        return t_ms;
    }
    let frame = |ms: i64| ((ms.max(0) as f64 / frame_ms).round() as usize).min(probs.len() - 1);
    let (lo, hi, at) = (
        frame(t_ms - radius_ms),
        frame(t_ms + radius_ms),
        frame(t_ms),
    );
    let best = (lo..=hi)
        .min_by(|&a, &b| {
            probs[a]
                .total_cmp(&probs[b])
                .then(a.abs_diff(at).cmp(&b.abs_diff(at)))
        })
        .unwrap_or(at);
    (best as f64 * frame_ms).round() as i64
}

/// `span` cut at `changes` (ms from the span's start), each snapped to a
/// quiet frame of the track's `probs`. A cut that would leave a piece
/// shorter than `min_side_ms / 2` after snapping is skipped.
pub fn split_span(
    span: Span,
    changes: &[i64],
    probs: &[f32],
    frame_ms: f64,
    p: &TurnParams,
) -> Vec<Span> {
    let mut out = Vec::with_capacity(changes.len() + 1);
    let mut start = span.t0_ms;
    let min_piece = p.min_side_ms / 2;
    for &c in changes {
        let cut = snap(span.t0_ms + c, probs, frame_ms, p.snap_ms);
        if cut - start < min_piece || span.t1_ms - cut < min_piece {
            continue;
        }
        out.push(Span::new(start, cut));
        start = cut;
    }
    out.push(Span::new(start, span.t1_ms));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> TurnParams {
        TurnParams::default()
    }

    fn turn(t0_ms: i64, t1_ms: i64, speaker: i32) -> Turn {
        Turn {
            t0_ms,
            t1_ms,
            speaker,
        }
    }

    /// Voiceprints of synthetic audio where speaker A is `[0, change)` and B
    /// after it: the share of each voice in a range, as a 2-d vector.
    fn voices(change: i64) -> impl FnMut(i64, i64) -> Result<Option<Vec<f32>>> {
        move |t0, t1| {
            let a = (change.min(t1) - t0).max(0) as f32;
            let b = (t1 - change.max(t0)).max(0) as f32;
            Ok(Some(vec![a, b]))
        }
    }

    #[test]
    fn model_turns_propose_changes_between_speakers() {
        let turns = [
            turn(0, 6_000, 0),
            turn(6_400, 9_000, 1),
            // A backchannel inside B's turn proposes nothing.
            turn(7_000, 7_400, 0),
            // B again, after a pause: still B's turn.
            turn(9_200, 14_000, 1),
            turn(13_600, 20_000, 0),
        ];
        assert_eq!(model_candidates(&turns, &p()), vec![6_200, 13_800]);
        assert!(model_candidates(&[turn(0, 9_000, 3)], &p()).is_empty());
    }

    #[test]
    fn the_scan_finds_the_dip_between_two_voices() {
        let found = scan_candidates(12_000, &mut voices(5_000), &p()).unwrap();
        assert_eq!(found, vec![5_000]);
        // One voice throughout: no dip.
        assert!(scan_candidates(12_000, &mut voices(99_000), &p())
            .unwrap()
            .is_empty());
        // Too short to compare two windows.
        assert!(scan_candidates(2_000, &mut voices(1_000), &p())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn proposals_close_together_are_one() {
        let c = |t_ms, from_model| Candidate { t_ms, from_model };
        assert_eq!(
            merge_candidates(&[5_400, 12_000], &[5_000, 9_000], 1_000),
            vec![c(5_400, true), c(9_000, false), c(12_000, true)]
        );
    }

    #[test]
    fn only_changes_between_different_voices_are_confirmed() {
        let c = |t_ms| Candidate {
            t_ms,
            from_model: true,
        };
        let mut embed = voices(6_000);
        // 3 s is all A, so a "change" there has the same voice both sides.
        let kept = confirm(14_000, &[c(3_000), c(6_000)], &mut embed, &p()).unwrap();
        assert_eq!(kept, vec![6_000]);
        // Too close to an end to compare.
        assert!(confirm(7_000, &[c(6_000)], &mut voices(6_000), &p())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn find_changes_splits_two_voices_without_a_model() {
        struct Tone;
        impl Embedder for Tone {
            // Speaker A is positive samples, B negative.
            fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
                let a = pcm.iter().filter(|&&x| x > 0.0).count() as f32;
                let b = pcm.iter().filter(|&&x| x < 0.0).count() as f32;
                Ok(Some(vec![a, b]))
            }
        }
        let mut pcm = vec![0.5f32; 7 * SAMPLE_RATE];
        pcm.extend(vec![-0.5f32; 6 * SAMPLE_RATE]);
        let changes = find_changes(&pcm, None, &mut Tone, &p()).unwrap();
        assert_eq!(changes, vec![7_000]);
        // Too short to search.
        assert!(find_changes(&pcm[..SAMPLE_RATE * 2], None, &mut Tone, &p())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_wrong_model_change_is_not_confirmed() {
        struct Says(Vec<Turn>);
        impl TurnSegmenter for Says {
            fn turns(&mut self, _: &[f32]) -> Result<Vec<Turn>> {
                Ok(self.0.clone())
            }
        }
        struct One;
        impl Embedder for One {
            fn embed(&mut self, _: &[f32]) -> Result<Option<Vec<f32>>> {
                Ok(Some(vec![1.0, 0.0]))
            }
        }
        let pcm = vec![0.5f32; 12 * SAMPLE_RATE];
        let mut model = Says(vec![turn(0, 5_000, 0), turn(5_000, 12_000, 1)]);
        let changes = find_changes(&pcm, Some(&mut model), &mut One, &p()).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn cuts_snap_to_the_quietest_frame_nearby() {
        // 32 ms frames; frame 10 (320 ms) is the quietest within reach.
        let mut probs = vec![0.9f32; 40];
        probs[10] = 0.2;
        probs[30] = 0.0;
        assert_eq!(snap(400, &probs, 32.0, 300), 320);
        // Nothing quieter within reach: stays put.
        assert_eq!(snap(800, &probs, 32.0, 100), 800);
        assert_eq!(snap(500, &[], 32.0, 300), 500);
    }

    #[test]
    fn a_span_splits_at_its_changes() {
        let probs = vec![0.9f32; 1_000];
        let span = Span::new(10_000, 22_000);
        let pieces = split_span(span, &[4_000, 8_000], &probs, 32.0, &p());
        assert_eq!(
            pieces,
            vec![
                Span::new(10_000, 14_016),
                Span::new(14_016, 18_016),
                Span::new(18_016, 22_000)
            ]
        );
        // A cut too near an end is skipped.
        assert_eq!(split_span(span, &[11_900], &probs, 32.0, &p()), vec![span]);
        assert_eq!(split_span(span, &[], &probs, 32.0, &p()), vec![span]);
    }

    /// Calibration on a real meeting track. For each span: the model's
    /// turns, how alike each speaker model hears the two sides of each
    /// proposed change and of control points inside one turn, and what
    /// [`find_changes`] keeps (with the first model). Run with
    /// `GUIDY_TURNS_TRACK=<.gopus> GUIDY_TURNS_SPANS=<t0-t1,...>
    /// GUIDY_TEST_SPEAKER_MODEL=<onnx>[,<onnx>...]
    /// GUIDY_TEST_SEGMENTATION_MODEL=<onnx> cargo test -p guidy-core
    /// --release --lib turns::tests::calibrate -- --ignored --nocapture`
    // IGNORE-REASON: needs a recorded meeting and downloaded models.
    #[test]
    #[ignore]
    fn calibrate_on_a_real_track() {
        use crate::meetings::audio_store::TrackReader;
        use crate::meetings::processing::speaker_model::{PyannoteSegmenter, SpeakerEmbedder};
        use std::path::{Path, PathBuf};

        struct Model<'a>(&'a SpeakerEmbedder);
        impl Embedder for Model<'_> {
            fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
                self.0.embed(pcm)
            }
        }

        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let mut track = TrackReader::open(Path::new(&var("GUIDY_TURNS_TRACK"))).unwrap();
        let models: Vec<PathBuf> = var("GUIDY_TEST_SPEAKER_MODEL")
            .split(',')
            .map(PathBuf::from)
            .collect();
        let embedders: Vec<SpeakerEmbedder> = models
            .iter()
            .map(|m| SpeakerEmbedder::with_threads(m, 4).unwrap())
            .collect();
        let seg = PathBuf::from(var("GUIDY_TEST_SEGMENTATION_MODEL"));
        let mut segmenter = PyannoteSegmenter::new(&seg, &models[0]).unwrap();
        let p = p();
        for range in var("GUIDY_TURNS_SPANS").split(',') {
            let (a, b) = range.split_once('-').unwrap();
            let (t0, t1): (i64, i64) = (a.parse().unwrap(), b.parse().unwrap());
            let pcm = track.read_range(t0, t1).unwrap();
            let len = t1 - t0;
            let at = |ms: i64| ((ms.max(0) as usize) * SAMPLE_RATE / 1000).min(pcm.len());
            println!("\n=== span {t0}-{t1} ({:.1} s)", len as f64 / 1000.0);
            let turns = segmenter.turns(&pcm).unwrap();
            for t in &turns {
                println!("  turn {:>6}-{:>6} speaker {}", t.t0_ms, t.t1_ms, t.speaker);
            }
            let changes = model_candidates(&turns, &p);
            let controls: Vec<i64> = turns
                .iter()
                .filter(|t| t.t1_ms - t.t0_ms >= 6_000)
                .map(|t| (t.t0_ms + t.t1_ms) / 2)
                .collect();
            for (kind, points) in [("change ", &changes), ("control", &controls)] {
                for &c in points.iter() {
                    let mut line = format!("  {kind} {c:>6}:");
                    let (l, r) = ((c - p.side_ms).max(0), (c + p.side_ms).min(len));
                    for (m, e) in models.iter().zip(&embedders) {
                        let sides = (e.embed(&pcm[at(l)..at(c)]), e.embed(&pcm[at(c)..at(r)]));
                        let sim = match sides {
                            (Ok(Some(x)), Ok(Some(y))) => format!("{:.2}", cluster::cosine(&x, &y)),
                            _ => "-".into(),
                        };
                        line += &format!(" {}={sim}", m.file_stem().unwrap().to_string_lossy());
                    }
                    println!("{line}");
                }
            }
            let started = std::time::Instant::now();
            let found =
                find_changes(&pcm, Some(&mut segmenter), &mut Model(&embedders[0]), &p).unwrap();
            println!(
                "  find_changes: {found:?} ({} ms)",
                started.elapsed().as_millis()
            );
        }
    }
}
