//! Live transcription of the meeting being recorded.
//!
//! The recorder copies every captured buffer into [`LiveTranscriber`] (a
//! [`LiveSink`]). A worker thread cuts each track into utterances with a
//! speech detector ([`segmenter`]) and decodes them on its own transcribe
//! worker: the utterance so far about every second while someone speaks
//! (tentative), and the whole of it once they pause (final), so a line
//! appears within about a second of being said and settles shortly after.
//!
//! The live transcript is only for following the meeting as it happens:
//! it is kept in memory, never stored. The meeting's transcript, with
//! speakers and word times, still comes from processing after the stop.
//! Mic lines are the user, system-audio lines everyone else. A mic line
//! that repeats what the system track said at the same time is the meeting
//! leaking from the speakers past echo cancellation: it is hidden
//! ([`EchoFilter`], the live form of the D9 safety net in
//! [`crate::meetings::processing::timeline`]).

pub mod segmenter;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use parking_lot::Mutex;

use crate::audio_toolkit::SpeechScorer;
use crate::host::AppHandle;
use crate::managers::model::ModelManager;
use crate::managers::transcription::meeting::{resolve_meeting_model, MeetingTranscriber};
use crate::meetings::processing::timeline::{echo_similarity, EchoParams};
use crate::meetings::processing::SAMPLE_RATE;
use crate::meetings::recorder::LiveSink;
use crate::meetings::types::*;
use crate::settings::{get_settings, VadBackend};
use segmenter::{Cut, Segmenter, SegmenterParams};

/// The model live transcription asks for: its own choice, else the
/// meetings model (then, when not downloaded, the same fallbacks).
pub fn live_model_choice(m: &crate::meetings::settings::MeetingSettings) -> Option<&str> {
    m.live_model
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .or(m.meeting_model.as_deref())
}

/// Audio the worker may fall behind by before new audio is dropped (both
/// tracks together): a stalled decode never grows memory without bound.
const MAX_BACKLOG_SAMPLES: usize = 60 * SAMPLE_RATE * 2;

enum Msg {
    Audio(Track, Vec<f32>),
    Stop,
}

struct Session {
    tx: Sender<Msg>,
    /// Samples sent and not yet taken by the worker.
    backlog: Arc<AtomicUsize>,
}

/// Feeds the live transcript of the recording in progress; see the module
/// docs. Cheap to share: the manager keeps one for the app's lifetime.
pub struct LiveTranscriber {
    app: AppHandle,
    transcript: Arc<Mutex<LiveTranscript>>,
    session: Mutex<Option<Session>>,
}

impl LiveTranscriber {
    pub fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            transcript: Default::default(),
            session: Mutex::new(None),
        }
    }

    /// The live transcript of the current (or last) recording.
    pub fn transcript(&self) -> LiveTranscript {
        self.transcript.lock().clone()
    }
}

impl LiveSink for LiveTranscriber {
    fn start(&self, meeting_id: MeetingId) {
        // A previous session's worker finishes on its own.
        self.stop();
        let enabled = get_settings(&self.app).meetings.live_transcription;
        let status = if enabled {
            LiveStatus::Starting
        } else {
            LiveStatus::Off
        };
        publish(
            &self.app,
            &self.transcript,
            LiveUpdate {
                meeting_id,
                status: Some(status),
                line: None,
            },
        );
        if !enabled {
            return;
        }
        let (tx, rx) = channel();
        let backlog = Arc::new(AtomicUsize::new(0));
        let worker = Worker {
            app: self.app.clone(),
            transcript: self.transcript.clone(),
            meeting_id,
            backlog: backlog.clone(),
        };
        match std::thread::Builder::new()
            .name(format!("meeting-live-{meeting_id}"))
            .spawn(move || worker.run(rx))
        {
            Ok(_) => *self.session.lock() = Some(Session { tx, backlog }),
            Err(e) => worker_failed(
                &self.app,
                &self.transcript,
                meeting_id,
                &format!("could not start live transcription: {e}"),
            ),
        }
    }

    fn push(&self, track: Track, samples: &[f32]) {
        // Never wait on the audio thread: a start or stop holding the lock
        // only costs these samples.
        let Some(session) = self.session.try_lock() else {
            return;
        };
        let Some(s) = session.as_ref() else { return };
        if s.backlog.load(Ordering::Relaxed) + samples.len() > MAX_BACKLOG_SAMPLES {
            return;
        }
        s.backlog.fetch_add(samples.len(), Ordering::Relaxed);
        let _ = s.tx.send(Msg::Audio(track, samples.to_vec()));
    }

    fn stop(&self) {
        if let Some(s) = self.session.lock().take() {
            // The worker decodes what was still being said, then exits.
            let _ = s.tx.send(Msg::Stop);
        }
    }
}

/// Record `update` and emit it.
fn publish(app: &AppHandle, transcript: &Mutex<LiveTranscript>, update: LiveUpdate) {
    transcript.lock().apply(&update);
    if let Err(e) = app.emit(events::LIVE_TRANSCRIPT, &update) {
        log::warn!("emit {}: {e}", events::LIVE_TRANSCRIPT);
    }
}

fn worker_failed(
    app: &AppHandle,
    transcript: &Mutex<LiveTranscript>,
    meeting_id: MeetingId,
    reason: &str,
) {
    log::warn!("meetings: live transcription unavailable: {reason}");
    publish(
        app,
        transcript,
        LiveUpdate {
            meeting_id,
            status: Some(LiveStatus::Unavailable {
                reason: reason.to_string(),
            }),
            line: None,
        },
    );
}

/// The id of a track's utterance in the live transcript (unique across
/// both tracks).
fn line_id(track: Track, cut_id: u64) -> u64 {
    cut_id * 2
        + match track {
            Track::Mic => 0,
            Track::System => 1,
        }
}

/// One track's speech detector, segmenter and not-yet-whole frame.
struct TrackCutter {
    track: Track,
    scorer: Box<dyn SpeechScorer>,
    segmenter: Segmenter,
    carry: Vec<f32>,
}

impl TrackCutter {
    fn new(track: Track, scorer: Box<dyn SpeechScorer>) -> Self {
        Self {
            track,
            scorer,
            segmenter: Segmenter::new(SegmenterParams::default()),
            carry: Vec::new(),
        }
    }

    /// Score the whole frames of `pcm` (and what was left over before) and
    /// return the cuts they complete.
    fn feed(&mut self, pcm: &[f32]) -> Vec<Cut> {
        self.carry.extend_from_slice(pcm);
        let frame = self.scorer.frame_samples();
        let whole = self.carry.len() / frame * frame;
        if whole == 0 {
            return Vec::new();
        }
        let frames: Vec<f32> = self.carry.drain(..whole).collect();
        let probs = match self.scorer.score(&frames) {
            Ok(p) => p,
            Err(e) => {
                log::debug!("meetings: live speech detection failed: {e:#}");
                vec![0.0; whole / frame]
            }
        };
        frames
            .chunks(frame)
            .zip(probs)
            .filter_map(|(f, p)| self.segmenter.push_frame(f, p))
            .collect()
    }
}

/// Hides live mic lines that echo the system track. Either line of a pair
/// can be decoded first, so each new line is checked both ways: a mic line
/// against the system lines around it, a system line against the mic lines
/// around it.
#[derive(Default)]
struct EchoFilter {
    /// Recent lines as decoded (original text), both tracks.
    lines: Vec<LiveLine>,
    /// Ids of the mic lines published blank.
    hidden: std::collections::HashSet<u64>,
}

/// How long lines are kept to compare against, behind the newest one.
const ECHO_MEMORY_MS: i64 = 60_000;

impl EchoFilter {
    /// Take a decoded line; returns what to publish: the line itself (blank
    /// when it is an echo) and any earlier mic line it shows to be one
    /// (blank).
    fn add(&mut self, line: LiveLine) -> Vec<LiveLine> {
        match self.lines.iter_mut().find(|l| l.id == line.id) {
            Some(existing) => *existing = line.clone(),
            None => self.lines.push(line.clone()),
        }
        let newest = self.lines.iter().map(|l| l.t1_ms).max().unwrap_or(0);
        self.lines.retain(|l| l.t1_ms >= newest - ECHO_MEMORY_MS);
        let lines = &self.lines;
        self.hidden.retain(|id| lines.iter().any(|l| l.id == *id));

        let mut out = Vec::new();
        match line.track {
            Track::Mic => {
                if self.is_echo(&line) {
                    self.hidden.insert(line.id);
                    out.push(blank(line));
                } else {
                    self.hidden.remove(&line.id);
                    out.push(line);
                }
            }
            Track::System => {
                let p = EchoParams::default();
                let revealed: Vec<LiveLine> = self
                    .lines
                    .iter()
                    .filter(|m| m.track == Track::Mic && !self.hidden.contains(&m.id))
                    .filter(|m| {
                        m.t0_ms - p.slack_ms < line.t1_ms && line.t0_ms < m.t1_ms + p.slack_ms
                    })
                    .filter(|m| self.is_echo(m))
                    .cloned()
                    .collect();
                out.push(line);
                for m in revealed {
                    self.hidden.insert(m.id);
                    out.push(blank(m));
                }
            }
        }
        out
    }

    /// Whether mic line `m` says (nearly) what the system track said around
    /// the same time (see [`echo_similarity`]).
    fn is_echo(&self, m: &LiveLine) -> bool {
        let p = EchoParams::default();
        let (t0, t1) = (m.t0_ms - p.slack_ms, m.t1_ms + p.slack_ms);
        let near: Vec<&str> = self
            .lines
            .iter()
            .filter(|s| s.track == Track::System && s.t0_ms < t1 && t0 < s.t1_ms)
            .map(|s| s.text.as_str())
            .collect();
        !near.is_empty() && echo_similarity(&m.text, &near.join(" ")) >= p.threshold
    }
}

/// `line` with no text: the transcript drops it.
fn blank(line: LiveLine) -> LiveLine {
    LiveLine {
        text: String::new(),
        ..line
    }
}

struct Worker {
    app: AppHandle,
    transcript: Arc<Mutex<LiveTranscript>>,
    meeting_id: MeetingId,
    backlog: Arc<AtomicUsize>,
}

/// What the worker has left to decode.
#[derive(Default)]
struct Pending {
    finals: std::collections::VecDeque<(Track, Cut)>,
    /// The latest partial per track (an older one is moot).
    partials: [Option<Cut>; 2],
}

impl Pending {
    fn slot(track: Track) -> usize {
        match track {
            Track::Mic => 0,
            Track::System => 1,
        }
    }

    fn add(&mut self, track: Track, cut: Cut) {
        let slot = &mut self.partials[Self::slot(track)];
        if cut.partial {
            *slot = Some(cut);
        } else {
            // Its final supersedes any partial of the same utterance.
            if slot.as_ref().is_some_and(|p| p.id == cut.id) {
                *slot = None;
            }
            self.finals.push_back((track, cut));
        }
    }

    /// Finals first, in order; a partial only when no final waits.
    fn next(&mut self) -> Option<(Track, Cut)> {
        if let Some(f) = self.finals.pop_front() {
            return Some(f);
        }
        let (i, _) = self
            .partials
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|p| (i, p.t0_ms)))
            .min_by_key(|(_, t0)| *t0)?;
        let track = if i == 0 { Track::Mic } else { Track::System };
        self.partials[i].take().map(|c| (track, c))
    }

    fn is_empty(&self) -> bool {
        self.finals.is_empty() && self.partials.iter().all(Option::is_none)
    }
}

impl Worker {
    fn run(self, rx: Receiver<Msg>) {
        let started = Instant::now();
        let ready = self.load();
        let (mut transcriber, mut cutters, language) = match ready {
            Ok(r) => r,
            Err(e) => {
                worker_failed(
                    &self.app,
                    &self.transcript,
                    self.meeting_id,
                    &format!("{e:#}"),
                );
                // Keep taking the audio so the backlog stays empty.
                while let Ok(Msg::Audio(..)) = rx.recv() {}
                return;
            }
        };
        log::info!(
            "meetings: live transcription ready in {} ms",
            started.elapsed().as_millis()
        );
        self.status(LiveStatus::Live);

        let mut pending = Pending::default();
        let mut echoes = EchoFilter::default();
        let mut stopping = false;
        loop {
            // Wait for audio only with nothing to decode; then take
            // everything that arrived, so decodes see the latest audio.
            let first = if pending.is_empty() && !stopping {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Msg::Stop),
                }
            } else {
                None
            };
            let mut msgs: Vec<Msg> = first.into_iter().collect();
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        msgs.push(Msg::Stop);
                        break;
                    }
                }
            }
            for msg in msgs {
                match msg {
                    Msg::Audio(track, pcm) => {
                        self.backlog.fetch_sub(pcm.len(), Ordering::Relaxed);
                        if stopping {
                            continue;
                        }
                        let cutter = &mut cutters[Pending::slot(track)];
                        for cut in cutter.feed(&pcm) {
                            pending.add(track, cut);
                        }
                    }
                    Msg::Stop if !stopping => {
                        stopping = true;
                        for cutter in &mut cutters {
                            if let Some(cut) = cutter.segmenter.flush() {
                                pending.add(cutter.track, cut);
                            }
                        }
                        // Nobody watches a tentative line after the stop.
                        pending.partials = [None, None];
                    }
                    Msg::Stop => {}
                }
            }
            match pending.next() {
                Some((track, cut)) => {
                    if !self.decode(&mut transcriber, &mut echoes, language.clone(), track, cut) {
                        break;
                    }
                }
                None if stopping => break,
                None => {}
            }
        }
        transcriber.unload();
        self.status(LiveStatus::Off);
    }

    /// The transcriber with the live model (see [`live_model_choice`]) loaded, a cutter per track,
    /// and the language to decode in (`None`: autodetect).
    fn load(&self) -> Result<(MeetingTranscriber, [TrackCutter; 2], Option<String>)> {
        let settings = get_settings(&self.app);
        let models = self
            .app
            .try_state::<Arc<ModelManager>>()
            .ok_or_else(|| anyhow::anyhow!("the model manager is not running"))?;
        let (model_id, path) = resolve_meeting_model(
            &models,
            live_model_choice(&settings.meetings),
            Some(&settings.selected_model),
        )?;
        let mut transcriber = MeetingTranscriber::new();
        transcriber.load(&self.app, &path)?;
        log::info!("meetings: live transcription uses model {model_id}");
        let cutters = [
            TrackCutter::new(Track::Mic, self.detector()?),
            TrackCutter::new(Track::System, self.detector()?),
        ];
        let language = Some(settings.selected_language).filter(|l| l != "auto");
        Ok((transcriber, cutters, language))
    }

    /// Silero, falling back to Earshot (as meeting processing does).
    fn detector(&self) -> Result<Box<dyn SpeechScorer>> {
        crate::managers::audio::create_detector(VadBackend::Silero, &self.app)
            .or_else(|_| crate::managers::audio::create_detector(VadBackend::Earshot, &self.app))
    }

    /// Decode `cut` and publish its line. False when the transcribe worker
    /// is gone for good (live transcription stops; recording goes on).
    fn decode(
        &self,
        transcriber: &mut MeetingTranscriber,
        echoes: &mut EchoFilter,
        language: Option<String>,
        track: Track,
        cut: Cut,
    ) -> bool {
        let started = Instant::now();
        let audio_ms = cut.t1_ms - cut.t0_ms;
        let text = match transcriber.transcribe_text(cut.pcm, language) {
            Ok(text) => text,
            Err(e) => {
                // A crashed worker restarts on the next run; one that can't
                // ends live transcription.
                log::warn!("meetings: live decode failed: {e}");
                return transcriber.engine().loaded().is_some();
            }
        };
        log::debug!(
            "meetings: live {} decode of {audio_ms} ms took {} ms",
            if cut.partial { "partial" } else { "final" },
            started.elapsed().as_millis()
        );
        let line = LiveLine {
            id: line_id(track, cut.id),
            track,
            t0_ms: cut.t0_ms,
            t1_ms: cut.t1_ms,
            text,
            tentative: cut.partial,
        };
        for line in echoes.add(line) {
            publish(
                &self.app,
                &self.transcript,
                LiveUpdate {
                    meeting_id: self.meeting_id,
                    status: None,
                    line: Some(line),
                },
            );
        }
        true
    }

    fn status(&self, status: LiveStatus) {
        publish(
            &self.app,
            &self.transcript,
            LiveUpdate {
                meeting_id: self.meeting_id,
                status: Some(status),
                line: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(id: u64, t0_ms: i64, partial: bool) -> Cut {
        Cut {
            id,
            t0_ms,
            t1_ms: t0_ms + 1_000,
            pcm: Vec::new(),
            partial,
        }
    }

    #[test]
    fn live_uses_its_own_model_else_the_meetings_one() {
        use crate::meetings::settings::MeetingSettings;
        let mut m = MeetingSettings {
            meeting_model: Some("parakeet-v3".into()),
            ..Default::default()
        };
        assert_eq!(live_model_choice(&m), Some("parakeet-v3"));
        m.live_model = Some("moonshine-base".into());
        assert_eq!(live_model_choice(&m), Some("moonshine-base"));
        m.live_model = Some(" ".into());
        assert_eq!(live_model_choice(&m), Some("parakeet-v3"));
        m.meeting_model = None;
        assert_eq!(live_model_choice(&m), None);
    }

    #[test]
    fn finals_go_first_and_replace_their_partials() {
        let mut p = Pending::default();
        p.add(Track::System, cut(0, 500, true));
        p.add(Track::Mic, cut(0, 0, true));
        p.add(Track::Mic, cut(0, 0, true));
        p.add(Track::Mic, cut(0, 0, false));
        let (track, c) = p.next().unwrap();
        assert_eq!((track, c.partial), (Track::Mic, false));
        // The mic partial was dropped with its final; the system one is left.
        let (track, c) = p.next().unwrap();
        assert_eq!((track, c.partial), (Track::System, true));
        assert!(p.next().is_none());
        assert!(p.is_empty());
    }

    /// The fixture through the live cutter and a real model, as a recording
    /// would: real time in 20 ms buffers, then a second of silence. Prints
    /// each decode and its time. Run with:
    /// `GUIDY_TRANSCRIBE_WORKER_EXE=target/debug/guidy GUIDY_TEST_MODEL=<gguf>
    ///  cargo test -p guidy-core --lib meetings::live -- --ignored --nocapture`
    // IGNORE-REASON: needs a downloaded model and the worker binary.
    #[test]
    #[ignore]
    fn live_decodes_speech_with_a_real_model() {
        let t = crate::host::testing::TestApp::new();
        let model = std::env::var_os("GUIDY_TEST_MODEL").expect("set GUIDY_TEST_MODEL");
        let wav = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/audio/speech-16k-mono.wav"
        );
        let mut pcm = crate::audio_toolkit::read_wav_samples(wav).expect("read wav");
        pcm.extend(std::iter::repeat_n(0.0, SAMPLE_RATE));
        let mut transcriber = MeetingTranscriber::new();
        transcriber
            .load(&t.app, std::path::Path::new(&model))
            .expect("load");
        let scorer = crate::managers::audio::create_detector(VadBackend::Earshot, &t.app).unwrap();
        let mut cutter = TrackCutter::new(Track::Mic, scorer);
        let mut finals = Vec::new();
        for buffer in pcm.chunks(SAMPLE_RATE / 50) {
            for cut in cutter.feed(buffer) {
                let partial = cut.partial;
                let audio_ms = cut.t1_ms - cut.t0_ms;
                let started = Instant::now();
                let text = transcriber.transcribe_text(cut.pcm, None).expect("decode");
                let took = started.elapsed().as_millis();
                println!(
                    "{} {audio_ms} ms in {took} ms: {text}",
                    if partial { "partial" } else { "final" }
                );
                assert!(took < 1_500, "a live decode took {took} ms");
                if !partial {
                    finals.push(text);
                }
            }
        }
        finals.extend(
            cutter
                .segmenter
                .flush()
                .map(|c| transcriber.transcribe_text(c.pcm, None).expect("decode")),
        );
        let text = finals.join(" ");
        println!("live transcript: {text}");
        assert!(text.split_whitespace().count() >= 3, "{text:?}");
        transcriber.unload();
    }

    fn live(id: u64, track: Track, t0_ms: i64, t1_ms: i64, text: &str) -> LiveLine {
        LiveLine {
            id: line_id(track, id),
            track,
            t0_ms,
            t1_ms,
            text: text.into(),
            tentative: false,
        }
    }

    fn texts(lines: &[LiveLine]) -> Vec<(Track, &str)> {
        lines.iter().map(|l| (l.track, l.text.as_str())).collect()
    }

    const SAID: &str = "So I think Santosh will join in sometime. But JD, the objective is \
                        to go through the action items that were there on Friday and.";
    const ECHO: &str = "So I think Santosh will join in sometime. But JD, the objective is \
                        to go through the action items that were there on.";

    #[test]
    fn a_mic_echo_decoded_after_the_system_line_is_hidden() {
        let mut f = EchoFilter::default();
        assert_eq!(
            texts(&f.add(live(0, Track::System, 0, 6_000, SAID))),
            [(Track::System, SAID)]
        );
        assert_eq!(
            texts(&f.add(live(0, Track::Mic, 200, 6_100, ECHO))),
            [(Track::Mic, "")]
        );
    }

    #[test]
    fn a_mic_echo_decoded_first_is_hidden_when_the_system_line_comes() {
        let mut f = EchoFilter::default();
        assert_eq!(
            texts(&f.add(live(0, Track::Mic, 200, 6_100, ECHO))),
            [(Track::Mic, ECHO)]
        );
        assert_eq!(
            texts(&f.add(live(0, Track::System, 0, 6_000, SAID))),
            [(Track::System, SAID), (Track::Mic, "")]
        );
        // Its later decodes stay hidden, and it isn't blanked twice.
        assert_eq!(
            texts(&f.add(live(0, Track::Mic, 200, 6_100, ECHO))),
            [(Track::Mic, "")]
        );
        assert_eq!(
            texts(&f.add(live(1, Track::System, 6_500, 8_000, "Sounds good."))).len(),
            1
        );
    }

    #[test]
    fn the_users_own_words_are_kept() {
        let mut f = EchoFilter::default();
        f.add(live(0, Track::System, 0, 6_000, SAID));
        // Talking over the other side, about something else.
        let mine = "Sure, I can share my screen in a minute.";
        assert_eq!(
            texts(&f.add(live(0, Track::Mic, 1_000, 4_000, mine))),
            [(Track::Mic, mine)]
        );
        // The same words much later: the user repeating them.
        assert_eq!(
            texts(&f.add(live(1, Track::Mic, 20_000, 26_000, ECHO))),
            [(Track::Mic, ECHO)]
        );
    }

    #[test]
    fn line_ids_are_unique_across_tracks() {
        assert_ne!(line_id(Track::Mic, 3), line_id(Track::System, 3));
        assert_ne!(line_id(Track::System, 0), line_id(Track::Mic, 1));
    }

    #[test]
    fn a_disabled_start_publishes_off_and_ignores_audio() {
        let t = crate::host::testing::TestApp::new();
        crate::settings::update_settings(&t.app, |s| s.meetings.live_transcription = false);
        let live = LiveTranscriber::new(&t.app);
        live.start(7);
        live.push(Track::Mic, &[0.0; 160]);
        let transcript = live.transcript();
        assert_eq!(
            (transcript.meeting_id, transcript.status),
            (7, LiveStatus::Off)
        );
        assert!(live.session.lock().is_none());
        live.stop();
        assert!(t.event_names().iter().all(|n| n == events::LIVE_TRANSCRIPT));
    }
}
