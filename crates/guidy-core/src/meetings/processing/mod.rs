//! Meeting processing (plan task 1.4): VAD -> transcription with word
//! timestamps -> voiceprints -> clustering + matching -> one timeline.
//!
//! - [`vad`]: speech segmentation over Silero probabilities.
//! - [`cluster`]: agglomerative clustering, matching against known people.
//! - [`timeline`]: merging the two tracks, with the D9 echo safety net.
//! - [`pipeline`]: the resumable stage runner for one meeting.
//! - [`jobs`]: the background queue (one meeting at a time) and cancel.
//! - [`speaker_model`]: the sherpa-onnx speaker model download + extractor.
//! - [`sidecar`]: encrypted checkpoint / per-segment voiceprint files.
//! - [`naming`]: speaker suggestions and naming (stores voiceprints).
//!
//! Where it runs: a background thread in the app process orchestrates the
//! work. Transcription goes out of process through a dedicated
//! `EngineSupervisor` worker (see `managers::transcription::meeting`), so a
//! model crash can't take the app down and the dictation model stays loaded.
//! Silero VAD and sherpa-onnx embeddings run in-process (CPU, small models),
//! as Silero already does for dictation.

pub mod cluster;
pub mod jobs;
pub mod naming;
pub mod pipeline;
pub mod sidecar;
pub mod speaker_model;
pub mod timeline;
pub mod vad;

#[cfg(test)]
mod e2e;

use anyhow::Result;

use crate::meetings::audio_store::TrackReader;

pub use jobs::Processor;
pub use sidecar::delete_sidecars;
pub use speaker_model::{
    ensure_speaker_model, installed_speaker_model, speaker_model_installed, speaker_model_status,
    start_speaker_model_download, SpeakerModelMissing, SpeakerModelStatus, SPEAKER_MODEL_NAME,
    SPEAKER_MODEL_SIZE, SPEAKER_MODEL_STATUS_EVENT, SPEAKER_MODEL_TRAINED_ON,
};

/// All meeting audio is 16 kHz mono.
pub const SAMPLE_RATE: usize = 16_000;

/// The run was cancelled. Downcast to tell a cancel from a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("processing was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Whether `e` is a cancellation (ours or the transcription engine's).
pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<Cancelled>().is_some()
            || matches!(
                c.downcast_ref::<crate::engine_supervisor::EngineError>(),
                Some(crate::engine_supervisor::EngineError::Cancelled)
            )
    })
}

/// A track's audio: 16 kHz mono f32 by time range.
pub trait AudioSource {
    fn duration_ms(&mut self) -> Result<i64>;
    /// `[t0_ms, t1_ms)`, clipped to the track.
    fn read_range(&mut self, t0_ms: i64, t1_ms: i64) -> Result<Vec<f32>>;
}

impl AudioSource for TrackReader {
    fn duration_ms(&mut self) -> Result<i64> {
        Ok(TrackReader::duration_ms(self)? as i64)
    }

    fn read_range(&mut self, t0_ms: i64, t1_ms: i64) -> Result<Vec<f32>> {
        TrackReader::read_range(self, t0_ms, t1_ms)
    }
}

/// In-memory PCM (tests, WAV files).
pub struct PcmSource {
    pcm: Vec<f32>,
}

impl PcmSource {
    pub fn new(pcm: Vec<f32>) -> Self {
        Self { pcm }
    }
}

impl AudioSource for PcmSource {
    fn duration_ms(&mut self) -> Result<i64> {
        Ok((self.pcm.len() * 1000 / SAMPLE_RATE) as i64)
    }

    fn read_range(&mut self, t0_ms: i64, t1_ms: i64) -> Result<Vec<f32>> {
        let at = |ms: i64| ((ms.max(0) as usize) * SAMPLE_RATE / 1000).min(self.pcm.len());
        let (a, b) = (at(t0_ms), at(t1_ms));
        Ok(if b > a {
            self.pcm[a..b].to_vec()
        } else {
            Vec::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_source_clips_ranges() {
        let mut s = PcmSource::new(vec![0.5; SAMPLE_RATE * 2]);
        assert_eq!(s.duration_ms().unwrap(), 2000);
        assert_eq!(s.read_range(500, 1000).unwrap().len(), SAMPLE_RATE / 2);
        assert_eq!(s.read_range(1500, 9000).unwrap().len(), SAMPLE_RATE / 2);
        assert!(s.read_range(3000, 4000).unwrap().is_empty());
    }

    #[test]
    fn recognizes_cancellation() {
        assert!(is_cancelled(&anyhow::Error::new(Cancelled)));
        assert!(is_cancelled(
            &anyhow::Error::new(crate::engine_supervisor::EngineError::Cancelled)
                .context("transcribing")
        ));
        assert!(!is_cancelled(&anyhow::anyhow!("boom")));
    }
}
