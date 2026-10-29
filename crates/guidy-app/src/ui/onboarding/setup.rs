//! Onboarding, step 3: "Choose your setup". For each chosen feature, the
//! models it needs (from `logic::requirements`, via `logic::onboarding`):
//! the speech model (language-aware, "Choose Another…"), and for meeting
//! notes the transcription and speaker models and where summaries are
//! written (Apple Intelligence, this Mac, or a cloud provider with the
//! user's key). The footer totals the download against the free space.
//! Finish queues the downloads and opens the app at once: nothing waits.

use super::assistant::{
    back_button, continue_button, row, row_icon, rows, trailing_text, Assistant,
};
use super::model_picker::ModelPicker;
use crate::logic::languages::{language_label, recognition_language, unique_capability_languages};
use crate::logic::onboarding::{plan_language, Plan, Summaries};
use crate::logic::requirements::Choice;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args, Dropdown, DropdownOption};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::format_bytes;
use guidy_core::settings::{AppSettings, APPLE_INTELLIGENCE_PROVIDER_ID};

pub struct SetupStep {
    state: Entity<AppState>,
    /// Debug preview: nothing changes.
    preview: bool,
    focus: FocusHandle,
    api_key: Entity<InputState>,
    /// The provider the key field is for.
    key_provider: String,
    saving_key: bool,
    testing: bool,
    test_result: Option<Result<String, String>>,
    /// "Choose Another…" is open.
    picker: Option<Entity<ModelPicker>>,
    _subs: Vec<Subscription>,
}

/// Cloud providers summaries can use (Apple Intelligence and a custom
/// endpoint are set up in Settings).
fn cloud_providers(s: &AppSettings) -> Vec<(String, String)> {
    s.post_process_providers
        .iter()
        .filter(|p| p.id != APPLE_INTELLIGENCE_PROVIDER_ID)
        .filter(|p| p.id != crate::logic::flows::CUSTOM_PROVIDER_ID)
        .map(|p| (p.id.clone(), p.label.clone()))
        .collect()
}

fn key_url(provider_id: &str) -> Option<&'static str> {
    crate::logic::meetings::PROVIDER_KEY_URLS
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, url)| *url)
}

impl SetupStep {
    pub fn new(preview: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        use crate::ui::kit::LocalizedPlaceholder;
        let state = AppState::global(cx);
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .localized_placeholder("settings.postProcessing.api.apiKey.placeholder", window, cx)
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&api_key, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = input.read(cx).value().trim().to_string();
                    this.save_key(v, cx);
                }
            }),
        ];
        let key_provider = match &state.read(cx).startup.choices.summaries {
            Some(Summaries::Cloud(id)) => id.clone(),
            _ => String::new(),
        };
        Self {
            state,
            preview,
            focus,
            api_key,
            key_provider,
            saving_key: false,
            testing: false,
            test_result: None,
            picker: None,
            _subs: subs,
        }
    }

    /// The picker sub-pane, when open (tests drive it).
    pub fn picker(&self) -> Option<Entity<ModelPicker>> {
        self.picker.clone()
    }

    fn choose_summaries(&mut self, which: Summaries, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        if let Summaries::Cloud(id) = &which {
            if *id != self.key_provider {
                self.key_provider = id.clone();
                self.test_result = None;
                self.api_key.update(cx, |s, cx| s.set_value("", window, cx));
            }
        }
        self.state.update(cx, |s, cx| {
            s.update_onboarding_choices(|c| c.summaries = Some(which), cx)
        });
    }

    /// The cloud option: the provider last picked, else one with a key,
    /// else the first.
    fn default_cloud(&self, cx: &App) -> Option<Summaries> {
        let st = self.state.read(cx);
        let providers = cloud_providers(st.app_settings()?);
        let keys = &st.models.providers_with_keys;
        let id = Some(self.key_provider.clone())
            .filter(|p| providers.iter().any(|(id, _)| id == p))
            .or_else(|| {
                providers
                    .iter()
                    .find(|(id, _)| keys.contains(id))
                    .map(|(id, _)| id.clone())
            })
            .or_else(|| providers.first().map(|(id, _)| id.clone()))?;
        Some(Summaries::Cloud(id))
    }

    /// Save the typed key in the Keychain (shared with post-processing).
    fn save_key(&mut self, value: String, cx: &mut Context<Self>) {
        if self.preview || value.is_empty() || self.key_provider.is_empty() {
            return;
        }
        self.saving_key = true;
        self.test_result = None;
        let weak = cx.entity().downgrade();
        let provider = self.key_provider.clone();
        self.state.update(cx, |s, cx| {
            s.update_post_process_then("api_key", &provider, value, cx, move |s, r, cx| {
                // The plan reads which providers have a key.
                s.load_library(cx);
                let ok = r.is_ok();
                cx.defer(move |cx| {
                    weak.update(cx, |this, cx| {
                        this.saving_key = false;
                        if !ok {
                            this.test_result =
                                Some(Err(guidy_i18n::t("onboarding.setup.cloud.saveFailed")));
                        }
                        cx.notify();
                    })
                    .ok();
                });
            })
        });
        cx.notify();
    }

    fn test_key(&mut self, cx: &mut Context<Self>) {
        if self.preview || self.key_provider.is_empty() {
            return;
        }
        self.testing = true;
        self.test_result = None;
        let meetings = self.state.read(cx).meetings.clone();
        let provider = self.key_provider.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    meetings.llm_test_connection(&provider, guidy_core::llm::Feature::Meetings)
                })
                .await;
            this.update(cx, |this, cx| {
                this.testing = false;
                this.test_result = Some(r.map_err(|e| crate::meetings_backend::error_message(&e)));
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Open "Choose Another…" for the speech model.
    pub fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        let st = self.state.read(cx);
        let selected = st.onboarding_plan().and_then(|p| p.speech_model);
        let language = st
            .app_settings()
            .map(|s| recognition_language(&plan_language(s, &guidy_i18n::language())));
        let weak = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            ModelPicker::new(
                selected,
                language,
                move |chosen, window, cx| {
                    weak.update(cx, |this, cx| {
                        this.picker = None;
                        if let Some(id) = chosen {
                            this.state.update(cx, |s, cx| {
                                s.update_onboarding_choices(|c| c.speech_model = Some(id), cx)
                            });
                        }
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })
                    .ok();
                },
                window,
                cx,
            )
        });
        self.picker = Some(picker);
        cx.notify();
    }

    fn finish(&mut self, skip: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        // A key typed but not committed yet.
        let typed = self.api_key.read(cx).value().trim().to_string();
        let cloud = matches!(
            self.state.read(cx).startup.choices.summaries,
            Some(Summaries::Cloud(_))
        );
        if !skip && cloud && !typed.is_empty() {
            self.save_key(typed, cx);
        }
        let _ = window;
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.finish_onboarding(skip, cx)));
    }

    /// Size to download, or a "Downloaded" / "Included" chip.
    fn size_cell(&self, c: Option<&Choice>, plus: bool, cx: &App) -> AnyElement {
        match c {
            Some(c) if c.ready => {
                design::chip(t("onboarding.setup.downloaded"), false, cx).into_any_element()
            }
            Some(c) => {
                let size = format_bytes(c.download_bytes);
                trailing_text(if plus { format!("+{size}") } else { size }, cx)
            }
            None => trailing_text("", cx),
        }
    }

    fn speech_description(&self, model_id: &str, cx: &App) -> SharedString {
        let st = self.state.read(cx);
        let Some(m) = st.models.model(model_id) else {
            return SharedString::default();
        };
        let langs = unique_capability_languages(&m.supported_languages);
        let languages = match langs.as_slice() {
            [one] => guidy_i18n::t_args(
                "modelSelector.capabilities.languageOnly",
                &[("language", language_label(one).unwrap_or(one))],
            ),
            _ => guidy_i18n::t_args(
                "modelSelector.capabilities.languageCount",
                &[("total", &langs.len().to_string())],
            ),
        };
        let language = st
            .app_settings()
            .map(|s| recognition_language(&plan_language(s, &guidy_i18n::language())))
            .unwrap_or_else(|| "en".into());
        let label = language_label(&language).unwrap_or("English");
        let speaks =
            crate::logic::languages::supports_language_code(&m.supported_languages, &language);
        if speaks {
            format!(
                "{languages} · {}",
                guidy_i18n::t_args("onboarding.setup.speaks", &[("language", label)])
            )
            .into()
        } else {
            languages.into()
        }
    }

    fn speech_row(&self, plan: &Plan, cx: &mut Context<Self>) -> AnyElement {
        let st = self.state.read(cx);
        let name = plan
            .speech_model
            .as_deref()
            .and_then(|id| st.models.model(id))
            .map(crate::ui::kit::model_name)
            .unwrap_or_else(|| guidy_i18n::t("onboarding.setup.noSpeechModel"));
        let description = plan
            .speech_model
            .as_deref()
            .map(|id| self.speech_description(id, cx))
            .unwrap_or_default();
        let trailing = h_flex()
            .gap(dp(8.))
            .child(self.size_cell(
                plan.speech.as_ref().or(plan.transcription.as_ref()),
                false,
                cx,
            ))
            .child(
                Button::new("setup-choose-speech")
                    .outline()
                    .small()
                    .label(t("onboarding.setup.chooseAnother"))
                    .on_click(cx.listener(|this, _, window, cx| this.open_picker(window, cx))),
            );
        row(
            "setup-speech",
            Some(row_icon("mic", cx)),
            name,
            Some(description),
            Some(trailing.into_any_element()),
            cx,
        )
        .test_support()
        .into_any_element()
    }

    /// Where summaries are written: one row with a segmented control
    /// (Apple Intelligence / On This Mac / Cloud), the pick explained below
    /// its title; the cloud pick adds the provider and key form.
    fn summaries_rows(&self, plan: &Plan, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (ram, other) = {
            let m = &self.state.read(cx).models;
            (m.ram_gb, m.other.clone())
        };
        let mut options: Vec<(&'static str, Summaries, SharedString)> = Vec::new();
        if plan.apple.as_ref().is_some_and(|c| c.available) {
            options.push((
                "summaries-apple",
                Summaries::AppleIntelligence,
                t("onboarding.setup.summaries.apple"),
            ));
        }
        if plan.local_llm.is_some() {
            options.push((
                "summaries-local",
                Summaries::OnThisMac,
                t("onboarding.setup.summaries.local"),
            ));
        }
        let cloud = match &plan.summaries {
            Summaries::Cloud(id) => Some(Summaries::Cloud(id.clone())),
            _ => self.default_cloud(cx),
        };
        if let Some(cloud) = cloud {
            options.push((
                "summaries-cloud",
                cloud,
                t("onboarding.setup.summaries.cloud"),
            ));
        }
        let description: SharedString = match &plan.summaries {
            Summaries::AppleIntelligence => t("onboarding.setup.summaries.appleDescription"),
            Summaries::OnThisMac => match &plan.local_llm {
                Some(llm) if !llm.available => {
                    let need = other
                        .iter()
                        .find(|m| m.id == llm.id)
                        .and_then(|m| m.recommended_min_ram_gb)
                        .unwrap_or(8);
                    t_args(
                        "onboarding.setup.summaries.tooBig",
                        &[
                            ("need", &need.to_string()),
                            ("have", &ram.unwrap_or(0).to_string()),
                        ],
                    )
                }
                Some(llm) if llm.ready => t_args(
                    "onboarding.setup.summaries.localReady",
                    &[("model", &llm.name)],
                ),
                Some(llm) => t_args(
                    "onboarding.setup.summaries.localDescription",
                    &[
                        ("model", &llm.name),
                        ("size", &format_bytes(llm.download_bytes)),
                    ],
                ),
                None => SharedString::default(),
            },
            Summaries::Cloud(_) => t("onboarding.setup.summaries.cloudDescription"),
        };
        let control = options
            .into_iter()
            .fold(design::segmented(cx), |d, (id, which, label)| {
                let selected = plan.summaries == which;
                d.child(
                    design::segment(id, label, selected, cx)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_summaries(which.clone(), window, cx)
                        }))
                        .test_support(),
                )
            });
        let mut out = vec![row(
            "setup-summaries",
            Some(row_icon("sparkles", cx)),
            t("onboarding.setup.summaries.title"),
            Some(description),
            Some(control.into_any_element()),
            cx,
        )
        .test_support()
        .into_any_element()];
        if let Summaries::Cloud(pid) = &plan.summaries {
            out.push(self.cloud_form(pid, cx));
        }
        out
    }

    /// Provider, API key and Test for cloud summaries.
    fn cloud_form(&self, pid: &str, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let st = self.state.read(cx);
        let Some(settings) = st.app_settings() else {
            return div().into_any_element();
        };
        let providers = cloud_providers(settings);
        let label = providers
            .iter()
            .find(|(id, _)| id == pid)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| pid.to_string());
        let saved = st.models.providers_with_keys.iter().any(|p| p == pid);
        let options = providers
            .iter()
            .map(|(id, label)| DropdownOption::new(id.clone(), label.clone()))
            .collect();
        let weak = cx.entity().downgrade();
        let line = h_flex()
            .w_full()
            .gap(dp(8.))
            .child(
                Dropdown::new("setup-cloud-provider")
                    .options(options)
                    .selected(Some(pid.to_string()))
                    .min_width(dp(130.))
                    .menu_width(dp(220.))
                    .on_select(move |v, window, cx| {
                        let v = v.to_string();
                        weak.update(cx, |this, cx| {
                            this.choose_summaries(Summaries::Cloud(v), window, cx)
                        })
                        .ok();
                    }),
            )
            .child(
                div()
                    .id("setup-api-key")
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.api_key).small().mask_toggle())
                    .test_support(),
            )
            .child(
                Button::new("setup-test-key")
                    .outline()
                    .small()
                    .label(t("meetingsAi.llm.testConnection"))
                    .loading(self.testing)
                    .disabled(self.testing || self.saving_key || !saved)
                    .on_click(cx.listener(|this, _, _, cx| this.test_key(cx))),
            );
        let status: AnyElement = match &self.test_result {
            Some(Ok(_)) => h_flex()
                .gap(dp(4.))
                .text_color(tone.accent_text)
                .child(icon("check").size(dp(12.)))
                .child(t("onboarding.setup.cloud.works"))
                .into_any_element(),
            Some(Err(e)) => div()
                .text_color(cx.theme().danger)
                .truncate()
                .child(e.clone())
                .into_any_element(),
            None => div()
                .truncate()
                .child(if saved {
                    t_args("onboarding.setup.cloud.savedHint", &[("provider", &label)])
                } else {
                    t_args("onboarding.setup.cloud.hint", &[("provider", &label)])
                })
                .into_any_element(),
        };
        let url = key_url(pid);
        let help = h_flex()
            .w_full()
            .gap(dp(8.))
            .text_size(design::TEXT_SECONDARY)
            .text_color(tone.text_muted)
            .child(div().flex_1().min_w_0().child(status))
            .when_some(url, |d, url| {
                d.child(
                    Button::new("setup-get-key")
                        .link()
                        .xsmall()
                        .icon(icon("external-link"))
                        .label(t_args("meetingsAi.llm.getKey", &[("provider", &label)]))
                        .on_click(move |_, _, cx| cx.open_url(url)),
                )
            });
        v_flex()
            .id("setup-cloud-form")
            .w_full()
            .gap(dp(6.))
            .pl(dp(34.))
            .pr(design::SECTION_X)
            .py(dp(8.))
            .child(line)
            .child(help)
            .test_support()
            .into_any_element()
    }

    fn meetings_rows(
        &self,
        plan: &Plan,
        dictation: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let st = self.state.read(cx);
        let mut out = Vec::new();
        // Transcription: the dictation model (nothing extra) or its own.
        let transcription_name = plan
            .transcription
            .as_ref()
            .map(|c| {
                plan.speech_model
                    .as_deref()
                    .and_then(|id| st.models.model(id))
                    .filter(|m| guidy_core::downloads::stt_id(&m.id) == c.id)
                    .map(crate::ui::kit::model_name)
                    .unwrap_or_else(|| c.name.clone())
            })
            .unwrap_or_default();
        if dictation {
            let (description, trailing) = if plan.transcription_is_speech {
                (
                    t_args(
                        "onboarding.setup.transcription.same",
                        &[("model", &transcription_name)],
                    ),
                    design::chip(t("onboarding.setup.included"), false, cx).into_any_element(),
                )
            } else {
                (
                    SharedString::from(transcription_name.clone()),
                    self.size_cell(plan.transcription.as_ref(), true, cx),
                )
            };
            out.push(
                row(
                    "setup-transcription",
                    Some(row_icon("audio-lines", cx)),
                    t("onboarding.setup.transcription.title"),
                    Some(description),
                    Some(trailing),
                    cx,
                )
                .test_support()
                .into_any_element(),
            );
        } else {
            // Meetings only: their model is the speech model row.
            out.push(self.speech_row(plan, cx));
        }
        out.push(
            row(
                "setup-speakers",
                Some(row_icon("users", cx)),
                t("onboarding.setup.speakers.title"),
                Some(t("onboarding.setup.speakers.description")),
                Some(self.size_cell(plan.speaker.as_ref(), dictation, cx)),
                cx,
            )
            .test_support()
            .into_any_element(),
        );
        out.extend(self.summaries_rows(plan, cx));
        if !plan.turns.is_empty() {
            let checked = self.state.read(cx).startup.choices.better_turns;
            let ready = plan.turns.iter().all(|c| c.ready);
            let toggle = |this: &mut Self, cx: &mut Context<Self>| {
                if this.preview {
                    return;
                }
                this.state.update(cx, |s, cx| {
                    s.update_onboarding_choices(|c| c.better_turns = !c.better_turns, cx)
                });
            };
            let checkbox = Checkbox::new("setup-turns-check")
                .checked(checked)
                .on_click(cx.listener(move |this, _: &bool, _, cx| {
                    cx.stop_propagation();
                    toggle(this, cx);
                }));
            let trailing = if ready {
                design::chip(t("onboarding.setup.downloaded"), false, cx).into_any_element()
            } else {
                trailing_text(format!("+{}", format_bytes(plan.turns_bytes())), cx)
            };
            out.push(
                row(
                    "setup-turns",
                    Some(checkbox.into_any_element()),
                    t("onboarding.setup.turns.title"),
                    Some(t("onboarding.setup.turns.description")),
                    Some(trailing),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx)))
                .test_support()
                .into_any_element(),
            );
        }
        out
    }

    fn skeleton(&self, cx: &App) -> AnyElement {
        let items = (0..3)
            .map(|i| {
                h_flex()
                    .id(("setup-skeleton", i as usize))
                    .w_full()
                    .h(dp(44.))
                    .px(design::SECTION_X)
                    .gap(dp(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap(dp(6.))
                            .child(Skeleton::new().w(dp(160.)).h(dp(10.)))
                            .child(Skeleton::new().secondary().w(dp(260.)).h(dp(8.))),
                    )
                    .child(Skeleton::new().w(dp(64.)).h(dp(16.)))
                    .into_any_element()
            })
            .collect();
        rows(None, items, cx).into_any_element()
    }
}

impl Render for SetupStep {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(picker) = &self.picker {
            return picker.clone().into_any_element();
        }
        let tone = design::tone(cx);
        let st = self.state.read(cx);
        let choices = st.startup.choices.clone();
        let plan = st.onboarding_plan().filter(|_| st.models.initialized);
        let free = st.models.disk.and_then(|d| d.free_bytes);
        let preview = self.preview;

        let mut assistant = Assistant::new("onboarding-setup", t("onboarding.setup.title"))
            .icon("hard-drive-download")
            .description(t("onboarding.setup.description"))
            .footer_start(back_button("setup-back", move |_, _, cx| {
                if !preview {
                    let state = AppState::global(cx);
                    state.update(cx, |s, cx| s.onboarding_back(cx));
                }
            }));

        let mut can_finish = !preview;
        match &plan {
            None => {
                assistant = assistant.child(self.skeleton(cx));
                can_finish = false;
            }
            Some(plan) => {
                if choices.dictation {
                    assistant = assistant.child(rows(
                        Some(t("onboarding.setup.dictation")),
                        vec![self.speech_row(plan, cx)],
                        cx,
                    ));
                }
                if choices.meetings {
                    let items = self.meetings_rows(plan, choices.dictation, cx);
                    assistant =
                        assistant.child(rows(Some(t("onboarding.setup.meetings")), items, cx));
                }
                let bytes = plan.download_bytes();
                let fits = free.is_none_or(|f| f >= bytes);
                can_finish &= fits;
                let total: SharedString = if bytes == 0 {
                    t("onboarding.setup.nothingToDownload")
                } else {
                    match free {
                        Some(f) => t_args(
                            "onboarding.setup.total",
                            &[("size", &format_bytes(bytes)), ("free", &format_bytes(f))],
                        ),
                        None => t_args(
                            "onboarding.setup.totalNoFree",
                            &[("size", &format_bytes(bytes))],
                        ),
                    }
                };
                assistant = assistant.note(
                    div()
                        .id("setup-total")
                        .truncate()
                        .when(!fits, |d| d.text_color(cx.theme().danger))
                        .child(if fits {
                            total
                        } else {
                            t("onboarding.setup.notEnoughSpace")
                        })
                        .test_support(),
                );
                if bytes > 0 {
                    assistant = assistant.child(
                        h_flex()
                            .id("setup-background-note")
                            .w_full()
                            .px(dp(2.))
                            .gap(dp(6.))
                            .items_start()
                            .text_size(design::TEXT_SECONDARY)
                            .line_height(dp(15.))
                            .text_color(tone.text_muted)
                            .child(
                                icon("download")
                                    .size(dp(12.))
                                    .mt(dp(1.))
                                    .text_color(tone.text_faint),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(t("onboarding.setup.background")),
                            ),
                    );
                }
            }
        }

        let assistant = assistant
            .action(
                Button::new("setup-later")
                    .ghost()
                    .label(t("onboarding.setup.later"))
                    .disabled(preview)
                    .on_click(cx.listener(|this, _, window, cx| this.finish(true, window, cx))),
            )
            .action(continue_button(
                "setup-finish",
                t("onboarding.setup.finish"),
                can_finish,
                cx.listener(move |this, _, window, cx| {
                    if can_finish {
                        this.finish(false, window, cx)
                    }
                }),
            ));
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "enter"
                    && !ev.keystroke.modifiers.modified()
                    && this.focus.is_focused(window)
                    && can_finish
                {
                    this.finish(false, window, cx);
                }
            }))
            .child(assistant)
            .into_any_element()
    }
}
