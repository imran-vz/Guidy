//! Banners above the page content: missing Accessibility permission (macOS)
//! and stuck Secure Input (macOS).

use crate::logic::flows::{secure_input_impacted, secure_input_message, SECURE_INPUT_HELP_URL};
use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::design::{self, Pill};
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AxState {
    Request,
    Verify,
    Granted,
}

/// Accessibility permission banner: shown on macOS until granted.
pub struct AccessibilityBanner {
    state: AxState,
    checked: bool,
    _activation: Option<Subscription>,
}

impl AccessibilityBanner {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            state: AxState::Granted,
            checked: false,
            _activation: None,
        };
        if OsType::current() == OsType::MacOs {
            // The window is created before permission onboarding, and the
            // user grants access in System Settings: re-check whenever the
            // window comes back to the front.
            this._activation = Some(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh(cx);
                }
            }));
            this.recheck(true, cx);
        }
        this
    }

    /// Re-check after the permission may have changed elsewhere (onboarding,
    /// System Settings), keeping the current prompt if still missing.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if OsType::current() == OsType::MacOs && self.state != AxState::Granted {
            self.recheck(self.state == AxState::Request, cx);
        }
    }

    fn recheck(&mut self, initial: bool, cx: &mut Context<Self>) {
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let granted = cx
                .background_spawn(async move { backend.check_accessibility_permission() })
                .await;
            this.update(cx, |this, cx| {
                this.checked = true;
                this.state = if granted {
                    AxState::Granted
                } else if initial {
                    AxState::Request
                } else {
                    AxState::Verify
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn on_click(&mut self, cx: &mut Context<Self>) {
        match self.state {
            AxState::Request => {
                let backend = AppState::global(cx).read(cx).backend.clone();
                cx.background_spawn(async move {
                    backend.request_accessibility_permission();
                })
                .detach();
                self.state = AxState::Verify;
                cx.notify();
            }
            AxState::Verify => self.recheck(false, cx),
            AxState::Granted => {}
        }
    }
}

impl Render for AccessibilityBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.checked || self.state == AxState::Granted {
            return div().into_any_element();
        }
        let theme = cx.theme();
        h_flex()
            .id("accessibility-banner")
            .w_full()
            .max_w(px(768.))
            .mb_4()
            .gap_3()
            .pl(px(14.))
            .pr(px(10.))
            .py(px(10.))
            .rounded(design::RADIUS_PANEL)
            .border_1()
            .border_color(theme.warning.opacity(0.3))
            .bg(theme.warning.opacity(0.08))
            .child(
                icon("triangle-alert")
                    .flex_shrink_0()
                    .text_color(theme.warning),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(design::TEXT_BODY)
                    .child(t("accessibility.permissionsDescription")),
            )
            .child(
                Button::new("accessibility-open-settings")
                    .flex_shrink_0()
                    .primary()
                    .small()
                    .pill()
                    .label(t("accessibility.openSettings"))
                    .on_click(cx.listener(|this, _, _, cx| this.on_click(cx))),
            )
            .test_support()
            .into_any_element()
    }
}

/// Warning shown while macOS Secure Input is active.
pub struct SecureInputBanner {
    dismissed: bool,
    _sub: Subscription,
}

impl SecureInputBanner {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let sub = cx.observe(&state, |this, state, cx| {
            // A dismissal lasts for the current episode only.
            let impacted = state
                .read(cx)
                .secure_input
                .as_ref()
                .is_some_and(secure_input_impacted);
            if !impacted {
                this.dismissed = false;
            }
            cx.notify();
        });
        Self {
            dismissed: false,
            _sub: sub,
        }
    }
}

impl Render for SecureInputBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = AppState::global(cx).read(cx).secure_input.clone();
        let Some(status) = status.filter(secure_input_impacted) else {
            return div().into_any_element();
        };
        if self.dismissed {
            return div().into_any_element();
        }
        let theme = cx.theme();
        h_flex()
            .id("secure-input-banner")
            .w_full()
            .max_w(px(768.))
            .mb_4()
            .gap_3()
            .pl(px(14.))
            .pr(px(10.))
            .py(px(10.))
            .rounded(design::RADIUS_PANEL)
            .border_1()
            .border_color(theme.warning.opacity(0.3))
            .bg(theme.warning.opacity(0.08))
            .child(
                icon("triangle-alert")
                    .flex_shrink_0()
                    .text_color(theme.warning),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(design::TEXT_BODY)
                    .child(secure_input_message(&status)),
            )
            .child(
                Button::new("secure-input-learn-more")
                    .flex_shrink_0()
                    .ghost()
                    .small()
                    .pill()
                    .label(t("secureInput.learnMore"))
                    .icon(icon("external-link"))
                    .on_click(|_, _, cx| cx.open_url(SECURE_INPUT_HELP_URL)),
            )
            .child(
                Button::new("secure-input-dismiss")
                    .flex_shrink_0()
                    .ghost()
                    .xsmall()
                    .icon(icon("x"))
                    .tooltip(t("secureInput.dismiss"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismissed = true;
                        cx.notify();
                    })),
            )
            .test_support()
            .into_any_element()
    }
}
