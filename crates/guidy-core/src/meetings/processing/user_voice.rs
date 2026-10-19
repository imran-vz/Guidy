//! The user's recorded voice (People > Your voice): a short speech sample
//! from the dictation microphone, cut into windows and embedded. With it,
//! processing diarizes the mic track too, so other people in the room get
//! their own speakers (see `pipeline::cluster_units`).

use anyhow::{bail, Result};

use super::cluster;
use super::SAMPLE_RATE;

/// Each voiceprint covers this much speech.
const WINDOW_MS: usize = 3_000;
/// Windows start this far apart (they overlap by half).
const STEP_MS: usize = 1_500;
/// Less speech than this is too little to recognize the voice by.
pub const MIN_SPEECH_MS: usize = 8_000;
/// At most this many voiceprints are kept (evenly spread over the sample).
const MAX_PRINTS: usize = 24;

/// The sample was too short (speech only, silence is trimmed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceSampleTooShort;

impl std::fmt::Display for VoiceSampleTooShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Not enough speech was recorded. Read the text aloud for at least {} seconds.",
            MIN_SPEECH_MS / 1000
        )
    }
}

impl std::error::Error for VoiceSampleTooShort {}

/// Normalized voiceprints of 16 kHz mono speech `pcm`, one per window.
pub fn voiceprints_from_sample(
    pcm: &[f32],
    mut embed: impl FnMut(&[f32]) -> Result<Option<Vec<f32>>>,
) -> Result<Vec<Vec<f32>>> {
    let at = |ms: usize| ms * SAMPLE_RATE / 1000;
    if pcm.len() < at(MIN_SPEECH_MS) {
        bail!(VoiceSampleTooShort);
    }
    let starts: Vec<usize> = (0..=pcm.len() - at(WINDOW_MS))
        .step_by(at(STEP_MS))
        .collect();
    let keep = starts.len().min(MAX_PRINTS);
    let mut prints = Vec::with_capacity(keep);
    for j in 0..keep {
        let start = starts[j * starts.len() / keep];
        if let Some(e) = embed(&pcm[start..start + at(WINDOW_MS)])? {
            prints.push(cluster::normalized(&e));
        }
    }
    if prints.len() < 2 {
        bail!(VoiceSampleTooShort);
    }
    Ok(prints)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(s: usize) -> Vec<f32> {
        vec![0.1; s * SAMPLE_RATE]
    }

    #[test]
    fn a_sample_is_cut_into_overlapping_windows() {
        let mut lens = Vec::new();
        let prints = voiceprints_from_sample(&seconds(9), |pcm| {
            lens.push(pcm.len());
            Ok(Some(vec![3.0, 4.0]))
        })
        .unwrap();
        // Starts at 0, 1.5, 3, 4.5, 6 s.
        assert_eq!(prints.len(), 5);
        assert!(lens.iter().all(|&l| l == 3 * SAMPLE_RATE));
        assert_eq!(prints[0], vec![0.6, 0.8]);
    }

    #[test]
    fn a_long_sample_keeps_a_bounded_number_of_prints() {
        let prints = voiceprints_from_sample(&seconds(120), |_| Ok(Some(vec![1.0]))).unwrap();
        assert_eq!(prints.len(), MAX_PRINTS);
    }

    #[test]
    fn too_little_speech_is_refused() {
        let e = voiceprints_from_sample(&seconds(5), |_| Ok(Some(vec![1.0]))).unwrap_err();
        assert!(e.downcast_ref::<VoiceSampleTooShort>().is_some());
        let e = voiceprints_from_sample(&seconds(20), |_| Ok(None)).unwrap_err();
        assert!(e.downcast_ref::<VoiceSampleTooShort>().is_some());
    }
}
