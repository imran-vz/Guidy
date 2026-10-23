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

const KEY: [u8; 32] = [42u8; 32];
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
    let store = MeetingsStore::open_in_memory(&KEY).unwrap();
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
    let result = Pipeline {
        meeting_id: f.id,
        store: &f.store,
        meetings_dir: f.dir.path(),
        key: KEY,
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
        cancel: &cancel,
        on_progress: Box::new(move |p| sink.borrow_mut().push(p)),
    }
    .run();
    let progress = progress.borrow().clone();
    Run {
        transcribe_calls: t_calls.get(),
        embed_calls: e_calls.get(),
        result,
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
        (5, 5000, 8000),
        (USER, 9000, 12000),
        (5, 13000, 16000),
        (6, 17000, 20000),
    ];
    let r = run_tracks(&f, None, &mic, &[]);
    r.result.unwrap();
    assert_eq!(r.embed_calls, 5);

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
fn processes_a_meeting_end_to_end() {
    let f = fixture();
    let r = run(&f, None);
    r.result.unwrap();
    // 6 system turns + 2 mic segments transcribed; 6 system voiceprints.
    assert_eq!(r.transcribe_calls, 8);
    assert_eq!(r.embed_calls, 6);
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    assert!(!sidecar::cache_path(f.dir.path(), f.id).exists());

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
    let prints: Vec<SegmentEmbedding> = sidecar::read_embeddings(f.dir.path(), f.id, &KEY)
        .unwrap()
        .unwrap();
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

#[test]
fn cancel_then_resume_skips_finished_work() {
    let f = fixture();
    let r = run(&f, Some(3));
    assert!(super::super::is_cancelled(&r.result.unwrap_err()));
    assert_eq!(r.transcribe_calls, 3);
    // VAD done, transcription part-way (checkpointed).
    assert_eq!(stage(&f), Some(ProcessingStage::Vad));
    let cache: ProcessingCache = sidecar::read_cache(f.dir.path(), f.id, &KEY)
        .unwrap()
        .unwrap();
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
        f.dir.path(),
        &KEY,
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
    naming::assign(
        &f.store,
        f.dir.path(),
        &KEY,
        speaker1.id,
        PersonRef::Existing(alice),
    )
    .unwrap();
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
        key: KEY,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        cancel: &cancel,
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
        f.dir.path(),
        &KEY,
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
        f.dir.path(),
        &KEY,
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
        f.dir.path(),
        &KEY,
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
        f.dir.path(),
        &KEY,
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
        f.dir.path(),
        &KEY,
        s.id,
        PersonRef::New {
            display_name: "Prashanth".into(),
            email: None,
        },
    )
    .unwrap();
    let prints_before = f.store.voiceprints_for_person(wrong).unwrap().len();
    // A line of theirs that has its own voiceprint.
    let printed: Vec<i64> = sidecar::read_embeddings(f.dir.path(), f.id, &KEY)
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
        f.dir.path(),
        &KEY,
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
        naming::assign_segment(
            &f.store,
            f.dir.path(),
            &KEY,
            other.id,
            PersonRef::Existing(right),
        )
        .unwrap();
        assert_eq!(
            f.store.get_segment(other.id).unwrap().unwrap().speaker_id,
            moved.speaker_id
        );
    }
    // Suggestions for a line include people already in the meeting.
    let names: Vec<String> = naming::segment_suggestions(
        &f.store,
        f.dir.path(),
        &KEY,
        line.id,
        &MeetingSettings::default(),
    )
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
    assert!(naming::confirm(&f.store, f.dir.path(), &KEY, s.id).is_err());
    let p = f.store.insert_person("Alice", None).unwrap();
    s.person_id = Some(p.id);
    s.confirmed = false;
    f.store.update_speaker(&s).unwrap();
    naming::confirm(&f.store, f.dir.path(), &KEY, s.id).unwrap();
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
        f.dir.path(),
        &KEY,
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
        key: KEY,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        cancel: &cancel,
        on_progress: Box::new(move |p| {
            // The user deletes the meeting (and its sidecars) while it runs.
            if p.stage == ProcessingStage::Transcribe && store.delete_meeting(id).unwrap() {
                sidecar::delete_sidecars(dir, id).unwrap();
            }
        }),
    }
    .run();
    assert!(result.is_err());
    assert!(!sidecar::cache_path(dir, id).exists());
    assert!(!sidecar::embeddings_path(dir, id).exists());
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
        key: KEY,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        cancel: &cancel,
        on_progress: Box::new(move |p| {
            // Restored (as `Processor::finish` does) while it is in VAD.
            if p.stage == ProcessingStage::Vad {
                store
                    .set_meeting_status(id, MeetingStatus::Processed, None)
                    .unwrap();
                store
                    .set_processing_stage(id, Some(ProcessingStage::Done))
                    .unwrap();
                sidecar::delete_cache(dir, id).unwrap();
            }
        }),
    }
    .run();
    assert!(super::super::is_cancelled(&result.unwrap_err()));
    assert_eq!(stage(&f), Some(ProcessingStage::Done));
    assert!(!sidecar::cache_path(dir, id).exists());
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
        key: KEY,
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
        cancel: &cancel,
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
        key: KEY,
        config: PipelineConfig::default(),
        mic: None,
        system: None,
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut t,
        embedder: &mut e,
        diarizer: Default::default(),
        cancel: &cancel,
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
        key: KEY,
        config: PipelineConfig::default(),
        mic: Some(Box::new(PcmSource::new(render(30_000, &mic_turns())))),
        system: Some(Box::new(PcmSource::new(render(30_000, &system_turns())))),
        vad: Box::new(|| Ok(Box::new(FakeVad) as Box<dyn SpeechScorer>)),
        transcriber: &mut transcriber,
        embedder: &mut embedder,
        diarizer: Default::default(),
        cancel: &cancel,
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
    let embeddings = sidecar::read_embeddings(f.dir.path(), f.id, &KEY)
        .unwrap()
        .unwrap();
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
    sidecar::write_embeddings(f.dir.path(), f.id, &KEY, &[stale]).unwrap();
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
    assert!(!sidecar::embeddings_path(f.dir.path(), f.id).exists());
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
        key: KEY,
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
        cancel: &cancel,
        on_progress: Box::new(move |p| sink.borrow_mut().push(p)),
    }
    .run()
    .unwrap();

    let calls = calls.borrow().clone();
    // 2 mic units, then 6 system ones: [mic, mic], then system pairs.
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
