//! History audio playback. One player for the whole app, so starting one
//! recording stops any other. rodio's output stream isn't `Send`,
//! so a dedicated thread owns it and takes commands over a channel.

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackState {
    /// The file currently loaded (playing or paused).
    pub path: Option<PathBuf>,
    pub playing: bool,
    pub position: Duration,
    pub duration: Duration,
    pub error: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            path: None,
            playing: false,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            error: None,
        }
    }
}

enum Cmd {
    Play(PathBuf),
    /// Play decoded mono samples kept in memory (meeting audio is encrypted
    /// at rest, so it is never written out). `key` identifies the clip in
    /// [`PlaybackState::path`].
    PlaySamples {
        key: PathBuf,
        samples: Arc<Vec<f32>>,
        sample_rate: u32,
    },
    Pause,
    Resume,
    Seek(Duration),
    Stop,
}

pub struct Playback {
    tx: mpsc::Sender<Cmd>,
    state: Arc<Mutex<PlaybackState>>,
}

/// WAV duration from the header.
pub fn wav_duration(path: &std::path::Path) -> Option<Duration> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let frames = reader.duration() as f64;
    (spec.sample_rate > 0).then(|| Duration::from_secs_f64(frames / spec.sample_rate as f64))
}

impl Playback {
    pub fn global() -> &'static Playback {
        static P: OnceLock<Playback> = OnceLock::new();
        P.get_or_init(Playback::spawn)
    }

    fn spawn() -> Playback {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let shared = state.clone();
        std::thread::Builder::new()
            .name("history-playback".into())
            .spawn(move || run(rx, shared))
            .expect("spawn playback thread");
        Playback { tx, state }
    }

    pub fn state(&self) -> PlaybackState {
        self.state.lock().clone()
    }

    pub fn play(&self, path: PathBuf) {
        let _ = self.tx.send(Cmd::Play(path));
    }
    /// Play in-memory mono samples. `key` identifies the clip (it shows up
    /// as `PlaybackState::path`); it is not a file.
    pub fn play_samples(&self, key: PathBuf, samples: Vec<f32>, sample_rate: u32) {
        let _ = self.tx.send(Cmd::PlaySamples {
            key,
            samples: Arc::new(samples),
            sample_rate,
        });
    }
    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }
    pub fn seek(&self, to: Duration) {
        let _ = self.tx.send(Cmd::Seek(to));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

fn run(rx: mpsc::Receiver<Cmd>, state: Arc<Mutex<PlaybackState>>) {
    let mut sink: Option<rodio::MixerDeviceSink> = None;
    let mut player: Option<rodio::Player> = None;
    // The in-memory clip being played, so Resume can replay it.
    let mut clip: Option<(Arc<Vec<f32>>, u32)> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Cmd::PlaySamples {
                key,
                samples,
                sample_rate,
            }) => {
                player = None;
                if sink.is_none() {
                    sink = open_sink();
                }
                let mut st = state.lock();
                *st = PlaybackState {
                    path: Some(key),
                    duration: samples_duration(samples.len(), sample_rate),
                    ..Default::default()
                };
                match &sink {
                    Some(s) => {
                        player = play_clip(s, samples.clone(), sample_rate);
                        st.playing = player.is_some();
                        if player.is_none() {
                            st.error = Some("Invalid audio".into());
                        }
                    }
                    None => st.error = Some("No audio output".into()),
                }
                clip = Some((samples, sample_rate));
            }
            Ok(Cmd::Play(path)) => {
                player = None;
                clip = None;
                if sink.is_none() {
                    sink = open_sink();
                }
                let mut st = state.lock();
                *st = PlaybackState {
                    path: Some(path.clone()),
                    duration: wav_duration(&path).unwrap_or_default(),
                    ..Default::default()
                };
                match &sink {
                    Some(s) => match play_file(s, &path) {
                        Ok(p) => {
                            player = Some(p);
                            st.playing = true;
                        }
                        Err(e) => st.error = Some(e),
                    },
                    None => st.error = Some("No audio output".into()),
                }
            }
            Ok(Cmd::Pause) => {
                if let Some(p) = &player {
                    p.pause();
                    state.lock().playing = false;
                }
            }
            Ok(Cmd::Resume) => {
                if let Some(p) = &player {
                    if p.empty() {
                        // Finished: replay from the start.
                        let path = state.lock().path.clone();
                        if let (Some((samples, rate)), Some(s)) = (&clip, &sink) {
                            player = play_clip(s, samples.clone(), *rate);
                        } else if let Some(path) = path {
                            player = sink.as_ref().and_then(|s| play_file(s, &path).ok());
                        }
                    } else {
                        p.play();
                    }
                    state.lock().playing = player.is_some();
                }
            }
            Ok(Cmd::Seek(to)) => {
                if let Some(p) = &player {
                    if let Err(e) = p.try_seek(to) {
                        log::warn!("Seek failed: {e}");
                    }
                    state.lock().position = to;
                }
            }
            Ok(Cmd::Stop) => {
                player = None;
                clip = None;
                *state.lock() = PlaybackState::default();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if let Some(p) = &player {
            let mut st = state.lock();
            st.position = p.get_pos().min(st.duration.max(p.get_pos()));
            if p.empty() && st.playing {
                st.playing = false;
                st.position = st.duration;
            }
        }
    }
}

fn open_sink() -> Option<rodio::MixerDeviceSink> {
    rodio::DeviceSinkBuilder::from_default_device()
        .and_then(|b| b.open_stream())
        .map_err(|e| log::error!("Failed to open audio output: {e}"))
        .ok()
}

fn play_file(
    sink: &rodio::MixerDeviceSink,
    path: &std::path::Path,
) -> Result<rodio::Player, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    rodio::play(sink.mixer(), std::io::BufReader::new(file)).map_err(|e| e.to_string())
}

fn play_clip(
    sink: &rodio::MixerDeviceSink,
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
) -> Option<rodio::Player> {
    let source = ArcSamples::new(samples, sample_rate)?;
    let player = rodio::Player::connect_new(sink.mixer());
    player.append(source);
    Some(player)
}

/// Mono samples played straight from a shared buffer, so each (re)play only
/// bumps a refcount instead of copying the clip (rodio's `SamplesBuffer`
/// wants an owned `Vec`).
struct ArcSamples {
    data: Arc<Vec<f32>>,
    pos: usize,
    sample_rate: rodio::SampleRate,
}

impl ArcSamples {
    fn new(data: Arc<Vec<f32>>, sample_rate: u32) -> Option<Self> {
        Some(Self {
            data,
            pos: 0,
            sample_rate: std::num::NonZero::new(sample_rate)?,
        })
    }
}

impl Iterator for ArcSamples {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.data.len().saturating_sub(self.pos);
        (remaining, Some(remaining))
    }
}

impl rodio::Source for ArcSamples {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.data.len().saturating_sub(self.pos))
    }

    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::<u16>::MIN
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(samples_duration(self.data.len(), self.sample_rate.get()))
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        // Mono, constant rate: jump straight to the sample, saturating at the end.
        let target = pos.as_secs_f64() * self.sample_rate.get() as f64;
        self.pos = (target as usize).min(self.data.len());
        Ok(())
    }
}

pub fn samples_duration(len: usize, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(len as f64 / sample_rate as f64)
}

/// `m:ss`
pub fn format_time(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_wav_duration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..32000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        assert_eq!(wav_duration(&path), Some(Duration::from_secs(2)));
        assert_eq!(format_time(Duration::from_secs(65)), "1:05");
        assert_eq!(samples_duration(32_000, 16_000), Duration::from_secs(2));
        assert_eq!(samples_duration(10, 0), Duration::ZERO);
    }

    #[test]
    fn arc_samples_plays_and_seeks_without_copying() {
        use rodio::Source;
        let data = Arc::new((0..8).map(|i| i as f32).collect::<Vec<_>>());
        assert!(ArcSamples::new(data.clone(), 0).is_none());
        let mut src = ArcSamples::new(data.clone(), 4).unwrap();
        assert_eq!(Arc::strong_count(&data), 2);
        assert_eq!(src.channels().get(), 1);
        assert_eq!(src.total_duration(), Some(Duration::from_secs(2)));
        assert_eq!(src.next(), Some(0.0));
        src.try_seek(Duration::from_secs(1)).unwrap();
        assert_eq!(src.size_hint(), (4, Some(4)));
        assert_eq!(src.collect::<Vec<_>>(), vec![4.0, 5.0, 6.0, 7.0]);
        let mut src = ArcSamples::new(data, 4).unwrap();
        src.try_seek(Duration::from_secs(10)).unwrap();
        assert_eq!(src.next(), None);
    }
}
