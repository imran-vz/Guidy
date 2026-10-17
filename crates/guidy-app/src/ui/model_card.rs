//! Model card (Handy's `ModelCard`), shared by onboarding and the Models page.

use crate::logic::languages::{language_label, unique_capability_languages};
use crate::logic::models::{
    format_model_size, is_legacy_model, quantization_label, ModelCardStatus,
};
use crate::ui::kit::{icon, model_description, model_name, spinner, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, StyledExt};
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

fn score_bar(label: SharedString, value: f32, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .gap_2()
        .child(
            div()
                .w(px(96.))
                .text_right()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .w(px(64.))
                .h(px(6.))
                .rounded_full()
                .bg(theme.muted)
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(theme.primary)
                        .w(relative(value.clamp(0.0, 1.0))),
                ),
        )
}

impl RenderOnce for ModelCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let m = &self.model;
        let id = m.id.clone();
        let clickable = self.status.is_clickable();
        let name = model_name(m);
        let description = model_description(m);
        let show_size = matches!(
            self.status,
            ModelCardStatus::Downloadable | ModelCardStatus::Available | ModelCardStatus::Active
        );
        let langs = unique_capability_languages(&m.supported_languages);
        let quant = quantization_label(&m.filename);

        let (border, bg) = if self.status == ModelCardStatus::Active {
            (theme.primary.opacity(0.6), theme.primary.opacity(0.08))
        } else if self.featured {
            (theme.primary.opacity(0.3), theme.primary.opacity(0.04))
        } else {
            (theme.border, theme.sidebar)
        };

        let mut badges = h_flex().gap_2().flex_wrap();
        if self.show_recommended && m.is_recommended {
            badges = badges.child(Tag::primary().small().child(t("onboarding.recommended")));
        }
        if self.status == ModelCardStatus::Active {
            badges = badges.child(
                Tag::primary().small().child(
                    h_flex()
                        .gap_1()
                        .child(icon("check").xsmall())
                        .child(t("modelSelector.active")),
                ),
            );
        }
        if m.is_custom {
            badges = badges.child(Tag::secondary().small().child(t("modelSelector.custom")));
        }
        if is_legacy_model(m) {
            badges = badges.child(Tag::secondary().small().child(t("modelSelector.legacy")));
        }
        if self.status == ModelCardStatus::Switching {
            badges = badges.child(
                Tag::secondary().small().child(
                    h_flex()
                        .gap_1()
                        .child(spinner("switching-spin", icon("loader-circle").xsmall()))
                        .child(t("modelSelector.switching")),
                ),
            );
        }

        let muted = theme.muted_foreground;
        let tag = |icon_name: &str, label: String, tip: SharedString| {
            h_flex()
                .id(SharedString::from(format!("cap-{icon_name}")))
                .gap_1()
                .text_xs()
                .text_color(muted)
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(icon(icon_name).xsmall())
                .child(label)
        };

        let mut bottom = h_flex().gap_3().w_full().h(px(20.));
        if !langs.is_empty() {
            let tip = if langs.len() == 1 {
                t("modelSelector.capabilities.singleLanguage")
            } else {
                t("modelSelector.capabilities.languageSelection")
            };
            bottom = bottom.child(tag(
                "globe",
                language_display_text(&m.supported_languages),
                tip,
            ));
        }
        if m.supports_translation {
            bottom = bottom.child(tag(
                "languages",
                guidy_i18n::t("modelSelector.capabilities.translate"),
                t("modelSelector.capabilities.translation"),
            ));
        }
        if m.supports_streaming {
            bottom = bottom.child(tag(
                "audio-lines",
                guidy_i18n::t("modelSelector.streaming"),
                t("modelSelector.capabilities.streaming"),
            ));
        }
        if show_size {
            bottom = bottom.child(
                h_flex()
                    .ml_auto()
                    .gap_1p5()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        icon(if self.status == ModelCardStatus::Downloadable {
                            "download"
                        } else {
                            "hard-drive"
                        })
                        .xsmall(),
                    )
                    .child(format_model_size(Some(m.size_mb as f64)))
                    .when(self.debug_mode && quant.is_some(), |d| {
                        d.child(div().opacity(0.7).child(quant.clone().unwrap_or_default()))
                    }),
            );
        }
        if let Some(on_delete) = self.on_delete.clone() {
            if self.status.can_delete() {
                let id = id.clone();
                bottom = bottom.child(
                    Button::new(SharedString::from(format!("delete-{}", m.id)))
                        .ghost()
                        .xsmall()
                        .icon(icon("trash"))
                        .label(t("common.delete"))
                        .tooltip(t_args("modelSelector.deleteModel", &[("modelName", &name)]))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            on_delete(&id, window, cx)
                        }),
                );
            }
        }

        let progress_section = match (self.status, self.progress) {
            (ModelCardStatus::Downloading, Some(p)) => {
                let cancel = self.on_cancel.clone();
                let id = id.clone();
                Some(
                    v_flex()
                        .w_full()
                        .mt_2()
                        .gap_1()
                        .child(
                            Progress::new(SharedString::from(format!("progress-{}", m.id)))
                                .value(p as f32),
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .text_xs()
                                .text_color(muted)
                                .child(t_args(
                                    "modelSelector.downloading",
                                    &[("percentage", &format!("{}", p.round() as i64))],
                                ))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .when(self.speed.is_some_and(|s| s > 0.0), |d| {
                                            d.child(t_args(
                                                "modelSelector.downloadSpeed",
                                                &[(
                                                    "speed",
                                                    &format!("{:.1}", self.speed.unwrap_or(0.0)),
                                                )],
                                            ))
                                        })
                                        .when_some(cancel, |d, cancel| {
                                            d.child(
                                                Button::new(SharedString::from(format!(
                                                    "cancel-{}",
                                                    m.id
                                                )))
                                                .ghost()
                                                .xsmall()
                                                .label(t("modelSelector.cancel"))
                                                .tooltip(t("modelSelector.cancelDownload"))
                                                .on_click(move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    cancel(&id, window, cx)
                                                }),
                                            )
                                        }),
                                ),
                        )
                        .into_any_element(),
                )
            }
            (ModelCardStatus::Verifying, _) => Some(
                v_flex()
                    .w_full()
                    .mt_2()
                    .gap_1()
                    .child(
                        Progress::new(SharedString::from(format!("verify-{}", m.id))).loading(true),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(t("modelSelector.verifyingGeneric")),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };

        let on_select = self.on_select.clone();
        let on_download = self.on_download.clone();
        let status = self.status;
        let disabled = self.disabled;
        let hover_bg = theme.primary.opacity(0.06);
        let hover_border = theme.primary.opacity(0.5);
        v_flex()
            .id(SharedString::from(format!("model-card-{}", m.id)))
            .aria_label(SharedString::from(name.clone()))
            .w_full()
            .px_4()
            .py_3()
            .gap_2()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(border)
            .bg(bg)
            .when(clickable && disabled, |d| d.opacity(0.5))
            .when(clickable && !disabled, |d| {
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
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .flex_wrap()
                                    .child(div().font_semibold().child(name.clone()))
                                    .child(badges),
                            )
                            .child(div().text_sm().text_color(muted).child(description)),
                    )
                    .when(m.accuracy_score > 0.0 || m.speed_score > 0.0, |d| {
                        d.child(
                            v_flex()
                                .ml_4()
                                .gap_1()
                                .child(score_bar(
                                    t("onboarding.modelCard.accuracy"),
                                    m.accuracy_score,
                                    cx,
                                ))
                                .child(score_bar(
                                    t("onboarding.modelCard.speed"),
                                    m.speed_score,
                                    cx,
                                )),
                        )
                    }),
            )
            .child(div().w_full().h(px(1.)).bg(theme.border))
            .child(bottom)
            .children(progress_section)
            .test_support()
    }
}
