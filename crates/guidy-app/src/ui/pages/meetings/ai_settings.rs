//! Meeting settings, AI + Linear part (plan tasks 2 and 3.1): the meetings
//! LLM provider and model (separate from dictation post-processing), the
//! built-in llama.cpp models, Apple Intelligence availability, and the Linear
//! personal API key.

use crate::logic::meetings::{meetings_patch, MeetingsEvent};
use crate::logic::models::format_model_size;
use crate::meetings_backend::{
    error_message, LocalLlmModel, MeetingsBackend, SharedMeetingsBackend,
};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, Pill};
use crate::ui::kit::{group, icon, t, t_args, Dropdown, DropdownOption, SettingRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::llm::{feature_model, is_builtin_provider, resolve_provider, Feature, ProviderRef};
use guidy_core::meetings::settings::{MeetingSettings, BUILTIN_LLM_PROVIDER_ID};
use guidy_core::settings::AppSettings;
use std::collections::HashMap;

pub const LINEAR_API_KEY_URL: &str = "https://linear.app/settings/api";
const APPLE_PROVIDER_ID: &str = crate::logic::flows::APPLE_PROVIDER_ID;

/// The meetings provider + model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeetingsLlmChoice {
    pub provider_id: String,
    /// The meetings model set for the provider (empty when none).
    pub model: String,
    /// The model summaries actually use ([`feature_model`]): `model`, or
    /// the provider's post-processing model when none is set.
    pub effective_model: String,
}

impl MeetingsLlmChoice {
    pub fn from_settings(s: &AppSettings) -> Self {
        let provider_id = if s.meetings.llm_provider_id.is_empty() {
            BUILTIN_LLM_PROVIDER_ID.to_string()
        } else {
            s.meetings.llm_provider_id.clone()
        };
        let model = s
            .meetings
            .llm_models
            .get(&provider_id)
            .cloned()
            .unwrap_or_default();
        let effective_model = feature_model(s, Feature::Meetings, &provider_id);
        Self {
            provider_id,
            model,
            effective_model,
        }
    }

    /// The post-processing model used because none is set for meetings.
    pub fn fallback_model(&self) -> Option<&str> {
        (self.model.trim().is_empty() && !self.effective_model.is_empty())
            .then_some(self.effective_model.as_str())
    }
}

/// Provider choices for meetings: the built-in runtime first, then the
/// post-processing providers (BYOK). Apple Intelligence only when available.
pub fn provider_options(s: &AppSettings, apple_available: bool) -> Vec<DropdownOption> {
    let mut out = vec![
        DropdownOption::new(BUILTIN_LLM_PROVIDER_ID, t("meetingsAi.llm.builtin"))
            .description(t("meetingsAi.llm.builtinDescription")),
    ];
    out.extend(
        s.post_process_providers
            .iter()
            .filter(|p| p.id != APPLE_PROVIDER_ID || apple_available)
            .map(|p| DropdownOption::new(p.id.clone(), p.label.clone())),
    );
    out
}

pub fn provider_label(s: &AppSettings, provider_id: &str) -> String {
    match resolve_provider(s, provider_id) {
        Some(ProviderRef::Builtin) => guidy_i18n::t("meetingsAi.llm.builtin"),
        Some(ProviderRef::Configured(p)) => p.label.clone(),
        None => provider_id.to_string(),
    }
}

pub fn model_label(model: &str) -> String {
    model.to_string()
}

/// `settings.meetings` with the provider/model changed.
pub fn with_llm_choice(
    m: &MeetingSettings,
    provider_id: &str,
    model: Option<&str>,
) -> MeetingSettings {
    let mut m = m.clone();
    m.llm_provider_id = provider_id.to_string();
    if let Some(model) = model {
        if model.is_empty() {
            m.llm_models.remove(provider_id);
        } else {
            m.llm_models
                .insert(provider_id.to_string(), model.to_string());
        }
    }
    m
}

pub struct AiSettingsView {
    state: Entity<AppState>,
    backend: SharedMeetingsBackend,
    apple_available: bool,
    model_input: Entity<InputState>,
    models: HashMap<String, Vec<String>>,
    /// The provider whose model list is being fetched.
    fetching_models: Option<String>,
    /// The last failed model listing: (provider id, message). Shown by the
    /// model field, apart from the connection test.
    models_error: Option<(String, String)>,
    testing: bool,
    test_result: Option<Result<String, String>>,
    local: Entity<LocalModelsView>,
    linear_key: Entity<InputState>,
    has_linear_key: bool,
    linear_testing: bool,
    linear_result: Option<Result<String, String>>,
    synced_provider: String,
    _subs: Vec<Subscription>,
}

impl AiSettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let backend = state.read(cx).meetings.clone();
        let apple_available = state.read(cx).backend.check_apple_intelligence_available();
        let model_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetingsAi.llm.modelPlaceholder"))
        });
        let linear_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("meetingsAi.linear.keyPlaceholder"))
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_model_input(false, window, cx);
                cx.notify();
            }),
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| {
                if let AppEvent::Meetings(MeetingsEvent::Ready) = ev {
                    this.load_linear_key_status(cx);
                }
            }),
            cx.subscribe(&model_input, |this, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.set_model(v, cx);
                }
            }),
            cx.subscribe(&linear_key, |this, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    if !v.is_empty() {
                        this.save_linear_key(v, cx);
                    }
                }
            }),
        ];
        let local = cx.new(LocalModelsView::new);
        // A downloaded/deleted built-in model changes the model choices.
        let local_sub = cx.observe(&local, |_, _, cx| cx.notify());
        let mut subs = subs;
        subs.push(local_sub);
        let mut this = Self {
            state,
            backend,
            apple_available,
            model_input,
            models: HashMap::new(),
            fetching_models: None,
            models_error: None,
            testing: false,
            test_result: None,
            local,
            linear_key,
            has_linear_key: false,
            linear_testing: false,
            linear_result: None,
            synced_provider: String::new(),
            _subs: subs,
        };
        this.sync_model_input(true, window, cx);
        this.load_linear_key_status(cx);
        this
    }

    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&dyn MeetingsBackend) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let r = cx.background_spawn(async move { f(&*backend) }).await;
            this.update(cx, |s, cx| {
                done(s, r, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn choice(&self, cx: &App) -> MeetingsLlmChoice {
        self.state
            .read(cx)
            .app_settings()
            .map(MeetingsLlmChoice::from_settings)
            .unwrap_or_default()
    }

    fn sync_model_input(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let c = self.choice(cx);
        let provider_changed = force || self.synced_provider != c.provider_id;
        let focused = self.model_input.focus_handle(cx).is_focused(window);
        if (provider_changed || !focused) && self.model_input.read(cx).value() != c.model.as_str() {
            let m = c.model.clone();
            self.model_input
                .update(cx, |s, cx| s.set_value(m, window, cx));
        }
        // No meetings model: the placeholder shows the one used instead.
        let placeholder = match c.fallback_model() {
            Some(m) => SharedString::from(m.to_string()),
            None => t("meetingsAi.llm.modelPlaceholder"),
        };
        self.model_input
            .update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
        if provider_changed {
            self.synced_provider = c.provider_id.clone();
            self.test_result = None;
            self.models_error = None;
            self.fetch_models(c.provider_id, cx);
        }
    }

    /// Whether a Linear key is stored (a Keychain read: on open and after
    /// saving, never on a timer).
    fn load_linear_key_status(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |b| b.has_linear_api_key(),
            |this, has, _| this.has_linear_key = has,
        );
    }

    fn linear_key_saved(&mut self, has: bool, cx: &mut Context<Self>) {
        self.has_linear_key = has;
        self.state.update(cx, |s, cx| {
            s.emit_meetings(MeetingsEvent::LinearKeyChanged, cx)
        });
    }

    /// Write only the fields that differ from the stored meetings settings.
    fn save_meetings(&mut self, m: MeetingSettings, cx: &mut Context<Self>) {
        let Some(before) = self
            .state
            .read(cx)
            .app_settings()
            .map(|s| s.meetings.clone())
        else {
            return;
        };
        let patch = meetings_patch(&before, &m);
        if patch.as_object().is_some_and(|p| !p.is_empty()) {
            self.state
                .update(cx, |s, cx| s.update_setting("meetings", patch, cx));
        }
    }

    fn set_provider(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        if s.meetings.llm_provider_id == id {
            return;
        }
        // Built-in: default to the first downloaded model.
        let model = (is_builtin_provider(&id) && !s.meetings.llm_models.contains_key(&id))
            .then(|| {
                self.local
                    .read(cx)
                    .models
                    .iter()
                    .find(|m| m.downloaded)
                    .map(|m| m.id.clone())
            })
            .flatten();
        self.save_meetings(with_llm_choice(&s.meetings, &id, model.as_deref()), cx);
    }

    fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return;
        };
        let c = MeetingsLlmChoice::from_settings(&s);
        if c.model == model {
            return;
        }
        self.save_meetings(
            with_llm_choice(&s.meetings, &c.provider_id, Some(&model)),
            cx,
        );
    }

    fn fetch_models(&mut self, provider_id: String, cx: &mut Context<Self>) {
        self.fetching_models = Some(provider_id.clone());
        let pid = provider_id.clone();
        self.run(
            cx,
            move |b| b.llm_models(&pid),
            move |this, r, cx| {
                if this.fetching_models.as_ref() == Some(&provider_id) {
                    this.fetching_models = None;
                }
                // A listing for a provider no longer selected is dropped.
                if this.choice(cx).provider_id != provider_id {
                    return;
                }
                match r {
                    Ok(m) => {
                        this.models_error = None;
                        this.models.insert(provider_id, m);
                    }
                    Err(e) => this.models_error = Some((provider_id, error_message(&e))),
                }
            },
        );
    }

    fn test_connection(&mut self, cx: &mut Context<Self>) {
        let pid = self.choice(cx).provider_id;
        self.testing = true;
        self.test_result = None;
        self.run(
            cx,
            move |b| b.llm_test_connection(&pid, guidy_core::llm::Feature::Meetings),
            |this, r, _| {
                this.testing = false;
                this.test_result = Some(r.map_err(|e| error_message(&e)));
            },
        );
    }

    fn save_linear_key(&mut self, key: String, cx: &mut Context<Self>) {
        self.linear_result = None;
        self.run(
            cx,
            move |b| b.set_linear_api_key(&key).map(|_| b.has_linear_api_key()),
            |this, r, cx| match r {
                Ok(has) => this.linear_key_saved(has, cx),
                Err(e) => this.linear_result = Some(Err(error_message(&e))),
            },
        );
    }

    /// Test hook: type a Linear key and commit it (as on Enter).
    #[cfg(test)]
    pub fn enter_linear_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let k = key.to_string();
        self.linear_key
            .update(cx, |s, cx| s.set_value(k.clone(), window, cx));
        self.save_linear_key(k, cx);
    }

    fn remove_linear_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.linear_key
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.linear_result = None;
        self.run(
            cx,
            |b| b.set_linear_api_key("").map(|_| b.has_linear_api_key()),
            |this, r, cx| match r {
                Ok(has) => this.linear_key_saved(has, cx),
                Err(e) => this.linear_result = Some(Err(error_message(&e))),
            },
        );
    }

    fn test_linear(&mut self, cx: &mut Context<Self>) {
        self.linear_testing = true;
        self.linear_result = None;
        self.run(
            cx,
            |b| b.test_linear(),
            |this, r, _| {
                this.linear_testing = false;
                this.linear_result = Some(r.map_err(|e| error_message(&e)));
            },
        );
    }

    fn open_url(&self, url: &'static str, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |b| b.open_url(url),
            move |_, r, _| {
                if let Err(e) = r {
                    log::warn!("Failed to open {url}: {e}");
                }
            },
        );
    }

    // -- render ---------------------------------------------------------

    fn llm_rows(&self, s: &AppSettings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = MeetingsLlmChoice::from_settings(s);
        let is_builtin = is_builtin_provider(&c.provider_id);
        let is_apple = c.provider_id == APPLE_PROVIDER_ID;
        let updating = self.state.read(cx).settings.is_updating("meetings");
        let weak = cx.entity().downgrade();
        let mut rows = vec![
            SettingRow::new("meetings-llm-provider", t("meetingsAi.llm.provider"))
                .description(t("meetingsAi.llm.providerDescription"))
                .control(
                    Dropdown::new("meetings-llm-provider-dropdown")
                        .options(provider_options(s, self.apple_available))
                        .selected(Some(c.provider_id.clone()))
                        .menu_width(px(300.))
                        .disabled(updating)
                        .on_select(move |v, _, cx| {
                            let v = v.to_string();
                            weak.update(cx, |this, cx| this.set_provider(v, cx)).ok();
                        }),
                )
                .into_any_element(),
        ];

        let models = self.models.get(&c.provider_id).cloned().unwrap_or_default();
        let fetching = self.fetching_models.as_ref() == Some(&c.provider_id);
        let models_error = self
            .models_error
            .as_ref()
            .filter(|(p, _)| *p == c.provider_id)
            .map(|(_, e)| e.clone());
        let weak = cx.entity().downgrade();
        let model_dropdown = Dropdown::new("meetings-llm-model-options")
            .options(
                models
                    .iter()
                    .map(|m| DropdownOption::new(m.clone(), model_label(m)))
                    .collect(),
            )
            .selected(Some(c.effective_model.clone()))
            .placeholder(if is_builtin && models.is_empty() {
                t("meetingsAi.llm.noLocalModels")
            } else {
                t("meetingsAi.llm.pickModel")
            })
            .menu_width(px(320.))
            .loading(fetching)
            .disabled(updating)
            .on_select(move |v, window, cx| {
                let v = v.to_string();
                weak.update(cx, |this, cx| {
                    this.model_input
                        .update(cx, |s, cx| s.set_value(v.clone(), window, cx));
                    this.set_model(v, cx);
                })
                .ok();
            });
        if !is_apple {
            let pid = c.provider_id.clone();
            let theme = cx.theme().clone();
            let fallback = c.fallback_model().map(|m| {
                div()
                    .id("meetings-llm-model-fallback")
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t_args(
                        "meetingsAi.llm.usingPostProcessModel",
                        &[("model", m)],
                    ))
                    .test_support()
            });
            let error = models_error.map(|e| {
                h_flex()
                    .id("meetings-llm-models-error")
                    .gap_1()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(icon("circle-alert").xsmall())
                    .child(div().min_w_0().truncate().child(e))
                    .test_support()
            });
            rows.push(
                SettingRow::new("meetings-llm-model", t("meetingsAi.llm.model"))
                    .description(t("meetingsAi.llm.modelDescription"))
                    .stacked()
                    .control(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .when(!is_builtin, |d| {
                                        d.child(
                                            div()
                                                .flex_1()
                                                .child(Input::new(&self.model_input).small()),
                                        )
                                    })
                                    .child(model_dropdown)
                                    .child(
                                        Button::new("meetings-llm-refresh-models")
                                            .ghost()
                                            .small()
                                            .icon(icon("refresh-ccw"))
                                            .tooltip(t("meetingsAi.llm.refreshModels"))
                                            .loading(fetching)
                                            .disabled(fetching)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.fetch_models(pid.clone(), cx)
                                            })),
                                    ),
                            )
                            .children(fallback)
                            .children(error),
                    )
                    .into_any_element(),
            );
        }
        rows.push(
            SettingRow::new("meetings-llm-test", t("meetingsAi.llm.testConnection"))
                .description(t("meetingsAi.llm.testDescription"))
                .control(
                    h_flex()
                        .gap_2()
                        .children(
                            self.test_result
                                .as_ref()
                                .map(|r| result_line("meetings-llm-test-result", r, cx)),
                        )
                        .child(
                            Button::new("meetings-llm-test-button")
                                .secondary()
                                .pill()
                                .small()
                                .label(t("meetingsAi.llm.testConnection"))
                                .loading(self.testing)
                                .disabled(self.testing)
                                .on_click(cx.listener(|this, _, _, cx| this.test_connection(cx))),
                        ),
                )
                .into_any_element(),
        );
        rows.push(
            SettingRow::new("meetings-apple-intelligence", t("meetingsAi.llm.apple"))
                .description(t("meetingsAi.llm.appleDescription"))
                .inline_description()
                .control(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.apple_available {
                            t("meetingsAi.llm.appleAvailable")
                        } else {
                            t("meetingsAi.llm.appleUnavailable")
                        }),
                )
                .into_any_element(),
        );
        rows
    }

    fn linear_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let muted = cx.theme().muted_foreground;
        vec![
            SettingRow::new("meetings-linear-key", t("meetingsAi.linear.key"))
                .description(t("meetingsAi.linear.keyDescription"))
                .stacked()
                .control(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Input::new(&self.linear_key).small().mask_toggle()),
                                )
                                .when(self.has_linear_key, |d| {
                                    d.child(
                                        Button::new("meetings-linear-remove-key")
                                            .ghost()
                                            .pill()
                                            .small()
                                            .label(t("meetingsAi.linear.removeKey"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.remove_linear_key(window, cx)
                                            })),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(muted)
                                .child(if self.has_linear_key {
                                    t("meetingsAi.linear.keySaved")
                                } else {
                                    t("meetingsAi.linear.noKey")
                                })
                                .child(
                                    Button::new("meetings-linear-get-key")
                                        .link()
                                        .xsmall()
                                        .label(t("meetingsAi.linear.getKey"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_url(LINEAR_API_KEY_URL, cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
            SettingRow::new("meetings-linear-test", t("meetingsAi.linear.test"))
                .description(t("meetingsAi.linear.testDescription"))
                .control(
                    h_flex()
                        .gap_2()
                        .children(self.linear_result.as_ref().map(|r| {
                            let r = r
                                .as_ref()
                                .map(|name| {
                                    guidy_i18n::t_args(
                                        "meetingsAi.linear.connectedAs",
                                        &[("name", name)],
                                    )
                                })
                                .map_err(Clone::clone);
                            result_line("meetings-linear-test-result", &r, cx)
                        }))
                        .child(
                            Button::new("meetings-linear-test-button")
                                .secondary()
                                .pill()
                                .small()
                                .label(t("meetingsAi.linear.test"))
                                .loading(self.linear_testing)
                                .disabled(self.linear_testing || !self.has_linear_key)
                                .on_click(cx.listener(|this, _, _, cx| this.test_linear(cx))),
                        ),
                )
                .into_any_element(),
        ]
    }
}

/// The built-in llama.cpp models: download, cancel, delete. Shared by the
/// meetings AI settings and the post-processing page (built-in provider),
/// so built-in models can be managed where meeting notes aren't available.
pub struct LocalModelsView {
    backend: SharedMeetingsBackend,
    pub models: Vec<LocalLlmModel>,
    error: Option<String>,
    /// A reload is running / another was asked for meanwhile.
    refreshing: bool,
    refresh_again: bool,
    _subs: Vec<Subscription>,
}

impl LocalModelsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let backend = state.read(cx).meetings.clone();
        let subs = vec![
            // Downloads report progress and state changes as events (no
            // polling).
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| match ev {
                AppEvent::Meetings(MeetingsEvent::LocalModelProgress { model_id, fraction }) => {
                    match this.models.iter_mut().find(|m| &m.id == model_id) {
                        Some(m) => {
                            m.download_progress = Some(*fraction);
                            cx.notify();
                        }
                        None => this.refresh(cx),
                    }
                }
                AppEvent::Meetings(MeetingsEvent::LocalModelsChanged | MeetingsEvent::Ready) => {
                    this.refresh(cx)
                }
                _ => {}
            }),
        ];
        let mut this = Self {
            backend,
            models: Vec::new(),
            error: None,
            refreshing: false,
            refresh_again: false,
            _subs: subs,
        };
        this.refresh(cx);
        this
    }

    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&dyn MeetingsBackend) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let r = cx.background_spawn(async move { f(&*backend) }).await;
            this.update(cx, |s, cx| {
                done(s, r, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Re-read the built-in models (one read at a time; events arriving
    /// meanwhile coalesce into one more read).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            self.refresh_again = true;
            return;
        }
        self.refreshing = true;
        self.run(
            cx,
            |b| b.local_llm_models(),
            |this, local, cx| {
                this.refreshing = false;
                match local {
                    Ok(l) => {
                        this.models = l;
                        this.error = None;
                    }
                    Err(e) => this.error = Some(error_message(&e)),
                }
                if std::mem::take(&mut this.refresh_again) {
                    this.refresh(cx);
                }
            },
        );
    }

    fn local_action(&mut self, action: &'static str, id: String, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |b| match action {
                "download" => b.download_local_llm_model(&id),
                "cancel" => b.cancel_local_llm_download(&id),
                _ => b.delete_local_llm_model(&id),
            },
            |this, r, cx| {
                if let Err(e) = r {
                    this.error = Some(error_message(&e));
                }
                this.refresh(cx);
            },
        );
    }

    fn local_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(e) = &self.error {
            rows.push(
                error_box("meetings-local-error", e.clone(), cx)
                    .test_support()
                    .into_any_element(),
            );
        }
        if self.models.is_empty() {
            rows.push(
                div()
                    .id("meetings-local-empty")
                    .p_3()
                    .text_sm()
                    .text_color(muted)
                    .child(t("meetingsAi.local.empty"))
                    .test_support()
                    .into_any_element(),
            );
        }
        for m in &self.models {
            let id = m.id.clone();
            let mut tags = h_flex().gap(px(6.)).flex_wrap();
            if m.is_default {
                tags = tags.child(design::chip(t("meetingsAi.local.default"), true, cx));
            }
            if let Some(gb) = m.recommended_ram_gb {
                tags = tags.child(design::chip(
                    t_args(
                        "meetingsAi.local.recommendedRam",
                        &[("gb", &gb.to_string())],
                    ),
                    false,
                    cx,
                ));
            }
            if m.downloaded {
                tags = tags.child(
                    design::chip("", false, cx)
                        .child(icon("check").size(px(11.)))
                        .child(t("meetingsAi.local.downloaded")),
                );
            }
            let action: AnyElement = match (m.downloaded, m.download_progress) {
                (_, Some(p)) => {
                    let id = id.clone();
                    v_flex()
                        .w(px(220.))
                        .gap(px(6.))
                        .child(
                            div()
                                .id(SharedString::from(format!("local-progress-{}", m.id)))
                                .child(design::track(
                                    SharedString::from(format!("local-track-{}", m.id)),
                                    Some(p),
                                    cx,
                                ))
                                .test_support(),
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .font_family(crate::theme::MONO_FONT)
                                .text_size(design::TEXT_MONO)
                                .text_color(muted)
                                .child(design::shimmer_text(
                                    SharedString::from(format!("local-dl-{}", m.id)),
                                    t_args(
                                        "meetingsAi.local.downloading",
                                        &[(
                                            "percentage",
                                            &format!("{}", (p * 100.0).round() as i64),
                                        )],
                                    ),
                                    cx,
                                ))
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "local-cancel-{}",
                                        m.id
                                    )))
                                    .ghost()
                                    .pill()
                                    .xsmall()
                                    .label(t("meetingsAi.common.cancel"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.local_action("cancel", id.clone(), cx)
                                    })),
                                ),
                        )
                        .into_any_element()
                }
                (true, None) => Button::new(SharedString::from(format!("local-delete-{}", m.id)))
                    .ghost()
                    .pill()
                    .small()
                    .icon(icon("trash"))
                    .label(t("meetingsAi.common.delete"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.local_action("delete", id.clone(), cx)
                    }))
                    .into_any_element(),
                (false, None) => {
                    Button::new(SharedString::from(format!("local-download-{}", m.id)))
                        .secondary()
                        .pill()
                        .small()
                        .icon(icon("download"))
                        .label(t("meetingsAi.local.download"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.local_action("download", id.clone(), cx)
                        }))
                        .into_any_element()
                }
            };
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("local-model-{}", m.id)))
                    .w_full()
                    .px(design::SECTION_X)
                    .py(px(12.))
                    .gap_4()
                    .justify_between()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .flex_wrap()
                                    .child(
                                        div()
                                            .text_size(design::TEXT_BODY)
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(m.name.clone()),
                                    )
                                    .child(tags),
                            )
                            .when(!m.description.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(relative(1.5))
                                        .text_color(muted)
                                        .child(m.description.clone()),
                                )
                            })
                            .when(!m.fits_this_mac, |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.warning)
                                        .child(t("meetingsAi.local.notEnoughRam")),
                                )
                            })
                            .child(
                                h_flex()
                                    .mt(px(2.))
                                    .gap(px(6.))
                                    .font_family(crate::theme::MONO_FONT)
                                    .text_size(design::TEXT_MONO)
                                    .text_color(muted)
                                    .child(
                                        icon(if m.downloaded {
                                            "hard-drive"
                                        } else {
                                            "download"
                                        })
                                        .xsmall(),
                                    )
                                    .child(format_model_size(Some(m.size_mb as f64))),
                            ),
                    )
                    .child(action)
                    .test_support()
                    .into_any_element(),
            );
        }
        rows
    }
}

impl Render for LocalModelsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.local_rows(cx);
        div().id("local-models").w_full().child(group(
            Some(t("meetingsAi.local.heading")),
            rows,
            cx,
        ))
    }
}

/// A one-line ✓ / ✗ result next to a "Test" button.
pub fn result_line(id: &'static str, r: &Result<String, String>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon_name, color, text) = match r {
        Ok(m) => ("check", theme.success, m.clone()),
        Err(e) => ("circle-alert", theme.danger, e.clone()),
    };
    h_flex()
        .id(id)
        .max_w(px(320.))
        .gap_1()
        .text_xs()
        .text_color(color)
        .child(icon(icon_name).xsmall())
        .child(div().min_w_0().truncate().child(text))
        .test_support()
        .into_any_element()
}

pub fn error_box(id: &'static str, message: String, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .m_3()
        .px_3()
        .py_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.danger.opacity(0.4))
        .bg(theme.danger.opacity(0.1))
        .text_sm()
        .child(message)
}

impl Render for AiSettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(s) = self.state.read(cx).app_settings().cloned() else {
            return v_flex().id("meetings-ai-settings").into_any_element();
        };
        let llm = self.llm_rows(&s, cx);
        let linear = self.linear_rows(cx);
        v_flex()
            .id("meetings-ai-settings")
            .w_full()
            .gap_6()
            .child(group(Some(t("meetingsAi.llm.heading")), llm, cx))
            .child(self.local.clone())
            .child(group(Some(t("meetingsAi.linear.heading")), linear, cx))
            .test_support()
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        provider_label, provider_options, with_llm_choice, MeetingsLlmChoice, APPLE_PROVIDER_ID,
    };
    use guidy_core::meetings::settings::{MeetingSettings, BUILTIN_LLM_PROVIDER_ID};

    #[test]
    fn choice_defaults_to_builtin() {
        let mut s = guidy_core::settings::get_default_settings();
        s.meetings.llm_provider_id.clear();
        let c = MeetingsLlmChoice::from_settings(&s);
        assert_eq!(c.provider_id, BUILTIN_LLM_PROVIDER_ID);
        assert_eq!(c.model, "");
    }

    #[test]
    fn llm_choice_is_per_provider() {
        let m = MeetingSettings::default();
        let m = with_llm_choice(&m, "openrouter", Some("a/b"));
        assert_eq!(m.llm_provider_id, "openrouter");
        assert_eq!(
            m.llm_models.get("openrouter").map(String::as_str),
            Some("a/b")
        );
        let m = with_llm_choice(&m, "openai", None);
        assert_eq!(m.llm_provider_id, "openai");
        assert_eq!(
            m.llm_models.get("openrouter").map(String::as_str),
            Some("a/b")
        );
        let m = with_llm_choice(&m, "openrouter", Some(""));
        assert!(!m.llm_models.contains_key("openrouter"));
    }

    #[test]
    fn provider_options_hide_unavailable_apple() {
        guidy_i18n::set_language("en");
        let mut s = guidy_core::settings::get_default_settings();
        s.post_process_providers
            .push(guidy_core::settings::PostProcessProvider {
                id: APPLE_PROVIDER_ID.into(),
                label: "Apple Intelligence".into(),
                base_url: String::new(),
                allow_base_url_edit: false,
                models_endpoint: None,
                supports_structured_output: true,
                api_format: Default::default(),
            });
        let ids = |apple| {
            provider_options(&s, apple)
                .into_iter()
                .map(|o| o.value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(false)[0], BUILTIN_LLM_PROVIDER_ID);
        assert!(!ids(false).contains(&APPLE_PROVIDER_ID.to_string()));
        assert!(ids(true).contains(&APPLE_PROVIDER_ID.to_string()));
    }

    #[test]
    fn provider_labels_cover_builtin_configured_and_unknown() {
        guidy_i18n::set_language("en");
        let s = guidy_core::settings::get_default_settings();
        assert_eq!(
            provider_label(&s, BUILTIN_LLM_PROVIDER_ID),
            guidy_i18n::t("meetingsAi.llm.builtin")
        );
        let openai = s.post_process_provider("openai").unwrap();
        assert_eq!(provider_label(&s, "openai"), openai.label);
        assert_eq!(provider_label(&s, "no_such"), "no_such");
    }
}

/// UI tests (GPUI test platform, fake backends).
#[cfg(test)]
mod ui {
    use super::AiSettingsView;
    use crate::meetings_backend::fake::FakeMeetingsBackend;
    use crate::meetings_backend::LocalLlmModel;
    use crate::ui::pages::meetings::tickets::ui::{setup, setup_with};
    use crate::ui::pages::post_processing::PostProcessingPage;
    use crate::ui_tests::{exists, open, with_window};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext as _, TestAppContext};

    fn local(id: &str, default: bool, ram: Option<u32>, downloaded: bool) -> LocalLlmModel {
        LocalLlmModel {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            size_mb: 2500,
            is_default: default,
            recommended_ram_gb: ram,
            fits_this_mac: true,
            downloaded,
            download_progress: None,
        }
    }

    #[gpui_kit::test]
    fn linear_key_is_saved_and_tested(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        setup(cx, mb.clone());
        let (w, view) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "meetings-linear-key"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-linear-get-key", cx)
        });
        assert!(mb.called("open_url https://linear.app/settings/api"));
        cx.update_window(w, |_, window, cx| {
            view.update(cx, |v, cx| v.enter_linear_key("lin_api_123", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert!(mb.called("set_linear_api_key lin_api_123"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-linear-test-button", cx)
        });
        assert!(mb.called("test_linear"));
        assert!(exists(cx, w, "meetings-linear-test-result"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-linear-remove-key", cx)
        });
        assert!(mb.called("set_linear_api_key "));
    }

    #[gpui_kit::test]
    fn local_models_download_and_delete(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        mb.data.lock().ai.local_models = vec![
            local("qwen3-4b", true, None, true),
            local("qwen3-8b", false, Some(16), false),
        ];
        setup(cx, mb.clone());
        let (w, _) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "local-model-qwen3-4b"));
        assert!(exists(cx, w, "local-model-qwen3-8b"));
        with_window(cx, w, |window, cx| {
            window.click("local-download-qwen3-8b", cx)
        });
        assert!(mb.called("download_local_llm_model qwen3-8b"));
        with_window(cx, w, |window, cx| {
            window.click("local-delete-qwen3-4b", cx)
        });
        assert!(mb.called("delete_local_llm_model qwen3-4b"));
    }

    #[gpui_kit::test]
    fn meetings_provider_test_connection(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        setup(cx, mb.clone());
        let (w, _) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "meetings-llm-provider"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-test-button", cx)
        });
        assert!(mb.called("llm_test_connection guidy_local Meetings"));
        assert!(exists(cx, w, "meetings-llm-test-result"));
    }

    #[gpui_kit::test]
    fn model_field_shows_the_post_processing_fallback(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        let fb = crate::testing::FakeBackend::new();
        fb.with_settings(|s| {
            s.meetings.llm_provider_id = "openai".into();
            s.meetings.llm_models.clear();
            s.post_process_models
                .insert("openai".into(), "gpt-4o-mini".into());
        });
        let state = setup_with(cx, fb.clone(), mb);
        let (w, view) = open(cx, AiSettingsView::new);
        let choice = view.read_with(cx, |v, cx| v.choice(cx));
        assert_eq!(choice.model, "");
        assert_eq!(choice.effective_model, "gpt-4o-mini");
        // Same model summaries use.
        let s = fb.settings();
        assert_eq!(
            choice.effective_model,
            guidy_core::llm::feature_model(&s, guidy_core::llm::Feature::Meetings, "openai")
        );
        assert!(exists(cx, w, "meetings-llm-model-fallback"));
        let placeholder = view.read_with(cx, |v, cx| {
            v.model_input.read(cx).presentation().placeholder().clone()
        });
        assert_eq!(placeholder.as_ref(), "gpt-4o-mini");
        // A meetings model of its own: no fallback hint.
        fb.with_settings(|s| {
            s.meetings
                .llm_models
                .insert("openai".into(), "gpt-4.1".into());
        });
        state.update(cx, |s, cx| s.refresh_settings(cx));
        cx.run_until_parked();
        assert!(!exists(cx, w, "meetings-llm-model-fallback"));
    }

    #[gpui_kit::test]
    fn failed_model_listing_is_not_a_test_result(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        mb.data.lock().ai.fail_llm_models = Some("listing failed".into());
        setup(cx, mb.clone());
        let (w, view) = open(cx, AiSettingsView::new);
        assert!(mb.called("llm_models guidy_local"));
        assert!(exists(cx, w, "meetings-llm-models-error"));
        assert!(!exists(cx, w, "meetings-llm-test-result"));
        assert!(view.read_with(cx, |v, _| v.test_result.is_none()));
        // A listing for a provider no longer selected is dropped.
        view.update(cx, |v, cx| {
            v.models_error = None;
            v.fetch_models("openai".into(), cx);
        });
        cx.run_until_parked();
        assert!(mb.called("llm_models openai"));
        assert!(view.read_with(cx, |v, _| v.models_error.is_none()));
        assert!(!exists(cx, w, "meetings-llm-models-error"));
        // A later success clears the error.
        mb.data.lock().ai.fail_llm_models = None;
        view.update(cx, |v, cx| v.fetch_models("guidy_local".into(), cx));
        cx.run_until_parked();
        assert!(!exists(cx, w, "meetings-llm-models-error"));
    }

    #[gpui_kit::test]
    fn byok_test_connection_and_editable_base_url(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        let fb = crate::testing::FakeBackend::new();
        let gateway = fb
            .settings()
            .post_process_providers
            .iter()
            .find(|p| p.allow_base_url_edit && p.id != "custom")
            .map(|p| p.id.clone());
        let state = setup_with(cx, fb.clone(), mb.clone());
        let (w, _) = open(cx, PostProcessingPage::new);
        assert!(exists(cx, w, "pp-test-connection"));
        let pid = fb.settings().post_process_provider_id.clone();
        with_window(cx, w, |window, cx| {
            window.click("pp-test-connection-button", cx)
        });
        assert!(
            mb.called(&format!("llm_test_connection {pid} PostProcess")),
            "{:?}",
            mb.calls()
        );
        assert!(exists(cx, w, "pp-test-result"));
        // Gateways whose URL holds an account id (Cloudflare) can edit it.
        if let Some(id) = gateway {
            assert!(!exists(cx, w, "pp-base-url"));
            cx.update(|cx| state.update(cx, |s, cx| s.set_post_process_provider(&id, cx)));
            cx.run_until_parked();
            assert!(exists(cx, w, "pp-base-url"), "{id}");
        }
    }
}
