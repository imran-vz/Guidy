//! Retired-model update screen.

use crate::logic::migration::{format_time_left, Action, Migration, Phase, RowState};
use crate::logic::models::format_model_size;
use crate::state::AppState;
use crate::ui::brand::wordmark;
use crate::ui::design::{self, Pill};
use crate::ui::kit::{icon, spinner, t, t_args};
use crate::ui::main_window::MainWindow;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub struct ModelMigration {
    main: WeakEntity<MainWindow>,
    state: Entity<AppState>,
    migration: Option<Migration>,
    _subs: Vec<Subscription>,
}

impl ModelMigration {
    pub fn new(main: WeakEntity<MainWindow>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe_in(&state, window, |this, _, window, cx| {
            this.drive(window, cx);
            cx.notify();
        })];
        let mut this = Self {
            main,
            state,
            migration: None,
            _subs: subs,
        };
        this.load_snapshot(cx);
        this
    }

    fn load_snapshot(&mut self, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        let backend = st.backend.clone();
        let in_use = st
            .app_settings()
            .map(|s| s.selected_model.clone())
            .unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let retired = cx
                .background_spawn(async move { backend.get_retired_models() })
                .await;
            this.update(cx, |this, cx| {
                this.migration = Some(Migration::new(retired, &in_use));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn choose_another(&mut self, cx: &mut Context<Self>) {
        self.main
            .update(cx, |m, cx| m.open_model_picker_from_migration(cx))
            .ok();
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let models = self.state.read(cx).models.models.clone();
        let Some(m) = self.migration.as_mut() else {
            return;
        };
        let downloads = m.start(&models);
        for id in &downloads {
            self.download(id, cx);
        }
        self.drive(window, cx);
        cx.notify();
    }

    fn retry(&mut self, retired_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let store = st.models.clone();
        let Some(m) = self.migration.as_mut() else {
            return;
        };
        if let Some(id) = m.retry(&retired_id, &models, &store) {
            self.download(&id, cx);
        }
        self.drive(window, cx);
        cx.notify();
    }

    /// Download a replacement; a failure marks its row failed (Retry /
    /// Choose another) instead of leaving it spinning.
    fn download(&mut self, id: &str, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let failed_id = id.to_string();
        self.state.update(cx, |s, cx| {
            s.download_model_then(id, cx, move |s, result, cx| {
                if result.is_err() {
                    let models = s.models.models.clone();
                    weak.update(cx, |this, cx| {
                        if let Some(m) = this.migration.as_mut() {
                            m.download_failed(&failed_id, &models);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            })
        });
    }

    /// Run whatever the state machine says is next.
    fn drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let store = st.models.clone();
        let backend = st.backend.clone();
        let Some(m) = self.migration.as_mut() else {
            return;
        };
        for action in m.next_actions(&models, &store) {
            match action {
                Action::Finalize {
                    retired,
                    replacement,
                    select,
                } => {
                    let backend = backend.clone();
                    let window_handle = window.window_handle();
                    let retired_for_cb = retired.clone();
                    cx.spawn(async move |this, cx| {
                        let ok = cx
                            .background_spawn(async move {
                                // Select first: the old file is only removed
                                // once the new model is in use.
                                if select && backend.set_active_model(&replacement).is_err() {
                                    return false;
                                }
                                if let Err(e) = backend.delete_retired_model(&retired) {
                                    log::warn!("Failed to delete retired model {retired}: {e}");
                                }
                                true
                            })
                            .await;
                        this.update(cx, |this, cx| {
                            if select && !ok {
                                let _ = cx.update_window(window_handle, |_, window, cx| {
                                    window.push_notification(
                                        (
                                            NotificationType::Error,
                                            t("onboarding.errors.selectModel"),
                                        ),
                                        cx,
                                    );
                                });
                            }
                            if select && ok {
                                this.state.update(cx, |s, cx| {
                                    s.load_current_model(cx);
                                    s.refresh_settings(cx);
                                });
                            }
                        })
                        .ok();
                        this.update_in(cx, |this, window, cx| {
                            if let Some(m) = this.migration.as_mut() {
                                m.finalized(&retired_for_cb, ok);
                            }
                            this.drive(window, cx);
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
                Action::WrapUp { delete } => {
                    let backend = backend.clone();
                    let main = self.main.clone();
                    cx.spawn(async move |_, cx| {
                        cx.background_spawn(async move {
                            for id in delete {
                                let _ = backend.delete_retired_model(&id);
                            }
                        })
                        .await;
                        main.update(cx, |m, cx| {
                            m.set_step(crate::logic::flows::OnboardingStep::Done, cx)
                        })
                        .ok();
                    })
                    .detach();
                }
            }
        }
    }
}

impl Render for ModelMigration {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(m) = self.migration.clone() else {
            return v_flex()
                .id("migration-loading")
                .size_full()
                .items_center()
                .justify_center()
                .child(spinner(
                    "migration-loading",
                    icon("loader-circle")
                        .large()
                        .text_color(theme.muted_foreground),
                ))
                .into_any_element();
        };
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let store = st.models.clone();
        let rows = m.rows(&models);
        let show_checks = rows.len() > 1;
        let chosen = m.chosen(&models);
        let no_way_forward = chosen.is_empty();
        let to_download = m.to_download(&models);
        let downloading = m.downloading(&models, &store);
        let target_failed = m.target_failed(&models);
        let target = m
            .target_id(&models)
            .and_then(|tid| chosen.iter().find(|p| p.retired.id == tid).cloned());
        let loading_target = m.phase == Phase::Migrating
            && !downloading
            && target.as_ref().is_some_and(|p| {
                !m.outcomes.contains_key(&p.retired.id) && p.replacement.is_downloaded
            });
        let (speed, secs) = m.eta(&models, &store);
        let time_left = format_time_left(secs);
        let entity = cx.entity().downgrade();

        let row_elems: Vec<AnyElement> = rows
            .iter()
            .map(|p| {
                let is_chosen = m.picked.contains(&p.retired.id);
                let toggleable = show_checks && m.phase == Phase::Choose;
                let state = m.row_state(p, &store);
                let status: AnyElement = match state {
                    RowState::Failed => {
                        let rid = p.retired.id.clone();
                        let entity = entity.clone();
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_color(theme.danger)
                                    .child(t("onboarding.migration.failed")),
                            )
                            .child(
                                Button::new(SharedString::from(format!("retry-{}", p.retired.id)))
                                    .secondary()
                                    .pill()
                                    .xsmall()
                                    .label(t("onboarding.migration.retry"))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        let rid = rid.clone();
                                        entity
                                            .update(cx, |this, cx| this.retry(rid, window, cx))
                                            .ok();
                                    }),
                            )
                            .into_any_element()
                    }
                    _ if state == RowState::Ready || (is_chosen && p.replacement.is_downloaded) => {
                        div()
                            .size(px(22.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(design::tone(cx).accent_soft)
                            .text_color(design::tone(cx).accent_text)
                            .child(icon("check").xsmall())
                            .into_any_element()
                    }
                    RowState::Progress(pct) => h_flex()
                        .gap_2()
                        .child(
                            design::mono(format!("{}%", pct.round() as i64))
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            ProgressCircle::new(SharedString::from(format!(
                                "ring-{}",
                                p.retired.id
                            )))
                            .value(pct as f32)
                            .small(),
                        )
                        .into_any_element(),
                    _ => design::mono(format_model_size(Some(p.replacement.size_mb as f64)))
                        .text_color(theme.muted_foreground)
                        .into_any_element(),
                };
                let rid = p.retired.id.clone();
                let entity = entity.clone();
                h_flex()
                    .id(SharedString::from(format!(
                        "migration-row-{}",
                        p.retired.id
                    )))
                    .px(design::SECTION_X)
                    .py(px(12.))
                    .gap_3()
                    .when(toggleable, |d| {
                        d.cursor_pointer().on_click(move |_, _, cx| {
                            let rid = rid.clone();
                            entity
                                .update(cx, |this, cx| {
                                    if let Some(m) = this.migration.as_mut() {
                                        m.toggle(&rid);
                                    }
                                    cx.notify();
                                })
                                .ok();
                        })
                    })
                    .when(show_checks, |d| {
                        d.child(
                            div()
                                .size(px(20.))
                                .rounded_full()
                                .border_2()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(is_chosen, |d| {
                                    d.bg(design::tone(cx).accent)
                                        .border_color(design::tone(cx).accent)
                                        .text_color(theme.primary_foreground)
                                        .child(icon("check").xsmall())
                                })
                                .when(!is_chosen, |d| d.border_color(theme.muted_foreground))
                                .when(!toggleable, |d| d.opacity(0.55)),
                        )
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(design::TEXT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(p.replacement.name.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .mt(px(3.))
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child(t_args(
                                        "onboarding.migration.replaces",
                                        &[("name", &p.retired.name)],
                                    ))
                                    .child(design::chip(
                                        t("onboarding.migration.oldFormat"),
                                        false,
                                        cx,
                                    )),
                            ),
                    )
                    .child(div().text_size(px(12.)).child(status))
                    .into_any_element()
            })
            .collect();

        let footer: AnyElement = if m.phase == Phase::Choose {
            if no_way_forward {
                Button::new("migration-choose-another")
                    .primary()
                    .pill()
                    .w_full()
                    .label(t("onboarding.migration.chooseAnother"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_another(cx)))
                    .into_any_element()
            } else {
                let label = if !to_download.is_empty() {
                    t_args(
                        "onboarding.migration.download",
                        &[("size", &format_model_size(Some(m.download_mb(&models))))],
                    )
                } else {
                    t("onboarding.migration.continue")
                };
                Button::new("migration-start")
                    .primary()
                    .pill()
                    .w_full()
                    .label(label)
                    .on_click(cx.listener(|this, _, window, cx| this.start(window, cx)))
                    .into_any_element()
            }
        } else if downloading || !target_failed {
            h_flex()
                .h(px(40.))
                .gap_2()
                .text_size(design::TEXT_BODY)
                .text_color(theme.muted_foreground)
                .child(spinner(
                    "migration-footer-spin",
                    icon("loader-circle").small(),
                ))
                .when_some(target.filter(|_| loading_target), |d, p| {
                    d.child(t_args(
                        "modelSelector.loading",
                        &[("modelName", &p.replacement.name)],
                    ))
                })
                .when_some(time_left.clone(), |d, tl| {
                    d.child(t_args(
                        "onboarding.migration.timeRemaining",
                        &[("time", &tl)],
                    ))
                })
                .when(time_left.is_some() && speed > 0.0, |d| d.child("·"))
                .when(speed > 0.0, |d| {
                    d.child(t_args(
                        "modelSelector.downloadSpeed",
                        &[("speed", &format!("{speed:.1}"))],
                    ))
                })
                .into_any_element()
        } else {
            Button::new("migration-choose-another-failed")
                .ghost()
                .pill()
                .label(t("onboarding.migration.chooseAnother"))
                .on_click(cx.listener(|this, _, _, cx| this.choose_another(cx)))
                .into_any_element()
        };

        v_flex()
            .id("model-migration")
            .size_full()
            .p_6()
            .gap_5()
            .items_center()
            .justify_center()
            .child(wordmark(px(24.), cx))
            .child(
                v_flex()
                    .max_w(px(448.))
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(design::TEXT_HEADLINE)
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("onboarding.migration.title")),
                    )
                    .child(
                        div()
                            .text_size(design::TEXT_BODY)
                            .line_height(relative(1.55))
                            .text_center()
                            .text_color(theme.muted_foreground)
                            .child(t("onboarding.migration.description")),
                    ),
            )
            .child(
                v_flex()
                    .id("migration-rows")
                    .w_full()
                    .max_w(px(560.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .rounded(design::RADIUS_PANEL)
                    .border_1()
                    .border_color(design::tone(cx).hairline)
                    .bg(design::tone(cx).surface)
                    .children(row_elems),
            )
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(560.))
                    .items_center()
                    .child(footer),
            )
            .into_any_element()
    }
}
