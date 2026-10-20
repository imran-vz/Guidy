//! Meeting controls wrapped around the notch while a meeting records: a live
//! waveform on the left (hovering shows pause/resume over it), the elapsed
//! time and stop on the right. Black, flush with the top edge, so the wings
//! read as part of the camera housing.

use crate::logic::notch::{bar_height, NotchModel};
use crate::state::AppState;
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::recorder::now_ms;
use guidy_core::meetings::types::RecorderState;
use std::time::Duration;

/// Level poll / waveform scroll interval.
const TICK: Duration = Duration::from_millis(90);
const WAVE_MAX: f32 = 16.;
const RADIUS: f32 = 12.;

pub struct NotchState {
    pub model: NotchModel,
    /// Width of the middle part the camera housing covers.
    pub notch_width: f32,
    ticking: bool,
}

pub struct GlobalNotchState(pub Entity<NotchState>);
impl Global for GlobalNotchState {}

impl NotchState {
    pub fn install(cx: &mut App) -> Entity<NotchState> {
        let e = cx.new(|_| NotchState {
            model: NotchModel::default(),
            notch_width: 0.,
            ticking: false,
        });
        cx.set_global(GlobalNotchState(e.clone()));
        e
    }

    pub fn global(cx: &App) -> Option<Entity<NotchState>> {
        cx.try_global::<GlobalNotchState>().map(|g| g.0.clone())
    }

    pub fn set_recorder(&mut self, state: RecorderState, cx: &mut Context<Self>) {
        self.model.set_recorder(state);
        if self.model.visible() && !self.ticking {
            self.start_ticking(cx);
        }
        cx.notify();
    }

    /// Poll the input level (and redraw the timer) while visible.
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
                    let level = AppState::global(cx)
                        .read(cx)
                        .meetings
                        .take_recording_level();
                    s.model.push_level(level);
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

fn set_paused(paused: bool, cx: &mut App) {
    let backend = AppState::global(cx).read(cx).meetings.clone();
    cx.background_spawn(async move { backend.set_recording_paused(paused) })
        .detach();
}

fn stop(cx: &mut App) {
    let backend = AppState::global(cx).read(cx).meetings.clone();
    cx.background_spawn(async move {
        if let Err(e) = backend.stop_recording() {
            log::error!("Stopping the meeting from the notch failed: {e}");
        }
    })
    .detach();
}

pub struct NotchView {
    state: Option<Entity<NotchState>>,
    _sub: Option<Subscription>,
}

impl NotchView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = NotchState::global(cx);
        let sub = state
            .as_ref()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()));
        Self { state, _sub: sub }
    }
}

fn round_button(id: &'static str, label: SharedString) -> Stateful<Div> {
    div()
        .id(id)
        .aria_label(label)
        .size(px(22.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
}

impl Render for NotchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.clone() else {
            return div().into_any_element();
        };
        let s = state.read(cx);
        if !s.model.visible() {
            return div().size_full().into_any_element();
        }
        let paused = s.model.paused();
        let notch_width = s.notch_width;
        let fg = hsla(0., 0., 1., 0.92);
        let dim = hsla(0., 0., 1., 0.5);
        let red = rgb(0xff453a);

        let wave = h_flex()
            .id("notch-wave")
            .gap(px(2.5))
            .h(px(WAVE_MAX))
            .items_center()
            .when(paused, |d| d.opacity(0.35))
            .when(!paused, |d| d.group_hover("notch", |s| s.opacity(0.2)))
            .children(s.model.levels().map(move |v| {
                div()
                    .w(px(3.))
                    .h(px(bar_height(v, WAVE_MAX)))
                    .rounded(px(1.5))
                    .bg(fg)
            }));

        let (pause_icon, pause_label) = if paused {
            ("play", t("notch.resume"))
        } else {
            ("pause", t("notch.pause"))
        };
        let pause = round_button("notch-pause", pause_label)
            .bg(hsla(0., 0., 1., 0.16))
            .text_color(fg)
            .child(icon(pause_icon).xsmall())
            // Always shown while paused; on hover otherwise.
            .when(!paused, |d| {
                d.opacity(0.).group_hover("notch", |s| s.opacity(1.))
            })
            .on_click(move |_, _, cx| set_paused(!paused, cx))
            .test_support();

        let left = div()
            .relative()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(wave)
            .child(
                // Centred over the waveform.
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pause),
            );

        let right = h_flex()
            .flex_1()
            .h_full()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(
                div()
                    .id("notch-timer")
                    .text_size(px(12.))
                    .font_family(crate::theme::MONO_FONT)
                    .text_color(if paused { dim } else { fg })
                    .child(s.model.timer(now_ms()))
                    .test_support(),
            )
            .child(
                round_button("notch-stop", t("notch.stop"))
                    .bg(hsla(0., 0., 1., 0.12))
                    .hover(|s| s.bg(hsla(0., 0., 1., 0.22)))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(red))
                    .on_click(|_, _, cx| stop(cx))
                    .test_support(),
            );

        div()
            .id("notch")
            .group("notch")
            .size_full()
            .flex()
            .items_center()
            .bg(black())
            .rounded_bl(px(RADIUS))
            .rounded_br(px(RADIUS))
            .child(left)
            .child(div().w(px(notch_width)).flex_none())
            .child(right)
            .with_animation(
                "notch-in",
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    }
}
