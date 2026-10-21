//! Usage page: what cloud LLM calls have cost, per model (total, average,
//! highest call) and per feature; and what local models did on this Mac
//! (audio transcribed and how much faster than real time, LLM tokens and
//! speed, load times).

use crate::logic::usage::{
    format_duration, format_speedup, format_tokens, format_tokens_per_second, format_usd,
    format_wait,
};
use crate::state::AppState;
use crate::ui::design::{self, Pill};
use crate::ui::kit::{group, icon, page, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::llm::usage::{
    LocalKind, LocalModelUsage, LocalUsageReport, ModelSpend, UsageFeature, UsageReport,
    CLOUD_TRANSCRIPTION_USD_PER_MIN,
};

pub struct UsagePage {
    report: Option<Result<UsageReport, String>>,
}

impl UsagePage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self { report: None };
        this.reload(cx);
        this
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_spawn(async move { backend.llm_usage_report() })
                .await;
            this.update(cx, |this, cx| {
                this.report = Some(report);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("usage.clearTitle"))
                .description(t("usage.clearConfirm"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| this.clear(cx)).ok();
                    true
                })
        });
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = cx
                .background_spawn(async move { backend.clear_llm_usage() })
                .await;
            this.update(cx, |this, cx| this.reload(cx)).ok();
        })
        .detach();
    }

    fn model_row(&self, m: &ModelSpend, provider: SharedString, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let accent = design::tone(cx).accent_text;
        let mut facts = vec![t_args("usage.calls", &[("count", &m.calls.to_string())]).to_string()];
        if let Some(avg) = m.avg_usd {
            facts.push(t_args("usage.average", &[("cost", &format_usd(avg))]).to_string());
        }
        if let Some(max) = m.max_usd {
            facts.push(t_args("usage.highest", &[("cost", &format_usd(max))]).to_string());
        }
        facts.push(
            t_args(
                "usage.tokens",
                &[
                    ("input", &format_tokens(m.input_tokens)),
                    ("output", &format_tokens(m.output_tokens)),
                ],
            )
            .to_string(),
        );
        let priced = m.calls - m.unpriced_calls;
        let total = if priced == 0 {
            t("usage.unknownPrice")
        } else if m.estimated_calls > 0 || m.unpriced_calls > 0 {
            format!("~{}", format_usd(m.total_usd)).into()
        } else {
            format_usd(m.total_usd).into()
        };
        h_flex()
            .id(SharedString::from(format!(
                "usage-model-{}-{}",
                m.provider_id, m.model
            )))
            .w_full()
            .gap_3()
            .px_3()
            .py_2()
            .justify_between()
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(design::mono(m.model.clone()).truncate())
                            .child(div().text_xs().text_color(muted).child(provider)),
                    )
                    .child(div().text_xs().text_color(muted).child(facts.join(" · "))),
            )
            .child(design::mono(total).flex_none().text_color(accent))
            .test_support()
            .into_any_element()
    }
}

fn feature_key(f: UsageFeature) -> &'static str {
    match f {
        UsageFeature::Dictation => "usage.feature.dictation",
        UsageFeature::MeetingTranscription => "usage.feature.meetingTranscription",
        UsageFeature::MeetingSummary => "usage.feature.meetingSummary",
        UsageFeature::Other => "usage.feature.other",
    }
}

fn kind_key(k: LocalKind) -> &'static str {
    match k {
        LocalKind::Transcription => "usage.kind.transcription",
        LocalKind::Llm => "usage.kind.llm",
    }
}

/// One local model: what it did, and its speed as the headline.
fn local_model_row(m: &LocalModelUsage, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let accent = design::tone(cx).accent_text;
    let mut facts = vec![t_args("usage.runs", &[("count", &m.runs.to_string())]).to_string()];
    let headline = match m.kind {
        LocalKind::Transcription => {
            facts.push(
                t_args("usage.audio", &[("duration", &format_duration(m.audio_ms))]).to_string(),
            );
            facts.push(t_args("usage.words", &[("count", &format_tokens(m.words))]).to_string());
            m.realtime_factor()
                .map(|x| t_args("usage.realtime", &[("speed", &format_speedup(x))]))
        }
        LocalKind::Llm => {
            facts.push(
                t_args(
                    "usage.tokens",
                    &[
                        ("input", &format_tokens(m.input_tokens)),
                        ("output", &format_tokens(m.output_tokens)),
                    ],
                )
                .to_string(),
            );
            if let Some(x) = m.prompt_tokens_per_second() {
                facts.push(
                    t_args(
                        "usage.promptSpeed",
                        &[("speed", &format_tokens_per_second(x))],
                    )
                    .to_string(),
                );
            }
            m.generation_tokens_per_second().map(|x| {
                t_args(
                    "usage.generationSpeed",
                    &[("speed", &format_tokens_per_second(x))],
                )
            })
        }
    };
    if let Some(ms) = m.avg_load_ms() {
        facts.push(t_args("usage.loadTime", &[("time", &format_wait(ms))]).to_string());
    }
    h_flex()
        .id(SharedString::from(format!(
            "usage-local-{}-{}",
            m.kind.as_str(),
            m.model
        )))
        .w_full()
        .gap_3()
        .px_3()
        .py_2()
        .justify_between()
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .gap(px(2.))
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .child(design::mono(m.model.clone()).truncate())
                        .child(div().text_xs().text_color(muted).child(t(kind_key(m.kind)))),
                )
                .child(div().text_xs().text_color(muted).child(facts.join(" · "))),
        )
        .when_some(headline, |d, h| {
            d.child(design::mono(h).flex_none().text_color(accent))
        })
        .test_support()
        .into_any_element()
}

/// The "On this Mac" section.
fn local_section(local: &LocalUsageReport, cx: &App) -> Vec<AnyElement> {
    let muted = cx.theme().muted_foreground;
    if local.models.is_empty() {
        return vec![group(
            Some(t("usage.local")),
            vec![div()
                .id("usage-local-empty")
                .px_4()
                .py(px(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(muted)
                .child(t("usage.localEmpty"))
                .test_support()
                .into_any_element()],
            cx,
        )
        .into_any_element()];
    }
    let totals = design::panel(cx).child(design::stat_grid(
        vec![
            design::Stat::new(t("usage.localAudio"), format_duration(local.audio_ms)).accent(true),
            design::Stat::new(t("usage.localWords"), format_tokens(local.words)),
            design::Stat::new(t("usage.localTokens"), format_tokens(local.tokens)),
            design::Stat::new(
                t("usage.localCloudEquivalent"),
                format!("~{}", format_usd(local.cloud_transcription_usd())),
            ),
        ],
        4,
        cx,
    ));
    let models = local
        .models
        .iter()
        .map(|m| local_model_row(m, cx))
        .collect();
    let features: Vec<AnyElement> = local
        .features
        .iter()
        .map(|f| {
            let amount = if f.audio_ms > 0 {
                t_args("usage.audio", &[("duration", &format_duration(f.audio_ms))])
            } else {
                t_args(
                    "usage.tokens",
                    &[("input", "–"), ("output", &format_tokens(f.output_tokens))],
                )
            };
            h_flex()
                .w_full()
                .px_3()
                .py_2()
                .justify_between()
                .child(div().text_sm().child(t(feature_key(f.feature))))
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(t_args("usage.runs", &[("count", &f.runs.to_string())])),
                        )
                        .child(design::mono(amount)),
                )
                .into_any_element()
        })
        .collect();
    vec![
        div()
            .px(px(2.))
            .child(design::eyebrow(t("usage.local"), cx))
            .into_any_element(),
        totals.into_any_element(),
        group(Some(t("usage.localByModel")), models, cx).into_any_element(),
        group(Some(t("usage.localByFeature")), features, cx).into_any_element(),
        div()
            .text_xs()
            .line_height(relative(1.5))
            .text_color(muted)
            .child(t_args(
                "usage.localNote",
                &[("price", &CLOUD_TRANSCRIPTION_USD_PER_MIN.to_string())],
            ))
            .into_any_element(),
    ]
}

impl Render for UsagePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let heading = h_flex()
            .w_full()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(design::TEXT_HEADLINE)
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("usage.title")),
                    )
                    .child(
                        div()
                            .text_size(design::TEXT_BODY)
                            .line_height(relative(1.55))
                            .text_color(muted)
                            .child(t("usage.description")),
                    ),
            )
            .child(
                Button::new("usage-refresh")
                    .ghost()
                    .pill()
                    .small()
                    .icon(icon("refresh-cw"))
                    .label(t("usage.refresh"))
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            );
        let report = match &self.report {
            None => {
                return page()
                    .id("page-usage")
                    .child(heading)
                    .child(div().text_color(muted).child(t("common.loading")))
                    .test_support()
                    .into_any_element()
            }
            Some(Err(e)) => {
                return page()
                    .id("page-usage")
                    .child(heading)
                    .child(div().text_color(muted).child(e.clone()))
                    .test_support()
                    .into_any_element()
            }
            Some(Ok(r)) => r.clone(),
        };
        let estimated = report
            .models
            .iter()
            .any(|m| m.estimated_calls > 0 || m.unpriced_calls > 0);
        let totals = design::panel(cx).child(design::stat_grid(
            vec![
                design::Stat::new(t("usage.total"), format_usd(report.total_usd)).accent(true),
                design::Stat::new(t("usage.last30Days"), format_usd(report.last_30_days_usd)),
                design::Stat::new(t("usage.callsTitle"), report.calls.to_string()),
            ],
            3,
            cx,
        ));
        let settings = AppState::global(cx).read(cx).app_settings().cloned();
        let provider_label = |id: &str| -> SharedString {
            settings
                .as_ref()
                .and_then(|s| s.post_process_provider(id))
                .map(|p| p.label.clone())
                .unwrap_or_else(|| id.to_string())
                .into()
        };
        let models: Vec<AnyElement> = if report.models.is_empty() {
            vec![div()
                .id("usage-empty")
                .px_4()
                .py(px(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(muted)
                .child(t("usage.empty"))
                .test_support()
                .into_any_element()]
        } else {
            report
                .models
                .iter()
                .map(|m| self.model_row(m, provider_label(&m.provider_id), cx))
                .collect()
        };
        let features: Vec<AnyElement> =
            report
                .features
                .iter()
                .map(|f| {
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2()
                        .justify_between()
                        .child(div().text_sm().child(t(feature_key(f.feature))))
                        .child(
                            h_flex()
                                .gap_3()
                                .child(div().text_xs().text_color(muted).child(t_args(
                                    "usage.calls",
                                    &[("count", &f.calls.to_string())],
                                )))
                                .child(design::mono(format_usd(f.total_usd))),
                        )
                        .into_any_element()
                })
                .collect();
        page()
            .id("page-usage")
            .child(heading)
            .child(
                div()
                    .px(px(2.))
                    .child(design::eyebrow(t("usage.cloud"), cx)),
            )
            .child(totals)
            .child(group(Some(t("usage.byModel")), models, cx))
            .when(!features.is_empty(), |d| {
                d.child(group(Some(t("usage.byFeature")), features, cx))
            })
            .child(
                h_flex().w_full().justify_between().gap_3().child(
                    div()
                        .text_xs()
                        .line_height(relative(1.5))
                        .text_color(muted)
                        .child(if estimated {
                            t("usage.estimateNote")
                        } else {
                            t("usage.reportedNote")
                        }),
                ),
            )
            .children(local_section(&report.local, cx))
            .child(
                h_flex().w_full().justify_end().child(
                    Button::new("usage-clear")
                        .ghost()
                        .pill()
                        .small()
                        .label(t("usage.clear"))
                        .disabled(report.calls == 0 && report.local.runs == 0)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_clear(window, cx)),
                        ),
                ),
            )
            .test_support()
            .into_any_element()
    }
}
