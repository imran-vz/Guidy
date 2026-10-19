//! Meeting settings, AI + Linear part (plan tasks 2 and 3.1): the meetings
//! LLM provider and model (separate from dictation post-processing), the
//! built-in llama.cpp models, Apple Intelligence availability, and the Linear
//! personal API key.

use crate::logic::flows::CUSTOM_PROVIDER_ID;
use crate::logic::meetings::{meetings_patch, MeetingsEvent, PROVIDER_KEY_URLS};
use crate::logic::models::format_model_size;
use crate::logic::settings_store::ToastSpec;
use crate::meetings_backend::{
    error_message, LocalLlmModel, MeetingsBackend, ModelCatalog, SharedMeetingsBackend,
};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, Pill};
use crate::ui::kit::{group, icon, t, t_args, toggle, Dropdown, DropdownOption, SettingRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::llm::registry::{Cost, ModelEntry};
use guidy_core::llm::{feature_model, is_builtin_provider, resolve_provider, Feature, ProviderRef};
use guidy_core::meetings::settings::{MeetingSettings, BUILTIN_LLM_PROVIDER_ID};
use guidy_core::settings::AppSettings;
use std::collections::{HashMap, HashSet};

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

/// Where to create an API key for a provider (the "Get a key" link).
pub fn provider_key_url(provider_id: &str) -> Option<&'static str> {
    PROVIDER_KEY_URLS
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, url)| *url)
}

/// A radio mark: accent ring when selected.
fn radio(selected: bool, cx: &App) -> Div {
    let tone = design::tone(cx);
    div()
        .flex_none()
        .size(px(14.))
        .rounded_full()
        .when(selected, |d| d.border_4().border_color(tone.accent))
        .when(!selected, |d| d.border_1().border_color(tone.text_faint))
}

/// Where summaries run: the two tabs of the Summaries settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryPlace {
    Cloud,
    Local,
}

impl SummaryPlace {
    /// On this Mac: the built-in runtime and Apple Intelligence.
    pub fn of(provider_id: &str) -> Self {
        if is_builtin_provider(provider_id) || provider_id == APPLE_PROVIDER_ID {
            Self::Local
        } else {
            Self::Cloud
        }
    }
}

/// Cloud providers for summaries (the post-processing providers, BYOK).
pub fn cloud_provider_options(s: &AppSettings) -> Vec<DropdownOption> {
    s.post_process_providers
        .iter()
        .filter(|p| SummaryPlace::of(&p.id) == SummaryPlace::Cloud)
        .map(|p| DropdownOption::new(p.id.clone(), p.label.clone()))
        .collect()
}

/// The provider the Cloud tab shows: the selected one when it's a cloud
/// provider, else the last one chosen, else Anthropic, else the first.
pub fn cloud_provider_id(s: &AppSettings) -> String {
    let is_cloud = |id: &str| {
        SummaryPlace::of(id) == SummaryPlace::Cloud && s.post_process_provider(id).is_some()
    };
    [
        s.meetings.llm_provider_id.as_str(),
        s.meetings.llm_cloud_provider_id.as_str(),
        "anthropic",
    ]
    .into_iter()
    .find(|id| is_cloud(id))
    .map(str::to_string)
    .or_else(|| {
        s.post_process_providers
            .iter()
            .find(|p| is_cloud(&p.id))
            .map(|p| p.id.clone())
    })
    .unwrap_or_default()
}

/// `1000000` -> `1M`, `262144` -> `262K`.
pub fn format_context(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", trim_number(tokens as f64 / 1_000_000.0, 2))
    } else {
        format!("{}K", (tokens as f64 / 1000.0).round() as u64)
    }
}

/// USD per million input / output tokens, `free`, or `None` when unknown.
pub fn format_price(cost: Option<Cost>) -> Option<String> {
    let c = cost?;
    let (input, output) = (c.input?, c.output?);
    if input == 0.0 && output == 0.0 {
        return Some(guidy_i18n::t("meetingsAi.llm.free"));
    }
    Some(format!(
        "${} / ${}",
        trim_number(input, 3),
        trim_number(output, 3)
    ))
}

fn trim_number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// "Model details from models.dev, updated 3 h ago" (or bundled).
pub fn registry_age(fetched_at: Option<u64>, now: u64) -> String {
    use guidy_i18n::{t, t_args};
    let Some(at) = fetched_at else {
        return t("meetingsAi.llm.registryBundled");
    };
    let secs = now.saturating_sub(at);
    let when = if secs < 120 {
        t("meetingsAi.llm.justNow")
    } else if secs < 2 * 3600 {
        t_args(
            "meetingsAi.llm.minutesAgo",
            &[("n", &(secs / 60).to_string())],
        )
    } else if secs < 2 * 86_400 {
        t_args(
            "meetingsAi.llm.hoursAgo",
            &[("n", &(secs / 3600).to_string())],
        )
    } else {
        t_args(
            "meetingsAi.llm.daysAgo",
            &[("n", &(secs / 86_400).to_string())],
        )
    };
    t_args("meetingsAi.llm.registryUpdated", &[("when", &when)])
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What the status line at the top of the Summaries settings says.
#[derive(Clone, Debug, PartialEq)]
pub enum ActiveSummary {
    /// Summaries are made by `model`, `via` a provider or on this Mac.
    Ready { model: String, via: String },
    /// Not usable yet, and what to do about it.
    NotReady(String),
}

pub struct AiSettingsView {
    state: Entity<AppState>,
    backend: SharedMeetingsBackend,
    apple_available: bool,
    api_key: Entity<InputState>,
    base_url: Entity<InputState>,
    /// A model id typed by hand, for providers models.dev doesn't describe.
    model_input: Entity<InputState>,
    search: Entity<InputState>,
    /// API keys per provider, read from the Keychain off the UI thread.
    api_keys: HashMap<String, String>,
    loading_keys: HashSet<String>,
    saving_key: Option<String>,
    catalogs: HashMap<String, ModelCatalog>,
    /// The provider whose catalog is being fetched.
    fetching: Option<String>,
    /// The latest catalog fetch per provider: an older one that returns
    /// later is dropped.
    catalog_fetches: HashMap<String, u64>,
    next_fetch: u64,
    /// The last failed catalog fetch: (provider id, message).
    catalog_error: Option<(String, String)>,
    /// Only models known to support structured output.
    good_only: bool,
    refreshing_registry: bool,
    testing: bool,
    test_result: Option<Result<String, String>>,
    local: Entity<LocalModelsView>,
    linear_key: Entity<InputState>,
    has_linear_key: bool,
    linear_testing: bool,
    linear_result: Option<Result<String, String>>,
    /// The cloud provider the inputs were last synced for.
    synced_provider: String,
    _subs: Vec<Subscription>,
}

impl AiSettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let backend = state.read(cx).meetings.clone();
        let apple_available = state.read(cx).backend.check_apple_intelligence_available();
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("settings.postProcessing.api.apiKey.placeholder"))
        });
        let base_url = cx.new(|cx| InputState::new(window, cx));
        let model_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetingsAi.llm.modelPlaceholder"))
        });
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("meetingsAi.llm.searchModels")));
        let linear_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("meetingsAi.linear.keyPlaceholder"))
        });
        let local = cx.new(|cx| {
            let mut v = LocalModelsView::new(cx);
            v.selectable = true;
            v
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_inputs(false, window, cx);
                cx.notify();
            }),
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| {
                if let AppEvent::Meetings(MeetingsEvent::Ready) = ev {
                    this.load_linear_key_status(cx);
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
            cx.subscribe(&base_url, |this, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save_base_url(v, cx);
                }
            }),
            cx.subscribe(&model_input, |this, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.set_model(v, cx);
                }
            }),
            cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
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
            // A downloaded/deleted built-in model changes the status line.
            cx.observe(&local, |_, _, cx| cx.notify()),
            cx.subscribe(&local, |this, _, ev: &LocalModelsEvent, cx| match ev {
                LocalModelsEvent::Select(id) => this.select_local(id.clone(), cx),
            }),
        ];
        let mut this = Self {
            state,
            backend,
            apple_available,
            api_key,
            base_url,
            model_input,
            search,
            api_keys: HashMap::new(),
            loading_keys: HashSet::new(),
            saving_key: None,
            catalogs: HashMap::new(),
            fetching: None,
            catalog_fetches: HashMap::new(),
            next_fetch: 0,
            catalog_error: None,
            good_only: true,
            refreshing_registry: false,
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
        this.sync_inputs(true, window, cx);
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

    fn settings(&self, cx: &App) -> Option<AppSettings> {
        self.state.read(cx).app_settings().cloned()
    }

    fn choice(&self, cx: &App) -> MeetingsLlmChoice {
        self.settings(cx)
            .as_ref()
            .map(MeetingsLlmChoice::from_settings)
            .unwrap_or_default()
    }

    /// Mirror stored values into the inputs when the cloud provider changes
    /// (never into a focused input), and fetch its key and catalog.
    fn sync_inputs(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
            return;
        };
        let pid = cloud_provider_id(&s);
        let changed = force || self.synced_provider != pid;
        let set_if = |input: &Entity<InputState>,
                      value: String,
                      changed: bool,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            let focused = input.focus_handle(cx).is_focused(window);
            if (changed || !focused) && input.read(cx).value() != value.as_str() {
                input.update(cx, |st, cx| st.set_value(value, window, cx));
            }
        };
        let provider = s.post_process_provider(&pid);
        let base = provider.map(|p| p.base_url.clone()).unwrap_or_default();
        set_if(&self.base_url, base, changed, window, cx);
        let model = s.meetings.llm_models.get(&pid).cloned().unwrap_or_default();
        set_if(&self.model_input, model, changed, window, cx);
        // No meetings model: the placeholder shows the one used instead.
        let fallback = feature_model(&s, Feature::Meetings, &pid);
        let placeholder = if fallback.is_empty() {
            t("meetingsAi.llm.modelPlaceholder")
        } else {
            SharedString::from(fallback)
        };
        self.model_input
            .update(cx, |st, cx| st.set_placeholder(placeholder, window, cx));
        if changed {
            let key = self.api_keys.get(&pid).cloned();
            set_if(&self.api_key, key.unwrap_or_default(), true, window, cx);
            self.synced_provider = pid.clone();
            self.test_result = None;
            self.catalog_error = None;
            if !self.api_keys.contains_key(&pid) {
                self.load_api_key(pid.clone(), window, cx);
            }
            self.fetch_catalog(pid, cx);
        }
    }

    /// Read a provider's key off the UI thread (Keychain), cache it and
    /// show it if that provider is still the one shown.
    fn load_api_key(&mut self, pid: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.loading_keys.insert(pid.clone()) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let read = pid.clone();
            let key = cx
                .background_spawn(async move { guidy_core::settings::provider_api_key(&read) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.loading_keys.remove(&pid);
                // A key saved while this read was in flight is newer.
                let key = this.api_keys.entry(pid.clone()).or_insert(key).clone();
                let focused = this.api_key.focus_handle(cx).is_focused(window);
                if this.synced_provider == pid && !focused {
                    this.api_key
                        .update(cx, |st, cx| st.set_value(key, window, cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Store the key in the Keychain (shared with post-processing), then
    /// reload the catalog with it. On failure the input goes back.
    fn save_api_key(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let pid = self.synced_provider.clone();
        match self.api_keys.get(&pid) {
            Some(current) if *current == value => return,
            // Still loading: an empty input is the placeholder.
            None if value.is_empty() => return,
            _ => {}
        }
        self.saving_key = Some(pid.clone());
        let view = cx.entity().downgrade();
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
                            let _ = view.update(cx, |this, cx| {
                                this.saving_key = None;
                                if ok {
                                    this.api_keys.insert(pid.clone(), value);
                                    this.test_result = None;
                                    this.fetch_catalog(pid, cx);
                                } else if let Some(old) = this.api_keys.get(&pid).cloned() {
                                    if this.synced_provider == pid {
                                        this.api_key
                                            .update(cx, |st, cx| st.set_value(old, window, cx));
                                    }
                                }
                                cx.notify();
                            });
                        });
                    });
                },
            )
        });
    }

    /// Test hook: type a key and commit it (as on Enter).
    #[cfg(test)]
    pub fn enter_api_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let k = key.to_string();
        self.api_key
            .update(cx, |s, cx| s.set_value(k.clone(), window, cx));
        self.save_api_key(k, window, cx);
    }

    fn save_base_url(&mut self, value: String, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
            return;
        };
        let pid = self.synced_provider.clone();
        let Some(p) = s.post_process_provider(&pid) else {
            return;
        };
        if !(p.allow_base_url_edit || pid == CUSTOM_PROVIDER_ID)
            || value.is_empty()
            || p.base_url == value
        {
            return;
        }
        // The old endpoint's models go; the catalog is fetched once the
        // save is done (from whichever URL is then stored), loading meanwhile.
        self.catalogs.remove(&pid);
        self.catalog_fetches.remove(&pid);
        self.fetching = Some(pid.clone());
        cx.notify();
        let view = cx.entity().downgrade();
        let provider = pid.clone();
        self.state.update(cx, |st, cx| {
            st.update_post_process_then("base_url", &provider, value, cx, move |_, _, cx| {
                cx.defer(move |cx| {
                    let _ = view.update(cx, |this, cx| this.fetch_catalog(pid, cx));
                });
            })
        });
    }

    /// Write only the fields that differ from the stored meetings settings.
    fn save_meetings(&mut self, m: MeetingSettings, cx: &mut Context<Self>) {
        let Some(before) = self.settings(cx).map(|s| s.meetings) else {
            return;
        };
        let patch = meetings_patch(&before, &m);
        if patch.as_object().is_some_and(|p| !p.is_empty()) {
            self.state
                .update(cx, |s, cx| s.update_setting("meetings", patch, cx));
        }
    }

    fn set_place(&mut self, place: SummaryPlace, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
            return;
        };
        if SummaryPlace::of(&s.meetings.llm_provider_id) == place {
            return;
        }
        match place {
            SummaryPlace::Cloud => self.set_provider(cloud_provider_id(&s), cx),
            SummaryPlace::Local => self.set_provider(BUILTIN_LLM_PROVIDER_ID.to_string(), cx),
        }
    }

    fn set_provider(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
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
        let mut m = with_llm_choice(&s.meetings, &id, model.as_deref());
        // Remember the cloud provider to switch back to: the new one, or
        // the one being left (it may predate `llm_cloud_provider_id`).
        let current = s.meetings.llm_provider_id.as_str();
        if SummaryPlace::of(&id) == SummaryPlace::Cloud {
            m.llm_cloud_provider_id = id;
        } else if !current.is_empty() && SummaryPlace::of(current) == SummaryPlace::Cloud {
            m.llm_cloud_provider_id = current.to_string();
        }
        self.save_meetings(m, cx);
    }

    fn select_local(&mut self, model: String, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
            return;
        };
        let m = with_llm_choice(&s.meetings, BUILTIN_LLM_PROVIDER_ID, Some(&model));
        self.save_meetings(m, cx);
    }

    /// The cloud model for the provider shown.
    fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        let Some(s) = self.settings(cx) else {
            return;
        };
        let pid = self.synced_provider.clone();
        if s.meetings
            .llm_models
            .get(&pid)
            .map(String::as_str)
            .unwrap_or("")
            == model
        {
            return;
        }
        let mut m = s.meetings.clone();
        if model.is_empty() {
            m.llm_models.remove(&pid);
        } else {
            m.llm_models.insert(pid.clone(), model);
        }
        self.save_meetings(m, cx);
    }

    fn fetch_catalog(&mut self, pid: String, cx: &mut Context<Self>) {
        if pid.is_empty() {
            return;
        }
        self.fetching = Some(pid.clone());
        self.next_fetch += 1;
        let seq = self.next_fetch;
        self.catalog_fetches.insert(pid.clone(), seq);
        let read = pid.clone();
        self.run(
            cx,
            move |b| b.llm_model_catalog(&read),
            move |this, r, _| {
                if this.catalog_fetches.get(&pid) != Some(&seq) {
                    // A newer fetch for this provider supersedes it.
                    return;
                }
                if this.fetching.as_ref() == Some(&pid) {
                    this.fetching = None;
                }
                match r {
                    Ok(c) => {
                        if this.catalog_error.as_ref().is_some_and(|(p, _)| *p == pid) {
                            this.catalog_error = None;
                        }
                        this.catalogs.insert(pid, c);
                    }
                    Err(e) => this.catalog_error = Some((pid, error_message(&e))),
                }
            },
        );
    }

    fn refresh_registry(&mut self, cx: &mut Context<Self>) {
        self.refreshing_registry = true;
        self.run(
            cx,
            |b| b.refresh_llm_registry(),
            |this, r, cx| {
                this.refreshing_registry = false;
                match r {
                    Ok(_) => this.fetch_catalog(this.synced_provider.clone(), cx),
                    Err(e) => {
                        let pid = this.synced_provider.clone();
                        this.catalog_error = Some((pid, error_message(&e)));
                    }
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

    /// What summaries use right now, for the status line.
    pub fn active_summary(&self, cx: &App) -> ActiveSummary {
        use guidy_i18n::{t, t_args};
        let Some(s) = self.settings(cx) else {
            return ActiveSummary::NotReady(String::new());
        };
        let c = MeetingsLlmChoice::from_settings(&s);
        if c.provider_id == APPLE_PROVIDER_ID {
            return if self.apple_available {
                ActiveSummary::Ready {
                    model: "Apple Intelligence".into(),
                    via: t("meetingsAi.llm.onThisMac"),
                }
            } else {
                ActiveSummary::NotReady(t("meetingsAi.llm.appleUnavailable"))
            };
        }
        if is_builtin_provider(&c.provider_id) {
            let models = &self.local.read(cx).models;
            let chosen = models
                .iter()
                .filter(|m| m.downloaded)
                .find(|m| m.id == c.model)
                .or_else(|| models.iter().find(|m| m.downloaded && m.is_default))
                .or_else(|| models.iter().find(|m| m.downloaded));
            return match chosen {
                Some(m) => ActiveSummary::Ready {
                    model: m.name.clone(),
                    via: t("meetingsAi.llm.onThisMac"),
                },
                None => ActiveSummary::NotReady(t("meetingsAi.llm.needsLocalModel")),
            };
        }
        let label = provider_label(&s, &c.provider_id);
        let key_missing = c.provider_id != CUSTOM_PROVIDER_ID
            && self
                .api_keys
                .get(&c.provider_id)
                .is_some_and(|k| k.trim().is_empty());
        if key_missing {
            return ActiveSummary::NotReady(t_args(
                "meetingsAi.llm.needsKey",
                &[("provider", &label)],
            ));
        }
        if c.effective_model.trim().is_empty() {
            return ActiveSummary::NotReady(t_args(
                "meetingsAi.llm.needsModel",
                &[("provider", &label)],
            ));
        }
        let name = self
            .catalogs
            .get(&c.provider_id)
            .and_then(|cat| cat.models.iter().find(|m| m.id == c.effective_model))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| c.effective_model.clone());
        ActiveSummary::Ready {
            model: name,
            via: label,
        }
    }

    // -- Linear -------------------------------------------------------------

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

    fn status_line(&self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let (dot, title, chip) = match self.active_summary(cx) {
            ActiveSummary::Ready { model, via } => (true, model, Some(via)),
            ActiveSummary::NotReady(why) => (false, why, None),
        };
        h_flex()
            .id("meetings-llm-active")
            .w_full()
            .gap_3()
            .px(design::SECTION_X)
            .py(px(12.))
            .child(if dot {
                design::signal_dot(cx).into_any_element()
            } else {
                div()
                    .flex_none()
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(7.)).rounded_full().bg(tone.text_faint))
                    .into_any_element()
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(design::eyebrow(t("meetingsAi.llm.activeHeading"), cx))
                    .child(
                        div()
                            .id("meetings-llm-active-model")
                            .text_size(design::TEXT_TITLE)
                            .font_weight(FontWeight::MEDIUM)
                            .when(!dot, |d| d.text_color(tone.text_muted))
                            .child(title)
                            .test_support(),
                    ),
            )
            .children(chip.map(|c| design::chip(c, false, cx)))
            .test_support()
            .into_any_element()
    }

    fn place_row(&self, place: SummaryPlace, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let seg =
            |id: &'static str, label: SharedString, p: SummaryPlace, cx: &mut Context<Self>| {
                let weak = weak.clone();
                design::segment(id, label, place == p, cx)
                    .on_click(move |_, _, cx| {
                        weak.update(cx, |this, cx| this.set_place(p, cx)).ok();
                    })
                    .test_support()
            };
        let cloud = seg(
            "meetings-llm-tab-cloud",
            t("meetingsAi.llm.tabCloud"),
            SummaryPlace::Cloud,
            cx,
        );
        let local = seg(
            "meetings-llm-tab-local",
            t("meetingsAi.llm.tabLocal"),
            SummaryPlace::Local,
            cx,
        );
        SettingRow::new("meetings-llm-place", t("meetingsAi.llm.place"))
            .description(t("meetingsAi.llm.placeDescription"))
            .inline_description()
            .control(design::segmented(cx).child(cloud).child(local))
            .into_any_element()
    }

    fn cloud_rows(&self, s: &AppSettings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let pid = self.synced_provider.clone();
        let provider = s.post_process_provider(&pid);
        let label = provider
            .map(|p| p.label.clone())
            .unwrap_or_else(|| pid.clone());
        let updating = self.state.read(cx).settings.is_updating("meetings");
        let weak = cx.entity().downgrade();
        let mut rows = vec![
            SettingRow::new("meetings-llm-provider", t("meetingsAi.llm.provider"))
                .control(
                    Dropdown::new("meetings-llm-provider-dropdown")
                        .options(cloud_provider_options(s))
                        .selected(Some(pid.clone()))
                        .min_width(px(220.))
                        .menu_width(px(260.))
                        .disabled(updating)
                        .on_select(move |v, _, cx| {
                            let v = v.to_string();
                            weak.update(cx, |this, cx| this.set_provider(v, cx)).ok();
                        }),
                )
                .into_any_element(),
        ];

        if provider.is_some_and(|p| p.allow_base_url_edit) || pid == CUSTOM_PROVIDER_ID {
            rows.push(
                SettingRow::new(
                    "meetings-llm-base-url",
                    t("settings.postProcessing.api.baseUrl.title"),
                )
                .stacked()
                .control(Input::new(&self.base_url).small())
                .into_any_element(),
            );
        }

        let key_url = provider_key_url(&pid);
        rows.push(
            SettingRow::new("meetings-llm-api-key", t("meetingsAi.llm.apiKey"))
                .description(t("meetingsAi.llm.apiKeyDescription"))
                .inline_description()
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
                                        .child(Input::new(&self.api_key).small().mask_toggle()),
                                )
                                .child(
                                    Button::new("meetings-llm-test-button")
                                        .secondary()
                                        .pill()
                                        .small()
                                        .label(t("meetingsAi.llm.testConnection"))
                                        .loading(self.testing)
                                        .disabled(self.testing || self.saving_key.is_some())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.test_connection(cx)),
                                        ),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .children(
                                    self.test_result
                                        .as_ref()
                                        .map(|r| result_line("meetings-llm-test-result", r, cx)),
                                )
                                .when_some(key_url, |d, url| {
                                    d.child(
                                        Button::new("meetings-llm-get-key")
                                            .link()
                                            .xsmall()
                                            .label(t_args(
                                                "meetingsAi.llm.getKey",
                                                &[("provider", &label)],
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.open_url(url, cx)
                                            })),
                                    )
                                }),
                        ),
                )
                .into_any_element(),
        );

        rows.push(self.model_row(s, cx));
        rows
    }

    fn model_row(&self, s: &AppSettings, cx: &mut Context<Self>) -> AnyElement {
        let pid = self.synced_provider.clone();
        let tone = design::tone(cx);
        let theme = cx.theme().clone();
        let selected = s.meetings.llm_models.get(&pid).cloned().unwrap_or_default();
        let effective = feature_model(s, Feature::Meetings, &pid);
        let catalog = self.catalogs.get(&pid);
        let fetching = self.fetching.as_ref() == Some(&pid);
        let error = self
            .catalog_error
            .as_ref()
            .filter(|(p, _)| *p == pid)
            .map(|(_, e)| e.clone());
        let in_registry = catalog.is_some_and(|c| c.in_registry);
        let query = self.search.read(cx).value().trim().to_lowercase();
        let all: Vec<&ModelEntry> = catalog
            .map(|c| c.models.iter().collect())
            .unwrap_or_default();
        let shown: Vec<&ModelEntry> = all
            .iter()
            .copied()
            // The model in use always shows, so the choice stays visible.
            .filter(|m| {
                !in_registry || !self.good_only || m.good_for_summaries() || m.id == effective
            })
            .filter(|m| {
                query.is_empty()
                    || m.id.to_lowercase().contains(&query)
                    || m.name.to_lowercase().contains(&query)
            })
            .collect();

        let rows = shown.iter().map(|m| {
            let id = m.id.clone();
            let is_sel = m.id == effective;
            let info = m.info.as_ref();
            let meta: Vec<String> = [
                info.and_then(|i| i.limit.context).map(|c| {
                    t_args("meetingsAi.llm.context", &[("n", &format_context(c))]).to_string()
                }),
                info.and_then(|i| format_price(i.cost)),
            ]
            .into_iter()
            .flatten()
            .collect();
            h_flex()
                .id(SharedString::from(format!("meetings-llm-model-{}", m.id)))
                .w_full()
                .gap_3()
                .px(px(10.))
                .py(px(8.))
                .cursor_pointer()
                .border_b_1()
                .border_color(tone.hairline)
                .when(is_sel, |d| d.bg(tone.accent_soft))
                .when(!is_sel, |d| {
                    d.hover(move |s| s.bg(tone.raised.opacity(0.5)))
                })
                .child(radio(is_sel, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(design::TEXT_BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(m.name.clone()),
                        )
                        .child(
                            design::mono(m.id.clone())
                                .text_size(px(11.))
                                .text_color(tone.text_faint)
                                .truncate(),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .when(m.good_for_summaries(), |d| {
                            d.child(design::chip(t("meetingsAi.llm.goodForSummaries"), true, cx))
                        })
                        .when(info.is_some_and(|i| i.reasoning), |d| {
                            d.child(design::chip(t("meetingsAi.llm.reasoning"), false, cx))
                        })
                        .when(in_registry && info.is_none(), |d| {
                            d.child(design::chip(t("meetingsAi.llm.unknownModel"), false, cx))
                        }),
                )
                .child(
                    v_flex()
                        .w(px(96.))
                        .flex_none()
                        .items_end()
                        .font_family(crate::theme::MONO_FONT)
                        .text_size(px(11.))
                        .text_color(tone.text_muted)
                        .children(meta.into_iter().map(|l| div().child(l))),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_model(id.clone(), cx)))
                .test_support()
                .into_any_element()
        });
        let empty = shown.is_empty().then(|| {
            div()
                .id("meetings-llm-models-empty")
                .p_3()
                .text_size(px(12.))
                .text_color(tone.text_muted)
                .child(if fetching && catalog.is_none() {
                    t("meetingsAi.common.loading")
                } else {
                    t("meetingsAi.llm.noModels")
                })
                .test_support()
        });
        let count = t_args(
            "meetingsAi.llm.modelCount",
            &[
                ("shown", &shown.len().to_string()),
                ("total", &all.len().to_string()),
            ],
        );
        let age = registry_age(catalog.and_then(|c| c.registry_fetched_at), unix_now());
        let weak = cx.entity().downgrade();
        let picker = v_flex()
            .id("meetings-llm-picker")
            .w_full()
            .rounded(px(6.))
            .border_1()
            .border_color(tone.hairline)
            .bg(tone.ink)
            .overflow_hidden()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .p(px(6.))
                    .border_b_1()
                    .border_color(tone.hairline)
                    .child(div().flex_1().child(Input::new(&self.search).small()))
                    .when(in_registry, |d| {
                        d.child(
                            h_flex()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(tone.text_muted)
                                .child(toggle(
                                    "meetings-llm-good-only",
                                    self.good_only,
                                    false,
                                    move |v, _, cx| {
                                        weak.update(cx, |this, cx| {
                                            this.good_only = v;
                                            cx.notify();
                                        })
                                        .ok();
                                    },
                                ))
                                .child(t("meetingsAi.llm.goodOnly")),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("meetings-llm-model-list")
                    .w_full()
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .children(rows)
                    .children(empty),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .px(px(10.))
                    .py(px(6.))
                    .font_family(crate::theme::MONO_FONT)
                    .text_size(px(11.))
                    .text_color(tone.text_faint)
                    .child(div().min_w_0().truncate().child(format!("{age} · {count}")))
                    .child(
                        Button::new("meetings-llm-refresh-registry")
                            .ghost()
                            .xsmall()
                            .icon(icon("refresh-ccw"))
                            .label(t("meetingsAi.llm.refreshModels"))
                            .loading(self.refreshing_registry || fetching)
                            .disabled(self.refreshing_registry)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_registry(cx))),
                    ),
            );
        let fallback = (selected.is_empty() && !effective.is_empty()).then(|| {
            div()
                .id("meetings-llm-model-fallback")
                .text_size(px(12.))
                .text_color(tone.text_muted)
                .child(t_args(
                    "meetingsAi.llm.usingPostProcessModel",
                    &[("model", &effective)],
                ))
                .test_support()
        });
        // The registry's models are shown while the provider's own list
        // can't be read (no key yet): say why.
        let live_error = catalog.and_then(|c| c.live_error.clone()).filter(|_| {
            self.api_keys
                .get(&pid)
                .is_some_and(|k| !k.trim().is_empty())
        });
        let error = error.or(live_error).map(|e| {
            h_flex()
                .id("meetings-llm-models-error")
                .gap_1()
                .text_size(px(12.))
                .text_color(theme.danger)
                .child(icon("circle-alert").xsmall())
                .child(div().min_w_0().truncate().child(e))
                .test_support()
        });
        SettingRow::new("meetings-llm-model", t("meetingsAi.llm.model"))
            .description(if in_registry {
                t("meetingsAi.llm.modelDescription")
            } else {
                t("meetingsAi.llm.modelDescriptionCustom")
            })
            .inline_description()
            .stacked()
            .control(
                v_flex()
                    .w_full()
                    .gap_2()
                    .when(!in_registry, |d| {
                        d.child(Input::new(&self.model_input).small())
                    })
                    .child(picker)
                    .children(fallback)
                    .children(error),
            )
            .into_any_element()
    }

    fn local_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tone = design::tone(cx);
        let c = self.choice(cx);
        let apple_selected = c.provider_id == APPLE_PROVIDER_ID;
        let available = self.apple_available;
        let weak = cx.entity().downgrade();
        let apple = h_flex()
            .id("meetings-apple-intelligence")
            .w_full()
            .gap_3()
            .px(design::SECTION_X)
            .py(px(12.))
            .when(available, |d| {
                d.cursor_pointer().on_click(move |_, _, cx| {
                    weak.update(cx, |this, cx| {
                        this.set_provider(APPLE_PROVIDER_ID.to_string(), cx)
                    })
                    .ok();
                })
            })
            .when(!available, |d| d.opacity(0.55))
            .child(radio(apple_selected, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(design::TEXT_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("meetingsAi.llm.apple")),
                    )
                    .child(div().text_size(px(12.)).text_color(tone.text_muted).child(
                        if available {
                            t("meetingsAi.llm.appleDescription")
                        } else {
                            t("meetingsAi.llm.appleUnavailableHint")
                        },
                    )),
            )
            .when(!available, |d| {
                d.child(design::chip(t("meetingsAi.llm.unavailable"), false, cx))
            })
            .test_support()
            .into_any_element();
        vec![
            div()
                .w_full()
                .px(design::SECTION_X)
                .py(px(10.))
                .text_size(px(12.))
                .text_color(tone.text_muted)
                .child(t("meetingsAi.llm.localDescription"))
                .into_any_element(),
            self.local.clone().into_any_element(),
            apple,
        ]
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
pub enum LocalModelsEvent {
    /// A downloaded model was clicked (only when [`LocalModelsView::selectable`]).
    Select(String),
}

impl EventEmitter<LocalModelsEvent> for LocalModelsView {}

pub struct LocalModelsView {
    backend: SharedMeetingsBackend,
    pub models: Vec<LocalLlmModel>,
    /// Rows have a radio mark and clicking a downloaded one emits
    /// [`LocalModelsEvent::Select`]; rendered as bare rows to embed in a
    /// panel. Off: a standalone group (the post-processing page).
    pub selectable: bool,
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
            selectable: false,
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

    /// The built-in model summaries use when the built-in runtime is
    /// selected: the chosen one if downloaded, else the default.
    fn selected_id(&self, cx: &App) -> Option<String> {
        let s = AppState::global(cx).read(cx).app_settings()?.clone();
        if !is_builtin_provider(&s.meetings.llm_provider_id) {
            return None;
        }
        let chosen = s.meetings.llm_models.get(BUILTIN_LLM_PROVIDER_ID);
        let downloaded = |m: &&LocalLlmModel| m.downloaded;
        self.models
            .iter()
            .filter(downloaded)
            .find(|m| Some(&m.id) == chosen)
            .or_else(|| self.models.iter().filter(downloaded).find(|m| m.is_default))
            .or_else(|| self.models.iter().find(downloaded))
            .map(|m| m.id.clone())
    }

    fn local_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let selected = if self.selectable {
            self.selected_id(cx)
        } else {
            None
        };
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
                    .when(self.selectable, |d| {
                        let pickable = m.downloaded && m.download_progress.is_none();
                        let id = m.id.clone();
                        d.child(
                            div()
                                .id(SharedString::from(format!("local-select-{}", m.id)))
                                .when(!pickable, |d| d.opacity(0.35))
                                .when(pickable, |d| {
                                    d.cursor_pointer().on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(LocalModelsEvent::Select(id.clone()))
                                    }))
                                })
                                .child(radio(selected.as_deref() == Some(m.id.as_str()), cx))
                                .test_support(),
                        )
                    })
                    .child(
                        v_flex()
                            .id(SharedString::from(format!("local-info-{}", m.id)))
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .when(self.selectable && m.downloaded, |d| {
                                let id = m.id.clone();
                                d.cursor_pointer().on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(LocalModelsEvent::Select(id.clone()))
                                }))
                            })
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
        if self.selectable {
            let hairline = design::tone(cx).hairline;
            return div()
                .id("local-models")
                .w_full()
                .children(
                    rows.into_iter()
                        .map(move |r| div().w_full().border_b_1().border_color(hairline).child(r)),
                )
                .test_support();
        }
        div()
            .id("local-models")
            .w_full()
            .child(group(Some(t("meetingsAi.local.heading")), rows, cx))
            .test_support()
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
        let place = SummaryPlace::of(&MeetingsLlmChoice::from_settings(&s).provider_id);
        let mut llm = vec![self.status_line(cx), self.place_row(place, cx)];
        match place {
            SummaryPlace::Cloud => llm.extend(self.cloud_rows(&s, cx)),
            SummaryPlace::Local => llm.extend(self.local_rows(cx)),
        }
        let linear = self.linear_rows(cx);
        v_flex()
            .id("meetings-ai-settings")
            .w_full()
            .gap_6()
            .child(group(Some(t("meetingsAi.llm.heading")), llm, cx))
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
    fn context_and_prices_read_compactly() {
        use super::{format_context, format_price};
        use guidy_core::llm::registry::Cost;
        guidy_i18n::set_language("en");
        assert_eq!(format_context(1_000_000), "1M");
        assert_eq!(format_context(1_048_576), "1.05M");
        assert_eq!(format_context(262_144), "262K");
        let cost = |i, o| {
            Some(Cost {
                input: Some(i),
                output: Some(o),
            })
        };
        assert_eq!(format_price(cost(2.0, 10.0)).as_deref(), Some("$2 / $10"));
        assert_eq!(
            format_price(cost(0.075, 0.3)).as_deref(),
            Some("$0.075 / $0.3")
        );
        assert_eq!(format_price(cost(0.0, 0.0)).as_deref(), Some("free"));
        assert_eq!(format_price(None), None);
    }

    #[test]
    fn registry_age_buckets() {
        use super::registry_age;
        guidy_i18n::set_language("en");
        assert!(registry_age(None, 0).contains("bundled"));
        assert!(registry_age(Some(1000), 1030).contains("just now"));
        assert!(registry_age(Some(0), 600).contains("10 min"));
        assert!(registry_age(Some(0), 3 * 3600).contains("3 h"));
        assert!(registry_age(Some(0), 3 * 86_400).contains("3 d"));
    }

    #[test]
    fn cloud_tab_restores_the_last_cloud_provider() {
        use super::{cloud_provider_id, SummaryPlace};
        let mut s = guidy_core::settings::get_default_settings();
        s.meetings.llm_provider_id = BUILTIN_LLM_PROVIDER_ID.into();
        s.meetings.llm_cloud_provider_id = "groq".into();
        assert_eq!(cloud_provider_id(&s), "groq");
        s.meetings.llm_cloud_provider_id.clear();
        assert_eq!(cloud_provider_id(&s), "anthropic");
        s.meetings.llm_provider_id = "openrouter".into();
        assert_eq!(cloud_provider_id(&s), "openrouter");
        assert_eq!(
            SummaryPlace::of(BUILTIN_LLM_PROVIDER_ID),
            SummaryPlace::Local
        );
        assert_eq!(SummaryPlace::of(APPLE_PROVIDER_ID), SummaryPlace::Local);
        assert_eq!(SummaryPlace::of("openai"), SummaryPlace::Cloud);
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
    use super::{ActiveSummary, AiSettingsView};
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

    fn entry(id: &str, structured: bool) -> guidy_core::llm::registry::ModelEntry {
        guidy_core::llm::registry::ModelEntry {
            id: id.into(),
            name: id.to_uppercase(),
            info: Some(guidy_core::llm::registry::RegistryModel {
                id: id.into(),
                name: id.to_uppercase(),
                structured_output: structured,
                limit: guidy_core::llm::registry::Limits {
                    context: Some(1_000_000),
                    output: None,
                },
                ..Default::default()
            }),
        }
    }

    #[gpui_kit::test]
    fn local_tab_selects_downloaded_models_and_shows_apple(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        mb.data.lock().ai.local_models = vec![
            local("qwen3-4b", true, None, true),
            local("qwen3-8b", false, Some(16), true),
            local("gemma", false, None, false),
        ];
        let fb = crate::testing::FakeBackend::new();
        fb.with_settings(|s| {
            s.meetings.llm_provider_id = "guidy_local".into();
            s.meetings.llm_models.clear();
        });
        setup_with(cx, fb.clone(), mb.clone());
        let (w, view) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "meetings-apple-intelligence"));
        assert!(!exists(cx, w, "meetings-llm-provider"));
        // The default downloaded model is the one in use.
        let active = view.read_with(cx, |v, cx| v.active_summary(cx));
        assert!(matches!(active, ActiveSummary::Ready { ref model, .. } if model == "qwen3-4b"));
        with_window(cx, w, |window, cx| {
            window.click("local-select-qwen3-8b", cx)
        });
        assert_eq!(
            fb.settings()
                .meetings
                .llm_models
                .get("guidy_local")
                .map(String::as_str),
            Some("qwen3-8b")
        );
        // Not downloaded: not selectable.
        with_window(cx, w, |window, cx| window.click("local-select-gemma", cx));
        assert_eq!(
            fb.settings()
                .meetings
                .llm_models
                .get("guidy_local")
                .map(String::as_str),
            Some("qwen3-8b")
        );
        assert!(!exists(cx, w, "meetings-llm-test-button"));
    }

    #[gpui_kit::test]
    fn switching_tabs_keeps_the_last_cloud_provider(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        let fb = crate::testing::FakeBackend::new();
        fb.with_settings(|s| {
            s.meetings.llm_provider_id = "openrouter".into();
            s.meetings.llm_cloud_provider_id = "openrouter".into();
        });
        setup_with(cx, fb.clone(), mb.clone());
        let (w, _) = open(cx, AiSettingsView::new);
        assert!(exists(cx, w, "meetings-llm-provider"));
        assert!(exists(cx, w, "meetings-llm-api-key"));
        assert!(mb.called("llm_model_catalog openrouter"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-tab-local", cx)
        });
        assert_eq!(fb.settings().meetings.llm_provider_id, "guidy_local");
        assert!(!exists(cx, w, "meetings-llm-provider"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-tab-cloud", cx)
        });
        assert_eq!(fb.settings().meetings.llm_provider_id, "openrouter");
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-test-button", cx)
        });
        assert!(mb.called("llm_test_connection openrouter Meetings"));
        assert!(exists(cx, w, "meetings-llm-test-result"));
    }

    #[gpui_kit::test]
    fn cloud_models_come_from_the_catalog(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        mb.data.lock().ai.catalog = crate::meetings_backend::ModelCatalog {
            models: vec![entry("good", true), entry("plain", false)],
            in_registry: true,
            registry_fetched_at: None,
            live_error: None,
        };
        let fb = crate::testing::FakeBackend::new();
        fb.with_settings(|s| {
            s.meetings.llm_provider_id = "anthropic".into();
            s.meetings.llm_models.clear();
            s.post_process_models.clear();
        });
        setup_with(cx, fb.clone(), mb.clone());
        let (w, view) = open(cx, AiSettingsView::new);
        // Only models good for summaries by default.
        assert!(exists(cx, w, "meetings-llm-model-good"));
        assert!(!exists(cx, w, "meetings-llm-model-plain"));
        let active = view.read_with(cx, |v, cx| v.active_summary(cx));
        assert!(matches!(active, ActiveSummary::NotReady(_)), "{active:?}");
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-good-only", cx)
        });
        assert!(exists(cx, w, "meetings-llm-model-plain"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-model-good", cx)
        });
        assert_eq!(
            fb.settings()
                .meetings
                .llm_models
                .get("anthropic")
                .map(String::as_str),
            Some("good")
        );
        view.update(cx, |v, _| {
            v.api_keys.insert("anthropic".into(), "sk-ant".into());
        });
        let active = view.read_with(cx, |v, cx| v.active_summary(cx));
        assert_eq!(
            active,
            ActiveSummary::Ready {
                model: "GOOD".into(),
                via: "Anthropic".into()
            }
        );
        // No key: say so.
        view.update(cx, |v, _| {
            v.api_keys.insert("anthropic".into(), String::new());
        });
        let active = view.read_with(cx, |v, cx| v.active_summary(cx));
        assert!(matches!(active, ActiveSummary::NotReady(ref m) if m.contains("Anthropic")));
        with_window(cx, w, |window, cx| {
            window.click("meetings-llm-refresh-registry", cx)
        });
        assert!(mb.called("refresh_llm_registry"));
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
        assert!(exists(cx, w, "meetings-llm-model-fallback"));
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
