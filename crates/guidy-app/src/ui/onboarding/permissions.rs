//! Onboarding, step 2: microphone and (macOS, for dictation)
//! Accessibility, with 1 s polling after the user acts. Accessibility can
//! be skipped ("Continue Without"): the text is then copied for the user to
//! paste. Meeting permissions are asked when the first meeting is recorded.

use super::assistant::{back_button, continue_button, row, row_icon, rows, Assistant};
use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use crate::ui::main_window::MainWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::{Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::commands::audio::PermissionAccess;
use std::time::Duration;

const MAX_POLLING_ERRORS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionStatus {
    Checking,
    Needed,
    Waiting,
    Granted,
}

pub struct PermissionsOnboarding {
    main: WeakEntity<MainWindow>,
    preview: bool,
    os: OsType,
    pub accessibility: PermissionStatus,
    pub microphone: PermissionStatus,
    polling: bool,
    completed: bool,
    focus: FocusHandle,
}

impl PermissionsOnboarding {
    pub fn new(
        main: WeakEntity<MainWindow>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let os = OsType::current();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            main,
            preview,
            os,
            accessibility: PermissionStatus::Checking,
            microphone: PermissionStatus::Checking,
            polling: false,
            completed: false,
            focus,
        };
        if preview {
            // Inert preview: show the request UI without touching the OS.
            this.accessibility = if os == OsType::MacOs {
                PermissionStatus::Needed
            } else {
                PermissionStatus::Granted
            };
            this.microphone = if matches!(os, OsType::MacOs | OsType::Windows) {
                PermissionStatus::Needed
            } else {
                PermissionStatus::Granted
            };
        } else if !matches!(os, OsType::MacOs | OsType::Windows) {
            // Nothing to grant on other platforms.
            this.accessibility = PermissionStatus::Granted;
            this.microphone = PermissionStatus::Granted;
            this.complete(cx);
        } else {
            this.check_initial(window, cx);
        }
        this
    }

    fn all_granted(&self) -> bool {
        match self.os {
            OsType::MacOs => {
                self.accessibility == PermissionStatus::Granted
                    && self.microphone == PermissionStatus::Granted
            }
            OsType::Windows => self.microphone == PermissionStatus::Granted,
            _ => true,
        }
    }

    fn backend(&self, cx: &App) -> crate::backend::SharedBackend {
        AppState::global(cx).read(cx).backend.clone()
    }

    fn check_initial(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend(cx);
        let os = self.os;
        cx.spawn_in(_window, async move |this, cx| {
            let (acc, mic) = cx
                .background_spawn(async move {
                    match os {
                        OsType::MacOs => {
                            let acc = backend.check_accessibility_permission();
                            let mic = backend.check_microphone_permission();
                            if acc {
                                let _ = backend.initialize_enigo();
                                let _ = backend.initialize_shortcuts();
                            }
                            (acc, mic)
                        }
                        _ => {
                            let s = backend.get_windows_microphone_permission_status();
                            (
                                true,
                                !s.supported || s.overall_access != PermissionAccess::Denied,
                            )
                        }
                    }
                })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.accessibility = if acc {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::Needed
                };
                this.microphone = if mic {
                    PermissionStatus::Granted
                } else {
                    PermissionStatus::Needed
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// On to the next step (once: a Back later builds a new view).
    fn complete(&mut self, cx: &mut Context<Self>) {
        if self.completed || self.preview {
            return;
        }
        self.completed = true;
        AppState::global(cx).update(cx, |s, cx| {
            s.refresh_audio_devices(cx);
            s.refresh_output_devices(cx);
        });
        let main = self.main.clone();
        cx.defer(move |cx| {
            main.update(cx, |m, cx| m.on_permissions_complete(cx)).ok();
        });
        cx.notify();
    }

    fn start_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        let backend = self.backend(cx);
        let os = self.os;
        cx.spawn_in(window, async move |this, cx| {
            let mut errors = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let b = backend.clone();
                let result: Result<(bool, bool), String> = cx
                    .background_spawn(async move {
                        Ok(match os {
                            OsType::Windows => {
                                let s = b.get_windows_microphone_permission_status();
                                (
                                    true,
                                    !s.supported || s.overall_access != PermissionAccess::Denied,
                                )
                            }
                            _ => (
                                b.check_accessibility_permission(),
                                b.check_microphone_permission(),
                            ),
                        })
                    })
                    .await;
                let keep = this
                    .update_in(cx, |this, window, cx| match result {
                        Ok((acc, mic)) => {
                            errors = 0;
                            if acc
                                && this.accessibility != PermissionStatus::Granted
                                && os == OsType::MacOs
                            {
                                // Accessibility just granted: start input + shortcuts.
                                let b = this.backend(cx);
                                cx.background_spawn(async move {
                                    let _ = b.initialize_enigo();
                                    let _ = b.initialize_shortcuts();
                                })
                                .detach();
                            }
                            if acc {
                                this.accessibility = PermissionStatus::Granted;
                            }
                            if mic {
                                this.microphone = PermissionStatus::Granted;
                            }
                            cx.notify();
                            if this.all_granted() {
                                this.polling = false;
                                false
                            } else {
                                true
                            }
                        }
                        Err(_) => {
                            errors += 1;
                            if errors >= MAX_POLLING_ERRORS {
                                this.polling = false;
                                window.push_notification(
                                    (
                                        NotificationType::Error,
                                        t("onboarding.permissions.errors.checkFailed"),
                                    ),
                                    cx,
                                );
                                false
                            } else {
                                true
                            }
                        }
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn grant_accessibility(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        let backend = self.backend(cx);
        cx.background_spawn(async move {
            backend.request_accessibility_permission();
        })
        .detach();
        self.accessibility = PermissionStatus::Waiting;
        self.start_polling(window, cx);
        cx.notify();
    }

    fn grant_microphone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        let backend = self.backend(cx);
        let windows = self.os == OsType::Windows;
        cx.background_spawn(async move {
            if windows {
                let _ = backend.open_microphone_privacy_settings();
            } else {
                backend.request_microphone_permission();
            }
        })
        .detach();
        self.microphone = PermissionStatus::Waiting;
        self.start_polling(window, cx);
        cx.notify();
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        if self.preview || !self.can_continue(cx) {
            return;
        }
        self.complete(cx);
    }

    /// The microphone is required; Accessibility isn't (without it the
    /// text is copied for the user to paste).
    fn can_continue(&self, _cx: &App) -> bool {
        match self.os {
            OsType::MacOs | OsType::Windows => self.microphone == PermissionStatus::Granted,
            _ => true,
        }
    }

    /// Whether this setup asks for Accessibility: macOS, for dictation.
    fn wants_accessibility(&self, cx: &App) -> bool {
        let st = AppState::global(cx).read(cx);
        self.os == OsType::MacOs && (st.startup.returning_user || st.startup.choices.dictation)
    }

    /// One permission: what it's for, and its state or the grant button.
    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        id: &'static str,
        icon_name: &'static str,
        title: SharedString,
        description: SharedString,
        status: PermissionStatus,
        button_label: SharedString,
        cx: &mut Context<Self>,
        on_grant: fn(&mut Self, &mut Window, &mut Context<Self>),
    ) -> AnyElement {
        let action: AnyElement = match status {
            PermissionStatus::Granted => design::chip("", true, cx)
                .child(icon("check").size(dp(11.)))
                .child(t("onboarding.permissions.granted"))
                .into_any_element(),
            PermissionStatus::Waiting => design::chip("", true, cx)
                .child(design::shimmer_text(
                    SharedString::from(format!("{id}-waiting")),
                    t("onboarding.permissions.waiting"),
                    cx,
                ))
                .into_any_element(),
            PermissionStatus::Checking => design::chip("", false, cx)
                .child(t("onboarding.permissions.checking"))
                .into_any_element(),
            PermissionStatus::Needed => Button::new(SharedString::from(format!("grant-{id}")))
                .outline()
                .small()
                .label(button_label)
                .on_click(cx.listener(move |this, _, window, cx| on_grant(this, window, cx)))
                .into_any_element(),
        };
        row(
            id,
            Some(row_icon(icon_name, cx)),
            title,
            Some(description),
            Some(action),
            cx,
        )
        .test_support()
        .into_any_element()
    }
}

impl Render for PermissionsOnboarding {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let st = AppState::global(cx).read(cx);
        let back = crate::logic::flows::step_back(
            crate::logic::flows::OnboardingStep::Permissions,
            st.startup.returning_user,
        )
        .is_some();
        let meetings = st.startup.choices.meetings && !st.startup.returning_user;
        let accessibility = self.wants_accessibility(cx);
        let is_windows = self.os == OsType::Windows;
        let mic_button = if is_windows {
            t("accessibility.openSettings")
        } else {
            t("onboarding.permissions.grant")
        };
        let mut items = Vec::new();
        if matches!(self.os, OsType::MacOs | OsType::Windows) {
            items.push(self.card(
                "microphone-permission",
                "mic",
                t("onboarding.permissions.microphone.title"),
                t("onboarding.permissions.microphone.description"),
                self.microphone,
                mic_button,
                cx,
                Self::grant_microphone,
            ));
        }
        let skipping_accessibility =
            accessibility && self.accessibility != PermissionStatus::Granted;
        if accessibility {
            let description = if skipping_accessibility {
                t("onboarding.permissions.accessibility.optional")
            } else {
                t("onboarding.permissions.accessibility.description")
            };
            items.push(self.card(
                "accessibility-permission",
                "keyboard",
                t("onboarding.permissions.accessibility.title"),
                description,
                self.accessibility,
                t("onboarding.permissions.grant"),
                cx,
                Self::grant_accessibility,
            ));
        }
        let can_continue = self.can_continue(cx) && !self.preview;
        let label = if skipping_accessibility {
            t("onboarding.permissions.continueWithout")
        } else {
            t("onboarding.continue")
        };
        let mut assistant =
            Assistant::new("permissions-onboarding", t("onboarding.permissions.title"))
                .icon("shield-check")
                .description(t("onboarding.permissions.description"))
                .child(rows(None, items, cx))
                .when(meetings, |a| {
                    a.child(
                        div()
                            .id("permissions-meetings-note")
                            .px(dp(2.))
                            .text_size(design::TEXT_SECONDARY)
                            .line_height(dp(15.))
                            .text_color(tone.text_muted)
                            .child(t("onboarding.permissions.meetingsLater")),
                    )
                });
        if back {
            let preview = self.preview;
            assistant = assistant.footer_start(back_button("permissions-back", move |_, _, cx| {
                if !preview {
                    let state = AppState::global(cx);
                    state.update(cx, |s, cx| s.onboarding_back(cx));
                }
            }));
        }
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "enter" && !ev.keystroke.modifiers.modified() {
                    this.next(cx);
                }
            }))
            .child(assistant.action(continue_button(
                "permissions-continue",
                label,
                can_continue,
                cx.listener(|this, _, _, cx| this.next(cx)),
            )))
    }
}
