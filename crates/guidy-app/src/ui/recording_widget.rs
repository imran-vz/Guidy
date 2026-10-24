//! The recording floater (see [`crate::logic::recording_widget`] for what it
//! shows and [`crate::logic::floater`] for how it moves) while a meeting
//! records:
//! - free, a black rounded rectangle with a small waveform and a stop button that
//!   the user drags anywhere; a click opens the panel with the meeting, its
//!   sources, the live transcript and the controls;
//! - docked, wings around the notch (waveform left, stop right); a click
//!   grows the notch down into the same panel.
//!
//! Dragged near the notch it is drawn in and snaps on; pulled away from it,
//! it stretches like jelly and breaks free. Docking is the "Recording
//! controls" setting, so the next recording starts where this one was left.
//!
//! The view covers the whole display in a transparent window (one canvas
//! for the blob, the wings and the neck between them), which lets the
//! pointer through everywhere but the floater itself
//! ([`crate::windows::set_ignores_mouse`], polled).

use crate::logic::floater::{Floater, Mode, Neck, Release, Stage, P, R};
use crate::logic::notch::{bar_height, ScreenInfo};
use crate::logic::recording_widget::{rows, Row, WidgetModel, BARS};
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
/// Bars in the floater's waveform (the newest).
const BLOB_BARS: usize = 5;

pub struct RecordingWidgetState {
    pub model: WidgetModel,
    /// Docked at the notch (the "Recording controls" setting).
    pub docked: bool,
    /// The display the floater's window covers.
    pub screen: Option<ScreenInfo>,
    /// Animate (off in tests; reduced motion turns it off too).
    pub animate: bool,
    ticking: bool,
}

pub struct GlobalRecordingWidget(pub Entity<RecordingWidgetState>);
impl Global for GlobalRecordingWidget {}

impl RecordingWidgetState {
    pub fn install(cx: &mut App) -> Entity<RecordingWidgetState> {
        let e = cx.new(|_| RecordingWidgetState {
            model: WidgetModel::default(),
            docked: false,
            screen: None,
            animate: true,
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

    fn stage(&self) -> Stage {
        self.screen
            .map(|s| Stage::from_screen(&s, design::SCALE))
            .unwrap_or_else(|| Stage::fallback(design::SCALE))
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

/// Docked or not, from now on (and for the next recording).
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

pub struct RecordingWidgetView {
    state: Option<Entity<RecordingWidgetState>>,
    scroll: ScrollHandle,
    /// Follow new lines (the user hasn't scrolled up).
    pinned: bool,
    /// How the floater moves; made when a recording shows.
    floater: Option<Floater>,
    /// The recording the floater is for.
    meeting: Option<i64>,
    /// The docking the floater was last told (the setting).
    docked_applied: bool,
    /// When the last animation frame was drawn.
    last_frame: Option<Instant>,
    /// The window takes the pointer now.
    accepting: bool,
    _sub: Option<Subscription>,
}

impl RecordingWidgetView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = RecordingWidgetState::global(cx);
        let sub = state
            .as_ref()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()));
        let animate = state.as_ref().is_some_and(|s| s.read(cx).animate);
        if animate {
            Self::poll_pointer(window, cx);
        }
        Self {
            state,
            scroll: ScrollHandle::new(),
            pinned: true,
            floater: None,
            meeting: None,
            docked_applied: false,
            last_frame: None,
            // The window opens taking the pointer; the first poll fixes it.
            accepting: true,
            _sub: sub,
        }
    }

    /// Let the pointer through the window except over the floater: the
    /// window covers the display, the floater a few small shapes.
    fn poll_pointer(window: &mut Window, cx: &mut Context<Self>) {
        let Some(ns_window) = crate::windows::native_window(window) else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(POINTER_POLL).await;
            let change = this.update(cx, |this, cx| {
                let state = this.state.clone()?;
                let s = state.read(cx);
                let (screen, floater) = (s.screen?, this.floater.as_ref()?);
                let (gx, gy) = crate::windows::pointer_location()?;
                // Cocoa (bottom-left origin) to the stage (top-left).
                let p = P::new(
                    (gx - screen.x) as f32,
                    (screen.y + screen.height - gy) as f32,
                );
                let hit = floater.hit(&s.stage(), p);
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

    fn stage(&self, cx: &App) -> Stage {
        self.state
            .as_ref()
            .map(|s| s.read(cx).stage())
            .unwrap_or_else(|| Stage::fallback(design::SCALE))
    }

    fn on_press(&mut self, p: P, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.floater {
            f.press(p);
            cx.notify();
        }
    }

    fn on_move(&mut self, p: P, cx: &mut Context<Self>) {
        let stage = self.stage(cx);
        let Some(f) = &mut self.floater else { return };
        if !f.pressed() {
            return;
        }
        if let Some(mode) = f.drag_to(&stage, p) {
            let docked = mode == Mode::Docked;
            self.docked_applied = docked;
            cx.defer(move |cx| remember_docked(docked, cx));
        }
        cx.notify();
    }

    fn on_release(&mut self, cx: &mut Context<Self>) {
        let stage = self.stage(cx);
        let Some(f) = &mut self.floater else { return };
        if f.release(&stage) == Release::Click && !f.expanded() {
            cx.defer(|cx| set_expanded(true, cx));
        }
        cx.notify();
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

/// The floater's stop button inside its rectangle `b`.
fn blob_stop_rect(b: R, stage: &Stage) -> R {
    let (_, stop_x) = blob_columns(b, stage);
    let s = blob_stop_size(b, stage);
    R::centered(P::new(stop_x, b.center().y), s, s)
}

fn blob_stop_size(b: R, stage: &Stage) -> f32 {
    stage.d(26.).min(b.h * 0.64)
}

/// Width of the floater's waveform: its bars and the gaps between them.
fn blob_wave_width(stage: &Stage) -> f32 {
    stage.d(3. * BLOB_BARS as f32 + 2.5 * (BLOB_BARS - 1) as f32)
}

/// The centers (x) of the waveform and of the stop button in the
/// floater `b`: the same gap before, between and after them.
fn blob_columns(b: R, stage: &Stage) -> (f32, f32) {
    let wave = blob_wave_width(stage);
    let stop = blob_stop_size(b, stage);
    let gap = ((b.w - wave - stop) / 3.).max(0.);
    (b.x + gap + wave / 2., b.x + 2. * gap + wave + stop / 2.)
}

/// The wings' stop button: in the middle of the right wing.
fn wings_stop_rect(w: R, stage: &Stage) -> R {
    let housing = stage.housing();
    let s = stage.d(20.).min(w.h - 6.);
    R::centered(P::new((housing.right() + w.right()) / 2.0, w.h / 2.0), s, s)
}

/// The liquid bridge from the wings to the blob as a filled path: flared
/// where it leaves the wings, thinning toward the middle, swelling into
/// the blob.
fn neck_path(n: Neck) -> Option<Path<Pixels>> {
    let d = n.b - n.a;
    let len = d.len();
    if len < 2.0 || n.half_a < 0.5 {
        return None;
    }
    let u = d.unit();
    // Left of the direction of the pull, as seen on screen.
    let mut side = u.perp();
    if side.x > 0.0 {
        side = side * -1.0;
    }
    let pt = |p: P| point(px(p.x), px(p.y));
    let m = n.a + d * 0.5;
    // Where it leaves the wings: along their bottom edge, a little inside.
    let a_l = P::new(n.a.x - n.half_a, n.a.y);
    let a_r = P::new(n.a.x + n.half_a, n.a.y);
    let (m_l, m_r) = (m + side * n.waist, m - side * n.waist);
    let (b_l, b_r) = (n.b + side * n.half_b, n.b - side * n.half_b);
    let down = P::new(0.0, len * 0.3);
    let mut path = PathBuilder::fill();
    path.move_to(pt(a_l - P::new(0.0, 3.0)));
    path.line_to(pt(a_l));
    path.cubic_bezier_to(pt(m_l), pt(a_l + down), pt(m_l - u * (len * 0.22)));
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
    path.cubic_bezier_to(pt(a_r), pt(m_r - u * (len * 0.22)), pt(a_r + down));
    path.line_to(pt(a_r - P::new(0.0, 3.0)));
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

/// Pause (or resume) over a waveform at `r`: shown while the pointer is
/// over `group` (the waveform's area), always while paused.
fn pause_button(id: &'static str, size: f32, paused: bool, group: &'static str) -> Stateful<Div> {
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
        .cursor_pointer()
        .bg(hsla(0., 0., 1., 0.18))
        .hover(|s| s.bg(hsla(0., 0., 1., 0.3)))
        .text_color(hsla(0., 0., 1., 0.95))
        .child(icon(name).xsmall())
        .when(!paused, |d| {
            d.opacity(0.).group_hover(group, |s| s.opacity(1.))
        })
        .on_click(move |_, _, cx| set_paused(!paused, cx))
}

/// A waveform at `r` that gives way to a centered [`pause_button`]
/// (`pause`: its id and size) while hovered. The button is inside the
/// waveform's area: GPUI applies group hover to descendants only.
fn hover_wave(
    group: &'static str,
    r: R,
    m: &WidgetModel,
    bars: usize,
    max: f32,
    fg: Hsla,
    pause: Option<(&'static str, f32)>,
) -> Stateful<Div> {
    let paused = m.paused();
    place(r)
        .id(group)
        .group(group)
        .flex()
        .items_center()
        .justify_center()
        .child(wave(m, bars, max, fg).when(!paused, |d| d.group_hover(group, |s| s.opacity(0.15))))
        .when_some(pause, |d, (id, size)| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pause_button(id, size, paused, group).test_support()),
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
    /// The free floater: a black rounded rectangle, the waveform and the stop
    /// button evenly spaced in it.
    fn render_blob(
        &self,
        m: &WidgetModel,
        f: &Floater,
        stage: &Stage,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let b = f.blob(stage)?;
        let content = f.blob_content();
        let fg = hsla(0., 0., 1., 0.92);
        let wave_c = P::new(blob_columns(b, stage).0, b.center().y);
        let stop_rect = blob_stop_rect(b, stage);
        let wave_rect = R::centered(
            wave_c,
            blob_wave_width(stage) + stage.d(8.),
            b.h - stage.d(8.),
        );
        let pause_rect = R::centered(wave_c, stage.d(24.), stage.d(24.));
        let press_ok =
            move |p: P| !stop_rect.inflate(2.).contains(p) && !pause_rect.inflate(2.).contains(p);
        Some(
            place(b)
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
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let p = P::new(ev.position.x.into(), ev.position.y.into());
                        if press_ok(p) {
                            this.on_press(p, cx);
                        }
                    }),
                )
                .test_support()
                .into_any_element(),
        )
        .map(|blob| {
            if content <= 0.0 {
                return blob;
            }
            div()
                .absolute()
                .inset_0()
                .child(blob)
                .child(
                    hover_wave(
                        "blob-wave",
                        wave_rect,
                        m,
                        BLOB_BARS,
                        12.,
                        fg,
                        (content >= 1.0).then_some(("recording-widget-pause-blob", pause_rect.w)),
                    )
                    .opacity(content)
                    .test_support(),
                )
                .child(
                    stop_button("recording-widget-stop-blob", stop_rect)
                        .opacity(content)
                        .test_support(),
                )
                .into_any_element()
        })
    }

    /// The wings around the notch (or the panel growing out of them).
    fn render_wings(
        &mut self,
        m: &WidgetModel,
        f: &Floater,
        stage: &Stage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let w = f.wings(stage)?;
        let content = f.wings_content();
        let panel = f.docked_panel_content();
        let expand = f.expand.value.clamp(0.0, 1.0);
        let radius = stage.d(12.) + (stage.d(22.) - stage.d(12.)) * expand;
        let housing = stage.housing();
        let stop_rect = wings_stop_rect(w, stage);
        // The left wing (the waveform), and pause in its middle.
        let left = R {
            x: w.x,
            y: 0.,
            w: (housing.x - w.x).max(0.),
            h: w.h,
        };
        let pause_size = stage.d(20.).min(w.h - 6.);
        let pause_rect = R::centered(left.center(), pause_size, pause_size);
        let pressable = f.mode == Mode::Docked && !f.expanded();
        let press_ok =
            move |p: P| !stop_rect.inflate(2.).contains(p) && !pause_rect.inflate(2.).contains(p);
        let fg = hsla(0., 0., 1., 0.92);
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
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let p = P::new(ev.position.x.into(), ev.position.y.into());
                        if press_ok(p) {
                            this.on_press(p, cx);
                        }
                    }),
                )
            })
            .when(panel > 0.0, |d| {
                let tone = design::tone_on_black();
                // The band beside the notch: status on the left of the
                // camera, collapse on the right.
                let band = h_flex()
                    .w_full()
                    .h(px(stage.top_inset))
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
            // Waveform in the middle of the left wing (pause over it on
            // hover), stop on the right.
            layer = layer
                .child(
                    hover_wave(
                        "wings-wave",
                        left,
                        m,
                        BARS,
                        WAVE_MAX,
                        fg,
                        (content >= 1.0).then_some(("recording-widget-pause-wings", pause_rect.w)),
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
        Some(layer.into_any_element())
    }

    fn render_neck(&self, f: &Floater, stage: &Stage) -> Option<AnyElement> {
        let n = f.neck(stage)?;
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

    /// The panel opened from the free floater.
    fn render_free_panel(
        &mut self,
        m: &WidgetModel,
        f: &Floater,
        stage: &Stage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let r = f.free_panel(stage)?;
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
        let (m, docked, stage, animate) = {
            let s = state.read(cx);
            (s.model.clone(), s.docked, s.stage(), s.animate)
        };
        if !m.visible() {
            self.floater = None;
            self.meeting = None;
            return div().size_full().into_any_element();
        }
        // A new recording starts where the setting says.
        if self.floater.is_none() || self.meeting != m.meeting_id() {
            self.floater = Some(Floater::new(&stage, docked));
            self.meeting = m.meeting_id();
            self.docked_applied = docked;
            self.last_frame = None;
        }
        let mut f = self.floater.take().expect("made above");
        f.animate = animate && !cx.reduce_motion();
        if docked != self.docked_applied {
            f.set_docked(&stage, docked);
            self.docked_applied = docked;
        }
        f.set_expanded(m.expanded);
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(1.0 / 60.0);
        f.step(dt);
        let animating = f.animating();
        self.last_frame = animating.then_some(now);
        if animating {
            window.request_animation_frame();
        }

        let neck = self.render_neck(&f, &stage);
        let wings = self.render_wings(&m, &f, &stage, window, cx);
        let blob = self.render_blob(&m, &f, &stage, cx);
        let panel = self.render_free_panel(&m, &f, &stage, window, cx);
        self.floater = Some(f);

        div()
            .id("recording-widget")
            .relative()
            .size_full()
            .font_family(crate::theme::UI_FONT)
            .text_color(design::tone(cx).text)
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.on_move(P::new(ev.position.x.into(), ev.position.y.into()), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.on_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.on_release(cx)),
            )
            .children(neck)
            .children(wings)
            .children(blob)
            .children(panel)
            .test_support()
            .into_any_element()
    }
}
