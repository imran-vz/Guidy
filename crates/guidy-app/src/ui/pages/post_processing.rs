//! Post-processing page: hotkey, LLM provider/API, prompt templates.

use crate::logic::flows::{
    post_process_model_options, prompt_is_dirty, should_auto_fetch_models, APPLE_PROVIDER_ID,
    CUSTOM_PROVIDER_ID,
};
use crate::logic::meetings::MeetingsEvent;
use crate::logic::settings_store::ToastSpec;
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{group, icon, page, t, Dropdown, DropdownOption, SettingRow};
use crate::ui::pages::meetings::ai_settings::LocalModelsView;
use crate::ui::shortcut_input::ShortcutInput;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::llm::{is_builtin_provider, resolve_provider, ProviderRef};
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
    /// BYOK "Test connection": running, and the last result per provider.
    testing: bool,
    test_result: Option<(String, Result<String, String>)>,
    /// A test asked for while that provider's key / base URL save is in
    /// flight: it runs once the save lands.
    pending_test: Option<String>,
    /// Built-in models list (download/delete), shown by "Manage built-in
    /// models"; created on first use.
    local_models: Option<Entity<LocalModelsView>>,
    show_local_models: bool,
    /// (provider id, prompt id) the inputs were last synced for.
    synced: (String, Option<String>),
    /// API keys per provider, read from the Keychain off the UI thread
    /// when the provider is first shown (never on render/notify).
    api_keys: std::collections::HashMap<String, String>,
    /// Providers whose key is being read.
    loading_keys: std::collections::HashSet<String>,
    /// Keys being saved, per provider: shown in the input until the save
    /// finishes, whatever a Keychain read in flight returns meanwhile.
    saving_keys: std::collections::HashMap<String, String>,
    _subs: Vec<Subscription>,
}

/// The selected provider id and what it resolves to. An unknown id falls
/// back to the first configured provider.
fn provider_of(s: &AppSettings) -> (String, Option<ProviderRef<'_>>) {
    let id = if s.post_process_provider_id.is_empty() {
        s.post_process_providers
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "openai".into())
    } else {
        s.post_process_provider_id.clone()
    };
    let provider = resolve_provider(s, &id).or_else(|| {
        s.post_process_providers
            .first()
            .map(ProviderRef::Configured)
    });
    (id, provider)
}

/// BYOK "Test connection" outcome: a check or an alert with the message.
fn test_result_line(r: &Result<String, String>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (name, color, text) = match r {
        Ok(m) => ("check", theme.success, m.clone()),
        Err(e) => ("circle-alert", theme.danger, e.clone()),
    };
    h_flex()
        .id("pp-test-result")
        .max_w(px(320.))
        .gap_1()
        .text_xs()
        .text_color(color)
        .child(icon(name).xsmall())
        .child(div().min_w_0().truncate().child(text))
        .test_support()
        .into_any_element()
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
                this.run_pending_test(cx);
                cx.notify();
            }),
            // A built-in model was downloaded or deleted: refresh the model
            // choices when the built-in provider is selected.
            cx.subscribe(&state, |this, state, ev: &AppEvent, cx| {
                if !matches!(ev, AppEvent::Meetings(MeetingsEvent::LocalModelsChanged)) {
                    return;
                }
                let builtin = state.read(cx).app_settings().and_then(|s| {
                    matches!(provider_of(s).1, Some(ProviderRef::Builtin)).then(|| provider_of(s).0)
                });
                if let Some(pid) = builtin {
                    this.state
                        .update(cx, |st, cx| st.fetch_post_process_models(&pid, cx));
                }
            }),
            // Base URL and API key save on blur (and Enter).
            cx.subscribe_in(&base_url, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save_base_url(v, cx);
                }
            }),
            cx.subscribe_in(
                &api_key,
                window,
                |this, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let v = input.read(cx).value().trim().to_string();
                        this.save_api_key(v, window, cx);
                    }
                },
            ),
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
            testing: false,
            test_result: None,
            pending_test: None,
            local_models: None,
            show_local_models: false,
            synced: (String::new(), None),
            api_keys: Default::default(),
            loading_keys: Default::default(),
            saving_keys: Default::default(),
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
        let is_builtin = matches!(provider, Some(ProviderRef::Builtin));
        let provider = provider.and_then(ProviderRef::configured);
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
            let base = provider.map(|p| p.base_url.clone()).unwrap_or_default();
            set_if(&self.base_url, base, window, cx);
            let shown_key = self
                .saving_keys
                .get(&pid)
                .or_else(|| self.api_keys.get(&pid))
                .cloned();
            match shown_key {
                Some(key) => set_if(&self.api_key, key, window, cx),
                // No key for the built-in runtime.
                None if is_builtin => {}
                None => {
                    if provider_changed {
                        // Don't show the previous provider's key meanwhile.
                        set_if(&self.api_key, String::new(), window, cx);
                    }
                    self.load_api_key(pid.clone(), cx, |_, _, _| {});
                }
            }
            set_if(
                &self.model,
                s.post_process_models.get(&pid).cloned().unwrap_or_default(),
                window,
                cx,
            );
            let anthropic = provider
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

    /// Read a provider's API key off the UI thread (Keychain), cache it,
    /// then `done(key)`.
    fn load_api_key(
        &mut self,
        provider_id: String,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
    ) {
        if self.loading_keys.contains(&provider_id) {
            return;
        }
        self.loading_keys.insert(provider_id.clone());
        cx.spawn(async move |this, cx| {
            let pid = provider_id.clone();
            let key = cx
                .background_spawn(async move { guidy_core::settings::provider_api_key(&pid) })
                .await;
            this.update(cx, |this, cx| {
                this.loading_keys.remove(&provider_id);
                // A key saved while this read was in flight is newer.
                let key = this.api_keys.entry(provider_id).or_insert(key).clone();
                // The observer mirrors the cached key into the input.
                cx.notify();
                done(this, key, cx);
            })
            .ok();
        })
        .detach();
    }

    #[cfg(test)]
    pub fn cached_api_key(&self, provider_id: &str) -> Option<&str> {
        self.api_keys.get(provider_id).map(String::as_str)
    }

    #[cfg(test)]
    pub fn enter_api_key(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.to_string();
        self.api_key
            .update(cx, |st, cx| st.set_value(value.clone(), window, cx));
        self.save_api_key(value, window, cx);
    }

    /// Type into the key / base URL inputs without committing (no blur).
    #[cfg(test)]
    pub fn type_api_key(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.to_string();
        self.api_key
            .update(cx, |st, cx| st.set_value(value, window, cx));
    }

    #[cfg(test)]
    pub fn type_base_url(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.to_string();
        self.base_url
            .update(cx, |st, cx| st.set_value(value, window, cx));
    }

    /// A test waits for a save to land.
    #[cfg(test)]
    pub fn test_waiting(&self) -> bool {
        self.pending_test.is_some()
    }

    #[cfg(test)]
    pub fn saving_api_key(&self, provider_id: &str) -> bool {
        self.saving_keys.contains_key(provider_id)
    }

    #[cfg(test)]
    pub fn test_outcome(&self) -> Option<&Result<String, String>> {
        self.test_result.as_ref().map(|(_, r)| r)
    }

    #[cfg(test)]
    pub fn api_key_input(&self, cx: &App) -> String {
        self.api_key.read(cx).value().to_string()
    }

    /// Store the key in the Keychain. The cached key changes only once the
    /// backend confirms; on failure the input goes back to the stored key
    /// and an error toast says why. A key typed before the stored one has
    /// loaded is saved too.
    fn save_api_key(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let (pid, _) = provider_of(&s);
        let current = self
            .saving_keys
            .get(&pid)
            .or_else(|| self.api_keys.get(&pid));
        match current {
            Some(current) if *current == value => return,
            // Still loading, the input shows nothing yet: an empty value
            // is the placeholder, not a cleared key.
            None if value.is_empty() => return,
            _ => {}
        }
        self.saving_keys.insert(pid.clone(), value.clone());
        let page = cx.entity().downgrade();
        let window = window.window_handle();
        let provider = pid.clone();
        self.state.update(cx, |st, cx| {
            st.update_post_process_then(
                "api_key",
                &provider,
                value.clone(),
                cx,
                move |st, r, cx| {
                    if let Err(e) = r {
                        st.toast(
                            ToastSpec::error(
                                guidy_i18n::t("settings.postProcessing.api.apiKey.saveFailed"),
                                Some(e.clone()),
                            ),
                            cx,
                        );
                    }
                    let ok = r.is_ok();
                    cx.defer(move |cx| {
                        let _ = window.update(cx, |_, window, cx| {
                            let _ = page.update(cx, |this, cx| {
                                this.finish_api_key_save(pid, value, ok, window, cx)
                            });
                        });
                    });
                },
            )
        });
    }

    fn finish_api_key_save(
        &mut self,
        pid: String,
        value: String,
        ok: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving_keys.get(&pid) == Some(&value) {
            self.saving_keys.remove(&pid);
        }
        if ok {
            self.api_keys.insert(pid, value);
        } else if let Some(old) = self
            .saving_keys
            .get(&pid)
            .or_else(|| self.api_keys.get(&pid))
            .cloned()
        {
            let shown = self.state.read(cx).app_settings().map(|s| provider_of(s).0);
            if shown.as_deref() == Some(pid.as_str()) {
                self.api_key
                    .update(cx, |st, cx| st.set_value(old, window, cx));
            }
        }
        self.run_pending_test(cx);
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
        let provider = provider.and_then(ProviderRef::configured);
        let editable = pid == CUSTOM_PROVIDER_ID || provider.is_some_and(|p| p.allow_base_url_edit);
        if !editable || value.is_empty() || provider.is_some_and(|p| p.base_url == value) {
            return;
        }
        // Notify once done so a test waiting on this save runs.
        self.state.update(cx, |st, cx| {
            st.update_post_process_then("base_url", &pid, value, cx, |_, _, cx| cx.notify())
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
        self.state
            .update(cx, |st, cx| st.set_post_process_provider(&id, cx));
        // Fetch models once the provider's key is known (Keychain read
        // off the UI thread, cached).
        let pid = id.clone();
        let cached = if is_builtin_provider(&id) {
            Some(String::new())
        } else {
            self.api_keys.get(&id).cloned()
        };
        let fetch_if = move |this: &mut Self, key: String, cx: &mut Context<Self>| {
            if should_auto_fetch_models(resolve_provider(&s, &id), &key) {
                this.state
                    .update(cx, |st, cx| st.fetch_post_process_models(&id, cx));
            }
        };
        match cached {
            Some(key) => fetch_if(self, key, cx),
            None => self.load_api_key(pid, cx, fetch_if),
        }
        cx.notify();
    }

    /// BYOK: check the provider's base URL, key and model. A key or base
    /// URL typed but not saved yet is saved first, and the test runs once
    /// every save for the provider has landed (the backend reads the stored
    /// values).
    fn test_connection(
        &mut self,
        provider_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.testing {
            return;
        }
        self.testing = true;
        self.test_result = None;
        let builtin = self
            .state
            .read(cx)
            .app_settings()
            .is_some_and(|s| matches!(provider_of(s).1, Some(ProviderRef::Builtin)));
        if !builtin {
            let base = self.base_url.read(cx).value().trim().to_string();
            self.save_base_url(base, cx);
            let key = self.api_key.read(cx).value().trim().to_string();
            self.save_api_key(key, window, cx);
        }
        self.pending_test = Some(provider_id);
        self.run_pending_test(cx);
        cx.notify();
    }

    /// Run the pending test once its provider has no save in flight.
    fn run_pending_test(&mut self, cx: &mut Context<Self>) {
        let Some(pid) = self.pending_test.clone() else {
            return;
        };
        let st = self.state.read(cx);
        let saving = self.saving_keys.contains_key(&pid)
            || ["base_url", "api_key", "model"]
                .iter()
                .any(|w| st.settings.is_updating(&format!("post_process_{w}:{pid}")));
        if saving {
            return;
        }
        self.pending_test = None;
        let provider_id = pid;
        let backend = self.state.read(cx).meetings.clone();
        cx.spawn(async move |this, cx| {
            let pid = provider_id.clone();
            let r = cx
                .background_spawn(async move {
                    backend.llm_test_connection(&pid, guidy_core::llm::Feature::PostProcess)
                })
                .await;
            this.update(cx, |this, cx| {
                this.testing = false;
                this.test_result = Some((provider_id, r));
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_local_models(&mut self, cx: &mut Context<Self>) {
        self.show_local_models = !self.show_local_models;
        if self.show_local_models && self.local_models.is_none() {
            self.local_models = Some(cx.new(LocalModelsView::new));
        }
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
        let is_builtin = matches!(provider, Some(ProviderRef::Builtin));
        let provider = provider.and_then(ProviderRef::configured);
        let base_url_editable =
            !is_builtin && (is_custom || provider.is_some_and(|p| p.allow_base_url_edit));
        let up = |k: String| st.settings.is_updating(&k);
        let mut rows = vec![SettingRow::new(
            "pp-provider",
            t("settings.postProcessing.api.provider.title"),
        )
        .description(t("settings.postProcessing.api.provider.description"))
        .control(
            Dropdown::new("pp-provider-dropdown")
                .options(
                    std::iter::once(
                        DropdownOption::new(
                            guidy_core::meetings::settings::BUILTIN_LLM_PROVIDER_ID,
                            t("settings.postProcessing.api.builtin.label"),
                        )
                        .description(t("settings.postProcessing.api.builtin.description")),
                    )
                    .chain(
                        s.post_process_providers
                            .iter()
                            .map(|p| DropdownOption::new(p.id.clone(), p.label.clone())),
                    )
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
            let fmt = provider.map(|p| p.api_format).unwrap_or_default();
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
        }
        if is_builtin {
            // No key or URL: the built-in models are managed right here
            // (meeting notes may be unavailable on this Mac).
            rows.push(
                SettingRow::new("pp-builtin", t("settings.postProcessing.api.builtin.label"))
                    .description(t("settings.postProcessing.api.builtin.description"))
                    .control(
                        Button::new("pp-builtin-manage")
                            .outline()
                            .small()
                            .icon(icon("download"))
                            .label(t("settings.postProcessing.api.builtin.manage"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_local_models(cx))),
                    )
                    .into_any_element(),
            );
        }
        if base_url_editable {
            rows.push(
                SettingRow::new(
                    "pp-base-url",
                    t("settings.postProcessing.api.baseUrl.title"),
                )
                .description(if is_custom {
                    t("settings.postProcessing.api.baseUrl.description")
                } else {
                    t("meetingsAi.byok.baseUrlEditable")
                })
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
        rows.extend((!is_builtin).then(|| {
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
                .into_any_element()
        }));

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
                        .when(!is_builtin, |d| {
                            d.child(
                                div().flex_1().child(
                                    Input::new(&self.model)
                                        .small()
                                        .disabled(up(format!("post_process_model:{pid}"))),
                                ),
                            )
                        })
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
        let result = self
            .test_result
            .as_ref()
            .filter(|(p, _)| *p == pid)
            .map(|(_, r)| test_result_line(r, cx));
        let pid_test = pid.clone();
        rows.push(
            SettingRow::new("pp-test-connection", t("meetingsAi.byok.testConnection"))
                .description(t("meetingsAi.byok.testDescription"))
                .control(
                    h_flex().gap_2().children(result).child(
                        Button::new("pp-test-connection-button")
                            .outline()
                            .small()
                            .label(t("meetingsAi.byok.testConnection"))
                            .loading(self.testing)
                            .disabled(self.testing)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.test_connection(pid_test.clone(), window, cx)
                            })),
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
        let builtin = matches!(provider_of(&s).1, Some(ProviderRef::Builtin));
        let local_models = self
            .local_models
            .clone()
            .filter(|_| builtin && self.show_local_models);
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
            .children(local_models)
            .child(group(
                Some(t("settings.postProcessing.prompts.title")),
                prompt_rows,
                cx,
            ))
            .test_support()
            .into_any_element()
    }
}
