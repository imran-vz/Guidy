//! The processing pipeline for one meeting:
//! decode -> VAD -> transcribe -> voiceprints -> cluster/match -> merge.
//!
//! Each completed stage is recorded as `Meeting::processing_stage`, and its
//! results are kept in the encrypted [`ProcessingCache`] sidecar, so a
//! cancelled or crashed run resumes from the next stage. The transcribe and
//! voiceprint stages also checkpoint part-way, so they resume mid-stage.
//!
//! The engines are traits ([`SegmentTranscriber`], [`Embedder`], a VAD
//! factory), so the whole pipeline runs in tests on synthetic audio.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use super::cluster::{self, ClusterParams, KnownVoice};
use super::sidecar::{self, Clustering, ProcessingCache, SegmentEmbedding, Unit, UnitTranscript};
use super::timeline::{self, EchoParams, RawSegment};
use super::vad::{self, VadParams};
use super::{AudioSource, Cancelled};
use crate::audio_toolkit::SpeechScorer;
use crate::meetings::settings::MeetingSettings;
use crate::meetings::store::MeetingsStore;
use crate::meetings::types::*;

/// Transcribes one segment's PCM. Returns the text and its words, with times
/// relative to the start of `pcm`.
pub trait SegmentTranscriber {
    fn transcribe(&mut self, pcm: Vec<f32>) -> Result<(String, Vec<Word>)>;

    /// The language `text` is in, when the transcriber autodetects the
    /// language and can tell. `None` (the default) skips the meeting-wide
    /// language pass.
    fn language_of(&self, _text: &str) -> Option<String> {
        None
    }

    /// [`transcribe`](Self::transcribe) in `language` (a code
    /// [`language_of`](Self::language_of) returned).
    fn transcribe_in(&mut self, pcm: Vec<f32>, _language: &str) -> Result<(String, Vec<Word>)> {
        self.transcribe(pcm)
    }

    /// The most audio (ms) one [`transcribe_batch`](Self::transcribe_batch)
    /// call takes. `Some` makes the pipeline send consecutive segments of a
    /// track together (a cloud API, priced and rate-limited per request);
    /// `None` (the default) sends one segment at a time.
    fn batch_limit_ms(&self) -> Option<i64> {
        None
    }

    /// Transcribe several segments; one result per segment, in order, with
    /// times relative to the start of its own `pcm`.
    fn transcribe_batch(&mut self, pcms: Vec<Vec<f32>>) -> Result<Vec<(String, Vec<Word>)>> {
        pcms.into_iter().map(|pcm| self.transcribe(pcm)).collect()
    }
}

/// Computes a voiceprint for one segment's PCM (`None` if too short).
pub trait Embedder {
    fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>>;
}

/// Tunables, from settings plus fixed defaults.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub vad: VadParams,
    pub cluster: ClusterParams,
    pub echo: EchoParams,
    /// System segments shorter than this get no voiceprint of their own; they
    /// join the nearest cluster in time.
    pub min_embed_ms: i64,
    pub match_threshold: f32,
    pub suggest_threshold: f32,
    /// With the user's voice recorded, mic speech at least this similar to
    /// it is the user's; less similar speech is someone else in the room.
    pub user_voice_threshold: f32,
    /// Label of the mic speaker (the user).
    pub user_label: String,
    /// The user's own language (the app's): the meeting's language when
    /// autodetect leaves no clear main one (see [`main_language`]).
    pub preferred_language: Option<String>,
}

impl PipelineConfig {
    pub fn from_settings(s: &MeetingSettings) -> Self {
        let name = s.user_display_name.trim();
        Self {
            vad: VadParams::default(),
            cluster: ClusterParams::default(),
            echo: EchoParams::default(),
            min_embed_ms: 1_500,
            match_threshold: s.speaker_match_threshold,
            suggest_threshold: s.speaker_suggest_threshold.min(s.speaker_match_threshold),
            user_voice_threshold: 0.5,
            user_label: if name.is_empty() {
                "me".into()
            } else {
                name.into()
            },
            preferred_language: None,
        }
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self::from_settings(&MeetingSettings::default())
    }
}

/// The stage after `s` (`Done` stays `Done`).
pub fn next_stage(s: ProcessingStage) -> ProcessingStage {
    use ProcessingStage::*;
    match s {
        Decode => Vad,
        Vad => Transcribe,
        Transcribe => Voiceprints,
        Voiceprints => Cluster,
        Cluster => Merge,
        Merge | Done => Done,
    }
}

/// Where a run starts, given the last completed stage and the cached
/// results (already checked for readability and version). Returns the cache
/// to continue from and the first stage to run (`Done` = nothing to do).
pub fn resume_point(
    completed: Option<ProcessingStage>,
    cache: Option<ProcessingCache>,
) -> (ProcessingCache, ProcessingStage) {
    let cache = cache.filter(|c| c.version == sidecar::CACHE_VERSION);
    match (completed, cache) {
        // Never started, or a finished meeting being reprocessed.
        (None, _) | (Some(ProcessingStage::Done), _) => {
            (ProcessingCache::new(), ProcessingStage::Decode)
        }
        // The transcript is stored; only the final bookkeeping was lost.
        (Some(ProcessingStage::Merge), _) => (ProcessingCache::new(), ProcessingStage::Done),
        (Some(stage), Some(cache)) if cache_covers(&cache, stage) => (cache, next_stage(stage)),
        // The checkpoint is gone or inconsistent: start over.
        (Some(_), _) => (ProcessingCache::new(), ProcessingStage::Decode),
    }
}

/// Whether `cache` holds every result up to and including `stage`.
fn cache_covers(cache: &ProcessingCache, stage: ProcessingStage) -> bool {
    use ProcessingStage::*;
    let n = cache.units.len();
    let decoded = cache.mic_ms.is_some() || cache.system_ms.is_some();
    let vad = cache.transcripts.len() == n && cache.embeddings.len() == n;
    match stage {
        Decode => decoded,
        Vad => decoded && vad,
        Transcribe => decoded && vad && cache.transcripts.iter().all(Option::is_some),
        Voiceprints | Cluster => {
            decoded
                && vad
                && cache.transcripts.iter().all(Option::is_some)
                && cache.embeddings.iter().all(Option::is_some)
                && (stage == Voiceprints || cache.clustering.is_some())
        }
        Merge | Done => true,
    }
}

/// The share of the whole run before `stage` starts, and its own share.
fn stage_window(stage: ProcessingStage) -> (f32, f32) {
    use ProcessingStage::*;
    match stage {
        Decode => (0.0, 0.02),
        Vad => (0.02, 0.08),
        Transcribe => (0.10, 0.65),
        Voiceprints => (0.75, 0.17),
        Cluster => (0.92, 0.03),
        Merge => (0.95, 0.05),
        Done => (1.0, 0.0),
    }
}

pub fn overall_fraction(stage: ProcessingStage, within: f32) -> f32 {
    let (start, len) = stage_window(stage);
    (start + len * within.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// What one run needs. Construct it, then call [`Pipeline::run`].
pub struct Pipeline<'a> {
    pub meeting_id: MeetingId,
    pub store: &'a MeetingsStore,
    /// `<app_data_dir>/meetings` (sidecars live under it).
    pub meetings_dir: &'a Path,
    pub key: [u8; 32],
    pub config: PipelineConfig,
    pub mic: Option<Box<dyn AudioSource + 'a>>,
    pub system: Option<Box<dyn AudioSource + 'a>>,
    /// Makes a fresh speech detector (one per track).
    pub vad: Box<dyn FnMut() -> Result<Box<dyn SpeechScorer>> + 'a>,
    pub transcriber: &'a mut dyn SegmentTranscriber,
    pub embedder: &'a mut dyn Embedder,
    pub cancel: &'a AtomicBool,
    pub on_progress: Box<dyn FnMut(ProcessingProgress) + 'a>,
}

/// Write the cache at most this often inside a long stage.
const CHECKPOINT_EVERY: Duration = Duration::from_secs(15);

impl<'a> Pipeline<'a> {
    /// Run (or resume) to the end. Errors with [`Cancelled`] when cancelled;
    /// the completed stages stay recorded either way. The meeting must be
    /// `Processing`: once it isn't, nothing more is saved and the run stops
    /// as cancelled.
    pub fn run(mut self) -> Result<()> {
        let id = self.meeting_id;
        let Some(meeting) = self.store.get_meeting(id)? else {
            bail!("meeting {id} not found");
        };
        let completed = meeting
            .processing_stage
            .as_deref()
            .and_then(ProcessingStage::parse);
        let cached = match sidecar::read_cache(self.meetings_dir, id, &self.key) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("meeting {id}: discarding the processing checkpoint: {e:#}");
                None
            }
        };
        let (mut cache, mut stage) = resume_point(completed, cached);
        if completed.is_some() && stage == ProcessingStage::Decode {
            // Starting over: forget the recorded stage too.
            self.record_stage(None)?;
        }
        log::info!("meeting {id}: processing from stage {}", stage.as_str());

        while stage != ProcessingStage::Done {
            self.check_cancel()?;
            self.progress(stage, 0.0, None);
            match stage {
                ProcessingStage::Decode => self.decode(&mut cache)?,
                ProcessingStage::Vad => self.vad(&mut cache)?,
                ProcessingStage::Transcribe => self.transcribe(&mut cache)?,
                ProcessingStage::Voiceprints => self.voiceprints(&mut cache)?,
                ProcessingStage::Cluster => self.cluster(&mut cache)?,
                ProcessingStage::Merge => self.merge(&cache)?,
                ProcessingStage::Done => unreachable!(),
            }
            if stage != ProcessingStage::Merge {
                self.save(&cache)?;
            }
            self.record_stage(Some(stage))?;
            stage = next_stage(stage);
        }
        self.record_stage(Some(ProcessingStage::Done))?;
        if let Err(e) = sidecar::delete_cache(self.meetings_dir, id) {
            log::warn!("meeting {id}: {e:#}");
        }
        self.progress(ProcessingStage::Done, 1.0, None);
        Ok(())
    }

    fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Cancelled.into())
        } else {
            Ok(())
        }
    }

    fn progress(&mut self, stage: ProcessingStage, within: f32, message: Option<String>) {
        (self.on_progress)(ProcessingProgress {
            meeting_id: self.meeting_id,
            stage,
            fraction: overall_fraction(stage, within),
            message,
        });
    }

    /// Fail when the meeting was deleted mid-run, so a save point never
    /// recreates sidecars (voiceprints) for a meeting that no longer exists;
    /// and stop (as [`Cancelled`]) when it is no longer `Processing`: this
    /// run was cancelled and its status already restored, so its stage and
    /// checkpoint must stay as they were put back.
    fn ensure_still_processing(&self) -> Result<()> {
        match self.store.get_meeting(self.meeting_id)? {
            None => bail!("meeting {} was deleted", self.meeting_id),
            Some(m) if m.status != MeetingStatus::Processing => Err(Cancelled.into()),
            Some(_) => Ok(()),
        }
    }

    /// Record the last completed stage, only while the meeting is
    /// `Processing` (see [`Self::ensure_still_processing`]).
    fn record_stage(&self, stage: Option<ProcessingStage>) -> Result<()> {
        if !self
            .store
            .set_processing_stage_if_processing(self.meeting_id, stage)?
        {
            self.ensure_still_processing()?;
            bail!("could not record the processing stage");
        }
        Ok(())
    }

    /// Write the checkpoint. The check and the write hold the embeddings
    /// lock, which [`sidecar::delete_sidecars`] takes after the meeting row
    /// is deleted: a run either writes first (and the file is deleted) or
    /// finds the meeting gone.
    fn save(&self, cache: &ProcessingCache) -> Result<()> {
        let _lock = sidecar::lock_embeddings(self.meetings_dir, self.meeting_id);
        self.ensure_still_processing()?;
        sidecar::write_cache(self.meetings_dir, self.meeting_id, &self.key, cache)
    }

    fn source(&mut self, track: Track) -> Result<&mut (dyn AudioSource + 'a)> {
        let source = match track {
            Track::Mic => self.mic.as_deref_mut(),
            Track::System => self.system.as_deref_mut(),
        };
        source.ok_or_else(|| {
            anyhow::anyhow!(
                "the meeting's {} audio is missing (deleted by audio retention?)",
                track.as_str()
            )
        })
    }

    fn decode(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        cache.mic_ms = match self.mic.as_deref_mut() {
            Some(s) => Some(s.duration_ms()?),
            None => None,
        };
        cache.system_ms = match self.system.as_deref_mut() {
            Some(s) => Some(s.duration_ms()?),
            None => None,
        };
        if cache.mic_ms.unwrap_or(0) == 0 && cache.system_ms.unwrap_or(0) == 0 {
            bail!("this meeting has no recorded audio to process");
        }
        Ok(())
    }

    fn vad(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        let mut units = Vec::new();
        let tracks: Vec<Track> = [Track::Mic, Track::System]
            .into_iter()
            .filter(|t| match t {
                Track::Mic => cache.mic_ms.unwrap_or(0) > 0,
                Track::System => cache.system_ms.unwrap_or(0) > 0,
            })
            .collect();
        let n_tracks = tracks.len().max(1) as f32;
        for (k, track) in tracks.into_iter().enumerate() {
            let mut scorer = (self.vad)()?;
            let cancel = self.cancel;
            let frame_ms = vad::frame_ms(scorer.frame_samples());
            let mut reports = Vec::new();
            let probs = vad::score_track(scorer.as_mut(), self.source(track)?, cancel, |f| {
                reports.push(f)
            })?;
            if let Some(&f) = reports.last() {
                self.progress(ProcessingStage::Vad, (k as f32 + f) / n_tracks, None);
            }
            units.extend(
                vad::speech_spans(&probs, frame_ms, &self.config.vad)
                    .into_iter()
                    .map(|span| Unit { track, span }),
            );
        }
        cache.transcripts = vec![None; units.len()];
        cache.embeddings = vec![None; units.len()];
        cache.units = units;
        cache.clustering = None;
        Ok(())
    }

    fn transcribe(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        if let Some(limit_ms) = self.transcriber.batch_limit_ms() {
            self.transcribe_batched(cache, limit_ms)?;
            return self.unify_language(cache);
        }
        let n = cache.units.len();
        let mut last_save = Instant::now();
        for i in 0..n {
            if cache.transcripts[i].is_some() {
                continue;
            }
            if let Err(e) = self.check_cancel() {
                self.save(cache)?;
                return Err(e);
            }
            match self.transcribe_unit(cache.units[i], None) {
                Ok(t) => cache.transcripts[i] = Some(t),
                Err(e) => {
                    // Keep what is done so a retry resumes here.
                    self.save(cache)?;
                    return Err(e);
                }
            }
            let done = cache.transcripts.iter().filter(|t| t.is_some()).count();
            self.progress(
                ProcessingStage::Transcribe,
                done as f32 / n.max(1) as f32,
                None,
            );
            if last_save.elapsed() >= CHECKPOINT_EVERY {
                self.save(cache)?;
                last_save = Instant::now();
            }
        }
        self.unify_language(cache)
    }

    /// [`Self::transcribe`] for a batching transcriber: the pending units in
    /// groups (see [`batches`]), one call each, progress and checkpoints per
    /// group.
    fn transcribe_batched(&mut self, cache: &mut ProcessingCache, limit_ms: i64) -> Result<()> {
        let n = cache.units.len();
        let pending: Vec<bool> = cache.transcripts.iter().map(Option::is_none).collect();
        let mut last_save = Instant::now();
        for batch in batches(&cache.units, &pending, limit_ms) {
            if let Err(e) = self.check_cancel() {
                self.save(cache)?;
                return Err(e);
            }
            match self.transcribe_group(cache, &batch) {
                Ok(done) => {
                    for (i, t) in batch.into_iter().zip(done) {
                        cache.transcripts[i] = Some(t);
                    }
                }
                Err(e) => {
                    // Keep what is done so a retry resumes here.
                    self.save(cache)?;
                    return Err(e);
                }
            }
            let done = cache.transcripts.iter().filter(|t| t.is_some()).count();
            self.progress(
                ProcessingStage::Transcribe,
                done as f32 / n.max(1) as f32,
                None,
            );
            if last_save.elapsed() >= CHECKPOINT_EVERY {
                self.save(cache)?;
                last_save = Instant::now();
            }
        }
        Ok(())
    }

    /// The transcripts of the units `batch` (indices into `cache.units`),
    /// from one [`SegmentTranscriber::transcribe_batch`] call. Empty audio
    /// is not sent.
    fn transcribe_group(
        &mut self,
        cache: &ProcessingCache,
        batch: &[usize],
    ) -> Result<Vec<UnitTranscript>> {
        let mut pcms = Vec::with_capacity(batch.len());
        for &i in batch {
            let unit = cache.units[i];
            pcms.push(
                self.source(unit.track)?
                    .read_range(unit.span.t0_ms, unit.span.t1_ms)?,
            );
        }
        let sent: Vec<usize> = (0..pcms.len()).filter(|&k| !pcms[k].is_empty()).collect();
        let results = if sent.is_empty() {
            Vec::new()
        } else {
            let audio = sent.iter().map(|&k| std::mem::take(&mut pcms[k])).collect();
            self.transcriber.transcribe_batch(audio)?
        };
        if results.len() != sent.len() {
            bail!(
                "the transcriber returned {} result(s) for {} segment(s)",
                results.len(),
                sent.len()
            );
        }
        let mut by_unit: Vec<Option<(String, Vec<Word>)>> = vec![None; batch.len()];
        for (&k, r) in sent.iter().zip(results) {
            by_unit[k] = Some(r);
        }
        Ok(batch
            .iter()
            .zip(by_unit)
            .map(|(&i, r)| {
                let (text, words) = r.unwrap_or_default();
                UnitTranscript {
                    text: text.trim().to_string(),
                    words: offset_words(words, cache.units[i].span),
                }
            })
            .collect())
    }

    /// One unit's transcript, in `language` when given.
    fn transcribe_unit(&mut self, unit: Unit, language: Option<&str>) -> Result<UnitTranscript> {
        let pcm = self
            .source(unit.track)?
            .read_range(unit.span.t0_ms, unit.span.t1_ms)?;
        let (text, words) = if pcm.is_empty() {
            (String::new(), Vec::new())
        } else if let Some(language) = language {
            self.transcriber.transcribe_in(pcm, language)?
        } else {
            self.transcriber.transcribe(pcm)?
        };
        Ok(UnitTranscript {
            text: text.trim().to_string(),
            words: offset_words(words, unit.span),
        })
    }

    /// Autodetect picks a language per segment, and a short or accented one
    /// can come out in another language's script (Indian English written as
    /// Devanagari Hindi), which also hides its echo from the D9 net. Every
    /// segment detected in a language other than the meeting's main one is
    /// transcribed again in the main one.
    fn unify_language(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        let detected: Vec<(Option<String>, usize)> = cache
            .transcripts
            .iter()
            .map(|t| {
                let text = t.as_ref().map_or("", |t| t.text.as_str());
                (
                    self.transcriber.language_of(text),
                    text.split_whitespace().count(),
                )
            })
            .collect();
        let Some(main) = main_language(&detected, self.config.preferred_language.as_deref()) else {
            return Ok(());
        };
        let redo: Vec<usize> = detected
            .iter()
            .enumerate()
            .filter(|(_, (lang, _))| lang.as_deref().is_some_and(|l| l != main))
            .map(|(i, _)| i)
            .collect();
        if redo.is_empty() {
            return Ok(());
        }
        log::info!(
            "meeting {}: transcribing {} segment(s) again in the meeting's language ({main})",
            self.meeting_id,
            redo.len()
        );
        for i in redo {
            if let Err(e) = self.check_cancel() {
                self.save(cache)?;
                return Err(e);
            }
            match self.transcribe_unit(cache.units[i], Some(&main)) {
                Ok(t) => cache.transcripts[i] = Some(t),
                Err(e) => {
                    self.save(cache)?;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn voiceprints(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        let n = cache.units.len();
        // With the user's voice recorded, the mic track is diarized too.
        let embed_mic = !self.store.user_voiceprints()?.is_empty();
        let mut last_save = Instant::now();
        for i in 0..n {
            if cache.embeddings[i].is_some() {
                continue;
            }
            if let Err(e) = self.check_cancel() {
                self.save(cache)?;
                return Err(e);
            }
            let unit = cache.units[i];
            let has_speech = cache.transcripts[i]
                .as_ref()
                .is_some_and(|t| !t.text.is_empty());
            let wanted = (unit.track == Track::System || embed_mic)
                && has_speech
                && unit.span.len_ms() >= self.config.min_embed_ms;
            let embedding = if wanted {
                let pcm = self
                    .source(unit.track)?
                    .read_range(unit.span.t0_ms, unit.span.t1_ms)?;
                match self.embedder.embed(&pcm) {
                    Ok(e) => e.map(|e| cluster::normalized(&e)),
                    Err(e) => {
                        self.save(cache)?;
                        return Err(e);
                    }
                }
            } else {
                None
            };
            cache.embeddings[i] = Some(embedding);
            let done = cache.embeddings.iter().filter(|e| e.is_some()).count();
            self.progress(
                ProcessingStage::Voiceprints,
                done as f32 / n.max(1) as f32,
                None,
            );
            if last_save.elapsed() >= CHECKPOINT_EVERY {
                self.save(cache)?;
                last_save = Instant::now();
            }
        }
        Ok(())
    }

    fn cluster(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        cache.clustering = Some(cluster_units(
            cache,
            &self.config,
            &super::naming::known_voices(self.store)?,
            &self.store.user_voiceprints()?,
        ));
        Ok(())
    }

    /// Store the transcript and its per-segment voiceprints.
    ///
    /// The last cancel/deletion check is before the transcript is replaced:
    /// once it is committed the run completes the merge (the sidecar always
    /// matches the stored segments), and a failure after it drops the old
    /// sidecar, whose segment ids are gone.
    fn merge(&mut self, cache: &ProcessingCache) -> Result<()> {
        let clustering = cache.clustering.clone().unwrap_or_default();
        let built = build_transcript(cache, &clustering, &self.config);
        // From the check to the new transcript's voiceprints: a person
        // deletion dropping segments from this sidecar waits (and then sees
        // both), and a meeting deletion (row, then sidecars under this lock)
        // either stops the run here or deletes the file written.
        let _lock = sidecar::lock_embeddings(self.meetings_dir, self.meeting_id);
        self.ensure_still_processing()?;
        // The clustering's matches may name people deleted or merged away
        // since (also before a resume from the checkpoint): this resolves
        // them in its transaction (a deleted one leaves the speaker unknown),
        // so a stale id never fails the merge on every retry.
        let stored = self
            .store
            .replace_transcript(self.meeting_id, &built.0, &built.1)?;
        let written = self.store_voiceprints(cache, &clustering, &built, &stored);
        if written.is_err() {
            if let Err(e) = sidecar::delete_embeddings(self.meetings_dir, self.meeting_id) {
                log::warn!("meeting {}: {e:#}", self.meeting_id);
            }
        }
        written
    }

    /// [`Self::merge`] after the transcript is stored: speaker centroids and
    /// the embeddings sidecar, keyed by the stored ids. No cancel check.
    fn store_voiceprints(
        &self,
        cache: &ProcessingCache,
        clustering: &Clustering,
        (speakers, segments, unit_of_segment): &(Vec<MeetingSpeaker>, Vec<Segment>, Vec<usize>),
        (stored_speakers, stored_segments): &(Vec<MeetingSpeaker>, Vec<Segment>),
    ) -> Result<()> {
        // Cluster centroids on the stored speakers, for naming suggestions.
        for (local, stored) in speakers.iter().zip(stored_speakers) {
            if local.is_user {
                continue;
            }
            let cluster = cluster_of_speaker(local);
            let centroid = cluster.and_then(|c| clustering.centroids.get(c).cloned().flatten());
            self.store
                .set_speaker_embedding(stored.id, centroid.as_deref())?;
        }

        // Per-segment voiceprints, keyed by the stored segment ids.
        let mut by_time: HashMap<(Track, i64, i64), usize> = HashMap::new();
        for (seg, unit) in segments.iter().zip(unit_of_segment) {
            by_time.insert((seg.track, seg.t0_ms, seg.t1_ms), *unit);
        }
        let embeddings: Vec<SegmentEmbedding> = stored_segments
            .iter()
            .filter_map(|s| {
                let unit = *by_time.get(&(s.track, s.t0_ms, s.t1_ms))?;
                let embedding = cache.embeddings.get(unit)?.clone()??;
                Some(SegmentEmbedding {
                    segment_id: s.id,
                    duration_ms: s.t1_ms - s.t0_ms,
                    embedding,
                })
            })
            .collect();
        sidecar::write_embeddings(self.meetings_dir, self.meeting_id, &self.key, &embeddings)
    }
}

/// [`segment_language`] of a segment written in more than one script.
pub const MIXED_LANGUAGE: &str = "mul";

/// The language of one segment's text, among the model's `languages`:
/// [`MIXED_LANGUAGE`] when its words are in more than one script (an
/// accented English segment partly written as Devanagari), else the
/// confident text detection, if any.
pub fn segment_language(text: &str, languages: &[String]) -> Option<String> {
    use whatlang::Script;
    // Japanese mixes kana and kanji; one script as far as this goes.
    let group = |s: Script| match s {
        Script::Hiragana | Script::Katakana | Script::Mandarin | Script::Hangul => Script::Mandarin,
        s => s,
    };
    let mut scripts = text.split_whitespace().filter_map(|w| {
        let letters: String = w.chars().filter(|c| c.is_alphabetic()).collect();
        whatlang::detect_script(&letters).map(group)
    });
    let first = scripts.next()?;
    if scripts.any(|s| s != first) {
        return Some(MIXED_LANGUAGE.to_string());
    }
    crate::audio_toolkit::detect_output_language(text, languages)
}

/// Fewest words in a detected language before a meeting has a main one.
const MAIN_LANGUAGE_MIN_WORDS: usize = 20;
/// Share of the detected words the main language must hold.
const MAIN_LANGUAGE_MIN_SHARE: f32 = 0.6;
/// Share the preferred language needs to be the main one when none leads.
const PREFERRED_LANGUAGE_MIN_SHARE: f32 = 1.0 / 3.0;

/// The language most of a meeting's words are in, given each segment's
/// detected language and word count. Without a clear leader (echoes of
/// misdetected speech count twice), `preferred` when it holds a fair share.
/// `None` when too little was detected or neither applies.
pub fn main_language(
    segments: &[(Option<String>, usize)],
    preferred: Option<&str>,
) -> Option<String> {
    let mut words: HashMap<&str, usize> = HashMap::new();
    for (lang, n) in segments {
        if let Some(lang) = lang.as_deref().filter(|&l| l != MIXED_LANGUAGE) {
            *words.entry(lang).or_default() += n;
        }
    }
    let total: usize = words.values().sum();
    if total < MAIN_LANGUAGE_MIN_WORDS {
        return None;
    }
    let share = |n: usize| n as f32 / total as f32;
    let (lang, n) = words
        .iter()
        .max_by_key(|&(l, n)| (*n, std::cmp::Reverse(*l)))?;
    if share(*n) >= MAIN_LANGUAGE_MIN_SHARE {
        return Some(lang.to_string());
    }
    let preferred = preferred?.split(['-', '_']).next()?.to_ascii_lowercase();
    let n = words.get(preferred.as_str()).copied().unwrap_or(0);
    (share(n) >= PREFERRED_LANGUAGE_MIN_SHARE).then_some(preferred)
}

/// Groups of units (indices) for a batching transcriber: the `pending`
/// ones, in unit order, each group on one track and holding at most
/// `limit_ms` of speech (a longer unit is a group of its own).
pub fn batches(units: &[Unit], pending: &[bool], limit_ms: i64) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_ms = 0;
    for (i, unit) in units.iter().enumerate() {
        if !pending.get(i).copied().unwrap_or(false) {
            continue;
        }
        let len = unit.span.len_ms();
        let same_track = current
            .first()
            .is_some_and(|&j| units[j].track == unit.track);
        if !current.is_empty() && (!same_track || current_ms + len > limit_ms) {
            out.push(std::mem::take(&mut current));
            current_ms = 0;
        }
        current.push(i);
        current_ms += len;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Shift words from segment time to meeting time, clamped to the segment.
fn offset_words(words: Vec<Word>, span: vad::Span) -> Vec<Word> {
    words
        .into_iter()
        .filter_map(|w| {
            let text = w.text.trim().to_string();
            if text.is_empty() {
                return None;
            }
            let t0 = (span.t0_ms + w.t0_ms.max(0)).min(span.t1_ms);
            let t1 = (span.t0_ms + w.t1_ms.max(w.t0_ms).max(0)).min(span.t1_ms);
            Some(Word {
                t0_ms: t0,
                t1_ms: t1.max(t0),
                text,
            })
        })
        .collect()
}

/// Mic units with speech that isn't the user's: embedded ones that don't
/// sound like `user_voice`. Without a recorded voice (or a voiceprint for
/// the unit, e.g. a short "yeah") mic speech stays the user's.
fn others_on_mic(
    cache: &ProcessingCache,
    config: &PipelineConfig,
    user_voice: &[Vec<f32>],
) -> Vec<bool> {
    (0..cache.units.len())
        .map(|i| {
            cache.units[i].track == Track::Mic
                && !user_voice.is_empty()
                && cache.transcripts[i]
                    .as_ref()
                    .is_some_and(|t| !t.text.is_empty())
                && cache.embeddings[i]
                    .as_ref()
                    .and_then(Option::as_deref)
                    .and_then(|e| cluster::person_similarity(e, user_voice))
                    .is_some_and(|s| s < config.user_voice_threshold)
        })
        .collect()
}

/// Cluster the units that have speech (system, plus mic speech that isn't
/// the user's: see [`others_on_mic`]) and match the clusters to known
/// people. Mic units keep `None` (the user) unless they are in a cluster.
pub fn cluster_units(
    cache: &ProcessingCache,
    config: &PipelineConfig,
    known: &[KnownVoice],
    user_voice: &[Vec<f32>],
) -> Clustering {
    let n = cache.units.len();
    let on_mic = others_on_mic(cache, config, user_voice);
    let speech: Vec<usize> = (0..n)
        .filter(|&i| {
            (cache.units[i].track == Track::System || on_mic[i])
                && cache.transcripts[i]
                    .as_ref()
                    .is_some_and(|t| !t.text.is_empty())
        })
        .collect();
    let embedded: Vec<usize> = speech
        .iter()
        .copied()
        .filter(|&i| matches!(cache.embeddings.get(i), Some(Some(Some(_)))))
        .collect();
    let vectors: Vec<Vec<f32>> = embedded
        .iter()
        .map(|&i| cache.embeddings[i].clone().flatten().expect("embedded"))
        .collect();
    let (labels_embedded, k_embedded) = cluster::agglomerate(&vectors, &config.cluster);

    let mut labels: Vec<Option<usize>> = vec![None; n];
    for (&i, &l) in embedded.iter().zip(&labels_embedded) {
        labels[i] = Some(l);
    }
    // Short or un-embeddable system speech joins the nearest system cluster
    // in time; with no system voiceprints at all, it is one speaker. (Other
    // people's mic speech is all embedded.)
    let system: Vec<usize> = speech
        .iter()
        .copied()
        .filter(|&i| cache.units[i].track == Track::System)
        .collect();
    let spans: Vec<(i64, i64)> = system
        .iter()
        .map(|&i| (cache.units[i].span.t0_ms, cache.units[i].span.t1_ms))
        .collect();
    let system_labels: Vec<Option<usize>> = system.iter().map(|&i| labels[i]).collect();
    let attached = cluster::attach_by_time(&spans, &system_labels);
    for (&i, l) in system.iter().zip(attached) {
        labels[i] = Some(l.unwrap_or(k_embedded));
    }

    // A cluster of mic speech only that still sounds like the user (as a
    // whole, segments are noisier) is the user after all.
    if !user_voice.is_empty() {
        let mut raw: Vec<usize> = labels.iter().flatten().copied().collect();
        raw.sort_unstable();
        raw.dedup();
        for l in raw {
            let members: Vec<usize> = (0..n).filter(|&i| labels[i] == Some(l)).collect();
            if members.iter().any(|&i| cache.units[i].track != Track::Mic) {
                continue;
            }
            let c = cluster::centroid(
                members
                    .iter()
                    .filter_map(|&i| cache.embeddings[i].as_ref()?.as_deref()),
            );
            let is_user = c
                .and_then(|c| cluster::person_similarity(&c, user_voice))
                .is_some_and(|s| s >= config.user_voice_threshold);
            if is_user {
                for i in members {
                    labels[i] = None;
                }
            }
        }
    }

    // Renumber clusters by first appearance in time.
    let mut order: Vec<usize> = Vec::new();
    let mut by_time: Vec<usize> = (0..n).collect();
    by_time.sort_by_key(|&i| (cache.units[i].span.t0_ms, i));
    for &i in &by_time {
        if let Some(l) = labels[i] {
            if !order.contains(&l) {
                order.push(l);
            }
        }
    }
    let renumber: HashMap<usize, usize> = order
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    for l in labels.iter_mut().flatten() {
        *l = renumber[l];
    }
    let k = order.len();

    let centroids: Vec<Option<Vec<f32>>> = (0..k)
        .map(|c| {
            cluster::centroid(
                (0..n)
                    .filter(|&i| labels[i] == Some(c))
                    .filter_map(|i| cache.embeddings[i].as_ref()?.as_deref()),
            )
        })
        .collect();
    let matches = cluster::match_clusters(
        &centroids,
        known,
        config.match_threshold,
        config.suggest_threshold,
    );
    Clustering {
        labels,
        centroids,
        matches,
    }
}

/// Local speaker id of the mic user, and of cluster `c`.
const USER_LOCAL_ID: SpeakerId = 0;
fn cluster_local_id(c: usize) -> SpeakerId {
    c as SpeakerId + 1
}
fn cluster_of_speaker(s: &MeetingSpeaker) -> Option<usize> {
    (!s.is_user && s.id >= 1).then(|| (s.id - 1) as usize)
}

/// The speakers and timeline segments to store (with local speaker ids), and
/// for each segment the unit it came from.
pub fn build_transcript(
    cache: &ProcessingCache,
    clustering: &Clustering,
    config: &PipelineConfig,
) -> (Vec<MeetingSpeaker>, Vec<Segment>, Vec<usize>) {
    let n = cache.units.len();
    let mut mic: Vec<(usize, RawSegment)> = Vec::new();
    let mut system: Vec<(usize, RawSegment)> = Vec::new();
    for i in 0..n {
        if let Some(seg) = cache.raw_segment(i) {
            match seg.track {
                Track::Mic => mic.push((i, seg)),
                Track::System => system.push((i, seg)),
            }
        }
    }
    let mic_segs: Vec<RawSegment> = mic.iter().map(|(_, s)| s.clone()).collect();
    let sys_segs: Vec<RawSegment> = system.iter().map(|(_, s)| s.clone()).collect();
    let speakers_of = |units: &[(usize, RawSegment)]| -> Vec<Option<usize>> {
        units
            .iter()
            .map(|(i, _)| clustering.labels.get(*i).copied().flatten())
            .collect()
    };
    let (mic_speakers, sys_speakers) = (speakers_of(&mic), speakers_of(&system));
    // `merge_timeline` takes speaker indices; map the user to a sentinel
    // past every cluster.
    let user_index = usize::MAX;
    let timeline = timeline::merge_timeline(
        &mic_segs,
        &sys_segs,
        &mic_speakers,
        &sys_speakers,
        user_index,
        &config.echo,
    );

    let unit_of = |seg: &RawSegment| -> usize {
        let list = if seg.track == Track::Mic {
            &mic
        } else {
            &system
        };
        list.iter()
            .find(|(_, s)| s.t0_ms == seg.t0_ms && s.t1_ms == seg.t1_ms)
            .map(|(i, _)| *i)
            .unwrap_or(usize::MAX)
    };

    let uses_user = timeline.iter().any(|e| e.speaker == Some(user_index));
    let mut speakers = Vec::new();
    if uses_user {
        speakers.push(MeetingSpeaker {
            id: USER_LOCAL_ID,
            meeting_id: 0,
            cluster_label: config.user_label.clone(),
            person_id: None,
            match_confidence: None,
            confirmed: true,
            is_user: true,
        });
    }
    let used_clusters: std::collections::BTreeSet<usize> = timeline
        .iter()
        .filter_map(|e| e.speaker)
        .filter(|&s| s != user_index)
        .collect();
    for (number, &c) in used_clusters.iter().enumerate() {
        let m = clustering.matches.get(c).copied().unwrap_or_default();
        speakers.push(MeetingSpeaker {
            id: cluster_local_id(c),
            meeting_id: 0,
            cluster_label: format!("Speaker {}", number + 1),
            person_id: m.person_id,
            match_confidence: m.confidence,
            confirmed: m.person_id.is_some() && m.confirmed,
            is_user: false,
        });
    }

    let mut segments = Vec::with_capacity(timeline.len());
    let mut units = Vec::with_capacity(timeline.len());
    for e in timeline {
        let speaker_id = match e.speaker {
            Some(s) if s == user_index => Some(USER_LOCAL_ID),
            Some(c) => Some(cluster_local_id(c)),
            None => None,
        };
        units.push(unit_of(&e.segment));
        segments.push(Segment {
            id: 0,
            meeting_id: 0,
            track: e.segment.track,
            t0_ms: e.segment.t0_ms,
            t1_ms: e.segment.t1_ms,
            text: e.segment.text,
            words: e.segment.words,
            speaker_id,
        });
    }
    (speakers, segments, units)
}

#[cfg(test)]
mod tests;
