//! How each source of a recording is doing, for the user to see while it
//! happens. Pure: a reducer over what capture reports, whether audio flowed,
//! permissions and a clock. It warns and never touches the recording, except
//! that [`HealthTracker::stop_due`] says when 15 minutes without any audio on
//! either source has run out; the recorder acts on that.

use crate::meetings::types::{RecordingHealth, SourceHealth, Track};

/// A microphone with nothing but zeros this long is silent.
pub const MIC_SILENT_MS: i64 = 30_000;
/// The same for system audio, only while a call is detected: without one,
/// nothing playing is normal.
pub const SYSTEM_SILENT_MS: i64 = 120_000;
/// No samples at all from a stream this long: it's reconnecting at best.
pub const NO_SAMPLES_RECONNECTING_MS: i64 = 5_000;
/// ... and this long: lost, within the 10 s the scorecard allows.
pub const NO_SAMPLES_LOST_MS: i64 = 9_000;
/// Reported stalled this long: lost. The native watchdog says so up to 3.5 s
/// after the last callback, and the stream's padding keeps samples flowing, so
/// [`NO_SAMPLES_LOST_MS`] alone would never see it. This makes it 9 s from the
/// last callback too.
pub const STALLED_LOST_MS: i64 = 5_500;
/// A permission status that can't be read is shown unless sound has reached
/// the track this recently, which proves access.
pub const UNREADABLE_PERMISSION_QUIET_MS: i64 = 5_000;
/// Both sources without sound this long: warn.
pub const NO_AUDIO_WARN_MS: i64 = 15 * 60_000;
/// ... then stop this long after, unless told to keep going.
pub const NO_AUDIO_STOP_GRACE_MS: i64 = 60_000;
/// Ticks further apart than this (a sleeping Mac) say nothing about the audio.
const MAX_TICK_GAP_MS: i64 = 10_000;

/// A message capture reported, by its text (`swift/meeting_bridge.swift`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMessage {
    Stalled,
    Recovered,
    /// The stream was gone this long.
    Gap(i64),
    /// A stream failed, and is being retried.
    Failed,
    /// It came back up after failing (with nothing to say about its audio).
    Restarted,
    /// Its queue is full: audio is being dropped.
    Dropping,
    /// The queue drained: this many samples were dropped and made up for
    /// with silence.
    Dropped(i64),
}

impl CaptureMessage {
    /// `None` for a plain note.
    pub fn parse(message: &str) -> Option<Self> {
        if message.starts_with("error:") {
            return Some(Self::Failed);
        }
        match message.strip_prefix("state:")? {
            "stalled" => Some(Self::Stalled),
            "recovered" => Some(Self::Recovered),
            "restarted" => Some(Self::Restarted),
            "dropping" => Some(Self::Dropping),
            other => {
                if let Some(ms) = other.strip_prefix("gap:") {
                    ms.parse::<i64>().ok().map(Self::Gap)
                } else {
                    other
                        .strip_prefix("dropped:")
                        .and_then(|samples| samples.parse::<i64>().ok())
                        .map(Self::Dropped)
                }
            }
        }
    }
}

/// Whether a stream delivered anything since the last tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    pub samples: bool,
    /// Anything but zeros.
    pub sound: bool,
}

#[derive(Clone, Debug, Default)]
struct TrackState {
    stalled: bool,
    /// When `stalled` was reported, to the tick.
    stalled_at: i64,
    failed: bool,
    write_failed: bool,
    denied: bool,
    /// Audio is being dropped by the native delivery queue ...
    delivery_dropping: bool,
    /// ... or by the writer's.
    writer_dropping: bool,
    last_samples: i64,
    last_sound: i64,
}

impl TrackState {
    fn new(now: i64) -> Self {
        Self {
            last_samples: now,
            last_sound: now,
            ..Self::default()
        }
    }

    fn is_dropping(&self) -> bool {
        self.delivery_dropping || self.writer_dropping
    }

    fn health(&self, now: i64, silent_after: Option<i64>) -> SourceHealth {
        let quiet = now - self.last_samples;
        if self.write_failed
            || self.denied
            || self.failed
            || self.is_dropping()
            || quiet >= NO_SAMPLES_LOST_MS
            || (self.stalled && now - self.stalled_at >= STALLED_LOST_MS)
        {
            return SourceHealth::Lost;
        }
        if self.stalled || quiet >= NO_SAMPLES_RECONNECTING_MS {
            return SourceHealth::Reconnecting;
        }
        if silent_after.is_some_and(|after| now - self.last_sound >= after) {
            return SourceHealth::Silent;
        }
        SourceHealth::Healthy
    }
}

/// See the module docs.
#[derive(Clone, Debug)]
pub struct HealthTracker {
    mic: TrackState,
    system: TrackState,
    in_call: bool,
    low_disk: bool,
    backlog: bool,
    system_permission_unreadable: bool,
    /// Since when neither source had sound (or the user last said keep going).
    no_audio_since: i64,
    last_tick: i64,
}

impl HealthTracker {
    pub fn new(now: i64) -> Self {
        Self {
            mic: TrackState::new(now),
            system: TrackState::new(now),
            in_call: false,
            low_disk: false,
            backlog: false,
            system_permission_unreadable: false,
            no_audio_since: now,
            last_tick: now,
        }
    }

    fn track(&mut self, track: Track) -> &mut TrackState {
        match track {
            Track::Mic => &mut self.mic,
            Track::System => &mut self.system,
        }
    }

    pub fn on_message(&mut self, track: Track, message: CaptureMessage) {
        let last_tick = self.last_tick;
        let state = self.track(track);
        match message {
            CaptureMessage::Stalled => {
                if !state.stalled {
                    state.stalled = true;
                    state.stalled_at = last_tick;
                }
            }
            CaptureMessage::Recovered => {
                state.stalled = false;
                state.failed = false;
            }
            CaptureMessage::Failed => state.failed = true,
            CaptureMessage::Restarted => state.failed = false,
            CaptureMessage::Gap(_) => {}
            CaptureMessage::Dropping => state.delivery_dropping = true,
            CaptureMessage::Dropped(_) => state.delivery_dropping = false,
        }
    }

    /// The track's file can't be written: it stays lost, for the rest of the
    /// recording.
    pub fn on_write_failed(&mut self, track: Track) {
        self.track(track).write_failed = true;
    }

    pub fn set_permissions(&mut self, mic_granted: bool, system_granted: bool) {
        self.mic.denied = !mic_granted;
        self.system.denied = !system_granted;
    }

    /// The system audio permission can't be read right now.
    pub fn set_system_permission_unreadable(&mut self, unreadable: bool) {
        self.system_permission_unreadable = unreadable;
    }

    pub fn set_in_call(&mut self, in_call: bool, now: i64) {
        if in_call && !self.in_call {
            // Silence counts from the call, not from before it.
            self.system.last_sound = self.system.last_sound.max(now);
        }
        self.in_call = in_call;
    }

    pub fn set_low_disk(&mut self, low: bool) {
        self.low_disk = low;
    }

    /// The writer's queue for `track` is full and audio is being dropped.
    pub fn set_writer_dropping(&mut self, track: Track, dropping: bool) {
        self.track(track).writer_dropping = dropping;
    }

    pub fn set_backlog(&mut self, behind: bool) {
        self.backlog = behind;
    }

    /// "Keep" on the no-audio warning: count from now.
    pub fn keep_going(&mut self, now: i64) {
        self.no_audio_since = now;
    }

    /// Count everything from `now`: nothing can be said about the time before.
    fn restart_clocks(&mut self, now: i64) {
        for state in [&mut self.mic, &mut self.system] {
            state.last_samples = now;
            state.last_sound = now;
            state.stalled_at = now;
        }
        self.no_audio_since = now;
    }

    /// One tick, with what flowed since the last. While paused there's no
    /// audio to judge. Across a sleep, or a clock going backwards, no silence
    /// is counted.
    pub fn tick(&mut self, now: i64, mic: Flow, system: Flow, paused: bool) {
        let elapsed = now - self.last_tick;
        self.last_tick = now;
        if paused || !(0..=MAX_TICK_GAP_MS).contains(&elapsed) {
            self.restart_clocks(now);
            return;
        }
        for (state, flow) in [(&mut self.mic, mic), (&mut self.system, system)] {
            if flow.samples {
                state.last_samples = now;
            }
            if flow.sound {
                state.last_sound = now;
                // Sound reaching the track says more than any report.
                state.failed = false;
                state.stalled = false;
            }
        }
        if mic.sound || system.sound {
            self.no_audio_since = now;
        }
    }

    pub fn health(&self, now: i64) -> RecordingHealth {
        let warn_at = self.no_audio_since + NO_AUDIO_WARN_MS;
        RecordingHealth {
            mic: self.mic.health(now, Some(MIC_SILENT_MS)),
            system: self
                .system
                .health(now, self.in_call.then_some(SYSTEM_SILENT_MS)),
            mic_denied: self.mic.denied,
            system_denied: self.system.denied,
            low_disk: self.low_disk,
            backlog: self.backlog,
            audio_dropping: self.mic.is_dropping() || self.system.is_dropping(),
            system_permission_unknown: self.system_permission_unreadable
                && now - self.system.last_sound >= UNREADABLE_PERMISSION_QUIET_MS,
            silence_stop_at: (now >= warn_at).then_some(warn_at + NO_AUDIO_STOP_GRACE_MS),
        }
    }

    /// 15 minutes without audio, a warning, and no answer.
    pub fn stop_due(&self, now: i64) -> bool {
        now >= self.no_audio_since + NO_AUDIO_WARN_MS + NO_AUDIO_STOP_GRACE_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: Flow = Flow {
        samples: true,
        sound: true,
    };
    const ZEROS: Flow = Flow {
        samples: true,
        sound: false,
    };
    const NOTHING: Flow = Flow {
        samples: false,
        sound: false,
    };

    fn run(h: &mut HealthTracker, from: i64, to: i64, mic: Flow, system: Flow) {
        let mut t = from;
        while t <= to {
            h.tick(t, mic, system, false);
            t += 1_000;
        }
    }

    #[test]
    fn messages_are_parsed() {
        use CaptureMessage::*;
        assert_eq!(CaptureMessage::parse("state:stalled"), Some(Stalled));
        assert_eq!(CaptureMessage::parse("state:recovered"), Some(Recovered));
        assert_eq!(CaptureMessage::parse("state:restarted"), Some(Restarted));
        assert_eq!(CaptureMessage::parse("state:gap:1234"), Some(Gap(1234)));
        assert_eq!(CaptureMessage::parse("error: tap gone"), Some(Failed));
        assert_eq!(CaptureMessage::parse("state:gap:x"), None);
        assert_eq!(CaptureMessage::parse("state:dropping"), Some(Dropping));
        assert_eq!(CaptureMessage::parse("state:dropped:9"), Some(Dropped(9)));
        assert_eq!(CaptureMessage::parse("state:dropped:x"), None);
        assert_eq!(CaptureMessage::parse("state:other"), None);
        assert_eq!(CaptureMessage::parse("microphone restarted"), None);
    }

    #[test]
    fn a_stall_reconnects_and_recovers() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 3_000, LIVE, LIVE);
        assert_eq!(h.health(3_000).system, SourceHealth::Healthy);
        h.on_message(Track::System, CaptureMessage::Stalled);
        assert_eq!(h.health(3_000).system, SourceHealth::Reconnecting);
        assert_eq!(h.health(3_000).mic, SourceHealth::Healthy);
        h.on_message(Track::System, CaptureMessage::Recovered);
        assert_eq!(h.health(3_000).system, SourceHealth::Healthy);
    }

    #[test]
    fn a_dead_stream_behind_its_padding_is_lost_by_nine_seconds() {
        let mut h = HealthTracker::new(0);
        // Padding: zeros keep coming after the last callback at 0.
        run(&mut h, 1_000, 3_000, ZEROS, LIVE);
        assert_eq!(h.health(3_000).mic, SourceHealth::Healthy);
        // The watchdog's report, at its latest.
        h.on_message(Track::Mic, CaptureMessage::Stalled);
        run(&mut h, 4_000, 8_000, ZEROS, LIVE);
        assert_eq!(h.health(8_000).mic, SourceHealth::Reconnecting);
        run(&mut h, 9_000, 9_000, ZEROS, LIVE);
        assert_eq!(h.health(9_000).mic, SourceHealth::Lost);
        // A repeated report doesn't restart the count.
        h.on_message(Track::Mic, CaptureMessage::Stalled);
        assert_eq!(h.health(9_000).mic, SourceHealth::Lost);
        // Back: and a later stall counts from then.
        h.on_message(Track::Mic, CaptureMessage::Recovered);
        assert_eq!(h.health(9_000).mic, SourceHealth::Healthy);
        run(&mut h, 10_000, 12_000, LIVE, LIVE);
        h.on_message(Track::Mic, CaptureMessage::Stalled);
        run(&mut h, 13_000, 13_000, ZEROS, LIVE);
        assert_eq!(h.health(13_000).mic, SourceHealth::Reconnecting);
    }

    #[test]
    fn plain_quiet_is_not_a_warning_at_ten_seconds() {
        let mut h = HealthTracker::new(0);
        h.set_in_call(true, 0);
        run(&mut h, 1_000, 10_000, ZEROS, ZEROS);
        let health = h.health(10_000);
        assert_eq!(health.mic, SourceHealth::Healthy);
        assert_eq!(health.system, SourceHealth::Healthy);
    }

    #[test]
    fn a_stall_across_a_sleep_counts_from_the_wake() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 3_000, ZEROS, LIVE);
        h.on_message(Track::System, CaptureMessage::Stalled);
        let woke = 3_000 + 20 * 60_000;
        h.tick(woke, ZEROS, ZEROS, false);
        assert_eq!(h.health(woke).system, SourceHealth::Reconnecting);
    }

    #[test]
    fn an_unreadable_permission_is_shown_unless_sound_proves_access() {
        let mut h = HealthTracker::new(0);
        h.set_system_permission_unreadable(true);
        // Not counted as refused.
        run(&mut h, 1_000, 6_000, LIVE, ZEROS);
        let health = h.health(6_000);
        assert!(health.system_permission_unknown);
        assert!(!health.system_denied);
        assert_eq!(health.system, SourceHealth::Healthy);
        // Sound on the track: access is there, whatever the status says.
        run(&mut h, 7_000, 7_000, LIVE, LIVE);
        assert!(!h.health(7_000).system_permission_unknown);
        run(&mut h, 8_000, 12_000, LIVE, ZEROS);
        assert!(h.health(12_000).system_permission_unknown);
        h.set_system_permission_unreadable(false);
        assert!(!h.health(12_000).system_permission_unknown);
    }

    #[test]
    fn a_failed_stream_is_lost_until_it_recovers() {
        let mut h = HealthTracker::new(0);
        h.on_message(Track::Mic, CaptureMessage::Failed);
        assert_eq!(h.health(0).mic, SourceHealth::Lost);
        h.on_message(Track::Mic, CaptureMessage::Recovered);
        assert_eq!(h.health(0).mic, SourceHealth::Healthy);
    }

    #[test]
    fn a_stream_that_came_back_is_not_lost_however_it_is_told() {
        let mut h = HealthTracker::new(0);
        h.on_message(Track::System, CaptureMessage::Failed);
        h.on_message(Track::System, CaptureMessage::Restarted);
        assert_eq!(h.health(0).system, SourceHealth::Healthy);
        // No report at all, but sound.
        h.on_message(Track::Mic, CaptureMessage::Failed);
        h.on_message(Track::Mic, CaptureMessage::Stalled);
        run(&mut h, 1_000, 1_000, LIVE, LIVE);
        assert_eq!(h.health(1_000).mic, SourceHealth::Healthy);
    }

    #[test]
    fn a_dead_stream_shows_within_ten_seconds() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 4_000, NOTHING, LIVE);
        assert_eq!(h.health(4_000).mic, SourceHealth::Healthy);
        run(&mut h, 5_000, 5_000, NOTHING, LIVE);
        assert_eq!(h.health(5_000).mic, SourceHealth::Reconnecting);
        run(&mut h, 6_000, 10_000, NOTHING, LIVE);
        assert_eq!(h.health(10_000).mic, SourceHealth::Lost);
    }

    #[test]
    fn zeros_from_the_mic_are_silent_after_30_seconds() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 29_000, ZEROS, LIVE);
        assert_eq!(h.health(29_000).mic, SourceHealth::Healthy);
        run(&mut h, 30_000, 30_000, ZEROS, LIVE);
        assert_eq!(h.health(30_000).mic, SourceHealth::Silent);
        run(&mut h, 31_000, 31_000, LIVE, LIVE);
        assert_eq!(h.health(31_000).mic, SourceHealth::Healthy);
    }

    #[test]
    fn zeros_from_the_system_only_count_during_a_call() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 200_000, LIVE, ZEROS);
        assert_eq!(h.health(200_000).system, SourceHealth::Healthy);
        h.set_in_call(true, 200_000);
        run(&mut h, 201_000, 319_000, LIVE, ZEROS);
        assert_eq!(h.health(319_000).system, SourceHealth::Healthy);
        run(&mut h, 320_000, 320_000, LIVE, ZEROS);
        assert_eq!(h.health(320_000).system, SourceHealth::Silent);
    }

    #[test]
    fn dropping_audio_is_lost_and_says_so_until_it_ends() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 2_000, LIVE, LIVE);
        h.set_writer_dropping(Track::Mic, true);
        let health = h.health(2_000);
        assert_eq!(health.mic, SourceHealth::Lost);
        assert_eq!(health.system, SourceHealth::Healthy);
        assert!(health.audio_dropping);
        // Sound still reaching the capture side doesn't hide it.
        run(&mut h, 3_000, 3_000, LIVE, LIVE);
        assert_eq!(h.health(3_000).mic, SourceHealth::Lost);
        h.set_writer_dropping(Track::Mic, false);
        let health = h.health(3_000);
        assert_eq!(health.mic, SourceHealth::Healthy);
        assert!(!health.audio_dropping);
        // The same from the delivery queue.
        h.on_message(Track::System, CaptureMessage::Dropping);
        assert_eq!(h.health(3_000).system, SourceHealth::Lost);
        h.on_message(Track::System, CaptureMessage::Dropped(160));
        assert_eq!(h.health(3_000).system, SourceHealth::Healthy);
    }

    #[test]
    fn a_write_failure_stays_lost() {
        let mut h = HealthTracker::new(0);
        h.on_write_failed(Track::System);
        h.on_message(Track::System, CaptureMessage::Recovered);
        run(&mut h, 1_000, 2_000, LIVE, LIVE);
        assert_eq!(h.health(2_000).system, SourceHealth::Lost);
        assert_eq!(h.health(2_000).mic, SourceHealth::Healthy);
    }

    #[test]
    fn a_revoked_permission_is_lost_and_named() {
        let mut h = HealthTracker::new(0);
        h.set_permissions(true, false);
        let health = h.health(0);
        assert_eq!(health.system, SourceHealth::Lost);
        assert!(health.system_denied && !health.mic_denied);
        h.set_permissions(true, true);
        assert_eq!(h.health(0).system, SourceHealth::Healthy);
    }

    #[test]
    fn fifteen_minutes_without_audio_warns_then_stops() {
        let mut h = HealthTracker::new(0);
        let warn = NO_AUDIO_WARN_MS;
        // Sound on one source keeps it going.
        run(&mut h, 1_000, warn, ZEROS, LIVE);
        assert_eq!(h.health(warn).silence_stop_at, None);
        let start = warn;
        run(&mut h, start + 1_000, start + warn - 1_000, ZEROS, ZEROS);
        assert_eq!(h.health(start + warn - 1_000).silence_stop_at, None);
        run(&mut h, start + warn, start + warn, ZEROS, ZEROS);
        let stop_at = start + warn + NO_AUDIO_STOP_GRACE_MS;
        assert_eq!(h.health(start + warn).silence_stop_at, Some(stop_at));
        assert!(!h.stop_due(stop_at - 1));
        assert!(h.stop_due(stop_at));
        // "Keep": counted again from now.
        h.keep_going(stop_at - 1);
        assert_eq!(h.health(stop_at).silence_stop_at, None);
        assert!(!h.stop_due(stop_at));
    }

    #[test]
    fn nothing_is_counted_across_a_sleep_or_a_pause() {
        let mut h = HealthTracker::new(0);
        run(&mut h, 1_000, 20_000, ZEROS, ZEROS);
        // 20 minutes asleep.
        let woke = 20_000 + 20 * 60_000;
        h.tick(woke, NOTHING, NOTHING, false);
        let health = h.health(woke);
        assert_eq!(health.mic, SourceHealth::Healthy);
        assert_eq!(health.silence_stop_at, None);
        assert!(!h.stop_due(woke));
        // Paused for an hour: the same.
        let resumed = woke + 60 * 60_000;
        let mut t = woke;
        while t < resumed {
            t += 1_000;
            h.tick(t, ZEROS, ZEROS, true);
        }
        assert_eq!(h.health(resumed).mic, SourceHealth::Healthy);
        assert!(!h.stop_due(resumed));
        // A clock going backwards too.
        h.tick(0, NOTHING, NOTHING, false);
        assert_eq!(h.health(0).mic, SourceHealth::Healthy);
    }
}
