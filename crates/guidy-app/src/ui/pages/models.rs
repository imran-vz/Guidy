//! Models page: search, filters, your models / available sections, retired
//! model cleanup.

use crate::logic::languages::{capability_languages, filter_languages, language_label};
use crate::logic::models::{
    filter_models, format_model_size, model_card_status, split_sections, ModelFilter,
};
use crate::logic::settings_store::ToastKind;
use crate::state::AppState;
use crate::ui::design::{self, Pill};
use crate::ui::kit::{icon, page, page_header, spinner, t, t_args};
use crate::ui::model_card::ModelCard;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants, Toggle};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::model::RetiredModel;
use std::collections::HashSet;

pub struct ModelsPage {
    state: Entity<AppState>,
    search: Entity<InputState>,
    language_search: Entity<InputState>,
    filter: ModelFilter,
    language_open: bool,
    switching: Option<String>,
    retired: Vec<RetiredModel>,
    deleting_retired: bool,
    _subs: Vec<Subscription>,
}

impl ModelsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("settings.models.searchPlaceholder"))
        });
        let language_search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("settings.general.language.searchPlaceholder"))
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
            _subs: subs,
        };
        this.load_retired(cx);
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

    fn confirm_delete(
        model_id: String,
        name: String,
        active: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let message = if active {
            t_args(
                "settings.models.deleteActiveConfirm",
                &[("modelName", &name)],
            )
        } else {
            t_args("settings.models.deleteConfirm", &[("modelName", &name)])
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let id = model_id.clone();
            alert
                .title(t("settings.models.deleteTitle"))
                .description(message.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    let id = id.clone();
                    AppState::global(cx).update(cx, |s, cx| s.delete_model(&id, cx));
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
            h_flex()
                .id("retired-notice")
                .w_full()
                .gap_3()
                .pl(px(14.))
                .pr(px(10.))
                .py(px(10.))
                .rounded(design::RADIUS_PANEL)
                .border_1()
                .border_color(theme.warning.opacity(0.3))
                .bg(theme.warning.opacity(0.08))
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
                                .text_size(px(12.))
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
                        .pill()
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
                            .rounded(px(4.))
                            .text_sm()
                            .cursor_pointer()
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
                    .w(px(240.))
                    .gap_1()
                    .child(Input::new(&search).small().cleanable(true))
                    .child(
                        v_flex()
                            .id("model-language-list")
                            .max_h(px(240.))
                            .overflow_y_scroll()
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
                                        .text_sm()
                                        .text_color(muted)
                                        .child(t("settings.general.language.noResults")),
                                )
                            }),
                    )
            })
    }

    fn card(&self, model: &guidy_core::managers::model::ModelInfo, cx: &App) -> ModelCard {
        let st = self.state.read(cx);
        let m = &st.models;
        let status = model_card_status(
            Some(model),
            &model.id,
            &m.verifying,
            &m.downloading,
            self.switching.as_deref(),
            &m.current_model,
        );
        let progress = m.progress.get(&model.id).map(|p| p.percentage);
        let speed = m.stats.get(&model.id).map(|s| s.speed);
        let debug = st.app_settings().map(|s| s.debug_mode).unwrap_or(false);
        let name = crate::ui::kit::model_name(model);
        let current = m.current_model.clone();
        ModelCard::new(model.clone(), status)
            .debug_mode(debug)
            .progress(progress, speed)
            .on_download(|id, _, cx| {
                let id = id.to_string();
                AppState::global(cx).update(cx, |s, cx| s.download_model(&id, cx));
            })
            .on_cancel(|id, _, cx| {
                let id = id.to_string();
                AppState::global(cx).update(cx, |s, cx| s.cancel_download(&id, cx));
            })
            .on_delete(move |id, window, cx| {
                Self::confirm_delete(id.to_string(), name.clone(), id == current, window, cx);
            })
    }
}

impl Render for ModelsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let st = self.state.read(cx);
        let theme = cx.theme().clone();
        let m = &st.models;
        if m.loading && !m.initialized {
            return page()
                .id("page-models")
                .child(
                    div()
                        .p_8()
                        .flex()
                        .justify_center()
                        .child(spinner("models-loading", icon("loader-circle").large())),
                )
                .into_any_element();
        }
        let filtered = filter_models(&m.models, &self.filter);
        let any = !filtered.is_empty();
        let downloading: HashSet<String> = m.downloading.clone();
        let (downloaded, available) = split_sections(filtered, &downloading, &m.current_model);
        let downloaded: Vec<_> = downloaded.into_iter().cloned().collect();
        let available: Vec<_> = available.into_iter().cloned().collect();
        let rescanning = m.rescanning;

        // Hide the "recommended" badge on models you already have. Every
        // model but the active one is a row in a shared panel.
        let current_model = m.current_model.clone();
        let make_cards = |models: &[guidy_core::managers::model::ModelInfo],
                          show_recommended: bool,
                          cx: &mut Context<Self>|
         -> Vec<(AnyElement, bool)> {
            models
                .iter()
                .map(|model| {
                    let entity = entity.clone();
                    let is_row = model.id != current_model;
                    let card = self
                        .card(model, cx)
                        .compact(true)
                        .show_recommended(show_recommended)
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
                        })
                        .into_any_element();
                    (card, is_row)
                })
                .collect()
        };
        let downloaded_cards = make_cards(&downloaded, false, cx);
        let available_cards = make_cards(&available, true, cx);

        let header = h_flex()
            .w_full()
            .justify_between()
            .child(design::eyebrow(t("settings.models.yourModels"), cx))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("models-rescan")
                            .ghost()
                            .small()
                            .icon(icon("refresh-cw"))
                            .tooltip(t("settings.models.rescan.tooltip"))
                            .loading(rescanning)
                            .disabled(rescanning)
                            .on_click(|_, _, cx| {
                                AppState::global(cx).update(cx, |s, cx| s.rescan_models(cx))
                            }),
                    )
                    .child(div().w(px(1.)).h(px(16.)).bg(design::tone(cx).hairline))
                    .child(
                        Toggle::new("filter-streaming")
                            .small()
                            .icon(icon("audio-lines"))
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
                            .icon(icon("languages"))
                            .checked(self.filter.translation)
                            .tooltip(t("settings.models.filters.translation"))
                            .on_click(cx.listener(|this, v: &bool, _, cx| {
                                this.filter.translation = *v;
                                cx.notify();
                            })),
                    )
                    .child(self.render_language_filter(cx)),
            );

        page()
            .id("page-models")
            .child(page_header(
                t("settings.models.title"),
                Some(t("settings.models.description")),
                cx,
            ))
            .children(self.render_retired_notice(cx))
            .child(
                Input::new(&self.search)
                    .prefix(icon("search").small())
                    .cleanable(true),
            )
            .child(header)
            .child(rows_panel(downloaded_cards, cx))
            .when(!available.is_empty(), |p| {
                p.child(design::eyebrow(t("settings.models.availableModels"), cx))
                    .child(rows_panel(available_cards, cx))
            })
            .when(!any, |p| {
                p.child(
                    div()
                        .p_6()
                        .text_center()
                        .text_size(design::TEXT_BODY)
                        .text_color(theme.muted_foreground)
                        .child(t("settings.models.noModelsMatch")),
                )
            })
            .test_support()
            .into_any_element()
    }
}

/// The active model keeps its own card; the rest share one hairline panel.
fn rows_panel(cards: Vec<(AnyElement, bool)>, cx: &App) -> Div {
    let hairline = design::tone(cx).hairline;
    let mut out = v_flex().w_full().gap(px(10.));
    let mut rows: Vec<AnyElement> = Vec::new();
    let flush = |out: Div, rows: &mut Vec<AnyElement>| {
        if rows.is_empty() {
            return out;
        }
        let count = rows.len();
        out.child(
            design::panel(cx).children(rows.drain(..).enumerate().map(move |(i, row)| {
                div()
                    .w_full()
                    .when(i + 1 < count, |d| d.border_b_1().border_color(hairline))
                    .child(row)
            })),
        )
    };
    for (card, is_row) in cards {
        if is_row {
            rows.push(card);
        } else {
            out = flush(out, &mut rows);
            out = out.child(card);
        }
    }
    flush(out, &mut rows)
}
