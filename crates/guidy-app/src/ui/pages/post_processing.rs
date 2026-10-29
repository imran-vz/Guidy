//! Post-processing page: the shortcut, the LLM provider as a compact form
//! (provider, base URL, API key, model with refresh; Test Connection in the
//! title bar), and the prompts as a list beside an editor with Revert and
//! Save (⌘S). Unsaved prompt edits are never dropped silently: switching
//! prompts asks first, switching providers keeps them.

use crate::logic::flows::{
    post_process_model_options, prompt_is_dirty, should_auto_fetch_models, APPLE_PROVIDER_ID,
    CUSTOM_PROVIDER_ID,
};
use crate::logic::meetings::MeetingsEvent;
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{
    confirm, group, icon, page, t, t_args, Dropdown, DropdownOption, LocalizedPlaceholder,
    ScrollTrap, SettingRow,
};
use crate::ui::pages::meetings::ai_settings::LocalModelsView;
use crate::ui::shortcut_input::ShortcutInput;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use std::collections::HashMap;

/// Width of the provider form's fields.
const FIELD_W: Pixels = dp(320.);
/// The model row's picker and refresh button (inside `FIELD_W`).
const MODEL_PICKER_W: Pixels = dp(96.);
const MODEL_REFRESH_W: Pixels = dp(24.);
const MODEL_GAP: Pixels = dp(4.);
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
    prompt_list_focus: FocusHandle,
    /// The prompt list scrolls inside the page.
    prompt_list_scroll: ScrollHandle,
    creating: bool,
    /// Why the last prompt create/update/delete failed.
    prompt_error: Option<SharedString>,
    /// Inline errors of the provider fields (base_url, api_key, model).
    field_errors: HashMap<&'static str, SharedString>,
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

/// BYOK "Test connection" outcome as a small chip beside the button: a
/// check or an alert; the full message is its tooltip.
fn test_result_chip(r: &Result<String, String>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (name, color, text) = match r {
        Ok(m) => ("check", theme.success, m.clone()),
        Err(e) => ("circle-alert", theme.danger, e.clone()),
    };
    let tip: SharedString = text.clone().into();
    h_flex()
        .id("pp-test-result")
        .max_w(dp(220.))
        .h(dp(20.))
        .px(dp(6.))
        .gap(dp(4.))
        .rounded(design::RADIUS_CHIP)
        .bg(color.opacity(0.1))
        .text_size(design::TEXT_SECONDARY)
        .text_color(color)
        .child(icon(name).xsmall())
        .child(div().min_w_0().truncate().child(text))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
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
                .localized_placeholder("settings.postProcessing.api.apiKey.placeholder", window, cx)
        });
        let model = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.postProcessing.api.model.placeholderInput",
                window,
                cx,
            )
        });
        let prompt_name = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.postProcessing.prompts.promptLabelPlaceholder",
                window,
                cx,
            )
        });
        let prompt_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(5, 16)
                .localized_placeholder(
                    "settings.postProcessing.prompts.promptInstructionsPlaceholder",
                    window,
                    cx,
                )
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
            cx.subscribe(&prompt_name, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.prompt_error = None;
                    cx.notify();
                }
            }),
            cx.subscribe(&prompt_text, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.prompt_error = None;
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
            prompt_list_focus: cx.focus_handle().tab_stop(true),
            prompt_list_scroll: ScrollHandle::new(),
            creating: false,
            prompt_error: None,
            field_errors: HashMap::new(),
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
        // Prompts don't depend on the provider: a provider switch keeps an
        // unsaved prompt draft.
        if !self.creating && (force || self.synced.1 != prompt_id) {
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
            st.update_post_process_then("api_key", &provider, value.clone(), cx, move |_, r, cx| {
                let error = r.as_ref().err().cloned();
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, cx| {
                        let _ = page.update(cx, |this, cx| {
                            this.finish_api_key_save(pid, value, error, window, cx)
                        });
                    });
                });
            })
        });
    }

    fn finish_api_key_save(
        &mut self,
        pid: String,
        value: String,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ok = error.is_none();
        match error {
            Some(e) => {
                self.field_errors.insert(
                    "api_key",
                    t_args(
                        "settings.postProcessing.api.apiKey.saveFailedInline",
                        &[("error", &e)],
                    ),
                );
            }
            None => {
                self.field_errors.remove("api_key");
            }
        }
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
        let page = cx.entity().downgrade();
        self.state.update(cx, |st, cx| {
            st.update_post_process_then(what, &pid, value, cx, move |_, r, cx| {
                let r = r.clone();
                page.update(cx, |this, cx| this.set_field_result(what, r, cx))
                    .ok();
            })
        });
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
        let page = cx.entity().downgrade();
        self.state.update(cx, |st, cx| {
            st.update_post_process_then("base_url", &pid, value, cx, move |_, r, cx| {
                let r = r.clone();
                page.update(cx, |this, cx| this.set_field_result("base_url", r, cx))
                    .ok();
                cx.notify();
            })
        });
    }

    /// Show (or clear) the inline error of a provider field after a save.
    fn set_field_result(
        &mut self,
        what: &'static str,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                self.field_errors.remove(what);
            }
            Err(e) => {
                self.field_errors
                    .insert(what, t_args("settings.saveFailed", &[("error", &e)]));
            }
        }
        cx.notify();
    }

    fn select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        self.apple_unavailable = false;
        self.field_errors.clear();
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

    // -- Prompts -------------------------------------------------------------

    /// The draft differs from what is stored (or a new prompt has text).
    fn prompt_dirty(&self, cx: &App) -> bool {
        let name = self.prompt_name.read(cx).value().to_string();
        let text = self.prompt_text.read(cx).value().to_string();
        if self.creating {
            return !name.trim().is_empty() || !text.trim().is_empty();
        }
        let Some(s) = self.state.read(cx).app_settings() else {
            return false;
        };
        s.post_process_selected_prompt_id
            .as_ref()
            .and_then(|id| s.post_process_prompts.iter().find(|p| &p.id == id))
            .is_some_and(|p| prompt_is_dirty(&p.name, &p.prompt, &name, &text))
    }

    /// Run `then` now, or after the user agrees to drop unsaved prompt edits.
    fn guard_unsaved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.prompt_dirty(cx) {
            then(self, window, cx);
            return;
        }
        let name = self.prompt_name.read(cx).value().trim().to_string();
        let weak = cx.entity().downgrade();
        confirm(
            window,
            cx,
            if name.is_empty() {
                t("settings.postProcessing.prompts.discardTitleUntitled")
            } else {
                t_args(
                    "settings.postProcessing.prompts.discardTitle",
                    &[("name", &name)],
                )
            },
            t("settings.postProcessing.prompts.discardDescription"),
            t("settings.postProcessing.prompts.discard"),
            true,
            move |window, cx| {
                weak.update(cx, |this, cx| then(this, window, cx)).ok();
            },
            |_, _| {},
        );
    }

    fn select_prompt(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, move |this, window, cx| {
            this.creating = false;
            this.prompt_error = None;
            let id = id.clone();
            this.state.update(cx, |s, cx| {
                s.update_setting("post_process_selected_prompt_id", serde_json::json!(id), cx)
            });
            this.sync_inputs(true, window, cx);
            cx.notify();
        });
    }

    fn start_new_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_unsaved(window, cx, |this, window, cx| {
            this.creating = true;
            this.prompt_error = None;
            this.prompt_name
                .update(cx, |s, cx| s.set_value("", window, cx));
            this.prompt_text
                .update(cx, |s, cx| s.set_value("", window, cx));
            let h = this.prompt_name.focus_handle(cx);
            window.focus(&h, cx);
            cx.notify();
        });
    }

    /// Put the stored prompt back in the editor (or drop the new one).
    fn revert_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating = false;
        self.prompt_error = None;
        self.sync_inputs(true, window, cx);
        cx.notify();
    }

    /// Save ⌘S: create the new prompt or update the selected one.
    fn save_prompt(&mut self, cx: &mut Context<Self>) {
        let name = self.prompt_name.read(cx).value().trim().to_string();
        let text = self.prompt_text.read(cx).value().trim().to_string();
        if name.is_empty() || text.is_empty() {
            self.prompt_error = Some(t("settings.postProcessing.prompts.incomplete"));
            cx.notify();
            return;
        }
        if self.creating {
            self.create_prompt(name, text, cx);
        } else if let Some(id) = self
            .state
            .read(cx)
            .app_settings()
            .and_then(|s| s.post_process_selected_prompt_id.clone())
        {
            if self.prompt_dirty(cx) {
                self.update_prompt(id, name, text, cx);
            }
        }
    }

    fn create_prompt(&mut self, name: String, text: String, cx: &mut Context<Self>) {
        self.prompt_error = None;
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
                    Err(e) => {
                        log::error!("Failed to create prompt: {e}");
                        weak.update(cx, |this, cx| {
                            this.prompt_error = Some(t_args(
                                "settings.postProcessing.prompts.saveFailed",
                                &[("error", &e)],
                            ));
                            cx.notify();
                        })
                        .ok();
                    }
                },
            );
        });
    }

    fn update_prompt(&mut self, id: String, name: String, text: String, cx: &mut Context<Self>) {
        self.prompt_error = None;
        let weak = cx.entity().downgrade();
        self.state.update(cx, |s, cx| {
            s.call(
                cx,
                move |b| b.update_post_process_prompt(&id, &name, &text),
                move |s, r, cx| {
                    if let Err(e) = r {
                        log::error!("Failed to update prompt: {e}");
                        weak.update(cx, |this, cx| {
                            this.prompt_error = Some(t_args(
                                "settings.postProcessing.prompts.saveFailed",
                                &[("error", &e)],
                            ));
                            cx.notify();
                        })
                        .ok();
                    }
                    s.refresh_settings(cx);
                },
            );
        });
    }

    fn confirm_delete_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, name)) = self.state.read(cx).app_settings().and_then(|s| {
            let id = s.post_process_selected_prompt_id.clone()?;
            let p = s.post_process_prompts.iter().find(|p| p.id == id)?;
            Some((id, p.name.clone()))
        }) else {
            return;
        };
        let weak = cx.entity().downgrade();
        confirm(
            window,
            cx,
            t_args(
                "settings.postProcessing.prompts.deleteTitle",
                &[("name", &name)],
            ),
            t("settings.postProcessing.prompts.deleteDescription"),
            t("common.delete"),
            true,
            move |_, cx| {
                let id = id.clone();
                weak.update(cx, |this, cx| this.delete_prompt(id, cx)).ok();
            },
            |_, _| {},
        );
    }

    fn delete_prompt(&mut self, id: String, cx: &mut Context<Self>) {
        self.creating = false;
        self.prompt_error = None;
        let weak = cx.entity().downgrade();
        self.state.update(cx, |s, cx| {
            s.call(
                cx,
                move |b| b.delete_post_process_prompt(&id),
                move |s, r, cx| {
                    if let Err(e) = r {
                        log::error!("Failed to delete prompt: {e}");
                        weak.update(cx, |this, cx| {
                            this.prompt_error = Some(t_args(
                                "settings.postProcessing.prompts.deleteFailed",
                                &[("error", &e)],
                            ));
                            cx.notify();
                        })
                        .ok();
                    }
                    s.refresh_settings(cx);
                },
            );
        });
    }

    #[cfg(test)]
    pub fn api_key_error(&self) -> Option<&SharedString> {
        self.field_errors.get("api_key")
    }

    #[cfg(test)]
    pub fn prompt_error(&self) -> Option<&SharedString> {
        self.prompt_error.as_ref()
    }

    #[cfg(test)]
    pub fn draft(&self, cx: &App) -> (String, String) {
        (
            self.prompt_name.read(cx).value().to_string(),
            self.prompt_text.read(cx).value().to_string(),
        )
    }

    // -- Provider ------------------------------------------------------------

    /// The inline error for `what` (base_url, api_key, model, provider).
    fn field_error(&self, what: &str, pid: &str, cx: &App) -> Option<SharedString> {
        self.field_errors.get(what).cloned().or_else(|| {
            self.state
                .read(cx)
                .settings
                .error_of(&format!("post_process_{what}:{pid}"))
                .map(|e| t_args("settings.saveFailed", &[("error", e)]))
        })
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
        .error(crate::ui::pages::advanced::save_error(
            st,
            "post_process_provider_id",
        ))
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
                .min_width(FIELD_W)
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
                        .px(design::SECTION_X)
                        .py(design::ROW_Y)
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(theme.danger)
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
                .error(self.field_error("api_format", &pid, cx))
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
                        .min_width(FIELD_W)
                        .menu_width(dp(300.))
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
            // No key or URL: the built-in models are managed right here.
            rows.push(
                SettingRow::new("pp-builtin", t("settings.postProcessing.api.builtin.label"))
                    .description(t("settings.postProcessing.api.builtin.description"))
                    .control(
                        Button::new("pp-builtin-manage")
                            .outline()
                            .small()
                            .label(t("settings.postProcessing.api.builtin.manage"))
                            .toggled(self.show_local_models)
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
                .error(self.field_error("base_url", &pid, cx))
                .control(
                    div()
                        .id("pp-base-url-input")
                        .w(FIELD_W)
                        .child(Input::new(&self.base_url).small()),
                )
                .into_any_element(),
            );
        }
        rows.extend((!is_builtin).then(|| {
            SettingRow::new("pp-api-key", t("settings.postProcessing.api.apiKey.title"))
                .error(self.field_error("api_key", &pid, cx))
                .control(
                    div()
                        .id("pp-api-key-input")
                        .w(FIELD_W)
                        .child(Input::new(&self.api_key).small().mask_toggle()),
                )
                .into_any_element()
        }));

        let current_model = s.post_process_models.get(&pid).cloned().unwrap_or_default();
        let fetched = st.pp_model_options.get(&pid).cloned().unwrap_or_default();
        let options = post_process_model_options(&fetched, &current_model);
        let fetching = up(format!("post_process_models_fetch:{pid}"));
        let pid_sel = pid.clone();
        let pid_ref = pid.clone();
        let model_options: Vec<DropdownOption> = options
            .into_iter()
            .map(|m| DropdownOption::new(m.clone(), m))
            .collect();
        let has_options = !model_options.is_empty();
        let picker = Dropdown::new("pp-model-options")
            .options(model_options)
            .selected(Some(current_model.clone()))
            .placeholder(if has_options || !is_builtin {
                t("settings.postProcessing.api.model.choose")
            } else {
                t("settings.postProcessing.api.model.placeholderNoOptions")
            })
            .min_width(if is_builtin {
                FIELD_W - MODEL_REFRESH_W - MODEL_GAP
            } else {
                MODEL_PICKER_W
            })
            .menu_width(dp(320.))
            .loading(fetching)
            .on_select(move |v, _, cx| {
                let (pid, v) = (pid_sel.clone(), v.trim().to_string());
                AppState::global(cx)
                    .update(cx, |s, cx| s.update_post_process("model", &pid, v, cx));
            });
        rows.push(
            SettingRow::new("pp-model", t("settings.postProcessing.api.model.title"))
                .description(if is_custom {
                    t("settings.postProcessing.api.model.descriptionCustom")
                } else {
                    t("settings.postProcessing.api.model.descriptionDefault")
                })
                .error(
                    self.field_error("model", &pid, cx)
                        .or_else(|| self.field_error("models_fetch", &pid, cx)),
                )
                .control(
                    // The same column as the fields above: the name, the
                    // picker and refresh together are FIELD_W wide.
                    h_flex()
                        .w(FIELD_W)
                        .gap(MODEL_GAP)
                        .when(!is_builtin, |d| {
                            d.child(
                                div()
                                    .id("pp-model-input")
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.model).small()),
                            )
                        })
                        .child(picker)
                        .child(
                            Button::new("pp-refresh-models")
                                .ghost()
                                .small()
                                .icon(icon("refresh-cw"))
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

    /// The prompt list (+ / −) beside the editor (name, instructions,
    /// Revert / Save ⌘S).
    fn prompts_view(
        &self,
        s: &AppSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let ring = cx.theme().ring;
        let prompts = s.post_process_prompts.clone();
        let selected_id = s.post_process_selected_prompt_id.clone();
        let has_selection = selected_id
            .as_ref()
            .is_some_and(|id| prompts.iter().any(|p| &p.id == id));
        let dirty = self.prompt_dirty(cx);
        let draft_name = self.prompt_name.read(cx).value().trim().to_string();
        let draft_text = self.prompt_text.read(cx).value().trim().to_string();
        let complete = !draft_name.is_empty() && !draft_text.is_empty();
        let list_focused = self.prompt_list_focus.is_focused(window);
        let tip = guidy_i18n::t("settings.postProcessing.prompts.promptTip")
            .replace("<code>", "")
            .replace("</code>", "");
        let row = |id: SharedString, label: SharedString, selected: bool| {
            div()
                .id(id)
                .mx(dp(4.))
                .px(dp(8.))
                .h(dp(24.))
                .flex()
                .items_center()
                .rounded(design::RADIUS_CHIP)
                .text_size(design::TEXT_BODY)
                .when(selected, |d| {
                    d.bg(if list_focused {
                        tone.selection_focused
                    } else {
                        tone.selection
                    })
                })
                .child(div().min_w_0().truncate().child(label))
                .test_support()
        };
        let mut list = v_flex()
            .id("pp-prompt-list")
            .flex_1()
            .min_h_0()
            .py(dp(4.))
            .overflow_y_scroll()
            .scroll_trap(&self.prompt_list_scroll)
            .children(prompts.iter().map(|p| {
                let pid = p.id.clone();
                row(
                    SharedString::from(format!("pp-prompt-{}", p.id)),
                    p.name.clone().into(),
                    !self.creating && selected_id.as_deref() == Some(p.id.as_str()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.prompt_list_focus, cx);
                    if this.creating
                        || this
                            .state
                            .read(cx)
                            .app_settings()
                            .and_then(|s| s.post_process_selected_prompt_id.clone())
                            .as_deref()
                            != Some(pid.as_str())
                    {
                        this.select_prompt(pid.clone(), window, cx);
                    }
                }))
            }));
        if self.creating {
            list = list.child(row(
                "pp-prompt-new".into(),
                if draft_name.is_empty() {
                    t("settings.postProcessing.prompts.newPrompt")
                } else {
                    draft_name.clone().into()
                },
                true,
            ));
        }
        let left =
            v_flex()
                .w(dp(180.))
                .flex_none()
                .border_r_1()
                .border_color(tone.hairline)
                .child(
                    div()
                        .id("pp-prompt-list-focus")
                        .track_focus(&self.prompt_list_focus)
                        .key_context("PromptList")
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(transparent_black())
                        .focus_visible(move |s| s.border_color(ring))
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                            let key = ev.keystroke.key.as_str();
                            if !matches!(key, "up" | "down") {
                                return;
                            }
                            let Some(s) = this.state.read(cx).app_settings().cloned() else {
                                return;
                            };
                            let ids: Vec<String> = s
                                .post_process_prompts
                                .iter()
                                .map(|p| p.id.clone())
                                .collect();
                            let at = s
                                .post_process_selected_prompt_id
                                .as_ref()
                                .and_then(|id| ids.iter().position(|i| i == id));
                            let next = match (key, at) {
                                ("up", Some(i)) => i.checked_sub(1),
                                ("down", Some(i)) => Some((i + 1).min(ids.len().saturating_sub(1))),
                                (_, None) => (!ids.is_empty()).then_some(0),
                                _ => None,
                            };
                            if let Some(id) = next.and_then(|i| ids.get(i)).cloned() {
                                if at.and_then(|i| ids.get(i)) != Some(&id) {
                                    this.select_prompt(id, window, cx);
                                }
                            }
                            cx.stop_propagation();
                        }))
                        .child(list),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .h(dp(24.))
                        .px(dp(2.))
                        .border_t_1()
                        .border_color(tone.hairline)
                        .child(
                            Button::new("pp-create-new")
                                .ghost()
                                .xsmall()
                                .icon(icon("plus"))
                                .tooltip(t("settings.postProcessing.prompts.createNew"))
                                .disabled(self.creating)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_prompt(window, cx)
                                })),
                        )
                        .child(
                            Button::new("pp-delete-prompt")
                                .ghost()
                                .xsmall()
                                .icon(icon("minus"))
                                .tooltip(t("settings.postProcessing.prompts.deletePrompt"))
                                .disabled(self.creating || !has_selection || prompts.len() <= 1)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_delete_prompt(window, cx)
                                })),
                        ),
                );
        let editing = self.creating || has_selection;
        let right: AnyElement =
            if !editing {
                div()
                    .flex_1()
                    .p(dp(16.))
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text_muted)
                    .child(if prompts.is_empty() {
                        t("settings.postProcessing.prompts.createFirstShort")
                    } else {
                        t("settings.postProcessing.prompts.selectToEditShort")
                    })
                    .into_any_element()
            } else {
                v_flex()
                    .id("pp-prompt-editor")
                    .flex_1()
                    .min_w_0()
                    .p(dp(12.))
                    .gap(dp(8.))
                    // ⌘S saves.
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                        let m = ev.keystroke.modifiers;
                        if ev.keystroke.key == "s" && m.platform && !m.alt && !m.control {
                            this.save_prompt(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div().id("pp-prompt-name").child(
                            Input::new(&self.prompt_name)
                                .small()
                                .aria_label(t("settings.postProcessing.prompts.promptLabel")),
                        ),
                    )
                    .child(
                        div()
                            .id("pp-prompt-text")
                            .child(Textarea::new(&self.prompt_text).aria_label(t(
                                "settings.postProcessing.prompts.promptInstructions",
                            )))
                            .test_support(),
                    )
                    .child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(tip),
                    )
                    .child(
                        h_flex()
                            .gap(dp(6.))
                            .child(
                                div()
                                    .id("pp-prompt-error")
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(design::TEXT_SECONDARY)
                                    .text_color(cx.theme().danger)
                                    .children(self.prompt_error.clone()),
                            )
                            .child(
                                Button::new("pp-revert-prompt")
                                    .ghost()
                                    .small()
                                    .label(t("settings.postProcessing.prompts.revert"))
                                    .disabled(!dirty)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.revert_prompt(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("pp-save-prompt")
                                    .primary()
                                    .small()
                                    .label(t("settings.postProcessing.prompts.save"))
                                    .tooltip(t("settings.postProcessing.prompts.saveShortcut"))
                                    .disabled(!dirty || !complete)
                                    .on_click(cx.listener(|this, _, _, cx| this.save_prompt(cx))),
                            ),
                    )
                    .into_any_element()
            };
        design::panel(cx)
            .id("pp-prompts")
            .child(
                h_flex()
                    .items_stretch()
                    .min_h(dp(220.))
                    .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
                    .child(left)
                    .child(right),
            )
            .into_any_element()
    }
}

impl Render for PostProcessingPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("postprocessing-scroll", self.render_body(window, cx))
    }
}

impl crate::ui::pages::Page for PostProcessingPage {
    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.state.read(cx).app_settings().cloned()?;
        let (pid, _) = provider_of(&s);
        if pid == APPLE_PROVIDER_ID {
            return None;
        }
        let result = self
            .test_result
            .as_ref()
            .filter(|(p, _)| *p == pid)
            .map(|(_, r)| test_result_chip(r, cx));
        Some(
            h_flex()
                .gap(dp(6.))
                .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
                .children(result)
                .child(
                    Button::new("pp-test-connection-button")
                        .outline()
                        .small()
                        .label(t("settings.postProcessing.api.testConnection"))
                        .tooltip(t("meetingsAi.byok.testDescription"))
                        .loading(self.testing)
                        .disabled(self.testing)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.test_connection(pid.clone(), window, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}

impl PostProcessingPage {
    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return page().into_any_element();
        };
        let api_rows = self.api_rows(&s, cx);
        let prompts = self.prompts_view(&s, window, cx);
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
            .child(
                v_flex()
                    .gap(design::HEADER_GAP)
                    .child(div().px(dp(2.)).child(design::section_header(
                        t("settings.postProcessing.prompts.listTitle"),
                        cx,
                    )))
                    .child(prompts),
            )
            .test_support()
            .into_any_element()
    }
}
