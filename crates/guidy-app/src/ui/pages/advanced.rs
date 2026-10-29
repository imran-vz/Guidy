//! Advanced page: app, overlay, output, transcription, history and
//! experimental settings.
//!
//! Text fields save on blur or Enter, never per keystroke, and never
//! disable themselves while a save is in flight. Lowering the history limit
//! below what is stored asks before anything is deleted.

use crate::logic::flows::{
    accelerator_options, add_custom_word, auto_submit_options, auto_submit_value,
    auto_submit_writes, decode_accelerator, displayed_accelerator, encode_accelerator,
    model_unload_options, normalize_custom_word, paste_method_options, AddWordOutcome,
    MAX_CUSTOM_WORD_LEN,
};
use crate::logic::history::{parse_history_limit, pruned_by_limit, HISTORY_LIMIT_MAX};
use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{
    confirm, group, icon, page, t, t_args, toggle, Dropdown, DropdownOption, LocalizedPlaceholder,
    SettingRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::transcription::AvailableAccelerators;
use guidy_core::settings::{
    AppSettings, KeyboardImplementation, OverlayStyle, PasteMethod, RecordingRetentionPeriod,
};
use serde_json::json;

pub struct AdvancedPage {
    state: Entity<AppState>,
    custom_word: Entity<InputState>,
    /// Inline error under the custom-words field (duplicate, too long).
    word_error: Option<SharedString>,
    history_limit: Entity<InputState>,
    /// Inline error under the history limit (not a number, out of range).
    limit_error: Option<SharedString>,
    /// A lower limit is being checked or confirmed: blur/Enter while the
    /// alert is up must not start a second one.
    limit_confirming: bool,
    script_path: Entity<InputState>,
    accelerators: Option<AvailableAccelerators>,
    typing_tools: Option<Vec<String>>,
    keyboard_impl_error: Option<SharedString>,
    _subs: Vec<Subscription>,
}

fn set(key: &'static str, value: serde_json::Value, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| s.update_setting(key, value, cx));
}

fn json_str<T: serde::Serialize>(v: T) -> Option<String> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
}

/// "Couldn't save: …" for a setting whose last write failed.
pub fn save_error(st: &AppState, key: &str) -> Option<SharedString> {
    st.settings
        .error_of(key)
        .map(|e| t_args("settings.saveFailed", &[("error", e)]))
}

impl AdvancedPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let settings = state.read(cx).app_settings().cloned();
        let custom_word = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.advanced.customWords.placeholder",
                window,
                cx,
            )
        });
        let history_limit = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                settings
                    .as_ref()
                    .map(|s| s.history_limit)
                    .unwrap_or(5)
                    .to_string(),
            )
        });
        let script_path = cx.new(|cx| {
            InputState::new(window, cx)
                .localized_placeholder(
                    "settings.advanced.pasteMethod.externalScriptPlaceholder",
                    window,
                    cx,
                )
                .default_value(
                    settings
                        .as_ref()
                        .and_then(|s| s.external_script_path.clone())
                        .unwrap_or_default(),
                )
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_inputs(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(
                &custom_word,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.add_word(window, cx),
                    InputEvent::Change => {
                        this.word_error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &history_limit,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        this.commit_history_limit(window, cx)
                    }
                    // Typing clears the previous error.
                    InputEvent::Change if this.limit_error.take().is_some() => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe(&script_path, |this, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    let current = this
                        .state
                        .read(cx)
                        .app_settings()
                        .and_then(|s| s.external_script_path.clone())
                        .unwrap_or_default();
                    if v != current {
                        set("external_script_path", json!(v), cx);
                    }
                }
            }),
        ];
        let mut this = Self {
            state,
            custom_word,
            word_error: None,
            history_limit,
            limit_error: None,
            limit_confirming: false,
            script_path,
            accelerators: None,
            typing_tools: None,
            keyboard_impl_error: None,
            _subs: subs,
        };
        this.load_options(cx);
        this
    }

    /// Mirror stored values into fields the user is not editing.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let limit = s.history_limit.to_string();
        if !self.limit_confirming
            && self.limit_error.is_none()
            && !self.history_limit.focus_handle(cx).is_focused(window)
            && self.history_limit.read(cx).value() != limit.as_str()
        {
            self.history_limit
                .update(cx, |st, cx| st.set_value(limit, window, cx));
        }
        let script = s.external_script_path.clone().unwrap_or_default();
        if !self.script_path.focus_handle(cx).is_focused(window)
            && self.script_path.read(cx).value() != script.as_str()
        {
            self.script_path
                .update(cx, |st, cx| st.set_value(script, window, cx));
        }
    }

    fn load_options(&mut self, cx: &mut Context<Self>) {
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let (acc, tools) = cx
                .background_spawn(async move {
                    let tools = (OsType::current() == OsType::Linux)
                        .then(|| backend.get_available_typing_tools());
                    (backend.get_available_accelerators(), tools)
                })
                .await;
            this.update(cx, |this, cx| {
                this.accelerators = Some(acc);
                this.typing_tools = tools;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -- History limit -------------------------------------------------------

    /// Save the typed history limit (blur/Enter). Invalid input shows an
    /// inline error and saves nothing; a limit that would delete stored
    /// transcriptions asks first.
    fn commit_history_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.limit_confirming {
            return;
        }
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let text = self.history_limit.read(cx).value().to_string();
        let Some(limit) = parse_history_limit(&text) else {
            self.limit_error = Some(t_args(
                "settings.advanced.historyLimit.invalid",
                &[("max", &HISTORY_LIMIT_MAX.to_string())],
            ));
            cx.notify();
            return;
        };
        self.limit_error = None;
        if limit == s.history_limit {
            // Normalize " 5" → "5".
            if text != limit.to_string() {
                self.history_limit
                    .update(cx, |st, cx| st.set_value(limit.to_string(), window, cx));
            }
            cx.notify();
            return;
        }
        let prunes = s.recording_retention_period == RecordingRetentionPeriod::PreserveLimit
            && limit < s.history_limit;
        if !prunes {
            set("history_limit", json!(limit), cx);
            return;
        }
        // Count what the backend would delete (unsaved entries beyond the
        // new limit) before writing anything.
        self.limit_confirming = true;
        cx.notify();
        let backend = self.state.read(cx).backend.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let unsaved = cx
                .background_spawn(async move { count_unsaved(&*backend) })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| match unsaved {
                    Ok(n) if pruned_by_limit(n, limit) == 0 => {
                        this.limit_confirming = false;
                        set("history_limit", json!(limit), cx);
                    }
                    Ok(n) => this.confirm_prune(limit, pruned_by_limit(n, limit), window, cx),
                    Err(e) => {
                        this.limit_confirming = false;
                        this.limit_error = Some(t_args("settings.saveFailed", &[("error", &e)]));
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn confirm_prune(
        &mut self,
        limit: usize,
        deleted: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = cx.entity().downgrade();
        let page_cancel = page.clone();
        confirm(
            window,
            cx,
            t_args(
                "settings.advanced.historyLimit.confirmTitle",
                &[("count", &deleted.to_string())],
            ),
            t_args(
                "settings.advanced.historyLimit.confirmDescription",
                &[("limit", &limit.to_string())],
            ),
            t("common.delete"),
            true,
            move |_, cx| {
                page.update(cx, |this, cx| {
                    this.limit_confirming = false;
                    set("history_limit", json!(limit), cx);
                    cx.notify();
                })
                .ok();
            },
            move |window, cx| {
                page_cancel
                    .update(cx, |this, cx| {
                        this.limit_confirming = false;
                        // Back to the stored limit.
                        if let Some(current) =
                            this.state.read(cx).app_settings().map(|s| s.history_limit)
                        {
                            this.history_limit
                                .update(cx, |st, cx| st.set_value(current.to_string(), window, cx));
                        }
                        cx.notify();
                    })
                    .ok();
            },
        );
    }

    #[cfg(test)]
    pub fn limit_error(&self) -> Option<&SharedString> {
        self.limit_error.as_ref()
    }

    #[cfg(test)]
    pub fn word_error(&self) -> Option<&SharedString> {
        self.word_error.as_ref()
    }

    // -- Custom words --------------------------------------------------------

    fn add_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.custom_word.read(cx).value().to_string();
        let words = self
            .state
            .read(cx)
            .app_settings()
            .map(|s| s.custom_words.clone())
            .unwrap_or_default();
        match add_custom_word(&words, &input) {
            AddWordOutcome::Added(next) => {
                self.word_error = None;
                set("custom_words", json!(next), cx);
                self.custom_word
                    .update(cx, |s, cx| s.set_value("", window, cx));
            }
            AddWordOutcome::Duplicate(word) => {
                self.word_error = Some(t_args(
                    "settings.advanced.customWords.duplicate",
                    &[("word", &word)],
                ));
            }
            AddWordOutcome::Invalid => {
                if normalize_custom_word(&input).chars().count() > MAX_CUSTOM_WORD_LEN {
                    self.word_error = Some(t_args(
                        "settings.advanced.customWords.tooLong",
                        &[("max", &MAX_CUSTOM_WORD_LEN.to_string())],
                    ));
                }
            }
        }
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_custom_word(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.custom_word
            .update(cx, |s, cx| s.set_value(text, window, cx));
        cx.notify();
    }

    fn remove_word(word: String, cx: &mut App) {
        let state = AppState::global(cx);
        let words: Vec<String> = state
            .read(cx)
            .app_settings()
            .map(|s| {
                s.custom_words
                    .iter()
                    .filter(|w| **w != word)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        set("custom_words", json!(words), cx);
    }

    fn change_keyboard_implementation(&mut self, value: String, cx: &mut Context<Self>) {
        self.keyboard_impl_error = None;
        let page = cx.entity().downgrade();
        self.state.update(cx, |s, cx| {
            s.settings.updating.insert("keyboard_implementation".into());
            cx.notify();
            s.call(
                cx,
                move |b| b.change_keyboard_implementation(&value),
                move |s, r, cx| {
                    s.settings.updating.remove("keyboard_implementation");
                    match r {
                        Ok(result) => {
                            if !result.reset_bindings.is_empty() {
                                s.toast_kind(
                                    crate::logic::settings_store::ToastKind::Warning,
                                    t("settings.debug.keyboardImplementation.bindingsReset"),
                                    cx,
                                );
                            }
                            s.refresh_settings(cx);
                        }
                        Err(e) => {
                            page.update(cx, |this, cx| {
                                this.keyboard_impl_error =
                                    Some(t_args("settings.saveFailed", &[("error", &e)]));
                                cx.notify();
                            })
                            .ok();
                        }
                    }
                },
            );
        });
    }

    // -- Groups --------------------------------------------------------------

    fn toggle_row(
        st: &AppState,
        id: &'static str,
        key: &'static str,
        title: &str,
        description: &str,
        checked: bool,
    ) -> AnyElement {
        SettingRow::new(id, t(title))
            .description(t(&platform_key(description)))
            .error(save_error(st, key))
            .control(toggle(
                SharedString::from(format!("{id}-toggle")),
                checked,
                false,
                move |v, _, cx| set(key, json!(v), cx),
            ))
            .into_any_element()
    }

    fn app_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let unload_options = model_unload_options(s.debug_mode)
            .into_iter()
            .map(|o| DropdownOption::new(o.value.clone(), o.label()))
            .collect();
        let rows = vec![
            Self::toggle_row(
                st,
                "start-hidden",
                "start_hidden",
                "settings.advanced.startHidden.label",
                "settings.advanced.startHidden.description",
                s.start_hidden,
            ),
            Self::toggle_row(
                st,
                "autostart",
                "autostart_enabled",
                "settings.advanced.autostart.label",
                "settings.advanced.autostart.description",
                s.autostart_enabled,
            ),
            Self::toggle_row(
                st,
                "show-tray-icon",
                "show_tray_icon",
                "settings.advanced.showTrayIcon.label",
                "settings.advanced.showTrayIcon.description",
                s.show_tray_icon,
            ),
            SettingRow::new("model-unload", t("settings.advanced.modelUnload.title"))
                .description(t("settings.advanced.modelUnload.description"))
                .error(save_error(st, "model_unload_timeout"))
                .control(
                    Dropdown::new("model-unload-dropdown")
                        .options(unload_options)
                        .selected(json_str(s.model_unload_timeout))
                        .on_select(|v, _, cx| set("model_unload_timeout", json!(v), cx)),
                )
                .into_any_element(),
            Self::toggle_row(
                st,
                "experimental",
                "experimental_enabled",
                "settings.advanced.experimentalToggle.label",
                "settings.advanced.experimentalToggle.description",
                s.experimental_enabled,
            ),
        ];
        group(Some(t("settings.advanced.groups.app")), rows, cx)
    }

    fn overlay_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let mut rows =
            vec![
                SettingRow::new("overlay-style", t("settings.advanced.overlay.style.label"))
                    .description(t(&platform_key(
                        "settings.advanced.overlay.style.description",
                    )))
                    .error(save_error(st, "overlay_style"))
                    .control(
                        Dropdown::new("overlay-style-dropdown")
                            .options(vec![
                                DropdownOption::new(
                                    "none",
                                    t("settings.advanced.overlay.style.options.none"),
                                ),
                                DropdownOption::new(
                                    "minimal",
                                    t("settings.advanced.overlay.style.options.minimal"),
                                ),
                                DropdownOption::new(
                                    "live",
                                    t("settings.advanced.overlay.style.options.live"),
                                ),
                            ])
                            .selected(json_str(s.overlay_style))
                            .on_select(|v, _, cx| set("overlay_style", json!(v), cx)),
                    )
                    .into_any_element(),
            ];
        if s.overlay_style != OverlayStyle::None {
            rows.push(
                SettingRow::new(
                    "overlay-position",
                    t("settings.advanced.overlay.position.title"),
                )
                .description(t("settings.advanced.overlay.position.description"))
                .error(save_error(st, "overlay_position"))
                .control(
                    Dropdown::new("overlay-position-dropdown")
                        .options(vec![
                            DropdownOption::new(
                                "bottom",
                                t("settings.advanced.overlay.position.options.bottom"),
                            ),
                            DropdownOption::new(
                                "top",
                                t("settings.advanced.overlay.position.options.top"),
                            ),
                        ])
                        .selected(json_str(s.overlay_position))
                        .on_select(|v, _, cx| set("overlay_position", json!(v), cx)),
                )
                .into_any_element(),
            );
        }
        group(Some(t("settings.advanced.groups.overlay")), rows, cx)
    }

    fn output_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let os = OsType::current();
        let paste_options = paste_method_options(os, s.paste_method)
            .into_iter()
            .map(|o| DropdownOption::new(o.value.clone(), o.label()).disabled(o.disabled))
            .collect();
        let mut paste_control = v_flex().gap(dp(6.)).items_end().child(
            Dropdown::new("paste-method-dropdown")
                .options(paste_options)
                .selected(json_str(s.paste_method))
                .on_select(|v, _, cx| set("paste_method", json!(v), cx)),
        );
        if s.paste_method == PasteMethod::ExternalScript {
            paste_control = paste_control.child(
                div()
                    .id("external-script-input")
                    .w(dp(200.))
                    .child(Input::new(&self.script_path).small())
                    .test_support(),
            );
        }
        let mut rows =
            vec![
                SettingRow::new("paste-method", t("settings.advanced.pasteMethod.title"))
                    .description(t("settings.advanced.pasteMethod.description"))
                    .error(
                        save_error(st, "paste_method").or(save_error(st, "external_script_path")),
                    )
                    .control(paste_control)
                    .into_any_element(),
            ];

        if os == OsType::Linux && s.paste_method == PasteMethod::Direct {
            let tools = self
                .typing_tools
                .clone()
                .unwrap_or_else(|| vec!["auto".into()]);
            let options = tools
                .into_iter()
                .map(|tool| {
                    if tool == "auto" {
                        DropdownOption::new("auto", t("settings.advanced.typingTool.options.auto"))
                    } else {
                        DropdownOption::new(tool.clone(), tool)
                    }
                })
                .collect();
            rows.push(
                SettingRow::new("typing-tool", t("settings.advanced.typingTool.title"))
                    .description(t("settings.advanced.typingTool.description"))
                    .error(save_error(st, "typing_tool"))
                    .control(
                        Dropdown::new("typing-tool-dropdown")
                            .options(options)
                            .selected(json_str(s.typing_tool))
                            .on_select(|v, _, cx| set("typing_tool", json!(v), cx)),
                    )
                    .into_any_element(),
            );
        }

        rows.push(
            SettingRow::new(
                "clipboard-handling",
                t("settings.advanced.clipboardHandling.title"),
            )
            .description(t("settings.advanced.clipboardHandling.description"))
            .error(save_error(st, "clipboard_handling"))
            .control(
                Dropdown::new("clipboard-handling-dropdown")
                    .options(vec![
                        DropdownOption::new(
                            "dont_modify",
                            t("settings.advanced.clipboardHandling.options.dontModify"),
                        ),
                        DropdownOption::new(
                            "copy_to_clipboard",
                            t("settings.advanced.clipboardHandling.options.copyToClipboard"),
                        ),
                    ])
                    .selected(json_str(s.clipboard_handling))
                    .on_select(|v, _, cx| set("clipboard_handling", json!(v), cx)),
            )
            .into_any_element(),
        );

        let auto_options = auto_submit_options(os)
            .into_iter()
            .map(|o| DropdownOption::new(o.value.clone(), o.label()))
            .collect();
        let enabled = s.auto_submit;
        rows.push(
            SettingRow::new("auto-submit", t("settings.advanced.autoSubmit.title"))
                .description(t(&platform_key("settings.advanced.autoSubmit.description")))
                .error(save_error(st, "auto_submit").or(save_error(st, "auto_submit_key")))
                .control(
                    Dropdown::new("auto-submit-dropdown")
                        .options(auto_options)
                        .selected(Some(auto_submit_value(s.auto_submit, s.auto_submit_key)))
                        .on_select(move |v, _, cx| {
                            let writes = auto_submit_writes(v, enabled);
                            AppState::global(cx)
                                .update(cx, |s, cx| s.update_settings_in_order(writes, cx));
                        }),
                )
                .into_any_element(),
        );
        rows.push(Self::toggle_row(
            st,
            "append-trailing-space",
            "append_trailing_space",
            "settings.debug.appendTrailingSpace.label",
            "settings.debug.appendTrailingSpace.description",
            s.append_trailing_space,
        ));
        group(Some(t("settings.advanced.groups.output")), rows, cx)
    }

    fn custom_words_row(&self, s: &AppSettings, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let normalized = normalize_custom_word(&self.custom_word.read(cx).value());
        let can_add = !normalized.is_empty();
        let error = self
            .word_error
            .clone()
            .or_else(|| save_error(self.state.read(cx), "custom_words"));
        let field = h_flex()
            .gap(dp(6.))
            .child(
                div()
                    .id("custom-words-input")
                    .w(dp(180.))
                    .child(Input::new(&self.custom_word).small())
                    .test_support(),
            )
            .child(
                Button::new("custom-words-add")
                    .outline()
                    .small()
                    .label(t("settings.advanced.customWords.add"))
                    .disabled(!can_add)
                    .on_click(cx.listener(|this, _, window, cx| this.add_word(window, cx))),
            );
        let tokens = (!s.custom_words.is_empty()).then(|| {
            h_flex()
                .id("custom-words-list")
                .w_full()
                .flex_wrap()
                .gap(dp(4.))
                .children(s.custom_words.iter().map(|word| {
                    let w = word.clone();
                    h_flex()
                        .id(SharedString::from(format!("custom-word-{word}")))
                        .flex_none()
                        .h(dp(20.))
                        .pl(dp(6.))
                        .gap(dp(2.))
                        .rounded(design::RADIUS_CHIP)
                        .border_1()
                        .border_color(tone.hairline)
                        .bg(tone.raised)
                        .text_size(dp(12.))
                        .child(word.clone())
                        .child(
                            Button::new(SharedString::from(format!("custom-word-remove-{word}")))
                                .ghost()
                                .xsmall()
                                .icon(icon("x"))
                                .tooltip(t_args(
                                    "settings.advanced.customWords.remove",
                                    &[("word", word)],
                                ))
                                .on_click(move |_, _, cx| Self::remove_word(w.clone(), cx)),
                        )
                        .test_support()
                }))
                .test_support()
        });
        SettingRow::new("custom-words", t("settings.advanced.customWords.title"))
            .description(t("settings.advanced.customWords.description"))
            .error(error)
            .stacked()
            .control(v_flex().w_full().gap(dp(6.)).child(field).children(tokens))
            .into_any_element()
    }

    fn transcription_group(&self, s: &AppSettings, cx: &mut Context<Self>) -> Div {
        let words = self.custom_words_row(s, cx);
        let st = self.state.read(cx);
        let rows = vec![
            Self::toggle_row(
                st,
                "vad",
                "vad_enabled",
                "settings.advanced.voiceActivityDetection.title",
                "settings.advanced.voiceActivityDetection.description",
                s.vad_enabled,
            ),
            Self::toggle_row(
                st,
                "long-form-chunking",
                "long_form_chunking",
                "settings.advanced.longFormChunking.title",
                "settings.advanced.longFormChunking.description",
                s.long_form_chunking,
            ),
            Self::toggle_row(
                st,
                "filler-word-removal",
                "filler_word_removal_enabled",
                "settings.advanced.fillerWordRemoval.title",
                "settings.advanced.fillerWordRemoval.description",
                s.filler_word_removal_enabled,
            ),
            SettingRow::new("chinese-script", t("settings.advanced.chineseScript.title"))
                .description(t("settings.advanced.chineseScript.description"))
                .error(save_error(st, "chinese_script"))
                .control(
                    Dropdown::new("chinese-script-dropdown")
                        .options(vec![
                            DropdownOption::new(
                                "as_transcribed",
                                t("settings.advanced.chineseScript.options.asTranscribed"),
                            ),
                            DropdownOption::new(
                                "simplified",
                                t("settings.advanced.chineseScript.options.simplified"),
                            ),
                            DropdownOption::new(
                                "traditional",
                                t("settings.advanced.chineseScript.options.traditional"),
                            ),
                        ])
                        .selected(json_str(s.chinese_script))
                        .on_select(|v, _, cx| set("chinese_script", json!(v), cx)),
                )
                .into_any_element(),
            words,
        ];
        group(Some(t("settings.advanced.groups.transcription")), rows, cx)
    }

    fn history_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let limit = s.history_limit.to_string();
        let limit_error = self
            .limit_error
            .clone()
            .or_else(|| save_error(st, "history_limit"));
        let rows = vec![
            SettingRow::new("history-limit", t("settings.debug.historyLimit.title"))
                .description(t("settings.debug.historyLimit.description"))
                .error(limit_error)
                .control(
                    h_flex()
                        .gap(dp(6.))
                        .child(
                            div()
                                .id("history-limit-input")
                                .w(dp(72.))
                                .child(Input::new(&self.history_limit).small())
                                .test_support(),
                        )
                        .child(
                            div()
                                .text_size(design::TEXT_BODY)
                                .text_color(design::tone(cx).text_muted)
                                .child(t("settings.debug.historyLimit.entries")),
                        ),
                )
                .into_any_element(),
            SettingRow::new(
                "recording-retention",
                t("settings.debug.recordingRetention.title"),
            )
            .description(t("settings.debug.recordingRetention.description"))
            .error(save_error(st, "recording_retention_period"))
            .control(
                Dropdown::new("recording-retention-dropdown")
                    .options(vec![
                        DropdownOption::new("never", t("settings.debug.recordingRetention.never")),
                        DropdownOption::new(
                            "preserve_limit",
                            t_args(
                                "settings.debug.recordingRetention.preserveLimit",
                                &[("count", &limit)],
                            ),
                        ),
                        DropdownOption::new("days3", t("settings.debug.recordingRetention.days3")),
                        DropdownOption::new(
                            "weeks2",
                            t("settings.debug.recordingRetention.weeks2"),
                        ),
                        DropdownOption::new(
                            "months3",
                            t("settings.debug.recordingRetention.months3"),
                        ),
                    ])
                    .placeholder(t("settings.debug.recordingRetention.placeholder"))
                    .selected(json_str(s.recording_retention_period))
                    .on_select(|v, _, cx| set("recording_retention_period", json!(v), cx)),
            )
            .into_any_element(),
        ];
        group(Some(t("settings.advanced.groups.history")), rows, cx)
    }

    fn experimental_group(&self, s: &AppSettings, cx: &mut Context<Self>) -> Div {
        let current_impl = match s.keyboard_implementation {
            KeyboardImplementation::Tauri => "tauri",
            KeyboardImplementation::NativeKeys => "native_keys",
        };
        let acc_options = self
            .accelerators
            .as_ref()
            .map(|a| {
                accelerator_options(
                    a,
                    &guidy_i18n::t("settings.advanced.acceleration.gpuDevice.auto"),
                )
            })
            .unwrap_or_default();
        let current_acc =
            encode_accelerator(s.transcribe_accelerator, s.transcribe_gpu_device.as_deref());
        let displayed = displayed_accelerator(&current_acc, &acc_options);
        let page = cx.entity().downgrade();
        let st = self.state.read(cx);
        let switching = st.settings.is_updating("keyboard_implementation");
        let rows = vec![
            Self::toggle_row(
                st,
                "post-process-enabled",
                "post_process_enabled",
                "settings.debug.postProcessingToggle.label",
                "settings.debug.postProcessingToggle.description",
                s.post_process_enabled,
            ),
            SettingRow::new(
                "keyboard-implementation",
                t("settings.debug.keyboardImplementation.title"),
            )
            .description(t("settings.debug.keyboardImplementation.description"))
            .error(self.keyboard_impl_error.clone())
            .control(
                Dropdown::new("keyboard-implementation-dropdown")
                    .options(vec![
                        DropdownOption::new(
                            "tauri",
                            t("settings.debug.keyboardImplementation.options.system"),
                        ),
                        DropdownOption::new(
                            "native_keys",
                            t("settings.debug.keyboardImplementation.options.nativeKeys"),
                        ),
                    ])
                    .selected(Some(current_impl))
                    // Switching re-registers every shortcut: one at a time.
                    .loading(switching)
                    .on_select(move |v, _, cx| {
                        if v != current_impl {
                            let v = v.to_string();
                            page.update(cx, |this, cx| this.change_keyboard_implementation(v, cx))
                                .ok();
                        }
                    }),
            )
            .into_any_element(),
            SettingRow::new(
                "acceleration",
                t("settings.advanced.acceleration.transcribe.title"),
            )
            .description(t("settings.advanced.acceleration.transcribe.description"))
            .error(
                save_error(st, "transcribe_accelerator")
                    .or(save_error(st, "transcribe_gpu_device")),
            )
            .control(
                Dropdown::new("acceleration-dropdown")
                    .options(
                        acc_options
                            .into_iter()
                            .map(|(v, l)| DropdownOption::new(v, l))
                            .collect(),
                    )
                    .selected(displayed)
                    .loading(self.accelerators.is_none())
                    .on_select(|v, _, cx| {
                        let (accelerator, device) = decode_accelerator(v);
                        // Device first so `gpu + null` isn't normalized to Auto.
                        let writes = vec![
                            ("transcribe_gpu_device", json!(device)),
                            ("transcribe_accelerator", json!(accelerator)),
                        ];
                        AppState::global(cx)
                            .update(cx, |s, cx| s.update_settings_in_order(writes, cx));
                    }),
            )
            .into_any_element(),
            Self::toggle_row(
                st,
                "lazy-stream-close",
                "lazy_stream_close",
                "settings.advanced.lazyStreamClose.label",
                "settings.advanced.lazyStreamClose.description",
                s.lazy_stream_close,
            ),
            SettingRow::new("vad-backend", t("settings.advanced.vadBackend.title"))
                .description(t("settings.advanced.vadBackend.description"))
                .error(save_error(st, "vad_backend"))
                .control(
                    Dropdown::new("vad-backend-dropdown")
                        .options(vec![
                            DropdownOption::new(
                                "silero",
                                t("settings.advanced.vadBackend.options.silero"),
                            ),
                            DropdownOption::new(
                                "earshot",
                                t("settings.advanced.vadBackend.options.earshot"),
                            ),
                        ])
                        .selected(json_str(s.vad_backend))
                        .on_select(|v, _, cx| set("vad_backend", json!(v), cx)),
                )
                .into_any_element(),
        ];
        group(Some(t("settings.advanced.groups.experimental")), rows, cx)
    }
}

/// Unsaved history entries (the ones a lower limit can delete), counted by
/// walking the pages.
fn count_unsaved(backend: &dyn crate::backend::Backend) -> Result<usize, String> {
    let mut cursor = None;
    let mut unsaved = 0;
    loop {
        let page = backend.get_history_entries(cursor, 500)?;
        unsaved += page.entries.iter().filter(|e| !e.saved).count();
        match (page.has_more, page.entries.last()) {
            (true, Some(last)) => cursor = Some(last.id),
            _ => return Ok(unsaved),
        }
    }
}

impl Render for AdvancedPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("advanced-scroll", self.render_body(window, cx))
    }
}

impl crate::ui::pages::Page for AdvancedPage {}

/// `key`, or its macOS wording (`<key>Mac`: the menu bar rather than the
/// system tray, no Linux/Windows notes) when there is one.
fn platform_key(key: &str) -> String {
    platform_key_for(key, cfg!(target_os = "macos"))
}

fn platform_key_for(key: &str, mac: bool) -> String {
    let mac_key = format!("{key}Mac");
    if mac && guidy_i18n::exists(&mac_key) {
        mac_key
    } else {
        key.to_string()
    }
}

impl AdvancedPage {
    fn render_body(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return page().into_any_element();
        };
        page()
            .id("page-advanced")
            .child(self.app_group(&s, cx))
            .child(self.overlay_group(&s, cx))
            .child(self.output_group(&s, cx))
            .child(self.transcription_group(&s, cx))
            .child(self.history_group(&s, cx))
            .when(s.experimental_enabled, |p| {
                p.child(self.experimental_group(&s, cx))
            })
            .test_support()
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::platform_key_for;

    #[test]
    fn macos_reads_its_own_wording_without_linux_or_windows_notes() {
        for key in [
            "settings.advanced.startHidden.description",
            "settings.advanced.showTrayIcon.description",
            "settings.advanced.overlay.style.description",
            "settings.advanced.autoSubmit.description",
        ] {
            let mac = guidy_i18n::t(&platform_key_for(key, true));
            assert!(!mac.contains("Linux") && !mac.contains("Windows"), "{mac}");
            assert!(!mac.contains("system tray"), "{mac}");
            assert_eq!(platform_key_for(key, false), key);
        }
        // Keys without a macOS variant stay as they are.
        assert_eq!(
            platform_key_for("settings.advanced.autostart.description", true),
            "settings.advanced.autostart.description"
        );
    }
}
