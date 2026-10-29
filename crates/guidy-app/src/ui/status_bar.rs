//! The status bar (22 pt): model status + switcher on the
//! left, download activity and update status on the right (the version is
//! on the About page).

use crate::logic::models::SelectorStatus;

use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::download_control::{progress_line, DownloadView};
use crate::ui::kit::{icon, model_name, t, t_args};
use crate::updater::{self, UpdateAction, UpdateStatus};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::v_flex;
use gpui_kit::component::{h_flex, ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::{format_bytes, DownloadItem, DownloadState, DownloadSummary};
use std::sync::Arc;
use std::time::Duration;

pub struct StatusBar {
    state: Entity<AppState>,
    update: UpdateStatus,
    portable_installer_url: String,
    started: bool,
    _subs: Vec<Subscription>,
}

struct GlobalStatusBar(Entity<StatusBar>);
impl Global for GlobalStatusBar {}

impl StatusBar {
    /// The session's status bar. Update state (and the once-per-session
    /// automatic check) outlives the settings window, which is destroyed on
    /// close; it also has to hear the tray's "check for updates" while closed.
    pub fn global(cx: &mut App) -> Entity<StatusBar> {
        if let Some(g) = cx.try_global::<GlobalStatusBar>() {
            return g.0.clone();
        }
        let bar = cx.new(StatusBar::new);
        cx.set_global(GlobalStatusBar(bar.clone()));
        bar
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![
            cx.observe(&state, |this, _, cx| {
                this.sync_enabled(cx);
                cx.notify();
            }),
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| {
                if let AppEvent::CheckForUpdates = ev {
                    this.manual_check(cx)
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
        // The footer (and its automatic check) mounts after onboarding.
        if !st.onboarding_done() {
            return;
        }
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
        self.update.failed = None;
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
                        this.update.fail_check();
                        cx.notify();
                        return;
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

    fn manual_check(&mut self, cx: &mut Context<Self>) {
        if !self.update.enabled {
            return;
        }
        // A check already running (e.g. the automatic one) still reports
        // "up to date" when the user asked.
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
        self.update.failed = None;
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
                Ok(()) => {
                    cx.update(crate::release_single_instance);
                    updater::relaunch()
                }
                Err(e) => {
                    log::error!("Failed to install update: {e}");
                    this.update(cx, |this, cx| {
                        this.update.fail_install();
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
                .w(dp(420.))
                .child(div().text_sm().child(if direct {
                    t("footer.portableUpdateMessage")
                } else {
                    t("footer.portableUpdateBrowseMessage")
                }))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().trigger(|b| b.secondary().label(t("common.close"))),
                        )
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

        // Downloads (of any model, for any feature) show in the downloads
        // indicator, not here: this is the dictation model's state only.
        let status = st.selector_status;
        let text: SharedString = {
            match (status, current) {
                (SelectorStatus::Ready, Some(c)) => model_name(c).into(),
                (SelectorStatus::Ready, None) => t("modelSelector.modelReady"),
                (SelectorStatus::Loading, Some(c)) => {
                    t_args("modelSelector.loading", &[("modelName", &model_name(c))])
                }
                (SelectorStatus::Loading, None) => t("modelSelector.loadingGeneric"),
                (SelectorStatus::Error, _) => st
                    .selector_error
                    .clone()
                    .map(SharedString::from)
                    .unwrap_or_else(|| t("modelSelector.modelError")),
                (SelectorStatus::None, _) => t("modelSelector.noModelDownloadRequired"),
                (SelectorStatus::Unknown, None) => "—".into(),
                (_, Some(c)) => model_name(c).into(),
                (_, None) => t("modelSelector.modelUnloaded"),
            }
        };
        let tone = design::tone(cx);
        // Ready is green, busy states pulse in the accent, missing or
        // failed is red, unloaded is neutral.
        let (dot, pulse) = match status {
            SelectorStatus::Ready => (theme.success, false),
            SelectorStatus::Loading | SelectorStatus::Downloading | SelectorStatus::Verifying => {
                (tone.accent, true)
            }
            SelectorStatus::Error | SelectorStatus::None => (theme.danger, false),
            SelectorStatus::Unloaded | SelectorStatus::Unknown => (tone.text_faint, false),
        };
        let dot_el = div()
            .id("model-status-dot")
            .flex_none()
            .size(dp(7.))
            .rounded_full()
            .bg(dot);
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

        h_flex().gap_2().child(
            Button::new("model-selector")
                .ghost()
                .xsmall()
                .child(
                    h_flex().gap(dp(6.)).child(dot_el).child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text)
                            .child(text),
                    ),
                )
                .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                    let mut menu = menu.min_w(dp(256.)).max_h(dp(320.)).scrollable(true);
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
    }

    fn attention_downloads(&self, cx: &App) -> Vec<DownloadItem> {
        self.state
            .read(cx)
            .models
            .downloads
            .iter()
            .filter(|d| d.state.needs_attention())
            .cloned()
            .collect()
    }

    /// Every model download (speech, LLM, speaker): one button with the
    /// count and overall percent; it opens the list with cancel / retry.
    fn render_downloads(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let attention = self.attention_downloads(cx);
        let label = activity_label(&attention).unwrap_or_default();
        let summary = DownloadSummary::of(&attention);
        let tone = design::tone(cx);
        Popover::new("download-activity-popover")
            .anchor(Anchor::BottomRight)
            .trigger(
                Button::new("download-activity").ghost().xsmall().child(
                    h_flex()
                        .gap(dp(6.))
                        .when(summary.active > 0, |d| {
                            d.child(div().w(dp(40.)).child(design::track(
                                "download-activity-track",
                                summary.percent().map(|p| p as f32 / 100.0),
                                cx,
                            )))
                        })
                        .when(summary.active == 0, |d| {
                            d.child(icon("circle-alert").xsmall().text_color(cx.theme().danger))
                        })
                        .child(
                            div()
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted)
                                .child(label),
                        ),
                ),
            )
            .content(|_, _, cx| downloads_list(cx))
    }

    fn render_update_checker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let accent = design::tone(cx).accent_text;
        let (key, arg) = self.update.text_key();
        let text = match arg {
            Some(p) => t_args(key, &[("progress", &p)]),
            None => t(key),
        };
        let color = if self.update.is_error() {
            theme.danger
        } else if self.update.available {
            accent
        } else {
            theme.muted_foreground
        };
        h_flex()
            .gap_2()
            .child(if self.update.clickable() {
                let action = self.update.action();
                Button::new("update-status")
                    .ghost()
                    .xsmall()
                    .text_color(color)
                    .when(self.update.is_error(), |b| b.icon(icon("rotate-ccw")))
                    .label(text)
                    .on_click(cx.listener(move |this, _, window, cx| match action {
                        Some(UpdateAction::Install) => this.install(window, cx),
                        Some(UpdateAction::Check) => this.manual_check(cx),
                        None => {}
                    }))
                    .into_any_element()
            } else {
                div()
                    .id("update-status")
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(color)
                    .child(text)
                    .test_support()
                    .into_any_element()
            })
            .when(
                self.update.installing && self.update.progress > 0 && self.update.progress < 100,
                |d| {
                    d.child(div().w(dp(80.)).child(design::track(
                        "update-progress",
                        Some((self.update.progress as f32) / 100.0),
                        cx,
                    )))
                },
            )
    }
}

/// The downloads indicator's text, for the status bar (and anything else
/// that wants one line about downloads): "Qwen3 4B · 34%", "Downloading 3
/// models · 41%", "Download failed · Qwen3 4B". `None` when nothing needs
/// attention.
pub fn activity_label(items: &[DownloadItem]) -> Option<SharedString> {
    let active: Vec<&DownloadItem> = items.iter().filter(|d| d.state.is_active()).collect();
    let summary = DownloadSummary::of(items);
    let pct = |p: Option<u32>| p.map(|p| p.to_string()).unwrap_or_else(|| "…".into());
    match active.as_slice() {
        [one] if one.state == DownloadState::Queued => {
            Some(t_args("models.statusBar.queued", &[("name", &one.name)]))
        }
        [one] => Some(t_args(
            "meetings.downloads.activity",
            &[("name", &one.name), ("percent", &pct(one.percent()))],
        )),
        [] => items
            .iter()
            .find(|d| d.state == DownloadState::Failed)
            .map(|d| t_args("meetings.downloads.activityFailed", &[("name", &d.name)])),
        many => Some(t_args(
            "models.statusBar.multiple",
            &[
                ("count", &many.len().to_string()),
                ("percent", &pct(summary.percent())),
            ],
        )),
    }
}

/// The downloads popover: every download with its progress and controls.
fn downloads_list(cx: &mut App) -> impl IntoElement {
    let items = AppState::global(cx).read(cx).models.downloads.clone();
    let tone = design::tone(cx);
    let danger = cx.theme().danger;
    let success = cx.theme().success;
    let rows = items.iter().rev().map(|item| {
        let view = DownloadView::of(Some(item), item.state == DownloadState::Done, 0, 0);
        let size = if item.total_bytes > 0 && item.state.is_active() {
            format!(
                "{} / {}",
                format_bytes(item.downloaded_bytes),
                format_bytes(item.total_bytes)
            )
        } else {
            format_bytes(item.total_bytes)
        };
        let state: AnyElement = match item.state {
            DownloadState::Queued | DownloadState::Downloading | DownloadState::Verifying => {
                progress_line(&item.id, &view, dp(64.), cx)
            }
            DownloadState::Failed => {
                let id = item.id.clone();
                h_flex()
                    .gap(dp(6.))
                    .child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(danger)
                            .child(t("meetings.downloads.failedShort")),
                    )
                    .child(
                        Button::new(SharedString::from(format!("dl-retry-{}", item.id)))
                            .ghost()
                            .xsmall()
                            .icon(icon("rotate-ccw"))
                            .tooltip(t("meetings.downloads.retry"))
                            .on_click(move |_, _, cx| crate::ui::download_control::retry(&id, cx)),
                    )
                    .into_any_element()
            }
            DownloadState::Done => icon("check").small().text_color(success).into_any_element(),
            DownloadState::Cancelled => div()
                .text_size(design::TEXT_SECONDARY)
                .text_color(tone.text_muted)
                .child(t("meetings.downloads.cancelled"))
                .into_any_element(),
        };
        h_flex()
            .id(SharedString::from(format!("download-item-{}", item.id)))
            .w_full()
            .gap(dp(10.))
            .py(dp(4.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(design::TEXT_BODY)
                            .text_color(tone.text)
                            .child(item.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(if item.state == DownloadState::Failed {
                                danger
                            } else {
                                tone.text_muted
                            })
                            .child(match (&item.error, item.state) {
                                (Some(e), DownloadState::Failed) => e.clone(),
                                _ => size,
                            }),
                    ),
            )
            .child(state)
    });
    v_flex()
        .id("downloads-list")
        .w(dp(340.))
        .gap(dp(2.))
        .child(
            div()
                .pb(dp(4.))
                .child(design::section_header(t("models.statusBar.title"), cx)),
        )
        .children(rows)
        .when(items.is_empty(), |d| {
            d.child(
                div()
                    .py(dp(6.))
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text_muted)
                    .child(t("meetings.downloads.empty")),
            )
        })
        .child(
            h_flex().pt(dp(6.)).justify_end().child(
                Button::new("downloads-show-models")
                    .ghost()
                    .xsmall()
                    .label(t("models.statusBar.showModels"))
                    .on_click(|_, _, cx| {
                        crate::windows::show_section(crate::ui::main_window::Section::Models, cx)
                    }),
            ),
        )
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let downloads = !self.attention_downloads(cx).is_empty();
        h_flex()
            .id("status-bar")
            .w_full()
            .h(design::STATUS_BAR_H)
            .flex_none()
            .px(dp(6.))
            .justify_between()
            .border_t_1()
            .border_color(tone.hairline)
            .bg(tone.frame)
            .text_size(design::TEXT_SECONDARY)
            .child(self.render_model_selector(cx))
            .child(
                h_flex()
                    .gap(dp(6.))
                    .pr(dp(4.))
                    .text_color(tone.text_muted)
                    .when(downloads, |d| d.child(self.render_downloads(cx)))
                    .when(self.update.enabled && downloads, |d| {
                        d.child(div().text_color(tone.text_faint).child("·"))
                    })
                    .when(self.update.enabled, |d| {
                        d.child(self.render_update_checker(cx))
                    }),
            )
    }
}
