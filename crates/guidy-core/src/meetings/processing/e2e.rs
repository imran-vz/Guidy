//! End-to-end tests with the real models. Ignored by default: they download
//! the speaker model (26.5 MB) and, unless `GUIDY_TEST_MODEL` points at a
//! GGUF, Parakeet TDT 0.6B v3 Q4_K_M (485 MB), into
//! `$GUIDY_TEST_MODELS_DIR` (default: `<tmp>/guidy-test-models`).
//!
//! ```text
//! cargo build -p guidy-app   # the transcribe worker binary
//! GUIDY_TRANSCRIBE_WORKER_EXE=target/debug/guidy \
//!   cargo test -p guidy-core meetings::processing::e2e -- --ignored --nocapture --test-threads=1
//! ```

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::pipeline::{Embedder, Pipeline, PipelineConfig, SegmentTranscriber};
use super::speaker_model::{
    download_verified, SpeakerEmbedder, SPEAKER_MODEL_FILE, SPEAKER_MODEL_SHA256, SPEAKER_MODEL_URL,
};
use super::{cluster, PcmSource, SAMPLE_RATE};
use crate::audio_toolkit::SpeechScorer;
use crate::managers::transcription::meeting::MeetingTranscriber;
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;

const PARAKEET_URL: &str = "https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/main/parakeet-tdt-0.6b-v3-Q4_K_M.gguf";
const PARAKEET_SHA256: &str = "b68557be1e3c40207fd7c4bd9d63f1d3316b963f15325bfb0cc16a8bb0ffd181";

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn models_dir() -> PathBuf {
    std::env::var_os("GUIDY_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("guidy-test-models"))
}

fn fetch(url: &str, file: &str, sha: &str) -> PathBuf {
    let path = models_dir().join(file);
    if !path.exists() {
        eprintln!("downloading {url}");
        crate::host::async_runtime::block_on(download_verified(url, &path, sha, |_, _| {}))
            .expect("download");
    }
    path
}

fn speaker_model() -> PathBuf {
    fetch(SPEAKER_MODEL_URL, SPEAKER_MODEL_FILE, SPEAKER_MODEL_SHA256)
}

fn fixture_speech() -> Vec<f32> {
    crate::audio_toolkit::read_wav_samples(root().join("fixtures/audio/speech-16k-mono.wav"))
        .expect("fixture wav")
}

#[test]
#[ignore = "downloads the speaker model"]
fn speaker_model_gives_stable_voiceprints() {
    let embedder = SpeakerEmbedder::new(&speaker_model()).unwrap();
    assert_eq!(embedder.dim(), 256);
    let speech = fixture_speech();
    let half = speech.len() / 2;
    let a = embedder.embed(&speech[..half]).unwrap().unwrap();
    let b = embedder.embed(&speech[half..]).unwrap().unwrap();
    // A crude "other voice": the same audio played 25% faster.
    let fast: Vec<f32> = (0..speech.len() * 4 / 5)
        .map(|i| speech[(i as f32 * 1.25) as usize])
        .collect();
    let c = embedder.embed(&fast).unwrap().unwrap();
    let same = cluster::cosine(&a, &b);
    let other = cluster::cosine(&a, &c);
    eprintln!("same speaker {same:.3}, pitch-shifted {other:.3}");
    assert!(same > 0.5, "{same}");
    assert!(same > other, "{same} vs {other}");
    // Very short audio must not fail (it may or may not yield a print).
    embedder.embed(&speech[..400]).unwrap();
}

struct RealTranscriber(MeetingTranscriber);

impl SegmentTranscriber for RealTranscriber {
    fn transcribe(&mut self, pcm: Vec<f32>) -> anyhow::Result<(String, Vec<Word>)> {
        let t = self.0.transcribe_words(pcm, None)?;
        let words = t
            .words
            .iter()
            .map(|w| Word {
                t0_ms: w.t0_ms,
                t1_ms: w.t1_ms,
                text: w.text.trim().to_string(),
            })
            .filter(|w| !w.text.is_empty())
            .collect();
        Ok((t.text.trim().to_string(), words))
    }
}

struct RealEmbedder(SpeakerEmbedder);

impl Embedder for RealEmbedder {
    fn embed(&mut self, pcm: &[f32]) -> anyhow::Result<Option<Vec<f32>>> {
        self.0.embed(pcm)
    }
}

/// The fixture is said twice by the "remote" speaker; the mic hears a faint
/// echo of the first time. Expect one remote speaker, word timestamps in
/// meeting time, and the echo dropped.
#[test]
#[ignore = "downloads models; needs GUIDY_TRANSCRIBE_WORKER_EXE"]
fn full_pipeline_on_the_fixture() {
    let model = std::env::var_os("GUIDY_TEST_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            fetch(
                PARAKEET_URL,
                "parakeet-tdt-0.6b-v3-Q4_K_M.gguf",
                PARAKEET_SHA256,
            )
        });
    let app = crate::host::testing::TestApp::new();
    let mut transcriber = MeetingTranscriber::new();
    transcriber
        .load(&app.app, &model)
        .expect("load the meeting model");

    let speech = fixture_speech();
    let at = |ms: usize| ms * SAMPLE_RATE / 1000;
    let total = at(20_000);
    let mut system = vec![0f32; total];
    system[at(1_000)..at(1_000) + speech.len()].copy_from_slice(&speech);
    system[at(10_000)..at(10_000) + speech.len()].copy_from_slice(&speech);
    let mut mic = vec![0f32; total];
    for (i, s) in speech.iter().enumerate() {
        mic[at(1_000) + i] = s * 0.5;
    }

    let key = [1u8; 32];
    let store = MeetingsStore::open_in_memory(&key).unwrap();
    let id = store
        .insert_meeting(&Meeting {
            id: 0,
            title: "e2e".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: 0,
            ended_at: Some(20_000),
            mic_audio_path: None,
            system_audio_path: None,
            status: MeetingStatus::Processing,
            error: None,
            processing_stage: None,
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let vad_model = root().join("assets/resources/models/silero-vad-v6.2-F32.gguf");
    let mut t = RealTranscriber(transcriber);
    let mut e = RealEmbedder(SpeakerEmbedder::new(&speaker_model()).unwrap());
    let cancel = AtomicBool::new(false);
    Pipeline {
        meeting_id: id,
        store: &store,
        meetings_dir: dir.path(),
        key,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(mic))),
        system: Some(Box::new(PcmSource::new(system))),
        vad: Box::new(move || {
            Ok(
                Box::new(crate::audio_toolkit::SileroVad::new(&vad_model, 0.3)?)
                    as Box<dyn SpeechScorer>,
            )
        }),
        transcriber: &mut t,
        embedder: &mut e,
        cancel: &cancel,
        on_progress: Box::new(|p| eprintln!("{:?} {:.2}", p.stage, p.fraction)),
    }
    .run()
    .unwrap();
    t.0.unload();

    let segments = store.segments(id).unwrap();
    for s in &segments {
        eprintln!("[{:?} {}-{}] {}", s.track, s.t0_ms, s.t1_ms, s.text);
    }
    assert!(
        segments.iter().all(|s| s.track == Track::System),
        "echo dropped"
    );
    assert!(segments.len() >= 2);
    let words: Vec<&Word> = segments.iter().flat_map(|s| &s.words).collect();
    assert!(!words.is_empty(), "word timestamps");
    assert!(
        words.iter().any(|w| w.t0_ms >= 10_000),
        "offset into meeting time"
    );
    let speakers = store.speakers(id).unwrap();
    assert_eq!(speakers.len(), 1, "{speakers:?}");
    assert_eq!(speakers[0].cluster_label, "Speaker 1");
}
