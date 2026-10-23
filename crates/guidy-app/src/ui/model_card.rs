//! Model card, shared by onboarding and the Models page.

use crate::logic::languages::{language_label, unique_capability_languages};
use crate::logic::models::{
    format_model_size, is_legacy_model, quantization_label, ModelCardStatus,
};
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{icon, model_description, model_name, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::model::ModelInfo;
use std::rc::Rc;

type ModelHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct ModelCard {
    model: ModelInfo,
    status: ModelCardStatus,
    featured: bool,
    /// A row inside a shared panel instead of its own bordered card.
    compact: bool,
    disabled: bool,
    show_recommended: bool,
    debug_mode: bool,
    progress: Option<f64>,
    speed: Option<f64>,
    on_select: Option<ModelHandler>,
    on_download: Option<ModelHandler>,
    on_delete: Option<ModelHandler>,
    on_cancel: Option<ModelHandler>,
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
            speed: None,
            on_select: None,
            on_download: None,
            on_delete: None,
            on_cancel: None,
        }
    }

    /// Render as a hairline row for a grouped list (see `kit::group`).
    pub fn compact(mut self, c: bool) -> Self {
        self.compact = c;
        self
    }

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
    pub fn progress(mut self, p: Option<f64>, speed: Option<f64>) -> Self {
        self.progress = p;
        self.speed = speed;
        self
    }
    pub fn on_select(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
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

/// Five-step meter for a 0–1 score.
fn pips(value: f32, cx: &App) -> Div {
    let t = design::tone(cx);
    let filled = (value.clamp(0.0, 1.0) * 5.0).round() as usize;
    h_flex().gap(dp(3.)).children((0..5).map(move |i| {
        div()
            .w(dp(10.))
            .h(dp(4.))
            .rounded(dp(2.))
            .bg(if i < filled { t.text } else { t.raised })
    }))
}

impl RenderOnce for ModelCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let m = &self.model;
        let id = m.id.clone();
        let clickable = self.status.is_clickable();
        let active = self.status == ModelCardStatus::Active;
        let name = model_name(m);
        let description = model_description(m);
        let show_size = matches!(
            self.status,
            ModelCardStatus::Downloadable | ModelCardStatus::Available | ModelCardStatus::Active
        );
        let langs = unique_capability_languages(&m.supported_languages);
        let quant = quantization_label(&m.filename);
        let size_text = {
            let size = format_model_size(Some(m.size_mb as f64));
            match (&quant, self.debug_mode) {
                (Some(q), true) => format!("{size} · {q}"),
                _ => size,
            }
        };

        let mut badges = h_flex().gap(dp(6.)).flex_wrap();
        if self.show_recommended && m.is_recommended {
            badges = badges.child(design::chip(t("onboarding.recommended"), true, cx));
        }
        if active {
            badges = badges.child(
                design::chip("", true, cx)
                    .child(icon("check").size(dp(11.)))
                    .child(t("modelSelector.active")),
            );
        }
        if m.is_custom {
            badges = badges.child(design::chip(t("modelSelector.custom"), false, cx));
        }
        if is_legacy_model(m) {
            badges = badges.child(design::chip(t("modelSelector.legacy"), false, cx));
        }
        if self.status == ModelCardStatus::Switching {
            badges = badges.child(design::chip("", true, cx).child(design::shimmer_text(
                "switching",
                t("modelSelector.switching"),
                cx,
            )));
        }

        // Capabilities, each with a tooltip: chips beside the name in a list
        // row, mono labels in the active card's footer.
        let muted = tone.text_muted;
        let as_row = self.compact && !active;
        let cap = |key: &str, label: String, tip: SharedString| {
            div()
                .id(SharedString::from(format!("cap-{key}")))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(if as_row {
                    design::chip(label, false, cx)
                } else {
                    design::mono(label).text_color(muted)
                })
        };
        let mut caps: Vec<AnyElement> = Vec::new();
        if !langs.is_empty() {
            let tip = if langs.len() == 1 {
                t("modelSelector.capabilities.singleLanguage")
            } else {
                t("modelSelector.capabilities.languageSelection")
            };
            caps.push(
                cap("globe", language_display_text(&m.supported_languages), tip).into_any_element(),
            );
        }
        if m.supports_translation {
            caps.push(
                cap(
                    "languages",
                    guidy_i18n::t("modelSelector.capabilities.translate"),
                    t("modelSelector.capabilities.translation"),
                )
                .into_any_element(),
            );
        }
        if m.supports_streaming {
            caps.push(
                cap(
                    "audio-lines",
                    guidy_i18n::t("modelSelector.streaming"),
                    t("modelSelector.capabilities.streaming"),
                )
                .into_any_element(),
            );
        }

        let delete = self
            .on_delete
            .clone()
            .filter(|_| self.status.can_delete())
            .map(|on_delete| {
                let id = id.clone();
                Button::new(SharedString::from(format!("delete-{}", m.id)))
                    .ghost()
                    .xsmall()
                    .pill()
                    .icon(icon("trash"))
                    .label(t("common.delete"))
                    .tooltip(t_args("modelSelector.deleteModel", &[("modelName", &name)]))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        on_delete(&id, window, cx)
                    })
            });

        let (caps, title_caps) = if as_row {
            (Vec::new(), caps)
        } else {
            (caps, Vec::new())
        };
        badges = badges.children(title_caps);
        let scored = m.accuracy_score > 0.0 || m.speed_score > 0.0;
        // Active model: specs as a stat grid. Others: one meta line.
        let details: AnyElement = if active {
            let mut stats = Vec::new();
            if scored {
                stats.push((
                    t("onboarding.modelCard.accuracy"),
                    pips(m.accuracy_score, cx).into_any_element(),
                ));
                stats.push((
                    t("onboarding.modelCard.speed"),
                    pips(m.speed_score, cx).into_any_element(),
                ));
            }
            stats.push((
                t("onboarding.modelCard.size"),
                design::mono(size_text.clone()).into_any_element(),
            ));
            v_flex()
                .w_full()
                .border_t_1()
                .border_color(tone.hairline)
                .child(
                    h_flex()
                        .w_full()
                        .children(stats.into_iter().enumerate().map(|(i, (label, value))| {
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .px(design::SECTION_X)
                                .py(dp(12.))
                                .gap(dp(8.))
                                .when(i > 0, |d| d.border_l_1().border_color(tone.hairline))
                                .child(design::eyebrow(label, cx))
                                .child(div().h(dp(16.)).flex().items_center().child(value))
                        })),
                )
                .when(!caps.is_empty() || delete.is_some(), |d| {
                    d.child(
                        h_flex()
                            .w_full()
                            .px(design::SECTION_X)
                            .py(dp(8.))
                            .gap(dp(14.))
                            .border_t_1()
                            .border_color(tone.hairline)
                            .children(caps)
                            .child(div().flex_1())
                            .children(delete),
                    )
                })
                .into_any_element()
        } else {
            h_flex()
                .w_full()
                .px(design::SECTION_X)
                .pb(dp(12.))
                .gap_x(dp(14.))
                .gap_y(dp(6.))
                .flex_wrap()
                .when(scored, |d| {
                    d.child(
                        h_flex()
                            .gap(dp(6.))
                            .child(design::eyebrow(t("onboarding.modelCard.accuracy"), cx))
                            .child(pips(m.accuracy_score, cx)),
                    )
                    .child(
                        h_flex()
                            .gap(dp(6.))
                            .child(design::eyebrow(t("onboarding.modelCard.speed"), cx))
                            .child(pips(m.speed_score, cx)),
                    )
                })
                .when(show_size, |d| {
                    d.child(design::mono(size_text.clone()).text_color(muted))
                })
                .children(caps)
                .child(div().flex_1())
                .children(delete)
                .into_any_element()
        };

        let progress_section = match (self.status, self.progress) {
            (ModelCardStatus::Downloading, Some(p)) => {
                let cancel = self.on_cancel.clone();
                let id = id.clone();
                let mut label = t_args(
                    "modelSelector.downloading",
                    &[("percentage", &format!("{}", p.round() as i64))],
                )
                .to_string();
                if let Some(speed) = self.speed.filter(|s| *s > 0.0) {
                    label = format!(
                        "{label} · {}",
                        guidy_i18n::t_args(
                            "modelSelector.downloadSpeed",
                            &[("speed", &format!("{speed:.1}"))]
                        )
                    );
                }
                Some(
                    v_flex()
                        .w_full()
                        .px(design::SECTION_X)
                        .pb(dp(12.))
                        .gap(dp(8.))
                        .child(
                            h_flex()
                                .justify_between()
                                .child(
                                    div()
                                        .font_family(crate::theme::MONO_FONT)
                                        .text_size(design::TEXT_MONO)
                                        .child(design::shimmer_text(
                                            SharedString::from(format!("dl-label-{}", m.id)),
                                            label,
                                            cx,
                                        )),
                                )
                                .when_some(cancel, |d, cancel| {
                                    d.child(
                                        Button::new(SharedString::from(format!("cancel-{}", m.id)))
                                            .ghost()
                                            .xsmall()
                                            .pill()
                                            .label(t("modelSelector.cancel"))
                                            .tooltip(t("modelSelector.cancelDownload"))
                                            .on_click(move |_, window, cx| {
                                                cx.stop_propagation();
                                                cancel(&id, window, cx)
                                            }),
                                    )
                                }),
                        )
                        .child(design::track(
                            SharedString::from(format!("progress-{}", m.id)),
                            Some(p as f32 / 100.0),
                            cx,
                        ))
                        .into_any_element(),
                )
            }
            (ModelCardStatus::Verifying, _) => Some(
                v_flex()
                    .w_full()
                    .px(design::SECTION_X)
                    .pb(dp(12.))
                    .gap(dp(8.))
                    .child(
                        div()
                            .font_family(crate::theme::MONO_FONT)
                            .text_size(design::TEXT_MONO)
                            .child(design::shimmer_text(
                                SharedString::from(format!("verify-label-{}", m.id)),
                                t("modelSelector.verifyingGeneric"),
                                cx,
                            )),
                    )
                    .child(design::track(
                        SharedString::from(format!("verify-{}", m.id)),
                        None,
                        cx,
                    ))
                    .into_any_element(),
            ),
            _ => None,
        };

        let on_select = self.on_select.clone();
        let on_download = self.on_download.clone();
        let status = self.status;
        let disabled = self.disabled;
        let border = if active {
            tone.accent_text.opacity(0.45)
        } else if self.featured {
            tone.accent_text.opacity(0.25)
        } else {
            tone.hairline
        };
        let hover_border = tone.accent_text.opacity(0.4);
        let hover_bg = tone.raised.opacity(0.35);
        let compact = self.compact && !active;
        let container = if compact {
            v_flex().w_full()
        } else {
            design::panel(cx).border_color(border)
        };
        // The explicit action a row offers (the whole row is clickable too).
        let action_label = match self.status {
            ModelCardStatus::Downloadable => Some(("download", t("modelSelector.download"))),
            ModelCardStatus::Available => Some(("check", t("modelSelector.use"))),
            _ => None,
        };
        let action = action_label.filter(|_| compact).map(|(ic, label)| {
            let handler = if status == ModelCardStatus::Downloadable {
                on_download.clone().or_else(|| on_select.clone())
            } else {
                on_select.clone()
            };
            let id = id.clone();
            Button::new(SharedString::from(format!("model-action-{}", m.id)))
                .secondary()
                .small()
                .pill()
                .icon(icon(ic))
                .label(label)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if let Some(f) = &handler {
                        f(&id, window, cx);
                    }
                })
        });
        container
            .id(SharedString::from(format!("model-card-{}", m.id)))
            .aria_label(SharedString::from(name.clone()))
            .when(clickable && disabled, |d| d.opacity(0.5))
            .when(clickable && !disabled && compact, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover_bg))
            })
            .when(clickable && !disabled && !compact, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg).border_color(hover_border))
            })
            .on_click(move |_, window, cx| {
                if !clickable || disabled {
                    return;
                }
                if status == ModelCardStatus::Downloadable {
                    if let Some(f) = &on_download {
                        f(&id, window, cx);
                        return;
                    }
                }
                if let Some(f) = &on_select {
                    f(&id, window, cx);
                }
            })
            .child(
                h_flex()
                    .w_full()
                    .px(design::SECTION_X)
                    .pt(dp(12.))
                    .pb(dp(if active { 12. } else { 8. }))
                    .gap(dp(12.))
                    .justify_between()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(dp(4.))
                            .child(
                                h_flex()
                                    .gap(dp(8.))
                                    .flex_wrap()
                                    .child(
                                        div()
                                            .text_size(if active {
                                                design::TEXT_TITLE
                                            } else {
                                                design::TEXT_BODY
                                            })
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(name.clone()),
                                    )
                                    .child(badges),
                            )
                            .child(
                                div()
                                    .text_size(dp(12.))
                                    .line_height(relative(1.5))
                                    .text_color(muted)
                                    .child(description),
                            ),
                    )
                    .children(action)
                    .when(
                        !compact && self.status == ModelCardStatus::Downloadable,
                        |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .text_color(muted)
                                    .child(icon("download").small()),
                            )
                        },
                    ),
            )
            .child(details)
            .children(progress_section)
            .test_support()
    }
}
