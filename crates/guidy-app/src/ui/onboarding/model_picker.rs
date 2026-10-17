//! First-run model picker (Handy's `Onboarding`).

use crate::logic::models::{picked_model_ready, picker_groups, ModelCardStatus};
use crate::state::AppState;
use crate::ui::brand::wordmark;
use crate::ui::kit::{icon, t, t_args};
use crate::ui::main_window::MainWindow;
use crate::ui::model_card::ModelCard;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub struct ModelPicker {
    main: WeakEntity<MainWindow>,
    preview: bool,
    state: Entity<AppState>,
    /// The model the user picked (downloading or about to be selected).
    pub selected: Option<String>,
    selecting: bool,
    show_all: bool,
    _subs: Vec<Subscription>,
}

impl ModelPicker {
    pub fn new(
        main: WeakEntity<MainWindow>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe_in(&state, window, |this, _, window, cx| {
            this.maybe_select(window, cx);
            cx.notify();
        })];
        Self {
            main,
            preview,
            state,
            selected: None,
            selecting: false,
            show_all: false,
            _subs: subs,
        }
    }

    /// Once the picked model is downloaded and verified, select it and move on.
    fn maybe_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview || self.selecting {
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let st = self.state.read(cx);
        if !picked_model_ready(
            &st.models.models,
            &id,
            &st.models.downloading,
            &st.models.verifying,
        ) {
            return;
        }
        self.selecting = true;
        let weak = cx.entity().downgrade();
        let window_handle = window.window_handle();
        self.state.update(cx, |s, cx| {
            s.select_model(&id, cx, move |s, ok, cx| {
                if ok {
                    s.on_model_selected(cx);
                } else {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(
                            (NotificationType::Error, t("onboarding.errors.selectModel")),
                            cx,
                        );
                    });
                    weak.update(cx, |this, cx| {
                        this.selecting = false;
                        this.selected = None;
                        cx.notify();
                    })
                    .ok();
                }
            });
        });
    }

    fn download(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        self.selected = Some(id.to_string());
        let weak = cx.entity().downgrade();
        let failed_id = id.to_string();
        self.state.update(cx, |s, cx| {
            s.download_model_then(id, cx, move |_, result, cx| {
                // Unlock the cards so the user can pick again (the failure
                // itself is toasted from the download-failed event).
                if result.is_err() {
                    weak.update(cx, |this, cx| {
                        if this.selected.as_deref() == Some(failed_id.as_str()) {
                            this.selected = None;
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
        });
        self.maybe_select(window, cx);
        cx.notify();
    }

    fn pick_existing(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        self.selected = Some(id.to_string());
        self.maybe_select(window, cx);
        cx.notify();
    }

    fn cancel(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        self.state.update(cx, |s, cx| s.cancel_download(id, cx));
        self.selected = None;
        cx.notify();
    }
}

impl Render for ModelPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let st = self.state.read(cx);
        let theme = cx.theme().clone();
        let m = st.models.clone();
        let groups = picker_groups(&m.models);
        let busy = self.selected.is_some();
        let has_recommended = !groups.top_picks.is_empty() || !groups.other_recommended.is_empty();
        let show_rest = self.show_all || !has_recommended;
        let from_migration = self
            .main
            .upgrade()
            .is_some_and(|main| main.read(cx).has_migration(cx));

        let download_card = |model: &guidy_core::managers::model::ModelInfo, featured: bool| {
            let status = if m.verifying.contains(&model.id) {
                ModelCardStatus::Verifying
            } else if m.downloading.contains(&model.id) {
                ModelCardStatus::Downloading
            } else {
                ModelCardStatus::Downloadable
            };
            let (e1, e2, e3) = (entity.clone(), entity.clone(), entity.clone());
            ModelCard::new(model.clone(), status)
                .featured(featured)
                .disabled(busy)
                .show_recommended(false)
                .progress(
                    m.progress.get(&model.id).map(|p| p.percentage),
                    m.stats.get(&model.id).map(|s| s.speed),
                )
                .on_download(move |id, window, cx| {
                    e1.update(cx, |this, cx| this.download(id, window, cx)).ok();
                })
                .on_select(move |id, window, cx| {
                    e2.update(cx, |this, cx| this.download(id, window, cx)).ok();
                })
                .on_cancel(move |id, _, cx| {
                    e3.update(cx, |this, cx| this.cancel(id, cx)).ok();
                })
                .into_any_element()
        };

        let existing: Vec<AnyElement> = groups
            .existing
            .iter()
            .map(|model| {
                let status = if self.selected.as_deref() == Some(model.id.as_str()) {
                    ModelCardStatus::Switching
                } else {
                    ModelCardStatus::Available
                };
                let e = entity.clone();
                ModelCard::new((*model).clone(), status)
                    .disabled(busy)
                    .show_recommended(false)
                    .on_select(move |id, window, cx| {
                        e.update(cx, |this, cx| this.pick_existing(id, window, cx))
                            .ok();
                    })
                    .into_any_element()
            })
            .collect();

        let section_title = |key: &str| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(t(key))
        };
        let mut download_section = v_flex().gap_3();
        if !groups.downloadable.is_empty() {
            download_section = download_section
                .child(section_title("onboarding.downloadModelsTitle"))
                .children(groups.top_picks.iter().map(|m| download_card(m, true)))
                .children(
                    groups
                        .other_recommended
                        .iter()
                        .map(|m| download_card(m, false)),
                );
            if has_recommended && !groups.rest.is_empty() {
                let label = if self.show_all {
                    t("onboarding.showFewerModels")
                } else {
                    t_args(
                        "onboarding.showAllModels",
                        &[("total", &groups.downloadable.len().to_string())],
                    )
                };
                download_section = download_section.child(
                    h_flex().justify_center().child(
                        Button::new("show-all-models")
                            .ghost()
                            .small()
                            .label(label)
                            .icon(icon(if self.show_all {
                                "chevron-up"
                            } else {
                                "chevron-down"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = !this.show_all;
                                cx.notify();
                            })),
                    ),
                );
            }
            if show_rest {
                download_section =
                    download_section.children(groups.rest.iter().map(|m| download_card(m, false)));
            }
        }

        let main = self.main.clone();
        v_flex()
            .id("model-picker")
            .size_full()
            .relative()
            .p_6()
            .gap_4()
            .when(from_migration, |d| {
                d.child(
                    div().absolute().top_4().left_4().child(
                        Button::new("picker-back")
                            .ghost()
                            .small()
                            .icon(icon("chevron-left"))
                            .label(t("common.back"))
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                main.update(cx, |m, cx| m.back_to_migration(cx)).ok();
                            }),
                    ),
                )
            })
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .child(wordmark(px(28.), cx))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(t("onboarding.subtitle")),
                    ),
            )
            .child(
                div()
                    .id("model-picker-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .max_w(px(600.))
                            .w_full()
                            .mx_auto()
                            .gap_6()
                            .pb_6()
                            .when(!existing.is_empty(), |d| {
                                d.child(
                                    v_flex()
                                        .gap_3()
                                        .child(section_title("onboarding.existingModelsTitle"))
                                        .children(existing),
                                )
                            })
                            .child(download_section),
                    ),
            )
            .test_support()
    }
}
