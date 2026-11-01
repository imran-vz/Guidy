//! Diarization evaluation: how well the speakers come out on labelled meetings
//! of your own, for choosing the defaults by measurement. Run it through
//! `cargo xtask diar-eval <manifest.json>`. No audio or RTTM is committed.
//!
//! ```json
//! { "meetings": [ { "name": "standup", "system": "standup-system.wav",
//!                   "mic": "standup-mic.wav", "ref": "standup.rttm",
//!                   "you": "imran" } ] }
//! ```
//!
//! Paths are relative to the manifest. Audio is 16 kHz mono 16-bit WAV. `mic`
//! and `you` (the user's name in the RTTM) are optional; with both, the user's
//! voice is enrolled from their three longest turns on the mic.
//!
//! The grid is {ResNet34, TitaNet} x {turns on, off} x {old, new clustering} x
//! {current, earlier short-turn tunables} x cut values. Nothing is transcribed, so every line counts as speech and text
//! echo detection doesn't run: mic bleed shows up as false alarm. "Old
//! clustering" is the previous cut rule without the two tiers; the tracks stay
//! apart, so a baseline of the old mixed tracks needs a pre-change commit.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::cluster::{self, ClusterParams};
use super::pipeline::{cluster_units_with, ClusterSource, Embedder, PipelineConfig};
use super::sidecar::{ProcessingCache, Unit, UnitTranscript};
use super::speaker_model::{PyannoteSegmenter, SpeakerEmbedder};
use super::turns::{self, TurnParams, TurnSegmenter};
use super::vad::{self, Span, VadParams};
use super::{PcmSource, SAMPLE_RATE};
use crate::audio_toolkit::SpeechScorer;
use crate::meetings::types::Track;

/// Scoring resolution.
const FRAME_MS: i64 = 10;
/// Reference boundaries this close (each way) aren't scored.
const COLLAR_MS: i64 = 250;
/// No meeting runs longer than this: a later RTTM time is a mistake.
const MAX_RTTM_SECONDS: f64 = 86_400.0;
/// A hypothesis speaker's name for the user's mic lines.
const YOU: &str = "you";
const CUTS: [f32; 3] = [0.4, 0.5, 0.6];

/// One speaker's stretch; `speaker: None` is speech left to an unknown speaker.
#[derive(Debug, Clone, PartialEq)]
struct Turn {
    t0_ms: i64,
    t1_ms: i64,
    speaker: Option<String>,
}

/// The reference turns of an RTTM file. Turns of no length are left out; they
/// would count as a speaker and mask speech with their collar.
fn parse_rttm(text: &str) -> Result<Vec<Turn>> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"SPEAKER") {
            continue;
        }
        let parsed = (|| {
            let start: f64 = fields.get(3)?.parse().ok()?;
            let duration: f64 = fields.get(4)?.parse().ok()?;
            let name = fields.get(7)?;
            Some((start, duration, name.to_string()))
        })();
        let Some((start, duration, name)) = parsed else {
            bail!("RTTM line {}: {line}", n + 1);
        };
        if !(start >= 0.0 && duration >= 0.0 && start + duration <= MAX_RTTM_SECONDS) {
            bail!("RTTM line {}: times out of range: {line}", n + 1);
        }
        if (duration * 1000.0).round() == 0.0 {
            continue;
        }
        let t0_ms = (start * 1000.0).round() as i64;
        out.push(Turn {
            t0_ms,
            t1_ms: t0_ms + (duration * 1000.0).round() as i64,
            speaker: Some(name),
        });
    }
    Ok(out)
}

/// The best assignment of rows to columns of `weights` (maximum total weight,
/// each used once): per row, its column, where it has any weight.
fn best_assignment(weights: &[Vec<u64>], cols: usize) -> Vec<Option<usize>> {
    let rows = weights.len();
    let n = rows.max(cols);
    if n == 0 {
        return Vec::new();
    }
    let top = weights.iter().flatten().copied().max().unwrap_or(0) as i64;
    let cost = |i: usize, j: usize| -> i64 {
        top - weights.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0) as i64
    };
    // The Hungarian algorithm, with potentials; indices from 1, 0 is a sentinel.
    let mut u = vec![0i64; n + 1];
    let mut v = vec![0i64; n + 1];
    let mut owner = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for i in 1..=n {
        owner[0] = i;
        let mut j0 = 0;
        let mut min_v = vec![i64::MAX; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = owner[j0];
            let mut delta = i64::MAX;
            let mut j1 = 0;
            for j in 1..=n {
                if !used[j] {
                    let reduced = cost(i0 - 1, j - 1) - u[i0] - v[j];
                    if reduced < min_v[j] {
                        min_v[j] = reduced;
                        way[j] = j0;
                    }
                    if min_v[j] < delta {
                        delta = min_v[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[owner[j]] += delta;
                    v[j] -= delta;
                } else {
                    min_v[j] -= delta;
                }
            }
            j0 = j1;
            if owner[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            owner[j0] = owner[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = vec![None; rows];
    for (j, &row_owner) in owner.iter().enumerate().skip(1) {
        let (row, col) = (row_owner - 1, j - 1);
        if row < rows && col < cols && weights[row][col] > 0 {
            out[row] = Some(col);
        }
    }
    out
}

/// How a hypothesis scores against a reference.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Score {
    /// Diarization error rate: `missed + false_alarm + confusion`, as shares
    /// of the scored reference speech.
    der: f64,
    missed: f64,
    false_alarm: f64,
    confusion: f64,
    /// How many speakers too many or too few.
    speaker_count_error: usize,
    /// How much of each hypothesis speaker is one reference speaker.
    purity: f64,
    /// How much of each reference speaker is one hypothesis speaker.
    coverage: f64,
    /// The share of hypothesis speech left to an unknown speaker.
    unknown_share: f64,
}

fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn frames_of(t: &Turn) -> std::ops::Range<usize> {
    let at = |ms: i64| (ms.max(0) / FRAME_MS) as usize;
    at(t.t0_ms)..at(t.t1_ms)
}

/// The names in `turns`, sorted, and each one's index.
fn names(turns: &[Turn]) -> BTreeMap<&str, usize> {
    let set: BTreeSet<&str> = turns.iter().filter_map(|t| t.speaker.as_deref()).collect();
    set.into_iter().enumerate().map(|(i, s)| (s, i)).collect()
}

/// Per frame, the indices of the named speakers talking.
fn speakers_by_frame(turns: &[Turn], ids: &BTreeMap<&str, usize>, len: usize) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); len];
    for t in turns {
        let Some(id) = t.speaker.as_deref().and_then(|s| ids.get(s)) else {
            continue;
        };
        for f in frames_of(t) {
            if !out[f].contains(id) {
                out[f].push(*id);
            }
        }
    }
    out
}

/// Score `hypothesis` against `reference`, without `collar_ms` each way around
/// the reference's boundaries, with hypothesis speakers mapped to reference
/// ones for the most overlap. Unknown speech never matches anyone.
fn score(reference: &[Turn], hypothesis: &[Turn], collar_ms: i64) -> Score {
    let len = reference
        .iter()
        .chain(hypothesis)
        .map(|t| frames_of(t).end)
        .max()
        .unwrap_or(0);
    let (ref_ids, hyp_ids) = (names(reference), names(hypothesis));
    let ref_at = speakers_by_frame(reference, &ref_ids, len);
    let hyp_at = speakers_by_frame(hypothesis, &hyp_ids, len);
    let mut unknown_at = vec![false; len];
    for t in hypothesis.iter().filter(|t| t.speaker.is_none()) {
        for f in frames_of(t) {
            unknown_at[f] = true;
        }
    }
    let mut scored = vec![true; len];
    let collar = |ms: i64| Turn {
        t0_ms: ms - collar_ms,
        t1_ms: ms + collar_ms,
        speaker: None,
    };
    for t in reference {
        for boundary in [t.t0_ms, t.t1_ms] {
            for f in frames_of(&collar(boundary)) {
                if f < len {
                    scored[f] = false;
                }
            }
        }
    }

    let mut overlap = vec![vec![0u64; ref_ids.len()]; hyp_ids.len()];
    for f in (0..len).filter(|&f| scored[f]) {
        for &h in &hyp_at[f] {
            for &r in &ref_at[f] {
                overlap[h][r] += 1;
            }
        }
    }
    let mapped = best_assignment(&overlap, ref_ids.len());

    let (mut total, mut missed, mut false_alarm, mut confusion) = (0u64, 0u64, 0u64, 0u64);
    let (mut hyp_speech, mut unknown, mut hyp_on_ref) = (0u64, 0u64, 0u64);
    let mut ref_frames = vec![0u64; ref_ids.len()];
    for f in (0..len).filter(|&f| scored[f]) {
        let unknown_here = u64::from(unknown_at[f]);
        let n_ref = ref_at[f].len() as u64;
        let n_hyp = hyp_at[f].len() as u64 + unknown_here;
        let correct = hyp_at[f]
            .iter()
            .filter(|&&h| mapped[h].is_some_and(|r| ref_at[f].contains(&r)))
            .count() as u64;
        total += n_ref;
        missed += n_ref.saturating_sub(n_hyp);
        false_alarm += n_hyp.saturating_sub(n_ref);
        confusion += n_ref.min(n_hyp) - correct;
        hyp_speech += n_hyp;
        unknown += unknown_here;
        if !ref_at[f].is_empty() {
            hyp_on_ref += hyp_at[f].len() as u64;
        }
        for &r in &ref_at[f] {
            ref_frames[r] += 1;
        }
    }
    let best_of_each_hyp: u64 = overlap
        .iter()
        .map(|row| row.iter().copied().max().unwrap_or(0))
        .sum();
    let best_of_each_ref: u64 = (0..ref_ids.len())
        .map(|r| overlap.iter().map(|row| row[r]).max().unwrap_or(0))
        .sum();
    // Speech where the reference scores none is all wrong, not perfect.
    let whole = if total == 0 { false_alarm } else { total };
    Score {
        der: share(missed + false_alarm + confusion, whole),
        missed: share(missed, whole),
        false_alarm: share(false_alarm, whole),
        confusion: share(confusion, whole),
        speaker_count_error: ref_ids.len().abs_diff(hyp_ids.len()),
        purity: share(best_of_each_hyp, hyp_on_ref),
        coverage: share(best_of_each_ref, ref_frames.iter().sum()),
        unknown_share: share(unknown, hyp_speech),
    }
}

/// `(precision, recall)` of the lines called [`YOU`], against the reference
/// turns of the user named `you`.
fn you_precision_recall(reference: &[Turn], hypothesis: &[Turn], you: &str) -> (f64, f64) {
    let frames = |turns: &[Turn], name: &str| -> BTreeSet<usize> {
        turns
            .iter()
            .filter(|t| t.speaker.as_deref() == Some(name))
            .flat_map(frames_of)
            .collect()
    };
    let (called, truly) = (frames(hypothesis, YOU), frames(reference, you));
    let hit = called.intersection(&truly).count() as u64;
    (
        share(hit, called.len() as u64),
        share(hit, truly.len() as u64),
    )
}

#[derive(Deserialize)]
struct Manifest {
    meetings: Vec<ManifestMeeting>,
}

#[derive(Deserialize)]
struct ManifestMeeting {
    name: String,
    system: PathBuf,
    mic: Option<PathBuf>,
    #[serde(rename = "ref")]
    reference: PathBuf,
    you: Option<String>,
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let spec = hound::WavReader::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .spec();
    if spec.sample_rate as usize != SAMPLE_RATE || spec.channels != 1 {
        bail!("{}: needs 16 kHz mono audio", path.display());
    }
    crate::audio_toolkit::read_wav_samples(path)
}

/// `[t0_ms, t1_ms)` of `pcm`, clipped.
fn slice(pcm: &[f32], t0_ms: i64, t1_ms: i64) -> &[f32] {
    let at = |ms: i64| ((ms.max(0) as usize) * SAMPLE_RATE / 1000).min(pcm.len());
    let (a, b) = (at(t0_ms), at(t1_ms));
    &pcm[a..b.max(a)]
}

/// One track's VAD result: its speech, and what cuts snap to.
struct Speech {
    spans: Vec<Span>,
    probs: Vec<f32>,
    frame_ms: f64,
}

fn detect_speech(pcm: &[f32], vad_model: &Path) -> Result<Speech> {
    let mut scorer: Box<dyn SpeechScorer> =
        Box::new(crate::audio_toolkit::SileroVad::new(vad_model, 0.3)?);
    let frame_ms = vad::frame_ms(scorer.frame_samples());
    let mut source = PcmSource::new(pcm.to_vec());
    let cancel = AtomicBool::new(false);
    let probs = vad::score_track(scorer.as_mut(), &mut source, &cancel, |_| {})?;
    let spans = vad::speech_spans(&probs, frame_ms, &VadParams::default());
    Ok(Speech {
        spans,
        probs,
        frame_ms,
    })
}

struct Voices<'a>(&'a SpeakerEmbedder);

impl Embedder for Voices<'_> {
    fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
        self.0.embed(pcm)
    }
}

/// The turn finding and voiceprint floor for a row: the defaults, or the values
/// from before the short-turn trial, to compare them.
fn short_turn_tunables(short_turns: bool) -> (TurnParams, i64) {
    if short_turns {
        (
            TurnParams::default(),
            PipelineConfig::default().min_embed_ms,
        )
    } else {
        let earlier = TurnParams {
            scan_window_ms: 1_500,
            scan_hop_ms: 500,
            min_side_ms: 1_500,
            ..TurnParams::default()
        };
        (earlier, 1_500)
    }
}

/// `speech` cut where the voice changes.
fn split_at_turns(
    pcm: &[f32],
    speech: &Speech,
    titanet: &SpeakerEmbedder,
    segmenter: &mut Option<PyannoteSegmenter>,
    params: &TurnParams,
) -> Result<Vec<Span>> {
    let mut out = Vec::new();
    for &span in &speech.spans {
        if span.len_ms() < params.min_span_ms {
            out.push(span);
            continue;
        }
        let changes = turns::find_changes(
            slice(pcm, span.t0_ms, span.t1_ms),
            segmenter.as_mut().map(|s| s as &mut dyn TurnSegmenter),
            &mut Voices(titanet),
            params,
        )?;
        out.extend(turns::split_span(
            span,
            &changes,
            &speech.probs,
            speech.frame_ms,
            params,
        ));
    }
    Ok(out)
}

/// Voiceprints by (model, track, span): a line's is the same in every row of
/// the grid.
#[derive(Default)]
struct VoiceCache(HashMap<(&'static str, Track, i64, i64), Option<Vec<f32>>>);

impl VoiceCache {
    fn get(
        &mut self,
        model_name: &'static str,
        model: &SpeakerEmbedder,
        track: Track,
        pcm: &[f32],
        span: Span,
    ) -> Result<Option<Vec<f32>>> {
        let key = (model_name, track, span.t0_ms, span.t1_ms);
        if let Some(v) = self.0.get(&key) {
            return Ok(v.clone());
        }
        let voice = model
            .embed(slice(pcm, span.t0_ms, span.t1_ms))?
            .map(|v| cluster::normalized(&v));
        self.0.insert(key, voice.clone());
        Ok(voice)
    }
}

/// One row of the grid.
#[derive(Clone, Copy)]
struct Row {
    /// TitaNet tells the voices apart, not the standard model.
    titanet: bool,
    turns: bool,
    legacy: bool,
    /// The current short-turn tunables, else the earlier values.
    short_turns: bool,
    cut: f32,
}

impl Row {
    fn label(&self) -> String {
        format!(
            "{:<8} turns={:<3} clustering={:<3} tunables={:<7} cut={:.2}",
            if self.titanet { "titanet" } else { "resnet34" },
            if self.turns { "on" } else { "off" },
            if self.legacy { "old" } else { "new" },
            if self.short_turns {
                "current"
            } else {
                "earlier"
            },
            self.cut
        )
    }
}

struct Models {
    resnet: SpeakerEmbedder,
    titanet: Option<SpeakerEmbedder>,
    segmenter: Option<PyannoteSegmenter>,
    vad: PathBuf,
}

/// One meeting's audio and reference, ready for the grid.
struct Loaded {
    system: Vec<f32>,
    mic: Option<Vec<f32>>,
    system_speech: Speech,
    /// `system_speech` cut at changes of voice (with TitaNet), with the earlier
    /// and the current short-turn tunables.
    system_turns: Option<[Vec<Span>; 2]>,
    mic_speech: Vec<Span>,
    reference: Vec<Turn>,
    you: Option<String>,
    user_voice: Vec<Vec<f32>>,
    voices: VoiceCache,
}

fn load(base: &Path, m: &ManifestMeeting, models: &mut Models) -> Result<Loaded> {
    let system = read_wav(&base.join(&m.system))?;
    let mic = m
        .mic
        .as_ref()
        .map(|p| read_wav(&base.join(p)))
        .transpose()?;
    let text = std::fs::read_to_string(base.join(&m.reference))
        .with_context(|| format!("read {}", m.reference.display()))?;
    let reference = parse_rttm(&text)?;
    if reference.is_empty() {
        bail!("{}: no speaker turns", m.reference.display());
    }
    let system_speech = detect_speech(&system, &models.vad)?;
    let system_turns = match &models.titanet {
        Some(t) => {
            let mut split = |short_turns: bool| {
                split_at_turns(
                    &system,
                    &system_speech,
                    t,
                    &mut models.segmenter,
                    &short_turn_tunables(short_turns).0,
                )
            };
            Some([split(false)?, split(true)?])
        }
        None => None,
    };
    let mic_speech = match &mic {
        Some(pcm) => detect_speech(pcm, &models.vad)?.spans,
        None => Vec::new(),
    };
    // The user's voice, as a recorded sample would give it.
    let mut user_voice = Vec::new();
    if let (Some(pcm), Some(you)) = (&mic, &m.you) {
        let mut turns: Vec<&Turn> = reference
            .iter()
            .filter(|t| t.speaker.as_deref() == Some(you.as_str()))
            .collect();
        turns.sort_by_key(|t| std::cmp::Reverse(t.t1_ms - t.t0_ms));
        for t in turns {
            let sample = slice(pcm, t.t0_ms, t.t1_ms);
            if sample.is_empty() {
                continue;
            }
            if let Some(v) = models.resnet.embed(sample)? {
                user_voice.push(cluster::normalized(&v));
            }
            if user_voice.len() == 3 {
                break;
            }
        }
    }
    Ok(Loaded {
        system,
        mic,
        system_speech,
        system_turns,
        mic_speech,
        reference,
        you: m.you.clone(),
        user_voice,
        voices: VoiceCache::default(),
    })
}

/// What `row` makes of `meeting`, as hypothesis turns.
fn diarize(meeting: &mut Loaded, models: &Models, row: Row) -> Result<Vec<Turn>> {
    let (turn_params, min_embed_ms) = short_turn_tunables(row.short_turns);
    let config = PipelineConfig {
        turns: turn_params,
        min_embed_ms,
        cluster: ClusterParams {
            default_threshold: row.cut,
            legacy: row.legacy,
            ..ClusterParams::default()
        },
        ..PipelineConfig::default()
    };
    let system_spans: Vec<Span> = match (row.turns, &meeting.system_turns) {
        (true, Some(spans)) => spans[usize::from(row.short_turns)].clone(),
        _ => meeting.system_speech.spans.clone(),
    };
    let lines: Vec<(Track, Span)> = meeting
        .mic_speech
        .iter()
        .map(|s| (Track::Mic, *s))
        .chain(system_spans.into_iter().map(|s| (Track::System, s)))
        .collect();
    let embed_mic = !meeting.user_voice.is_empty();
    let mut cache = ProcessingCache::new();
    let mut better: Vec<Option<Vec<f32>>> = Vec::with_capacity(lines.len());
    for &(track, span) in &lines {
        let wanted = (track == Track::System || embed_mic) && span.len_ms() >= config.min_embed_ms;
        let empty: &[f32] = &[];
        let pcm: &[f32] = match track {
            Track::System => &meeting.system,
            Track::Mic => meeting.mic.as_deref().unwrap_or(empty),
        };
        let standard = if wanted {
            meeting
                .voices
                .get("resnet34", &models.resnet, track, pcm, span)?
        } else {
            None
        };
        better.push(match (&models.titanet, row.titanet, &standard) {
            (Some(t), true, Some(_)) => meeting.voices.get("titanet", t, track, pcm, span)?,
            _ => None,
        });
        cache.units.push(Unit { track, span });
        // No transcription: any text makes it a line with speech.
        cache.transcripts.push(Some(UnitTranscript {
            text: "speech".into(),
            words: Vec::new(),
        }));
        cache.embeddings.push(Some(standard));
    }
    let source = if row.titanet {
        ClusterSource::Vectors(better)
    } else {
        ClusterSource::Voiceprints
    };
    let clustering = cluster_units_with(&cache, &config, &[], &meeting.user_voice, &source);
    Ok(lines
        .iter()
        .zip(&clustering.labels)
        .map(|(&(track, span), label)| Turn {
            t0_ms: span.t0_ms,
            t1_ms: span.t1_ms,
            speaker: match (track, label) {
                (_, Some(l)) => Some(format!("speaker {l}")),
                (Track::Mic, None) => Some(YOU.to_string()),
                (Track::System, None) => None,
            },
        })
        .collect())
}

fn run_grid(manifest_path: &Path, vad_model: PathBuf) -> Result<()> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let resnet = var("GUIDY_TEST_SPEAKER_MODEL").context("set GUIDY_TEST_SPEAKER_MODEL")?;
    let titanet = var("GUIDY_TEST_TITANET_MODEL");
    let segmenter = match (var("GUIDY_TEST_SEGMENTATION_MODEL"), &titanet) {
        (Some(seg), Some(t)) => Some(PyannoteSegmenter::new(&seg, t)?),
        _ => None,
    };
    let mut models = Models {
        resnet: SpeakerEmbedder::with_threads(&resnet, 4)?,
        titanet: match &titanet {
            Some(t) => Some(SpeakerEmbedder::with_threads(t, 4)?),
            None => None,
        },
        segmenter,
        vad: vad_model,
    };
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&text).context("read the manifest")?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));

    let mut rows = Vec::new();
    for titanet in [false, true] {
        for turns in [false, true] {
            for legacy in [true, false] {
                for short_turns in [false, true] {
                    for cut in CUTS {
                        let needs_titanet = titanet || turns;
                        if !needs_titanet || models.titanet.is_some() {
                            rows.push(Row {
                                titanet,
                                turns,
                                legacy,
                                short_turns,
                                cut,
                            });
                        }
                    }
                }
            }
        }
    }
    let mut totals: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    for m in &manifest.meetings {
        let mut meeting = load(base, m, &mut models)?;
        for &row in &rows {
            let hypothesis = diarize(&mut meeting, &models, row)?;
            let s = score(&meeting.reference, &hypothesis, COLLAR_MS);
            println!(
                "{:<20} {} DER={:.3} miss={:.3} fa={:.3} conf={:.3} count_err={} purity={:.3} \
                 coverage={:.3} unknown={:.3}{}",
                m.name,
                row.label(),
                s.der,
                s.missed,
                s.false_alarm,
                s.confusion,
                s.speaker_count_error,
                s.purity,
                s.coverage,
                s.unknown_share,
                match &meeting.you {
                    Some(you) => {
                        let (p, r) = you_precision_recall(&meeting.reference, &hypothesis, you);
                        format!(" you_precision={p:.3} you_recall={r:.3}")
                    }
                    None => String::new(),
                }
            );
            let total = totals.entry(row.label()).or_default();
            total.0 += s.der;
            total.1 += 1;
        }
    }
    let mut means: Vec<(f64, String)> = totals
        .into_iter()
        .map(|(label, (sum, n))| (sum / n.max(1) as f64, label))
        .collect();
    means.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\nMean DER, best first:");
    for (der, label) in means {
        println!("{der:.3}  {label}");
    }
    Ok(())
}

/// Run through `cargo xtask diar-eval <manifest.json>`, which sets
/// `GUIDY_DIAR_MANIFEST`. Also set `GUIDY_TEST_SPEAKER_MODEL=<ResNet34 onnx>`
/// and, for the rows that use them, `GUIDY_TEST_TITANET_MODEL=<onnx>` and
/// `GUIDY_TEST_SEGMENTATION_MODEL=<onnx>`.
// IGNORE-REASON: needs labelled meeting audio and downloaded models.
#[test]
#[ignore]
fn diar_eval() {
    // `xtask test --full` runs every ignored test: without a manifest, nothing.
    let Some(manifest) = std::env::var_os("GUIDY_DIAR_MANIFEST") else {
        eprintln!("GUIDY_DIAR_MANIFEST not set; nothing to evaluate");
        return;
    };
    let vad_model = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/resources/models/silero-vad-v6.2-F32.gguf");
    run_grid(Path::new(&manifest), vad_model).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_can_compare_the_short_turn_tunables() {
        let (turns, floor) = short_turn_tunables(true);
        assert_eq!(
            (
                turns.scan_window_ms,
                turns.scan_hop_ms,
                turns.min_side_ms,
                floor
            ),
            (500, 250, 1_000, 1_000)
        );
        let (turns, floor) = short_turn_tunables(false);
        assert_eq!(
            (
                turns.scan_window_ms,
                turns.scan_hop_ms,
                turns.min_side_ms,
                floor
            ),
            (1_500, 500, 1_500, 1_500)
        );
        // Nothing else differs.
        assert_eq!(turns.side_ms, TurnParams::default().side_ms);
        assert_eq!(turns.merge_ms, TurnParams::default().merge_ms);
    }

    fn turn(t0_ms: i64, t1_ms: i64, speaker: &str) -> Turn {
        Turn {
            t0_ms,
            t1_ms,
            speaker: Some(speaker.into()),
        }
    }

    fn two_speakers() -> Vec<Turn> {
        vec![turn(0, 10_000, "a"), turn(10_000, 20_000, "b")]
    }

    #[test]
    fn a_perfect_hypothesis_scores_zero() {
        let r = two_speakers();
        let s = score(&r, &r, COLLAR_MS);
        assert_eq!(s.der, 0.0);
        assert_eq!(s.speaker_count_error, 0);
        assert_eq!((s.purity, s.coverage), (1.0, 1.0));
        assert_eq!(s.unknown_share, 0.0);
    }

    #[test]
    fn swapped_labels_score_zero() {
        let h = vec![turn(0, 10_000, "b"), turn(10_000, 20_000, "a")];
        assert_eq!(score(&two_speakers(), &h, COLLAR_MS).der, 0.0);
        let h = vec![turn(0, 10_000, "x"), turn(10_000, 20_000, "y")];
        assert_eq!(score(&two_speakers(), &h, COLLAR_MS).der, 0.0);
    }

    #[test]
    fn known_wrong_hypotheses_score_their_share() {
        let r = two_speakers();
        // One speaker over two equal ones: half is confused.
        let s = score(&r, &[turn(0, 20_000, "x")], 0);
        assert!((s.der - 0.5).abs() < 1e-9, "{s:?}");
        assert!((s.confusion - 0.5).abs() < 1e-9, "{s:?}");
        assert_eq!(s.speaker_count_error, 1);
        assert!((s.purity - 0.5).abs() < 1e-9, "{s:?}");
        // Nothing found: all missed.
        let s = score(&r, &[], 0);
        assert_eq!((s.der, s.missed), (1.0, 1.0));
        // Speech where none is: false alarm on top.
        let h = vec![
            turn(0, 10_000, "x"),
            turn(10_000, 20_000, "y"),
            turn(20_000, 30_000, "x"),
        ];
        let s = score(&r, &h, 0);
        assert!((s.false_alarm - 0.5).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn the_collar_forgives_only_near_a_boundary() {
        let r = two_speakers();
        // 100 ms late: all inside the collar.
        let h = vec![turn(0, 10_100, "x"), turn(10_100, 20_000, "y")];
        assert_eq!(score(&r, &h, COLLAR_MS).der, 0.0);
        assert!(score(&r, &h, 0).der > 0.0);
        // 1 s late: 750 ms of it counts, of 19 s scored.
        let h = vec![turn(0, 11_000, "x"), turn(11_000, 20_000, "y")];
        let want = 75.0 / 1_900.0;
        let got = score(&r, &h, COLLAR_MS).der;
        assert!((got - want).abs() < 1e-9, "{got}");
    }

    #[test]
    fn unknown_speech_matches_nobody() {
        let r = two_speakers();
        let h = vec![
            turn(0, 10_000, "x"),
            Turn {
                t0_ms: 10_000,
                t1_ms: 20_000,
                speaker: None,
            },
        ];
        let s = score(&r, &h, 0);
        assert!((s.confusion - 0.5).abs() < 1e-9, "{s:?}");
        assert!((s.unknown_share - 0.5).abs() < 1e-9, "{s:?}");
        assert_eq!(s.speaker_count_error, 1);
    }

    #[test]
    fn nothing_to_score_is_not_a_number() {
        let s = score(&[], &[], COLLAR_MS);
        assert_eq!(s, Score::default());
    }

    #[test]
    fn speech_against_an_empty_reference_is_all_wrong() {
        let s = score(&[], &[turn(0, 10_000, "x")], COLLAR_MS);
        assert_eq!((s.der, s.false_alarm), (1.0, 1.0));
    }

    #[test]
    fn the_assignment_takes_the_best_total_over_the_greedy_pick() {
        // Greedy takes 9 and is left with 1; the best total is 8 + 8.
        let w = vec![vec![9, 8], vec![8, 1]];
        assert_eq!(best_assignment(&w, 2), vec![Some(1), Some(0)]);
        // More hypothesis speakers than reference ones, and fewer.
        let w = vec![vec![5], vec![7], vec![1]];
        assert_eq!(best_assignment(&w, 1), vec![None, Some(0), None]);
        let w = vec![vec![1, 7, 3]];
        assert_eq!(best_assignment(&w, 3), vec![Some(1)]);
        // No overlap is no mapping.
        assert_eq!(best_assignment(&[vec![0]], 1), vec![None]);
        assert!(best_assignment(&[], 0).is_empty());
    }

    #[test]
    fn rttm_lines_become_turns() {
        let text = "SPEAKER m 1 1.500 2.25 <NA> <NA> alice <NA> <NA>\n\
                    ; a comment\n\
                    SPEAKER m 1 4 1 <NA> <NA> bob <NA> <NA>\n";
        assert_eq!(
            parse_rttm(text).unwrap(),
            vec![turn(1_500, 3_750, "alice"), turn(4_000, 5_000, "bob")]
        );
        assert!(parse_rttm("SPEAKER m 1 x 1 <NA> <NA> bob").is_err());
        assert!(parse_rttm("SPEAKER m 1 4").is_err());
        for bad in [
            "NaN 1", "inf 1", "1 inf", "1 NaN", "-1 1", "1 -1", "1 999999",
        ] {
            let line = format!("SPEAKER m 1 {bad} <NA> <NA> bob <NA> <NA>");
            assert!(parse_rttm(&line).is_err(), "{bad}");
        }
        // No length: not a speaker at all.
        let ghost = "SPEAKER m 1 4 0 <NA> <NA> ghost <NA> <NA>";
        assert!(parse_rttm(ghost).unwrap().is_empty());
    }

    #[test]
    fn you_are_scored_against_your_own_turns() {
        let r = vec![turn(0, 10_000, "imran"), turn(10_000, 20_000, "guest")];
        // Called you for all of it: half right, and all of the user's found.
        let h = vec![turn(0, 20_000, YOU)];
        let (p, recall) = you_precision_recall(&r, &h, "imran");
        assert!((p - 0.5).abs() < 1e-9 && (recall - 1.0).abs() < 1e-9);
        // Never called you.
        let h = vec![turn(0, 20_000, "speaker 0")];
        assert_eq!(you_precision_recall(&r, &h, "imran"), (0.0, 0.0));
    }
}
