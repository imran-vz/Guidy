//! The meeting player: plays the whole meeting (both tracks mixed) through
//! the app's one [`Playback`], a few minutes at a time, with play/pause,
//! 15-second skips, a scrubber and the time. Only this bar repaints while
//! it plays; the detail hears where the playhead is through
//! [`PlayerEvent::Moved`].

use super::meetings_backend;
use crate::logic::meetings::{format_offset, player_chunk};
use crate::playback::Playback;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::types::MeetingId;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// `meeting_audio` returns 16 kHz mono.
const SAMPLE_RATE: u32 = 16_000;
/// How often the bar follows the playhead while playing.
const TICK: Duration = Duration::from_millis(200);
/// Load the next stretch this long before the current one ends.
const PREFETCH_MS: i64 = 20_000;
/// The skip buttons' step.
const SKIP_MS: i64 = 15_000;
/// After asking the player thread to play, how long its state may still
/// show the previous clip.
const START_GRACE: Duration = Duration::from_millis(1_500);

/// Where the playhead is now (ms from the recording start).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayerEvent {
    Moved(i64),
}

/// The stretch of the meeting loaded into [`Playback`].
struct Chunk {
    t0: i64,
    t1: i64,
    key: PathBuf,
}

pub struct MeetingPlayer {
    meeting_id: MeetingId,
    duration_ms: i64,
    position_ms: i64,
    /// The user wants it playing (it may still be loading).
    playing: bool,
    loading: bool,
    chunk: Option<Chunk>,
    /// The next stretch, loaded ahead: (t0, t1, samples).
    prefetched: Option<(i64, i64, Vec<f32>)>,
    prefetching: bool,
    /// Bumped per load: an older load that finishes late is dropped.
    seq: u64,
    started: Option<Instant>,
    error: Option<String>,
    ticking: bool,
    /// The scrubber's bounds from the last paint.
    track: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// A drag on the scrubber is moving the playhead.
    scrubbing: bool,
}

impl EventEmitter<PlayerEvent> for MeetingPlayer {}

impl MeetingPlayer {
    pub fn new(meeting_id: MeetingId) -> Self {
        Self {
            meeting_id,
            duration_ms: 0,
            position_ms: 0,
            playing: false,
            loading: false,
            chunk: None,
            prefetched: None,
            prefetching: false,
            seq: 0,
            started: None,
            error: None,
            ticking: false,
            track: Rc::new(Cell::new(None)),
            scrubbing: false,
        }
    }

    pub fn set_duration(&mut self, duration_ms: i64, cx: &mut Context<Self>) {
        if duration_ms != self.duration_ms {
            self.duration_ms = duration_ms.max(0);
            self.position_ms = self.position_ms.min(self.duration_ms);
            cx.notify();
        }
    }

    pub fn position_ms(&self) -> i64 {
        self.position_ms
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Show the bar as if it played at `position_ms` (snapshots and tests:
    /// no audio output).
    #[cfg(test)]
    pub fn preview(&mut self, position_ms: i64, playing: bool, cx: &mut Context<Self>) {
        self.position_ms = position_ms.clamp(0, self.duration_ms);
        self.playing = playing;
        cx.emit(PlayerEvent::Moved(self.position_ms));
        cx.notify();
    }

    fn key(&self, t0: i64) -> PathBuf {
        PathBuf::from(format!("meeting-{}-audio-{t0}", self.meeting_id))
    }

    /// The clip in [`Playback`] is one of this meeting's.
    fn owns_playback(&self) -> bool {
        let prefix = format!("meeting-{}-audio-", self.meeting_id);
        Playback::global()
            .state()
            .path
            .is_some_and(|p| p.to_string_lossy().starts_with(&prefix))
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            self.pause(cx);
        } else {
            if self.position_ms >= self.duration_ms {
                self.position_ms = 0;
            }
            self.playing = true;
            self.start(cx);
        }
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        if self.owns_playback() {
            Playback::global().pause();
        }
        self.playing = false;
        self.loading = false;
        cx.notify();
    }

    /// Play from `t_ms`.
    pub fn play_at(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        self.position_ms = t_ms.clamp(0, self.duration_ms);
        self.playing = true;
        self.start(cx);
        cx.emit(PlayerEvent::Moved(self.position_ms));
    }

    /// Move the playhead to `t_ms`, playing on from there if it plays.
    pub fn seek(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        self.position_ms = t_ms.clamp(0, self.duration_ms);
        if self.playing {
            self.start(cx);
        }
        cx.emit(PlayerEvent::Moved(self.position_ms));
        cx.notify();
    }

    pub fn skip(&mut self, delta_ms: i64, cx: &mut Context<Self>) {
        self.seek(self.position_ms + delta_ms, cx);
    }

    /// Play from the playhead: in the loaded stretch when it covers it,
    /// else load the stretch that starts there.
    fn start(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let pos = self.position_ms;
        let st = Playback::global().state();
        if let Some(c) = &self.chunk {
            if st.path.as_ref() == Some(&c.key) && c.t0 <= pos && pos < c.t1 {
                let player = Playback::global();
                // Resume first: a finished clip restarts, then seeks.
                player.resume();
                player.seek(Duration::from_millis((pos - c.t0) as u64));
                self.ensure_ticking(cx);
                cx.notify();
                return;
            }
        }
        self.load(pos, cx);
    }

    fn load(&mut self, at: i64, cx: &mut Context<Self>) {
        let (t0, t1) = player_chunk(at, self.duration_ms);
        if t1 <= t0 {
            self.playing = false;
            self.loading = false;
            cx.notify();
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        if let Some((p0, p1, samples)) = self.prefetched.take() {
            if p0 == t0 {
                self.begin_chunk(p0, p1, samples, cx);
                return;
            }
        }
        self.loading = true;
        cx.notify();
        let id = self.meeting_id;
        let backend = meetings_backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.meeting_audio(id, t0, t1) })
                .await;
            this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                this.loading = false;
                match r {
                    Ok(samples) if this.playing => this.begin_chunk(t0, t1, samples, cx),
                    Ok(_) => cx.notify(),
                    Err(e) => {
                        this.playing = false;
                        this.error = Some(crate::meetings_backend::error_message(&e));
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
        self.ensure_ticking(cx);
    }

    fn begin_chunk(&mut self, t0: i64, t1: i64, samples: Vec<f32>, cx: &mut Context<Self>) {
        let key = self.key(t0);
        Playback::global().play_samples(key.clone(), samples, SAMPLE_RATE);
        self.chunk = Some(Chunk { t0, t1, key });
        self.loading = false;
        self.started = Some(Instant::now());
        self.ensure_ticking(cx);
        cx.notify();
    }

    fn prefetch(&mut self, t0: i64, cx: &mut Context<Self>) {
        if self.prefetching || self.prefetched.is_some() {
            return;
        }
        let (t0, t1) = player_chunk(t0, self.duration_ms);
        if t1 <= t0 {
            return;
        }
        self.prefetching = true;
        let id = self.meeting_id;
        let backend = meetings_backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.meeting_audio(id, t0, t1) })
                .await;
            this.update(cx, |this, _| {
                this.prefetching = false;
                if let Ok(samples) = r {
                    this.prefetched = Some((t0, t1, samples));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Follow the player thread: the playhead, the end of a stretch (load
    /// the next), errors, and another clip taking over.
    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.scrubbing {
            return;
        }
        let Some(c) = &self.chunk else {
            self.playing = false;
            return;
        };
        let st = Playback::global().state();
        if st.path.as_ref() != Some(&c.key) {
            let starting = self.started.is_some_and(|s| s.elapsed() < START_GRACE);
            if !starting {
                // Something else plays now (History, a voice sample).
                self.playing = false;
            }
            return;
        }
        if let Some(e) = st.error {
            self.playing = false;
            self.error = Some(e);
            cx.notify();
            return;
        }
        let (t0, t1) = (c.t0, c.t1);
        self.position_ms = (t0 + st.position.as_millis() as i64).min(self.duration_ms);
        let ended = !st.playing && st.duration > Duration::ZERO && st.position >= st.duration;
        if self.playing && ended {
            if t1 < self.duration_ms {
                self.position_ms = t1;
                self.load(t1, cx);
            } else {
                self.playing = false;
                self.position_ms = self.duration_ms;
            }
        } else if self.playing && !st.playing && !ended {
            let starting = self.started.is_some_and(|s| s.elapsed() < START_GRACE);
            if !starting {
                // Paused from somewhere else.
                self.playing = false;
            }
        }
        if self.playing && t1 < self.duration_ms && t1 - self.position_ms < PREFETCH_MS {
            self.prefetch(t1, cx);
        }
        cx.emit(PlayerEvent::Moved(self.position_ms));
        cx.notify();
    }

    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            let keep = this
                .update(cx, |this, cx| {
                    this.tick(cx);
                    let keep = this.playing || this.loading;
                    if !keep {
                        this.ticking = false;
                        cx.notify();
                    }
                    keep
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    /// The meeting time under `x` on the scrubber.
    fn time_at(&self, x: Pixels) -> Option<i64> {
        let b = self.track.get()?;
        let w = f32::from(b.size.width).max(1.);
        let f = (f32::from(x - b.left()) / w).clamp(0., 1.);
        Some((f as f64 * self.duration_ms as f64) as i64)
    }

    fn render_scrubber(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let fraction = if self.duration_ms > 0 {
            (self.position_ms as f32 / self.duration_ms as f32).clamp(0., 1.)
        } else {
            0.
        };
        let track = self.track.clone();
        let disabled = self.duration_ms <= 0;
        div()
            .id("player-scrubber")
            .relative()
            .flex_1()
            .min_w(dp(40.))
            .h(dp(16.))
            .child(
                canvas(move |b, _, _| track.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(dp(6.))
                    .h(dp(4.))
                    .rounded(design::RADIUS_TRACK)
                    .bg(tone.raised)
                    .child(
                        div()
                            .h_full()
                            .rounded(design::RADIUS_TRACK)
                            .bg(tone.accent)
                            .w(relative(fraction)),
                    ),
            )
            .child(
                // The knob: centred on the playhead.
                div()
                    .absolute()
                    .top(dp(3.))
                    .left(relative(fraction))
                    .ml(dp(-5.))
                    .size(dp(10.))
                    .rounded_full()
                    .bg(tone.surface)
                    .border_1()
                    .border_color(tone.line_art)
                    .shadow_xs(),
            )
            .when(!disabled, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        if let Some(t) = this.time_at(ev.position.x) {
                            this.scrubbing = true;
                            this.position_ms = t;
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                    if this.scrubbing && ev.pressed_button == Some(MouseButton::Left) {
                        if let Some(t) = this.time_at(ev.position.x) {
                            this.position_ms = t;
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        if std::mem::take(&mut this.scrubbing) {
                            this.seek(this.position_ms, cx);
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        if std::mem::take(&mut this.scrubbing) {
                            this.seek(this.position_ms, cx);
                        }
                    }),
                )
            })
    }
}

impl Drop for MeetingPlayer {
    /// A closed meeting stops its audio.
    fn drop(&mut self) {
        if self.owns_playback() {
            Playback::global().stop();
        }
    }
}

impl Render for MeetingPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let danger = cx.theme().danger;
        let disabled = self.duration_ms <= 0;
        let time = |ms: i64| {
            design::mono(format_offset(ms))
                .flex_none()
                .text_size(dp(11.))
                .text_color(tone.text_muted)
        };
        h_flex()
            .id("meeting-player")
            .w_full()
            .flex_none()
            .h(dp(28.))
            // Inside the detail header, which gives the side padding; the
            // first button's icon lines up with the title.
            .ml(dp(-4.))
            .gap(dp(6.))
            .child(
                Button::new("player-back")
                    .ghost()
                    .xsmall()
                    .icon(icon("rotate-ccw"))
                    .tooltip(t("meetings.player.back"))
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.skip(-SKIP_MS, cx))),
            )
            .child(
                Button::new("player-toggle")
                    .ghost()
                    .small()
                    .icon(icon(if self.playing { "pause" } else { "play" }))
                    .tooltip(if self.playing {
                        t("meetings.player.pause")
                    } else {
                        t("meetings.player.play")
                    })
                    .loading(self.loading)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                Button::new("player-forward")
                    .ghost()
                    .xsmall()
                    .icon(icon("rotate-cw"))
                    .tooltip(t("meetings.player.forward"))
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.skip(SKIP_MS, cx))),
            )
            .child(
                time(self.position_ms)
                    .id("player-position")
                    .w(dp(48.))
                    .text_right(),
            )
            // Room for the knob, which overhangs the track's ends.
            .child(
                h_flex()
                    .flex_1()
                    .min_w(dp(40.))
                    .px(dp(6.))
                    .child(self.render_scrubber(cx)),
            )
            .child(time(self.duration_ms))
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .id("player-error")
                        .flex_none()
                        .max_w(dp(160.))
                        .truncate()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(danger)
                        .child(t("meetings.player.unavailable"))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(e.clone()).build(window, cx)
                        })
                        .test_support(),
                )
            })
            .test_support()
    }
}
