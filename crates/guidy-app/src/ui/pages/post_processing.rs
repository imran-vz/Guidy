//! Post-processing page: hotkey, LLM provider/API, prompt templates.

use crate::logic::flows::{
    post_process_model_options, prompt_is_dirty, should_auto_fetch_models, APPLE_PROVIDER_ID,
    CUSTOM_PROVIDER_ID,
};
use crate::state::AppState;
use crate::ui::kit::{group, icon, page, t, Dropdown, DropdownOption, SettingRow};
use crate::ui::shortcut_input::ShortcutInput;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::settings::{AppSettings, PostProcessApiFormat};

pub struct PostProcessingPage {
    state: Entity<AppState>,
    shortcut: Entity<ShortcutInput>,
    base_url: Entity<InputState>,
    api_key: Entity<InputState>,
    model: Entity<InputState>,
    prompt_name: Entity<InputState>,
    prompt_text: Entity<TextareaState>,
    creating: bool,
    apple_unavailable: bool,
    /// (provider id, prompt id) the inputs were last synced for.
    synced: (String, Option<String>),
    _subs: Vec<Subscription>,
}

fn provider_of(s: &AppSettings) -> (String, Option<guidy_core::settings::PostProcessProvider>) {
    let id = if s.post_process_provider_id.is_empty() {
        s.post_process_providers
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "openai".into())
    } else {
        s.post_process_provider_id.clone()
    };
    let provider = s
        .post_process_providers
        .iter()
        .find(|p| p.id == id)
        .or(s.post_process_providers.first())
        .cloned();
    (id, provider)
}

impl PostProcessingPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let base_url = cx.new(|cx| InputState::new(window, cx));
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("settings.postProcessing.api.apiKey.placeholder"))
        });
        let model = cx.new(|cx| InputState::new(window, cx));
        let prompt_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("settings.postProcessing.prompts.promptLabelPlaceholder"))
        });
        let prompt_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 12)
                .placeholder(t(
                    "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                ))
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_inputs(false, window, cx);
                cx.notify();
            }),
            // Base URL and API key save on blur (and Enter).
            cx.subscribe_in(&base_url, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save_base_url(v, cx);
                }
            }),
            cx.subscribe_in(&api_key, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save(
                        "api_key",
                        v,
                        |s, id| s.post_process_api_keys.get(id).cloned().unwrap_or_default(),
                        cx,
                    );
                }
            }),
            cx.subscribe_in(&model, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save(
                        "model",
                        v,
                        |s, id| s.post_process_models.get(id).cloned().unwrap_or_default(),
                        cx,
                    );
                }
            }),
            cx.subscribe(&prompt_name, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
            cx.subscribe(&prompt_text, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            state,
            shortcut: cx.new(|cx| ShortcutInput::new("transcribe_with_post_process", window, cx)),
            base_url,
            api_key,
            model,
            prompt_name,
            prompt_text,
            creating: false,
            apple_unavailable: false,
            synced: (String::new(), None),
            _subs: subs,
        };
        this.sync_inputs(true, window, cx);
        this
    }

    /// Mirror stored values into the text inputs when the provider or prompt
    /// changes (never while the user is typing in a focused field).
    fn sync_inputs(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let (pid, provider) = provider_of(&s);
        let prompt_id = s.post_process_selected_prompt_id.clone();
        let provider_changed = force || self.synced.0 != pid;
        let set_if = |input: &Entity<InputState>,
                      value: String,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            if !input.focus_handle(cx).is_focused(window)
                && input.read(cx).value() != value.as_str()
            {
                input.update(cx, |st, cx| st.set_value(value, window, cx));
            }
        };
        if provider_changed || !self.base_url.focus_handle(cx).is_focused(window) {
            let base = provider
                .as_ref()
                .map(|p| p.base_url.clone())
                .unwrap_or_default();
            set_if(&self.base_url, base, window, cx);
            set_if(
                &self.api_key,
                s.post_process_api_keys
                    .get(&pid)
                    .cloned()
                    .unwrap_or_default(),
                window,
                cx,
            );
            set_if(
                &self.model,
                s.post_process_models.get(&pid).cloned().unwrap_or_default(),
                window,
                cx,
            );
            let anthropic = provider
                .as_ref()
                .map(|p| p.api_format == PostProcessApiFormat::Anthropic)
                .unwrap_or(false);
            let placeholder = if anthropic {
                t("settings.postProcessing.api.baseUrl.placeholderAnthropic")
            } else {
                t("settings.postProcessing.api.baseUrl.placeholder")
            };
            self.base_url
                .update(cx, |st, cx| st.set_placeholder(placeholder, window, cx));
        }
        if !self.creating && (force || self.synced.1 != prompt_id || provider_changed) {
            let selected = prompt_id
                .as_ref()
                .and_then(|id| s.post_process_prompts.iter().find(|p| &p.id == id));
            let (name, text) = selected
                .map(|p| (p.name.clone(), p.prompt.clone()))
                .unwrap_or_default();
            self.prompt_name
                .update(cx, |st, cx| st.set_value(name, window, cx));
            self.prompt_text
                .update(cx, |st, cx| st.set_value(text, window, cx));
        }
        self.synced = (pid, prompt_id);
    }

    #[cfg(test)]
    pub fn set_prompt_draft(
        &mut self,
        name: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, text) = (name.to_string(), text.to_string());
        self.prompt_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.prompt_text
            .update(cx, |s, cx| s.set_value(text, window, cx));
        cx.notify();
    }

    fn save(
        &mut self,
        what: &'static str,
        value: String,
        current: fn(&AppSettings, &str) -> String,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let (pid, _) = provider_of(&s);
        if value == current(&s, &pid) {
            return;
        }
        self.state
            .update(cx, |st, cx| st.update_post_process(what, &pid, value, cx));
    }

    fn save_base_url(&mut self, value: String, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let (pid, provider) = provider_of(&s);
        if pid != CUSTOM_PROVIDER_ID
            || value.is_empty()
            || provider.is_some_and(|p| p.base_url == value)
        {
            return;
        }
        self.state.update(cx, |st, cx| {
            st.update_post_process("base_url", &pid, value, cx)
        });
    }

    fn select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        self.apple_unavailable = false;
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let (current, _) = provider_of(&s);
        if id == current {
            return;
        }
        if id == APPLE_PROVIDER_ID {
            // Keep the selection even when unavailable; just explain.
            self.apple_unavailable = !self
                .state
                .read(cx)
                .backend
                .check_apple_intelligence_available();
        }
        let base = s
            .post_process_providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.base_url.clone())
            .unwrap_or_default();
        let key = s
            .post_process_api_keys
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let fetch = should_auto_fetch_models(&id, &base, &key);
        self.state.update(cx, |st, cx| {
            st.set_post_process_provider(&id, cx);
            if fetch {
                st.fetch_post_process_models(&id, cx);
            }
        });
        cx.notify();
    }

    fn create_prompt(&mut self, cx: &mut Context<Self>) {
        let name = self.prompt_name.read(cx).value().trim().to_string();
        let text = self.prompt_text.read(cx).value().trim().to_string();
        if name.is_empty() || text.is_empty() {
            return;
        }
        let weak = cx.entity().downgrade();
        self.state.update(cx, |s, cx| {
            s.call(
                cx,
                move |b| b.add_post_process_prompt(&name, &text),
                move |s, r, cx| match r {
                    Ok(prompt) => {
                        // Select only after the write lands, then refresh: a
                        // refresh racing the write would restore the old id
                        // (the setter emits no settings-changed).
                        s.update_setting_then(
                            "post_process_selected_prompt_id",
                            serde_json::json!(prompt.id),
                            cx,
                            |s, _, cx| s.refresh_settings(cx),
                        );
                        weak.update(cx, |this, cx| {
                            this.creating = false;
                            cx.notify();
                        })
                        .ok();
                    }
                    Err(e) => log::error!("Failed to create prompt: {e}"),
                },
            );
        });
    }

    fn update_prompt(&mut self, id: String, cx: &mut Context<Self>) {
        let name = self.prompt_name.read(cx).value().trim().to_string();
        let text = self.prompt_text.read(cx).value().trim().to_string();
        if name.is_empty() || text.is_empty() {
            return;
        }
        self.state.update(cx, |s, cx| {
            s.call(
                cx,
                move |b| b.update_post_process_prompt(&id, &name, &text),
                |s, r, cx| {
                    if let Err(e) = r {
                        log::error!("Failed to update prompt: {e}");
                    }
                    s.refresh_settings(cx);
                },
            );
        });
    }

    fn delete_prompt(&mut self, id: String, cx: &mut Context<Self>) {
        self.creating = false;
        self.state.update(cx, |s, cx| {
            s.call(
                cx,
                move |b| b.delete_post_process_prompt(&id),
                |s, r, cx| {
                    if let Err(e) = r {
                        log::error!("Failed to delete prompt: {e}");
                    }
                    s.refresh_settings(cx);
                },
            );
        });
    }

    fn api_rows(&self, s: &AppSettings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let st = self.state.read(cx);
        let theme = cx.theme().clone();
        let (pid, provider) = provider_of(s);
        let is_apple = pid == APPLE_PROVIDER_ID;
        let is_custom = pid == CUSTOM_PROVIDER_ID;
        let up = |k: String| st.settings.is_updating(&k);
        let mut rows = vec![SettingRow::new(
            "pp-provider",
            t("settings.postProcessing.api.provider.title"),
        )
        .description(t("settings.postProcessing.api.provider.description"))
        .control(
            Dropdown::new("pp-provider-dropdown")
                .options(
                    s.post_process_providers
                        .iter()
                        .map(|p| DropdownOption::new(p.id.clone(), p.label.clone()))
                        .collect(),
                )
                .selected(Some(pid.clone()))
                .disabled(up("post_process_provider_id".into()))
                .on_select({
                    let weak = cx.entity().downgrade();
                    move |v, _, cx| {
                        let v = v.to_string();
                        weak.update(cx, |this, cx| this.select_provider(v, cx)).ok();
                    }
                }),
        )
        .into_any_element()];

        if is_apple {
            if self.apple_unavailable {
                rows.push(
                    div()
                        .id("apple-unavailable")
                        .m_3()
                        .px_3()
                        .py_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.danger.opacity(0.4))
                        .bg(theme.danger.opacity(0.1))
                        .text_sm()
                        .child(t(
                            "settings.postProcessing.api.appleIntelligence.unavailable",
                        ))
                        .into_any_element(),
                );
            }
            return rows;
        }

        if is_custom {
            let fmt = provider.as_ref().map(|p| p.api_format).unwrap_or_default();
            let fmt_str = if fmt == PostProcessApiFormat::Anthropic {
                "anthropic"
            } else {
                "openai"
            };
            let pid2 = pid.clone();
            rows.push(
                SettingRow::new(
                    "pp-api-format",
                    t("settings.postProcessing.api.apiFormat.title"),
                )
                .description(t("settings.postProcessing.api.apiFormat.description"))
                .control(
                    Dropdown::new("pp-api-format-dropdown")
                        .options(vec![
                            DropdownOption::new(
                                "openai",
                                t("settings.postProcessing.api.apiFormat.options.openai"),
                            ),
                            DropdownOption::new(
                                "anthropic",
                                t("settings.postProcessing.api.apiFormat.options.anthropic"),
                            ),
                        ])
                        .selected(Some(fmt_str))
                        .menu_width(px(300.))
                        .disabled(up(format!("post_process_api_format:{pid}")))
                        .on_select(move |v, _, cx| {
                            if v != fmt_str {
                                let (pid, v) = (pid2.clone(), v.to_string());
                                AppState::global(cx).update(cx, |s, cx| {
                                    s.update_post_process("api_format", &pid, v, cx)
                                });
                            }
                        }),
                )
                .into_any_element(),
            );
            rows.push(
                SettingRow::new(
                    "pp-base-url",
                    t("settings.postProcessing.api.baseUrl.title"),
                )
                .description(t("settings.postProcessing.api.baseUrl.description"))
                .control(
                    div().w(px(320.)).child(
                        Input::new(&self.base_url)
                            .small()
                            .disabled(up(format!("post_process_base_url:{pid}"))),
                    ),
                )
                .into_any_element(),
            );
        }
        rows.push(
            SettingRow::new("pp-api-key", t("settings.postProcessing.api.apiKey.title"))
                .description(t("settings.postProcessing.api.apiKey.description"))
                .control(
                    div().w(px(320.)).child(
                        Input::new(&self.api_key)
                            .small()
                            .mask_toggle()
                            .disabled(up(format!("post_process_api_key:{pid}"))),
                    ),
                )
                .into_any_element(),
        );

        let current_model = s.post_process_models.get(&pid).cloned().unwrap_or_default();
        let fetched = st.pp_model_options.get(&pid).cloned().unwrap_or_default();
        let options = post_process_model_options(&fetched, &current_model);
        let fetching = up(format!("post_process_models_fetch:{pid}"));
        let placeholder = if options.is_empty() {
            t("settings.postProcessing.api.model.placeholderNoOptions")
        } else {
            t("settings.postProcessing.api.model.placeholderWithOptions")
        };
        let pid_sel = pid.clone();
        let pid_ref = pid.clone();
        rows.push(
            SettingRow::new("pp-model", t("settings.postProcessing.api.model.title"))
                .description(if is_custom {
                    t("settings.postProcessing.api.model.descriptionCustom")
                } else {
                    t("settings.postProcessing.api.model.descriptionDefault")
                })
                .stacked()
                .control(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div().flex_1().child(
                                Input::new(&self.model)
                                    .small()
                                    .disabled(up(format!("post_process_model:{pid}"))),
                            ),
                        )
                        .child(
                            Dropdown::new("pp-model-options")
                                .options(
                                    options
                                        .into_iter()
                                        .map(|m| DropdownOption::new(m.clone(), m))
                                        .collect(),
                                )
                                .selected(Some(current_model.clone()))
                                .placeholder(placeholder)
                                .menu_width(px(320.))
                                .loading(fetching)
                                .disabled(up(format!("post_process_model:{pid}")))
                                .on_select(move |v, _, cx| {
                                    let (pid, v) = (pid_sel.clone(), v.trim().to_string());
                                    AppState::global(cx).update(cx, |s, cx| {
                                        s.update_post_process("model", &pid, v, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("pp-refresh-models")
                                .ghost()
                                .small()
                                .icon(icon("refresh-ccw"))
                                .tooltip(t("settings.postProcessing.api.model.refreshModels"))
                                .loading(fetching)
                                .disabled(fetching)
                                .on_click(move |_, _, cx| {
                                    let pid = pid_ref.clone();
                                    AppState::global(cx)
                                        .update(cx, |s, cx| s.fetch_post_process_models(&pid, cx));
                                }),
                        ),
                )
                .into_any_element(),
        );
        rows
    }

    fn prompt_rows(&self, s: &AppSettings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let st = self.state.read(cx);
        let theme = cx.theme().clone();
        let prompts = s.post_process_prompts.clone();
        let selected_id = s.post_process_selected_prompt_id.clone();
        let selected = selected_id
            .as_ref()
            .and_then(|id| prompts.iter().find(|p| &p.id == id).cloned());
        let draft_name = self.prompt_name.read(cx).value().to_string();
        let draft_text = self.prompt_text.read(cx).value().to_string();
        let dirty = selected
            .as_ref()
            .is_some_and(|p| prompt_is_dirty(&p.name, &p.prompt, &draft_name, &draft_text));
        let complete = !draft_name.trim().is_empty() && !draft_text.trim().is_empty();
        let tip = guidy_i18n::t("settings.postProcessing.prompts.promptTip")
            .replace("<code>", "")
            .replace("</code>", "");

        let editor = v_flex()
            .w_full()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(t("settings.postProcessing.prompts.promptLabel")),
                    )
                    .child(Input::new(&self.prompt_name).small()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(t("settings.postProcessing.prompts.promptInstructions")),
                    )
                    .child(Textarea::new(&self.prompt_text))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tip),
                    ),
            );

        let mut body = v_flex().w_full().gap_3().child(
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    div().flex_1().child(
                        Dropdown::new("pp-prompt-dropdown")
                            .options(
                                prompts
                                    .iter()
                                    .map(|p| DropdownOption::new(p.id.clone(), p.name.clone()))
                                    .collect(),
                            )
                            .selected(selected_id.clone())
                            .placeholder(if prompts.is_empty() {
                                t("settings.postProcessing.prompts.noPrompts")
                            } else {
                                t("settings.postProcessing.prompts.selectPrompt")
                            })
                            .min_width(px(260.))
                            .disabled(
                                st.settings.is_updating("post_process_selected_prompt_id")
                                    || self.creating,
                            )
                            .on_select({
                                let weak = cx.entity().downgrade();
                                move |v, _, cx| {
                                    let v = v.to_string();
                                    AppState::global(cx).update(cx, |s, cx| {
                                        s.update_setting(
                                            "post_process_selected_prompt_id",
                                            serde_json::json!(v),
                                            cx,
                                        )
                                    });
                                    weak.update(cx, |this, cx| {
                                        this.creating = false;
                                        cx.notify();
                                    })
                                    .ok();
                                }
                            }),
                    ),
                )
                .child(
                    Button::new("pp-create-new")
                        .primary()
                        .small()
                        .label(t("settings.postProcessing.prompts.createNew"))
                        .disabled(self.creating)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.creating = true;
                            this.prompt_name
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            this.prompt_text
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            cx.notify();
                        })),
                ),
        );

        if self.creating {
            body = body.child(editor).child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("pp-create-prompt")
                            .primary()
                            .small()
                            .label(t("settings.postProcessing.prompts.createPrompt"))
                            .disabled(!complete)
                            .on_click(cx.listener(|this, _, _, cx| this.create_prompt(cx))),
                    )
                    .child(
                        Button::new("pp-cancel-create")
                            .outline()
                            .small()
                            .label(t("settings.postProcessing.prompts.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.creating = false;
                                this.sync_inputs(true, window, cx);
                                cx.notify();
                            })),
                    ),
            );
        } else if let Some(prompt) = selected {
            let (id_update, id_delete) = (prompt.id.clone(), prompt.id.clone());
            body = body.child(editor).child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        Button::new("pp-update-prompt")
                            .primary()
                            .small()
                            .label(t("settings.postProcessing.prompts.updatePrompt"))
                            .disabled(!dirty || !complete)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_prompt(id_update.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new("pp-delete-prompt")
                            .outline()
                            .small()
                            .label(t("settings.postProcessing.prompts.deletePrompt"))
                            .disabled(prompts.len() <= 1)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_prompt(id_delete.clone(), cx)
                            })),
                    ),
            );
        } else {
            body = body.child(div().text_sm().text_color(theme.muted_foreground).child(
                if prompts.is_empty() {
                    t("settings.postProcessing.prompts.createFirst")
                } else {
                    t("settings.postProcessing.prompts.selectToEdit")
                },
            ));
        }

        vec![SettingRow::new(
            "pp-selected-prompt",
            t("settings.postProcessing.prompts.selectedPrompt.title"),
        )
        .description(t(
            "settings.postProcessing.prompts.selectedPrompt.description",
        ))
        .stacked()
        .control(body)
        .into_any_element()]
    }
}

impl Render for PostProcessingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return page().into_any_element();
        };
        let api_rows = self.api_rows(&s, cx);
        let prompt_rows = self.prompt_rows(&s, cx);
        page()
            .id("page-post-processing")
            .child(group(
                Some(t("settings.postProcessing.hotkey.title")),
                vec![self.shortcut.clone().into_any_element()],
                cx,
            ))
            .child(group(
                Some(t("settings.postProcessing.api.title")),
                api_rows,
                cx,
            ))
            .child(group(
                Some(t("settings.postProcessing.prompts.title")),
                prompt_rows,
                cx,
            ))
            .test_support()
            .into_any_element()
    }
}
