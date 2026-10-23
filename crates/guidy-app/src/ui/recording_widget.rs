//! The floating recording widget (see [`crate::logic::recording_widget`]):
//! a pill at the right edge of the screen while a meeting records, which
//! expands into a panel with what is being recorded, the live transcript
//! and the controls. Its window is a non-activating panel, so clicking it
//! never takes focus from the meeting app.

use crate::logic::notch::bar_height;
use crate::logic::recording_widget::{rows, Row, WidgetModel, BARS};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::recorder::now_ms;
use guidy_core::meetings::types::{LiveStatus, LiveUpdate, RecorderState, Track};
use std::time::Duration;

/// Level poll / waveform scroll interval.
const TICK: Duration = Duration::from_millis(90);
const WAVE_MAX: f32 = 16.;
const RED: u32 = 0xff453a;

pub struct RecordingWidgetState {
    pub model: WidgetModel,
    ticking: bool,
}

pub struct GlobalRecordingWidget(pub Entity<RecordingWidgetState>);
impl Global for GlobalRecordingWidget {}

impl RecordingWidgetState {
    pub fn install(cx: &mut App) -> Entity<RecordingWidgetState> {
        let e = cx.new(|_| RecordingWidgetState {
            model: WidgetModel::default(),
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

/// Expand the pill into the panel (or collapse it back), resizing its window.
pub fn set_expanded(expanded: bool, cx: &mut App) {
    if let Some(state) = RecordingWidgetState::global(cx) {
        state.update(cx, |s, cx| {
            s.model.expanded = expanded;
            cx.notify();
        });
    }
    // Deferred: a click in the widget calls this from inside its window.
    cx.defer(crate::windows::place_recording_widget);
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

pub struct RecordingWidgetView {
    state: Option<Entity<RecordingWidgetState>>,
    scroll: ScrollHandle,
    /// Follow new lines (the user hasn't scrolled up).
    pinned: bool,
    _sub: Option<Subscription>,
}

impl RecordingWidgetView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = RecordingWidgetState::global(cx);
        let sub = state
            .as_ref()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()));
        Self {
            state,
            scroll: ScrollHandle::new(),
            pinned: true,
            _sub: sub,
        }
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
}

/// The recording dot: red and breathing while recording, amber and still
/// while paused.
fn live_dot(paused: bool, cx: &App) -> AnyElement {
    let dot = div().size(dp(8.)).rounded_full();
    if paused {
        return dot.bg(design::tone(cx).accent).into_any_element();
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

fn wave(model: &WidgetModel, fg: Hsla) -> impl IntoElement {
    h_flex()
        .gap(dp(2.5))
        .h(dp(WAVE_MAX))
        .items_center()
        .when(model.paused(), |d| d.opacity(0.35))
        .children(model.wave().take(BARS).map(move |v| {
            div()
                .w(dp(3.))
                .h(dp(bar_height(v, WAVE_MAX)))
                .rounded(dp(1.5))
                .bg(fg)
        }))
}

/// A round icon button.
fn icon_button(id: &'static str, name: &str, label: SharedString, cx: &App) -> Stateful<Div> {
    let tone = design::tone(cx);
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
fn pill(id: &'static str, name: &str, label: SharedString, cx: &App) -> Stateful<Div> {
    let tone = design::tone(cx);
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
fn source_chip(model: &WidgetModel, track: Track, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
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

fn transcript_row(i: usize, row: &Row, window: &Window, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
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
        .child(design::eyebrow(label, cx).when(you, |d| d.text_color(tone.accent_text)))
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
fn transcript_notice(status: &LiveStatus, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
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
    fn render_pill(&self, m: &WidgetModel, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        h_flex()
            .id("recording-widget-pill")
            .aria_label(t("meetings.widget.expand"))
            .group("pill")
            .relative()
            .size_full()
            .px(dp(14.))
            .gap(dp(10.))
            .items_center()
            .rounded(design::RADIUS_PILL)
            .border_1()
            .border_color(tone.hairline)
            .bg(tone.surface.opacity(0.97))
            .overflow_hidden()
            .cursor_pointer()
            .child(design::rim_light(cx))
            .child(live_dot(m.paused(), cx))
            .child(wave(m, tone.text))
            .child(div().flex_1())
            .child(
                div()
                    .id("recording-widget-timer")
                    .font_family(crate::theme::MONO_FONT)
                    .text_size(design::TEXT_MONO)
                    .text_color(if m.paused() {
                        tone.text_muted
                    } else {
                        tone.text
                    })
                    .child(m.timer(now_ms()))
                    .test_support(),
            )
            .child(
                div()
                    .text_color(tone.text_faint)
                    .group_hover("pill", |s| s.text_color(tone.text))
                    .child(icon("chevron-left").xsmall()),
            )
            .on_click(|_, _, cx| set_expanded(true, cx))
            .test_support()
            .into_any_element()
    }

    fn render_panel(
        &mut self,
        m: &WidgetModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let paused = m.paused();
        let meeting_id = m.meeting_id();
        let status = m.live_status();
        let rows = m.live().map(|l| rows(&l.lines)).unwrap_or_default();

        let header = h_flex()
            .w_full()
            .px(dp(16.))
            .pt(dp(12.))
            .gap(dp(8.))
            .items_center()
            .child(live_dot(paused, cx))
            .child(design::eyebrow(
                if paused {
                    t("meetings.widget.paused")
                } else {
                    t("meetings.widget.recording")
                },
                cx,
            ))
            .child(
                div()
                    .font_family(crate::theme::MONO_FONT)
                    .text_size(design::TEXT_MONO)
                    .text_color(tone.text_muted)
                    .child(m.timer(now_ms())),
            )
            .child(div().flex_1())
            .child(
                icon_button(
                    "recording-widget-collapse",
                    "minimize-2",
                    t("meetings.widget.collapse"),
                    cx,
                )
                .on_click(|_, _, cx| set_expanded(false, cx))
                .test_support(),
            );

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
            .child(source_chip(m, Track::Mic, cx))
            .child(source_chip(m, Track::System, cx));

        let body: Vec<AnyElement> = if rows.is_empty() {
            vec![transcript_notice(&status, cx)]
        } else {
            rows.iter()
                .enumerate()
                .map(|(i, r)| transcript_row(i, r, window, cx))
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
                pill("recording-widget-pause", pause_icon, pause_label, cx)
                    .on_click(move |_, _, cx| set_paused(!paused, cx))
                    .test_support(),
            )
            .child(
                pill(
                    "recording-widget-stop",
                    "square",
                    t("meetings.widget.stop"),
                    cx,
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
                        cx,
                    )
                    .bg(gpui_kit::transparent_black())
                    .text_color(tone.text_muted)
                    .on_click(move |_, _, cx| open_notes(id, cx))
                    .test_support(),
                )
            });

        v_flex()
            .id("recording-widget-panel")
            .relative()
            .size_full()
            .rounded(design::RADIUS_WELL)
            .border_1()
            .border_color(tone.hairline)
            .bg(tone.surface.opacity(0.98))
            .overflow_hidden()
            .child(design::rim_light(cx))
            .child(header)
            .child(title)
            .child(sources)
            .child(transcript)
            .child(footer)
            .test_support()
            .with_animation(
                "recording-widget-open",
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    }
}

impl Render for RecordingWidgetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.clone() else {
            return div().into_any_element();
        };
        let m = state.read(cx).model.clone();
        if !m.visible() {
            return div().size_full().into_any_element();
        }
        let content = if m.expanded {
            self.render_panel(&m, window, cx)
        } else {
            self.render_pill(&m, cx)
        };
        div()
            .id("recording-widget")
            .size_full()
            .font_family(crate::theme::UI_FONT)
            .text_color(design::tone(cx).text)
            .child(content)
            .into_any_element()
    }
}
