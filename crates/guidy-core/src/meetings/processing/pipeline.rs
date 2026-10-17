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
    /// Label of the mic speaker (the user).
    pub user_label: String,
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
            user_label: if name.is_empty() {
                "me".into()
            } else {
                name.into()
            },
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
    /// the completed stages stay recorded either way.
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
            self.store.set_processing_stage(id, None)?;
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
            self.store.set_processing_stage(id, Some(stage))?;
            stage = next_stage(stage);
        }
        self.store
            .set_processing_stage(id, Some(ProcessingStage::Done))?;
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
    /// recreates sidecars (voiceprints) for a meeting that no longer exists.
    fn ensure_meeting_exists(&self) -> Result<()> {
        if self.store.get_meeting(self.meeting_id)?.is_none() {
            bail!("meeting {} was deleted", self.meeting_id);
        }
        Ok(())
    }

    fn save(&self, cache: &ProcessingCache) -> Result<()> {
        self.ensure_meeting_exists()?;
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
            let unit = cache.units[i];
            let pcm = self
                .source(unit.track)?
                .read_range(unit.span.t0_ms, unit.span.t1_ms)?;
            let result = if pcm.is_empty() {
                Ok((String::new(), Vec::new()))
            } else {
                self.transcriber.transcribe(pcm)
            };
            let (text, words) = match result {
                Ok(r) => r,
                Err(e) => {
                    // Keep what is done so a retry resumes here.
                    self.save(cache)?;
                    return Err(e);
                }
            };
            cache.transcripts[i] = Some(UnitTranscript {
                text: text.trim().to_string(),
                words: offset_words(words, unit.span),
            });
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

    fn voiceprints(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        let n = cache.units.len();
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
            let wanted = unit.track == Track::System
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

    fn known_voices(&self) -> Result<Vec<KnownVoice>> {
        let mut by_person: HashMap<PersonId, Vec<Vec<f32>>> = HashMap::new();
        for vp in self.store.voiceprints()? {
            by_person
                .entry(vp.person_id)
                .or_default()
                .push(vp.embedding);
        }
        let mut voices: Vec<KnownVoice> = by_person
            .into_iter()
            .map(|(person_id, prints)| KnownVoice { person_id, prints })
            .collect();
        voices.sort_by_key(|v| v.person_id);
        Ok(voices)
    }

    fn cluster(&mut self, cache: &mut ProcessingCache) -> Result<()> {
        cache.clustering = Some(cluster_units(cache, &self.config, &self.known_voices()?));
        Ok(())
    }

    fn merge(&mut self, cache: &ProcessingCache) -> Result<()> {
        let clustering = cache.clustering.clone().unwrap_or_default();
        let (speakers, segments, unit_of_segment) =
            build_transcript(cache, &clustering, &self.config);
        let (stored_speakers, stored_segments) =
            self.store
                .replace_transcript(self.meeting_id, &speakers, &segments)?;

        // Cluster centroids on the stored speakers, for naming suggestions.
        for (local, stored) in speakers.iter().zip(&stored_speakers) {
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
        for (seg, unit) in segments.iter().zip(&unit_of_segment) {
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
        self.ensure_meeting_exists()?;
        sidecar::write_embeddings(self.meetings_dir, self.meeting_id, &self.key, &embeddings)?;
        Ok(())
    }
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

/// Cluster the system units that have speech and match the clusters to
/// known people.
pub fn cluster_units(
    cache: &ProcessingCache,
    config: &PipelineConfig,
    known: &[KnownVoice],
) -> Clustering {
    let n = cache.units.len();
    let speech: Vec<usize> = (0..n)
        .filter(|&i| {
            cache.units[i].track == Track::System
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
    let (labels_embedded, _) = cluster::agglomerate(&vectors, &config.cluster);

    let mut labels: Vec<Option<usize>> = vec![None; n];
    for (&i, &l) in embedded.iter().zip(&labels_embedded) {
        labels[i] = Some(l);
    }
    // Short or un-embeddable speech joins the nearest cluster in time; with
    // no voiceprints at all, the system track is one speaker.
    let spans: Vec<(i64, i64)> = speech
        .iter()
        .map(|&i| (cache.units[i].span.t0_ms, cache.units[i].span.t1_ms))
        .collect();
    let speech_labels: Vec<Option<usize>> = speech.iter().map(|&i| labels[i]).collect();
    let attached = cluster::attach_by_time(&spans, &speech_labels);
    for (&i, l) in speech.iter().zip(attached) {
        labels[i] = Some(l.unwrap_or(0));
    }

    // Renumber clusters by first appearance in time.
    let mut order: Vec<usize> = Vec::new();
    for &l in labels.iter().flatten() {
        if !order.contains(&l) {
            order.push(l);
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
    let sys_speakers: Vec<Option<usize>> = system
        .iter()
        .map(|(i, _)| clustering.labels.get(*i).copied().flatten())
        .collect();
    // `merge_timeline` takes speaker indices; map the user to a sentinel
    // past every cluster.
    let user_index = usize::MAX;
    let timeline = timeline::merge_timeline(
        &mic_segs,
        &sys_segs,
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
