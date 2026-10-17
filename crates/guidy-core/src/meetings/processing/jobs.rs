//! Background processing jobs: one meeting at a time, the rest queue.
//!
//! A single worker thread (started on demand, exiting when the queue is
//! empty) runs [`Pipeline`] for each queued meeting. Transcription goes to a
//! dedicated out-of-process transcribe worker (separate from dictation's), so
//! cancel kills that worker mid-run; VAD and voiceprints check the cancel
//! flag between segments. The meeting model is unloaded when the queue
//! drains.

use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{bail, Result};
use parking_lot::Mutex;

use super::pipeline::{
    next_stage, overall_fraction, Embedder, Pipeline, PipelineConfig, SegmentTranscriber,
};
use super::speaker_model::{ensure_speaker_model, SpeakerEmbedder};
use super::{is_cancelled, AudioSource};
use crate::audio_toolkit::SpeechScorer;
use crate::engine_supervisor::EngineSupervisor;
use crate::host::AppHandle;
use crate::managers::model::ModelManager;
use crate::managers::transcription::meeting::{resolve_meeting_model, MeetingTranscriber};
use crate::meetings::audio_store::{self, TrackReader};
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;
use crate::settings::{get_settings, VadBackend};

#[derive(Default)]
struct JobState {
    queue: VecDeque<MeetingId>,
    current: Option<(MeetingId, Arc<AtomicBool>)>,
    progress: HashMap<MeetingId, ProcessingProgress>,
    running: bool,
}

struct Inner {
    app: AppHandle,
    store: Arc<MeetingsStore>,
    meetings_dir: PathBuf,
    state: Mutex<JobState>,
    /// The meeting transcribe worker's supervisor (created on first use).
    engine: OnceLock<EngineSupervisor>,
}

/// The processing queue. Cheap to clone.
#[derive(Clone)]
pub struct Processor {
    inner: Arc<Inner>,
}

impl Processor {
    pub fn new(app: &AppHandle, store: Arc<MeetingsStore>, meetings_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                app: app.clone(),
                store,
                meetings_dir,
                state: Mutex::new(JobState::default()),
                engine: OnceLock::new(),
            }),
        }
    }

    /// Queue (or resume) processing `id`. A no-op if it is already queued or
    /// running.
    pub fn enqueue(&self, id: MeetingId) -> Result<()> {
        let Some(meeting) = self.inner.store.get_meeting(id)? else {
            bail!("meeting {id} not found");
        };
        if meeting.status == MeetingStatus::Recording {
            bail!("the meeting is still being recorded");
        }
        // Where it will resume from (if its checkpoint is intact).
        let next = match meeting
            .processing_stage
            .as_deref()
            .and_then(ProcessingStage::parse)
        {
            None | Some(ProcessingStage::Done) => ProcessingStage::Decode,
            Some(ProcessingStage::Merge) => ProcessingStage::Merge,
            Some(stage) => next_stage(stage),
        };
        // Fail fast (nothing queued) when transcription is still to do and
        // the meeting model isn't downloaded: the UI gets the typed
        // `MeetingModelMissing` error to offer the download.
        if next <= ProcessingStage::Transcribe {
            if let Some(models) = self.inner.app.try_state::<Arc<ModelManager>>() {
                let configured = get_settings(&self.inner.app).meetings.meeting_model;
                resolve_meeting_model(&models, configured.as_deref())?;
            }
        }
        let mut state = self.inner.state.lock();
        if state.queue.contains(&id) || state.current.as_ref().is_some_and(|(c, _)| *c == id) {
            return Ok(());
        }
        let queued = ProcessingProgress {
            meeting_id: id,
            stage: next,
            fraction: overall_fraction(next, 0.0).min(0.99),
            message: Some("queued".into()),
        };
        state.progress.insert(id, queued.clone());
        state.queue.push_back(id);
        let start = !state.running;
        state.running = true;
        drop(state);
        self.emit_progress(&queued);
        if start {
            let this = self.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("meeting-processing".into())
                .spawn(move || this.worker_loop())
            {
                let mut state = self.inner.state.lock();
                state.running = false;
                state.queue.retain(|q| *q != id);
                state.progress.remove(&id);
                bail!("could not start meeting processing: {e}");
            }
        }
        Ok(())
    }

    /// Cancel `id`: dropped from the queue, or stopped if it is running (its
    /// completed stages are kept, so processing it again resumes).
    pub fn cancel(&self, id: MeetingId) -> Result<()> {
        let mut state = self.inner.state.lock();
        if let Some(pos) = state.queue.iter().position(|q| *q == id) {
            state.queue.remove(pos);
            state.progress.remove(&id);
            return Ok(());
        }
        if let Some((current, flag)) = &state.current {
            if *current == id {
                flag.store(true, Ordering::SeqCst);
                if let Some(engine) = self.inner.engine.get() {
                    engine.cancel();
                }
            }
        }
        Ok(())
    }

    pub fn progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
        self.inner.state.lock().progress.get(&id).cloned()
    }

    pub fn is_busy(&self, id: MeetingId) -> bool {
        let state = self.inner.state.lock();
        state.queue.contains(&id) || state.current.as_ref().is_some_and(|(c, _)| *c == id)
    }

    /// Queue every meeting a crash left in `Processing`. Returns how many.
    pub fn resume_interrupted(&self) -> Result<usize> {
        let interrupted = self
            .inner
            .store
            .meetings_with_status(&[MeetingStatus::Processing])?;
        for m in &interrupted {
            self.enqueue(m.id)?;
        }
        Ok(interrupted.len())
    }

    fn emit_progress(&self, p: &ProcessingProgress) {
        let _ = self.inner.app.emit(events::PROCESSING_PROGRESS, p.clone());
    }

    fn emit_changed(&self, id: MeetingId) {
        let _ = self.inner.app.emit(events::MEETING_CHANGED, id);
    }

    fn engine(&self) -> EngineSupervisor {
        self.inner
            .engine
            .get_or_init(|| MeetingTranscriber::new().engine().clone())
            .clone()
    }

    fn worker_loop(self) {
        let mut transcriber: Option<MeetingTranscriber> = None;
        loop {
            let next = {
                let mut state = self.inner.state.lock();
                match state.queue.pop_front() {
                    Some(id) => {
                        let flag = Arc::new(AtomicBool::new(false));
                        state.current = Some((id, flag.clone()));
                        Some((id, flag))
                    }
                    None => None,
                }
            };
            let Some((id, cancel)) = next else {
                // Free the meeting model before deciding to exit, so a job
                // queued meanwhile never races the unload.
                if let Some(t) = transcriber.as_mut() {
                    t.unload();
                }
                let mut state = self.inner.state.lock();
                if state.queue.is_empty() {
                    state.running = false;
                    return;
                }
                continue;
            };
            let t =
                transcriber.get_or_insert_with(|| MeetingTranscriber::with_engine(self.engine()));
            let outcome = catch_unwind(AssertUnwindSafe(|| self.run_job(id, &cancel, t)));
            let outcome = outcome.unwrap_or_else(|panic| {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic".into());
                Err(anyhow::anyhow!("processing crashed: {msg}"))
            });
            self.inner.state.lock().current = None;
            self.finish(id, outcome);
        }
    }

    fn finish(&self, id: MeetingId, outcome: Result<()>) {
        let store = &self.inner.store;
        let result = match &outcome {
            Ok(()) => store.set_meeting_status(id, MeetingStatus::Processed, None),
            Err(e) if is_cancelled(e) => {
                log::info!("meeting {id}: processing cancelled");
                store.set_meeting_status(id, MeetingStatus::Recorded, None)
            }
            Err(e) => {
                log::error!("meeting {id}: processing failed: {e:#}");
                store.set_meeting_status(id, MeetingStatus::Failed, Some(&format!("{e:#}")))
            }
        };
        if let Err(e) = result {
            log::warn!("meeting {id}: could not record the processing outcome: {e:#}");
        }
        self.inner.state.lock().progress.remove(&id);
        self.emit_changed(id);
    }

    fn run_job(
        &self,
        id: MeetingId,
        cancel: &AtomicBool,
        transcriber: &mut MeetingTranscriber,
    ) -> Result<()> {
        let inner = &self.inner;
        let Some(meeting) = inner.store.get_meeting(id)? else {
            bail!("meeting {id} not found");
        };
        inner
            .store
            .set_meeting_status(id, MeetingStatus::Processing, None)?;
        self.emit_changed(id);

        let key = crate::secrets::meetings_data_key()?;
        let settings = get_settings(&inner.app);
        let audio_dir =
            audio_store::audio_dir(inner.meetings_dir.parent().unwrap_or(&inner.meetings_dir));
        let open = |rel: &Option<String>| -> Result<Option<Box<dyn AudioSource>>> {
            let Some(rel) = rel else { return Ok(None) };
            let path = audio_store::resolve_track_path(&audio_dir, rel)?;
            if !path.exists() {
                log::warn!("meeting {id}: track {} is missing", path.display());
                return Ok(None);
            }
            Ok(Some(Box::new(TrackReader::open(&path, &key)?)))
        };
        let mic = open(&meeting.mic_audio_path)?;
        let system = open(&meeting.system_audio_path)?;

        let language = Some(settings.selected_language.clone()).filter(|l| l != "auto");
        let mut segment_transcriber = LazyTranscriber {
            app: &inner.app,
            transcriber,
            configured: settings.meetings.meeting_model.clone(),
            language,
            ready: false,
        };
        let mut embedder = LazyEmbedder {
            app: &inner.app,
            embedder: None,
        };
        let app = inner.app.clone();
        let this = self.clone();
        Pipeline {
            meeting_id: id,
            store: &inner.store,
            meetings_dir: &inner.meetings_dir,
            key,
            config: PipelineConfig::from_settings(&settings.meetings),
            mic,
            system,
            vad: Box::new(move || make_detector(&app)),
            transcriber: &mut segment_transcriber,
            embedder: &mut embedder,
            cancel,
            on_progress: Box::new(move |p| {
                this.inner
                    .state
                    .lock()
                    .progress
                    .insert(p.meeting_id, p.clone());
                this.emit_progress(&p);
            }),
        }
        .run()
    }
}

/// Silero (the better detector for long-form audio), falling back to
/// Earshot when its model can't be loaded.
fn make_detector(app: &AppHandle) -> Result<Box<dyn SpeechScorer>> {
    crate::managers::audio::create_detector(VadBackend::Silero, app).or_else(|e| {
        log::warn!("Silero VAD unavailable for meeting processing ({e:#}); using Earshot");
        crate::managers::audio::create_detector(VadBackend::Earshot, app)
    })
}

/// Loads the meeting model on the first segment, so a run resuming past
/// transcription never loads it.
struct LazyTranscriber<'a> {
    app: &'a AppHandle,
    transcriber: &'a mut MeetingTranscriber,
    configured: Option<String>,
    language: Option<String>,
    ready: bool,
}

impl SegmentTranscriber for LazyTranscriber<'_> {
    fn transcribe(&mut self, pcm: Vec<f32>) -> Result<(String, Vec<Word>)> {
        if !self.ready {
            let models = self
                .app
                .try_state::<Arc<ModelManager>>()
                .ok_or_else(|| anyhow::anyhow!("the model manager is not running"))?;
            let (model_id, path) = resolve_meeting_model(&models, self.configured.as_deref())?;
            log::info!("meeting processing uses model {model_id}");
            self.transcriber.load(self.app, &path)?;
            self.ready = true;
        }
        let t = self
            .transcriber
            .transcribe_words(pcm, self.language.clone())?;
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

/// Downloads (if needed) and loads the speaker model on first use.
struct LazyEmbedder<'a> {
    app: &'a AppHandle,
    embedder: Option<SpeakerEmbedder>,
}

impl Embedder for LazyEmbedder<'_> {
    fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
        if self.embedder.is_none() {
            let path = ensure_speaker_model(self.app)?;
            self.embedder = Some(SpeakerEmbedder::new(&path)?);
        }
        self.embedder.as_ref().expect("loaded").embed(pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing::TestApp;

    fn meeting(status: MeetingStatus) -> Meeting {
        Meeting {
            id: 0,
            title: "t".into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at: 0,
            ended_at: Some(1),
            mic_audio_path: None,
            system_audio_path: None,
            status,
            error: None,
            processing_stage: None,
        }
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !done() {
            assert!(
                start.elapsed().as_secs() < 20,
                "timed out waiting for {what}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_job_runs_in_the_background_and_records_its_outcome() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let p = Processor::new(&app.app, store.clone(), dir.path().join("meetings"));
        let id = store
            .insert_meeting(&meeting(MeetingStatus::Recorded))
            .unwrap();

        p.enqueue(id).unwrap();
        wait_until("the job to finish", || {
            !p.is_busy(id)
                && store.get_meeting(id).unwrap().unwrap().status != MeetingStatus::Recorded
        });
        // No audio: it fails with a readable error, and can be retried.
        let m = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Failed);
        assert!(m.error.unwrap().contains("no recorded audio"));
        assert!(p.progress(id).is_none());
        let names = app.event_names();
        assert!(names.iter().any(|n| n == events::PROCESSING_PROGRESS));
        assert!(names.iter().any(|n| n == events::MEETING_CHANGED));
        // The worker thread exits once the queue is empty.
        wait_until("the worker to exit", || !p.inner.state.lock().running);
        p.enqueue(id).unwrap();
        wait_until("the retry", || !p.is_busy(id));
    }

    #[test]
    fn recording_or_unknown_meetings_are_refused() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let id = store
            .insert_meeting(&meeting(MeetingStatus::Recording))
            .unwrap();
        assert!(p.enqueue(id).is_err());
        assert!(p.enqueue(9999).is_err());
        assert!(!p.is_busy(id));
        // Cancelling something that isn't running is fine.
        p.cancel(id).unwrap();
    }

    #[test]
    fn cancelling_a_queued_meeting_unqueues_it() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let id = store
            .insert_meeting(&meeting(MeetingStatus::Recorded))
            .unwrap();
        // Hold the state lock's queue as if a worker were busy elsewhere.
        p.inner.state.lock().running = true;
        p.enqueue(id).unwrap();
        assert!(p.is_busy(id));
        assert_eq!(p.progress(id).unwrap().message.as_deref(), Some("queued"));
        p.enqueue(id).unwrap(); // no duplicate
        assert_eq!(p.inner.state.lock().queue.len(), 1);
        p.cancel(id).unwrap();
        assert!(!p.is_busy(id));
        assert!(p.progress(id).is_none());
    }
}
