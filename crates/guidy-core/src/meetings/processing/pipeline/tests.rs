//! Pipeline tests over synthetic audio with fake engines.
//!
//! The synthetic "voice" of speaker k is a constant signal of amplitude
//! `0.1 * (k + 1)`: the fake VAD hears anything above 0.01, the fake
//! transcriber says "hello from speaker k", and the fake embedder returns a
//! vector pointing along axis k.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
use crate::meetings::manager::{PersonRef, SuggestionSource};
use crate::meetings::processing::naming;
use crate::meetings::processing::{PcmSource, SAMPLE_RATE};
use crate::meetings::settings::{MeetingSettings, UserEmail};

const USER: usize = 8;

struct FakeVad;

impl VoiceActivityDetector for FakeVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
        Ok(if frame.iter().any(|s| s.abs() > 0.01) {
            VadFrame::Speech(frame)
        } else {
            VadFrame::Noise
        })
    }
    fn frame_samples(&self) -> usize {
        512
    }
}

impl SpeechScorer for FakeVad {
    fn score(&mut self, frames: &[f32]) -> Result<Vec<f32>> {
        Ok(frames
            .chunks(512)
            .map(|f| {
                let loud = f.iter().filter(|s| s.abs() > 0.01).count();
                if loud * 2 >= f.len() {
                    0.95
                } else {
                    0.05
                }
            })
            .collect())
    }
}

fn speaker_of(pcm: &[f32]) -> usize {
    let max = pcm.iter().fold(0f32, |m, s| m.max(s.abs()));
    ((max / 0.1).round() as usize).saturating_sub(1)
}

/// Counts calls; optionally raises the cancel flag after `cancel_after`.
struct FakeTranscriber<'c> {
    calls: Rc<Cell<usize>>,
    cancel_after: Option<(usize, &'c AtomicBool)>,
    /// A speaker whose English autodetect writes in Devanagari, as
    /// Nemotron 3.5 does with an Indian accent. Enables the language pass.
    accented: Option<usize>,
}

impl FakeTranscriber<'_> {
    fn say(&mut self, pcm: Vec<f32>, language: Option<&str>) -> Result<(String, Vec<Word>)> {
        self.calls.set(self.calls.get() + 1);
        if let Some((n, flag)) = self.cancel_after {
            if self.calls.get() >= n {
                flag.store(true, Ordering::SeqCst);
            }
        }
        let k = speaker_of(&pcm);
        let text = if self.accented == Some(k) && language.is_none() {
            format!("हेलो फ्रॉम स्पीकर {k} टुडे")
        } else {
            format!("hello from speaker {k} today")
        };
        let words = text
            .split(' ')
            .enumerate()
            .map(|(i, w)| Word {
                t0_ms: 200 + i as i64 * 300,
                t1_ms: 450 + i as i64 * 300,
                text: w.to_string(),
            })
            .collect();
        Ok((text, words))
    }
}

impl SegmentTranscriber for FakeTranscriber<'_> {
    fn transcribe(&mut self, pcm: Vec<f32>) -> Result<(String, Vec<Word>)> {
        self.say(pcm, None)
    }

    fn language_of(&self, text: &str) -> Option<String> {
        self.accented?;
        segment_language(text, &["en-US".into(), "hi-IN".into()])
    }

    fn transcribe_in(&mut self, pcm: Vec<f32>, language: &str) -> Result<(String, Vec<Word>)> {
        self.say(pcm, Some(language))
    }
}

struct FakeEmbedder {
    calls: Rc<Cell<usize>>,
}

impl Embedder for FakeEmbedder {
    fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
        self.calls.set(self.calls.get() + 1);
        let mut v = vec![0f32; 16];
        v[speaker_of(pcm)] = 1.0;
        v[(pcm.len() / 1000) % 16] += 0.15;
        Ok(Some(v))
    }
}

/// `(speaker, t0_ms, t1_ms)` turns rendered into a track of `total_ms`.
fn render(total_ms: i64, turns: &[(usize, i64, i64)]) -> Vec<f32> {
    let at = |ms: i64| ms as usize * SAMPLE_RATE / 1000;
    let mut pcm = vec![0f32; at(total_ms)];
    for &(k, t0, t1) in turns {
        pcm[at(t0)..at(t1)].fill(0.1 * (k + 1) as f32);
    }
    pcm
}

/// Remote speakers 0, 1, 2 take turns twice; the user speaks at the end; the
/// mic also picks up an echo of speaker 1's first turn.
fn system_turns() -> Vec<(usize, i64, i64)> {
    vec![
        (0, 1000, 4000),
        (1, 5000, 8000),
        (2, 9000, 12000),
        (0, 13000, 16000),
        (1, 17000, 20000),
        (2, 21000, 24000),
    ]
}

fn mic_turns() -> Vec<(usize, i64, i64)> {
    vec![(1, 5000, 8000), (USER, 26000, 29000)]
}

struct Fixture {
    store: MeetingsStore,
    dir: tempfile::TempDir,
    id: MeetingId,
}

fn fixture() -> Fixture {
    let store = MeetingsStore::open_in_memory().unwrap();
    let id = store
        .insert_meeting(&Meeting {
            id: 0,
            title: "Weekly sync".into(),
            calendar_event_id: Some("evt".into()),
            calendar_event: Some(CalendarEvent {
                id: "evt".into(),
                title: "Weekly sync".into(),
                start_at: 0,
                end_at: 1,
                meeting_url: None,
                attendees: vec![
                    Attendee {
                        name: Some("Alice".into()),
                        email: Some("alice@example.com".into()),
                        is_current_user: false,
                    },
                    Attendee {
                        name: Some("Me Myself".into()),
                        email: Some("me@example.com".into()),
                        is_current_user: false,
                    },
                    Attendee {
                        name: Some("Bob".into()),
                        email: Some("bob@example.com".into()),
                        is_current_user: false,
                    },
                ],
            }),
            started_at: 0,
            ended_at: Some(30_000),
            mic_audio_path: Some("1-mic.gopus".into()),
            system_audio_path: Some("1-system.gopus".into()),
            status: MeetingStatus::Processing,
            error: None,
            processing_stage: None,
            description: None,
            title_source: Default::default(),
            has_summary: false,
        })
        .unwrap();
    Fixture {
        store,
        dir: tempfile::tempdir().unwrap(),
        id,
    }
}

struct Run {
    transcribe_calls: usize,
    embed_calls: usize,
    result: Result<()>,
    progress: Vec<ProcessingProgress>,
    lines: Vec<ProvisionalLine>,
}

fn run(f: &Fixture, cancel_after: Option<usize>) -> Run {
    run_tracks(f, cancel_after, &mic_turns(), &system_turns())
}

fn run_tracks(
    f: &Fixture,
    cancel_after: Option<usize>,
    mic: &[(usize, i64, i64)],
    system: &[(usize, i64, i64)],
) -> Run {
    run_accented(f, cancel_after, None, mic, system)
}

fn run_accented(
    f: &Fixture,
    cancel_after: Option<usize>,
    accented: Option<usize>,
    mic: &[(usize, i64, i64)],
    system: &[(usize, i64, i64)],
) -> Run {
    let cancel = AtomicBool::new(false);
    let t_calls = Rc::new(Cell::new(0));
    let e_calls = Rc::new(Cell::new(0));
    let mut transcriber = FakeTranscriber {
        calls: t_calls.clone(),
        cancel_after: cancel_after.map(|n| (n, &cancel)),
        accented,
    };
    let mut embedder = FakeEmbedder {
        calls: e_calls.clone(),
    };
    let progress = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = progress.clone();
    let lines = Rc::new(std::cell::RefCell::new(Vec::new()));
    let line_sink = lines.clone();
    let result = Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig {
            user_label: "Imran".into(),
            preferred_language: Some("en".into()),
            ..PipelineConfig::default()
        },
        mic: Some(Box::new(PcmSource::new(render(30_000, mic)))),
        system: Some(Box::new(PcmSource::new(render(30_000, system)))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: Some(Box::new(move |line| line_sink.borrow_mut().push(line))),
        on_progress: Box::new(move |p| sink.borrow_mut().push(p)),
    }
    .run();
    let progress = progress.borrow().clone();
    let lines = lines.borrow().clone();
    Run {
        transcribe_calls: t_calls.get(),
        embed_calls: e_calls.get(),
        result,
        lines,
        progress,
    }
}

fn stage(f: &Fixture) -> Option<ProcessingStage> {
    f.store
        .get_meeting(f.id)
        .unwrap()
        .unwrap()
        .processing_stage
        .as_deref()
        .and_then(ProcessingStage::parse)
}

/// The [`FakeEmbedder`] voice of speaker `k`.
fn voice(k: usize) -> Vec<f32> {
    let mut v = vec![0f32; 16];
    v[k] = 1.0;
    v
}

#[test]
fn an_in_person_meeting_is_diarized_with_the_users_recorded_voice() {
    let f = fixture();
    f.store.replace_user_voiceprints(&[voice(USER)]).unwrap();
    // Everyone is in the room: only the mic hears them.
    let mic = [
        (USER, 1000, 4000),
        (5, 5000, 9500),
        (USER, 10500, 12500),
        (5, 13500, 18000),
        (6, 19000, 23500),
        (6, 24500, 29000),
    ];
    let r = run_tracks(&f, None, &mic, &[]);
    r.result.unwrap();
    assert_eq!(r.embed_calls, 6);

    let speakers = f.store.speakers(f.id).unwrap();
    let labels: Vec<&str> = speakers.iter().map(|s| s.cluster_label.as_str()).collect();
    assert_eq!(labels, vec!["Imran", "Speaker 1", "Speaker 2"]);
    let label_of = |id| {
        speakers
            .iter()
            .find(|s| Some(s.id) == id)
            .map(|s| s.cluster_label.as_str())
    };
    let segments = f.store.segments(f.id).unwrap();
    let said: Vec<(&str, &str)> = segments
        .iter()
        .map(|s| {
            let k = s.text.split(' ').nth(3).unwrap();
            (label_of(s.speaker_id).unwrap(), k)
        })
        .collect();
    assert_eq!(
        said,
        vec![
            ("Imran", "8"),
            ("Speaker 1", "5"),
            ("Imran", "8"),
            ("Speaker 1", "5"),
            ("Speaker 2", "6"),
            ("Speaker 2", "6"),
        ]
    );
}

#[test]
fn without_a_recorded_voice_the_mic_is_the_user() {
    let f = fixture();
    let mic = [(USER, 1000, 4000), (5, 5000, 8000)];
    let r = run_tracks(&f, None, &mic, &[]);
    r.result.unwrap();
    assert_eq!(r.embed_calls, 0);
    let speakers = f.store.speakers(f.id).unwrap();
    assert_eq!(speakers.len(), 1);
    assert!(speakers[0].is_user);
}

#[test]
fn a_line_of_a_second_gets_a_voiceprint_and_a_shorter_one_none() {
    let f = fixture();
    // With the VAD's 150 ms padding each way: about 1.3 s and 0.9 s.
    let r = run_tracks(&f, None, &[], &[(0, 1_000, 2_000), (1, 5_000, 5_600)]);
    r.result.unwrap();
    assert_eq!(r.embed_calls, 1);
}

#[test]
fn tagging_a_short_line_stores_no_voiceprint() {
    let f = fixture();
    run_tracks(&f, None, &[], &[(0, 1_000, 2_000)])
        .result
        .unwrap();
    let line = f.store.segments(f.id).unwrap().remove(0);
    let person = naming::assign_segment(
        &f.store,
        line.id,
        PersonRef::New {
            display_name: "Alice".into(),
            email: None,
        },
    )
    .unwrap();
    assert!(f.store.voiceprints_for_person(person).unwrap().is_empty());
}

#[test]
fn processes_a_meeting_end_to_end() {
    let f = fixture();
    let r = run(&f, None);
    r.result.unwrap();
    // 6 system turns + 2 mic segments transcribed; 6 system voiceprints.
    assert_eq!(r.transcribe_calls, 8);
    assert_eq!(r.embed_calls, 6);
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    assert!(!sidecar::has_cache(&f.store, f.id).unwrap());

    let speakers = f.store.speakers(f.id).unwrap();
    let labels: Vec<&str> = speakers.iter().map(|s| s.cluster_label.as_str()).collect();
    assert_eq!(labels, vec!["Imran", "Speaker 1", "Speaker 2", "Speaker 3"]);
    assert!(speakers[0].is_user && speakers[0].confirmed);
    assert!(speakers[1..]
        .iter()
        .all(|s| s.person_id.is_none() && !s.confirmed));

    let segments = f.store.segments(f.id).unwrap();
    // The mic echo of speaker 1 is gone: 6 system + 1 user segment.
    assert_eq!(segments.len(), 7);
    assert_eq!(segments.iter().filter(|s| s.track == Track::Mic).count(), 1);
    let user_seg = segments.iter().find(|s| s.track == Track::Mic).unwrap();
    assert_eq!(user_seg.speaker_id, Some(speakers[0].id));
    assert!(user_seg.t0_ms >= 25_000);
    // Turns of the same remote speaker share a speaker id.
    let sys: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.track == Track::System)
        .collect();
    assert_eq!(sys[0].speaker_id, sys[3].speaker_id);
    assert_eq!(sys[1].speaker_id, sys[4].speaker_id);
    assert_ne!(sys[0].speaker_id, sys[1].speaker_id);
    assert_ne!(sys[1].speaker_id, sys[2].speaker_id);
    // Words are in meeting time, inside their segment.
    for s in &segments {
        assert!(!s.words.is_empty());
        assert!(s
            .words
            .iter()
            .all(|w| w.t0_ms >= s.t0_ms && w.t1_ms <= s.t1_ms));
    }
    // Centroids on the cluster speakers; per-segment voiceprints sidecar.
    assert!(f.store.speaker_embedding(speakers[1].id).unwrap().is_some());
    let prints: Vec<SegmentEmbedding> = sidecar::read_embeddings(&f.store, f.id).unwrap().unwrap();
    assert_eq!(prints.len(), 6);

    // Progress is monotone and ends at 1.0.
    let fractions: Vec<f32> = r.progress.iter().map(|p| p.fraction).collect();
    assert!(
        fractions.windows(2).all(|w| w[0] <= w[1] + 1e-6),
        "{fractions:?}"
    );
    assert_eq!(r.progress.last().unwrap().stage, ProcessingStage::Done);
    assert_eq!(*fractions.last().unwrap(), 1.0);
}

/// Per 1 ms of `pcm`, the synthetic speaker talking (`None`: silence).
fn speakers_by_ms(pcm: &[f32]) -> Vec<Option<usize>> {
    pcm.chunks(SAMPLE_RATE / 1000)
        .map(|ms| {
            let max = ms.iter().fold(0f32, |m, s| m.max(s.abs()));
            (max > 0.01).then(|| ((max / 0.1).round() as usize).saturating_sub(1))
        })
        .collect()
}

/// Turns where the synthetic speaker changes, as a segmentation model
/// would report them.
struct FakeSegmenter;

impl TurnSegmenter for FakeSegmenter {
    fn turns(&mut self, pcm: &[f32]) -> Result<Vec<turns::Turn>> {
        let mut out: Vec<turns::Turn> = Vec::new();
        for (t, k) in speakers_by_ms(pcm).into_iter().enumerate() {
            let (Some(k), t) = (k, t as i64) else {
                continue;
            };
            match out.last_mut() {
                Some(last) if last.speaker == k as i32 && last.t1_ms == t => last.t1_ms = t + 1,
                _ => out.push(turns::Turn {
                    t0_ms: t,
                    t1_ms: t + 1,
                    speaker: k as i32,
                }),
            }
        }
        Ok(out)
    }
}

/// Voiceprints for finding turns: how much of the audio each synthetic
/// speaker says.
struct ShareOfVoices;

impl Embedder for ShareOfVoices {
    fn embed(&mut self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
        let mut v = vec![0f32; 16];
        for k in speakers_by_ms(pcm).into_iter().flatten() {
            v[k] += 1.0;
        }
        Ok(Some(v))
    }
}

/// Two remote speakers back to back, no pause: one VAD span. With turn
/// detection it becomes two lines, each transcribed and diarized alone.
fn run_back_to_back(f: &Fixture, detect: bool) -> Run {
    let cancel = AtomicBool::new(false);
    let t_calls = Rc::new(Cell::new(0));
    let e_calls = Rc::new(Cell::new(0));
    let mut transcriber = FakeTranscriber {
        calls: t_calls.clone(),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: e_calls.clone(),
    };
    let (mut segmenter, mut voices) = (FakeSegmenter, ShareOfVoices);
    // A change on a VAD frame boundary (32 ms), where a cut snaps to.
    let system = [(0, 1000, 7008), (1, 7008, 13000)];
    let result = Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: None,
        system: Some(Box::new(PcmSource::new(render(30_000, &system)))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: detect.then_some(TurnDetection {
            segmenter: Some(&mut segmenter),
            embedder: &mut voices,
        }),
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(|_| {}),
    }
    .run();
    Run {
        transcribe_calls: t_calls.get(),
        embed_calls: e_calls.get(),
        result,
        progress: Vec::new(),
        lines: Vec::new(),
    }
}

#[test]
fn a_line_is_split_where_the_voice_changes() {
    let f = fixture();
    run_back_to_back(&f, true).result.unwrap();
    let segments = f.store.segments(f.id).unwrap();
    assert_eq!(segments.len(), 2, "{segments:?}");
    assert_eq!(segments[0].t1_ms, 7008);
    assert_eq!(segments[1].t0_ms, 7008);
    assert_eq!(segments[0].text, "hello from speaker 0 today");
    assert_eq!(segments[1].text, "hello from speaker 1 today");
    assert_ne!(segments[0].speaker_id, segments[1].speaker_id);
}

#[test]
fn without_turn_detection_a_line_holds_both_voices() {
    let f = fixture();
    let r = run_back_to_back(&f, false);
    r.result.unwrap();
    assert_eq!(r.transcribe_calls, 1);
    assert_eq!(f.store.segments(f.id).unwrap().len(), 1);
}

#[test]
fn a_runs_progress_is_shared_by_the_stages_it_runs() {
    // A redo of the speakers runs voiceprints, cluster and merge: 0 to 1
    // over those, the slow larger-model cluster getting most of the bar.
    let mut model = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let diarizer = Diarizer::Model(&mut model);
    let plan = ProgressPlan::new(ProcessingStage::Voiceprints, |s| {
        stage_cost(s, &diarizer, false)
    });
    assert_eq!(plan.fraction(ProcessingStage::Voiceprints, 0.0), 0.0);
    let cluster = plan.fraction(ProcessingStage::Cluster, 0.0);
    let merge = plan.fraction(ProcessingStage::Merge, 0.0);
    assert!(cluster < 0.2 && merge > 0.9, "{cluster} {merge}");
    assert_eq!(plan.fraction(ProcessingStage::Done, 0.0), 1.0);
    // Stages before the run's first count as not reached.
    assert_eq!(plan.fraction(ProcessingStage::Transcribe, 0.5), 0.0);

    // With turn detection, finding speech is no longer a sliver.
    let full = |turns| {
        ProgressPlan::new(ProcessingStage::Decode, |s| {
            stage_cost(s, &Diarizer::Voiceprints, turns)
        })
    };
    let vad_share = |p: &ProgressPlan| {
        p.fraction(ProcessingStage::Transcribe, 0.0) - p.fraction(ProcessingStage::Vad, 0.0)
    };
    assert!(vad_share(&full(true)) > 0.5);
    assert!(vad_share(&full(false)) < 0.05);
}

#[test]
fn progress_counts_the_stages_items() {
    let f = fixture();
    let r = run(&f, None);
    r.result.unwrap();
    let transcribing: Vec<(Option<usize>, Option<usize>)> = r
        .progress
        .iter()
        .filter(|p| p.stage == ProcessingStage::Transcribe && p.done.is_some())
        .map(|p| (p.done, p.total))
        .collect();
    assert_eq!(transcribing.first(), Some(&(Some(1), Some(8))));
    assert_eq!(transcribing.last(), Some(&(Some(8), Some(8))));
    assert_eq!(r.progress.first().unwrap().fraction, 0.0);
}

#[test]
fn cancel_then_resume_skips_finished_work() {
    let f = fixture();
    let r = run(&f, Some(3));
    assert!(super::super::is_cancelled(&r.result.unwrap_err()));
    assert_eq!(r.transcribe_calls, 3);
    // VAD done, transcription part-way (checkpointed).
    assert_eq!(stage(&f), Some(ProcessingStage::Vad));
    let cache: ProcessingCache = sidecar::read_cache(&f.store, f.id).unwrap().unwrap();
    assert_eq!(cache.transcripts.iter().filter(|t| t.is_some()).count(), 3);

    let r = run(&f, None);
    r.result.unwrap();
    assert_eq!(r.transcribe_calls, 5, "only the remaining segments");
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    assert_eq!(f.store.segments(f.id).unwrap().len(), 7);
}

#[test]
fn a_lost_checkpoint_restarts_from_the_beginning() {
    let f = fixture();
    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Voiceprints))
        .unwrap();
    let r = run(&f, None);
    r.result.unwrap();
    assert_eq!(r.transcribe_calls, 8);
}

#[test]
fn a_stored_transcript_resumes_at_the_voiceprints() {
    let seg = |id: i64, track: Track, t0_ms: i64, text: &str| Segment {
        id,
        meeting_id: 1,
        track,
        t0_ms,
        t1_ms: t0_ms + 1_000,
        text: text.into(),
        words: Vec::new(),
        speaker_id: Some(1),
    };
    let segments = [
        seg(1, Track::System, 5_000, "later"),
        seg(2, Track::Mic, 9_000, "mine"),
        seg(3, Track::System, 1_000, "first"),
    ];
    let cache = cache_from_segments(&segments, true, true, 60_000);
    let order: Vec<(Track, i64)> = cache
        .units
        .iter()
        .map(|u| (u.track, u.span.t0_ms))
        .collect();
    assert_eq!(
        order,
        [
            (Track::Mic, 9_000),
            (Track::System, 1_000),
            (Track::System, 5_000)
        ]
    );
    assert_eq!(cache.transcripts[1].as_ref().unwrap().text, "first");
    assert_eq!(
        (cache.mic_ms, cache.system_ms),
        (Some(60_000), Some(60_000))
    );
    let (resumed, next) = resume_point(Some(ProcessingStage::Transcribe), Some(cache));
    assert_eq!(next, ProcessingStage::Voiceprints);
    assert_eq!(resumed.units.len(), 3);
    // A track whose audio is gone isn't rebuilt.
    let cache = cache_from_segments(&segments, false, true, 60_000);
    assert_eq!(cache.units.len(), 2);
    assert_eq!(cache.mic_ms, None);
}

#[test]
fn an_outlier_line_gives_its_speaker_no_voiceprint() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let speaker = f.store.speakers(f.id).unwrap()[1].clone();
    let lines: Vec<i64> = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .filter(|s| s.speaker_id == Some(speaker.id))
        .map(|s| s.id)
        .collect();
    assert_eq!(lines.len(), 2);
    let mut stored = sidecar::read_embeddings(&f.store, f.id).unwrap().unwrap();
    for e in stored.iter_mut().filter(|e| e.segment_id == lines[0]) {
        e.embedding = voice(15);
    }
    sidecar::write_embeddings(&f.store, f.id, &stored).unwrap();
    let prints = naming::speaker_prints(&f.store, &speaker).unwrap();
    assert_eq!(prints.len(), 1);
    assert!(!prints.contains(&voice(15)));
    let person = f.store.insert_person("Alice", None).unwrap().id;
    assert_eq!(naming::store_prints(&f.store, &speaker, person).unwrap(), 1);
    // Given to Alice anyway, by an earlier version say: renaming still takes
    // it back, though it wouldn't be enrolled today.
    f.store
        .add_voiceprint(person, &voice(15), Some(f.id))
        .unwrap();
    naming::assign(
        &f.store,
        speaker.id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: None,
        },
    )
    .unwrap();
    assert!(f.store.voiceprints_for_person(person).unwrap().is_empty());
}

#[test]
fn a_speaker_with_no_line_like_it_gets_no_print_but_a_lost_sidecar_keeps_the_centroid() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let speaker = f.store.speakers(f.id).unwrap()[1].clone();
    let lines: Vec<i64> = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .filter(|s| s.speaker_id == Some(speaker.id))
        .map(|s| s.id)
        .collect();
    let mut stored = sidecar::read_embeddings(&f.store, f.id).unwrap().unwrap();
    for e in stored.iter_mut().filter(|e| lines.contains(&e.segment_id)) {
        e.embedding = voice(15);
    }
    sidecar::write_embeddings(&f.store, f.id, &stored).unwrap();
    assert!(naming::speaker_prints(&f.store, &speaker)
        .unwrap()
        .is_empty());
    sidecar::delete_embeddings(&f.store, f.id).unwrap();
    let centroid = f.store.speaker_embedding(speaker.id).unwrap().unwrap();
    assert_eq!(
        naming::speaker_prints(&f.store, &speaker).unwrap(),
        vec![centroid]
    );
}

/// Twelve words, one a second from `t0_ms`, each lasting 900 ms.
fn twelve_words(t0_ms: i64) -> Vec<Word> {
    (0..12)
        .map(|i| Word {
            t0_ms: t0_ms + i * 1_000,
            t1_ms: t0_ms + i * 1_000 + 900,
            text: format!("w{i}"),
        })
        .collect()
}

#[test]
fn a_cut_shares_out_the_words_by_midpoint() {
    let span = vad::Span::new;
    let transcript = |words: Vec<Word>, text: &str| UnitTranscript {
        text: text.into(),
        words,
    };
    let word = |t0_ms, t1_ms, text: &str| Word {
        t0_ms,
        t1_ms,
        text: text.into(),
    };
    let pieces = [span(0, 1_000), span(1_000, 2_000)];
    // Over the cut: its midpoint decides. On the cut: the later piece, once.
    let t = transcript(
        vec![
            word(100, 300, "one"),
            word(800, 1_100, "two"),
            word(900, 1_100, "three"),
            word(1_500, 1_500, "four"),
        ],
        "One, two three four.",
    );
    let parts = split_transcript(&t, &pieces).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].1.text, "One, two");
    assert_eq!(parts[1].1.text, "three four.");
    assert_eq!(parts[0].1.words.len(), 2);
    assert_eq!(parts[1].1.words.len(), 2);
    assert_eq!((parts[0].0, parts[1].0), (pieces[0], pieces[1]));
    assert_eq!(parts[0].1.words[1].t1_ms, 1_000, "clamped to its piece");
    assert_eq!(parts[1].1.words[0].t0_ms, 1_000, "clamped to its piece");
    // Overlapping words stay in the text's order; a reversed one is repaired.
    let t = transcript(
        vec![
            word(100, 200, "zero"),
            word(800, 1_800, "first"),
            word(900, 1_000, "second"),
            word(1_300, 1_100, "third"),
        ],
        "zero first second third",
    );
    let parts = split_transcript(&t, &pieces).unwrap();
    assert_eq!(parts[0].1.text, "zero");
    assert_eq!(parts[1].1.text, "first second third");
    assert_eq!(parts[1].1.words[2].t1_ms, 1_300);
    // Tokens that don't line up with the words: the words' own text.
    let t = transcript(
        vec![word(100, 200, "one"), word(1_100, 1_200, "two")],
        "one two three",
    );
    let parts = split_transcript(&t, &pieces).unwrap();
    assert_eq!(parts[0].1.text, "one");
    assert_eq!(parts[1].1.text, "two");
    // No spaces in the text: none added.
    let t = transcript(vec![word(100, 200, "明"), word(1_100, 1_200, "日")], "明日");
    let parts = split_transcript(&t, &pieces).unwrap();
    assert_eq!(parts[0].1.text, "明");
    // All the words in one piece, none at all, or nothing cut: stays whole.
    let t = transcript(vec![word(100, 200, "one")], "one");
    assert!(split_transcript(&t, &pieces).is_none());
    assert!(split_transcript(&transcript(Vec::new(), "text"), &pieces).is_none());
    assert!(split_transcript(&t, &pieces[..1]).is_none());
}

/// One stored line holding two voices, its checkpoint ready for a redo, with
/// a hand-made pin on it. Returns the person pinned.
fn stored_line_of_two_voices(f: &Fixture) -> PersonId {
    let words = twelve_words(1_000);
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let line = Segment {
        id: 1,
        meeting_id: f.id,
        track: Track::System,
        t0_ms: 1_000,
        t1_ms: 13_000,
        text,
        words,
        speaker_id: None,
    };
    let cache = cache_from_segments(&[line], false, true, 30_000);
    assert!(cache.resplit_pending);
    sidecar::write_cache(&f.store, f.id, &cache).unwrap();
    f.store
        .set_processing_stage_if_processing(f.id, Some(ProcessingStage::Transcribe))
        .unwrap();
    let person = f.store.insert_person("Alice", None).unwrap().id;
    f.store
        .replace_segment_pins(
            f.id,
            &[SegmentPin {
                track: Track::System,
                t0_ms: 1_000,
                t1_ms: 13_000,
                person_id: person,
                by_hand: true,
            }],
        )
        .unwrap();
    person
}

/// Redo the speakers of [`stored_line_of_two_voices`]. Returns how many
/// system lines the transcriber was asked for, and the run's result.
fn redo_stored_line(f: &Fixture, detect: bool) -> (usize, Result<()>) {
    let cancel = AtomicBool::new(false);
    let calls = Rc::new(Cell::new(0));
    let mut transcriber = FakeTranscriber {
        calls: calls.clone(),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let (mut segmenter, mut voices) = (FakeSegmenter, ShareOfVoices);
    let system = [(0, 1_000, 7_008), (1, 7_008, 13_000)];
    let result = Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: None,
        system: Some(Box::new(PcmSource::new(render(30_000, &system)))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: detect.then_some(TurnDetection {
            segmenter: Some(&mut segmenter),
            embedder: &mut voices,
        }),
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(|_| {}),
    }
    .run();
    (calls.get(), result)
}

#[test]
fn redoing_the_speakers_splits_a_line_of_two_voices_and_keeps_its_pin() {
    let f = fixture();
    let person = stored_line_of_two_voices(&f);
    let (transcribed, result) = redo_stored_line(&f, true);
    result.unwrap();
    assert_eq!(
        transcribed, 0,
        "words are shared out, not transcribed again"
    );
    let segments = f.store.segments(f.id).unwrap();
    assert_eq!(segments.len(), 2, "{segments:?}");
    assert_eq!((segments[0].t0_ms, segments[0].t1_ms), (1_000, 7_008));
    assert_eq!((segments[1].t0_ms, segments[1].t1_ms), (7_008, 13_000));
    assert_eq!(segments[0].text, "w0 w1 w2 w3 w4 w5");
    assert_eq!(segments[1].text, "w6 w7 w8 w9 w10 w11");
    // The pin holds on both pieces, and moves onto them.
    for s in &segments {
        let speaker = f.store.get_speaker(s.speaker_id.unwrap()).unwrap().unwrap();
        assert_eq!(speaker.person_id, Some(person));
    }
    let pins = f.store.segment_pins(f.id).unwrap();
    assert_eq!(pins.len(), 2, "{pins:?}");
    assert!(pins.iter().all(|p| p.by_hand && p.person_id == person));
}

#[test]
fn a_split_line_with_no_pin_keeps_its_two_voices_apart() {
    let f = fixture();
    stored_line_of_two_voices(&f);
    f.store.replace_segment_pins(f.id, &[]).unwrap();
    redo_stored_line(&f, true).1.unwrap();
    let segments = f.store.segments(f.id).unwrap();
    assert_eq!(segments.len(), 2, "{segments:?}");
    // The second piece has too little speech for a speaker of its own, and
    // sounds like nobody: unknown, not the first voice's.
    assert!(segments[0].speaker_id.is_some());
    assert_eq!(segments[1].speaker_id, None);
}

#[test]
fn without_turn_detection_a_redo_keeps_the_line_whole() {
    let f = fixture();
    stored_line_of_two_voices(&f);
    redo_stored_line(&f, false).1.unwrap();
    assert_eq!(f.store.segments(f.id).unwrap().len(), 1);
}

#[test]
fn a_checkpoint_from_before_the_flag_needs_no_split() {
    let cache = ProcessingCache::new();
    let mut json: serde_json::Value = serde_json::to_value(&cache).unwrap();
    json.as_object_mut().unwrap().remove("resplit_pending");
    let old: ProcessingCache = serde_json::from_value(json).unwrap();
    assert!(!old.resplit_pending);
}

#[test]
fn resume_point_logic() {
    use ProcessingStage::*;
    let full = {
        let mut c = ProcessingCache::new();
        c.mic_ms = Some(1000);
        c.units = vec![Unit {
            track: Track::System,
            span: vad::Span::new(0, 1000),
        }];
        c.transcripts = vec![Some(UnitTranscript {
            text: "hi".into(),
            words: vec![],
        })];
        c.embeddings = vec![Some(None)];
        c.clustering = Some(Clustering::default());
        c
    };
    assert_eq!(resume_point(None, Some(full.clone())).1, Decode);
    assert_eq!(resume_point(Some(Done), Some(full.clone())).1, Decode);
    assert_eq!(resume_point(Some(Decode), Some(full.clone())).1, Vad);
    assert_eq!(resume_point(Some(Vad), Some(full.clone())).1, Transcribe);
    assert_eq!(
        resume_point(Some(Transcribe), Some(full.clone())).1,
        Voiceprints
    );
    assert_eq!(
        resume_point(Some(Voiceprints), Some(full.clone())).1,
        Cluster
    );
    assert_eq!(resume_point(Some(Cluster), Some(full.clone())).1, Merge);
    assert_eq!(resume_point(Some(Merge), None).1, Done);
    // Missing, stale or inconsistent checkpoints start over.
    assert_eq!(resume_point(Some(Cluster), None).1, Decode);
    let mut stale = full.clone();
    stale.version = 0;
    assert_eq!(resume_point(Some(Cluster), Some(stale)).1, Decode);
    let mut partial = full.clone();
    partial.transcripts = vec![None];
    assert_eq!(resume_point(Some(Vad), Some(partial.clone())).1, Transcribe);
    assert_eq!(resume_point(Some(Transcribe), Some(partial)).1, Decode);
    let mut unclustered = full;
    unclustered.clustering = None;
    assert_eq!(resume_point(Some(Cluster), Some(unclustered)).1, Decode);
    assert_eq!(next_stage(Merge), Done);
    assert_eq!(next_stage(Done), Done);
}

#[test]
fn known_voices_are_matched_in_the_next_meeting() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let speakers = f.store.speakers(f.id).unwrap();
    let speaker1 = speakers[1].clone();

    // Name Speaker 1 (remote speaker 0) as Alice.
    let alice = naming::assign(
        &f.store,
        speaker1.id,
        PersonRef::New {
            display_name: "Alice".into(),
            email: Some("alice@example.com".into()),
        },
    )
    .unwrap();
    let named = f.store.get_speaker(speaker1.id).unwrap().unwrap();
    assert_eq!(named.person_id, Some(alice));
    assert!(named.confirmed);
    // Her two turns' voiceprints were stored.
    assert_eq!(f.store.voiceprints_for_person(alice).unwrap().len(), 2);
    // Naming again doesn't duplicate them.
    naming::assign(&f.store, speaker1.id, PersonRef::Existing(alice)).unwrap();
    assert_eq!(f.store.voiceprints_for_person(alice).unwrap().len(), 2);

    // Reprocessing (like the next meeting with her) recognizes her.
    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Done))
        .unwrap();
    run(&f, None).result.unwrap();
    let speakers = f.store.speakers(f.id).unwrap();
    let s1 = speakers
        .iter()
        .find(|s| s.cluster_label == "Speaker 1")
        .unwrap();
    assert_eq!(s1.person_id, Some(alice));
    assert!(s1.confirmed);
    assert!(s1.match_confidence.unwrap() >= 0.65);
    assert!(speakers
        .iter()
        .filter(|s| s.cluster_label != "Speaker 1")
        .all(|s| s.person_id.is_none()));
}

/// Run with `hook` called on every progress update (the transcriber and
/// embedder calls are not counted).
fn run_hooked<'a>(f: &'a Fixture, hook: impl FnMut(&ProcessingProgress) + 'a) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut transcriber = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let mut hook = hook;
    Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(move |p| hook(&p)),
    }
    .run()
}

/// Process, name Speaker 1 Alice, and queue a reprocess (which matches
/// her by voice at the cluster stage). Returns Alice.
fn named_alice(f: &Fixture) -> PersonId {
    run(f, None).result.unwrap();
    let speaker1 = f.store.speakers(f.id).unwrap()[1].clone();
    let alice = naming::assign(
        &f.store,
        speaker1.id,
        PersonRef::New {
            display_name: "Alice".into(),
            email: None,
        },
    )
    .unwrap();
    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Done))
        .unwrap();
    alice
}

fn speaker_1(f: &Fixture) -> MeetingSpeaker {
    f.store
        .speakers(f.id)
        .unwrap()
        .into_iter()
        .find(|s| s.cluster_label == "Speaker 1")
        .unwrap()
}

/// Stop a run once clustering (which matched Alice) is checkpointed: the
/// merge finds the meeting no longer processing. Then `change` the people
/// and resume, which starts at the merge.
fn resume_at_merge_after(f: &Fixture, change: impl FnOnce()) {
    let stopped = run_hooked(f, |p| {
        if p.stage == ProcessingStage::Merge {
            f.store
                .set_meeting_status(f.id, MeetingStatus::Recorded, None)
                .unwrap();
        }
    });
    assert!(super::super::is_cancelled(&stopped.unwrap_err()));
    assert_eq!(stage(f), Some(ProcessingStage::Cluster));
    change();
    f.store
        .set_meeting_status(f.id, MeetingStatus::Processing, None)
        .unwrap();
    let r = run(f, None);
    r.result.unwrap();
    assert_eq!(r.transcribe_calls, 0, "resumed at the merge");
    assert_eq!(stage(f), Some(ProcessingStage::Done));
}

/// A person deleted after the checkpoint matched them: the speaker is
/// stored unknown instead of failing the merge on every retry.
#[test]
fn resuming_after_the_matched_person_is_deleted_leaves_the_speaker_unknown() {
    let f = fixture();
    let alice = named_alice(&f);
    resume_at_merge_after(&f, || assert!(f.store.delete_person(alice).unwrap()));
    let s1 = speaker_1(&f);
    assert_eq!(s1.person_id, None);
    assert_eq!(s1.match_confidence, None);
    assert!(!s1.confirmed);
}

/// A person merged away after the checkpoint matched them: the speaker
/// links whom they were merged into.
#[test]
fn resuming_after_the_matched_person_is_merged_links_the_merge_target() {
    let f = fixture();
    let alice = named_alice(&f);
    let into = f.store.insert_person("Alice Smith", None).unwrap();
    resume_at_merge_after(&f, || f.store.merge_people(alice, into.id).unwrap());
    let s1 = speaker_1(&f);
    assert_eq!(s1.person_id, Some(into.id));
    assert!(s1.confirmed);
}

/// The matched person deleted between the cluster and merge stages of one
/// run: the run still completes.
#[test]
fn a_person_deleted_between_cluster_and_merge_does_not_fail_the_run() {
    let f = fixture();
    let alice = named_alice(&f);
    run_hooked(&f, |p| {
        if p.stage == ProcessingStage::Merge {
            f.store.delete_person(alice).unwrap();
        }
    })
    .unwrap();
    assert_eq!(speaker_1(&f).person_id, None);
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
}

#[test]
fn reassigning_moves_the_voiceprints() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let s = f.store.speakers(f.id).unwrap()[2].clone();
    let wrong = naming::assign(
        &f.store,
        s.id,
        PersonRef::New {
            display_name: "Carol".into(),
            email: None,
        },
    )
    .unwrap();
    assert_eq!(f.store.voiceprints_for_person(wrong).unwrap().len(), 2);
    let right = naming::assign(
        &f.store,
        s.id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: Some("bob@example.com".into()),
        },
    )
    .unwrap();
    assert_ne!(wrong, right);
    assert_eq!(f.store.voiceprints_for_person(wrong).unwrap().len(), 0);
    assert_eq!(f.store.voiceprints_for_person(right).unwrap().len(), 2);
    assert!(naming::assign(
        &f.store,
        s.id,
        PersonRef::New {
            display_name: "  ".into(),
            email: None
        }
    )
    .is_err());
}

#[test]
fn tagging_one_line_moves_it_and_its_voiceprint() {
    let f = fixture();
    run(&f, None).result.unwrap();
    // Name a speaker wrongly, as a merged cluster would be.
    let s = f.store.speakers(f.id).unwrap()[2].clone();
    let wrong = naming::assign(
        &f.store,
        s.id,
        PersonRef::New {
            display_name: "Prashanth".into(),
            email: None,
        },
    )
    .unwrap();
    let prints_before = f.store.voiceprints_for_person(wrong).unwrap().len();
    // A line of theirs that has its own voiceprint.
    let printed: Vec<i64> = sidecar::read_embeddings(&f.store, f.id)
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.segment_id)
        .collect();
    let line = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .find(|seg| seg.speaker_id == Some(s.id) && printed.contains(&seg.id))
        .unwrap();
    let speakers_before = f.store.speakers(f.id).unwrap().len();

    let right = naming::assign_segment(
        &f.store,
        line.id,
        PersonRef::New {
            display_name: "Mageshwari".into(),
            email: None,
        },
    )
    .unwrap();
    assert_ne!(right, wrong);
    let moved = f.store.get_segment(line.id).unwrap().unwrap();
    let speaker = f
        .store
        .get_speaker(moved.speaker_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(speaker.person_id, Some(right));
    assert!(speaker.confirmed);
    assert_eq!(f.store.speakers(f.id).unwrap().len(), speakers_before + 1);
    // Its voiceprint left Prashanth and went to Mageshwari.
    assert_eq!(f.store.voiceprints_for_person(right).unwrap().len(), 1);
    assert_eq!(
        f.store.voiceprints_for_person(wrong).unwrap().len(),
        prints_before - 1
    );

    // A second line of the same speaker joins Mageshwari's speaker.
    let other = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .find(|seg| seg.speaker_id == Some(s.id));
    if let Some(other) = other {
        naming::assign_segment(&f.store, other.id, PersonRef::Existing(right)).unwrap();
        assert_eq!(
            f.store.get_segment(other.id).unwrap().unwrap().speaker_id,
            moved.speaker_id
        );
    }
    // Suggestions for a line include people already in the meeting.
    let names: Vec<String> =
        naming::segment_suggestions(&f.store, line.id, &MeetingSettings::default())
            .unwrap()
            .into_iter()
            .map(|s| s.display_name)
            .collect();
    assert!(names.contains(&"Prashanth".to_string()), "{names:?}");
}

#[test]
fn confirm_stores_voiceprints_and_needs_a_person() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let mut s = f.store.speakers(f.id).unwrap()[1].clone();
    assert!(naming::confirm(&f.store, s.id).is_err());
    let p = f.store.insert_person("Alice", None).unwrap();
    s.person_id = Some(p.id);
    s.confirmed = false;
    f.store.update_speaker(&s).unwrap();
    naming::confirm(&f.store, s.id).unwrap();
    assert!(f.store.get_speaker(s.id).unwrap().unwrap().confirmed);
    assert_eq!(f.store.voiceprints_for_person(p.id).unwrap().len(), 2);
}

#[test]
fn suggestions_list_unassigned_attendees_then_people_by_voice() {
    let f = fixture();
    run(&f, None).result.unwrap();
    let speakers = f.store.speakers(f.id).unwrap();
    // Bob is already given to Speaker 2; Zed and Yann are known people, Yann
    // sounding like Speaker 1.
    naming::assign(
        &f.store,
        speakers[2].id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: Some("bob@example.com".into()),
        },
    )
    .unwrap();
    let zed = f.store.insert_person("Zed", None).unwrap();
    let yann = f.store.insert_person("Yann", None).unwrap();
    let centroid = f.store.speaker_embedding(speakers[1].id).unwrap().unwrap();
    f.store.add_voiceprint(yann.id, &centroid, None).unwrap();

    let settings = MeetingSettings {
        user_emails: ["work@example.com", "ME@example.com"]
            .map(|email| UserEmail {
                label: String::new(),
                email: email.into(),
            })
            .to_vec(),
        ..Default::default()
    };
    let got = naming::suggestions(&f.store, speakers[1].id, &settings).unwrap();
    let names: Vec<&str> = got.iter().map(|s| s.display_name.as_str()).collect();
    // Alice (attendee, not assigned); not me, not Bob. Then Yann (voice
    // match) before Bob and Zed (no similarity to this speaker / no prints).
    assert_eq!(names[0], "Alice");
    assert_eq!(got[0].source, SuggestionSource::Attendee);
    assert_eq!(got[0].person_id, None);
    assert!(!names.contains(&"Me Myself"));
    assert_eq!(names[1], "Yann");
    assert_eq!(got[1].source, SuggestionSource::KnownPerson);
    assert!(got[1].similarity.unwrap() > 0.99);
    let zed_pos = got
        .iter()
        .position(|s| s.person_id == Some(zed.id))
        .unwrap();
    assert!(zed_pos > 1);
    assert!(got
        .iter()
        .any(|s| s.display_name == "Bob" && s.source == SuggestionSource::KnownPerson));
}

#[test]
fn the_current_user_attendee_is_never_suggested() {
    let f = fixture();
    run(&f, None).result.unwrap();
    // EventKit flags "Me Myself" as the Mac's user; no email in settings.
    let mut m = f.store.get_meeting(f.id).unwrap().unwrap();
    let mut event = m.calendar_event.take().unwrap();
    event.attendees[1].is_current_user = true;
    f.store.set_calendar_event(f.id, Some(&event)).unwrap();
    let speakers = f.store.speakers(f.id).unwrap();
    let got = naming::suggestions(&f.store, speakers[1].id, &MeetingSettings::default()).unwrap();
    let names: Vec<&str> = got.iter().map(|s| s.display_name.as_str()).collect();
    assert!(!names.contains(&"Me Myself"), "{names:?}");
    assert!(names.contains(&"Alice") && names.contains(&"Bob"));
}

#[test]
fn deleting_the_meeting_mid_run_writes_no_more_sidecars() {
    let f = fixture();
    let cancel = AtomicBool::new(false);
    let mut transcriber = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let store = &f.store;
    let (id, dir) = (f.id, f.dir.path());
    let result = Pipeline {
        meeting_id: id,
        store,
        meetings_dir: dir,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(move |p| {
            // The user deletes the meeting (and its sidecars) while it runs.
            if p.stage == ProcessingStage::Transcribe && store.delete_meeting(id).unwrap() {
                sidecar::delete_sidecars(dir, store, id).unwrap();
            }
        }),
    }
    .run();
    assert!(result.is_err());
    assert!(!sidecar::has_cache(store, id).unwrap());
    assert!(!sidecar::has_embeddings(store, id).unwrap());
}

/// A cancelled run whose status was already put back (a reprocess:
/// `Processed`, stage `done`) stops without moving the stage or writing a
/// checkpoint.
#[test]
fn a_run_no_longer_processing_saves_nothing() {
    let f = fixture();
    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Done))
        .unwrap();
    let cancel = AtomicBool::new(false);
    let mut transcriber = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let store = &f.store;
    let (id, dir) = (f.id, f.dir.path());
    let result = Pipeline {
        meeting_id: id,
        store,
        meetings_dir: dir,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(move |p| {
            // Restored (as `Processor::finish` does) while it is in VAD.
            if p.stage == ProcessingStage::Vad {
                store
                    .set_meeting_status(id, MeetingStatus::Processed, None)
                    .unwrap();
                store
                    .set_processing_stage(id, Some(ProcessingStage::Done))
                    .unwrap();
                sidecar::delete_cache(store, id).unwrap();
            }
        }),
    }
    .run();
    assert!(super::super::is_cancelled(&result.unwrap_err()));
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    assert!(!sidecar::has_cache(store, id).unwrap());
}

#[test]
fn mic_only_meeting_has_just_the_user() {
    let f = fixture();
    let cancel = AtomicBool::new(false);
    let calls = Rc::new(Cell::new(0));
    let mut t = FakeTranscriber {
        calls: calls.clone(),
        cancel_after: None,
        accented: None,
    };
    let mut e = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(
            10_000,
            &[(USER, 1000, 4000)],
        )))),
        system: None,
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut t,
        embedder: &mut e,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(|_| {}),
    }
    .run()
    .unwrap();
    let speakers = f.store.speakers(f.id).unwrap();
    assert_eq!(speakers.len(), 1);
    assert_eq!(speakers[0].cluster_label, "me");
    assert_eq!(e.calls.get(), 0, "no speaker model needed");
}

#[test]
fn no_audio_is_an_error() {
    let f = fixture();
    let cancel = AtomicBool::new(false);
    let mut t = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut e = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let err = Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: None,
        system: None,
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut t,
        embedder: &mut e,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(|_| {}),
    }
    .run()
    .unwrap_err();
    assert!(err.to_string().contains("no recorded audio"));
}

/// A cancel that reaches the run once it is merging lets it complete: the
/// transcript and its voiceprints sidecar are stored together.
#[test]
fn a_cancel_during_the_merge_lets_it_complete() {
    let f = fixture();
    let cancel = AtomicBool::new(false);
    let mut transcriber = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let flag = &cancel;
    Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(move |p| {
            if p.stage == ProcessingStage::Merge {
                flag.store(true, Ordering::SeqCst);
            }
        }),
    }
    .run()
    .unwrap();
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    let ids: Vec<i64> = f
        .store
        .segments(f.id)
        .unwrap()
        .iter()
        .map(|s| s.id)
        .collect();
    let embeddings = sidecar::read_embeddings(&f.store, f.id).unwrap().unwrap();
    assert!(!embeddings.is_empty());
    assert!(embeddings.iter().all(|e| ids.contains(&e.segment_id)));
}

/// A merge failing after it replaced the transcript drops the old
/// voiceprints sidecar (keyed by the deleted segments) instead of leaving
/// it next to the new transcript.
#[test]
fn a_merge_failing_after_the_transcript_drops_the_old_sidecar() {
    let f = fixture();
    let stopped = run_hooked(&f, |p| {
        if p.stage == ProcessingStage::Merge {
            f.store
                .set_meeting_status(f.id, MeetingStatus::Recorded, None)
                .unwrap();
        }
    });
    assert!(super::super::is_cancelled(&stopped.unwrap_err()));
    let stale = SegmentEmbedding {
        segment_id: 999_999,
        duration_ms: 1000,
        embedding: vec![1.0],
    };
    sidecar::write_embeddings(&f.store, f.id, &[stale]).unwrap();
    f.store
        .set_meeting_status(f.id, MeetingStatus::Processing, None)
        .unwrap();
    f.store
        .execute_batch_for_tests(
            "CREATE TEMP TRIGGER fail_centroid BEFORE UPDATE OF embedding ON meeting_speakers
             BEGIN SELECT RAISE(ABORT, 'boom'); END",
        )
        .unwrap();

    assert!(run(&f, None).result.is_err());
    assert_eq!(
        f.store.segments(f.id).unwrap().len(),
        7,
        "transcript stored"
    );
    assert!(!sidecar::has_embeddings(&f.store, f.id).unwrap());
}

#[test]
fn segments_in_another_script_are_transcribed_again_in_the_meetings_language() {
    let f = fixture();
    // Speaker 1 (two system turns and their mic echo) comes out in
    // Devanagari; everyone else in English.
    let r = run_accented(&f, None, Some(1), &mic_turns(), &system_turns());
    r.result.unwrap();
    // 8 units, then speaker 1's three again.
    assert_eq!(r.transcribe_calls, 11);
    let segments = f.store.segments(f.id).unwrap();
    assert!(
        segments.iter().all(|s| s.text.is_ascii()),
        "{:?}",
        segments.iter().map(|s| &s.text).collect::<Vec<_>>()
    );
    // Back in English, the echo is recognised and dropped again.
    assert_eq!(segments.len(), 7);
}

#[test]
fn the_main_language_needs_enough_words_and_a_clear_lead() {
    let seg = |l: &str, n| (Some(l.to_string()), n);
    assert_eq!(
        main_language(&[seg("en", 25), seg("hi", 15), (None, 50)], None).as_deref(),
        Some("en")
    );
    // A clear leader wins over the preferred language.
    assert_eq!(
        main_language(&[seg("en", 25), seg("hi", 75)], Some("en")).as_deref(),
        Some("hi")
    );
    // Too few words to tell.
    assert_eq!(main_language(&[seg("en", 10)], Some("en")), None);
    // Without a clear leader: the preferred language, if it has a fair share.
    let even = [seg("en", 69), seg("hi", 80)];
    assert_eq!(main_language(&even, None), None);
    assert_eq!(main_language(&even, Some("en-US")).as_deref(), Some("en"));
    assert_eq!(main_language(&even, Some("fr")), None);
    // Mixed-script segments don't vote.
    assert_eq!(
        main_language(&[seg("hi", 30), seg(MIXED_LANGUAGE, 100)], None).as_deref(),
        Some("hi")
    );
}

#[test]
fn a_segment_in_two_scripts_is_mixed() {
    let langs: Vec<String> = vec!["en-US".into(), "hi-IN".into(), "ja-JP".into()];
    assert_eq!(
        segment_language(
            "ओके सो कंट्री ऑल्सो दिस टू. They were not appearing for you in the data.",
            &langs
        )
        .as_deref(),
        Some(MIXED_LANGUAGE)
    );
    assert_eq!(
        segment_language(
            "ओके एस ओ करंटली ऑल्सो दिस टू डी एस आर नॉट प्रेजेंट फॉर द टू डी एस आर",
            &langs
        )
        .as_deref(),
        Some("hi")
    );
    assert_eq!(
        segment_language(
            "Okay, so currently also these two DSRs are not present for the two DSRs",
            &langs
        )
        .as_deref(),
        Some("en")
    );
    // Kana and kanji are one script.
    assert_ne!(
        segment_language("今日はとても良い天気ですね、カフェに行きましょう", &langs).as_deref(),
        Some(MIXED_LANGUAGE)
    );
    assert_eq!(segment_language("", &langs), None);
}

#[test]
fn provisional_lines_are_emitted_once_per_unit_in_time_order() {
    let f = fixture();
    // Mic, then a system turn that starts in between, then mic again.
    // Storage is mic-then-system; the announcements must follow the clock.
    let mic = [(0usize, 0, 2_000), (0, 8_000, 10_000)];
    let system = [(1, 3_000, 5_000)];
    let r = run_tracks(&f, None, &mic, &system);
    r.result.unwrap();
    assert_eq!(r.lines.len(), 3, "{:?}", r.lines);
    let starts: Vec<i64> = r.lines.iter().map(|l| l.t0_ms).collect();
    assert!(
        starts.windows(2).all(|w| w[0] <= w[1]),
        "not in time order: {starts:?}"
    );
    assert_eq!(
        r.lines.iter().map(|l| l.track).collect::<Vec<_>>(),
        vec![Track::Mic, Track::System, Track::Mic]
    );
    let mut spans: Vec<_> = r
        .lines
        .iter()
        .map(|l| (l.t0_ms, l.t1_ms, l.track.as_str()))
        .collect();
    let n = spans.len();
    spans.sort();
    spans.dedup();
    assert_eq!(spans.len(), n);
    assert!(r.lines.iter().all(|l| !l.text.is_empty()));
}

#[test]
fn batches_group_pending_units_of_one_track_up_to_the_limit() {
    let unit = |track, t0, t1| Unit {
        track,
        span: vad::Span::new(t0, t1),
    };
    let units = [
        unit(Track::Mic, 0, 3000),
        unit(Track::Mic, 4000, 7000),
        unit(Track::Mic, 8000, 9000),
        unit(Track::System, 0, 2000),
        unit(Track::System, 3000, 20000),
        unit(Track::System, 21000, 22000),
    ];
    let all = [true; 6];
    // Storage order is mic, then system. Interleaved start times stay in
    // the same per-track group, up to the limit.
    assert_eq!(
        batches(&units, &all, 7000),
        vec![vec![0, 1, 2], vec![3], vec![4], vec![5]]
    );
    assert_eq!(
        batches(&units, &all, 6000),
        vec![vec![0, 1], vec![2], vec![3], vec![4], vec![5]]
    );
    // Done units are skipped (a resumed run).
    let pending = [false, true, true, false, false, true];
    assert_eq!(batches(&units, &pending, 60_000), vec![vec![1, 2], vec![5]]);
    assert!(batches(&units, &[false; 6], 60_000).is_empty());
}

/// [`FakeTranscriber`] behind the batch interface: it records the audio
/// length of each segment in each call.
struct BatchingTranscriber<'c> {
    inner: FakeTranscriber<'c>,
    limit_ms: i64,
    calls: Rc<std::cell::RefCell<Vec<Vec<i64>>>>,
}

impl SegmentTranscriber for BatchingTranscriber<'_> {
    fn transcribe(&mut self, _pcm: Vec<f32>) -> Result<(String, Vec<Word>)> {
        panic!("a batching transcriber gets batches");
    }

    fn batch_limit_ms(&self) -> Option<i64> {
        Some(self.limit_ms)
    }

    fn transcribe_batch(&mut self, pcms: Vec<Vec<f32>>) -> Result<Vec<(String, Vec<Word>)>> {
        self.calls.borrow_mut().push(
            pcms.iter()
                .map(|p| (p.len() * 1000 / SAMPLE_RATE) as i64)
                .collect(),
        );
        pcms.into_iter().map(|p| self.inner.say(p, None)).collect()
    }
}

#[test]
fn a_batching_transcriber_gets_same_track_groups_and_results_land_on_their_units() {
    // The same meeting, one segment at a time.
    let reference = fixture();
    run(&reference, None).result.unwrap();

    let f = fixture();
    let cancel = AtomicBool::new(false);
    let calls = Rc::new(std::cell::RefCell::new(Vec::new()));
    let limit_ms = 7_000;
    let mut transcriber = BatchingTranscriber {
        inner: FakeTranscriber {
            calls: Rc::new(Cell::new(0)),
            cancel_after: None,
            accented: None,
        },
        limit_ms,
        calls: calls.clone(),
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    let progress = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = progress.clone();
    Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig {
            user_label: "Imran".into(),
            preferred_language: Some("en".into()),
            ..PipelineConfig::default()
        },
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(move |p| sink.borrow_mut().push(p)),
    }
    .run()
    .unwrap();

    let calls = calls.borrow().clone();
    // Storage order, one track per call, 7000 ms cap: all of the mic, then
    // all of the system, even though the turns interleave on the clock.
    let units: Vec<usize> = calls.iter().map(Vec::len).collect();
    assert_eq!(units, vec![2, 2, 2, 2], "{calls:?}");
    for call in &calls {
        let total: i64 = call.iter().sum();
        assert!(total <= limit_ms, "{call:?}");
    }
    // One progress report per batch.
    let transcribing = progress
        .borrow()
        .iter()
        .filter(|p| {
            p.stage == ProcessingStage::Transcribe
                && p.fraction > overall_fraction(ProcessingStage::Transcribe, 0.0)
        })
        .count();
    assert_eq!(transcribing, calls.len());

    // Same transcript as one segment at a time: texts, times and words.
    let strip = |segments: Vec<Segment>| -> Vec<(Track, i64, i64, String, Vec<Word>)> {
        segments
            .into_iter()
            .map(|s| (s.track, s.t0_ms, s.t1_ms, s.text, s.words))
            .collect()
    };
    let batched = strip(f.store.segments(f.id).unwrap());
    assert_eq!(
        batched,
        strip(reference.store.segments(reference.id).unwrap())
    );
    assert!(!batched.is_empty());
    // Words are in meeting time, inside their own segment.
    for (_, t0, t1, _, words) in &batched {
        assert!(words.iter().all(|w| w.t0_ms >= *t0 && w.t1_ms <= *t1));
    }
}

/// Four system segments; their standard voiceprints all look alike.
fn one_voice_cache() -> ProcessingCache {
    let mut cache = ProcessingCache::new();
    cache.system_ms = Some(20_000);
    cache.units = (0..4)
        .map(|i| Unit {
            track: Track::System,
            span: vad::Span {
                t0_ms: i * 5_000,
                t1_ms: i * 5_000 + 4_000,
            },
        })
        .collect();
    cache.transcripts = (0..4)
        .map(|i| {
            Some(UnitTranscript {
                text: format!("line {i}"),
                words: Vec::new(),
            })
        })
        .collect();
    cache.embeddings = (0..4)
        .map(|i| Some(Some(vec![1.0, 0.01 * i as f32, 0.0])))
        .collect();
    cache
}

#[test]
fn a_diarization_service_decides_the_system_speakers() {
    let cache = one_voice_cache();
    let config = PipelineConfig::default();
    // The standard voiceprints say one speaker...
    let c = cluster_units(&cache, &config, &[], &[]);
    assert_eq!(c.labels.iter().flatten().max(), Some(&0));
    // ...the service hears two, and isn't sure about the last line.
    let source = ClusterSource::SystemLabels(vec![Some(4), Some(7), Some(4), None]);
    let c = cluster_units_with(&cache, &config, &[], &[], &source);
    assert_eq!(c.labels, vec![Some(0), Some(1), Some(0), None]);
    // Centroids still come from the standard voiceprints (for matching).
    assert_eq!(c.centroids.len(), 2);
    assert!(c.centroids.iter().all(Option::is_some));
}

#[test]
fn a_better_models_voiceprints_decide_the_clusters() {
    let cache = one_voice_cache();
    let config = PipelineConfig::default();
    // The accurate model tells lines 1 and 3 apart from 0 and 2.
    let a = Some(vec![1.0, 0.0, 0.0]);
    let b = Some(vec![0.0, 0.0, 1.0]);
    let source = ClusterSource::Vectors(vec![a.clone(), b.clone(), a, b]);
    let c = cluster_units_with(&cache, &config, &[], &[], &source);
    assert_eq!(c.labels, vec![Some(0), Some(1), Some(0), Some(1)]);
    // A line it can't embed is left to the timing rule: here, a second
    // from the same speaker on both sides.
    let source = ClusterSource::Vectors(vec![
        Some(vec![1.0, 0.0, 0.0]),
        None,
        Some(vec![1.0, 0.0, 0.0]),
        Some(vec![1.0, 0.01, 0.0]),
    ]);
    let c = cluster_units_with(&cache, &config, &[], &[], &source);
    assert_eq!(c.labels[1], Some(0));
}

/// A line only the standard model could embed, named by its voice as someone
/// no cluster matches, is all its new cluster holds. The timing rule, run
/// again after the move, must not take it back out.
#[test]
fn a_moved_line_without_a_better_voiceprint_keeps_its_new_cluster() {
    let mut cache = two_close_voices_cache();
    let (a, b) = (vec![1.0, 0.0, 0.0], vec![0.8, 0.6, 0.0]);
    cache.embeddings = [&a, &b, &a, &a]
        .iter()
        .map(|v| Some(Some((*v).clone())))
        .collect();
    let source = ClusterSource::Vectors(vec![Some(a.clone()), None, Some(a.clone()), Some(a)]);
    let known = [
        KnownVoice {
            person_id: 7,
            prints: vec![vec![1.0, 0.1, 0.0], vec![1.0, -0.1, 0.05]],
        },
        KnownVoice {
            person_id: 9,
            prints: vec![vec![0.8, 0.6, 0.1], vec![0.75, 0.65, -0.05]],
        },
    ];
    let c = cluster_units_with(&cache, &PipelineConfig::default(), &known, &[], &source);
    assert_eq!(c.voices, vec![Some(7), Some(9), Some(7), Some(7)]);
    let line_1 = c.labels[1].unwrap();
    assert_ne!(c.labels[0], Some(line_1));
    assert_eq!(c.matches[line_1].person_id, Some(9));
}

/// A checkpoint of these `(track, t0_ms, t1_ms, voiceprint)` lines, all with
/// speech.
fn cache_of(lines: &[(Track, i64, i64, Option<Vec<f32>>)]) -> ProcessingCache {
    let mut cache = ProcessingCache::new();
    cache.units = lines
        .iter()
        .map(|(track, t0_ms, t1_ms, _)| Unit {
            track: *track,
            span: vad::Span {
                t0_ms: *t0_ms,
                t1_ms: *t1_ms,
            },
        })
        .collect();
    cache.transcripts = (0..lines.len())
        .map(|i| {
            Some(UnitTranscript {
                text: format!("line {i}"),
                words: Vec::new(),
            })
        })
        .collect();
    cache.embeddings = lines.iter().map(|l| Some(l.3.clone())).collect();
    cache
}

/// Axis `k` of a 4-d voiceprint.
fn axis(k: usize) -> Vec<f32> {
    let mut v = vec![0f32; 4];
    v[k] = 1.0;
    v
}

#[test]
fn the_tracks_are_clustered_apart() {
    use Track::{Mic, System};
    let cache = cache_of(&[
        (Mic, 0, 4_000, Some(axis(0))),
        (Mic, 5_000, 9_000, Some(axis(0))),
        (System, 10_000, 14_000, Some(axis(1))),
        (System, 15_000, 19_000, Some(axis(2))),
    ]);
    let c = cluster_units(&cache, &PipelineConfig::default(), &[], &[axis(3)]);
    assert_eq!(c.labels, vec![Some(0), Some(0), Some(1), Some(2)]);
}

#[test]
fn a_user_like_mic_line_never_joins_a_system_cluster() {
    use Track::{Mic, System};
    // The same voice on both tracks, enough of it on the mic to be a speaker.
    let cache = cache_of(&[
        (Mic, 0, 4_000, Some(axis(0))),
        (Mic, 5_000, 9_000, Some(axis(0))),
        (System, 10_000, 14_000, Some(axis(0))),
        (System, 15_000, 19_000, Some(axis(0))),
    ]);
    let c = cluster_units(&cache, &PipelineConfig::default(), &[], &[axis(3)]);
    assert_eq!(c.labels, vec![Some(0), Some(0), Some(1), Some(1)]);
    // Too little of it on the mic: the user's, with no system cluster to join.
    let cache = cache_of(&[
        (Mic, 0, 4_000, Some(axis(0))),
        (System, 10_000, 14_000, Some(axis(0))),
        (System, 15_000, 19_000, Some(axis(0))),
    ]);
    let c = cluster_units(&cache, &PipelineConfig::default(), &[], &[axis(3)]);
    assert_eq!(c.labels, vec![None, Some(0), Some(0)]);
    // Sounding like the user: never clustered at all, however much of it.
    let cache = cache_of(&[
        (Mic, 0, 9_000, Some(axis(3))),
        (Mic, 10_000, 19_000, Some(axis(3))),
    ]);
    let c = cluster_units(&cache, &PipelineConfig::default(), &[], &[axis(3)]);
    assert_eq!(c.labels, vec![None, None]);
}

#[test]
fn echo_units_are_left_out_of_clustering_and_the_transcript() {
    use Track::{Mic, System};
    let mut cache = cache_of(&[
        (Mic, 0, 9_000, Some(axis(0))),
        (Mic, 20_000, 29_000, Some(axis(0))),
        (System, 0, 9_000, Some(axis(1))),
        (System, 10_000, 19_000, Some(axis(1))),
        (System, 20_000, 29_000, Some(axis(1))),
    ]);
    let say = |text: &str| {
        Some(UnitTranscript {
            text: text.into(),
            words: Vec::new(),
        })
    };
    cache.transcripts = vec![
        say("let's ship the release on friday"),
        say("what about the budget though"),
        say("let's ship the release on friday"),
        say("something else entirely here"),
        say("we can look at numbers next"),
    ];
    let config = PipelineConfig::default();
    // Line 1's 9 s alone is enough for a mic speaker; line 0 would add to it.
    let c = cluster_units(&cache, &config, &[], &[axis(3)]);
    assert_eq!(c.labels[0], None, "an echo is nobody's");
    assert!(c.labels[1].is_some());
    assert_ne!(c.labels[1], c.labels[2]);
    assert_eq!(c.voices[0], None);
    // The merge drops the same line, and only it.
    let (_, segments, units) = build_transcript(&cache, &c, &config);
    assert_eq!(segments.len(), 4);
    assert!(!units.contains(&0));
    // A mic line no longer an echo counts again: 18 s in one cluster.
    cache.transcripts[0] = say("completely different words said here");
    let c = cluster_units(&cache, &config, &[], &[axis(3)]);
    assert!(c.labels[0].is_some());
    assert_eq!(c.labels[0], c.labels[1]);
}

/// A line of `words`, each 500 ms from `t0`.
fn spoken(t0: i64, words: &[&str]) -> UnitTranscript {
    UnitTranscript {
        text: words.join(" "),
        words: words
            .iter()
            .enumerate()
            .map(|(i, w)| Word {
                t0_ms: t0 + i as i64 * 500,
                t1_ms: t0 + (i as i64 + 1) * 500,
                text: w.to_string(),
            })
            .collect(),
    }
}

#[test]
fn a_unit_with_echo_taken_out_stays_the_users_and_points_at_its_unit() {
    use Track::{Mic, System};
    // Bleed, then the user's own words, on one mic unit. Its voiceprint is
    // mostly the remote voice, unlike the user's: it must not become a guest.
    let mut cache = cache_of(&[
        (Mic, 0, 9_000, Some(axis(1))),
        (Mic, 10_000, 19_000, Some(axis(0))),
        (System, 0, 4_000, Some(axis(1))),
        (System, 20_000, 29_000, Some(axis(1))),
    ]);
    let bleed = ["let's", "ship", "the", "release", "friday"];
    let mut mixed = spoken(0, &bleed);
    let mine = spoken(7_000, &["sorry", "my", "line", "dropped"]);
    mixed.text = format!("{} {}", mixed.text, mine.text);
    mixed.words.extend(mine.words);
    cache.transcripts = vec![
        Some(mixed),
        Some(spoken(10_000, &["what", "about", "the", "budget"])),
        Some(spoken(0, &bleed)),
        Some(spoken(20_000, &["we", "can", "look", "next"])),
    ];
    let config = PipelineConfig::default();
    let c = cluster_units(&cache, &config, &[], &[axis(3)]);
    assert_eq!(c.labels[0], None);
    let (speakers, segments, units) = build_transcript(&cache, &c, &config);
    let user = speakers.iter().find(|s| s.is_user).unwrap().id;
    let kept = segments
        .iter()
        .find(|s| s.track == Mic && s.t0_ms < 10_000)
        .unwrap();
    assert_eq!(kept.text, "sorry my line dropped");
    assert_eq!((kept.t0_ms, kept.t1_ms), (7_000, 9_000));
    assert_eq!(kept.speaker_id, Some(user));
    let at = segments.iter().position(|s| s == kept).unwrap();
    assert_eq!(units[at], 0, "still knows which unit it came from");
    assert!(units.iter().all(|&u| u < cache.units.len()));
}

#[test]
fn redoing_the_speakers_takes_no_words_from_a_stored_mic_line() {
    use Track::{Mic, System};
    // The user repeated the remote's phrase just after its echo came out.
    let phrase = ["ship", "it", "now"];
    let mut cache = cache_of(&[
        (Mic, 0, 1_500, Some(axis(0))),
        (System, 0, 1_500, Some(axis(1))),
    ]);
    cache.transcripts = vec![Some(spoken(0, &phrase)), Some(spoken(0, &phrase))];
    let config = PipelineConfig::default();
    let c = cluster_units(&cache, &config, &[], &[]);
    let mic_lines = |cache: &ProcessingCache| {
        let (_, segments, _) = build_transcript(cache, &c, &config);
        segments.iter().filter(|s| s.track == Mic).count()
    };
    assert_eq!(mic_lines(&cache), 0, "a first run takes the echo out");
    cache.echo_removed = true;
    assert_eq!(mic_lines(&cache), 1, "a redo goes by the stored transcript");
    assert!(!labels_echo_units(&cache, &c, &config.echo));
}

#[test]
fn a_checkpoint_that_labelled_an_echo_unit_is_clustered_again() {
    use Track::{Mic, System};
    let mut cache = cache_of(&[
        (Mic, 0, 9_000, Some(axis(1))),
        (System, 0, 9_000, Some(axis(1))),
    ]);
    let phrase = ["ship", "it", "now"];
    cache.transcripts = vec![Some(spoken(0, &phrase)), Some(spoken(0, &phrase))];
    let echo = EchoParams::default();
    let mut c = cluster_units(&cache, &PipelineConfig::default(), &[], &[axis(3)]);
    assert!(!labels_echo_units(&cache, &c, &echo), "fresh clustering");
    c.labels[0] = Some(1);
    assert!(labels_echo_units(&cache, &c, &echo), "an old checkpoint");
}

#[test]
fn a_checkpoint_from_before_the_echo_flag_goes_over_its_echo() {
    let mut json: serde_json::Value = serde_json::to_value(ProcessingCache::new()).unwrap();
    json.as_object_mut().unwrap().remove("echo_removed");
    let old: ProcessingCache = serde_json::from_value(json).unwrap();
    assert!(!old.echo_removed);
}

#[test]
fn without_a_recorded_voice_every_mic_line_is_the_users() {
    use Track::Mic;
    let cache = cache_of(&[
        (Mic, 0, 9_000, Some(axis(0))),
        (Mic, 10_000, 19_000, Some(axis(1))),
    ]);
    let c = cluster_units(&cache, &PipelineConfig::default(), &[], &[]);
    assert_eq!(c.labels, vec![None, None]);
}

#[test]
fn only_a_known_person_joins_the_tracks() {
    use Track::{Mic, System};
    let cache = cache_of(&[
        (Mic, 0, 5_000, Some(axis(0))),
        (Mic, 6_000, 11_000, Some(axis(0))),
        (System, 12_000, 16_000, Some(axis(0))),
        (System, 17_000, 21_000, Some(axis(0))),
    ]);
    let known = [KnownVoice {
        person_id: 3,
        prints: vec![vec![1.0, 0.1, 0.0, 0.0], vec![1.0, -0.1, 0.0, 0.0]],
    }];
    let c = cluster_units(&cache, &PipelineConfig::default(), &known, &[axis(3)]);
    assert_eq!(c.voices, vec![Some(3); 4]);
    assert!(c.labels[0].is_some());
    assert!(c.labels.iter().all(|l| *l == c.labels[0]), "{:?}", c.labels);
    assert_eq!(c.matches[c.labels[0].unwrap()].person_id, Some(3));
}

#[test]
fn a_diarization_service_keeps_the_speakers_in_the_room() {
    use Track::{Mic, System};
    let cache = cache_of(&[
        (Mic, 0, 5_000, Some(axis(0))),
        (Mic, 6_000, 11_000, Some(axis(0))),
        (Mic, 12_000, 14_000, Some(axis(3))),
        (System, 15_000, 19_000, None),
        (System, 20_000, 24_000, None),
    ]);
    // The service's labels have holes; the mic's must clear the highest.
    let source = ClusterSource::SystemLabels(vec![None, None, None, Some(0), Some(7)]);
    let c = cluster_units_with(&cache, &PipelineConfig::default(), &[], &[axis(3)], &source);
    let labels = &c.labels;
    assert_eq!(labels[0], labels[1]);
    assert!(labels[0].is_some());
    assert_eq!(labels[2], None, "the user's own voice");
    assert!(labels[3].is_some() && labels[4].is_some());
    assert_ne!(labels[3], labels[4]);
    assert_ne!(labels[0], labels[3]);
    assert_ne!(labels[0], labels[4]);
    assert_eq!(c.centroids.len(), 3);
}

#[test]
fn short_speech_follows_a_line_placed_only_by_its_own_voice() {
    use Track::System;
    let (a, b, c) = (axis(0), axis(1), axis(2));
    let lines = [
        (System, 0, 4_000, Some(a.clone())),
        (System, 8_000, 12_000, Some(b.clone())),
        (System, 16_000, 20_000, Some(a.clone())),
        (System, 24_000, 28_000, Some(b.clone())),
        (System, 40_000, 42_000, Some(c)),
        (System, 42_100, 42_600, None),
    ];
    let cache = cache_of(&lines);
    // The better model hears line 4 as equally near both clusters.
    let source = ClusterSource::Vectors(vec![
        Some(a.clone()),
        Some(b.clone()),
        Some(a),
        Some(b),
        Some(vec![1.0, 1.0, 0.0, 0.0]),
        None,
    ]);
    let known = [KnownVoice {
        person_id: 5,
        prints: vec![vec![0.1, 0.0, 1.0, 0.0], vec![-0.1, 0.0, 1.0, 0.0]],
    }];
    let c = cluster_units_with(&cache, &PipelineConfig::default(), &known, &[], &source);
    assert_eq!(c.voices[4], Some(5));
    assert!(c.labels[4].is_some());
    assert_eq!(c.labels[5], c.labels[4]);
}

/// A meeting of [`system_turns`] with this separation model.
fn run_with_separation_model(f: &Fixture, model: &mut dyn Embedder) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut transcriber = FakeTranscriber {
        calls: Rc::new(Cell::new(0)),
        cancel_after: None,
        accented: None,
    };
    let mut embedder = FakeEmbedder {
        calls: Rc::new(Cell::new(0)),
    };
    Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        config: PipelineConfig::default(),
        mic: None,
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Diarizer::Model(model),
        turns: None,
        cancel: &cancel,
        on_line: None,
        on_progress: Box::new(|_| {}),
    }
    .run()
}

/// Fails on its `fail_on`th call (from 1), with what `error` makes.
struct FailingModel {
    calls: usize,
    fail_on: usize,
    error: fn() -> anyhow::Error,
}

impl Embedder for FailingModel {
    fn embed(&mut self, _: &[f32]) -> Result<Option<Vec<f32>>> {
        self.calls += 1;
        if self.calls == self.fail_on {
            return Err((self.error)());
        }
        // Would put every line in one cluster, if it were ever used.
        Ok(Some(vec![1.0; 16]))
    }
}

#[test]
fn a_failed_separation_model_falls_back_to_the_standard_voiceprints() {
    // Failing part-way drops its earlier vectors too.
    for fail_on in [1, 3] {
        let f = fixture();
        let mut model = FailingModel {
            calls: 0,
            fail_on,
            error: || anyhow::anyhow!("corrupt model"),
        };
        run_with_separation_model(&f, &mut model).unwrap();
        assert_eq!(model.calls, fail_on, "stops at the first failure");
        let segments = f.store.segments(f.id).unwrap();
        let speaker = |ms: i64| {
            let at = |s: &&Segment| s.t0_ms <= ms && ms < s.t1_ms;
            segments.iter().find(at).unwrap().speaker_id
        };
        assert_eq!(speaker(1000), speaker(13000));
        assert_eq!(speaker(5000), speaker(17000));
        assert_eq!(speaker(9000), speaker(21000));
        assert_ne!(speaker(1000), speaker(5000));
        assert_ne!(speaker(5000), speaker(9000));
    }
}

#[test]
fn a_cancelled_separation_model_cancels_the_run() {
    let f = fixture();
    let mut model = FailingModel {
        calls: 0,
        fail_on: 1,
        error: || Cancelled.into(),
    };
    let e = run_with_separation_model(&f, &mut model).unwrap_err();
    assert!(crate::meetings::processing::is_cancelled(&e), "{e:#}");
    assert!(f.store.segments(f.id).unwrap().is_empty());
}

#[test]
fn a_cancelled_model_download_stops_the_run_rather_than_falling_back() {
    let f = fixture();
    let mut model = FailingModel {
        calls: 0,
        fail_on: 1,
        error: || crate::downloads::DownloadCancelled.into(),
    };
    let e = run_with_separation_model(&f, &mut model).unwrap_err();
    assert!(e.is::<crate::downloads::DownloadCancelled>(), "{e:#}");
    assert!(f.store.segments(f.id).unwrap().is_empty());
}

/// What the user said (a speaker named, a line tagged) survives running
/// the meeting again: the model only decides the lines nobody pinned.
#[test]
fn a_rerun_keeps_the_names_and_tags_the_user_gave() {
    let f = fixture();
    let alice = named_alice(&f);
    let alice_speaker = speaker_1(&f);
    let alice_lines: Vec<(Track, i64, i64)> = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .filter(|s| s.speaker_id == Some(alice_speaker.id))
        .map(|s| (s.track, s.t0_ms, s.t1_ms))
        .collect();
    // One line of another speaker, tagged by hand as Bob.
    let line = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .find(|s| {
            s.speaker_id.is_some_and(|id| id != alice_speaker.id)
                && f.store
                    .get_speaker(s.speaker_id.unwrap())
                    .unwrap()
                    .is_some_and(|s| !s.is_user)
        })
        .unwrap();
    let bob = naming::assign_segment(
        &f.store,
        line.id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: None,
        },
    )
    .unwrap();

    run(&f, None).result.unwrap();

    let speakers = f.store.speakers(f.id).unwrap();
    let person_of = |track: Track, t0: i64, t1: i64| {
        let seg = f
            .store
            .segments(f.id)
            .unwrap()
            .into_iter()
            .find(|s| (s.track, s.t0_ms, s.t1_ms) == (track, t0, t1))
            .unwrap();
        let sp = speakers
            .iter()
            .find(|s| Some(s.id) == seg.speaker_id)
            .unwrap();
        (sp.person_id, sp.confirmed)
    };
    for &(track, t0, t1) in &alice_lines {
        assert_eq!(person_of(track, t0, t1), (Some(alice), true));
    }
    assert_eq!(
        person_of(line.track, line.t0_ms, line.t1_ms),
        (Some(bob), true)
    );
    // The pins are still there for the next one.
    let pins = f.store.segment_pins(f.id).unwrap();
    assert_eq!(pins.len(), alice_lines.len() + 1);

    // Merging Bob into Alice moves his pin with him.
    f.store.merge_people(bob, alice).unwrap();
    assert!(f
        .store
        .segment_pins(f.id)
        .unwrap()
        .iter()
        .all(|p| p.person_id == alice));
}

#[test]
fn pins_override_the_model_and_steer_its_guesses() {
    let speaker = |id: SpeakerId, person: Option<PersonId>, confirmed: bool| MeetingSpeaker {
        id,
        meeting_id: 0,
        cluster_label: format!("Speaker {id}"),
        person_id: person,
        match_confidence: person.map(|_| 0.6),
        confirmed,
        is_user: false,
    };
    let seg = |t0_ms: i64, speaker_id: Option<SpeakerId>| Segment {
        id: 0,
        meeting_id: 0,
        track: Track::System,
        t0_ms,
        t1_ms: t0_ms + 1_000,
        text: String::new(),
        words: Vec::new(),
        speaker_id,
    };
    let pin = |t0_ms: i64, t1_ms: i64, person_id: PersonId| SegmentPin {
        track: Track::System,
        t0_ms,
        t1_ms,
        person_id,
        by_hand: false,
    };
    let mut built = (
        vec![
            speaker(1, Some(7), true),
            speaker(2, None, false),
            speaker(3, Some(7), false),
        ],
        vec![
            seg(0, Some(1)),     // the model's 7, pinned to 9
            seg(2_000, Some(2)), // unknown voice, pinned to 9
            seg(4_000, Some(2)), // unpinned, same unnamed voice
            seg(6_000, Some(3)), // pinned to 7: the guess's only line
            seg(8_000, None),    // nobody, pinned to 7 (pin a bit off)
            seg(10_000, Some(1)),
        ],
        vec![0; 6],
    );
    let pins = [
        pin(0, 1_000, 9),
        pin(2_000, 3_000, 9),
        pin(6_000, 7_000, 7),
        pin(8_300, 9_200, 7),
    ];
    apply_pins(&mut built, &pins);
    let (speakers, segments, _) = &built;
    let person = |i: usize| {
        let sp = speakers
            .iter()
            .find(|s| Some(s.id) == segments[i].speaker_id)
            .unwrap();
        (sp.person_id, sp.confirmed)
    };
    assert_eq!(person(0), (Some(9), true));
    assert_eq!(person(1), (Some(9), true));
    assert_eq!(segments[0].speaker_id, segments[1].speaker_id);
    // Its only pinned line was 9's: the rest of that voice is suggested as 9.
    assert_eq!(person(2), (Some(9), false));
    assert_eq!(person(3), (Some(7), true));
    assert_eq!(segments[3].speaker_id, Some(1));
    assert_eq!(person(4), (Some(7), true));
    assert_eq!(person(5), (Some(7), true));
    // Speaker 3 lost its only line.
    assert!(speakers.iter().all(|s| s.id != 3));

    // A pin covering under half of a line doesn't decide it.
    let line = seg(0, None);
    assert!(pin_for(&line, &[pin(600, 2_000, 1)]).is_none());
    assert_eq!(
        pin_for(&line, &[pin(400, 2_000, 1)]).map(|p| p.person_id),
        Some(1)
    );
}

#[test]
fn each_voiceprint_leads_back_to_the_line_it_was_taken_from() {
    let f = fixture();
    let alice = named_alice(&f);
    let speaker = speaker_1(&f);
    let samples = naming::voice_samples(&f.store, alice).unwrap();
    assert_eq!(
        samples.len(),
        f.store.voiceprints_for_person(alice).unwrap().len()
    );
    assert!(!samples.is_empty());
    for s in &samples {
        assert_eq!(s.meeting_id, Some(f.id));
        let seg = s.segment.as_ref().expect("its line");
        assert_eq!(seg.speaker_id, Some(speaker.id));
    }
}

/// Two people whose voices the clustering can't tell apart (their lines
/// are closer than its merge cut) but their voiceprints can.
fn two_close_voices_cache() -> ProcessingCache {
    let mut cache = one_voice_cache();
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![0.8, 0.6, 0.0];
    cache.embeddings = [&a, &b, &a, &b]
        .iter()
        .map(|v| Some(Some((*v).clone())))
        .collect();
    cache
}

#[test]
fn the_voiceprints_of_people_named_split_a_merged_cluster() {
    let cache = two_close_voices_cache();
    let config = PipelineConfig::default();
    // On their own, the four lines are one speaker.
    let c = cluster_units(&cache, &config, &[], &[]);
    assert_eq!(c.labels, vec![Some(0); 4]);
    assert_eq!(c.voices, vec![None; 4]);

    let known = [
        KnownVoice {
            person_id: 7,
            prints: vec![vec![1.0, 0.1, 0.0], vec![1.0, -0.1, 0.05]],
        },
        KnownVoice {
            person_id: 9,
            prints: vec![vec![0.8, 0.6, 0.1], vec![0.75, 0.65, -0.05]],
        },
    ];
    let c = cluster_units(&cache, &config, &known, &[]);
    assert_eq!(c.voices, vec![Some(7), Some(9), Some(7), Some(9)]);
    assert_eq!(c.labels[0], c.labels[2]);
    assert_eq!(c.labels[1], c.labels[3]);
    assert_ne!(c.labels[0], c.labels[1]);
    let person = |i: usize| c.matches[c.labels[i].unwrap()];
    assert_eq!((person(0).person_id, person(0).confirmed), (Some(7), true));
    assert_eq!((person(1).person_id, person(1).confirmed), (Some(9), true));

    // A line a stored print was taken from doesn't vouch for itself: with
    // 9's only print being line 1's own, line 1 is too close to 7's to say.
    let known = [
        known[0].clone(),
        KnownVoice {
            person_id: 9,
            prints: vec![vec![0.8, 0.6, 0.0]],
        },
    ];
    let c = cluster_units(&cache, &config, &known, &[]);
    assert_eq!(c.voices[1], None);
}

/// A line's turn is `secs` long: the fake voiceprint of each length is a
/// little different, so the two turns of a speaker aren't the same print.
fn turns_of_two_voices() -> Vec<(usize, i64, i64)> {
    vec![
        (0, 1_000, 4_000),
        (1, 5_000, 8_000),
        (0, 13_000, 16_250),
        (1, 18_000, 21_250),
    ]
}

/// The system line spoken at `ms`.
fn line_at(f: &Fixture, ms: i64) -> Segment {
    f.store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .find(|s| s.track == Track::System && s.t0_ms <= ms && ms < s.t1_ms)
        .unwrap()
}

fn person_at(f: &Fixture, t0_ms: i64) -> (Option<PersonId>, bool) {
    let line = line_at(f, t0_ms);
    let speaker = f
        .store
        .get_speaker(line.speaker_id.unwrap())
        .unwrap()
        .unwrap();
    (speaker.person_id, speaker.confirmed)
}

/// Naming a speaker that held two voices pins all its lines to one person;
/// once the other has voiceprints (a line tagged by hand), redoing the
/// speakers gives them the rest of their lines and takes their voice out
/// of the first person's voiceprints. Tagged lines stay as tagged.
#[test]
fn redoing_the_speakers_moves_a_named_speakers_lines_to_their_voice() {
    let f = fixture();
    let mic = [(USER, 26_000, 29_000)];
    run_tracks(&f, None, &mic, &turns_of_two_voices())
        .result
        .unwrap();
    let speaker = |t0_ms: i64| {
        let id = line_at(&f, t0_ms).speaker_id.unwrap();
        f.store.get_speaker(id).unwrap().unwrap()
    };
    let (first, second) = (speaker(1_000), speaker(5_000));
    assert_ne!(first.id, second.id);

    // As if both voices had been one speaker named Alice: every line is
    // pinned to her and she has both voices' prints.
    let alice = naming::assign(
        &f.store,
        first.id,
        PersonRef::New {
            display_name: "Alice".into(),
            email: None,
        },
    )
    .unwrap();
    let theirs: Vec<Segment> = f
        .store
        .segments(f.id)
        .unwrap()
        .into_iter()
        .filter(|s| s.speaker_id == Some(second.id))
        .collect();
    f.store.pin_segments(f.id, &theirs, alice, false).unwrap();
    for p in naming::speaker_prints(&f.store, &second).unwrap() {
        f.store.add_voiceprint(alice, &p, Some(f.id)).unwrap();
    }
    assert_eq!(f.store.voiceprints_for_person(alice).unwrap().len(), 4);
    // One of the other voice's lines is tagged as Bob.
    let bob = naming::assign_segment(
        &f.store,
        line_at(&f, 18_000).id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: None,
        },
    )
    .unwrap();
    let wrong_print = naming::speaker_prints(&f.store, &speaker(5_000))
        .unwrap()
        .remove(0);

    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Done))
        .unwrap();
    run_tracks(&f, None, &mic, &turns_of_two_voices())
        .result
        .unwrap();

    assert_eq!(person_at(&f, 1_000), (Some(alice), true));
    assert_eq!(person_at(&f, 13_000), (Some(alice), true));
    assert_eq!(person_at(&f, 5_000), (Some(bob), true));
    assert_eq!(person_at(&f, 18_000), (Some(bob), true));
    assert_eq!(
        line_at(&f, 5_000).speaker_id,
        line_at(&f, 18_000).speaker_id
    );
    // Bob's other line was Alice's only by her speaker's name: its pin is
    // gone (the voice decides it now), and so is its print from hers.
    let pins = f.store.segment_pins(f.id).unwrap();
    let pin_at = |ms: i64| pins.iter().find(|p| p.t0_ms <= ms && ms < p.t1_ms);
    assert!(pin_at(5_000).is_none());
    let tagged = pin_at(18_000).unwrap();
    assert_eq!((tagged.person_id, tagged.by_hand), (bob, true));
    let prints = f.store.voiceprints_for_person(alice).unwrap();
    assert_eq!(prints.len(), 2);
    assert!(prints
        .iter()
        .all(|p| cluster::cosine(&p.embedding, &wrong_print) < cluster::SAME_PRINT_SIMILARITY));
}

/// A line tagged by hand keeps its person whatever its voice sounds like.
#[test]
fn a_line_tagged_by_hand_is_never_overruled_by_its_voice() {
    let f = fixture();
    let mic = [(USER, 26_000, 29_000)];
    run_tracks(&f, None, &mic, &turns_of_two_voices())
        .result
        .unwrap();
    let first = f
        .store
        .get_speaker(line_at(&f, 1_000).speaker_id.unwrap())
        .unwrap()
        .unwrap();
    let alice = naming::assign(
        &f.store,
        first.id,
        PersonRef::New {
            display_name: "Alice".into(),
            email: None,
        },
    )
    .unwrap();
    // Alice's voice, tagged as Bob (the user knows better).
    let bob = naming::assign_segment(
        &f.store,
        line_at(&f, 1_000).id,
        PersonRef::New {
            display_name: "Bob".into(),
            email: None,
        },
    )
    .unwrap();
    f.store
        .set_processing_stage(f.id, Some(ProcessingStage::Done))
        .unwrap();
    run_tracks(&f, None, &mic, &turns_of_two_voices())
        .result
        .unwrap();
    assert_eq!(person_at(&f, 1_000), (Some(bob), true));
    assert_eq!(person_at(&f, 13_000).0, Some(alice));
}

/// A pin overruled on one line still holds the others it covers.
#[test]
fn an_overruled_pin_keeps_the_lines_its_voice_agreed_with() {
    let seg = |id: i64, t0: i64, t1: i64| Segment {
        id,
        meeting_id: 1,
        track: Track::System,
        t0_ms: t0,
        t1_ms: t1,
        text: "x".into(),
        words: Vec::new(),
        speaker_id: None,
    };
    // One pin from naming a whole speaker; a redo split its stretch into
    // two lines, and only the second sounds like someone else.
    let pin = SegmentPin {
        track: Track::System,
        t0_ms: 0,
        t1_ms: 4_000,
        person_id: 7,
        by_hand: false,
    };
    let other = SegmentPin {
        track: Track::System,
        t0_ms: 9_000,
        t1_ms: 9_500,
        person_id: 8,
        by_hand: true,
    };
    let built = (
        Vec::new(),
        vec![seg(1, 0, 2_000), seg(2, 2_000, 4_000), seg(3, 9_000, 9_500)],
        vec![0, 1, 2],
    );
    let overruled = [Overruled {
        pin: 0,
        person_id: 7,
        unit: 1,
    }];
    let kept = without_overruled(&built, &[pin.clone(), other.clone()], &overruled);
    assert_eq!(
        kept,
        vec![
            SegmentPin {
                t0_ms: 0,
                t1_ms: 2_000,
                ..pin
            },
            other,
        ]
    );
}
