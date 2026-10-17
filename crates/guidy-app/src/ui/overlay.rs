//! The recording overlay (Handy's `RecordingOverlay`), drawn as a floating
//! Zed-style panel: compact pill for recording/working, and the Live card
//! that opens into a transcript panel while streaming.

use crate::logic::overlay::{bar_height, format_timer, OverlayKind, OverlayModel};
use crate::state::now_ms;
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::settings::OverlayStyle;
use std::time::Duration;

pub struct OverlayState {
    pub model: OverlayModel,
    pub style: OverlayStyle,
    ticking: bool,
}

pub struct GlobalOverlayState(pub Entity<OverlayState>);
impl Global for GlobalOverlayState {}

impl OverlayState {
    pub fn install(cx: &mut App) -> Entity<OverlayState> {
        let e = cx.new(|_| OverlayState {
            model: OverlayModel::default(),
            style: OverlayStyle::Live,
            ticking: false,
        });
        cx.set_global(GlobalOverlayState(e.clone()));
        e
    }

    pub fn global(cx: &App) -> Option<Entity<OverlayState>> {
        cx.try_global::<GlobalOverlayState>().map(|g| g.0.clone())
    }

    pub fn handle_core_event(
        &mut self,
        name: &str,
        payload: &serde_json::Value,
        overlay_top: bool,
        style: OverlayStyle,
        cx: &mut Context<Self>,
    ) {
        self.style = style;
        self.model.apply_event(name, payload, now_ms(), overlay_top);
        if self.model.visible && !self.ticking {
            self.start_ticking(cx);
        }
        cx.notify();
    }

    /// Re-render ~4×/s while visible for the timer and the slow-load latch.
    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let keep = this
                .update(cx, |s, cx| {
                    s.model.tick(now_ms());
                    cx.notify();
                    if !s.model.visible {
                        s.ticking = false;
                    }
                    s.model.visible
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }
}

pub struct OverlayView {
    state: Option<Entity<OverlayState>>,
    scroll: ScrollHandle,
    _sub: Option<Subscription>,
}

impl OverlayView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = OverlayState::global(cx);
        let sub = state
            .as_ref()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()));
        Self {
            state,
            scroll: ScrollHandle::new(),
            _sub: sub,
        }
    }
}

impl OverlayView {
    fn sync_pinned(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let scrolled = f32::from(-self.scroll.offset().y);
        let max = f32::from(self.scroll.max_offset().y);
        let pinned = crate::logic::overlay::is_pinned(max, scrolled, 0.0);
        state.update(cx, |s, cx| {
            if s.model.pinned != pinned {
                s.model.pinned = pinned;
                cx.notify();
            }
        });
    }
}

fn cancel_button(cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let hover = cx.theme().accent;
    div()
        .id("overlay-cancel")
        .aria_label("cancel")
        .size(px(22.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.bg(hover))
        .child(icon("x").xsmall())
        .on_click(|_, _, cx| {
            if let Some(g) = cx.try_global::<crate::state::GlobalAppState>() {
                let backend = g.0.read(cx).backend.clone();
                cx.background_spawn(async move { backend.cancel_operation() })
                    .detach();
            }
        })
        .test_support()
}

fn status_dot(ready: bool, cx: &App) -> AnyElement {
    let accent = cx.theme().primary;
    let muted = cx.theme().muted_foreground;
    if ready {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(accent)
            .with_animation(
                "dot-pulse",
                Animation::new(Duration::from_millis(1900)).repeat(),
                |el, delta| el.opacity(1.0 - 0.45 * (delta * std::f32::consts::PI).sin()),
            )
            .into_any_element()
    } else {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(muted)
            .opacity(0.45)
            .into_any_element()
    }
}

fn waveform(levels: [f32; 9], ready: bool, cx: &App) -> impl IntoElement {
    let fg = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    h_flex()
        .id("overlay-wave")
        .gap(px(3.))
        .h(px(18.))
        .items_center()
        .children(levels.into_iter().enumerate().map(move |(i, v)| {
            let bar = div().w(px(4.)).rounded(px(2.));
            if ready {
                bar.h(px(bar_height(v))).bg(fg).into_any_element()
            } else {
                bar.h(px(6.))
                    .bg(muted)
                    .with_animation(
                        ("arming", i),
                        Animation::new(Duration::from_millis(900)).repeat(),
                        move |el, delta| {
                            let phase = (delta + i as f32 * 0.08) % 1.0;
                            el.opacity(0.35 + 0.4 * (phase * std::f32::consts::PI).sin())
                        },
                    )
                    .into_any_element()
            }
        }))
}

fn spinner(cx: &App) -> impl IntoElement {
    icon("loader-circle")
        .with_size(px(13.))
        .text_color(cx.theme().muted_foreground)
        .with_animation(
            "overlay-spin",
            Animation::new(Duration::from_millis(700)).repeat(),
            |el, delta| el.transform(Transformation::rotate(percentage(delta))),
        )
}

impl Render for OverlayView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.clone() else {
            return div().into_any_element();
        };
        let s = state.read(cx);
        let m = s.model.clone();
        let style = s.style;
        let now = now_ms();
        let theme = cx.theme();
        let (bg, border, fg, muted) = (
            theme.popover,
            theme.border,
            theme.foreground,
            theme.muted_foreground,
        );

        if !m.visible {
            return div().size_full().into_any_element();
        }

        let card = div()
            .bg(bg.opacity(0.98))
            .border_1()
            .border_color(border)
            .text_color(fg)
            .shadow_md()
            .overflow_hidden();

        let listening_row = |show_timer: bool, cx: &App| {
            h_flex()
                .h(px(40.))
                .px_3()
                .gap_2()
                .justify_between()
                .child(
                    div()
                        .w(px(40.))
                        .flex()
                        .child(status_dot(m.capture_ready, cx)),
                )
                .child(waveform(m.levels(), m.capture_ready, cx))
                .child(
                    h_flex()
                        .w(px(40.))
                        .justify_end()
                        .gap_1()
                        .when(show_timer, |d| {
                            d.child(
                                div()
                                    .id("overlay-timer")
                                    .text_xs()
                                    .text_color(muted)
                                    .font_family(crate::theme::MONO_FONT)
                                    .child(format_timer(m.elapsed_secs(now)))
                                    .test_support(),
                            )
                        })
                        .child(cancel_button(cx)),
                )
        };
        let working_row = |label: SharedString, cx: &App| {
            h_flex()
                .h(px(40.))
                .px_3()
                .gap_2()
                .justify_between()
                .child(div().w(px(22.)).child(spinner(cx)))
                .child(
                    div()
                        .id("overlay-work-label")
                        .text_sm()
                        .text_color(muted)
                        .child(label)
                        .test_support(),
                )
                .child(cancel_button(cx))
        };

        let body = if m.kind == OverlayKind::Streaming && style == OverlayStyle::Live {
            let l = m.live_layout(now);
            let width = if l.open {
                392.
            } else if l.collapsed {
                216.
            } else {
                184.
            };
            let radius = if l.open { 16. } else { 18. };
            let text_id = "overlay-text";
            let text = v_flex()
                .id(text_id)
                .max_h(px(64.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                    // Measure after the wheel has moved the content: scrolling
                    // up unpins, coming back near the bottom re-pins.
                    cx.defer_in(window, |this, _, cx| this.sync_pinned(cx));
                }))
                .px_3()
                .pt_2()
                .text_size(px(15.))
                .italic()
                .child(
                    div()
                        .when(l.loading_notice, |d| {
                            d.child(div().text_color(muted).child(t("overlay.loadingModel")))
                        })
                        .child(
                            // One flex item per word so the transcript wraps
                            // inside the card and the caret follows the last
                            // word; `max_w_full` lets an unspaced run (CJK)
                            // wrap within its own item.
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_x(px(4.))
                                .children(m.committed.split_whitespace().map(|w| {
                                    div().max_w_full().child(SharedString::from(w.to_string()))
                                }))
                                .children(m.tentative.split_whitespace().map(|w| {
                                    div()
                                        .max_w_full()
                                        .text_color(muted)
                                        .child(SharedString::from(w.to_string()))
                                }))
                                .when(l.show_caret, |d| {
                                    d.child(
                                        div().w(px(1.5)).h(px(16.)).bg(fg).with_animation(
                                            "caret",
                                            Animation::new(Duration::from_millis(1050)).repeat(),
                                            |el, delta| {
                                                el.opacity(if delta < 0.5 { 1.0 } else { 0.0 })
                                            },
                                        ),
                                    )
                                }),
                        ),
                );
            if m.pinned {
                self.scroll.scroll_to_bottom();
            }
            let row = if l.working {
                working_row(t(m.working_label_key()), cx).into_any_element()
            } else {
                listening_row(l.open, cx).into_any_element()
            };
            let text = text.test_support();
            let mut inner = v_flex().w(px(width)).rounded(px(radius));
            inner = if m.top {
                // Top placement: pill on top, text flows downward.
                inner.child(row).when(l.open, |d| d.child(text))
            } else {
                inner.when(l.open, |d| d.child(text)).child(row)
            };
            card.rounded(px(radius))
                .child(inner)
                .with_animation(
                    ("pop-in", m.session as usize),
                    Animation::new(Duration::from_millis(460)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
        } else {
            let working = m.compact_working();
            let width = if working { 216. } else { 172. };
            let row = if working {
                working_row(t(m.working_label_key()), cx).into_any_element()
            } else {
                listening_row(false, cx).into_any_element()
            };
            card.w(px(width))
                .rounded_full()
                .child(row)
                .with_animation(
                    "fade",
                    Animation::new(Duration::from_millis(200)),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
        };

        div()
            .id("overlay-stage")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .when(m.top, |d| d.justify_start())
            .when(!m.top, |d| d.justify_end())
            .child(body)
            .test_support()
            .into_any_element()
    }
}
