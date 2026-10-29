//! The compact download control for one model of any kind, by its
//! download-tracker id (`stt:<model id>`, `llm:<catalog id>`,
//! `speaker:resnet34`, `separation:<id>`, `segmentation:pyannote`).
//!
//! It reads the download list from [`AppState`] when it renders and drives
//! the shared queue, so any view can embed it where a model may be missing
//! (meeting banners, onboarding, settings rows):
//!
//! ```ignore
//! // Speech, LLM or speaker model: size and installed state from the app state.
//! row.child(DownloadControl::for_model("speaker:resnet34", cx))
//! // Or spelled out:
//! row.child(DownloadControl::new("llm:qwen3-4b-instruct-2507-q4_k_m")
//!     .installed(false)
//!     .size(2_497_281_120))
//! ```
//!
//! States: Download (with size) → Queued (Cancel) → progress bar, percent
//! and time left (Cancel) → Verifying → Downloaded; Failed shows Retry with
//! the error as a tooltip; a cancelled download with a partial file shows
//! Resume. Element ids: `dl-download-<id>`, `dl-cancel-<id>`,
//! `dl-retry-<id>`, `dl-resume-<id>` (use them in UI tests).

use crate::logic::models::{format_eta, ModelCardStatus};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::{format_bytes, DownloadItem, DownloadKind, DownloadState};
use std::rc::Rc;

/// What a control (or a model row) shows for a download id.
#[derive(Clone, Debug, PartialEq)]
pub enum DownloadView {
    Missing,
    Queued,
    Downloading {
        fraction: Option<f32>,
        percent: Option<u32>,
        eta_secs: Option<u64>,
    },
    Verifying,
    /// Cancelled or interrupted with a partial file (0..=100).
    Paused {
        percent: Option<u32>,
    },
    Failed {
        error: Option<String>,
    },
    Installed,
}

impl DownloadView {
    /// From the tracker entry (if any), whether the model is on disk, and
    /// its partial bytes.
    pub fn of(item: Option<&DownloadItem>, installed: bool, size: u64, partial: u64) -> Self {
        let percent_of = |done: u64, total: u64| {
            (total > 0).then(|| ((done as f64 / total as f64) * 100.0).floor().min(99.0) as u32)
        };
        match item.map(|i| (i, i.state)) {
            Some((_, DownloadState::Queued)) => Self::Queued,
            Some((i, DownloadState::Downloading)) => Self::Downloading {
                fraction: i.fraction(),
                percent: i.percent(),
                eta_secs: i.eta_secs(),
            },
            Some((_, DownloadState::Verifying)) => Self::Verifying,
            _ if installed => Self::Installed,
            Some((i, DownloadState::Failed)) => Self::Failed {
                error: i.error.clone(),
            },
            Some((i, DownloadState::Cancelled)) if i.downloaded_bytes > 0 => Self::Paused {
                percent: i.percent(),
            },
            _ if partial > 0 => Self::Paused {
                percent: percent_of(partial, size),
            },
            _ => Self::Missing,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Downloading { .. } | Self::Verifying
        )
    }

    /// The equivalent model-row status (rows add Active / Switching).
    pub fn row_status(&self) -> ModelCardStatus {
        match self {
            Self::Missing => ModelCardStatus::Downloadable,
            Self::Queued => ModelCardStatus::Queued,
            Self::Downloading { .. } => ModelCardStatus::Downloading,
            Self::Verifying => ModelCardStatus::Verifying,
            Self::Paused { .. } => ModelCardStatus::Paused,
            Self::Failed { .. } => ModelCardStatus::Failed,
            Self::Installed => ModelCardStatus::Available,
        }
    }
}

/// Runs before a download is queued ([`DownloadControl::on_start`]).
type OnStart = Rc<dyn Fn(&mut App)>;

/// See the module docs.
#[derive(IntoElement)]
pub struct DownloadControl {
    id: String,
    installed: bool,
    size: u64,
    partial: u64,
    show_installed: bool,
    label_size: bool,
    disabled: bool,
    on_start: Option<OnStart>,
}

impl DownloadControl {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            installed: false,
            size: 0,
            partial: 0,
            show_installed: true,
            label_size: true,
            disabled: false,
            on_start: None,
        }
    }

    /// Size, partial bytes and installed state from the app state's model
    /// lists (speech models, LLMs, speaker models).
    pub fn for_model(id: impl Into<String>, cx: &App) -> Self {
        let id = id.into();
        let models = &AppState::global(cx).read(cx).models;
        let mut this = Self::new(id.clone());
        match DownloadKind::parse(&id) {
            Some((DownloadKind::Stt, key)) => {
                if let Some(m) = models.model(key) {
                    this.installed = m.is_downloaded;
                    this.size = m.size_mb * 1024 * 1024;
                    this.partial = m.partial_size;
                }
            }
            Some(_) => {
                if let Some(m) = models.other_model(&id) {
                    this.installed = m.installed;
                    this.size = m.size_bytes;
                    this.partial = m.partial_bytes;
                }
            }
            None => {}
        }
        this
    }

    pub fn installed(mut self, installed: bool) -> Self {
        self.installed = installed;
        self
    }

    /// Full size in bytes (shown on the Download button).
    pub fn size(mut self, bytes: u64) -> Self {
        self.size = bytes;
        self
    }

    /// Bytes of a paused download already on disk.
    pub fn partial(mut self, bytes: u64) -> Self {
        self.partial = bytes;
        self
    }

    /// Show "Downloaded" when installed (default); off renders nothing.
    pub fn show_installed(mut self, show: bool) -> Self {
        self.show_installed = show;
        self
    }

    /// Put the size on the Download button (default; off when the row
    /// already shows it).
    pub fn label_size(mut self, show: bool) -> Self {
        self.label_size = show;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Runs before Download, Retry or Resume queue the download (e.g. to
    /// record consent, or that a speech model is for meetings only).
    pub fn on_start(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        self.on_start = Some(Rc::new(f));
        self
    }
}

/// Queue a download of `id` (any kind).
pub fn start(id: &str, cx: &mut App) {
    let id = id.to_string();
    AppState::global(cx).update(cx, |s, cx| s.request_download(&id, cx));
}

/// Cancel a queued or running download of `id`.
pub fn cancel(id: &str, cx: &mut App) {
    let id = id.to_string();
    AppState::global(cx).update(cx, |s, cx| s.cancel_tracked_download(&id, cx));
}

/// Retry or resume `id` (falls back to a fresh request).
pub fn retry(id: &str, cx: &mut App) {
    let id = id.to_string();
    AppState::global(cx).update(cx, |s, cx| s.retry_download(&id, cx));
}

/// The bar fraction and "34% · 2 min" / "Queued" / "Verifying…" of a
/// busy download.
pub fn progress_text(view: &DownloadView) -> (Option<f32>, SharedString) {
    match view {
        DownloadView::Queued => (Some(0.0), t("models.state.queued")),
        DownloadView::Verifying => (None, t("models.state.verifying")),
        DownloadView::Downloading {
            fraction,
            percent,
            eta_secs,
        } => {
            let pct = percent
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "…".into());
            let text = match eta_secs {
                Some(s) if *s > 0 => format!("{pct} · {}", format_eta(*s)),
                _ => pct,
            };
            (*fraction, text.into())
        }
        _ => (None, SharedString::default()),
    }
}

/// A progress bar with "34% · 2 min" and a cancel button: what every busy
/// download looks like.
pub fn progress_line(id: &str, view: &DownloadView, width: Pixels, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
    let (fraction, text) = progress_text(view);
    let cancel_id = id.to_string();
    h_flex()
        .flex_none()
        .gap(dp(8.))
        .child(div().w(width).child(design::track(
            SharedString::from(format!("dl-track-{id}")),
            fraction,
            cx,
        )))
        .child(
            design::mono(text)
                .min_w(dp(64.))
                .text_size(design::TEXT_SECONDARY)
                .text_color(tone.text_muted),
        )
        .when(!matches!(view, DownloadView::Verifying), |d| {
            d.child(
                Button::new(SharedString::from(format!("dl-cancel-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(icon("x"))
                    .tooltip(t("models.actions.cancel"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cancel(&cancel_id, cx)
                    }),
            )
        })
        .into_any_element()
}

impl RenderOnce for DownloadControl {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let item = AppState::global(cx)
            .read(cx)
            .models
            .download(&self.id)
            .cloned();
        let view = DownloadView::of(item.as_ref(), self.installed, self.size, self.partial);
        let tone = design::tone(cx);
        let danger = cx.theme().danger;
        let id = self.id.clone();
        let on_start = self.on_start.clone();
        let begin = move |cx: &mut App| {
            if let Some(f) = &on_start {
                f(cx);
            }
        };
        let el: AnyElement = match &view {
            DownloadView::Queued | DownloadView::Downloading { .. } | DownloadView::Verifying => {
                progress_line(&id, &view, dp(72.), cx)
            }
            DownloadView::Installed if !self.show_installed => div().into_any_element(),
            DownloadView::Installed => h_flex()
                .gap(dp(4.))
                .text_size(design::TEXT_SECONDARY)
                .text_color(tone.text_muted)
                .child(icon("check").small().text_color(cx.theme().success))
                .child(t("models.state.downloaded"))
                .into_any_element(),
            DownloadView::Failed { error } => {
                let tip: SharedString = error
                    .clone()
                    .unwrap_or_else(|| guidy_i18n::t("models.state.failed"))
                    .into();
                h_flex()
                    .gap(dp(6.))
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("dl-error-{id}")))
                            .gap(dp(4.))
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(danger)
                            .child(icon("circle-alert").small())
                            .child(t("models.state.failed"))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                    .build(window, cx)
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("dl-retry-{id}")))
                            .secondary()
                            .small()
                            .icon(icon("rotate-ccw"))
                            .label(t("models.actions.retry"))
                            .disabled(self.disabled)
                            .on_click({
                                let begin = begin.clone();
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    begin(cx);
                                    retry(&id, cx)
                                }
                            }),
                    )
                    .into_any_element()
            }
            DownloadView::Paused { percent } => {
                let label = percent
                    .map(|p| t_args("models.state.pausedAt", &[("percent", &p.to_string())]))
                    .unwrap_or_else(|| t("models.state.paused"));
                h_flex()
                    .gap(dp(6.))
                    .child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(label),
                    )
                    .child(
                        Button::new(SharedString::from(format!("dl-resume-{id}")))
                            .secondary()
                            .small()
                            .icon(icon("download"))
                            .label(t("models.actions.resume"))
                            .disabled(self.disabled)
                            .on_click({
                                let begin = begin.clone();
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    begin(cx);
                                    retry(&id, cx)
                                }
                            }),
                    )
                    .into_any_element()
            }
            DownloadView::Missing => {
                let label = if self.size > 0 && self.label_size {
                    t_args(
                        "models.actions.downloadSize",
                        &[("size", &format_bytes(self.size))],
                    )
                } else {
                    t("models.actions.download")
                };
                Button::new(SharedString::from(format!("dl-download-{id}")))
                    .secondary()
                    .small()
                    .icon(icon("download"))
                    .label(label)
                    .disabled(self.disabled)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        begin(cx);
                        start(&id, cx)
                    })
                    .into_any_element()
            }
        };
        div()
            .id(SharedString::from(format!("dl-control-{}", self.id)))
            .flex_none()
            .child(el)
            .test_support()
    }
}

/// The toast for a speech model that finished downloading but wasn't picked
/// for dictation: "Use for Dictation" switches to it.
pub fn model_ready_notification(id: &str, name: &str) -> Notification {
    let id = id.to_string();
    Notification::new()
        .title(t_args("models.ready.title", &[("name", name)]))
        .message(t("models.ready.message"))
        .action(move |_, _, _| {
            let id = id.clone();
            Button::new("model-ready-use")
                .primary()
                .small()
                .label(t("models.actions.useForDictation"))
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    AppState::global(cx).update(cx, |s, cx| s.use_for_dictation(&id, cx));
                })
        })
}

#[cfg(test)]
mod tests {
    use super::{DownloadView, ModelCardStatus};
    use guidy_core::downloads::{DownloadItem, DownloadState};

    #[::core::prelude::v1::test]
    fn view_follows_the_tracker_then_the_disk() {
        let item = |state| DownloadItem::new("llm:q", "Q", 100, state);
        assert_eq!(DownloadView::of(None, false, 100, 0), DownloadView::Missing);
        assert_eq!(
            DownloadView::of(None, true, 100, 0),
            DownloadView::Installed
        );
        assert_eq!(
            DownloadView::of(None, false, 100, 40),
            DownloadView::Paused { percent: Some(40) }
        );
        assert_eq!(
            DownloadView::of(Some(&item(DownloadState::Queued)), false, 100, 0),
            DownloadView::Queued
        );
        let mut running = item(DownloadState::Downloading);
        running.downloaded_bytes = 25;
        running.bytes_per_sec = 15;
        assert_eq!(
            DownloadView::of(Some(&running), false, 100, 0),
            DownloadView::Downloading {
                fraction: Some(0.25),
                percent: Some(25),
                eta_secs: Some(5)
            }
        );
        let mut failed = item(DownloadState::Failed);
        failed.error = Some("offline".into());
        assert_eq!(
            DownloadView::of(Some(&failed), false, 100, 0),
            DownloadView::Failed {
                error: Some("offline".into())
            }
        );
        // Installed since (e.g. retried elsewhere): installed wins.
        assert_eq!(
            DownloadView::of(Some(&failed), true, 100, 0),
            DownloadView::Installed
        );
        let mut cancelled = item(DownloadState::Cancelled);
        cancelled.downloaded_bytes = 60;
        assert_eq!(
            DownloadView::of(Some(&cancelled), false, 100, 0),
            DownloadView::Paused { percent: Some(60) }
        );
        assert!(DownloadView::Queued.is_busy());
        assert_eq!(
            DownloadView::Paused { percent: None }.row_status(),
            ModelCardStatus::Paused
        );
    }
}
