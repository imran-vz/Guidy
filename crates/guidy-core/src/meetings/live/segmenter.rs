//! Cuts one track's audio into utterances for live transcription, from a
//! speech detector's per-frame probabilities. Pure: frames and
//! probabilities in, [`Cut`]s out.
//!
//! An utterance opens on the first speech frame (with a little audio from
//! before it, so the first word isn't clipped) and closes after a pause.
//! While it goes on, the audio so far is offered every
//! [`SegmenterParams::partial_every_ms`] for a tentative decode; a long
//! monologue is cut at [`SegmenterParams::max_ms`] so its text keeps coming.

use std::collections::VecDeque;

use crate::meetings::processing::SAMPLE_RATE;

#[derive(Debug, Clone)]
pub struct SegmenterParams {
    /// A frame at or above this probability is speech.
    pub speech_prob: f32,
    /// Silence that ends an utterance.
    pub end_silence_ms: i64,
    /// Audio kept from before the first speech frame.
    pub pre_roll_ms: i64,
    /// Longest utterance before it is cut.
    pub max_ms: i64,
    /// Utterances with less speech than this are dropped (clicks, coughs).
    pub min_speech_ms: i64,
    /// How often the utterance so far is offered while it goes on.
    pub partial_every_ms: i64,
}

impl Default for SegmenterParams {
    fn default() -> Self {
        Self {
            speech_prob: 0.5,
            end_silence_ms: 600,
            pre_roll_ms: 200,
            max_ms: 12_000,
            min_speech_ms: 250,
            partial_every_ms: 1_200,
        }
    }
}

/// An utterance to decode. `id` is the segmenter's own count: the same id
/// for every partial of an utterance and its final.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub id: u64,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub pcm: Vec<f32>,
    /// The utterance is still going.
    pub partial: bool,
}

struct Open {
    id: u64,
    t0_ms: i64,
    pcm: Vec<f32>,
    speech_ms: i64,
    silence_ms: i64,
    /// Length (ms) when it was last offered as a partial.
    offered_ms: i64,
}

pub struct Segmenter {
    params: SegmenterParams,
    /// Recording time of the next frame.
    clock_ms: i64,
    pre_roll: VecDeque<f32>,
    open: Option<Open>,
    next_id: u64,
}

fn ms_of(samples: usize) -> i64 {
    (samples * 1000 / SAMPLE_RATE) as i64
}

fn samples_of(ms: i64) -> usize {
    ms.max(0) as usize * SAMPLE_RATE / 1000
}

impl Segmenter {
    pub fn new(params: SegmenterParams) -> Self {
        Self {
            params,
            clock_ms: 0,
            pre_roll: VecDeque::new(),
            open: None,
            next_id: 0,
        }
    }

    /// Feed the next frame and the detector's speech probability for it.
    pub fn push_frame(&mut self, frame: &[f32], prob: f32) -> Option<Cut> {
        let frame_ms = ms_of(frame.len());
        let speech = prob >= self.params.speech_prob;
        let t0 = self.clock_ms;
        self.clock_ms += frame_ms;

        let Some(open) = self.open.as_mut() else {
            if speech {
                let pre: Vec<f32> = self.pre_roll.drain(..).collect();
                let mut pcm = pre;
                let pre_ms = ms_of(pcm.len());
                pcm.extend_from_slice(frame);
                self.open = Some(Open {
                    id: self.next_id,
                    t0_ms: t0 - pre_ms,
                    pcm,
                    speech_ms: frame_ms,
                    silence_ms: 0,
                    offered_ms: 0,
                });
                self.next_id += 1;
            } else {
                self.pre_roll.extend(frame.iter().copied());
                let keep = samples_of(self.params.pre_roll_ms);
                while self.pre_roll.len() > keep {
                    self.pre_roll.pop_front();
                }
            }
            return None;
        };

        open.pcm.extend_from_slice(frame);
        if speech {
            open.speech_ms += frame_ms;
            open.silence_ms = 0;
        } else {
            open.silence_ms += frame_ms;
        }
        let len_ms = ms_of(open.pcm.len());
        if open.silence_ms >= self.params.end_silence_ms || len_ms >= self.params.max_ms {
            return self.close();
        }
        // Not in a pause: the final is about to come anyway.
        if speech && len_ms - open.offered_ms >= self.params.partial_every_ms {
            open.offered_ms = len_ms;
            return Some(Cut {
                id: open.id,
                t0_ms: open.t0_ms,
                t1_ms: open.t0_ms + len_ms,
                pcm: open.pcm.clone(),
                partial: true,
            });
        }
        None
    }

    /// End the utterance in progress (the recording stopped).
    pub fn flush(&mut self) -> Option<Cut> {
        self.close()
    }

    /// The final cut of the open utterance, without most of its trailing
    /// silence; `None` when it had too little speech.
    fn close(&mut self) -> Option<Cut> {
        let mut open = self.open.take()?;
        // Keep a pre-roll's worth of the closing pause.
        let trailing = (open.silence_ms - self.params.pre_roll_ms).max(0);
        let keep = open.pcm.len().saturating_sub(samples_of(trailing));
        open.pcm.truncate(keep);
        if open.speech_ms < self.params.min_speech_ms {
            return None;
        }
        Some(Cut {
            id: open.id,
            t0_ms: open.t0_ms,
            t1_ms: open.t0_ms + ms_of(open.pcm.len()),
            pcm: open.pcm,
            partial: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: usize = 512; // 32 ms

    /// Feed `ms` of frames at `prob`; returns the cuts.
    fn feed(s: &mut Segmenter, ms: i64, prob: f32) -> Vec<Cut> {
        let frames = samples_of(ms) / FRAME;
        (0..frames)
            .filter_map(|_| s.push_frame(&[0.1; FRAME], prob))
            .collect()
    }

    #[test]
    fn an_utterance_closes_after_a_pause_with_its_pre_roll() {
        let mut s = Segmenter::new(SegmenterParams::default());
        assert!(feed(&mut s, 1_000, 0.0).is_empty());
        let cuts = feed(&mut s, 800, 0.9);
        assert!(cuts.is_empty(), "shorter than a partial interval");
        let cuts = feed(&mut s, 700, 0.0);
        assert_eq!(cuts.len(), 1);
        let c = &cuts[0];
        assert!(!c.partial);
        assert_eq!(c.id, 0);
        // Opened ~200 ms before the speech at 1 s.
        assert!((780..=1_000).contains(&c.t0_ms), "{}", c.t0_ms);
        // The closing pause is trimmed to about a pre-roll.
        let len = c.t1_ms - c.t0_ms;
        assert!((900..=1_300).contains(&len), "{len}");
        assert_eq!(ms_of(c.pcm.len()), len);
    }

    #[test]
    fn partials_come_while_speaking_with_the_same_id_as_the_final() {
        let mut s = Segmenter::new(SegmenterParams::default());
        let cuts = feed(&mut s, 3_000, 0.9);
        assert_eq!(cuts.len(), 2);
        assert!(cuts.iter().all(|c| c.partial && c.id == 0));
        assert!(cuts[1].pcm.len() > cuts[0].pcm.len());
        let fin = feed(&mut s, 700, 0.0);
        assert_eq!(fin.len(), 1);
        assert!(!fin[0].partial);
        assert_eq!(fin[0].id, 0);
        // The next utterance is a new one.
        feed(&mut s, 500, 0.9);
        assert_eq!(s.flush().map(|c| c.id), Some(1));
    }

    #[test]
    fn a_monologue_is_cut_at_the_longest_length() {
        let mut s = Segmenter::new(SegmenterParams::default());
        let finals: Vec<Cut> = feed(&mut s, 30_000, 0.9)
            .into_iter()
            .filter(|c| !c.partial)
            .collect();
        assert_eq!(finals.len(), 2);
        assert!(finals.iter().all(|c| c.t1_ms - c.t0_ms <= 12_000));
        assert_eq!(finals[1].t0_ms, finals[0].t1_ms);
    }

    #[test]
    fn blips_are_dropped_and_flush_ends_the_last_utterance() {
        let mut s = Segmenter::new(SegmenterParams::default());
        // One 32 ms frame of "speech": a click.
        feed(&mut s, 32, 0.9);
        assert!(feed(&mut s, 800, 0.0).is_empty());
        assert!(s.flush().is_none());
        feed(&mut s, 600, 0.9);
        let c = s.flush().expect("a final cut");
        assert!(!c.partial);
        assert!(s.flush().is_none());
    }
}
