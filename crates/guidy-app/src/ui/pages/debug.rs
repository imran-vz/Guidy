//! Debug page (only when debug mode is on): a compact settings group,
//! then the live logs filling the rest of the page as their own
//! virtualized region. Click a line to select it, Shift-click or Shift-↑/↓
//! to extend, ⌘A selects all, ⌘C copies; Pause / Copy / Clear are in the
//! title bar.

use crate::logic::flows::{
    format_logs_for_copy, keyboard_verdict_key, log_level_tag, LogLine, OnboardingStep,
};
use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{
    group, reset_button, t, t_args, toggle, Dropdown, DropdownOption, SettingRow, SliderRow,
};
use crate::ui::pages::advanced::save_error;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
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
    log_list: UniformListScrollHandle,
    log_focus: FocusHandle,
    /// Selected lines by id: (anchor, head).
    log_selection: Option<(u64, u64)>,
    /// Follow new lines (scrolled to the end).
    pinned: bool,
    _subs: Vec<Subscription>,
}

fn set(key: &'static str, value: serde_json::Value, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| s.update_setting(key, value, cx));
}

const LOG_ROW_H: Pixels = dp(18.);

fn ms(v: f32) -> String {
    format!("{}ms", v.round() as i64)
}

impl DebugPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        let sliders = vec![
            cx.new(|cx| {
                SliderRow::new(
                    "word_correction_threshold",
                    "settings.debug.wordCorrectionThreshold.title",
                    Some("settings.debug.wordCorrectionThreshold.description"),
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
                    "settings.debug.pasteDelay.title",
                    Some("settings.debug.pasteDelay.description"),
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
                    "settings.debug.pasteDelayAfter.title",
                    Some("settings.debug.pasteDelayAfter.description"),
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
                    "settings.debug.holdThreshold.title",
                    Some("settings.debug.holdThreshold.description"),
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
                    "settings.debug.recordingBuffer.title",
                    Some("settings.debug.recordingBuffer.description"),
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
            log_list: UniformListScrollHandle::new(),
            log_focus: cx.focus_handle().tab_stop(true),
            log_selection: None,
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
                .secondary()
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
                    .rounded(crate::ui::design::RADIUS_CONTROL)
                    .bg(theme.muted)
                    .font_family(crate::theme::MONO_FONT)
                    .text_size(crate::ui::design::TEXT_SECONDARY)
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

    /// The lines shown: the paused snapshot, or the live buffer.
    fn lines<'a>(&'a self, cx: &'a App) -> &'a [LogLine] {
        match &self.logs_paused {
            Some(snapshot) => snapshot,
            None => &self.state.read(cx).log_lines,
        }
    }

    /// The selected lines (anchor..=head by position), or none.
    fn selected_range(&self, cx: &App) -> Option<std::ops::RangeInclusive<usize>> {
        let (anchor, head) = self.log_selection?;
        let lines = self.lines(cx);
        let a = lines.iter().position(|l| l.id == anchor)?;
        let h = lines.iter().position(|l| l.id == head)?;
        Some(a.min(h)..=a.max(h))
    }

    /// Copy the selected lines, or every line when nothing is selected.
    fn copy_logs(&mut self, cx: &mut Context<Self>) {
        let lines = self.lines(cx);
        let text = match self.selected_range(cx) {
            Some(r) => format_logs_for_copy(&lines[r]),
            None => format_logs_for_copy(lines),
        };
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.logs_copied = true;
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
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.logs_paused = match self.logs_paused {
            Some(_) => None,
            None => Some(self.state.read(cx).log_lines.clone()),
        };
        cx.notify();
    }

    fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.pinned = true;
        self.log_selection = None;
        if self.logs_paused.is_some() {
            self.logs_paused = Some(Vec::new());
        }
        self.state.update(cx, |s, cx| s.clear_logs(cx));
    }

    /// Click a line: select it; Shift-click extends the selection.
    fn click_line(&mut self, id: u64, extend: bool, cx: &mut Context<Self>) {
        self.log_selection = match (extend, self.log_selection) {
            (true, Some((anchor, _))) => Some((anchor, id)),
            _ => Some((id, id)),
        };
        cx.notify();
    }

    fn on_logs_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let cmd = m.platform && !m.alt && !m.control;
        let count = self.lines(cx).len();
        match ev.keystroke.key.as_str() {
            "a" if cmd => {
                let lines = self.lines(cx);
                self.log_selection = lines.first().zip(lines.last()).map(|(a, b)| (a.id, b.id));
            }
            "c" if cmd => self.copy_logs(cx),
            "escape" => self.log_selection = None,
            "up" | "down" if count > 0 && !cmd => {
                let lines = self.lines(cx);
                let head = self
                    .log_selection
                    .and_then(|(_, h)| lines.iter().position(|l| l.id == h));
                let next = match (ev.keystroke.key.as_str(), head) {
                    ("up", Some(i)) => i.saturating_sub(1),
                    ("down", Some(i)) => (i + 1).min(count - 1),
                    _ => count - 1,
                };
                let id = lines[next].id;
                self.log_selection = match (m.shift, self.log_selection) {
                    (true, Some((anchor, _))) => Some((anchor, id)),
                    _ => Some((id, id)),
                };
                self.pinned = next + 1 == count;
                self.log_list.scroll_to_item(next, ScrollStrategy::Nearest);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn render_log_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tone = crate::ui::design::tone(cx);
        let Some(l) = self.lines(cx).get(ix).cloned() else {
            return div().into_any_element();
        };
        let selected = self.selected_range(cx).is_some_and(|r| r.contains(&ix));
        let level_color = match l.level {
            2 => theme.info,
            3 => theme.success,
            4 => theme.warning,
            5 => theme.danger,
            _ => theme.muted_foreground,
        };
        let id = l.id;
        h_flex()
            .id(("log-line", id as usize))
            .h(LOG_ROW_H)
            .px(dp(8.))
            .gap(dp(8.))
            .when(selected, |d| d.bg(tone.selection_focused))
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(l.time.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .w(dp(44.))
                    .text_color(level_color)
                    .child(log_level_tag(l.level)),
            )
            .child(div().flex_1().min_w_0().truncate().child(l.message.clone()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.log_focus, cx);
                    this.click_line(id, ev.modifiers.shift, cx);
                }),
            )
            .into_any_element()
    }

    fn render_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tone = crate::ui::design::tone(cx);
        let ring = theme.ring;
        let paused = self.logs_paused.is_some();
        let count = self.lines(cx).len();
        if self.pinned && count > 0 && !paused {
            self.log_list
                .scroll_to_item(count - 1, ScrollStrategy::Bottom);
        }
        let _ = window;
        let header = h_flex()
            .flex_none()
            .h(dp(26.))
            .px(dp(10.))
            .gap(dp(6.))
            .border_b_1()
            .border_color(tone.hairline)
            .text_size(crate::ui::design::TEXT_SECONDARY)
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("settings.debug.liveLogs.title")),
            )
            .child(div().size(dp(6.)).rounded_full().bg(if paused {
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
                &[("count", &count.to_string())],
            ));
        let list = uniform_list(
            "live-logs-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .map(|ix| this.render_log_row(ix, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.log_list)
        .size_full();
        let body = div()
            .id("live-logs")
            .key_context("LiveLogs")
            .track_focus(&self.log_focus)
            .flex_1()
            .min_h_0()
            .relative()
            .font_family(crate::theme::MONO_FONT)
            .text_size(crate::ui::design::TEXT_SECONDARY)
            .border_1()
            .border_color(transparent_black())
            .focus_visible(move |s| s.border_color(ring))
            .on_key_down(cx.listener(Self::on_logs_key))
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Copy, _, cx| this.copy_logs(cx)),
            )
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::SelectAll, _, cx| {
                    let lines = this.lines(cx);
                    this.log_selection = lines.first().zip(lines.last()).map(|(a, b)| (a.id, b.id));
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                // Follow new lines only while scrolled to the end.
                this.pinned = this.log_list.is_scrolled_to_end().unwrap_or(true);
                cx.notify();
            }))
            .when(count == 0, |d| {
                d.child(
                    div()
                        .p(dp(10.))
                        .font_family(crate::theme::UI_FONT)
                        .text_color(theme.muted_foreground)
                        .child(t("settings.debug.liveLogs.empty")),
                )
            })
            .when(count > 0, |d| d.child(list));
        crate::ui::design::panel(cx)
            .id("live-logs-row")
            .flex_1()
            .min_h_0()
            .child(header)
            .child(body)
            .test_support()
            .into_any_element()
    }
}

impl Render for DebugPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.render_settings(window, cx);
        let logs = self.render_logs(window, cx);
        // Settings start with half of the page (they scroll; the divider moves).
        let settings_h =
            ((window.viewport_size().height - design::TOOLBAR_H - design::STATUS_BAR_H) / 2.)
                .max(dp(120.));
        // A split view like a console under its settings: the divider shows
        // where the settings pane ends (it scrolls), and can be dragged.
        crate::ui::kit::fill_page("page-debug")
            .child(
                v_resizable("debug-split")
                    .child(
                        resizable_panel()
                            .size(settings_h)
                            .size_range(dp(96.)..dp(2000.))
                            .child(
                                div()
                                    .id("debug-settings")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .px(design::PAGE_X)
                                    .pt(design::PAGE_TOP)
                                    .pb(dp(12.))
                                    .child(settings),
                            ),
                    )
                    .child(
                        resizable_panel().size_range(dp(120.)..dp(4000.)).child(
                            v_flex()
                                .size_full()
                                .min_h_0()
                                .px(design::PAGE_X)
                                .pt(dp(12.))
                                .pb(design::PAGE_X)
                                .child(logs),
                        ),
                    ),
            )
            .test_support()
    }
}

impl crate::ui::pages::Page for DebugPage {
    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let paused = self.logs_paused.is_some();
        let empty = self.lines(cx).is_empty();
        let selection = self.log_selection.is_some();
        Some(
            h_flex()
                .gap(dp(4.))
                .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
                .child(
                    Button::new("logs-pause")
                        .ghost()
                        .small()
                        .icon(crate::ui::kit::icon(if paused { "play" } else { "pause" }))
                        .label(if paused {
                            t("settings.debug.liveLogs.resume")
                        } else {
                            t("settings.debug.liveLogs.pause")
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                )
                .child(
                    Button::new("logs-copy")
                        .ghost()
                        .small()
                        .icon(crate::ui::kit::icon(if self.logs_copied {
                            "check"
                        } else {
                            "copy"
                        }))
                        .label(if self.logs_copied {
                            t("settings.debug.liveLogs.copied")
                        } else if selection {
                            t("settings.debug.liveLogs.copySelection")
                        } else {
                            t("settings.debug.liveLogs.copyAll")
                        })
                        .disabled(empty)
                        .on_click(cx.listener(|this, _, _, cx| this.copy_logs(cx))),
                )
                .child(
                    Button::new("logs-clear")
                        .ghost()
                        .small()
                        .icon(crate::ui::kit::icon("trash"))
                        .label(t("common.clear"))
                        .disabled(empty)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_logs(cx))),
                )
                .into_any_element(),
        )
    }
}

impl DebugPage {
    fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let st = self.state.read(cx);
        let Some(s) = st.app_settings().cloned() else {
            return div().into_any_element();
        };
        let locked = st.update_checks_locked;
        let custom = st.custom_sounds.clone();
        let os = OsType::current();
        let _ = window;
        let main = crate::windows::main_window_entity(cx);

        let mut rows = vec![
            SettingRow::new("log-level", t("settings.debug.logLevel.title"))
                .description(t("settings.debug.logLevel.description"))
                .error(save_error(st, "log_level"))
                .control(
                    Dropdown::new("log-level-dropdown")
                        .options(
                            ["error", "warn", "info", "debug", "trace"]
                                .into_iter()
                                .map(|v| {
                                    DropdownOption::new(
                                        v,
                                        t(&format!("settings.debug.logLevel.options.{v}")),
                                    )
                                })
                                .collect(),
                        )
                        .selected(
                            serde_json::to_value(s.log_level)
                                .ok()
                                .and_then(|v| v.as_str().map(str::to_string)),
                        )
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
                    .secondary()
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
                                .secondary()
                                .small()
                                .label(t("settings.debug.onboardingPreview.permissionsButton"))
                                .on_click(move |_, _, cx| {
                                    m1.update(cx, |m, cx| {
                                        m.start_preview(OnboardingStep::Permissions, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("preview-models")
                                .secondary()
                                .small()
                                .label(t("settings.debug.onboardingPreview.modelsButton"))
                                .on_click(move |_, _, cx| {
                                    m2.update(cx, |m, cx| {
                                        m.start_preview(OnboardingStep::Setup, cx)
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
                .error(save_error(st, "update_checks_enabled"))
                .control(toggle(
                    "update-checks-toggle",
                    s.update_checks_enabled && locked == Some(false),
                    is_locked,
                    |v, _, cx| set("update_checks_enabled", json!(v), cx),
                ))
                .into_any_element(),
        );
        let mut sound_options = vec![
            DropdownOption::new("marimba", t("settings.debug.soundTheme.options.marimba")),
            DropdownOption::new("pop", t("settings.debug.soundTheme.options.pop")),
        ];
        if custom.start && custom.stop {
            sound_options.push(DropdownOption::new(
                "custom",
                t("settings.debug.soundTheme.options.custom"),
            ));
        }
        rows.push(
            SettingRow::new("sound-theme", t("settings.debug.soundTheme.label"))
                .description(t("settings.debug.soundTheme.description"))
                .error(save_error(st, "sound_theme"))
                .control(
                    h_flex()
                        .gap_1()
                        // Before the select, like reset buttons, so selects
                        // line up at the row's right edge.
                        .child(
                            Button::new("sound-theme-play")
                                .ghost()
                                .small()
                                .icon(crate::ui::kit::icon("play"))
                                .tooltip(t("settings.debug.soundTheme.preview"))
                                .on_click(|_, _, cx| {
                                    let backend = AppState::global(cx).read(cx).backend.clone();
                                    cx.background_spawn(async move {
                                        backend.play_test_sound("start");
                                        backend.play_test_sound("stop");
                                    })
                                    .detach();
                                }),
                        )
                        .child(
                            Dropdown::new("sound-theme-dropdown")
                                .options(sound_options)
                                .selected(
                                    serde_json::to_value(s.sound_theme)
                                        .ok()
                                        .and_then(|v| v.as_str().map(str::to_string)),
                                )
                                // SoundPicker re-checks for custom sounds on open.
                                .on_open(|_, cx| {
                                    AppState::global(cx)
                                        .update(cx, |s, cx| s.check_custom_sounds(cx))
                                })
                                .on_select(|v, _, cx| set("sound_theme", json!(v), cx)),
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
                    .error(save_error(st, "reliable_paste"))
                    .control(toggle(
                        "reliable-paste-toggle",
                        s.reliable_paste,
                        false,
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
            .error(save_error(st, "always_on_microphone"))
            .control(toggle(
                "always-on-toggle",
                s.always_on_microphone,
                false,
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
            // The list always holds "Default" once loaded.
            let loading = st.audio_devices.is_empty();
            rows.push(
                SettingRow::new(
                    "clamshell-microphone",
                    t("settings.debug.clamshellMicrophone.title"),
                )
                .description(t("settings.debug.clamshellMicrophone.description"))
                .error(save_error(st, "clamshell_microphone"))
                .control(
                    h_flex()
                        .gap_1()
                        .child(reset_button("clamshell-reset", loading, |_, cx| {
                            AppState::global(cx)
                                .update(cx, |s, cx| s.reset_setting("clamshell_microphone", cx))
                        }))
                        .child(
                            Dropdown::new("clamshell-dropdown")
                                .options(options)
                                .selected(Some(selected))
                                .placeholder(if loading {
                                    t("settings.sound.microphone.loading")
                                } else {
                                    t("settings.sound.microphone.placeholder")
                                })
                                .loading(loading)
                                .disabled(loading)
                                .on_open(|_, cx| {
                                    AppState::global(cx)
                                        .update(cx, |s, cx| s.refresh_audio_devices(cx))
                                })
                                .on_select(|v, _, cx| set("clamshell_microphone", json!(v), cx)),
                        ),
                )
                .into_any_element(),
            );
        }
        if os == OsType::MacOs {
            rows.push(self.render_diagnostic(cx));
        }
        // The title bar already says "Debug": no group heading.
        div()
            .max_w(design::FORM_MAX_W)
            .child(group(None, rows, cx))
            .into_any_element()
    }
}
