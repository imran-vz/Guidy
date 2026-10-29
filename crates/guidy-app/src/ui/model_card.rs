//! Model rows: the dense, list-like row every model is shown as (Models
//! page, onboarding picker), and [`ModelCard`], the speech-model row with
//! its download, select and delete behavior.
//!
//! ```text
//! ✓ Parakeet V3  [Required by Dictation · Meetings]        478 MB  [Use] […]
//!   Fast and accurate · 25 languages · Streaming
//! ```

use crate::logic::languages::{language_label, unique_capability_languages};
use crate::logic::models::{is_legacy_model, quantization_label, ModelCardStatus};
use crate::ui::design::{self, dp};
use crate::ui::download_control::{progress_line, DownloadView};
use crate::ui::kit::{icon, model_description, model_name, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::DownloadItem;
use guidy_core::managers::model::ModelInfo;
use std::rc::Rc;

type ModelHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type ClickHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One entry of a row's "…" menu.
pub struct RowMenuItem {
    pub label: SharedString,
    pub on_click: ClickHandler,
}

impl RowMenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            on_click: Rc::new(on_click),
        }
    }
}

/// The generic two-line model row: title with badges over a muted detail
/// line, a mono size column, then the row's controls and a "…" menu.
#[derive(IntoElement)]
pub struct ModelRow {
    id: SharedString,
    title: SharedString,
    leading: Option<AnyElement>,
    badges: Vec<AnyElement>,
    detail: SharedString,
    size: Option<SharedString>,
    trailing: Vec<AnyElement>,
    menu: Vec<RowMenuItem>,
    dimmed: bool,
    on_click: Option<ClickHandler>,
}

impl ModelRow {
    /// `id` is domain-derived (the model's id); the row's element id is
    /// `model-row-<id>` unless [`Self::element_id`] says otherwise.
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        let id: SharedString = id.into();
        Self {
            id: format!("model-row-{id}").into(),
            title: title.into(),
            leading: None,
            badges: Vec::new(),
            detail: SharedString::default(),
            size: None,
            trailing: Vec::new(),
            menu: Vec::new(),
            dimmed: false,
            on_click: None,
        }
    }

    pub fn element_id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = id.into();
        self
    }

    /// An icon before the title (in-use check, provider glyph).
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn badge(mut self, el: impl IntoElement) -> Self {
        self.badges.push(el.into_any_element());
        self
    }

    pub fn detail(mut self, text: impl Into<SharedString>) -> Self {
        self.detail = text.into();
        self
    }

    pub fn size(mut self, text: impl Into<SharedString>) -> Self {
        self.size = Some(text.into());
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing.push(el.into_any_element());
        self
    }

    pub fn menu_item(mut self, item: RowMenuItem) -> Self {
        self.menu.push(item);
        self
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

/// "Required by Dictation · Meetings" (accent chip), or nothing.
pub fn required_by_chip(features: &[SharedString], cx: &App) -> Option<Div> {
    (!features.is_empty()).then(|| {
        let list = features
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(" · ");
        design::chip(
            t_args("models.requiredBy", &[("features", &list)]),
            true,
            cx,
        )
        .font_family(crate::theme::UI_FONT)
    })
}

impl RenderOnce for ModelRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let hover_bg = tone.raised.opacity(0.35);
        let menu = (!self.menu.is_empty()).then(|| {
            let items = Rc::new(self.menu);
            Button::new(SharedString::from(format!("{}-menu", self.id)))
                .ghost()
                .xsmall()
                .icon(icon("ellipsis"))
                .tooltip(t("models.actions.more"))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                    for item in items.iter() {
                        let f = item.on_click.clone();
                        menu = menu.item(
                            PopupMenuItem::new(item.label.clone())
                                .on_click(move |_, window, cx| f(window, cx)),
                        );
                    }
                    menu
                })
        });
        let on_click = self.on_click.clone();
        h_flex()
            .id(self.id.clone())
            .w_full()
            .min_h(dp(44.))
            .px(design::SECTION_X)
            .py(dp(6.))
            .gap(dp(12.))
            .when(self.dimmed, |d| d.opacity(0.55))
            .when_some(on_click, |d, f| {
                d.hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, window, cx| f(window, cx))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(8.))
                    .child(
                        div()
                            .flex_none()
                            .w(dp(14.))
                            .flex()
                            .justify_center()
                            .children(self.leading),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(dp(2.))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap(dp(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(design::TEXT_BODY)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(tone.text)
                                            .child(self.title.clone()),
                                    )
                                    .children(self.badges),
                            )
                            .when(!self.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(design::TEXT_SECONDARY)
                                        .line_height(dp(14.))
                                        .text_color(tone.text_muted)
                                        .child(self.detail.clone()),
                                )
                            }),
                    ),
            )
            .when_some(self.size, |d, size| {
                d.child(
                    design::mono(size)
                        .flex_none()
                        .min_w(dp(52.))
                        .text_right()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(tone.text_muted),
                )
            })
            // A fixed column, right-aligned, so sizes line up from row to
            // row whatever the actions (Use, Resume, Retry, a trash icon).
            .child(
                h_flex()
                    .flex_none()
                    .min_w(TRAILING_MIN_W)
                    .justify_end()
                    .gap(dp(4.))
                    .children(self.trailing)
                    .children(menu),
            )
            .test_support()
    }
}

/// Width kept for a row's actions (fits Paused at 30% + Resume + trash).
const TRAILING_MIN_W: Pixels = dp(150.);

/// A speech-to-text model row (Models page, onboarding picker).
#[derive(IntoElement)]
pub struct ModelCard {
    model: ModelInfo,
    status: ModelCardStatus,
    featured: bool,
    /// A row inside a shared panel instead of a bordered one of its own.
    compact: bool,
    disabled: bool,
    show_recommended: bool,
    debug_mode: bool,
    progress: Option<f64>,
    download: Option<DownloadItem>,
    required_by: Vec<SharedString>,
    meetings_model: bool,
    on_select: Option<ModelHandler>,
    on_download: Option<ModelHandler>,
    on_delete: Option<ModelHandler>,
    on_cancel: Option<ModelHandler>,
    on_use_for_meetings: Option<ModelHandler>,
}

impl ModelCard {
    pub fn new(model: ModelInfo, status: ModelCardStatus) -> Self {
        Self {
            model,
            status,
            featured: false,
            compact: false,
            disabled: false,
            show_recommended: true,
            debug_mode: false,
            progress: None,
            download: None,
            required_by: Vec::new(),
            meetings_model: false,
            on_select: None,
            on_download: None,
            on_delete: None,
            on_cancel: None,
            on_use_for_meetings: None,
        }
    }

    /// Render as a row of a shared panel (no border of its own).
    pub fn compact(mut self, c: bool) -> Self {
        self.compact = c;
        self
    }

    /// Recommended-first emphasis (accent "Recommended" chip).
    pub fn featured(mut self, f: bool) -> Self {
        self.featured = f;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn show_recommended(mut self, s: bool) -> Self {
        self.show_recommended = s;
        self
    }
    pub fn debug_mode(mut self, d: bool) -> Self {
        self.debug_mode = d;
        self
    }
    /// Percent while downloading (when there is no tracker entry); the
    /// speed argument is kept for callers and ignored (the tracker has it).
    pub fn progress(mut self, p: Option<f64>, _speed: Option<f64>) -> Self {
        self.progress = p;
        self
    }
    /// The download tracker's entry (progress, time left, errors).
    pub fn download(mut self, item: Option<DownloadItem>) -> Self {
        self.download = item;
        self
    }
    /// Names of the features using it ("Dictation", "Meetings").
    pub fn required_by(mut self, features: Vec<SharedString>) -> Self {
        self.required_by = features;
        self
    }
    /// It is the model meetings are transcribed with.
    pub fn meetings_model(mut self, m: bool) -> Self {
        self.meetings_model = m;
        self
    }
    pub fn on_select(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
    /// Download, resume and retry.
    pub fn on_download(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_download = Some(Rc::new(f));
        self
    }
    pub fn on_delete(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_delete = Some(Rc::new(f));
        self
    }
    pub fn on_cancel(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }
    pub fn on_use_for_meetings(
        mut self,
        f: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_use_for_meetings = Some(Rc::new(f));
        self
    }
}

/// "12 languages" / "English only".
pub fn language_display_text(supported: &[String]) -> String {
    let langs = unique_capability_languages(supported);
    if langs.len() == 1 {
        let name = language_label(&langs[0]).unwrap_or(&langs[0]).to_string();
        return guidy_i18n::t_args(
            "modelSelector.capabilities.languageOnly",
            &[("language", &name)],
        );
    }
    guidy_i18n::t_args(
        "modelSelector.capabilities.languageCount",
        &[("total", &langs.len().to_string())],
    )
}

/// Whether a model description already says which languages it covers:
/// it names the count ("25-language speech-to-text", "Supports 25 European
/// languages") or, for one language, the language ("English only."). The
/// row then doesn't repeat it.
pub fn describes_languages(description: &str, supported: &[String]) -> bool {
    let langs = unique_capability_languages(supported);
    if langs.len() == 1 {
        let name = language_label(&langs[0]).unwrap_or(&langs[0]);
        return description.contains(name);
    }
    let count = langs.len().to_string();
    description
        .split(|c: char| !c.is_ascii_digit())
        .any(|number| number == count)
}

/// "Description · 25 languages · Streaming · Translate".
pub fn model_detail(m: &ModelInfo, debug: bool) -> String {
    let description = model_description(m);
    let languages = (!m.supported_languages.is_empty()
        && !describes_languages(&description, &m.supported_languages))
    .then(|| language_display_text(&m.supported_languages));
    let mut parts = vec![description];
    parts.extend(languages);
    if m.supports_streaming {
        parts.push(guidy_i18n::t("modelSelector.streaming"));
    }
    if m.supports_translation {
        parts.push(guidy_i18n::t("modelSelector.capabilities.translate"));
    }
    if debug {
        if let Some(q) = quantization_label(&m.filename) {
            parts.push(q);
        }
    }
    parts.retain(|p| !p.trim().is_empty());
    parts.join(" · ")
}

impl RenderOnce for ModelCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let m = &self.model;
        let id = m.id.clone();
        let tracker_id = guidy_core::downloads::stt_id(&m.id);
        let status = self.status;
        let active = status == ModelCardStatus::Active;
        let name = model_name(m);
        let disabled = self.disabled;

        let mut row = ModelRow::new(m.id.clone(), name.clone())
            .element_id(format!("model-card-{}", m.id))
            .detail(model_detail(m, self.debug_mode))
            .size(guidy_core::downloads::format_bytes(m.size_mb * 1024 * 1024))
            .dimmed(disabled && status.is_clickable());
        if active {
            row = row.leading(icon("check").small().text_color(tone.accent_text));
        }
        if let Some(chip) = required_by_chip(&self.required_by, cx) {
            row = row.badge(chip);
        }
        if self.show_recommended && m.is_recommended && !m.is_downloaded {
            row = row.badge(design::chip(t("onboarding.recommended"), self.featured, cx));
        }
        if m.is_custom {
            row = row.badge(design::chip(t("modelSelector.custom"), false, cx));
        }
        if is_legacy_model(m) {
            row = row.badge(design::chip(t("modelSelector.legacy"), false, cx));
        }
        if status == ModelCardStatus::Switching {
            row = row.badge(design::chip("", true, cx).child(design::shimmer_text(
                SharedString::from(format!("switching-{}", m.id)),
                t("modelSelector.switching"),
                cx,
            )));
        }

        // The download state: the tracker's entry when there is one.
        let view = match (&self.download, status) {
            (Some(item), _) if status.is_busy() || item.state.is_active() => {
                DownloadView::of(Some(item), m.is_downloaded, 0, m.partial_size)
            }
            (_, ModelCardStatus::Verifying) => DownloadView::Verifying,
            (_, ModelCardStatus::Queued) => DownloadView::Queued,
            (_, ModelCardStatus::Downloading) => DownloadView::Downloading {
                fraction: self.progress.map(|p| (p / 100.0) as f32),
                percent: self.progress.map(|p| p.clamp(0.0, 100.0) as u32),
                eta_secs: None,
            },
            (item, ModelCardStatus::Failed) => DownloadView::Failed {
                error: item.as_ref().and_then(|i| i.error.clone()),
            },
            (item, ModelCardStatus::Paused) => {
                match DownloadView::of(
                    item.as_ref(),
                    false,
                    m.size_mb * 1024 * 1024,
                    m.partial_size,
                ) {
                    v @ DownloadView::Paused { .. } => v,
                    _ => DownloadView::Paused { percent: None },
                }
            }
            _ => DownloadView::Missing,
        };

        let download = self.on_download.clone().or_else(|| self.on_select.clone());
        let action = |key: &str, ic: &str, label: SharedString, f: Option<ModelHandler>| {
            let id = id.clone();
            Button::new(SharedString::from(format!("model-{key}-{}", m.id)))
                .secondary()
                .small()
                .icon(icon(ic))
                .label(label)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if let Some(f) = &f {
                        f(&id, window, cx);
                    }
                })
        };
        match status {
            ModelCardStatus::Queued | ModelCardStatus::Downloading | ModelCardStatus::Verifying => {
                let cancel = self.on_cancel.clone();
                let el = match cancel {
                    Some(cancel) => {
                        let id = id.clone();
                        progress_line_with(
                            &tracker_id,
                            &view,
                            move |window, cx| cancel(&id, window, cx),
                            cx,
                        )
                    }
                    None => progress_line(&tracker_id, &view, dp(72.), cx),
                };
                row = row.trailing(el);
            }
            ModelCardStatus::Failed => {
                let error: SharedString = match &view {
                    DownloadView::Failed { error: Some(e) } => e.clone().into(),
                    _ => t("models.state.failed"),
                };
                row = row
                    .trailing(
                        div()
                            .id(SharedString::from(format!("model-error-{}", m.id)))
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(cx.theme().danger)
                            .child(t("models.state.failed"))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(error.clone())
                                    .build(window, cx)
                            }),
                    )
                    .trailing(action(
                        "retry",
                        "rotate-ccw",
                        t("models.actions.retry"),
                        download.clone(),
                    ));
            }
            ModelCardStatus::Paused => {
                let label = match &view {
                    DownloadView::Paused { percent: Some(p) } => {
                        t_args("models.state.pausedAt", &[("percent", &p.to_string())])
                    }
                    _ => t("models.state.paused"),
                };
                row = row
                    .trailing(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(label),
                    )
                    .trailing(action(
                        "resume",
                        "download",
                        t("models.actions.resume"),
                        download.clone(),
                    ));
            }
            ModelCardStatus::Downloadable => {
                row = row.trailing(action(
                    "action",
                    "download",
                    t("modelSelector.download"),
                    download.clone(),
                ));
            }
            ModelCardStatus::Available if self.on_select.is_some() => {
                row = row.trailing(action(
                    "action",
                    "check",
                    t("modelSelector.use"),
                    self.on_select.clone(),
                ));
            }
            _ => {}
        }

        if m.is_downloaded && !self.meetings_model {
            if let Some(f) = self.on_use_for_meetings.clone() {
                let id = id.clone();
                row = row.menu_item(RowMenuItem::new(
                    t("models.actions.useForMeetings"),
                    move |window, cx| f(&id, window, cx),
                ));
            }
        }
        if status.can_delete() || status == ModelCardStatus::Paused {
            if let Some(f) = self.on_delete.clone() {
                let id = id.clone();
                row = row.trailing(delete_button(&m.id, &name, move |window, cx| {
                    f(&id, window, cx)
                }));
            }
        }

        // Clicking the row downloads or selects (the picker relies on it).
        if status.is_clickable() && !disabled {
            let on_select = self.on_select.clone();
            let on_download = self.on_download.clone();
            let id = id.clone();
            row = row.on_click(move |window, cx| {
                let f = if status == ModelCardStatus::Downloadable {
                    on_download.as_ref().or(on_select.as_ref())
                } else {
                    on_select.as_ref()
                };
                if let Some(f) = f {
                    f(&id, window, cx);
                }
            });
        }

        let row = row.into_any_element();
        if self.compact {
            row
        } else {
            design::panel(cx)
                .when(self.featured || active, |d| {
                    d.border_color(tone.accent_text.opacity(0.35))
                })
                .child(row)
                .into_any_element()
        }
    }
}

/// The row's trash button (`delete-<id>`); the handler confirms.
pub fn delete_button(
    id: &str,
    name: &str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(SharedString::from(format!("delete-{id}")))
        .ghost()
        .xsmall()
        .icon(icon("trash"))
        .tooltip(t_args("modelSelector.deleteModel", &[("modelName", name)]))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_click(window, cx)
        })
}

/// [`progress_line`] with the caller's own cancel handler.
fn progress_line_with(
    id: &str,
    view: &DownloadView,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let tone = design::tone(cx);
    let (fraction, text) = crate::ui::download_control::progress_text(view);
    h_flex()
        .flex_none()
        .gap(dp(8.))
        .child(div().w(dp(72.)).child(design::track(
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
        .child(
            Button::new(SharedString::from(format!(
                "cancel-{}",
                id.trim_start_matches("stt:")
            )))
            .ghost()
            .xsmall()
            .icon(icon("x"))
            .tooltip(t("modelSelector.cancelDownload"))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                on_cancel(window, cx)
            }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::describes_languages;

    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn a_description_that_names_its_languages_is_not_repeated() {
        let european: Vec<String> = (0..25).map(|i| format!("x{i}")).collect();
        assert!(describes_languages(
            "Fast and accurate. Supports 25 European languages",
            &european
        ));
        assert!(describes_languages(
            "25-language speech-to-text.",
            &european
        ));
        assert!(describes_languages(
            "English speech-to-text.",
            &langs(&["en"])
        ));
        // A different number, or none, still gets the count.
        assert!(!describes_languages("Supports 4 languages", &european));
        assert!(!describes_languages("Supports 125 languages", &european));
        assert!(!describes_languages(
            "Balanced accuracy and speed.",
            &european
        ));
        assert!(!describes_languages("Fast.", &langs(&["en"])));
    }
}
