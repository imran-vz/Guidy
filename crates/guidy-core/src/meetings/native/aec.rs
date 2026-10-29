//! Echo cancellation of the mic track, with the system track as reference.
//!
//! Without headphones the meeting plays through the speakers and the mic
//! records it again. The system track is exactly what was played, so it is
//! the echo canceller's reference ("far end"): WebRTC's AEC3 (the pure Rust
//! port in `sonora`) learns the speaker-to-mic path from it and removes the
//! echo from the mic, while the user's own voice passes. With headphones
//! there is no echo to find and AEC3's transparent mode leaves the mic alone.
//!
//! Both tracks arrive on one timeline (sample `n` of either was captured
//! `n / 16 kHz` after the capture started, see `guidy_mb_capture_start`), so
//! mic frame `k` is processed right after system frame `k`. The system
//! stream is captured as it is mixed, a little before the speakers play it,
//! so the reference leads the echo, as AEC3 expects; it estimates the
//! remaining delay itself.

use std::collections::VecDeque;

use sonora::config::{EchoCanceller as Aec3, HighPassFilter, TransparentModeType};
use sonora::{AudioProcessing, Config, StreamConfig};

use super::CAPTURE_SAMPLE_RATE;

/// AEC3 works on 10 ms frames.
const FRAME: usize = CAPTURE_SAMPLE_RATE as usize / 100;

/// How far the mic may run ahead of the system stream before its frames are
/// processed without a reference (the system stream stalled). The system
/// stream pads silence every 100 ms, so it is normally well within this.
const MAX_WAIT: usize = CAPTURE_SAMPLE_RATE as usize / 2;

/// Pairs the two tracks frame by frame and returns the echo-cancelled mic.
pub(crate) struct EchoCanceller {
    apm: AudioProcessing,
    /// Mic samples not yet processed; the first is at frame `done`.
    mic: VecDeque<f32>,
    /// System samples not yet used; the first is at frame `done`, after
    /// `skip` samples that belong to frames already processed without them.
    system: VecDeque<f32>,
    skip: usize,
    /// Frames processed so far.
    done: u64,
    /// Frames processed without a reference.
    unreferenced: u64,
}

impl EchoCanceller {
    pub(crate) fn new() -> Self {
        let config = Config {
            echo_canceller: Some(Aec3 {
                // Detects headphones (no echo) more quickly than Legacy.
                transparent_mode: TransparentModeType::Hmm,
                ..Default::default()
            }),
            high_pass_filter: Some(HighPassFilter::default()),
            // No noise suppression or gain control: the transcriber copes
            // with noise better than with their artifacts.
            ..Default::default()
        };
        let stream = StreamConfig::new(CAPTURE_SAMPLE_RATE, 1);
        Self {
            apm: AudioProcessing::builder()
                .config(config)
                .capture_config(stream)
                .render_config(stream)
                .build(),
            mic: VecDeque::new(),
            system: VecDeque::new(),
            skip: 0,
            done: 0,
            unreferenced: 0,
        }
    }

    /// Add mic audio; returns the cancelled mic audio that is ready.
    pub(crate) fn push_mic(&mut self, samples: &[f32]) -> Vec<f32> {
        self.mic.extend(samples);
        self.run(false)
    }

    /// Add system audio; returns the cancelled mic audio it made ready.
    pub(crate) fn push_system(&mut self, samples: &[f32]) -> Vec<f32> {
        let dropped = self.skip.min(samples.len());
        self.skip -= dropped;
        self.system.extend(&samples[dropped..]);
        self.run(false)
    }

    /// The rest of the mic audio, at the end of the capture.
    pub(crate) fn finish(&mut self) -> Vec<f32> {
        let mut out = self.run(true);
        // Less than a frame: passed through as is.
        out.extend(self.mic.drain(..));
        if self.unreferenced > 0 {
            log::info!(
                "meetings: echo cancellation ran without system audio for {} of {} frames",
                self.unreferenced,
                self.done
            );
        }
        out
    }

    fn run(&mut self, flushing: bool) -> Vec<f32> {
        let mut out = Vec::new();
        while self.mic.len() >= FRAME {
            let has_reference = self.system.len() >= FRAME;
            if !has_reference && !flushing && self.mic.len() < MAX_WAIT {
                break;
            }
            let mut mic: Vec<f32> = self.mic.drain(..FRAME).collect();
            let reference: Vec<f32> = if has_reference {
                self.system.drain(..FRAME).collect()
            } else {
                // The frame's system samples, when they come, are stale.
                self.skip += FRAME - self.system.len();
                self.system.clear();
                self.unreferenced += 1;
                vec![0.0; FRAME]
            };
            self.process(&reference, &mut mic);
            self.done += 1;
            out.extend_from_slice(&mic);
        }
        out
    }

    /// Cancel the echo of `reference` in `mic`, in place. On an error (it
    /// should not happen with fixed 10 ms mono frames) the frame stays as
    /// recorded.
    fn process(&mut self, reference: &[f32], mic: &mut [f32]) {
        let mut render_out = [0.0f32; FRAME];
        if let Err(e) = self
            .apm
            .process_render_f32(&[reference], &mut [&mut render_out[..]])
        {
            log::debug!("meetings: echo cancellation render failed: {e:?}");
            return;
        }
        let mut cleaned = [0.0f32; FRAME];
        match self
            .apm
            .process_capture_f32(&[&*mic], &mut [&mut cleaned[..]])
        {
            Ok(()) => mic.copy_from_slice(&cleaned),
            Err(e) => log::debug!("meetings: echo cancellation capture failed: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = CAPTURE_SAMPLE_RATE as usize;

    /// Speech-like noise: a few harmonics with a slow amplitude wobble plus
    /// a little deterministic noise, so the adaptive filter has a broadband
    /// signal to learn from.
    fn far_end(n: usize) -> Vec<f32> {
        let mut seed = 0x2545_f491_u32;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.2;
                let envelope = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 3.0 * t).sin();
                let tone: f32 = [220.0, 440.0, 660.0, 1320.0]
                    .iter()
                    .map(|f| (2.0 * std::f32::consts::PI * f * t).sin())
                    .sum::<f32>()
                    / 4.0;
                0.3 * envelope * (tone + noise)
            })
            .collect()
    }

    /// The far end as the mic hears it: delayed and quieter.
    fn echo_of(far: &[f32], delay: usize, gain: f32) -> Vec<f32> {
        (0..far.len())
            .map(|i| {
                if i >= delay {
                    far[i - delay] * gain
                } else {
                    0.0
                }
            })
            .collect()
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32
    }

    /// Feed both tracks in 100 ms chunks, system first as in capture.
    fn run(aec: &mut EchoCanceller, mic: &[f32], system: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for (m, s) in mic.chunks(RATE / 10).zip(system.chunks(RATE / 10)) {
            out.extend(aec.push_system(s));
            out.extend(aec.push_mic(m));
        }
        out.extend(aec.finish());
        out
    }

    #[test]
    fn output_has_the_mic_length_and_waits_for_the_reference() {
        let mut aec = EchoCanceller::new();
        // Mic ahead of the system stream: held back, not yet processed.
        assert!(aec.push_mic(&[0.0; FRAME * 3]).is_empty());
        assert_eq!(aec.push_system(&[0.0; FRAME * 2]).len(), FRAME * 2);
        assert_eq!(aec.push_system(&[0.0; FRAME]).len(), FRAME);
        // A partial frame is passed through at the end.
        assert!(aec.push_mic(&[0.0; 10]).is_empty());
        assert_eq!(aec.finish().len(), 10);
    }

    #[test]
    fn a_stalled_system_stream_does_not_hold_the_mic_back() {
        let mut aec = EchoCanceller::new();
        let out = aec.push_mic(&vec![0.0; MAX_WAIT + FRAME]);
        assert!(!out.is_empty(), "the mic waited past MAX_WAIT");
        let processed = out.len() / FRAME;
        // The late system audio for those frames is dropped, not used for
        // later frames.
        aec.push_system(&vec![0.0; processed * FRAME]);
        assert!(aec.system.is_empty());
        assert_eq!(aec.skip, 0);
    }

    #[test]
    fn removes_most_of_the_echo() {
        let secs = 8;
        let far = far_end(RATE * secs);
        // ~20 ms acoustic delay at -10 dB.
        let mic = echo_of(&far, RATE / 50, 0.3);
        let mut aec = EchoCanceller::new();
        let out = run(&mut aec, &mic, &far);
        assert_eq!(out.len(), mic.len());
        // After the filter has converged (the first seconds).
        let tail = RATE * 4;
        let before = energy(&mic[tail..]);
        let after = energy(&out[tail..]);
        let reduction_db = 10.0 * (before / after.max(1e-12)).log10();
        assert!(
            reduction_db > 20.0,
            "echo only reduced by {reduction_db:.1} dB"
        );
    }

    #[test]
    fn keeps_the_users_voice_while_the_meeting_plays() {
        let secs = 10;
        let far = far_end(RATE * secs);
        // The user: other pitches and rhythm, starting once AEC3 converged.
        let start = RATE * 5;
        let near: Vec<f32> = (0..far.len())
            .map(|i| {
                if i < start {
                    return 0.0;
                }
                let t = i as f32 / RATE as f32;
                let envelope = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 2.0 * t).cos();
                0.3 * envelope
                    * [150.0, 300.0, 900.0, 2000.0]
                        .iter()
                        .map(|f| (2.0 * std::f32::consts::PI * f * t).sin())
                        .sum::<f32>()
                    / 4.0
            })
            .collect();
        let echo = echo_of(&far, RATE / 50, 0.3);
        let mic: Vec<f32> = echo.iter().zip(&near).map(|(e, n)| e + n).collect();
        let mut aec = EchoCanceller::new();
        let out = run(&mut aec, &mic, &far);
        let kept = energy(&out[start + RATE..]) / energy(&near[start + RATE..]);
        assert!(
            kept > 0.3,
            "the user's voice was suppressed: energy ratio {kept:.2}"
        );
    }

    #[test]
    fn keeps_the_users_voice_when_nothing_plays() {
        let near = far_end(RATE * 4);
        let silence = vec![0.0; near.len()];
        let mut aec = EchoCanceller::new();
        let out = run(&mut aec, &near, &silence);
        // The high-pass filter takes a little; the voice stays.
        let ratio = energy(&out[RATE..]) / energy(&near[RATE..]);
        assert!(ratio > 0.5, "near-end speech lost: energy ratio {ratio:.2}");
    }
}
