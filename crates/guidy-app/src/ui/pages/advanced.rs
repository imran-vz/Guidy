//! Advanced page: app behavior, output, transcription, history, experimental.

use crate::logic::flows::{
    accelerator_options, add_custom_word, auto_submit_options, auto_submit_value,
    auto_submit_writes, decode_accelerator, displayed_accelerator, encode_accelerator,
    model_unload_options, normalize_custom_word, paste_method_options, AddWordOutcome,
    MAX_CUSTOM_WORD_LEN,
};
use crate::logic::keyboard::OsType;
use crate::logic::settings_store::ToastKind;
use crate::state::AppState;
use crate::ui::kit::{group, icon, page, t, t_args, toggle, Dropdown, DropdownOption, SettingRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::transcription::AvailableAccelerators;
use guidy_core::settings::{AppSettings, KeyboardImplementation, OverlayStyle, PasteMethod};
use serde_json::json;

pub struct AdvancedPage {
    state: Entity<AppState>,
    custom_word: Entity<InputState>,
    history_limit: Entity<InputState>,
    script_path: Entity<InputState>,
    accelerators: Option<AvailableAccelerators>,
    typing_tools: Option<Vec<String>>,
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

impl AdvancedPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let settings = state.read(cx).app_settings().cloned();
        let custom_word = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("settings.advanced.customWords.placeholder"))
        });
        let history_limit = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    settings
                        .as_ref()
                        .map(|s| s.history_limit)
                        .unwrap_or(5)
                        .to_string(),
                )
                .min(0.)
                .max(1000.)
                .step(1.)
        });
        let script_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("settings.advanced.pasteMethod.externalScriptPlaceholder"))
                .default_value(
                    settings
                        .as_ref()
                        .and_then(|s| s.external_script_path.clone())
                        .unwrap_or_default(),
                )
        });
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &custom_word,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.add_word(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe(&history_limit, |this, input, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    if let Ok(v) = input.read(cx).value().trim().parse::<i64>() {
                        if (0..=1000).contains(&v) {
                            let current = this
                                .state
                                .read(cx)
                                .app_settings()
                                .map(|s| s.history_limit as i64);
                            if current != Some(v) {
                                set("history_limit", json!(v), cx);
                            }
                        }
                    }
                }
            }),
            cx.subscribe(&script_path, |_, input, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    let v = input.read(cx).value().to_string();
                    set("external_script_path", json!(v), cx);
                }
            }),
        ];
        let mut this = Self {
            state,
            custom_word,
            history_limit,
            script_path,
            accelerators: None,
            typing_tools: None,
            _subs: subs,
        };
        this.load_options(cx);
        this
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
                set("custom_words", json!(next), cx);
                self.custom_word
                    .update(cx, |s, cx| s.set_value("", window, cx));
            }
            AddWordOutcome::Duplicate(word) => {
                self.state.update(cx, |s, cx| {
                    s.toast_kind(
                        ToastKind::Error,
                        t_args(
                            "settings.advanced.customWords.duplicate",
                            &[("word", &word)],
                        ),
                        cx,
                    )
                });
            }
            AddWordOutcome::Invalid => {}
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

    fn change_keyboard_implementation(value: String, cx: &mut App) {
        let state = AppState::global(cx);
        state.update(cx, |s, cx| {
            s.settings.updating.insert("keyboard_implementation".into());
            cx.notify();
            s.call(
                cx,
                move |b| b.change_keyboard_implementation(&value),
                |s, r, cx| {
                    s.settings.updating.remove("keyboard_implementation");
                    match r {
                        Ok(result) => {
                            if !result.reset_bindings.is_empty() {
                                s.toast_kind(
                                    ToastKind::Warning,
                                    t("settings.debug.keyboardImplementation.bindingsReset"),
                                    cx,
                                );
                            }
                            s.refresh_settings(cx);
                        }
                        Err(e) => s.toast_error(e, cx),
                    }
                },
            );
        });
    }

    fn app_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let up = |k: &str| st.settings.is_updating(k);
        let mut rows = vec![
            SettingRow::new("start-hidden", t("settings.advanced.startHidden.label"))
                .description(t("settings.advanced.startHidden.description"))
                .control(toggle(
                    "start-hidden-toggle",
                    s.start_hidden,
                    up("start_hidden"),
                    |v, _, cx| set("start_hidden", json!(v), cx),
                ))
                .into_any_element(),
            SettingRow::new("autostart", t("settings.advanced.autostart.label"))
                .description(t("settings.advanced.autostart.description"))
                .control(toggle(
                    "autostart-toggle",
                    s.autostart_enabled,
                    up("autostart_enabled"),
                    |v, _, cx| set("autostart_enabled", json!(v), cx),
                ))
                .into_any_element(),
            SettingRow::new("show-tray-icon", t("settings.advanced.showTrayIcon.label"))
                .description(t("settings.advanced.showTrayIcon.description"))
                .control(toggle(
                    "show-tray-icon-toggle",
                    s.show_tray_icon,
                    up("show_tray_icon"),
                    |v, _, cx| set("show_tray_icon", json!(v), cx),
                ))
                .into_any_element(),
            SettingRow::new("overlay-style", t("settings.advanced.overlay.style.title"))
                .description(t("settings.advanced.overlay.style.description"))
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
                        .disabled(up("overlay_style"))
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
                        .disabled(up("overlay_position"))
                        .on_select(|v, _, cx| set("overlay_position", json!(v), cx)),
                )
                .into_any_element(),
            );
        }
        let unload_options = model_unload_options(s.debug_mode)
            .into_iter()
            .map(|o| DropdownOption::new(o.value.clone(), o.label()))
            .collect();
        rows.push(
            SettingRow::new("model-unload", t("settings.advanced.modelUnload.title"))
                .description(t("settings.advanced.modelUnload.description"))
                .control(
                    Dropdown::new("model-unload-dropdown")
                        .options(unload_options)
                        .selected(json_str(s.model_unload_timeout))
                        .disabled(up("model_unload_timeout"))
                        .on_select(|v, _, cx| set("model_unload_timeout", json!(v), cx)),
                )
                .into_any_element(),
        );
        rows.push(
            SettingRow::new(
                "experimental",
                t("settings.advanced.experimentalToggle.label"),
            )
            .description(t("settings.advanced.experimentalToggle.description"))
            .control(toggle(
                "experimental-toggle",
                s.experimental_enabled,
                up("experimental_enabled"),
                |v, _, cx| set("experimental_enabled", json!(v), cx),
            ))
            .into_any_element(),
        );
        group(Some(t("settings.advanced.groups.app")), rows, cx)
    }

    fn output_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let up = |k: &str| st.settings.is_updating(k);
        let os = OsType::current();
        let paste_options = paste_method_options(os, s.paste_method)
            .into_iter()
            .map(|o| DropdownOption::new(o.value.clone(), o.label()).disabled(o.disabled))
            .collect();
        let mut paste_control = gpui_kit::component::v_flex().gap_2().items_end().child(
            Dropdown::new("paste-method-dropdown")
                .options(paste_options)
                .selected(json_str(s.paste_method))
                .disabled(up("paste_method"))
                .on_select(|v, _, cx| set("paste_method", json!(v), cx)),
        );
        if s.paste_method == PasteMethod::ExternalScript {
            paste_control = paste_control.child(
                div().w(px(200.)).child(
                    Input::new(&self.script_path)
                        .small()
                        .disabled(up("external_script_path")),
                ),
            );
        }
        let mut rows =
            vec![
                SettingRow::new("paste-method", t("settings.advanced.pasteMethod.title"))
                    .description(t("settings.advanced.pasteMethod.description"))
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
                    .control(
                        Dropdown::new("typing-tool-dropdown")
                            .options(options)
                            .selected(json_str(s.typing_tool))
                            .disabled(up("typing_tool"))
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
                    .disabled(up("clipboard_handling"))
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
                .description(t("settings.advanced.autoSubmit.description"))
                .control(
                    Dropdown::new("auto-submit-dropdown")
                        .options(auto_options)
                        .selected(Some(auto_submit_value(s.auto_submit, s.auto_submit_key)))
                        .disabled(up("auto_submit") || up("auto_submit_key"))
                        .on_select(move |v, _, cx| {
                            let writes = auto_submit_writes(v, enabled);
                            AppState::global(cx)
                                .update(cx, |s, cx| s.update_settings_in_order(writes, cx));
                        }),
                )
                .into_any_element(),
        );
        group(Some(t("settings.advanced.groups.output")), rows, cx)
    }

    fn transcription_group(&self, s: &AppSettings, cx: &mut Context<Self>) -> Div {
        let st = self.state.read(cx);
        let up = |k: &str| st.settings.is_updating(k);
        let theme = cx.theme();
        let normalized = normalize_custom_word(&self.custom_word.read(cx).value());
        let can_add = !normalized.is_empty()
            && normalized.chars().count() <= MAX_CUSTOM_WORD_LEN
            && !up("custom_words");
        let words_updating = up("custom_words");
        let mut rows = vec![
            SettingRow::new("vad", t("settings.advanced.voiceActivityDetection.title"))
                .description(t("settings.advanced.voiceActivityDetection.description"))
                .control(toggle(
                    "vad-toggle",
                    s.vad_enabled,
                    up("vad_enabled"),
                    |v, _, cx| set("vad_enabled", json!(v), cx),
                ))
                .into_any_element(),
            SettingRow::new(
                "long-form-chunking",
                t("settings.advanced.longFormChunking.title"),
            )
            .description(t("settings.advanced.longFormChunking.description"))
            .control(toggle(
                "long-form-toggle",
                s.long_form_chunking,
                up("long_form_chunking"),
                |v, _, cx| set("long_form_chunking", json!(v), cx),
            ))
            .into_any_element(),
            SettingRow::new(
                "filler-word-removal",
                t("settings.advanced.fillerWordRemoval.title"),
            )
            .description(t("settings.advanced.fillerWordRemoval.description"))
            .control(toggle(
                "filler-toggle",
                s.filler_word_removal_enabled,
                up("filler_word_removal_enabled"),
                |v, _, cx| set("filler_word_removal_enabled", json!(v), cx),
            ))
            .into_any_element(),
            SettingRow::new("chinese-script", t("settings.advanced.chineseScript.title"))
                .description(t("settings.advanced.chineseScript.description"))
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
                        .disabled(up("chinese_script"))
                        .on_select(|v, _, cx| set("chinese_script", json!(v), cx)),
                )
                .into_any_element(),
            SettingRow::new("custom-words", t("settings.advanced.customWords.title"))
                .description(t("settings.advanced.customWords.description"))
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            div().w(px(160.)).child(
                                Input::new(&self.custom_word)
                                    .small()
                                    .disabled(words_updating),
                            ),
                        )
                        .child(
                            Button::new("custom-words-add")
                                .primary()
                                .small()
                                .label(t("settings.advanced.customWords.add"))
                                .disabled(!can_add)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_word(window, cx)),
                                ),
                        ),
                )
                .into_any_element(),
        ];
        if !s.custom_words.is_empty() {
            rows.push(
                h_flex()
                    .id("custom-words-list")
                    .flex_wrap()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .children(s.custom_words.iter().enumerate().map(|(i, word)| {
                        let w = word.clone();
                        Button::new(("custom-word", i))
                            .outline()
                            .xsmall()
                            .label(word.clone())
                            .icon(icon("x"))
                            .tooltip(t_args(
                                "settings.advanced.customWords.remove",
                                &[("word", word)],
                            ))
                            .disabled(words_updating)
                            .on_click(move |_, _, cx| Self::remove_word(w.clone(), cx))
                    }))
                    .text_color(theme.foreground)
                    .test_support()
                    .into_any_element(),
            );
        }
        rows.push(
            SettingRow::new(
                "append-trailing-space",
                t("settings.debug.appendTrailingSpace.label"),
            )
            .description(t("settings.debug.appendTrailingSpace.description"))
            .control(toggle(
                "trailing-space-toggle",
                s.append_trailing_space,
                up("append_trailing_space"),
                |v, _, cx| set("append_trailing_space", json!(v), cx),
            ))
            .into_any_element(),
        );
        group(Some(t("settings.advanced.groups.transcription")), rows, cx)
    }

    fn history_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let up = |k: &str| st.settings.is_updating(k);
        let limit = s.history_limit.to_string();
        let rows = vec![
            SettingRow::new("history-limit", t("settings.debug.historyLimit.title"))
                .description(t("settings.debug.historyLimit.description"))
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            div().w(px(110.)).child(
                                gpui_kit::component::input::NumberInput::new(&self.history_limit)
                                    .small()
                                    .disabled(up("history_limit")),
                            ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .child(t("settings.debug.historyLimit.entries")),
                        ),
                )
                .into_any_element(),
            SettingRow::new(
                "recording-retention",
                t("settings.debug.recordingRetention.title"),
            )
            .description(t("settings.debug.recordingRetention.description"))
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
                    .disabled(up("recording_retention_period"))
                    .on_select(|v, _, cx| set("recording_retention_period", json!(v), cx)),
            )
            .into_any_element(),
        ];
        group(Some(t("settings.advanced.groups.history")), rows, cx)
    }

    fn experimental_group(&self, s: &AppSettings, cx: &App) -> Div {
        let st = self.state.read(cx);
        let up = |k: &str| st.settings.is_updating(k);
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
        let rows = vec![
            SettingRow::new(
                "post-process-enabled",
                t("settings.debug.postProcessingToggle.label"),
            )
            .description(t("settings.debug.postProcessingToggle.description"))
            .control(toggle(
                "post-process-toggle",
                s.post_process_enabled,
                up("post_process_enabled"),
                |v, _, cx| set("post_process_enabled", json!(v), cx),
            ))
            .into_any_element(),
            SettingRow::new(
                "keyboard-implementation",
                t("settings.debug.keyboardImplementation.title"),
            )
            .description(t("settings.debug.keyboardImplementation.description"))
            .control(
                Dropdown::new("keyboard-implementation-dropdown")
                    .options(vec![
                        DropdownOption::new("tauri", "System Global Shortcut"),
                        DropdownOption::new("native_keys", "Native Key Listener"),
                    ])
                    .selected(Some(current_impl))
                    .disabled(up("keyboard_implementation"))
                    .on_select(move |v, _, cx| {
                        if v != current_impl {
                            Self::change_keyboard_implementation(v.to_string(), cx)
                        }
                    }),
            )
            .into_any_element(),
            SettingRow::new(
                "acceleration",
                t("settings.advanced.acceleration.transcribe.title"),
            )
            .description(t("settings.advanced.acceleration.transcribe.description"))
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
                    .disabled(up("transcribe_accelerator") || up("transcribe_gpu_device"))
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
            SettingRow::new(
                "lazy-stream-close",
                t("settings.advanced.lazyStreamClose.label"),
            )
            .description(t("settings.advanced.lazyStreamClose.description"))
            .control(toggle(
                "lazy-stream-toggle",
                s.lazy_stream_close,
                up("lazy_stream_close"),
                |v, _, cx| set("lazy_stream_close", json!(v), cx),
            ))
            .into_any_element(),
            SettingRow::new("vad-backend", t("settings.advanced.vadBackend.title"))
                .description(t("settings.advanced.vadBackend.description"))
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
                        .disabled(up("vad_backend"))
                        .on_select(|v, _, cx| set("vad_backend", json!(v), cx)),
                )
                .into_any_element(),
        ];
        group(Some(t("settings.advanced.groups.experimental")), rows, cx)
    }
}

impl Render for AdvancedPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return page().into_any_element();
        };
        // Keep the number input in sync with external changes.
        let limit = s.history_limit.to_string();
        if self.history_limit.read(cx).value() != limit.as_str()
            && !self.history_limit.focus_handle(cx).is_focused(window)
        {
            self.history_limit
                .update(cx, |st, cx| st.set_value(limit, window, cx));
        }
        page()
            .id("page-advanced")
            .child(self.app_group(&s, cx))
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
