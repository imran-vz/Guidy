//! Speech segmentation of one meeting track.
//!
//! The detector (Silero, from `audio_toolkit::vad`) only scores frames; the
//! segmentation itself is a pure function over those probabilities
//! ([`speech_spans`]), so it is tested without a model.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{AudioSource, Cancelled, SAMPLE_RATE};
use crate::audio_toolkit::SpeechScorer;

/// A stretch of a track, in meeting milliseconds.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub t0_ms: i64,
    pub t1_ms: i64,
}

impl Span {
    pub fn new(t0_ms: i64, t1_ms: i64) -> Self {
        Self { t0_ms, t1_ms }
    }

    pub fn len_ms(&self) -> i64 {
        (self.t1_ms - self.t0_ms).max(0)
    }

    /// Overlap with `other` in ms (0 when disjoint).
    pub fn overlap_ms(&self, other: &Span) -> i64 {
        (self.t1_ms.min(other.t1_ms) - self.t0_ms.max(other.t0_ms)).max(0)
    }

    /// Gap to `other` in ms (0 when they overlap or touch).
    pub fn gap_ms(&self, other: &Span) -> i64 {
        (other.t0_ms - self.t1_ms)
            .max(self.t0_ms - other.t1_ms)
            .max(0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VadParams {
    /// Speech starts at a frame scoring at least this.
    pub onset: f32,
    /// ...and ends at the first frame scoring below this (hysteresis).
    pub offset: f32,
    /// Speech runs shorter than this are dropped (clicks, coughs).
    pub min_speech_ms: i64,
    /// Padding added on both sides of each run, so word edges aren't cut.
    pub pad_ms: i64,
    /// Runs closer than this are merged into one segment...
    pub merge_gap_ms: i64,
    /// ...unless the merged segment would be longer than this. Longer runs
    /// are split at their quietest frame.
    pub max_segment_ms: i64,
    /// A split never leaves a piece shorter than this.
    pub min_split_ms: i64,
}

impl Default for VadParams {
    fn default() -> Self {
        Self {
            onset: 0.45,
            offset: 0.3,
            min_speech_ms: 250,
            pad_ms: 150,
            merge_gap_ms: 600,
            max_segment_ms: 30_000,
            min_split_ms: 8_000,
        }
    }
}

/// Speech spans from per-frame speech probabilities (`frame_ms` apart,
/// starting at 0 ms). Spans are sorted, disjoint, and never longer than
/// `max_segment_ms`.
pub fn speech_spans(probs: &[f32], frame_ms: f64, params: &VadParams) -> Vec<Span> {
    let n = probs.len();
    if n == 0 || frame_ms <= 0.0 {
        return Vec::new();
    }
    let frames = |ms: i64| ((ms as f64) / frame_ms).round().max(0.0) as usize;
    let min_speech = frames(params.min_speech_ms).max(1);
    let pad = frames(params.pad_ms);
    let merge_gap = frames(params.merge_gap_ms);
    // Floor, so a segment never exceeds the cap after rounding.
    let max_len = ((params.max_segment_ms as f64 / frame_ms).floor() as usize).max(1);
    let min_split = frames(params.min_split_ms).clamp(1, max_len);

    // 1. Hysteresis runs [start, end) in frames.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &p) in probs.iter().enumerate() {
        match start {
            None if p >= params.onset => start = Some(i),
            Some(s) if p < params.offset => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, n));
    }

    // 2. Drop blips, 3. pad.
    let padded: Vec<(usize, usize)> = runs
        .into_iter()
        .filter(|(s, e)| e - s >= min_speech)
        .map(|(s, e)| (s.saturating_sub(pad), (e + pad).min(n)))
        .collect();

    // 4. Merge close runs while the result stays within the length cap.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in padded {
        if let Some(last) = merged.last_mut() {
            let overlapping = s <= last.1;
            let close = s.saturating_sub(last.1) <= merge_gap;
            if overlapping || (close && e - last.0 <= max_len) {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    // 5. Split anything too long at its quietest frame.
    let mut out = Vec::new();
    for (s, e) in merged {
        split_long(probs, s, e, max_len, min_split, &mut out);
    }
    out.into_iter()
        .map(|(s, e)| {
            Span::new(
                (s as f64 * frame_ms).round() as i64,
                (e as f64 * frame_ms).round() as i64,
            )
        })
        .collect()
}

fn split_long(
    probs: &[f32],
    mut s: usize,
    e: usize,
    max_len: usize,
    min_split: usize,
    out: &mut Vec<(usize, usize)>,
) {
    while e - s > max_len {
        // Cut somewhere in [s + min_split, s + max_len], leaving at least
        // `min_split` for the rest when possible; prefer the quietest frame.
        let lo = s + min_split;
        let hi = (s + max_len).min(e.saturating_sub(min_split)).max(lo);
        let cut = (lo..=hi.min(e - 1))
            .min_by(|&a, &b| probs[a].total_cmp(&probs[b]).then(b.cmp(&a)))
            .unwrap_or(s + max_len);
        out.push((s, cut));
        s = cut;
    }
    if e > s {
        out.push((s, e));
    }
}

/// Score a whole track with `scorer`, reading it in windows so a long
/// meeting is never decoded at once. Returns one probability per
/// `scorer.frame_samples()` frame from the start of the track.
pub fn score_track(
    scorer: &mut dyn SpeechScorer,
    source: &mut dyn AudioSource,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(f32),
) -> Result<Vec<f32>> {
    const WINDOW_MS: i64 = 60_000;
    scorer.reset();
    let frame = scorer.frame_samples();
    let duration = source.duration_ms()?;
    let mut probs = Vec::with_capacity((duration as usize * SAMPLE_RATE / 1000) / frame + 1);
    let mut pending: Vec<f32> = Vec::new();
    let mut t = 0;
    while t < duration {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let t1 = (t + WINDOW_MS).min(duration);
        pending.extend(source.read_range(t, t1)?);
        let whole = pending.len() / frame * frame;
        if whole > 0 {
            probs.extend(scorer.score(&pending[..whole])?);
            pending.drain(..whole);
        }
        t = t1;
        on_progress(t as f32 / duration.max(1) as f32);
    }
    if !pending.is_empty() {
        pending.resize(frame, 0.0);
        probs.extend(scorer.score(&pending)?);
    }
    Ok(probs)
}

/// Frame length in ms for a detector with `frame_samples` per frame.
pub fn frame_ms(frame_samples: usize) -> f64 {
    frame_samples as f64 * 1000.0 / SAMPLE_RATE as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_MS: f64 = 32.0;

    /// Probabilities from `(ms, p)` runs.
    fn probs(runs: &[(i64, f32)]) -> Vec<f32> {
        runs.iter()
            .flat_map(|&(ms, p)| std::iter::repeat_n(p, (ms as f64 / FRAME_MS) as usize))
            .collect()
    }

    fn params() -> VadParams {
        VadParams {
            pad_ms: 0,
            ..VadParams::default()
        }
    }

    #[test]
    fn finds_speech_with_hysteresis() {
        // A dip to 0.35 stays speech (above offset 0.3); 0.1 ends it.
        let p = probs(&[
            (960, 0.1),
            (960, 0.9),
            (320, 0.35),
            (640, 0.9),
            (1920, 0.05),
        ]);
        let spans = speech_spans(&p, FRAME_MS, &params());
        assert_eq!(spans, vec![Span::new(960, 2880)]);
    }

    #[test]
    fn drops_blips_and_merges_short_gaps() {
        let p = probs(&[
            (960, 0.0),
            (64, 0.9), // blip: dropped
            (960, 0.0),
            (1600, 0.9),
            (320, 0.0), // short gap: merged
            (1600, 0.9),
            (1600, 0.0), // long gap: kept apart
            (960, 0.9),
        ]);
        let spans = speech_spans(&p, FRAME_MS, &params());
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert_eq!(spans[0], Span::new(1984, 5504));
        assert_eq!(spans[1].t1_ms, p.len() as i64 * 32);
    }

    #[test]
    fn pads_without_running_off_the_ends() {
        let p = probs(&[(64, 0.0), (640, 0.9), (640, 0.0)]);
        let spans = speech_spans(&p, FRAME_MS, &VadParams::default());
        assert_eq!(spans, vec![Span::new(0, 64 + 640 + 160)]);
    }

    #[test]
    fn splits_long_speech_at_the_quietest_frame() {
        // 70 s of speech with a soft dip (0.5, still speech) at 20 s.
        let mut p = probs(&[(70_016, 0.9)]);
        let dip = (20_000.0 / FRAME_MS) as usize;
        p[dip] = 0.5;
        let spans = speech_spans(&p, FRAME_MS, &params());
        assert!(spans.len() >= 3, "{spans:?}");
        assert!(spans.iter().all(|s| s.len_ms() <= 30_000), "{spans:?}");
        assert_eq!(spans[0], Span::new(0, dip as i64 * 32));
        assert_eq!(spans.first().unwrap().t0_ms, 0);
        assert_eq!(spans.last().unwrap().t1_ms, p.len() as i64 * 32);
        for w in spans.windows(2) {
            assert_eq!(w[0].t1_ms, w[1].t0_ms);
        }
    }

    #[test]
    fn merging_never_exceeds_the_cap() {
        // Many 5 s runs with 300 ms gaps: merged up to <= 30 s pieces.
        let mut runs = Vec::new();
        for _ in 0..20 {
            runs.push((4_992, 0.9));
            runs.push((288, 0.0));
        }
        let spans = speech_spans(&probs(&runs), FRAME_MS, &params());
        assert!(spans.len() > 1);
        assert!(spans.iter().all(|s| s.len_ms() <= 30_000), "{spans:?}");
    }

    #[test]
    fn empty_and_silent_input() {
        assert!(speech_spans(&[], FRAME_MS, &params()).is_empty());
        assert!(speech_spans(&[0.1; 500], FRAME_MS, &params()).is_empty());
    }

    #[test]
    fn span_geometry() {
        let a = Span::new(0, 1000);
        let b = Span::new(800, 2000);
        let c = Span::new(2500, 3000);
        assert_eq!(a.overlap_ms(&b), 200);
        assert_eq!(a.gap_ms(&b), 0);
        assert_eq!(a.gap_ms(&c), 1500);
        assert_eq!(c.gap_ms(&a), 1500);
    }

    /// Silero over the speech fixture, padded with silence on both sides.
    #[test]
    fn silero_finds_the_fixture_speech() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let wav = format!("{root}/fixtures/audio/speech-16k-mono.wav");
        let model = format!("{root}/assets/resources/models/silero-vad-v6.2-F32.gguf");
        let speech = crate::audio_toolkit::read_wav_samples(&wav).expect("fixture wav");
        let mut pcm = vec![0.0f32; 3 * SAMPLE_RATE];
        pcm.extend(&speech);
        pcm.extend(vec![0.0f32; 3 * SAMPLE_RATE]);
        let speech_span = Span::new(3000, 3000 + (speech.len() * 1000 / SAMPLE_RATE) as i64);

        let mut vad = crate::audio_toolkit::SileroVad::new(model, 0.3).expect("silero");
        let mut source = super::super::PcmSource::new(pcm);
        let p = score_track(&mut vad, &mut source, &AtomicBool::new(false), |_| {}).unwrap();
        let frame = crate::audio_toolkit::VoiceActivityDetector::frame_samples(&vad);
        let spans = speech_spans(&p, frame_ms(frame), &VadParams::default());
        assert!(!spans.is_empty());
        let first = spans.first().unwrap();
        let last = spans.last().unwrap();
        assert!(first.t0_ms >= 2500, "{spans:?}");
        assert!(last.t1_ms <= speech_span.t1_ms + 1000, "{spans:?}");
        let covered: i64 = spans.iter().map(|s| s.overlap_ms(&speech_span)).sum();
        assert!(covered * 10 >= speech_span.len_ms() * 6, "{spans:?}");
    }
}
