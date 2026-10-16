//! Debug page (only when debug mode is on).

use crate::logic::flows::{
    format_logs_for_copy, keyboard_verdict_key, log_level_tag, LogLine, OnboardingStep,
};
use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::kit::{
    group, page, reset_button, t, t_args, toggle, Dropdown, DropdownOption, SettingRow, SliderRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Colorize, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::secure_input::KeyboardDiagnosticReport;
use serde_json::json;
use std::time::Duration;

pub struct DebugPage {
    state: Entity<AppState>,
    sliders: Vec<Entity<SliderRow>>,
    is_laptop: bool,
    diag_running: bool,
    diag_report: Option<KeyboardDiagnosticReport>,
    diag_error: Option<String>,
    logs_paused: Option<Vec<LogLine>>,
    logs_copied: bool,
    log_scroll: ScrollHandle,
    pinned: bool,
    _subs: Vec<Subscription>,
}

fn set(key: &'static str, value: serde_json::Value, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| s.update_setting(key, value, cx));
}

fn ms(v: f32) -> String {
    format!("{}ms", v.round() as i64)
}

impl DebugPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe(&state, |this, _, cx| {
            if this.pinned {
                this.log_scroll.scroll_to_bottom();
            }
            cx.notify();
        })];
        let sliders = vec![
            cx.new(|cx| {
                SliderRow::new(
                    "word_correction_threshold",
                    t("settings.debug.wordCorrectionThreshold.title"),
                    t("settings.debug.wordCorrectionThreshold.description"),
                    0.0,
                    1.0,
                    0.01,
                    |v| format!("{v:.2}"),
                    true,
                    cx,
                )
            }),
            cx.new(|cx| {
                SliderRow::new(
                    "paste_delay_ms",
                    t("settings.debug.pasteDelay.title"),
                    t("settings.debug.pasteDelay.description"),
                    10.0,
                    5000.0,
                    10.0,
                    ms,
                    true,
                    cx,
                )
            }),
            cx.new(|cx| {
                SliderRow::new(
                    "paste_delay_after_ms",
                    t("settings.debug.pasteDelayAfter.title"),
                    t("settings.debug.pasteDelayAfter.description"),
                    10.0,
                    5000.0,
                    10.0,
                    ms,
                    true,
                    cx,
                )
            }),
            cx.new(|cx| {
                SliderRow::new(
                    "hold_threshold_ms",
                    t("settings.debug.holdThreshold.title"),
                    t("settings.debug.holdThreshold.description"),
                    100.0,
                    1000.0,
                    50.0,
                    ms,
                    true,
                    cx,
                )
            }),
            cx.new(|cx| {
                SliderRow::new(
                    "extra_recording_buffer_ms",
                    t("settings.debug.recordingBuffer.title"),
                    t("settings.debug.recordingBuffer.description"),
                    0.0,
                    1500.0,
                    50.0,
                    ms,
                    true,
                    cx,
                )
            }),
        ];
        let this = Self {
            state,
            sliders,
            is_laptop: false,
            diag_running: false,
            diag_report: None,
            diag_error: None,
            logs_paused: None,
            logs_copied: false,
            log_scroll: ScrollHandle::new(),
            pinned: true,
            _subs: subs,
        };
        let backend = this.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let laptop = cx
                .background_spawn(async move { backend.is_laptop().unwrap_or(false) })
                .await;
            this.update(cx, |this, cx| {
                this.is_laptop = laptop;
                cx.notify();
            })
            .ok();
        })
        .detach();
        this.state.update(cx, |s, cx| s.check_custom_sounds(cx));
        this
    }

    fn run_diagnostic(&mut self, cx: &mut Context<Self>) {
        self.diag_running = true;
        self.diag_error = None;
        cx.notify();
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.run_keyboard_diagnostic(10) })
                .await;
            this.update(cx, |this, cx| {
                this.diag_running = false;
                match r {
                    Ok(rep) => this.diag_report = Some(rep),
                    Err(e) => this.diag_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_diagnostic(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut body = v_flex().w_full().gap_2().child(
            Button::new("diag-run")
                .outline()
                .small()
                .label(if self.diag_running {
                    t("settings.debug.keyboardDiagnostic.running")
                } else {
                    t("settings.debug.keyboardDiagnostic.run")
                })
                .loading(self.diag_running)
                .disabled(self.diag_running)
                .on_click(cx.listener(|this, _, _, cx| this.run_diagnostic(cx))),
        );
        if let Some(e) = &self.diag_error {
            body = body.child(div().text_sm().text_color(theme.danger).child(t_args(
                "settings.debug.keyboardDiagnostic.failed",
                &[("error", e)],
            )));
        }
        if let Some(r) = &self.diag_report {
            let state = if r.secure_input_enabled {
                guidy_i18n::t("settings.debug.keyboardDiagnostic.enabled")
            } else {
                guidy_i18n::t("settings.debug.keyboardDiagnostic.disabled")
            };
            let line = if !r.secure_input_enabled {
                state
            } else {
                let holder = match &r.culprit_name {
                    Some(name) => guidy_i18n::t_args(
                        "settings.debug.keyboardDiagnostic.holder",
                        &[
                            ("name", name),
                            (
                                "pid",
                                &r.culprit_pid.map(|p| p.to_string()).unwrap_or_default(),
                            ),
                        ],
                    ),
                    None => guidy_i18n::t("settings.debug.keyboardDiagnostic.holderUnknown"),
                };
                format!("{state} — {holder}")
            };
            body = body.child(
                v_flex()
                    .p_3()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .font_family(crate::theme::MONO_FONT)
                    .text_xs()
                    .gap_1()
                    .child(format!(
                        "{}: {}",
                        guidy_i18n::t("settings.debug.keyboardDiagnostic.secureInputLabel"),
                        line
                    ))
                    .child(format!(
                        "{}: {} · {}: {} · {}: {} · {}: {}",
                        guidy_i18n::t("settings.debug.keyboardDiagnostic.keyDown"),
                        r.key_down,
                        guidy_i18n::t("settings.debug.keyboardDiagnostic.keyUp"),
                        r.key_up,
                        guidy_i18n::t("settings.debug.keyboardDiagnostic.flagsChanged"),
                        r.flags_changed,
                        guidy_i18n::t("settings.debug.keyboardDiagnostic.mouse"),
                        r.mouse
                    ))
                    .child(
                        div()
                            .font_family(crate::theme::UI_FONT)
                            .text_sm()
                            .child(t(keyboard_verdict_key(r))),
                    ),
            );
        }
        SettingRow::new(
            "keyboard-diagnostic",
            t("settings.debug.keyboardDiagnostic.title"),
        )
        .description(t("settings.debug.keyboardDiagnostic.description"))
        .stacked()
        .control(body)
        .into_any_element()
    }

    fn render_logs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let lines = match &self.logs_paused {
            Some(snapshot) => snapshot.clone(),
            None => self.state.read(cx).log_lines.clone(),
        };
        let paused = self.logs_paused.is_some();
        let empty = lines.is_empty();
        let count = lines.len().to_string();
        let level_color = |level: u8| match level {
            2 => theme.info,
            3 => theme.success,
            4 => theme.warning,
            5 => theme.danger,
            _ => theme.muted_foreground,
        };
        let header = h_flex()
            .w_full()
            .justify_between()
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().size(px(8.)).rounded_full().bg(if paused {
                        theme.muted_foreground
                    } else {
                        theme.success
                    }))
                    .child(if paused {
                        t("settings.debug.liveLogs.paused")
                    } else {
                        t("settings.debug.liveLogs.live")
                    })
                    .child("·")
                    .child(t_args(
                        "settings.debug.liveLogs.lineCount",
                        &[("count", &count)],
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("logs-pause")
                            .outline()
                            .xsmall()
                            .label(if paused {
                                t("settings.debug.liveLogs.resume")
                            } else {
                                t("settings.debug.liveLogs.pause")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.logs_paused = match this.logs_paused {
                                    Some(_) => None,
                                    None => Some(this.state.read(cx).log_lines.clone()),
                                };
                                cx.notify();
                            })),
                    )
                    .child({
                        let text = format_logs_for_copy(&lines);
                        Button::new("logs-copy")
                            .outline()
                            .xsmall()
                            .label(if self.logs_copied {
                                t("settings.debug.liveLogs.copied")
                            } else {
                                t("common.copy")
                            })
                            .disabled(empty)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                this.logs_copied = true;
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(1500))
                                        .await;
                                    this.update(cx, |this, cx| {
                                        this.logs_copied = false;
                                        cx.notify();
                                    })
                                    .ok();
                                })
                                .detach();
                                cx.notify();
                            }))
                    })
                    .child(
                        Button::new("logs-clear")
                            .outline()
                            .xsmall()
                            .label(t("common.clear"))
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pinned = true;
                                if this.logs_paused.is_some() {
                                    this.logs_paused = Some(Vec::new());
                                }
                                this.state.update(cx, |s, cx| s.clear_logs(cx));
                            })),
                    ),
            );
        let surface = if theme.mode.is_dark() {
            theme.background.darken(0.15)
        } else {
            theme.background.darken(0.06)
        };
        let body = v_flex()
            .id("live-logs")
            .h(px(288.))
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.log_scroll)
            .p_3()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(surface)
            .font_family(crate::theme::MONO_FONT)
            .text_xs()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                // Re-pin only within 24 px of the bottom.
                let offset = this.log_scroll.offset();
                let max = this.log_scroll.max_offset();
                this.pinned = (max.y + offset.y) < px(crate::logic::flows::LOG_PIN_THRESHOLD_PX);
                cx.notify();
            }))
            .when(empty, |d| {
                d.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(t("settings.debug.liveLogs.empty")),
                )
            })
            .children(lines.into_iter().map(|l| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(l.time.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(48.))
                            .text_color(level_color(l.level))
                            .child(log_level_tag(l.level)),
                    )
                    .child(div().flex_1().min_w_0().child(l.message.clone()))
            }));
        SettingRow::new("live-logs-row", t("settings.debug.liveLogs.title"))
            .description(t("settings.debug.liveLogs.description"))
            .stacked()
            .control(v_flex().w_full().gap_2().child(header).child(body))
            .into_any_element()
    }
}

impl Render for DebugPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let Some(s) = st.app_settings().cloned() else {
            return page().into_any_element();
        };
        let locked = st.update_checks_locked;
        let custom = st.custom_sounds.clone();
        let up = |k: &str| st.settings.is_updating(k);
        let os = OsType::current();
        let _ = window;
        let main = crate::windows::main_window_entity(cx);

        let mut rows = vec![
            SettingRow::new("log-level", t("settings.debug.logLevel.title"))
                .description(t("settings.debug.logLevel.description"))
                .control(
                    Dropdown::new("log-level-dropdown")
                        .options(
                            [
                                ("error", "Error"),
                                ("warn", "Warn"),
                                ("info", "Info"),
                                ("debug", "Debug"),
                                ("trace", "Trace"),
                            ]
                            .into_iter()
                            .map(|(v, l)| DropdownOption::new(v, l))
                            .collect(),
                        )
                        .selected(
                            serde_json::to_value(s.log_level)
                                .ok()
                                .and_then(|v| v.as_str().map(str::to_string)),
                        )
                        .disabled(up("log_level"))
                        .on_select(|v, _, cx| set("log_level", json!(v), cx)),
                )
                .into_any_element(),
            SettingRow::new(
                "whats-new-preview",
                t("settings.debug.whatsNewPreview.title"),
            )
            .description(t("settings.debug.whatsNewPreview.description"))
            .control(
                Button::new("whats-new-preview-button")
                    .outline()
                    .small()
                    .label(t("settings.debug.whatsNewPreview.button"))
                    .on_click(|_, window, cx| {
                        if !crate::ui::whats_new::preview_latest(window, cx) {
                            AppState::global(cx).update(cx, |s, cx| {
                                s.toast_error(t("settings.debug.whatsNewPreview.noNotes"), cx)
                            });
                        }
                    }),
            )
            .into_any_element(),
        ];
        if let Some(main) = main {
            let (m1, m2) = (main.clone(), main.clone());
            rows.push(
                SettingRow::new(
                    "onboarding-preview",
                    t("settings.debug.onboardingPreview.title"),
                )
                .description(t("settings.debug.onboardingPreview.description"))
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("preview-permissions")
                                .outline()
                                .small()
                                .label(t("settings.debug.onboardingPreview.permissionsButton"))
                                .on_click(move |_, _, cx| {
                                    m1.update(cx, |m, cx| {
                                        m.start_preview(OnboardingStep::Accessibility, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("preview-models")
                                .outline()
                                .small()
                                .label(t("settings.debug.onboardingPreview.modelsButton"))
                                .on_click(move |_, _, cx| {
                                    m2.update(cx, |m, cx| {
                                        m.start_preview(OnboardingStep::Model, cx)
                                    });
                                }),
                        ),
                )
                .into_any_element(),
            );
        }
        let is_locked = locked != Some(false);
        rows.push(
            SettingRow::new("update-checks", t("settings.debug.updateChecks.label"))
                .description(if locked == Some(true) {
                    t("settings.debug.updateChecks.lockedDescription")
                } else {
                    t("settings.debug.updateChecks.description")
                })
                .control(toggle(
                    "update-checks-toggle",
                    s.update_checks_enabled && locked == Some(false),
                    is_locked || up("update_checks_enabled"),
                    |v, _, cx| set("update_checks_enabled", json!(v), cx),
                ))
                .into_any_element(),
        );
        let mut sound_options = vec![
            DropdownOption::new("marimba", "Marimba"),
            DropdownOption::new("pop", "Pop"),
        ];
        if custom.start && custom.stop {
            sound_options.push(DropdownOption::new("custom", "Custom"));
        }
        rows.push(
            SettingRow::new("sound-theme", t("settings.debug.soundTheme.label"))
                .description(t("settings.debug.soundTheme.description"))
                .control(
                    h_flex()
                        .gap_1()
                        .child(
                            Dropdown::new("sound-theme-dropdown")
                                .options(sound_options)
                                .selected(
                                    serde_json::to_value(s.sound_theme)
                                        .ok()
                                        .and_then(|v| v.as_str().map(str::to_string)),
                                )
                                .disabled(up("sound_theme"))
                                .on_select(|v, _, cx| set("sound_theme", json!(v), cx)),
                        )
                        .child(
                            Button::new("sound-theme-play")
                                .ghost()
                                .small()
                                .icon(crate::ui::kit::icon("play"))
                                .on_click(|_, _, cx| {
                                    let backend = AppState::global(cx).read(cx).backend.clone();
                                    cx.background_spawn(async move {
                                        backend.play_test_sound("start");
                                        backend.play_test_sound("stop");
                                    })
                                    .detach();
                                }),
                        ),
                )
                .into_any_element(),
        );
        rows.push(self.sliders[0].clone().into_any_element());
        rows.push(self.sliders[1].clone().into_any_element());
        rows.push(self.sliders[2].clone().into_any_element());
        if matches!(os, OsType::MacOs | OsType::Windows) {
            rows.push(
                SettingRow::new("reliable-paste", t("settings.debug.reliablePaste.title"))
                    .description(t("settings.debug.reliablePaste.description"))
                    .control(toggle(
                        "reliable-paste-toggle",
                        s.reliable_paste,
                        up("reliable_paste"),
                        |v, _, cx| set("reliable_paste", json!(v), cx),
                    ))
                    .into_any_element(),
            );
        }
        rows.push(self.sliders[3].clone().into_any_element());
        rows.push(self.sliders[4].clone().into_any_element());
        rows.push(
            SettingRow::new(
                "always-on-microphone",
                t("settings.debug.alwaysOnMicrophone.label"),
            )
            .description(t("settings.debug.alwaysOnMicrophone.description"))
            .control(toggle(
                "always-on-toggle",
                s.always_on_microphone,
                up("always_on_microphone"),
                |v, _, cx| set("always_on_microphone", json!(v), cx),
            ))
            .into_any_element(),
        );
        if self.is_laptop {
            let options = st
                .audio_devices
                .iter()
                .map(|d| DropdownOption::new(d.name.clone(), d.name.clone()))
                .collect();
            let selected = match s.clamshell_microphone.as_deref() {
                None | Some("default") => "Default".to_string(),
                Some(m) => m.to_string(),
            };
            let busy = up("clamshell_microphone");
            rows.push(
                SettingRow::new(
                    "clamshell-microphone",
                    t("settings.debug.clamshellMicrophone.title"),
                )
                .description(t("settings.debug.clamshellMicrophone.description"))
                .control(
                    h_flex()
                        .gap_1()
                        .child(
                            Dropdown::new("clamshell-dropdown")
                                .options(options)
                                .selected(Some(selected))
                                .placeholder(t("settings.sound.microphone.placeholder"))
                                .disabled(busy)
                                .on_open(|_, cx| {
                                    AppState::global(cx)
                                        .update(cx, |s, cx| s.refresh_audio_devices(cx))
                                })
                                .on_select(|v, _, cx| set("clamshell_microphone", json!(v), cx)),
                        )
                        .child(reset_button("clamshell-reset", busy, |_, cx| {
                            AppState::global(cx)
                                .update(cx, |s, cx| s.reset_setting("clamshell_microphone", cx))
                        })),
                )
                .into_any_element(),
            );
        }
        if os == OsType::MacOs {
            rows.push(self.render_diagnostic(cx));
        }
        rows.push(self.render_logs(cx));

        page()
            .id("page-debug")
            .child(group(Some(t("settings.debug.title")), rows, cx))
            .test_support()
            .into_any_element()
    }
}
