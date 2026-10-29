//! Retired-model update screen (a returning user whose model is in a
//! format Guidy no longer runs): download the replacement, choose another
//! model, or do it later (asked again next launch).

use super::assistant::{continue_button, rows, Assistant};
use super::model_picker::ModelPicker;
use crate::logic::migration::{format_time_left, Action, Migration, Phase, RowState};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, spinner, t, t_args};
use crate::ui::main_window::MainWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::format_bytes;

pub struct ModelMigration {
    main: WeakEntity<MainWindow>,
    state: Entity<AppState>,
    migration: Option<Migration>,
    /// "Choose Another…" is open.
    picker: Option<Entity<ModelPicker>>,
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
            picker: None,
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

    /// Pick another model instead: it is selected now, or downloaded in the
    /// background and selected when done; the retired models are removed.
    pub fn choose_another(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            ModelPicker::new(
                None,
                None,
                move |chosen, _, cx| {
                    weak.update(cx, |this, cx| {
                        this.picker = None;
                        if let Some(id) = chosen {
                            this.state.update(cx, |s, cx| s.migrate_to_model(&id, cx));
                        }
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

    /// The picker sub-pane, when open (tests drive it).
    pub fn picker(&self) -> Option<Entity<ModelPicker>> {
        self.picker.clone()
    }

    fn later(&mut self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.postpone_migration(cx)));
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let store = st.models.clone();
        let Some(m) = self.migration.as_mut() else {
            return;
        };
        let downloads = m.start(&models, &store);
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

    /// Download a replacement. One that can't be queued fails its row at
    /// once; one that fails later is noticed in the download list
    /// ([`Migration::note_failures`]). Either way: Retry / Choose Another.
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
        m.note_failures(&models, &store);
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
        if let Some(picker) = &self.picker {
            return picker.clone().into_any_element();
        }
        let theme = cx.theme().clone();
        let tone = design::tone(cx);
        let frame = |body: AnyElement| {
            Assistant::new("model-migration", t("onboarding.migration.title"))
                .icon("circle-arrow-up")
                .description(t("onboarding.migration.description"))
                .child(body)
        };
        let Some(m) = self.migration.clone() else {
            let rows_skeleton = rows(
                None,
                vec![h_flex()
                    .id("migration-loading")
                    .h(dp(44.))
                    .px(design::SECTION_X)
                    .gap(dp(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap(dp(6.))
                            .child(Skeleton::new().w(dp(140.)).h(dp(10.)))
                            .child(Skeleton::new().secondary().w(dp(220.)).h(dp(8.))),
                    )
                    .into_any_element()],
                cx,
            );
            return frame(rows_skeleton.into_any_element()).into_any_element();
        };
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let store = st.models.clone();
        let rows_data = m.rows(&models);
        let show_checks = rows_data.len() > 1;
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

        let row_elems: Vec<AnyElement> = rows_data
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
                                    .outline()
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
                        design::chip("", true, cx)
                            .child(icon("check").size(dp(11.)))
                            .child(t("onboarding.setup.downloaded"))
                            .into_any_element()
                    }
                    RowState::Progress(pct) => h_flex()
                        .gap_2()
                        .child(
                            design::mono(format!("{}%", pct.round() as i64))
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted),
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
                    _ => design::mono(format_bytes(p.replacement.size_mb * 1024 * 1024))
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(tone.text_muted)
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
                    .py(dp(8.))
                    .gap(dp(10.))
                    .when(toggleable, |d| {
                        d.on_click(move |_, _, cx| {
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
                            gpui_kit::component::checkbox::Checkbox::new(SharedString::from(
                                format!("migration-check-{}", p.retired.id),
                            ))
                            .checked(is_chosen)
                            .disabled(!toggleable),
                        )
                    })
                    .when(!show_checks, |d| {
                        d.child(
                            icon("audio-lines")
                                .size(dp(16.))
                                .text_color(tone.text_muted),
                        )
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(dp(2.))
                            .child(
                                div()
                                    .text_size(design::TEXT_BODY)
                                    .truncate()
                                    .child(p.replacement.name.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap(dp(6.))
                                    .text_size(design::TEXT_SECONDARY)
                                    .text_color(tone.text_muted)
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
                    .child(div().text_size(design::TEXT_SECONDARY).child(status))
                    .into_any_element()
            })
            .collect();

        let mut assistant = frame(rows(None, row_elems, cx).into_any_element());
        if m.phase == Phase::Choose {
            assistant = assistant.footer_start(
                Button::new("migration-later")
                    .outline()
                    .label(t("onboarding.migration.later"))
                    .on_click(cx.listener(|this, _, _, cx| this.later(cx))),
            );
            if no_way_forward {
                assistant = assistant.action(continue_button(
                    "migration-choose-another",
                    t("onboarding.setup.chooseAnother"),
                    true,
                    cx.listener(|this, _, window, cx| this.choose_another(window, cx)),
                ));
            } else {
                let label = if !to_download.is_empty() {
                    t_args(
                        "onboarding.migration.download",
                        &[(
                            "size",
                            &format_bytes((m.download_mb(&models) * 1024.0 * 1024.0) as u64),
                        )],
                    )
                } else {
                    t("onboarding.migration.continue")
                };
                assistant = assistant
                    .action(
                        Button::new("migration-choose-another")
                            .outline()
                            .label(t("onboarding.setup.chooseAnother"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_another(window, cx)),
                            ),
                    )
                    .action(continue_button(
                        "migration-start",
                        label,
                        true,
                        cx.listener(|this, _, window, cx| this.start(window, cx)),
                    ));
            }
        } else if downloading || !target_failed {
            assistant = assistant.note(
                h_flex()
                    .id("migration-progress")
                    .gap(dp(6.))
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
                    .test_support(),
            );
        } else {
            assistant = assistant.action(
                Button::new("migration-choose-another-failed")
                    .outline()
                    .label(t("onboarding.setup.chooseAnother"))
                    .on_click(cx.listener(|this, _, window, cx| this.choose_another(window, cx))),
            );
        }
        assistant.into_any_element()
    }
}
