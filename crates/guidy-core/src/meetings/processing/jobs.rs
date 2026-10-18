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
    ///
    /// The row is marked `Processing` before this returns (by a statement
    /// that also re-checks the audio is still there), so audio retention,
    /// which only clears meetings that aren't `Processing`, can never delete
    /// the audio of a queued meeting. Its previous status and error are
    /// saved in the row, so a cancel restores them even after a restart.
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
        // Retention deletes a processed meeting's audio. Re-running from
        // the audio would only fail and mark a good meeting `Failed`, so
        // refuse up front and leave its status and stage alone.
        if next <= ProcessingStage::Voiceprints && !has_audio(&meeting) {
            bail!(AUDIO_DELETED);
        }
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
        // Under the queue lock: one enqueue claims it.
        let needs_audio = next <= ProcessingStage::Voiceprints;
        if !self.inner.store.mark_processing(id, needs_audio)? {
            drop(state);
            // Changed since it was read: deleted, or retention cleared its
            // audio meanwhile.
            match self.inner.store.get_meeting(id)? {
                None => bail!("meeting {id} not found"),
                Some(m) if m.status == MeetingStatus::Recording => {
                    bail!("the meeting is still being recorded")
                }
                Some(_) => bail!(AUDIO_DELETED),
            }
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
        self.emit_changed(id);
        if start {
            let this = self.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("meeting-processing".into())
                .spawn(move || this.worker_loop())
            {
                {
                    let mut state = self.inner.state.lock();
                    state.running = false;
                    state.queue.retain(|q| *q != id);
                    state.progress.remove(&id);
                }
                if let Err(e2) = self.restore_status(id) {
                    log::warn!("meeting {id}: {e2:#}");
                }
                bail!("could not start meeting processing: {e}");
            }
        }
        Ok(())
    }

    /// Cancel `id`: dropped from the queue, or stopped if it is running (its
    /// completed stages are kept, so processing it again resumes). Either
    /// way it gets back the status (and error) it had before it was queued:
    /// a reprocessed meeting stays `Processed`, a retry stays `Failed`. One
    /// with nothing saved (a crash leftover from an older version) goes back
    /// to `Recorded`, so the UI offers "Process" again instead of a spinner.
    pub fn cancel(&self, id: MeetingId) -> Result<()> {
        let mut state = self.inner.state.lock();
        if let Some(pos) = state.queue.iter().position(|q| *q == id) {
            state.queue.remove(pos);
            state.progress.remove(&id);
            drop(state);
            if self.restore_status(id)? {
                self.emit_changed(id);
            }
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

    /// Undo [`Self::enqueue`]'s `Processing`: back to the status saved when
    /// it was queued (`Recorded` when none was). Returns whether the meeting
    /// still exists.
    fn restore_status(&self, id: MeetingId) -> Result<bool> {
        if self.inner.store.get_meeting(id)?.is_none() {
            return Ok(false);
        }
        self.inner.store.restore_status_before_processing(id)?;
        Ok(true)
    }

    pub fn progress(&self, id: MeetingId) -> Option<ProcessingProgress> {
        self.inner.state.lock().progress.get(&id).cloned()
    }

    pub fn is_busy(&self, id: MeetingId) -> bool {
        let state = self.inner.state.lock();
        state.queue.contains(&id) || state.current.as_ref().is_some_and(|(c, _)| *c == id)
    }

    /// True while `id`'s job is the one running.
    pub fn is_running(&self, id: MeetingId) -> bool {
        self.inner
            .state
            .lock()
            .current
            .as_ref()
            .is_some_and(|(c, _)| *c == id)
    }

    /// [`Self::cancel`], then wait (at most `timeout`) until `id`'s job has
    /// stopped running, so nothing writes its files afterwards. Returns false
    /// on timeout.
    pub fn cancel_and_wait(&self, id: MeetingId, timeout: std::time::Duration) -> Result<bool> {
        self.cancel(id)?;
        let deadline = std::time::Instant::now() + timeout;
        while self.is_running(id) {
            if std::time::Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Ok(true)
    }

    /// Queue every meeting left in `Processing` (queued or running when the
    /// app quit or crashed). One that can't be queued (e.g. the meeting
    /// model is missing) never sits in `Processing` forever: a meeting that
    /// was `Processed` before it was queued keeps its notes (the reason is
    /// logged); any other is set to `Failed` with the reason. The others
    /// still resume. Returns how many were queued.
    pub fn resume_interrupted(&self) -> Result<usize> {
        self.resume_interrupted_with(|id| self.enqueue(id))
    }

    fn resume_interrupted_with(&self, enqueue: impl Fn(MeetingId) -> Result<()>) -> Result<usize> {
        let interrupted = self
            .inner
            .store
            .meetings_with_status(&[MeetingStatus::Processing])?;
        let mut queued = 0;
        for m in &interrupted {
            match enqueue(m.id) {
                Ok(()) => queued += 1,
                Err(e) => {
                    log::error!("meeting {}: could not resume processing: {e:#}", m.id);
                    let store = &self.inner.store;
                    let result = match store.status_before_processing(m.id) {
                        // Its notes are still good: only the rerun is lost.
                        Ok(Some(MeetingStatus::Processed)) => {
                            store.restore_status_before_processing(m.id).map(|_| ())
                        }
                        _ => store.finish_processing(
                            m.id,
                            MeetingStatus::Failed,
                            Some(&format!("{e:#}")),
                        ),
                    };
                    if let Err(e2) = result {
                        log::warn!("meeting {}: {e2:#}", m.id);
                    }
                    self.emit_changed(m.id);
                }
            }
        }
        Ok(queued)
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
        // Deleted while it ran: drop whatever sidecars it wrote (they hold
        // voiceprints of people nobody named) instead of recording an outcome.
        if matches!(store.get_meeting(id), Ok(None)) {
            log::info!("meeting {id}: deleted during processing; removing its files");
            if let Err(e) = super::sidecar::delete_sidecars(&self.inner.meetings_dir, id) {
                log::warn!("meeting {id}: {e:#}");
            }
            self.inner.state.lock().progress.remove(&id);
            self.emit_changed(id);
            return;
        }
        let result = match &outcome {
            Ok(()) => store.finish_processing(id, MeetingStatus::Processed, None),
            Err(e) if is_cancelled(e) => {
                log::info!("meeting {id}: processing cancelled");
                store.restore_status_before_processing(id).map(|_| ())
            }
            Err(e) => {
                log::error!("meeting {id}: processing failed: {e:#}");
                store.finish_processing(id, MeetingStatus::Failed, Some(&format!("{e:#}")))
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

/// Why [`Processor::enqueue`] refuses a meeting without audio.
pub const AUDIO_DELETED: &str =
    "this meeting's audio has been deleted, so it can't be processed again";

/// Whether the meeting still has a recorded track to process.
pub fn has_audio(meeting: &Meeting) -> bool {
    meeting.mic_audio_path.is_some() || meeting.system_audio_path.is_some()
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
            // A track whose file doesn't exist: runs fail in the pipeline.
            mic_audio_path: Some("missing-mic.gopus".into()),
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
        // Marked before enqueue returns, so retention keeps its audio.
        assert_ne!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Recorded
        );
        // The outcome is recorded before the progress entry goes.
        wait_until("the job to finish", || {
            !p.is_busy(id) && p.progress(id).is_none()
        });
        // No audio file: it fails with a readable error, and can be retried.
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
    fn meetings_whose_audio_was_deleted_are_refused_untouched() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        // Retention cleared the paths of a processed meeting.
        let id = store
            .insert_meeting(&Meeting {
                mic_audio_path: None,
                system_audio_path: None,
                processing_stage: Some(ProcessingStage::Done.as_str().into()),
                ..meeting(MeetingStatus::Processed)
            })
            .unwrap();
        let err = p.enqueue(id).unwrap_err();
        assert_eq!(err.to_string(), AUDIO_DELETED);
        assert!(!p.is_busy(id));
        assert!(p.progress(id).is_none());
        let m = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Processed);
        assert_eq!(m.processing_stage.as_deref(), Some("done"));
        assert_eq!(m.error, None);
        assert!(app.event_names().is_empty());
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
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Processing
        );
        p.cancel(id).unwrap();
        assert!(!p.is_busy(id));
        assert!(p.progress(id).is_none());
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Recorded
        );
    }

    #[test]
    fn cancelling_a_queued_retry_restores_the_failure() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let id = store
            .insert_meeting(&Meeting {
                error: Some("boom".into()),
                ..meeting(MeetingStatus::Failed)
            })
            .unwrap();
        p.inner.state.lock().running = true;
        p.enqueue(id).unwrap();
        let m = store.get_meeting(id).unwrap().unwrap();
        assert_eq!((m.status, m.error), (MeetingStatus::Processing, None));
        p.cancel(id).unwrap();
        let m = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Failed);
        assert_eq!(m.error.as_deref(), Some("boom"));
    }

    /// Retention checks `is_busy`, then clears the paths. A reprocess queued
    /// in between must still keep its audio.
    #[test]
    fn retention_keeps_the_audio_of_a_meeting_queued_meanwhile() {
        use crate::meetings::recorder::adapters::StoreRepo;
        const DAY: i64 = 24 * 60 * 60 * 1000;
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let p = Processor::new(&app.app, store.clone(), dir.path().join("meetings"));
        let audio_dir = dir.path().join("audio");
        std::fs::create_dir_all(&audio_dir).unwrap();
        let file = audio_dir.join("1-mic.gopus");
        std::fs::write(&file, b"x").unwrap();
        let now = 100 * DAY;
        let id = store
            .insert_meeting(&Meeting {
                ended_at: Some(now - 9 * DAY),
                mic_audio_path: Some("1-mic.gopus".into()),
                processing_stage: Some(ProcessingStage::Done.as_str().into()),
                ..meeting(MeetingStatus::Processed)
            })
            .unwrap();
        // No worker: the job stays queued.
        p.inner.state.lock().running = true;
        let repo = StoreRepo(store.clone());

        // Not busy when retention looks, queued right after.
        let enqueue_after_check = |m: MeetingId| {
            p.enqueue(m).unwrap();
            false
        };
        let out =
            audio_store::apply_retention(&repo, &audio_dir, Some(7), now, &enqueue_after_check)
                .unwrap();
        assert!(out.meetings.is_empty());
        assert!(file.exists());
        let m = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(m.mic_audio_path.as_deref(), Some("1-mic.gopus"));
        assert_eq!(m.status, MeetingStatus::Processing);
        assert!(p.is_busy(id));

        // Cancelled before it ran: processed again, so retention applies.
        p.cancel(id).unwrap();
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Processed
        );
        let out =
            audio_store::apply_retention(&repo, &audio_dir, Some(7), now, &|_| false).unwrap();
        assert_eq!(out.meetings, vec![id]);
        assert!(!file.exists());
        // And a reprocess is now refused, leaving the meeting alone.
        assert_eq!(p.enqueue(id).unwrap_err().to_string(), AUDIO_DELETED);
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Processed
        );
    }

    #[test]
    fn cancelling_a_queued_interrupted_meeting_resets_it_to_recorded() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let id = store
            .insert_meeting(&meeting(MeetingStatus::Processing))
            .unwrap();
        p.inner.state.lock().running = true;
        assert_eq!(p.resume_interrupted().unwrap(), 1);
        assert!(p.is_busy(id));
        p.cancel(id).unwrap();
        assert!(!p.is_busy(id));
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Recorded
        );
        // A queued meeting in another state keeps it.
        let done = store
            .insert_meeting(&meeting(MeetingStatus::Processed))
            .unwrap();
        p.enqueue(done).unwrap();
        p.cancel(done).unwrap();
        assert_eq!(
            store.get_meeting(done).unwrap().unwrap().status,
            MeetingStatus::Processed
        );
    }

    #[test]
    fn resume_marks_unqueueable_meetings_failed_and_continues() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let bad = store
            .insert_meeting(&meeting(MeetingStatus::Processing))
            .unwrap();
        let good = store
            .insert_meeting(&meeting(MeetingStatus::Processing))
            .unwrap();
        p.inner.state.lock().running = true;
        let n = p
            .resume_interrupted_with(|id| {
                if id == bad {
                    anyhow::bail!("the meeting model is not downloaded")
                }
                p.enqueue(id)
            })
            .unwrap();
        assert_eq!(n, 1);
        let m = store.get_meeting(bad).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Failed);
        assert!(m.error.unwrap().contains("not downloaded"));
        assert!(p.is_busy(good));
    }

    /// The status before queueing is in the database: after a restart, a
    /// failed resume or a cancel still gives it back.
    #[test]
    fn the_status_before_queueing_survives_a_restart() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let before = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        let done = store
            .insert_meeting(&meeting(MeetingStatus::Processed))
            .unwrap();
        let failed = store
            .insert_meeting(&Meeting {
                error: Some("boom".into()),
                ..meeting(MeetingStatus::Failed)
            })
            .unwrap();
        let recorded = store
            .insert_meeting(&meeting(MeetingStatus::Recorded))
            .unwrap();
        before.inner.state.lock().running = true;
        for id in [done, failed, recorded] {
            before.enqueue(id).unwrap();
        }
        drop(before);

        // Restarted: nothing in memory. The meeting model is gone, so the
        // reprocessed and the new meeting can't be queued again.
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        p.inner.state.lock().running = true;
        let n = p
            .resume_interrupted_with(|id| {
                if id != failed {
                    anyhow::bail!("the meeting model is not downloaded")
                }
                p.enqueue(id)
            })
            .unwrap();
        assert_eq!(n, 1);
        // A good meeting keeps its notes.
        let m = store.get_meeting(done).unwrap().unwrap();
        assert_eq!((m.status, m.error), (MeetingStatus::Processed, None));
        // One that never got processed says why.
        let m = store.get_meeting(recorded).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Failed);
        assert!(m.error.unwrap().contains("not downloaded"));
        // The queued retry, cancelled, is back to its failure.
        assert!(p.is_busy(failed));
        p.cancel(failed).unwrap();
        let m = store.get_meeting(failed).unwrap().unwrap();
        assert_eq!(m.status, MeetingStatus::Failed);
        assert_eq!(m.error.as_deref(), Some("boom"));
    }

    #[test]
    fn cancelling_a_running_job_restores_the_status_before_queueing() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store.clone(), std::env::temp_dir());
        p.inner.state.lock().running = true;
        let cancelled = || anyhow::Error::new(super::super::Cancelled);
        let done = store
            .insert_meeting(&meeting(MeetingStatus::Processed))
            .unwrap();
        let failed = store
            .insert_meeting(&Meeting {
                error: Some("boom".into()),
                ..meeting(MeetingStatus::Failed)
            })
            .unwrap();
        let recorded = store
            .insert_meeting(&meeting(MeetingStatus::Recorded))
            .unwrap();
        let leftover = store
            .insert_meeting(&meeting(MeetingStatus::Processing))
            .unwrap();
        for id in [done, failed, recorded, leftover] {
            p.enqueue(id).unwrap();
            // What the worker does when it takes the job.
            p.inner.state.lock().queue.retain(|q| *q != id);
            store
                .set_meeting_status(id, MeetingStatus::Processing, None)
                .unwrap();
            p.finish(id, Err(cancelled()));
        }
        let status = |id| {
            let m = store.get_meeting(id).unwrap().unwrap();
            (m.status, m.error)
        };
        assert_eq!(status(done), (MeetingStatus::Processed, None));
        assert_eq!(
            status(failed),
            (MeetingStatus::Failed, Some("boom".to_string()))
        );
        assert_eq!(status(recorded), (MeetingStatus::Recorded, None));
        assert_eq!(status(leftover), (MeetingStatus::Recorded, None));

        // A finished run drops the saved status: a later cancel of a crash
        // leftover can't bring back a stale one.
        p.enqueue(failed).unwrap();
        p.inner.state.lock().queue.clear();
        p.finish(failed, Ok(()));
        assert_eq!(status(failed), (MeetingStatus::Processed, None));
        assert_eq!(store.status_before_processing(failed).unwrap(), None);
    }

    #[test]
    fn cancel_and_wait_waits_for_the_running_job() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let p = Processor::new(&app.app, store, std::env::temp_dir());
        let flag = Arc::new(AtomicBool::new(false));
        p.inner.state.lock().current = Some((7, flag.clone()));
        // Never stops: times out.
        assert!(!p
            .cancel_and_wait(7, std::time::Duration::from_millis(50))
            .unwrap());
        assert!(flag.load(Ordering::SeqCst));
        let worker = p.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            worker.inner.state.lock().current = None;
        });
        assert!(p
            .cancel_and_wait(7, std::time::Duration::from_secs(10))
            .unwrap());
        assert!(!p.is_running(7));
        t.join().unwrap();
    }

    #[test]
    fn finishing_a_deleted_meeting_removes_its_sidecars() {
        let app = TestApp::new();
        let store = Arc::new(MeetingsStore::open_in_memory(&[3u8; 32]).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let p = Processor::new(&app.app, store.clone(), dir.path().to_path_buf());
        let id = store
            .insert_meeting(&meeting(MeetingStatus::Processing))
            .unwrap();
        let key = [3u8; 32];
        super::super::sidecar::write_embeddings(dir.path(), id, &key, &[]).unwrap();
        super::super::sidecar::write_cache(
            dir.path(),
            id,
            &key,
            &super::super::sidecar::ProcessingCache::new(),
        )
        .unwrap();
        store.delete_meeting(id).unwrap();
        p.finish(id, Ok(()));
        assert!(!super::super::sidecar::embeddings_path(dir.path(), id).exists());
        assert!(!super::super::sidecar::cache_path(dir.path(), id).exists());
    }
}
