//! Models page: every model Guidy can use, in one place.
//!
//! - Setups: "Dictation", "Meetings on this Mac", "Meetings with cloud AI"
//!   with what they download and one button (from `logic::requirements`),
//!   plus disk usage.
//! - Speech recognition: the speech-to-text catalog (downloaded first, then
//!   the ones recommended for the user's language, then the rest on demand).
//! - Summaries & AI: the built-in LLMs, Apple Intelligence and the cloud
//!   providers with a key.
//! - Meeting speakers: the speaker, separation and segmentation models.
//!
//! Every row says which enabled feature needs it ("Required by Dictation ·
//! Meetings") and drives the shared download queue (progress, cancel,
//! resume, retry, delete).

use crate::logic::languages::{
    capability_languages, filter_languages, language_label, supports_language_code,
};
use crate::logic::models::{filter_models, format_model_size, model_row_status, ModelFilter};
use crate::logic::requirements::{self, Feature, Inputs, Setup, SetupPlan};
use crate::logic::settings_store::ToastKind;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::download_control::{progress_line, DownloadControl, DownloadView};
use crate::ui::kit::{icon, t, t_args, LocalizedPlaceholder, ScrollTrap};
use crate::ui::main_window::Section;
use crate::ui::model_card::{delete_button, required_by_chip, ModelCard, ModelRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants, Toggle};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::{format_bytes, DownloadKind, DownloadSummary, OtherModel};
use guidy_core::managers::model::{ModelInfo, RetiredModel};
use guidy_core::settings::AppSettings;

pub struct ModelsPage {
    state: Entity<AppState>,
    search: Entity<InputState>,
    language_search: Entity<InputState>,
    filter: ModelFilter,
    language_open: bool,
    switching: Option<String>,
    retired: Vec<RetiredModel>,
    deleting_retired: bool,
    /// The whole speech catalog is listed (else downloaded + recommended).
    show_all: bool,
    /// The language popover's list (a scroll region over the page's).
    language_scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl ModelsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.models.searchPlaceholder",
                window,
                cx,
            )
        });
        let language_search = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.general.language.searchPlaceholder",
                window,
                cx,
            )
        });
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, input, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.filter.query = input.read(cx).value().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &language_search,
                window,
                |this, input, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => {
                        let q = input.read(cx).value().to_string();
                        let langs: Vec<_> = capability_languages().collect();
                        if let Some((code, _)) = filter_languages(&langs, &q).first() {
                            this.set_language(Some(code.to_string()), window, cx);
                        }
                    }
                    _ => {}
                },
            ),
        ];
        let mut this = Self {
            state,
            search,
            language_search,
            filter: ModelFilter::default(),
            language_open: false,
            switching: None,
            retired: Vec::new(),
            deleting_retired: false,
            show_all: false,
            language_scroll: ScrollHandle::new(),
            _subs: subs,
        };
        this.load_retired(cx);
        // Fresh numbers each visit (keys and disk change elsewhere).
        this.state.update(cx, |s, cx| {
            s.load_library(cx);
            s.load_disk_usage(cx);
        });
        this
    }

    fn load_retired(&mut self, cx: &mut Context<Self>) {
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let retired = cx
                .background_spawn(async move { backend.get_retired_models() })
                .await;
            this.update(cx, |this, cx| {
                this.retired = retired;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_language(&mut self, code: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.language = code;
        self.language_open = false;
        self.language_search
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn delete_retired(&mut self, cx: &mut Context<Self>) {
        let current = self.state.read(cx).models.current_model.clone();
        let targets: Vec<String> = self
            .retired
            .iter()
            .filter(|m| m.id != current)
            .map(|m| m.id.clone())
            .collect();
        self.deleting_retired = true;
        cx.notify();
        let backend = self.state.read(cx).backend.clone();
        let total = targets.len();
        cx.spawn(async move |this, cx| {
            let (deleted, remaining) = cx
                .background_spawn(async move {
                    let deleted = targets
                        .iter()
                        .filter(|id| backend.delete_retired_model(id).is_ok())
                        .count();
                    (deleted, backend.get_retired_models())
                })
                .await;
            this.update(cx, |this, cx| {
                this.retired = remaining;
                this.deleting_retired = false;
                let ok = deleted == total;
                this.state.update(cx, |s, cx| {
                    if ok {
                        s.toast_kind(
                            ToastKind::Success,
                            t("settings.models.retiredNotice.deleted"),
                            cx,
                        )
                    } else {
                        s.toast_error(t("settings.models.retiredNotice.deleteFailed"), cx)
                    }
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Confirm, then delete a model of any kind (`id` is a tracker id).
    /// Warns when an enabled feature needs it.
    fn confirm_delete(
        id: String,
        name: String,
        required_by: Vec<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let stt = DownloadKind::parse(&id)
            .filter(|(k, _)| *k == DownloadKind::Stt)
            .map(|(_, key)| key.to_string());
        let mut message =
            t_args("settings.models.deleteConfirm", &[("modelName", &name)]).to_string();
        if !required_by.is_empty() {
            let list = required_by
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            message = guidy_i18n::t_args(
                "models.deleteRequiredWarning",
                &[("modelName", &name), ("features", &list)],
            );
        }
        window.open_alert_dialog(cx, move |alert, _, _| {
            let id = id.clone();
            let stt = stt.clone();
            alert
                .title(t("settings.models.deleteTitle"))
                .description(message.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    let id = id.clone();
                    let stt = stt.clone();
                    AppState::global(cx).update(cx, |s, cx| match &stt {
                        Some(model_id) => s.delete_model(model_id, cx),
                        None => s.delete_tracked_model(&id, cx),
                    });
                    true
                })
        });
    }

    fn render_retired_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let current = &self.state.read(cx).models.current_model;
        let retired: Vec<&RetiredModel> =
            self.retired.iter().filter(|m| &m.id != current).collect();
        if retired.is_empty() {
            return None;
        }
        let size = format_model_size(Some(retired.iter().map(|m| m.size_mb as f64).sum()));
        let theme = cx.theme();
        Some(
            design::notice(theme.warning)
                .id("retired-notice")
                .flex()
                .items_center()
                .gap_3()
                .child(icon("triangle-alert").text_color(theme.warning))
                .child(
                    v_flex()
                        .flex_1()
                        .child(
                            div()
                                .text_size(design::TEXT_BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .child(t("settings.models.retiredNotice.title")),
                        )
                        .child(
                            div()
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(theme.muted_foreground)
                                .child(t_args(
                                    "settings.models.retiredNotice.description",
                                    &[("size", &size)],
                                )),
                        ),
                )
                .child(
                    Button::new("retired-delete")
                        .warning()
                        .small()
                        .icon(icon("trash"))
                        .loading(self.deleting_retired)
                        .label(t("settings.models.retiredNotice.delete"))
                        .disabled(self.deleting_retired)
                        .on_click(cx.listener(|this, _, _, cx| this.delete_retired(cx))),
                )
                .test_support()
                .into_any_element(),
        )
    }

    fn render_language_filter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label: SharedString = match &self.filter.language {
            Some(code) => language_label(code).unwrap_or(code).to_string().into(),
            None => t("settings.models.filters.allLanguages"),
        };
        let langs: Vec<_> = capability_languages().collect();
        let filtered = filter_languages(&langs, &self.language_search.read(cx).value());
        let entity = cx.entity().downgrade();
        let search = self.language_search.clone();
        let language_scroll = self.language_scroll.clone();
        let muted = cx.theme().muted_foreground;
        let hover = cx.theme().accent;
        let selected = self.filter.language.clone();
        Popover::new("model-language-filter")
            .anchor(Anchor::TopRight)
            .open(self.language_open)
            .on_open_change({
                let entity = entity.clone();
                move |open, window, cx| {
                    let open = *open;
                    entity
                        .update(cx, |this, cx| {
                            this.language_open = open;
                            if open {
                                let h = this.language_search.focus_handle(cx);
                                window.focus(&h, cx);
                            }
                            cx.notify();
                        })
                        .ok();
                }
            })
            .trigger(
                Button::new("language-filter-trigger")
                    .ghost()
                    .small()
                    .icon(icon("globe"))
                    .label(label)
                    .dropdown_caret(true),
            )
            .content(move |_, _, _| {
                let entity = entity.clone();
                let row =
                    |id: SharedString, label: SharedString, code: Option<String>, active: bool| {
                        let entity = entity.clone();
                        div()
                            .id(id)
                            .px_2()
                            .py_1()
                            .rounded(design::RADIUS_CHIP)
                            .text_size(design::TEXT_BODY)
                            .when(active, |d| d.font_weight(FontWeight::MEDIUM))
                            .hover(move |s| s.bg(hover))
                            .child(label)
                            .on_click(move |_, window, cx| {
                                let code = code.clone();
                                entity
                                    .update(cx, |this, cx| this.set_language(code, window, cx))
                                    .ok();
                            })
                    };
                v_flex()
                    .w(dp(240.))
                    .gap_1()
                    .child(Input::new(&search).small().cleanable(true))
                    .child(
                        v_flex()
                            .id("model-language-list")
                            .max_h(dp(240.))
                            .overflow_y_scroll()
                            .scroll_trap(&language_scroll)
                            .child(row(
                                "lang-all".into(),
                                t("settings.models.filters.allLanguages"),
                                None,
                                selected.is_none(),
                            ))
                            .children(filtered.iter().map(|(code, label)| {
                                row(
                                    SharedString::from(format!("lang-filter-{code}")),
                                    SharedString::from(*label),
                                    Some(code.to_string()),
                                    selected.as_deref() == Some(*code),
                                )
                            }))
                            .when(filtered.is_empty(), |d| {
                                d.child(
                                    div()
                                        .p_2()
                                        .text_size(design::TEXT_BODY)
                                        .text_color(muted)
                                        .child(t("settings.general.language.noResults")),
                                )
                            }),
                    )
            })
    }
}

/// Speech models beyond the downloaded and recommended ones are listed
/// whole up to this many; more fold behind "Show All Models".
const FOLD_AFTER: usize = 6;

/// Feature names for a model's "Required by" chip.
fn feature_labels(features: &[Feature]) -> Vec<SharedString> {
    features.iter().map(|f| t(f.label_key())).collect()
}

/// A group: a header line (title left, extras right) over a panel of rows
/// separated by hairlines.
fn group(
    id: &str,
    title: SharedString,
    extras: Option<AnyElement>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> Stateful<Div> {
    let hairline = design::tone(cx).hairline;
    let count = rows.len();
    v_flex()
        .id(SharedString::from(id.to_string()))
        .w_full()
        .gap(design::HEADER_GAP)
        .child(
            h_flex()
                .w_full()
                .min_h(dp(22.))
                .justify_between()
                .gap(dp(8.))
                .child(design::section_header(title, cx))
                .children(extras),
        )
        .child(
            design::panel(cx).children(rows.into_iter().enumerate().map(move |(i, row)| {
                div()
                    .w_full()
                    .when(i + 1 < count, |d| d.border_b_1().border_color(hairline))
                    .child(row)
            })),
        )
}

/// A muted label row inside a panel ("Recommended for German").
fn sub_header(text: SharedString, cx: &App) -> AnyElement {
    // A plain section header (no band), like the page's group headers.
    div()
        .w_full()
        .px(design::SECTION_X)
        .pt(dp(10.))
        .pb(dp(4.))
        .child(design::section_header(text, cx))
        .into_any_element()
}

/// The language plans recommend for: the transcription language when the
/// user set one, else the app language.
fn plan_language(settings: &AppSettings) -> String {
    let lang = settings.selected_language.trim();
    if lang.is_empty() || lang == "auto" {
        guidy_i18n::language()
    } else {
        lang.to_string()
    }
}

impl Render for ModelsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("models-scroll", self.render_body(window, cx))
    }
}

impl crate::ui::pages::Page for ModelsPage {
    fn title(&self, _cx: &App) -> Option<SharedString> {
        Some(t("models.title"))
    }

    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(
            h_flex()
                .gap(dp(6.))
                .child(
                    div().id("models-search").w(dp(180.)).child(
                        Input::new(&self.search).small().cleanable(true).prefix(
                            icon("search")
                                .xsmall()
                                .text_color(design::tone(cx).text_muted),
                        ),
                    ),
                )
                .child(self.render_language_filter(cx))
                .into_any_element(),
        )
    }
}

impl ModelsPage {
    fn render_body(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let st = self.state.read(cx);
        if st.models.loading && !st.models.initialized {
            return self.render_skeleton(cx);
        }
        // A snapshot, so rendering can borrow `cx` mutably.
        let snap = st.models.clone();
        let m = &snap;
        let settings = st.app_settings().cloned().unwrap_or_default();
        let language = plan_language(&settings);
        let meetings_supported = st.meetings_supported;
        let inputs = Inputs {
            stt_models: &m.models,
            other_models: &m.other,
            settings: &settings,
            providers_with_keys: &m.providers_with_keys,
            apple_intelligence: m.apple_intelligence,
            language: &language,
            ram_gb: m.ram_gb,
        };
        let error = m.error.clone();
        let setups = self.render_setups(m, &inputs, meetings_supported, cx);
        let speech = self.render_speech(m, &inputs, &settings, meetings_supported, cx);
        let ai = self.render_ai(m, &inputs, cx);
        let speakers = meetings_supported.then(|| self.render_speakers(m, &inputs, cx));
        v_flex()
            .id("page-models")
            .w_full()
            .min_w_0()
            .gap(design::GROUP_GAP)
            .when_some(error, |d, e| d.child(self.render_error(e, cx)))
            .children(self.render_retired_notice(cx))
            .child(setups)
            .child(speech)
            .child(ai)
            .children(speakers)
            .test_support()
            .into_any_element()
    }

    fn render_skeleton(&self, cx: &App) -> AnyElement {
        let rows = (0..6)
            .map(|i| {
                h_flex()
                    .id(("model-skeleton", i as usize))
                    .w_full()
                    .h(dp(44.))
                    .px(design::SECTION_X)
                    .gap(dp(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap(dp(6.))
                            .child(Skeleton::new().w(dp(160.)).h(dp(10.)))
                            .child(Skeleton::new().secondary().w(dp(280.)).h(dp(8.))),
                    )
                    .child(Skeleton::new().w(dp(72.)).h(dp(18.)))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("page-models")
            .w_full()
            .gap(design::GROUP_GAP)
            .child(group(
                "models-loading",
                t("models.sections.speech"),
                None,
                rows,
                cx,
            ))
            .test_support()
            .into_any_element()
    }

    fn render_error(&self, error: String, cx: &mut Context<Self>) -> AnyElement {
        let danger = cx.theme().danger;
        design::notice(danger)
            .id("models-error")
            .flex()
            .items_center()
            .gap(dp(8.))
            .child(icon("circle-alert").small().text_color(danger))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(design::TEXT_BODY)
                    .child(error),
            )
            .child(
                Button::new("models-error-dismiss")
                    .ghost()
                    .xsmall()
                    .icon(icon("x"))
                    .tooltip(t("common.close"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.update(cx, |s, cx| {
                            s.models.error = None;
                            cx.notify();
                        });
                    })),
            )
            .test_support()
            .into_any_element()
    }

    // -- setups ---------------------------------------------------------------

    fn render_setups(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        inputs: &Inputs,
        meetings_supported: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let disk: Option<SharedString> = m.disk.map(|d| match d.free_bytes {
            Some(free) => t_args(
                "models.diskUsage",
                &[
                    ("used", &format_bytes(d.models_bytes)),
                    ("free", &format_bytes(free)),
                ],
            ),
            None => t_args(
                "models.diskUsageNoFree",
                &[("used", &format_bytes(d.models_bytes))],
            ),
        });
        let extras = disk.map(|text| {
            h_flex()
                .id("models-disk-usage")
                .gap(dp(5.))
                .text_color(tone.text_muted)
                .child(icon("hard-drive").xsmall())
                .child(design::mono(text).text_size(design::TEXT_SECONDARY))
                .test_support()
                .into_any_element()
        });
        let rows = Setup::ALL
            .into_iter()
            .filter(|s| meetings_supported || *s == Setup::Dictation)
            .map(|which| {
                let plan = requirements::setup(inputs, which);
                self.setup_row(m, plan, cx)
            })
            .collect();
        group("models-setups", t("models.setups.title"), extras, rows, cx).into_any_element()
    }

    fn setup_row(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        plan: SetupPlan,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let key = plan.setup.key();
        let (ic, title) = match plan.setup {
            Setup::Dictation => ("mic", t("models.setups.dictation.title")),
            Setup::MeetingsOnThisMac => ("cpu", t("models.setups.meetingsLocal.title")),
            Setup::MeetingsWithCloudAi => ("cloud", t("models.setups.meetingsCloud.title")),
        };
        let mut parts: Vec<String> = plan.models.iter().map(|c| c.name.clone()).collect();
        if let Some(cloud) = &plan.cloud {
            parts.push(if cloud.name.is_empty() {
                guidy_i18n::t("models.setups.cloudProvider")
            } else {
                cloud.name.clone()
            });
        }
        let detail = parts.join(" · ");
        let items: Vec<_> = plan
            .models
            .iter()
            .filter_map(|c| m.download(&c.id).cloned())
            .filter(|i| i.state.is_active())
            .collect();
        let summary = DownloadSummary::of(&items);
        let mut row = ModelRow::new(format!("setup-{key}"), title)
            .element_id(format!("setup-{key}"))
            .leading(icon(ic).small().text_color(tone.text_muted))
            .detail(detail);
        if plan.total_bytes() > 0 {
            row = row.size(format_bytes(plan.total_bytes()));
        }
        if summary.active > 0 {
            let view = DownloadView::Downloading {
                fraction: summary.percent().map(|p| p as f32 / 100.0).or(Some(0.0)),
                percent: summary.percent(),
                eta_secs: None,
            };
            let tone = design::tone(cx);
            let pct = summary
                .percent()
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "…".into());
            let fraction = match view {
                DownloadView::Downloading { fraction, .. } => fraction,
                _ => None,
            };
            row = row.trailing(
                h_flex()
                    .gap(dp(8.))
                    .child(div().w(dp(72.)).child(design::track(
                        SharedString::from(format!("setup-track-{key}")),
                        fraction,
                        cx,
                    )))
                    .child(
                        design::mono(pct)
                            .min_w(dp(36.))
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted),
                    ),
            );
        } else if plan.ready() {
            row = row.trailing(
                h_flex()
                    .id(SharedString::from(format!("setup-ready-{key}")))
                    .gap(dp(4.))
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(icon("circle-check").small().text_color(cx.theme().success))
                    .child(t("models.setups.ready")),
            );
        } else {
            let ids = plan.download_ids();
            if !ids.is_empty() {
                let dictation = plan.setup == Setup::Dictation;
                let label = t_args(
                    "models.actions.downloadSize",
                    &[("size", &format_bytes(plan.download_bytes()))],
                );
                row = row.trailing(
                    Button::new(SharedString::from(format!("setup-download-{key}")))
                        .secondary()
                        .small()
                        .icon(icon("download"))
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let ids = ids.clone();
                            this.state.update(cx, |s, cx| {
                                if dictation {
                                    for id in &ids {
                                        if let Some((DownloadKind::Stt, key)) =
                                            DownloadKind::parse(id)
                                        {
                                            s.select_on_download.insert(key.to_string());
                                        }
                                    }
                                }
                                s.request_downloads(ids, cx);
                            });
                        })),
                );
            } else if plan.setup == Setup::Dictation {
                // On disk but not selected: use it.
                if let Some(model) = plan.models.first() {
                    let model_id = model.id.trim_start_matches("stt:").to_string();
                    row = row.trailing(
                        Button::new("setup-use-dictation")
                            .secondary()
                            .small()
                            .label(t("models.actions.useForDictation"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = model_id.clone();
                                this.state.update(cx, |s, cx| s.use_for_dictation(&id, cx));
                            })),
                    );
                }
            }
            if plan.needs_key() {
                row = row.trailing(
                    Button::new(SharedString::from(format!("setup-key-{key}")))
                        .ghost()
                        .small()
                        .icon(icon("key-round"))
                        .label(t("models.actions.addKey"))
                        .on_click(|_, _, cx| {
                            crate::windows::show_section(Section::PostProcessing, cx)
                        }),
                );
            }
        }
        row.into_any_element()
    }

    // -- speech recognition ------------------------------------------------------

    fn render_speech(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        inputs: &Inputs,
        settings: &AppSettings,
        meetings_supported: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let debug = settings.debug_mode;
        let meetings_on = meetings_supported && settings.meetings.enabled;
        let filtering = !self.filter.query.trim().is_empty()
            || self.filter.language.is_some()
            || self.filter.streaming
            || self.filter.translation;
        let filtered = filter_models(&m.models, &self.filter);
        let lang = inputs.language.split(['-', '_']).next().unwrap_or("en");
        let suggested_id = requirements::recommended_stt(inputs).map(|x| x.id.clone());

        let busy = |model: &ModelInfo| {
            m.stt_download(&model.id)
                .is_some_and(|d| d.state.needs_attention())
                || m.downloading.contains(&model.id)
                || model.partial_size > 0
        };
        let mut mine: Vec<&ModelInfo> = filtered
            .iter()
            .copied()
            .filter(|x| x.is_downloaded || x.is_custom || busy(x))
            .collect();
        mine.sort_by_key(|x| {
            if x.id == m.current_model {
                0
            } else if !x.is_downloaded {
                1
            } else if x.is_custom {
                3
            } else {
                2
            }
        });
        let others: Vec<&ModelInfo> = filtered
            .iter()
            .copied()
            .filter(|x| !mine.iter().any(|y| y.id == x.id))
            .collect();
        let (suggested, rest): (Vec<&ModelInfo>, Vec<&ModelInfo>) =
            others.into_iter().partition(|x| {
                !filtering
                    && (suggested_id.as_deref() == Some(x.id.as_str())
                        || (x.is_recommended
                            && supports_language_code(&x.supported_languages, lang)))
            });

        let entity = cx.entity().downgrade();
        let row = |model: &ModelInfo, _cx: &mut Context<Self>| -> AnyElement {
            let item = m.stt_download(&model.id).cloned();
            let mut status = model_row_status(
                model,
                item.as_ref(),
                m.verifying.contains(&model.id),
                self.switching.as_deref(),
                &m.current_model,
            );
            let progress = m.progress.get(&model.id).map(|p| p.percentage);
            if m.downloading.contains(&model.id) && !status.is_busy() && !model.is_downloaded {
                status = crate::logic::models::ModelCardStatus::Downloading;
            }
            let tracker_id = guidy_core::downloads::stt_id(&model.id);
            let features = feature_labels(&requirements::required_by(inputs, &tracker_id));
            let name = crate::ui::kit::model_name(model);
            let entity = entity.clone();
            let features_for_delete = features.clone();
            let is_meetings_model =
                requirements::required_by(inputs, &tracker_id).contains(&Feature::MeetingNotes);
            let mut card = ModelCard::new(model.clone(), status)
                .compact(true)
                .debug_mode(debug)
                .progress(progress, None)
                .download(item)
                .required_by(features)
                .meetings_model(is_meetings_model)
                .show_recommended(true)
                .on_download(|id, _, cx| {
                    let id = id.to_string();
                    AppState::global(cx).update(cx, |s, cx| s.download_model(&id, cx));
                })
                .on_cancel(|id, _, cx| {
                    let id = id.to_string();
                    AppState::global(cx).update(cx, |s, cx| s.cancel_download(&id, cx));
                })
                .on_delete(move |id, window, cx| {
                    Self::confirm_delete(
                        guidy_core::downloads::stt_id(id),
                        name.clone(),
                        features_for_delete.clone(),
                        window,
                        cx,
                    );
                })
                .on_select(move |id, _, cx| {
                    let id = id.to_string();
                    entity
                        .update(cx, |this, cx| {
                            this.switching = Some(id.clone());
                            cx.notify();
                            let weak = cx.entity().downgrade();
                            this.state.update(cx, |s, cx| {
                                s.select_model(&id, cx, move |_, _, cx| {
                                    weak.update(cx, |this, cx| {
                                        this.switching = None;
                                        cx.notify();
                                    })
                                    .ok();
                                });
                            });
                        })
                        .ok();
                });
            if meetings_on {
                card = card.on_use_for_meetings(|id, _, cx| {
                    let id = id.to_string();
                    AppState::global(cx).update(cx, |s, cx| s.use_for_meetings(&id, cx));
                });
            }
            card.into_any_element()
        };

        let mut rows: Vec<AnyElement> = Vec::new();
        for model in &mine {
            rows.push(row(model, cx));
        }
        if !suggested.is_empty() {
            let label = language_label(lang)
                .map(|l| t_args("models.speech.recommendedFor", &[("language", l)]))
                .unwrap_or_else(|| t("models.speech.recommended"));
            rows.push(sub_header(label, cx));
            for model in &suggested {
                rows.push(row(model, cx));
            }
        }
        let rest_count = rest.len();
        // A short catalog is listed whole; a long one folds behind a button.
        let fold = rest_count > FOLD_AFTER;
        if filtering || self.show_all || !fold {
            if !rest.is_empty() && !filtering && (!mine.is_empty() || !suggested.is_empty()) {
                rows.push(sub_header(t("models.speech.all"), cx));
            }
            for model in &rest {
                rows.push(row(model, cx));
            }
        }
        if !filtering && fold {
            let expanded = self.show_all;
            let label = if expanded {
                t("models.speech.showFewer")
            } else {
                t_args(
                    "models.speech.showAll",
                    &[("count", &rest_count.to_string())],
                )
            };
            rows.push(
                h_flex()
                    .w_full()
                    .px(design::SECTION_X)
                    .py(dp(4.))
                    .child(
                        Button::new("models-show-all")
                            .ghost()
                            .xsmall()
                            .icon(icon(if expanded {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            }))
                            .label(label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = !this.show_all;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .id("models-no-match")
                    .p(dp(12.))
                    .text_size(design::TEXT_BODY)
                    .text_color(design::tone(cx).text_muted)
                    .child(t("settings.models.noModelsMatch"))
                    .into_any_element(),
            );
        }

        let rescanning = m.rescanning;
        let tone = design::tone(cx);
        let extras = h_flex()
            .gap_1()
            .child(
                Toggle::new("filter-streaming")
                    .small()
                    .gap(dp(4.))
                    .px(dp(6.))
                    .icon(icon("audio-lines").xsmall())
                    .label(t("modelSelector.streaming"))
                    .checked(self.filter.streaming)
                    .tooltip(t("settings.models.filters.streaming"))
                    .on_click(cx.listener(|this, v: &bool, _, cx| {
                        this.filter.streaming = *v;
                        cx.notify();
                    })),
            )
            .child(
                Toggle::new("filter-translation")
                    .small()
                    .gap(dp(4.))
                    .px(dp(6.))
                    .icon(icon("languages").xsmall())
                    .label(t("modelSelector.capabilities.translate"))
                    .checked(self.filter.translation)
                    .tooltip(t("settings.models.filters.translation"))
                    .on_click(cx.listener(|this, v: &bool, _, cx| {
                        this.filter.translation = *v;
                        cx.notify();
                    })),
            )
            .child(div().w(px(1.)).h(dp(14.)).bg(tone.hairline))
            .child(
                Button::new("models-rescan")
                    .ghost()
                    .xsmall()
                    .icon(icon("refresh-cw"))
                    .tooltip(t("settings.models.rescan.tooltip"))
                    .loading(rescanning)
                    .disabled(rescanning)
                    .on_click(|_, _, cx| {
                        AppState::global(cx).update(cx, |s, cx| s.rescan_models(cx))
                    }),
            )
            .into_any_element();
        group(
            "models-speech",
            t("models.sections.speech"),
            Some(extras),
            rows,
            cx,
        )
        .into_any_element()
    }

    // -- summaries & AI ------------------------------------------------------------

    /// A row for an LLM or speaker model: download control or delete.
    fn other_row(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        model: &OtherModel,
        detail: SharedString,
        inputs: &Inputs,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let features = feature_labels(&requirements::required_by(inputs, &model.id));
        let mut row = ModelRow::new(model.id.clone(), model.name.clone())
            .detail(detail)
            .size(format_bytes(model.size_bytes));
        if model.installed && !features.is_empty() {
            row = row.leading(icon("check").small().text_color(tone.accent_text));
        }
        if let Some(chip) = required_by_chip(&features, cx) {
            row = row.badge(chip);
        }
        if model.is_default && !model.installed && model.kind == DownloadKind::Llm {
            row = row.badge(design::chip(t("onboarding.recommended"), false, cx));
        }
        let item = m.download(&model.id).cloned();
        let view = DownloadView::of(
            item.as_ref(),
            model.installed,
            model.size_bytes,
            model.partial_bytes,
        );
        if view.is_busy() {
            row = row.trailing(progress_line(&model.id, &view, dp(72.), cx));
        } else if model.installed {
            let id = model.id.clone();
            let name = model.name.clone();
            row = row.trailing(delete_button(&model.id, &model.name, move |window, cx| {
                Self::confirm_delete(id.clone(), name.clone(), features.clone(), window, cx)
            }));
        } else {
            row = row.trailing(
                DownloadControl::new(model.id.clone())
                    .installed(false)
                    .size(model.size_bytes)
                    .label_size(false)
                    .partial(model.partial_bytes),
            );
            if model.partial_bytes > 0 {
                let id = model.id.clone();
                let name = model.name.clone();
                row = row.trailing(delete_button(&model.id, &model.name, move |window, cx| {
                    Self::confirm_delete(id.clone(), name.clone(), Vec::new(), window, cx)
                }));
            }
        }
        row.into_any_element()
    }

    fn render_ai(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        inputs: &Inputs,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let llms: Vec<OtherModel> = m
            .other
            .iter()
            .filter(|x| x.kind == DownloadKind::Llm)
            .cloned()
            .collect();
        let mut rows: Vec<AnyElement> = Vec::new();
        for model in &llms {
            let mut detail = model.description.clone();
            if let Some(gb) = model
                .recommended_min_ram_gb
                .filter(|_| !model.fits_this_mac)
            {
                detail = format!(
                    "{detail} · {}",
                    guidy_i18n::t_args("models.ai.needsMemory", &[("gb", &gb.to_string())])
                );
            }
            rows.push(self.other_row(m, model, detail.into(), inputs, cx));
        }

        // Apple Intelligence: built in, nothing to download.
        let ai = m.apple_intelligence;
        let ai_features = feature_labels(&requirements::required_by(
            inputs,
            guidy_core::settings::APPLE_INTELLIGENCE_PROVIDER_ID,
        ));
        let mut ai_row = ModelRow::new("apple-intelligence", "Apple Intelligence")
            .detail(if ai {
                t("models.ai.appleAvailable")
            } else {
                t("models.ai.appleUnavailable")
            })
            .size("—")
            .dimmed(!ai)
            .trailing(
                div()
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(if ai {
                        t("models.ai.builtIn")
                    } else {
                        t("models.ai.unavailable")
                    }),
            );
        if let Some(chip) = required_by_chip(&ai_features, cx) {
            ai_row = ai_row
                .badge(chip)
                .leading(icon("check").small().text_color(tone.accent_text));
        }
        rows.push(ai_row.into_any_element());

        // Cloud providers with a key: read-only, keys live in Post Process.
        let settings = inputs.settings;
        let mut any_cloud = false;
        for p in settings
            .post_process_providers
            .iter()
            .filter(|p| m.providers_with_keys.contains(&p.id))
        {
            any_cloud = true;
            let features = feature_labels(&requirements::required_by(inputs, &p.id));
            let mut row = ModelRow::new(format!("cloud-{}", p.id), p.label.clone())
                .detail(t("models.ai.keySaved"))
                .size("—")
                .trailing(
                    Button::new(SharedString::from(format!("cloud-edit-{}", p.id)))
                        .ghost()
                        .small()
                        .label(t("models.actions.edit"))
                        .on_click(|_, _, cx| {
                            crate::windows::show_section(Section::PostProcessing, cx)
                        }),
                );
            if let Some(chip) = required_by_chip(&features, cx) {
                row = row
                    .badge(chip)
                    .leading(icon("check").small().text_color(tone.accent_text));
            }
            rows.push(row.into_any_element());
        }
        if !any_cloud {
            rows.push(
                ModelRow::new("cloud-providers", t("models.ai.cloudTitle"))
                    .leading(icon("cloud").small().text_color(tone.text_muted))
                    .detail(t("models.ai.cloudDetail"))
                    .trailing(
                        Button::new("cloud-add-key")
                            .ghost()
                            .small()
                            .icon(icon("key-round"))
                            .label(t("models.actions.addKey"))
                            .on_click(|_, _, cx| {
                                crate::windows::show_section(Section::PostProcessing, cx)
                            }),
                    )
                    .into_any_element(),
            );
        }
        group("models-ai", t("models.sections.ai"), None, rows, cx).into_any_element()
    }

    // -- meeting speakers -----------------------------------------------------------

    fn render_speakers(
        &self,
        m: &crate::logic::models_store::ModelsStore,
        inputs: &Inputs,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let speakers: Vec<OtherModel> = m
            .other
            .iter()
            .filter(|x| x.kind != DownloadKind::Llm)
            .cloned()
            .collect();
        let rows = speakers
            .iter()
            .map(|model| {
                let detail = match model.kind {
                    DownloadKind::Speaker => t("models.speakers.standard"),
                    DownloadKind::Segmentation => t("models.speakers.segmentation"),
                    _ => t("models.speakers.separation"),
                };
                self.other_row(m, model, detail, inputs, cx)
            })
            .collect();
        group(
            "models-speakers",
            t("models.sections.speakers"),
            None,
            rows,
            cx,
        )
        .into_any_element()
    }
}
