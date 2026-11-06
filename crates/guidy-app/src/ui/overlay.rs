//! The recording overlay, drawn as a floating
//! glass panel: compact pill for recording/working, and the Live card
//! that opens into a transcript panel while streaming.

use crate::logic::overlay::{bar_height, format_timer, OverlayKind, OverlayModel};
use crate::state::now_ms;
use crate::ui::design;
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
    // Pressed: a step past the hover fill, same footprint.
    let pressed = hover.blend(cx.theme().foreground.opacity(0.12));
    div()
        .id("overlay-cancel")
        .aria_label(t("common.cancel"))
        // The shared 24 pt icon-button target.
        .size(design::ICON_BUTTON)
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
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

/// The live dot: the recording red with a soft halo once the microphone is
/// capturing (steady: the level meter already moves), a dim grey dot while
/// it arms.
fn status_dot(ready: bool, cx: &App) -> AnyElement {
    if ready {
        let red = design::status_color(design::Status::Live, cx);
        div()
            .id("overlay-live-dot")
            .flex_none()
            .size(design::hud(12.))
            .rounded_full()
            .bg(red.opacity(0.2))
            .flex()
            .items_center()
            .justify_center()
            .child(div().size(design::hud(7.)).rounded_full().bg(red))
            .test_support()
            .into_any_element()
    } else {
        div()
            .flex_none()
            .size(design::hud(7.))
            .rounded_full()
            .bg(cx.theme().muted_foreground)
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
        .children(levels.into_iter().map(move |v| {
            let bar = div().w(px(4.)).rounded(px(2.));
            if ready {
                // The live level meter.
                bar.h(px(bar_height(v))).bg(fg).into_any_element()
            } else {
                // Arming: flat, dim bars, still (the grey dot already says
                // the microphone isn't capturing yet; a ripple only
                // decorated it).
                bar.h(px(6.)).bg(muted).opacity(0.5).into_any_element()
            }
        }))
}

/// The working row's loader: static under Reduce Motion (the label beside
/// it names the work).
fn spinner(cx: &App) -> AnyElement {
    crate::ui::kit::spinner(
        "overlay-spin",
        icon("loader-circle")
            .with_size(px(13.))
            .text_color(cx.theme().muted_foreground),
    )
    .into_any_element()
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(design::HUD_REM);
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

        // Glass: near-opaque surface, hairline edge, rim light on top.
        let card = div()
            .relative()
            .bg(bg.opacity(0.96))
            .border_1()
            .border_color(border)
            .text_color(fg)
            .shadow_lg()
            .overflow_hidden()
            .child(design::rim_light(cx));

        let reduce_motion = cx.reduce_motion();
        let listening_row = |show_timer: bool, cx: &App| {
            // Equal side columns keep the meter centered; the open card has
            // room for the timer beside Cancel.
            let side = px(if show_timer { 72. } else { 40. });
            h_flex()
                .h(px(40.))
                .px_3()
                .gap_2()
                .justify_between()
                .child(
                    h_flex()
                        .w(side)
                        .items_center()
                        .child(status_dot(m.capture_ready, cx)),
                )
                .child(waveform(m.levels(), m.capture_ready, cx))
                .child(
                    h_flex()
                        .w(side)
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .when(show_timer, |d| {
                            d.child(
                                div()
                                    .id("overlay-timer")
                                    .text_size(design::HUD_TEXT_LABEL)
                                    .font_features(design::tabular_figures())
                                    .text_color(muted)
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
                .child(div().w(design::ICON_BUTTON).child(spinner(cx)))
                .child(
                    div()
                        .id("overlay-work-label")
                        .text_size(design::HUD_TEXT_BODY)
                        .text_color(muted)
                        .whitespace_nowrap()
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
                .line_height(px(21.))
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
                                    let caret = div().w(px(1.5)).h(px(16.)).bg(fg);
                                    d.child(if reduce_motion {
                                        caret.into_any_element()
                                    } else {
                                        caret
                                            .with_animation(
                                                "caret",
                                                Animation::new(Duration::from_millis(1050))
                                                    .repeat(),
                                                |el, delta| {
                                                    el.opacity(if delta < 0.5 { 1.0 } else { 0.0 })
                                                },
                                            )
                                            .into_any_element()
                                    })
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
            let card = card.rounded(px(radius)).child(inner);
            if reduce_motion {
                card.into_any_element()
            } else {
                card.with_animation(
                    ("pop-in", m.session as usize),
                    Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
            }
        } else {
            let working = m.compact_working();
            let width = if working { 216. } else { 172. };
            let row = if working {
                working_row(t(m.working_label_key()), cx).into_any_element()
            } else {
                listening_row(false, cx).into_any_element()
            };
            let card = card.w(px(width)).rounded_full().child(row);
            if reduce_motion {
                card.into_any_element()
            } else {
                card.with_animation(
                    "fade",
                    Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
            }
        };

        div()
            .id("overlay-stage")
            .size_full()
            .font_family(crate::theme::UI_FONT)
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
