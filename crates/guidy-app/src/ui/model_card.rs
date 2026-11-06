//! Model rows: the dense, list-like row every model is shown as (Models
//! page, onboarding picker), and [`ModelCard`], the speech-model row with
//! its download, select and delete behavior.
//!
//! ```text
//! ✓ Parakeet V3                         478 MB   ▬▬▬── 40% · 2 min  [Use] […]
//!   Fast and accurate · 25 languages
//!   Required by Dictation · Meetings
//! ```
//!
//! The title line carries fixed slots, so every column keeps its x from
//! row to row: the name (never narrower than [`NAME_MIN_W`]), the size
//! (right-aligned, [`SIZE_W`]), the status or progress ([`STATUS_W`]), the
//! primary action ([`ACTION_W`] or wider) and the "More" menu, always
//! last, which holds Delete. The description and caption span the row
//! below. Narrow rows let the slots hug (More stays last).

use crate::logic::languages::{language_label, unique_capability_languages};
use crate::logic::models::{is_legacy_model, quantization_label, ModelCardStatus};
use crate::ui::design::{self, dp};
use crate::ui::download_control::{cancel_button, progress_status, DownloadView};
use crate::ui::kit::{icon, model_description, model_name, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable};
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
    /// The item's element id (tests click it), e.g. `delete-<model>`.
    pub id: Option<SharedString>,
    pub icon: Option<&'static str>,
}

impl RowMenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            on_click: Rc::new(on_click),
            id: None,
            icon: None,
        }
    }

    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// A row's Delete… menu item (`delete-<id>`); the handler confirms. It is
/// the last item of the row's More menu, so the trash never moves.
pub fn delete_item(id: &str, on_click: impl Fn(&mut Window, &mut App) + 'static) -> RowMenuItem {
    RowMenuItem::new(t("models.actions.delete"), on_click)
        .id(format!("delete-{id}"))
        .icon("trash")
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
    caption: Option<SharedString>,
    error: Option<SharedString>,
    size: Option<SharedString>,
    status: Option<AnyElement>,
    trailing: Vec<AnyElement>,
    menu: Vec<RowMenuItem>,
    dimmed: bool,
    narrow: bool,
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
            caption: None,
            error: None,
            size: None,
            status: None,
            trailing: Vec::new(),
            menu: Vec::new(),
            dimmed: false,
            narrow: false,
            on_click: None,
        }
    }

    /// A narrow row (minimum window width): the size and the actions move
    /// to a band under the text, so they never take the description's room.
    pub fn narrow(mut self, narrow: bool) -> Self {
        self.narrow = narrow;
        self
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

    /// The muted metadata line. Every row's detail goes through
    /// [`join_detail`], so sentences in a description become " · "
    /// fragments without periods, the same as every other model line.
    pub fn detail(mut self, text: impl Into<SharedString>) -> Self {
        let text: SharedString = text.into();
        self.detail = join_detail([text.as_ref()]).into();
        self
    }

    /// A caption line under the detail ("Required by Dictation · Meetings").
    pub fn caption(mut self, text: Option<impl Into<SharedString>>) -> Self {
        self.caption = text.map(Into::into);
        self
    }

    /// Why the last download failed, as one line in the error color under
    /// the detail ("Failed" in the status column says that it did; this
    /// says why, so the reader can tell whether Retry will help). Long
    /// reasons truncate; the status keeps the full text in its tooltip.
    pub fn error(mut self, reason: Option<impl Into<SharedString>>) -> Self {
        self.error = reason
            .map(Into::into)
            .filter(|r: &SharedString| !r.trim().is_empty());
        self
    }

    pub fn size(mut self, text: impl Into<SharedString>) -> Self {
        self.size = Some(text.into());
        self
    }

    /// The state or progress: "✓ Ready", "Paused at 30%", a progress bar
    /// with "40% · 2 min" ([`STATUS_W`] slot).
    pub fn status(mut self, el: impl IntoElement) -> Self {
        self.status = Some(el.into_any_element());
        self
    }

    /// The primary action (Use, Resume, Retry, Download, cancel).
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

/// "Required by Dictation · Meetings", or nothing: a row's
/// [`ModelRow::caption`].
pub fn required_by_caption(features: &[SharedString]) -> Option<SharedString> {
    (!features.is_empty()).then(|| {
        let list = features
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(" · ");
        t_args("models.requiredBy", &[("features", &list)])
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
                .small()
                .w(design::ICON_BUTTON)
                .h(design::ICON_BUTTON)
                .icon(icon("ellipsis"))
                .tooltip(t("models.actions.more"))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                    for item in items.iter() {
                        let f = item.on_click.clone();
                        let label = item.label.clone();
                        let entry = match item.id.clone() {
                            Some(id) => PopupMenuItem::element(move |_, _| {
                                div().id(id.clone()).child(label.clone()).test_support()
                            }),
                            None => PopupMenuItem::new(label),
                        };
                        let entry = match item.icon {
                            Some(ic) => entry.icon(icon(ic)),
                            None => entry,
                        };
                        menu = menu.item(entry.on_click(move |_, window, cx| f(window, cx)));
                    }
                    menu
                })
        });
        let on_click = self.on_click.clone();
        let narrow = self.narrow;
        let has_badges = !self.badges.is_empty();
        // The name with its badges after it (wrapping under it when the
        // row is tight), the description in up to two lines, then the
        // caption on a line of its own.
        let title = h_flex()
            .flex_wrap()
            .gap_x(dp(8.))
            .gap_y(dp(2.))
            .child(
                div()
                    .min_w_0()
                    .text_size(design::TEXT_BODY)
                    .line_height(design::LINE_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tone.text)
                    .child(self.title.clone()),
            )
            .when(has_badges, |d| {
                d.child(h_flex().flex_none().gap(dp(4.)).children(self.badges))
            });
        let detail = (!self.detail.is_empty()).then(|| {
            div()
                .w_full()
                .line_clamp(2)
                .text_ellipsis()
                .text_size(design::TEXT_SECONDARY)
                .line_height(design::LINE_SECONDARY)
                .text_color(tone.text_muted)
                .child(self.detail.clone())
        });
        let caption = self.caption.clone().map(|c| {
            div()
                .w_full()
                .text_size(design::TEXT_CAPTION)
                .line_height(design::LINE_CAPTION)
                .text_color(tone.text_muted)
                .child(c)
        });
        let error = self.error.clone().map(|e| {
            div()
                .id(SharedString::from(format!("{}-error", self.id)))
                .w_full()
                .min_w_0()
                .truncate()
                .text_size(design::TEXT_SECONDARY)
                .line_height(design::LINE_SECONDARY)
                .text_color(design::status_color(design::Status::Error, cx))
                .child(e)
                .test_support()
        });
        let size = self.size.map(|size| {
            design::mono(size)
                .text_size(design::TEXT_SECONDARY)
                .line_height(design::LINE_SECONDARY)
                .text_color(tone.text_muted)
        });
        let has_controls = !self.trailing.is_empty() || menu.is_some();
        let actions = h_flex()
            .flex_none()
            .justify_end()
            .gap(dp(4.))
            .children(self.trailing);
        // The More slot is reserved even when empty, so actions keep one
        // right edge and the menu one x.
        let menu_slot = div()
            .flex_none()
            .w(design::ICON_BUTTON)
            .flex()
            .justify_center()
            .children(menu);
        // The slots sit on the title line, right-aligned; the description
        // and caption span the whole row below them. Wide rows give each
        // slot a fixed width (columns line up row to row); narrow rows let
        // them hug, and the cluster wraps under the title if it must.
        let cluster = if narrow {
            h_flex()
                .flex_none()
                .ml_auto()
                .gap(dp(8.))
                .children(size)
                .children(self.status)
                .child(actions)
                .child(menu_slot)
        } else {
            h_flex()
                .flex_none()
                .ml_auto()
                .gap(dp(8.))
                .child(h_flex().w(SIZE_W).justify_end().children(size))
                .child(h_flex().w(STATUS_W).justify_end().children(self.status))
                .child(actions.min_w(ACTION_W))
                .child(menu_slot)
        };
        let line_h = if has_controls {
            SLOT_H
        } else {
            design::LINE_BODY
        };
        let title_line = h_flex()
            .w_full()
            .min_h(line_h)
            .flex_wrap()
            .items_center()
            .gap_x(dp(8.))
            .gap_y(dp(2.))
            .child(
                title
                    .flex_1()
                    .min_w(if narrow { dp(120.) } else { NAME_MIN_W }),
            )
            .child(cluster);
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap(dp(2.))
            .child(title_line)
            .children(detail)
            .children(caption)
            .children(error);
        h_flex()
            .id(self.id.clone())
            .w_full()
            .min_h(dp(44.))
            .px(design::SECTION_X)
            .py(dp(8.))
            .gap(dp(8.))
            .items_start()
            .when(self.dimmed, |d| d.opacity(0.55))
            .when_some(on_click, |d, f| {
                d.hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, window, cx| f(window, cx))
            })
            .child(
                div()
                    .flex_none()
                    .w(dp(16.))
                    .h(line_h)
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(self.leading),
            )
            .child(text)
            .test_support()
    }
}

/// The name column never gets narrower than this (badges never take it).
pub const NAME_MIN_W: Pixels = dp(180.);
/// The size column (right-aligned, fits "1.6 GB").
pub const SIZE_W: Pixels = dp(64.);
/// The status/progress column (fits "Paused at 30%", or a 72 pt bar with
/// "40% · 2 min").
pub const STATUS_W: Pixels = dp(160.);
/// The primary action's slot (fits Resume, Retry, Use; wider labels grow
/// it).
pub const ACTION_W: Pixels = dp(88.);
/// The title line's height when it holds a control.
const SLOT_H: Pixels = dp(26.);

/// Whether model rows on a page `content_w` wide are narrow
/// ([`ModelRow::narrow`]).
pub fn narrow_rows(content_w: Pixels) -> bool {
    content_w < dp(600.)
}

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
    narrow: bool,
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
            narrow: false,
            on_select: None,
            on_download: None,
            on_delete: None,
            on_cancel: None,
            on_use_for_meetings: None,
        }
    }

    /// A narrow row ([`ModelRow::narrow`]).
    pub fn narrow(mut self, n: bool) -> Self {
        self.narrow = n;
        self
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
            // Debug builds: "Q8_0 quantization", not a bare code.
            parts.push(guidy_i18n::t_args(
                "modelSelector.quantization",
                &[("label", &q)],
            ));
        }
    }
    join_detail(parts)
}

/// One metadata line from a model's parts: "Fast and accurate · 25
/// languages · Streaming". The line is fragments joined by " · " only: a
/// part written as sentences ("Optimized for Taiwanese Mandarin.
/// Code-switching support.") is split into its sentences, each loses its
/// period, and empty parts drop out, so a line never mixes ". " and " · ".
/// The Models page ([`model_detail`]) and the onboarding picker use it, so
/// a model reads the same in both.
pub fn join_detail<S: AsRef<str>>(parts: impl IntoIterator<Item = S>) -> String {
    parts
        .into_iter()
        .flat_map(|p| sentence_fragments(p.as_ref()))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// "A. B." → ["A", "B"]: splits at a sentence end (". " or "。" before
/// the next sentence) and drops each fragment's final period. A period
/// inside a token ("v2.5", "e.g. small") stays: only ". " followed by a
/// capital letter or a digit ends a sentence.
fn sentence_fragments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(at, c)) in chars.iter().enumerate() {
        let next = chars.get(i + 1).map(|&(_, n)| n);
        let after = chars.get(i + 2).map(|&(_, n)| n);
        let ends = match c {
            '。' => true,
            '.' => {
                next == Some(' ') && after.is_some_and(|a| a.is_uppercase() || a.is_ascii_digit())
            }
            _ => false,
        };
        if ends {
            out.push(text[start..at].to_string());
            start = at + c.len_utf8();
        }
    }
    out.push(text[start..].to_string());
    out.into_iter()
        .map(|f| {
            f.trim()
                .trim_end_matches(['.', '。'])
                .trim_end()
                .to_string()
        })
        .filter(|f| !f.is_empty())
        .collect()
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
            .narrow(self.narrow)
            .dimmed(disabled && status.is_clickable());
        if active {
            // In use: a neutral check (the accent is for actions).
            row = row.leading(icon("check").small().text_color(tone.text));
        }
        row = row.caption(required_by_caption(&self.required_by));
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
            row = row.badge(design::chip("", false, cx).child(design::shimmer_text(
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
        let narrow = self.narrow;
        // Every action keeps its label (Resume, Retry too): a narrow row
        // has the room in its action band.
        let action = |key: &str, ic: &str, label: SharedString, f: Option<ModelHandler>| {
            let id = id.clone();
            Button::new(SharedString::from(format!("model-{key}-{}", m.id)))
                .outline()
                .label(label.clone())
                .icon(icon(ic))
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
                row = row.status(progress_status(&tracker_id, &view, track_w(narrow), cx));
                let cancel = match self.on_cancel.clone() {
                    // The card's own cancel (`cancel-<model>`).
                    Some(cancel) => {
                        let id = id.clone();
                        (!matches!(view, DownloadView::Verifying)).then(|| {
                            cancel_icon_button(
                                SharedString::from(format!("cancel-{}", m.id)),
                                t("modelSelector.cancelDownload"),
                                move |window, cx| cancel(&id, window, cx),
                            )
                            .into_any_element()
                        })
                    }
                    None => cancel_button(&tracker_id, &view),
                };
                if let Some(cancel) = cancel {
                    row = row.trailing(cancel);
                }
            }
            ModelCardStatus::Failed => {
                let reason: Option<SharedString> = match &view {
                    DownloadView::Failed { error: Some(e) } => Some(e.clone().into()),
                    _ => None,
                };
                let error = reason.clone().unwrap_or_else(|| t("models.state.failed"));
                row = row
                    .error(reason)
                    .status(
                        status_word(
                            "circle-alert",
                            t("models.state.failed"),
                            design::Status::Error,
                            cx,
                        )
                        .id(SharedString::from(format!("model-error-{}", m.id)))
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
                    .status(status_word(
                        "circle-pause",
                        label,
                        design::Status::Neutral,
                        cx,
                    ))
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
                row = row.menu_item(delete_item(&m.id, move |window, cx| f(&id, window, cx)));
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
            design::panel(cx).child(row).into_any_element()
        }
    }
}

/// A state as a status icon and a word ("⊘ Failed", "⏸ Paused at 30%"):
/// the icon in the status color, the word muted (red for errors).
pub fn status_word(
    ic: &str,
    word: impl Into<SharedString>,
    status: design::Status,
    cx: &App,
) -> Div {
    let color = design::status_color(status, cx);
    let text = if status == design::Status::Error {
        color
    } else {
        design::tone(cx).text_muted
    };
    h_flex()
        .flex_none()
        .gap(dp(4.))
        .text_size(design::TEXT_SECONDARY)
        .line_height(design::LINE_SECONDARY)
        .text_color(text)
        .child(icon(ic).xsmall().text_color(color))
        .child(word.into())
}

/// Width of a row's progress track.
pub fn track_w(narrow: bool) -> Pixels {
    if narrow {
        dp(48.)
    } else {
        dp(72.)
    }
}

/// A standalone icon action: 24 × 24 pt hit area around a 16 pt glyph.
pub fn cancel_icon_button(
    id: SharedString,
    tooltip: SharedString,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .w(design::ICON_BUTTON)
        .h(design::ICON_BUTTON)
        .icon(icon("x"))
        .tooltip(tooltip)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_click(window, cx)
        })
}

#[cfg(test)]
mod tests {
    use super::{describes_languages, join_detail};

    #[test]
    fn detail_parts_join_without_trailing_periods() {
        assert_eq!(
            join_detail([
                "Fast and accurate. Supports 25 European languages.",
                "Streaming",
                ""
            ]),
            "Fast and accurate · Supports 25 European languages · Streaming"
        );
        assert_eq!(
            join_detail([
                "Optimized for Taiwanese Mandarin. Code-switching support.",
                "2 languages"
            ]),
            "Optimized for Taiwanese Mandarin · Code-switching support · 2 languages"
        );
        // A period inside a token is not a sentence end.
        assert_eq!(
            join_detail(["Whisper v2.5 model", "e.g. small"]),
            "Whisper v2.5 model · e.g. small"
        );
        assert_eq!(
            join_detail(["Good accuracy, but slow.", "20 languages"]),
            "Good accuracy, but slow · 20 languages"
        );
    }

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
