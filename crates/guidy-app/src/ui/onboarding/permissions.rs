//! Permission onboarding: microphone and
//! (macOS) accessibility, with 1 s polling after the user acts.

use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::brand::wordmark;
use crate::ui::kit::{icon, spinner, t};
use crate::ui::main_window::MainWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, StyledExt, WindowExt};
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
}

impl PermissionsOnboarding {
    pub fn new(
        main: WeakEntity<MainWindow>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let os = OsType::current();
        let mut this = Self {
            main,
            preview,
            os,
            accessibility: PermissionStatus::Checking,
            microphone: PermissionStatus::Checking,
            polling: false,
            completed: false,
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
            this.complete(window, cx);
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
            this.update_in(cx, |this, window, cx| {
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
                if this.all_granted() {
                    this.complete(window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.completed || self.preview {
            return;
        }
        self.completed = true;
        AppState::global(cx).update(cx, |s, cx| {
            s.refresh_audio_devices(cx);
            s.refresh_output_devices(cx);
        });
        let main = self.main.clone();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            main.update(cx, |m, cx| m.on_permissions_complete(cx)).ok();
        })
        .detach();
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
                                this.complete(window, cx);
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
    ) -> impl IntoElement {
        let theme = cx.theme();
        let action: AnyElement = match status {
            PermissionStatus::Granted => h_flex()
                .gap_2()
                .text_sm()
                .text_color(theme.success)
                .child(icon("check").small())
                .child(t("onboarding.permissions.granted"))
                .into_any_element(),
            PermissionStatus::Waiting => h_flex()
                .gap_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(spinner("permission-wait", icon("loader-circle").small()))
                .child(t("onboarding.permissions.waiting"))
                .into_any_element(),
            _ => Button::new(SharedString::from(format!("grant-{id}")))
                .primary()
                .small()
                .label(button_label)
                .on_click(cx.listener(move |this, _, window, cx| on_grant(this, window, cx)))
                .into_any_element(),
        };
        h_flex()
            .id(id)
            .w_full()
            .p_4()
            .gap_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                div()
                    .p_3()
                    .rounded_full()
                    .bg(theme.primary.opacity(0.15))
                    .text_color(theme.primary)
                    .child(icon(icon_name)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .items_start()
                    .child(div().font_semibold().child(title))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .mb_2()
                            .child(description),
                    )
                    .child(action),
            )
    }
}

impl Render for PermissionsOnboarding {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let checking = match self.os {
            OsType::MacOs => {
                self.accessibility == PermissionStatus::Checking
                    && self.microphone == PermissionStatus::Checking
            }
            OsType::Windows => self.microphone == PermissionStatus::Checking,
            _ => false,
        };
        if checking {
            return v_flex()
                .id("permissions-checking")
                .size_full()
                .items_center()
                .justify_center()
                .child(spinner(
                    "permissions-loading",
                    icon("loader-circle")
                        .large()
                        .text_color(theme.muted_foreground),
                ))
                .into_any_element();
        }
        if self.all_granted() && !self.preview {
            return v_flex()
                .id("permissions-granted")
                .size_full()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .p_4()
                        .rounded_full()
                        .bg(theme.success.opacity(0.2))
                        .text_color(theme.success)
                        .child(icon("check").large()),
                )
                .child(
                    div()
                        .text_lg()
                        .child(t("onboarding.permissions.allGranted")),
                )
                .into_any_element();
        }
        let is_windows = self.os == OsType::Windows;
        let mic_button = if is_windows {
            t("accessibility.openSettings")
        } else {
            t("onboarding.permissions.grant")
        };
        v_flex()
            .id("permissions-onboarding")
            .size_full()
            .p_6()
            .gap_6()
            .items_center()
            .justify_center()
            .child(wordmark(px(28.), cx))
            .child(
                v_flex()
                    .max_w(px(448.))
                    .w_full()
                    .gap_4()
                    .items_center()
                    .child(
                        v_flex()
                            .items_center()
                            .gap_2()
                            .mb_2()
                            .child(
                                div()
                                    .text_xl()
                                    .font_semibold()
                                    .child(t("onboarding.permissions.title")),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_color(theme.muted_foreground)
                                    .child(t("onboarding.permissions.description")),
                            ),
                    )
                    .when(matches!(self.os, OsType::MacOs | OsType::Windows), |d| {
                        d.child(self.card(
                            "microphone-permission",
                            "mic",
                            t("onboarding.permissions.microphone.title"),
                            t("onboarding.permissions.microphone.description"),
                            self.microphone,
                            mic_button.clone(),
                            cx,
                            Self::grant_microphone,
                        ))
                    })
                    .when(self.os == OsType::MacOs, |d| {
                        d.child(self.card(
                            "accessibility-permission",
                            "keyboard",
                            t("onboarding.permissions.accessibility.title"),
                            t("onboarding.permissions.accessibility.description"),
                            self.accessibility,
                            t("onboarding.permissions.grant"),
                            cx,
                            Self::grant_accessibility,
                        ))
                    }),
            )
            .test_support()
            .into_any_element()
    }
}
