//! The recording floater (see [`crate::logic::recording_widget`] for what it
//! shows and [`crate::logic::floater`] for how it moves) while a meeting
//! records:
//! - free, a black rounded rectangle with a small waveform and a stop
//!   button that the user drags anywhere, to any display; a click opens the
//!   panel with the meeting, its sources, the live transcript and the
//!   controls;
//! - docked at a notch, wings around it (waveform left, stop right); a
//!   click grows the notch down into the same panel;
//! - tucked into a corner, a leaf with the waveform and stop; a click opens
//!   the panel beside it.
//!
//! Dragged near a notch or a corner it is drawn in and snaps on; pulled
//! away, it stretches like jelly and breaks free. A click on any waveform
//! pauses (the pause button shows over it on hover). Docking at the notch
//! is the "Recording controls" setting, so the next recording starts where
//! this one was left.
//!
//! Every display has a transparent window over it ([`RecordingWidgetView`]
//! for display `n`), each drawing what of the floater is on it, so it can
//! stretch across displays. A window lets the pointer through everywhere
//! but the floater itself ([`crate::windows::set_ignores_mouse`], polled).

use crate::logic::floater::{
    Corner, Desk, DockChange, Floater, Neck, Release, Stage, Target, P, R,
};
use crate::logic::notch::bar_height;
use crate::logic::recording_widget::{rows, Row, WidgetModel};
use crate::state::AppState;
use crate::ui::design::{self, dp, Tone};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::recorder::now_ms;
use guidy_core::meetings::settings::RecordingControls;
use guidy_core::meetings::types::{LiveStatus, LiveUpdate, RecorderState, Track};
use std::time::{Duration, Instant};

/// Level poll / waveform scroll interval.
const TICK: Duration = Duration::from_millis(90);
/// Pointer poll while shown: whether the window takes the pointer.
const POINTER_POLL: Duration = Duration::from_millis(16);
const WAVE_MAX: f32 = 16.;
const RED: u32 = 0xff453a;
/// Bars in the floater's and the leaf's waveform (the newest).
const BLOB_BARS: usize = 5;
/// Bars in the wings' waveform: it is a pause button, so the rest of the
/// wing is left to grab.
const WING_BARS: usize = 7;

pub struct RecordingWidgetState {
    pub model: WidgetModel,
    /// Docked at the notch (the "Recording controls" setting).
    pub docked: bool,
    /// The displays the floater can be on.
    pub desk: Desk,
    /// Animate (off in tests; reduced motion turns it off too).
    pub animate: bool,
    /// How the floater moves; made when a recording shows. Shared by the
    /// windows of every display.
    floater: Option<Floater>,
    /// The recording the floater is for.
    meeting: Option<i64>,
    /// The docking the floater was last told (the setting).
    docked_applied: bool,
    /// When the floater was last stepped, while it moves.
    last_step: Option<Instant>,
    ticking: bool,
}

pub struct GlobalRecordingWidget(pub Entity<RecordingWidgetState>);
impl Global for GlobalRecordingWidget {}

impl RecordingWidgetState {
    pub fn install(cx: &mut App) -> Entity<RecordingWidgetState> {
        let e = cx.new(|_| RecordingWidgetState {
            model: WidgetModel::default(),
            docked: false,
            desk: Desk::single(Stage::fallback(design::SCALE)),
            animate: true,
            floater: None,
            meeting: None,
            docked_applied: false,
            last_step: None,
            ticking: false,
        });
        cx.set_global(GlobalRecordingWidget(e.clone()));
        e
    }

    pub fn global(cx: &App) -> Option<Entity<RecordingWidgetState>> {
        cx.try_global::<GlobalRecordingWidget>()
            .map(|g| g.0.clone())
    }

    pub fn set_recorder(&mut self, state: RecorderState, cx: &mut Context<Self>) {
        self.model.set_recorder(state);
        if self.model.visible() && !self.ticking {
            self.start_ticking(cx);
        }
        cx.notify();
    }

    pub fn apply_live(&mut self, update: &LiveUpdate, cx: &mut Context<Self>) {
        self.model.apply_live(update);
        cx.notify();
    }

    /// The displays changed (the windows were reopened over them).
    pub fn set_desk(&mut self, desk: Desk) {
        if let Some(f) = &mut self.floater {
            f.restage(&desk);
        }
        self.desk = desk;
    }

    /// Bring the floater up to `now`: made for a new recording where the
    /// setting says, told the setting and whether the panel is open, then
    /// stepped. Each display's window calls this as it draws; the first
    /// call of a frame does the stepping. Returns whether it still moves.
    fn frame(&mut self, now: Instant, reduce_motion: bool) -> bool {
        if !self.model.visible() {
            self.floater = None;
            self.meeting = None;
            self.last_step = None;
            return false;
        }
        if self.floater.is_none() || self.meeting != self.model.meeting_id() {
            self.floater = Some(Floater::new(&self.desk, self.docked));
            self.meeting = self.model.meeting_id();
            self.docked_applied = self.docked;
            self.last_step = None;
        }
        let Some(f) = self.floater.as_mut() else {
            return false;
        };
        f.animate = self.animate && !reduce_motion;
        if self.docked != self.docked_applied {
            f.set_docked(&self.desk, self.docked);
            self.docked_applied = self.docked;
        }
        f.set_expanded(self.model.expanded);
        let dt = self
            .last_step
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(1.0 / 60.0);
        f.step(dt);
        let animating = f.animating();
        self.last_step = animating.then_some(now);
        animating
    }

    pub fn floater(&self) -> Option<&Floater> {
        self.floater.as_ref()
    }

    /// Poll the input levels (and redraw the timer) while visible.
    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            let keep = this
                .update(cx, |s, cx| {
                    if !s.model.visible() {
                        s.ticking = false;
                        return false;
                    }
                    let levels = AppState::global(cx)
                        .read(cx)
                        .meetings
                        .take_recording_levels();
                    s.model.push_levels(levels);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }
}

/// Open the panel (or close it back to the floater or the wings).
pub fn set_expanded(expanded: bool, cx: &mut App) {
    if let Some(state) = RecordingWidgetState::global(cx) {
        state.update(cx, |s, cx| {
            s.model.expanded = expanded;
            cx.notify();
        });
    }
}

fn set_paused(paused: bool, cx: &mut App) {
    let backend = AppState::global(cx).read(cx).meetings.clone();
    cx.background_spawn(async move { backend.set_recording_paused(paused) })
        .detach();
}

fn stop(cx: &mut App) {
    let backend = AppState::global(cx).read(cx).meetings.clone();
    cx.background_spawn(async move {
        if let Err(e) = backend.stop_recording() {
            log::error!("Stopping the meeting from the recording widget failed: {e}");
        }
    })
    .detach();
}

fn open_notes(meeting_id: i64, cx: &mut App) {
    crate::windows::show_main(cx);
    if let Some(main) = crate::windows::main_window_entity(cx) {
        main.update(cx, |m, cx| {
            m.open_meeting_link(
                crate::logic::meetings::MeetingLink {
                    meeting_id,
                    t_ms: None,
                },
                cx,
            )
        });
    }
}

/// Docked at the notch or not, from now on (and for the next recording).
/// A corner counts as not: the next recording starts free.
fn remember_docked(docked: bool, cx: &mut App) {
    if let Some(state) = RecordingWidgetState::global(cx) {
        state.update(cx, |s, _| s.docked = docked);
    }
    let controls = if docked {
        RecordingControls::Notch
    } else {
        RecordingControls::Floating
    };
    crate::ui::pages::meetings::settings::update_meetings(cx, |m| m.recording_controls = controls);
}

/// A mouse-down listener on the floater.
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

pub struct RecordingWidgetView {
    /// The display this window covers (an index into the desk).
    screen: usize,
    state: Option<Entity<RecordingWidgetState>>,
    scroll: ScrollHandle,
    /// Follow new lines (the user hasn't scrolled up).
    pinned: bool,
    /// The window takes the pointer now.
    accepting: bool,
    _sub: Option<Subscription>,
}

impl RecordingWidgetView {
    pub fn new(screen: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = RecordingWidgetState::global(cx);
        let sub = state
            .as_ref()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()));
        let animate = state.as_ref().is_some_and(|s| s.read(cx).animate);
        if animate {
            Self::poll_pointer(window, cx);
        }
        Self {
            screen,
            state,
            scroll: ScrollHandle::new(),
            pinned: true,
            // The window opens taking the pointer; the first poll fixes it.
            accepting: true,
            _sub: sub,
        }
    }

    /// Let the pointer through the window except over the floater: the
    /// window covers its display, the floater a few small shapes.
    fn poll_pointer(window: &mut Window, cx: &mut Context<Self>) {
        let Some(ns_window) = crate::windows::native_window(window) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(POINTER_POLL).await;
            let change = this.update(cx, |this, cx| {
                let state = this.state.clone()?;
                let s = state.read(cx);
                let floater = s.floater.as_ref()?;
                let (gx, gy) = crate::windows::pointer_location()?;
                let p = s.desk.from_cocoa(gx, gy);
                let hit = s.desk.screen_rect(this.screen).contains(p) && floater.hit(&s.desk, p);
                (hit != this.accepting).then(|| {
                    this.accepting = hit;
                    !hit
                })
            });
            match change {
                // Outside the update: AppKit answers at once.
                Ok(Some(ignore)) => crate::windows::set_ignores_mouse(ns_window, ignore),
                Ok(None) => {}
                Err(_) => break,
            }
        })
        .detach();
    }

    fn sync_pinned(&mut self, cx: &mut Context<Self>) {
        let scrolled = f32::from(-self.scroll.offset().y);
        let max = f32::from(self.scroll.max_offset().y);
        let pinned = crate::logic::overlay::is_pinned(max, scrolled, 0.0);
        if self.pinned != pinned {
            self.pinned = pinned;
            cx.notify();
        }
    }

    /// A point in this window to the desktop.
    fn global(&self, p: Point<Pixels>, cx: &App) -> P {
        let o = self
            .state
            .as_ref()
            .map(|s| s.read(cx).desk.origin(self.screen))
            .unwrap_or_default();
        P::new(f32::from(p.x) + o.x, f32::from(p.y) + o.y)
    }

    fn on_press(&mut self, p: P, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        state.update(cx, |s, _| {
            if let Some(f) = &mut s.floater {
                f.press(p);
            }
        });
        cx.notify();
    }

    fn on_move(&mut self, p: P, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let change = state.update(cx, |s, _| {
            let desk = s.desk.clone();
            let f = s.floater.as_mut()?;
            if !f.pressed() {
                return None;
            }
            let change = f.drag_to(&desk, p);
            if let Some(change) = change {
                // Only the notch is the setting.
                let docked = matches!(change, DockChange::Docked(Target::Notch(_)));
                s.docked = docked;
                s.docked_applied = docked;
            }
            Some(change)
        });
        if let Some(Some(change)) = change {
            let docked = matches!(change, DockChange::Docked(Target::Notch(_)));
            cx.defer(move |cx| remember_docked(docked, cx));
        }
        cx.notify();
    }

    fn on_release(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let click = state.update(cx, |s, _| {
            let desk = s.desk.clone();
            let f = s.floater.as_mut()?;
            Some(f.release(&desk) == Release::Click && !f.expanded())
        });
        if click == Some(true) {
            cx.defer(|cx| set_expanded(true, cx));
        }
        cx.notify();
    }
}

/// `r` (desktop) in the window of the display at `o`.
fn local(r: R, o: P) -> R {
    R {
        x: r.x - o.x,
        y: r.y - o.y,
        ..r
    }
}

fn black() -> Hsla {
    hsla(0., 0., 0., 1.)
}

fn place(r: R) -> Div {
    div()
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
}

/// The recording dot: red and breathing while recording, amber and still
/// while paused.
fn live_dot(paused: bool, tone: Tone, cx: &App) -> AnyElement {
    let dot = div().size(dp(8.)).rounded_full();
    if paused {
        return dot.bg(tone.accent).into_any_element();
    }
    let dot = dot.bg(rgb(RED));
    if cx.reduce_motion() {
        return dot.into_any_element();
    }
    dot.with_animation(
        "recording-widget-dot",
        Animation::new(Duration::from_millis(1600)).repeat(),
        |el, delta| el.opacity(0.45 + 0.55 * (delta * std::f32::consts::TAU).cos().abs()),
    )
    .into_any_element()
}

/// The newest `bars` levels as bars up to `max` tall.
fn wave(model: &WidgetModel, bars: usize, max: f32, fg: Hsla) -> Div {
    let levels: Vec<f32> = model.wave().collect();
    let from = levels.len().saturating_sub(bars);
    h_flex()
        .gap(dp(2.5))
        .h(dp(max))
        .items_center()
        .when(model.paused(), |d| d.opacity(0.35))
        .children(levels[from..].iter().map(move |v| {
            div()
                .w(dp(3.))
                .h(dp(bar_height(*v, max)))
                .rounded(dp(1.5))
                .bg(fg)
        }))
}

/// The red stop button of the floater and the wings, at `r`.
fn stop_button(id: &'static str, r: R) -> Stateful<Div> {
    place(r)
        .id(id)
        .aria_label(t("notch.stop"))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(hsla(0., 0., 1., 0.14))
        .hover(|s| s.bg(hsla(0., 0., 1., 0.26)))
        .child(
            div()
                .size(px(r.w * 0.36))
                .rounded(px(r.w * 0.09))
                .bg(rgb(RED)),
        )
        .on_click(|_, _, cx| stop(cx))
}

/// The stop button inside the floater (or the leaf) `b`.
fn blob_stop_rect(b: R, stage: &Stage) -> R {
    let (_, stop_x) = blob_columns(b, stage);
    let s = blob_stop_size(b, stage);
    R::centered(P::new(stop_x, b.center().y), s, s)
}

fn blob_stop_size(b: R, stage: &Stage) -> f32 {
    stage.d(26.).min(b.h * 0.64)
}

/// The centers (x) of the waveform and of the stop button in the floater
/// (or the leaf) `b`: the same gap before, between and after them.
fn blob_columns(b: R, stage: &Stage) -> (f32, f32) {
    let wave = wave_width(BLOB_BARS, stage);
    let stop = blob_stop_size(b, stage);
    let gap = ((b.w - wave - stop) / 3.).max(0.);
    (b.x + gap + wave / 2., b.x + 2. * gap + wave + stop / 2.)
}

/// The wings' stop button: in the middle of the right wing (`housing`:
/// the camera's part of the wings `w`).
fn wings_stop_rect(w: R, housing: R, stage: &Stage) -> R {
    let s = stage.d(20.).min(w.h - 6.);
    R::centered(
        P::new((housing.right() + w.right()) / 2.0, w.y + w.h / 2.0),
        s,
        s,
    )
}

/// Width of a waveform of `bars` bars.
fn wave_width(bars: usize, stage: &Stage) -> f32 {
    stage.d(3. * bars as f32 + 2.5 * bars.saturating_sub(1) as f32)
}

/// The waveform's clickable area (it pauses), centered at `c`.
fn wave_area(c: P, bars: usize, h: f32, stage: &Stage) -> R {
    R::centered(c, wave_width(bars, stage) + stage.d(10.), h)
}

/// The liquid bridge from a docked shape to the blob as a filled path:
/// flared where it leaves the shape's edge, thinning toward the middle,
/// swelling into the blob. In window coordinates.
fn neck_path(n: Neck) -> Option<Path<Pixels>> {
    let d = n.b - n.a;
    let len = d.len();
    if len < 2.0 || n.half_a < 0.5 {
        return None;
    }
    let u = d.unit();
    // Left of the direction of the pull, as seen on screen.
    let mut side = u.perp();
    if side.x > 0.0 || (side.x == 0.0 && side.y > 0.0) {
        side = side * -1.0;
    }
    // Along the shape's edge where the neck leaves it, on the same side.
    let mut edge = n.normal.perp();
    if edge.x * side.x + edge.y * side.y < 0.0 {
        edge = edge * -1.0;
    }
    let pt = |p: P| point(px(p.x), px(p.y));
    let m = n.a + d * 0.5;
    let (a_l, a_r) = (n.a + edge * n.half_a, n.a - edge * n.half_a);
    let (m_l, m_r) = (m + side * n.waist, m - side * n.waist);
    let (b_l, b_r) = (n.b + side * n.half_b, n.b - side * n.half_b);
    // The flare leaves along the edge's normal; it starts a little inside.
    let out = n.normal * (len * 0.3);
    let inside = n.normal * -3.0;
    let mut path = PathBuilder::fill();
    path.move_to(pt(a_l + inside));
    path.line_to(pt(a_l));
    path.cubic_bezier_to(pt(m_l), pt(a_l + out), pt(m_l - u * (len * 0.22)));
    path.cubic_bezier_to(
        pt(b_l),
        pt(m_l + u * (len * 0.22)),
        pt(b_l - u * (len * 0.12)),
    );
    path.line_to(pt(b_r));
    path.cubic_bezier_to(
        pt(m_r),
        pt(b_r - u * (len * 0.12)),
        pt(m_r + u * (len * 0.22)),
    );
    path.cubic_bezier_to(pt(a_r), pt(m_r - u * (len * 0.22)), pt(a_r + out));
    path.line_to(pt(a_r + inside));
    path.close();
    path.build().ok()
}

/// The recording dot, "Recording" (or "Paused") and the timer.
fn panel_status(m: &WidgetModel, tone: Tone, cx: &App) -> Div {
    h_flex()
        .gap(dp(8.))
        .items_center()
        .child(live_dot(m.paused(), tone, cx))
        .child(
            design::eyebrow(
                if m.paused() {
                    t("meetings.widget.paused")
                } else {
                    t("meetings.widget.recording")
                },
                cx,
            )
            .text_color(tone.text_muted),
        )
        .child(
            div()
                .font_family(crate::theme::MONO_FONT)
                .text_size(design::TEXT_MONO)
                .text_color(tone.text_muted)
                .child(m.timer(now_ms())),
        )
}

fn collapse_button(tone: Tone) -> impl IntoElement {
    icon_button(
        "recording-widget-collapse",
        "minimize-2",
        t("meetings.widget.collapse"),
        tone,
    )
    .on_click(|_, _, cx| set_expanded(false, cx))
    .test_support()
}

/// The pause (or resume) sign shown over a waveform: while the pointer is
/// over `group` (the waveform's area), always while paused.
fn pause_sign(id: &'static str, size: f32, paused: bool, group: &'static str) -> Stateful<Div> {
    let (name, label) = if paused {
        ("play", t("notch.resume"))
    } else {
        ("pause", t("notch.pause"))
    };
    div()
        .id(id)
        .aria_label(label)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(0., 0., 1., 0.18))
        .group_hover(group, |s| s.bg(hsla(0., 0., 1., 0.3)))
        .text_color(hsla(0., 0., 1., 0.95))
        .child(icon(name).xsmall())
        .when(!paused, |d| {
            d.opacity(0.).group_hover(group, |s| s.opacity(1.))
        })
}

/// A waveform at `r` that is also the pause button: hovered, it gives way
/// to a centered [`pause_sign`] (`sign`: its id and size); clicked, it
/// pauses (or resumes). The sign is inside the area: GPUI applies group
/// hover to descendants only.
fn wave_button(
    group: &'static str,
    r: R,
    m: &WidgetModel,
    bars: usize,
    max: f32,
    sign: Option<(&'static str, f32)>,
) -> Stateful<Div> {
    let paused = m.paused();
    let fg = hsla(0., 0., 1., 0.92);
    place(r)
        .id(group)
        .group(group)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(r.h / 2.))
        .when(sign.is_some(), |d| {
            d.cursor_pointer()
                .on_click(move |_, _, cx| set_paused(!paused, cx))
        })
        .child(wave(m, bars, max, fg).when(!paused, |d| d.group_hover(group, |s| s.opacity(0.15))))
        .when_some(sign, |d, (id, size)| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pause_sign(id, size, paused, group).test_support()),
            )
        })
}

/// A round icon button.
fn icon_button(id: &'static str, name: &str, label: SharedString, tone: Tone) -> Stateful<Div> {
    div()
        .id(id)
        .aria_label(label)
        .size(dp(28.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(tone.text_muted)
        .hover(move |s| s.bg(tone.raised).text_color(tone.text))
        .child(icon(name).small())
}

/// A labelled pill button.
fn pill(id: &'static str, name: &str, label: SharedString, tone: Tone) -> Stateful<Div> {
    h_flex()
        .id(id)
        .aria_label(label.clone())
        .h(dp(30.))
        .px(dp(12.))
        .gap(dp(6.))
        .items_center()
        .rounded(design::RADIUS_PILL)
        .cursor_pointer()
        .text_size(dp(12.))
        .bg(tone.raised)
        .text_color(tone.text)
        .hover(move |s| s.bg(tone.raised.opacity(0.7)))
        .child(icon(name).xsmall())
        .child(label)
}

/// One audio source: its icon, name and a three-bar activity meter.
fn source_chip(model: &WidgetModel, track: Track, tone: Tone) -> AnyElement {
    let (id, name, label) = match track {
        Track::Mic => ("source-mic", "mic", t("meetings.widget.mic")),
        Track::System => ("source-system", "volume-2", t("meetings.widget.system")),
    };
    let level = model.source_level(track);
    let active = model.source_active(track);
    let meter = h_flex()
        .gap(dp(2.))
        .h(dp(10.))
        .items_end()
        .children([0.06f32, 0.25, 0.5].map(|step| {
            let lit = level >= step;
            div()
                .w(dp(2.5))
                .h(dp(4. + step * 12.))
                .rounded(dp(1.))
                .bg(if lit { tone.accent } else { tone.hairline })
        }));
    h_flex()
        .id(id)
        .gap(dp(6.))
        .px(dp(10.))
        .py(dp(5.))
        .items_center()
        .rounded(design::RADIUS_PILL)
        .border_1()
        .border_color(tone.hairline)
        .bg(tone.surface)
        .text_size(dp(12.))
        .text_color(if active { tone.text } else { tone.text_muted })
        .child(icon(name).xsmall())
        .child(label)
        .child(meter)
        .test_support()
        .into_any_element()
}

fn transcript_row(i: usize, row: &Row, tone: Tone, window: &Window, cx: &App) -> AnyElement {
    let you = row.track == Track::Mic;
    let label = if you {
        t("meetings.widget.you")
    } else {
        t("meetings.widget.others")
    };
    let full = match (row.text.is_empty(), row.tentative.is_empty()) {
        (_, true) => row.text.clone(),
        (true, false) => row.tentative.clone(),
        (false, false) => format!("{} {}", row.text, row.tentative),
    };
    let tentative_from = full.len() - row.tentative.len();
    // This window has no gpui-kit Root: the base style is set here.
    let mut style = window.text_style();
    style.color = tone.text;
    style.font_family = crate::theme::UI_FONT.into();
    style.font_size = design::TEXT_BODY.into();
    style.line_height = relative(1.5);
    let text = StyledText::new(full.clone()).with_default_highlights(
        &style,
        (tentative_from < full.len()).then(|| {
            (
                tentative_from..full.len(),
                HighlightStyle {
                    color: Some(tone.text_muted),
                    ..Default::default()
                },
            )
        }),
    );
    v_flex()
        .id(SharedString::from(format!("live-row-{i}")))
        .gap(dp(4.))
        .child(design::eyebrow(label, cx).text_color(if you {
            tone.accent_text
        } else {
            tone.text_muted
        }))
        .child(
            div()
                .text_size(design::TEXT_BODY)
                .line_height(relative(1.5))
                .text_color(tone.text)
                .child(text),
        )
        .test_support()
        .into_any_element()
}

/// What the transcript area says when it has no lines.
fn transcript_notice(status: &LiveStatus, tone: Tone, cx: &App) -> AnyElement {
    let body = div()
        .text_size(dp(12.))
        .line_height(relative(1.5))
        .text_color(tone.text_muted);
    match status {
        LiveStatus::Starting => body
            .child(design::shimmer_text(
                "live-starting",
                t("meetings.widget.liveStarting"),
                cx,
            ))
            .into_any_element(),
        LiveStatus::Live => body
            .child(t("meetings.widget.liveListening"))
            .into_any_element(),
        LiveStatus::Off => body.child(t("meetings.widget.liveOff")).into_any_element(),
        LiveStatus::Unavailable { reason } => body
            .child(t_args(
                "meetings.widget.liveUnavailable",
                &[("reason", reason)],
            ))
            .into_any_element(),
    }
}

impl RecordingWidgetView {
    /// Presses on the floater (outside its buttons) start a drag or a click.
    fn press_handler(
        &self,
        avoid: Vec<R>,
        cx: &mut Context<Self>,
    ) -> PressHandler {
        Box::new(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
            let at = P::new(ev.position.x.into(), ev.position.y.into());
            if avoid.iter().any(|r| r.inflate(2.).contains(at)) {
                return;
            }
            let p = this.global(ev.position, cx);
            this.on_press(p, cx);
        }))
    }

    /// The free floater: a black rounded rectangle, the waveform and the
    /// stop button evenly spaced in it.
    fn render_blob(
        &self,
        m: &WidgetModel,
        f: &Floater,
        desk: &Desk,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let stage = desk.stage(self.screen);
        let b = local(f.blob(desk)?, desk.origin(self.screen));
        let content = f.blob_content();
        let wave_c = P::new(blob_columns(b, stage).0, b.center().y);
        let stop_rect = blob_stop_rect(b, stage);
        let wave_rect = wave_area(wave_c, BLOB_BARS, b.h - stage.d(8.), stage);
        let blob = place(b)
            .id("recording-widget-blob")
            .aria_label(t("meetings.widget.expand"))
            .rounded(px(stage.d(12.).min(b.h * 0.3)))
            .bg(black())
            .when(content > 0.0, |d| {
                d.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.35 * content),
                    offset: point(px(0.), dp(6.)),
                    blur_radius: dp(18.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .cursor(if f.dragging() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_mouse_down(
                MouseButton::Left,
                self.press_handler(vec![stop_rect, wave_rect], cx),
            )
            .test_support();
        if content <= 0.0 {
            return Some(blob.into_any_element());
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .child(blob)
                .child(
                    wave_button(
                        "blob-wave",
                        wave_rect,
                        m,
                        BLOB_BARS,
                        12.,
                        (content >= 1.0).then_some(("recording-widget-pause-blob", stage.d(24.))),
                    )
                    .opacity(content)
                    .test_support(),
                )
                .child(
                    stop_button("recording-widget-stop-blob", stop_rect)
                        .opacity(content)
                        .test_support(),
                )
                .into_any_element(),
        )
    }

    /// The docked shape: wings around a notch (or the panel growing out of
    /// them), or a leaf in a corner.
    fn render_shape(
        &mut self,
        m: &WidgetModel,
        f: &Floater,
        desk: &Desk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let o = desk.origin(self.screen);
        let w = local(f.shape(desk)?, o);
        match f.target {
            Target::Notch(_) => Some(self.render_wings(m, f, desk, w, window, cx)),
            Target::Corner(_, corner) => Some(self.render_leaf(m, f, desk, w, corner, cx)),
        }
    }

    fn render_wings(
        &mut self,
        m: &WidgetModel,
        f: &Floater,
        desk: &Desk,
        w: R,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stage = desk.stage(self.screen);
        let content = f.shape_content();
        // The panel shows on the notch's own display.
        let panel = if f.target.screen() == self.screen {
            f.docked_panel_content()
        } else {
            0.0
        };
        let expand = f.expand.value.clamp(0.0, 1.0);
        let radius = stage.d(12.) + (stage.d(22.) - stage.d(12.)) * expand;
        let housing = local(desk.housing(f.target), desk.origin(self.screen));
        let stop_rect = wings_stop_rect(w, housing, stage);
        // The waveform in the middle of the left wing.
        let left = R {
            x: w.x,
            y: w.y,
            w: (housing.x - w.x).max(0.),
            h: w.h,
        };
        let wave_rect = wave_area(left.center(), WING_BARS, w.h - 6., stage);
        let pressable = f.is_docked() && !f.expanded();
        let top_inset = stage.top_inset;
        let shape = place(w)
            .id("recording-widget-wings")
            .aria_label(t("meetings.widget.expand"))
            .bg(black())
            .rounded_bl(px(radius))
            .rounded_br(px(radius))
            .overflow_hidden()
            .when(pressable, |d| {
                d.cursor(CursorStyle::OpenHand).on_mouse_down(
                    MouseButton::Left,
                    self.press_handler(vec![stop_rect, wave_rect], cx),
                )
            })
            .when(panel > 0.0, |d| {
                let tone = design::tone_on_black();
                // The band beside the notch: status on the left of the
                // camera, collapse on the right.
                let band = h_flex()
                    .w_full()
                    .h(px(top_inset))
                    .flex_none()
                    .items_center()
                    .child(
                        h_flex()
                            .w(px((housing.x - w.x).max(0.)))
                            .flex_none()
                            .pl(dp(16.))
                            .child(panel_status(m, tone, cx)),
                    )
                    .child(div().w(px(housing.w)).flex_none())
                    .child(
                        h_flex()
                            .flex_1()
                            .justify_end()
                            .pr(dp(10.))
                            .child(collapse_button(tone)),
                    );
                d.child(
                    v_flex().size_full().opacity(panel).child(band).child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_panel(m, tone, false, window, cx)),
                    ),
                )
            })
            .test_support();
        let mut layer = div().absolute().inset_0().child(shape);
        if content > 0.0 {
            layer = layer
                .child(
                    wave_button(
                        "wings-wave",
                        wave_rect,
                        m,
                        WING_BARS,
                        WAVE_MAX,
                        (content >= 1.0).then_some(("recording-widget-pause-wings", stop_rect.w)),
                    )
                    .opacity(content)
                    .test_support(),
                )
                .child(
                    stop_button("recording-widget-stop-wings", stop_rect)
                        .opacity(content)
                        .test_support(),
                );
        }
        layer.into_any_element()
    }

    /// The leaf in a corner: flush with the display's edges there, its
    /// opposite corner rounded like a leaf; the waveform and stop inside.
    fn render_leaf(
        &self,
        m: &WidgetModel,
        f: &Floater,
        desk: &Desk,
        r: R,
        corner: Corner,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stage = desk.stage(self.screen);
        let content = f.shape_content();
        let big = px((r.h * 0.5).min(stage.d(20.)));
        let small = px(stage.d(8.).min(r.h * 0.3));
        let (tl, tr, bl, br) = match corner {
            Corner::TopLeft => (px(0.), small, small, big),
            Corner::TopRight => (small, px(0.), big, small),
            Corner::BottomLeft => (small, big, px(0.), small),
            Corner::BottomRight => (big, small, small, px(0.)),
        };
        let wave_c = P::new(blob_columns(r, stage).0, r.center().y);
        let stop_rect = blob_stop_rect(r, stage);
        let wave_rect = wave_area(wave_c, BLOB_BARS, r.h - stage.d(8.), stage);
        let shape = place(r)
            .id("recording-widget-leaf")
            .aria_label(t("meetings.widget.expand"))
            .bg(black())
            .rounded_tl(tl)
            .rounded_tr(tr)
            .rounded_bl(bl)
            .rounded_br(br)
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                self.press_handler(vec![stop_rect, wave_rect], cx),
            )
            .test_support();
        let mut layer = div().absolute().inset_0().child(shape);
        if content > 0.0 {
            layer = layer
                .child(
                    wave_button(
                        "leaf-wave",
                        wave_rect,
                        m,
                        BLOB_BARS,
                        12.,
                        (content >= 1.0).then_some(("recording-widget-pause-leaf", stage.d(22.))),
                    )
                    .opacity(content)
                    .test_support(),
                )
                .child(
                    stop_button("recording-widget-stop-leaf", stop_rect)
                        .opacity(content)
                        .test_support(),
                );
        }
        layer.into_any_element()
    }

    fn render_neck(&self, f: &Floater, desk: &Desk) -> Option<AnyElement> {
        let n = f.neck(desk)?;
        let o = desk.origin(self.screen);
        let n = Neck {
            a: n.a - o,
            b: n.b - o,
            ..n
        };
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    if let Some(path) = neck_path(n) {
                        window.paint_path(path, black());
                    }
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// The panel opened from the free floater or from a leaf, on the
    /// display the floater is on.
    fn render_free_panel(
        &mut self,
        m: &WidgetModel,
        f: &Floater,
        desk: &Desk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if f.screen(desk) != self.screen {
            return None;
        }
        let r = local(f.free_panel(desk)?, desk.origin(self.screen));
        let tone = design::tone(cx);
        Some(
            place(r)
                .child(
                    v_flex()
                        .id("recording-widget-panel-frame")
                        .relative()
                        .size_full()
                        .rounded(design::RADIUS_WELL)
                        .border_1()
                        .border_color(tone.hairline)
                        .bg(tone.surface.opacity(0.98))
                        .overflow_hidden()
                        .child(design::rim_light(cx))
                        .child(self.render_panel(m, tone, true, window, cx))
                        .with_animation(
                            "recording-widget-open",
                            Animation::new(Duration::from_millis(220))
                                .with_easing(ease_out_quint()),
                            |el, delta| el.opacity(delta),
                        ),
                )
                .into_any_element(),
        )
    }

    /// What is being recorded, its sources, the live transcript and the
    /// controls.
    fn render_panel(
        &mut self,
        m: &WidgetModel,
        tone: Tone,
        with_header: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let paused = m.paused();
        let meeting_id = m.meeting_id();
        let status = m.live_status();
        let rows = m.live().map(|l| rows(&l.lines)).unwrap_or_default();

        // Docked, these sit beside the notch instead (`render_wings`).
        let header = with_header.then(|| {
            h_flex()
                .w_full()
                .px(dp(16.))
                .pt(dp(12.))
                .gap(dp(8.))
                .items_center()
                .child(panel_status(m, tone, cx))
                .child(div().flex_1())
                .child(collapse_button(tone))
        });

        let title = div()
            .id("recording-widget-title")
            .w_full()
            .px(dp(16.))
            .pt(dp(2.))
            .truncate()
            .text_size(design::TEXT_TITLE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(tone.text)
            .child(SharedString::from(m.title().to_string()))
            .test_support();

        let sources = h_flex()
            .w_full()
            .px(dp(16.))
            .pt(dp(10.))
            .pb(dp(14.))
            .gap(dp(8.))
            .border_b_1()
            .border_color(tone.hairline)
            .child(source_chip(m, Track::Mic, tone))
            .child(source_chip(m, Track::System, tone));

        let body: Vec<AnyElement> = if rows.is_empty() {
            vec![transcript_notice(&status, tone, cx)]
        } else {
            rows.iter()
                .enumerate()
                .map(|(i, r)| transcript_row(i, r, tone, window, cx))
                .collect()
        };
        if self.pinned {
            self.scroll.scroll_to_bottom();
        }
        let transcript = div()
            .id("recording-widget-transcript")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.defer_in(window, |this, _, cx| this.sync_pinned(cx));
            }))
            .child(
                v_flex()
                    .w_full()
                    .px(dp(16.))
                    .py(dp(14.))
                    .gap(dp(14.))
                    .children(body),
            )
            .test_support();

        let (pause_icon, pause_label) = if paused {
            ("play", t("meetings.widget.resume"))
        } else {
            ("pause", t("meetings.widget.pause"))
        };
        let footer = h_flex()
            .w_full()
            .px(dp(12.))
            .py(dp(10.))
            .gap(dp(8.))
            .items_center()
            .border_t_1()
            .border_color(tone.hairline)
            .child(
                pill("recording-widget-pause", pause_icon, pause_label, tone)
                    .on_click(move |_, _, cx| set_paused(!paused, cx))
                    .test_support(),
            )
            .child(
                pill(
                    "recording-widget-stop",
                    "square",
                    t("meetings.widget.stop"),
                    tone,
                )
                .text_color(rgb(RED))
                .on_click(|_, _, cx| stop(cx))
                .test_support(),
            )
            .child(div().flex_1())
            .when_some(meeting_id, |d, id| {
                d.child(
                    pill(
                        "recording-widget-open",
                        "arrow-up-right",
                        t("meetings.widget.openNotes"),
                        tone,
                    )
                    .bg(gpui_kit::transparent_black())
                    .text_color(tone.text_muted)
                    .on_click(move |_, _, cx| open_notes(id, cx))
                    .test_support(),
                )
            });

        v_flex()
            .id("recording-widget-panel")
            .size_full()
            .children(header)
            .child(title)
            .child(sources)
            .child(transcript)
            .child(footer)
            .test_support()
            .into_any_element()
    }
}

impl Render for RecordingWidgetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.clone() else {
            return div().into_any_element();
        };
        let reduce_motion = cx.reduce_motion();
        let animating = state.update(cx, |s, _| s.frame(Instant::now(), reduce_motion));
        let (m, desk, f) = {
            let s = state.read(cx);
            (s.model.clone(), s.desk.clone(), s.floater.clone())
        };
        let Some(f) = f.filter(|_| m.visible()) else {
            return div().size_full().into_any_element();
        };
        if animating {
            window.request_animation_frame();
        }

        let neck = self.render_neck(&f, &desk);
        let shape = self.render_shape(&m, &f, &desk, window, cx);
        let blob = self.render_blob(&m, &f, &desk, cx);
        let panel = self.render_free_panel(&m, &f, &desk, window, cx);

        // A drag goes on outside this window (onto another display): the
        // window that took the press keeps getting the pointer, so these
        // listen to the window, not to an element under the pointer.
        let view = cx.entity().downgrade();
        let pointer = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moves = view.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble
                        || ev.pressed_button != Some(MouseButton::Left)
                    {
                        return;
                    }
                    moves
                        .update(cx, |this, cx| {
                            let p = this.global(ev.position, cx);
                            this.on_move(p, cx);
                        })
                        .ok();
                });
                let ups = view.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                        ups.update(cx, |this, cx| this.on_release(cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_full();

        div()
            .id("recording-widget")
            .relative()
            .size_full()
            .font_family(crate::theme::UI_FONT)
            .text_color(design::tone(cx).text)
            .child(pointer)
            .children(neck)
            .children(shape)
            .children(blob)
            .children(panel)
            .test_support()
            .into_any_element()
    }
}
