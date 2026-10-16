//! Zed-style status bar (Handy's footer): model status + switcher on the
//! left, update status and version on the right.

use crate::logic::models::SelectorStatus;
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{model_name, t, t_args};
use crate::updater::{self, UpdateAction, UpdateStatus};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{h_flex, ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::sync::Arc;
use std::time::Duration;

pub struct StatusBar {
    state: Entity<AppState>,
    update: UpdateStatus,
    portable_installer_url: String,
    started: bool,
    _subs: Vec<Subscription>,
}

impl StatusBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![
            cx.observe(&state, |this, _, cx| {
                this.sync_enabled(cx);
                cx.notify();
            }),
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, window, cx| {
                if let AppEvent::CheckForUpdates = ev {
                    this.manual_check(window, cx);
                }
            }),
        ];
        let mut this = Self {
            state,
            update: UpdateStatus::default(),
            portable_installer_url: crate::logic::flows::PORTABLE_RELEASES_URL.into(),
            started: false,
            _subs: subs,
        };
        this.sync_enabled(cx);
        this
    }

    /// Effective updater state = setting && !locked (waits for both).
    fn sync_enabled(&mut self, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        let (Some(settings), Some(locked)) = (st.app_settings(), st.update_checks_locked) else {
            return;
        };
        let enabled = settings.update_checks_enabled && !locked;
        if enabled != self.update.enabled || !self.started {
            self.started = true;
            self.update.enabled = enabled;
            if enabled {
                self.check(cx);
            } else {
                self.update = UpdateStatus::default();
            }
        }
    }

    #[cfg(test)]
    pub fn update_status(&self) -> UpdateStatus {
        self.update.clone()
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        if !self.update.enabled || self.update.checking {
            return;
        }
        self.update.checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { updater::check() }).await;
            this.update(cx, |this, cx| {
                let available = match &result {
                    Ok(Some(u)) => {
                        this.portable_installer_url =
                            crate::logic::flows::resolve_portable_installer_url(
                                u.raw_manifest.as_ref(),
                                std::env::consts::OS,
                                std::env::consts::ARCH,
                            );
                        true
                    }
                    Ok(None) => false,
                    Err(e) => {
                        log::error!("Failed to check for updates: {e}");
                        false
                    }
                };
                let manual = this.update.manual;
                this.update.finish_check(available);
                if manual && !available {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(3)).await;
                        this.update(cx, |this, cx| {
                            this.update.show_up_to_date = false;
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn manual_check(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.update.enabled {
            return;
        }
        self.update.manual = true;
        self.check(cx);
    }

    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.update.enabled {
            return;
        }
        if self.state.read(cx).is_portable {
            self.open_portable_dialog(window, cx);
            return;
        }
        self.update.installing = true;
        self.update.progress = 0;
        cx.notify();
        let (tx, rx) = smol::channel::unbounded::<u32>();
        cx.spawn(async move |this, cx| {
            while let Ok(p) = rx.recv().await {
                this.update(cx, |this, cx| {
                    this.update.progress = p;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let progress: Arc<dyn Fn(u64, Option<u64>) + Send + Sync> =
                Arc::new(move |d, total| {
                    let _ = tx.try_send(UpdateStatus::progress_percent(d, total));
                });
            let result = cx
                .background_spawn(async move { updater::download_and_install(progress) })
                .await;
            match result {
                Ok(()) => updater::relaunch(),
                Err(e) => {
                    log::error!("Failed to install update: {e}");
                    this.update(cx, |this, cx| {
                        this.update.installing = false;
                        this.update.progress = 0;
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn open_portable_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.portable_installer_url.clone();
        let direct = url != crate::logic::flows::PORTABLE_RELEASES_URL;
        window.open_dialog(cx, move |dialog, _, _| {
            let url = url.clone();
            dialog
                .title(t("footer.portableUpdateTitle"))
                .w(px(420.))
                .child(div().text_sm().child(if direct {
                    t("footer.portableUpdateMessage")
                } else {
                    t("footer.portableUpdateBrowseMessage")
                }))
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().trigger(|b| b.outline().label(t("common.close"))))
                        .when(!url.is_empty(), |f| {
                            f.child(DialogAction::new().child(
                                Button::new("portable-open").primary().label(if direct {
                                    t("footer.portableUpdateButton")
                                } else {
                                    t("footer.portableUpdateBrowseButton")
                                }),
                            ))
                        }),
                )
                .on_ok(move |_, _, cx| {
                    if !url.is_empty() {
                        cx.open_url(&url);
                    }
                    true
                })
        });
    }

    fn select_model(&mut self, id: String, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| {
            s.pending_model = Some(id.clone());
            s.select_model(&id, cx, |s, ok, _| {
                if !ok {
                    s.pending_model = None;
                    s.selector_status = SelectorStatus::Error;
                }
            });
        });
    }

    fn render_model_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let theme = cx.theme();
        let m = &st.models;
        let display_id = st
            .pending_model
            .clone()
            .unwrap_or_else(|| m.current_model.clone());
        let current = m.model(&display_id);

        let status = if !m.verifying.is_empty() {
            SelectorStatus::Verifying
        } else if !m.progress.is_empty() {
            SelectorStatus::Downloading
        } else {
            st.selector_status
        };
        let text: SharedString = if !m.verifying.is_empty() {
            if m.verifying.len() == 1 {
                let id = m.verifying.iter().next().unwrap();
                let name = m.model(id).map(model_name).unwrap_or_else(|| {
                    guidy_i18n::t("modelSelector.verifyingGeneric").replace("...", "")
                });
                t_args("modelSelector.verifying", &[("modelName", &name)])
            } else {
                t("modelSelector.verifyingGeneric")
            }
        } else if !m.progress.is_empty() {
            if m.progress.len() == 1 {
                let p = m.progress.values().next().unwrap();
                let pct = p.percentage.round().clamp(0.0, 100.0) as u32;
                t_args(
                    "modelSelector.downloading",
                    &[("percentage", &pct.to_string())],
                )
            } else {
                t_args(
                    "modelSelector.downloadingMultiple",
                    &[("count", &m.progress.len().to_string())],
                )
            }
        } else {
            match (status, current) {
                (SelectorStatus::Ready, Some(c)) => model_name(c).into(),
                (SelectorStatus::Ready, None) => t("modelSelector.modelReady"),
                (SelectorStatus::Loading, Some(c)) => {
                    t_args("modelSelector.loading", &[("modelName", &model_name(c))])
                }
                (SelectorStatus::Loading, None) => t("modelSelector.loadingGeneric"),
                (SelectorStatus::Error, _) => t("modelSelector.modelError"),
                (SelectorStatus::None, _) => t("modelSelector.noModelDownloadRequired"),
                (_, Some(c)) => model_name(c).into(),
                (_, None) => t("modelSelector.modelUnloaded"),
            }
        };
        let (dot, pulse) = match status {
            SelectorStatus::Ready => (theme.success, false),
            SelectorStatus::Loading => (theme.warning, true),
            SelectorStatus::Downloading => (theme.primary, true),
            SelectorStatus::Verifying => (theme.warning, true),
            SelectorStatus::Error | SelectorStatus::None => (theme.danger, false),
            SelectorStatus::Unloaded => (theme.muted_foreground, false),
        };
        let dot_el = div().size(px(7.)).rounded_full().bg(dot);
        let dot_el: AnyElement = if pulse {
            dot_el
                .with_animation(
                    "status-pulse",
                    Animation::new(Duration::from_millis(1200)).repeat(),
                    |el, delta| el.opacity(0.4 + 0.6 * (delta * std::f32::consts::PI).sin()),
                )
                .into_any_element()
        } else {
            dot_el.into_any_element()
        };

        let downloaded: Vec<_> = m.downloaded_models().into_iter().cloned().collect();
        let entity = cx.entity().downgrade();
        let display_id_for_menu = display_id.clone();
        let progress = m.progress.clone();
        let stats = m.stats.clone();

        h_flex()
            .gap_2()
            .child(
                Button::new("model-selector")
                    .ghost()
                    .xsmall()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(dot_el)
                            .child(div().text_xs().child(text)),
                    )
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        let mut menu = menu.min_w(px(256.)).max_h(px(320.)).scrollable(true);
                        if downloaded.is_empty() {
                            return menu
                                .item(PopupMenuItem::label(t("modelSelector.noModelsAvailable")));
                        }
                        for model in &downloaded {
                            let id = model.id.clone();
                            let active = id == display_id_for_menu;
                            let name = model_name(model);
                            let mut tags = Vec::new();
                            if model.is_custom {
                                tags.push(guidy_i18n::t("modelSelector.custom"));
                            }
                            if model.supports_streaming {
                                tags.push(guidy_i18n::t("modelSelector.streaming"));
                            }
                            if active {
                                tags.push(guidy_i18n::t("modelSelector.active"));
                            }
                            let label: SharedString = if tags.is_empty() {
                                name.into()
                            } else {
                                format!("{name}  ·  {}", tags.join(" · ")).into()
                            };
                            let entity = entity.clone();
                            menu = menu.item(PopupMenuItem::new(label).checked(active).on_click(
                                move |_, _, cx| {
                                    let id = id.clone();
                                    entity.update(cx, |this, cx| this.select_model(id, cx)).ok();
                                },
                            ));
                        }
                        menu
                    }),
            )
            .when(!progress.is_empty(), |d| {
                if progress.len() == 1 {
                    let (id, p) = progress.iter().next().unwrap();
                    let speed = stats.get(id).map(|s| s.speed).unwrap_or(0.0);
                    d.child(
                        div()
                            .w(px(80.))
                            .child(Progress::new("dl-progress").value(p.percentage as f32)),
                    )
                    .when(speed > 0.0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(t_args(
                                    "modelSelector.downloadSpeed",
                                    &[("speed", &format!("{speed:.1}"))],
                                )),
                        )
                    })
                } else {
                    d.children(progress.iter().map(|(id, p)| {
                        div().w(px(32.)).child(
                            Progress::new(SharedString::from(format!("dl-{id}")))
                                .value(p.percentage as f32),
                        )
                    }))
                }
            })
    }

    fn render_update_checker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (key, arg) = self.update.text_key();
        let text = match arg {
            Some(p) => t_args(key, &[("progress", &p)]),
            None => t(key),
        };
        let available = self.update.available;
        let color = if available {
            theme.primary
        } else {
            theme.muted_foreground
        };
        let label = div().text_xs().text_color(color).child(text);
        h_flex()
            .gap_2()
            .child(if self.update.clickable() {
                let action = self.update.action();
                div()
                    .id("update-status")
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.8))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| match action {
                        Some(UpdateAction::Install) => this.install(window, cx),
                        Some(UpdateAction::Check) => this.manual_check(window, cx),
                        None => {}
                    }))
                    .test_support()
                    .into_any_element()
            } else {
                div()
                    .id("update-status")
                    .child(label)
                    .test_support()
                    .into_any_element()
            })
            .when(
                self.update.installing && self.update.progress > 0 && self.update.progress < 100,
                |d| {
                    d.child(
                        div().w(px(80.)).child(
                            Progress::new("update-progress").value(self.update.progress as f32),
                        ),
                    )
                },
            )
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, muted) = (theme.status_bar, theme.border, theme.muted_foreground);
        h_flex()
            .id("status-bar")
            .w_full()
            .h(px(28.))
            .px_2()
            .justify_between()
            .bg(bg)
            .border_t_1()
            .border_color(border)
            .child(self.render_model_selector(cx))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(self.render_update_checker(cx))
                    .child("•")
                    .child(
                        div()
                            .id("app-version")
                            .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .test_support(),
                    ),
            )
    }
}
