//! Usage page: what cloud LLM calls have cost and what local models did on
//! this Mac. One row of totals, then two tables (cloud spend by model, local
//! work by model). Refresh and Clear Usage… live in the title bar.

use crate::logic::usage::{
    format_duration, format_speedup, format_tokens, format_tokens_per_second, format_usd,
    format_wait,
};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{confirm, icon, page, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::llm::usage::{
    LocalKind, LocalModelUsage, ModelSpend, UsageFeature, UsageReport,
    CLOUD_TRANSCRIPTION_USD_PER_MIN,
};

/// Below this content width the tables drop their secondary columns.
const WIDE_MIN_W: Pixels = dp(640.);
const ROW_H: Pixels = dp(26.);

pub struct UsagePage {
    report: Option<Result<UsageReport, String>>,
    loading: bool,
    clearing: bool,
    clear_error: Option<String>,
}

impl UsagePage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            report: None,
            loading: false,
            clearing: false,
            clear_error: None,
        };
        this.reload(cx);
        this
    }

    /// Load the report; the previous one stays on screen meanwhile.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_spawn(async move { backend.llm_usage_report() })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                this.report = Some(report);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn confirm_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        confirm(
            window,
            cx,
            t("usage.clearTitle"),
            t("usage.clearConfirm"),
            t("usage.clearButton"),
            true,
            move |_, cx| {
                this.update(cx, |this, cx| this.clear(cx)).ok();
            },
            |_, _| {},
        );
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.clearing = true;
        self.clear_error = None;
        cx.notify();
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend.clear_llm_usage() })
                .await;
            this.update(cx, |this, cx| {
                this.clearing = false;
                if let Err(e) = result {
                    this.clear_error = Some(e);
                }
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    #[cfg(test)]
    pub fn clear_error(&self) -> Option<&str> {
        self.clear_error.as_deref()
    }
}

fn feature_key(f: UsageFeature) -> &'static str {
    match f {
        UsageFeature::Dictation => "usage.feature.dictation",
        UsageFeature::MeetingTranscription => "usage.feature.meetingTranscription",
        UsageFeature::MeetingSummary => "usage.feature.meetingSummary",
        UsageFeature::MeetingTitle => "usage.feature.meetingTitle",
        UsageFeature::Other => "usage.feature.other",
    }
}

fn kind_key(k: LocalKind) -> &'static str {
    match k {
        LocalKind::Transcription => "usage.kind.transcription",
        LocalKind::Llm => "usage.kind.llm",
    }
}

/// One table column: header, width (`None` = takes the rest), alignment.
#[derive(Clone, Copy)]
struct Column {
    title: &'static str,
    width: Option<f32>,
    numeric: bool,
}

const fn col(title: &'static str, width: Option<f32>, numeric: bool) -> Column {
    Column {
        title,
        width,
        numeric,
    }
}

fn cell(c: Column, text: SharedString, cx: &App) -> Div {
    let d = div()
        .min_w_0()
        .truncate()
        .px(dp(8.))
        .when_some(c.width, |d, w| d.w(dp(w)).flex_none())
        .when(c.width.is_none(), |d| d.flex_1());
    if c.numeric {
        d.text_right()
            .font_family(crate::theme::MONO_FONT)
            .text_size(design::TEXT_MONO)
            .child(text)
    } else {
        d.text_size(design::TEXT_BODY)
            .text_color(design::tone(cx).text)
            .child(text)
    }
}

/// A bordered table: a header row and `rows` of cells, hairlines between.
fn table(
    id: &'static str,
    columns: &[Column],
    rows: Vec<(SharedString, Vec<SharedString>)>,
    empty: Option<(&'static str, SharedString)>,
    cx: &App,
) -> Stateful<Div> {
    let tone = design::tone(cx);
    let rtl = crate::ui::kit::rtl();
    let header = h_flex()
        .h(ROW_H)
        .px(dp(4.))
        .border_b_1()
        .border_color(tone.hairline)
        .bg(tone.raised.opacity(0.5))
        .when(rtl, |d| d.flex_row_reverse())
        .children(columns.iter().map(|c| {
            cell(*c, t(c.title), cx)
                .font_family(crate::theme::UI_FONT)
                .text_size(design::TEXT_SECONDARY)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tone.text_muted)
        }));
    let count = rows.len();
    design::panel(cx)
        .id(id)
        .child(header)
        .when_some(empty.filter(|_| count == 0), |d, (empty_id, text)| {
            d.child(
                div()
                    .id(empty_id)
                    .px(dp(12.))
                    .py(dp(10.))
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text_muted)
                    .child(text)
                    .test_support(),
            )
        })
        .children(rows.into_iter().enumerate().map(|(i, (row_id, cells))| {
            h_flex()
                .id(row_id)
                .h(ROW_H)
                .px(dp(4.))
                .when(i + 1 < count, |d| {
                    d.border_b_1().border_color(tone.hairline)
                })
                .when(rtl, |d| d.flex_row_reverse())
                .children(
                    columns
                        .iter()
                        .zip(cells)
                        .map(|(c, text)| cell(*c, text, cx)),
                )
                .test_support()
        }))
}

/// Loading: the same frame with placeholder rows.
fn skeleton_table(id: &'static str, columns: &[Column], cx: &App) -> Stateful<Div> {
    let tone = design::tone(cx);
    let bar = move |w: f32| {
        div()
            .h(dp(8.))
            .w(relative(w))
            .rounded(design::RADIUS_CHIP)
            .bg(tone.raised)
    };
    design::panel(cx)
        .id(id)
        .child(
            h_flex()
                .h(ROW_H)
                .px(dp(4.))
                .border_b_1()
                .border_color(tone.hairline)
                .bg(tone.raised.opacity(0.5))
                .children(columns.iter().map(|c| {
                    cell(*c, t(c.title), cx)
                        .font_family(crate::theme::UI_FONT)
                        .text_size(design::TEXT_SECONDARY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tone.text_muted)
                })),
        )
        .children((0..3).map(move |r| {
            h_flex()
                .h(ROW_H)
                .px(dp(12.))
                .gap(dp(16.))
                .when(r < 2, |d| d.border_b_1().border_color(tone.hairline))
                .child(div().flex_1().child(bar(0.5)))
                .child(div().w(dp(60.)).child(bar(1.)))
                .child(div().w(dp(60.)).child(bar(1.)))
        }))
}

fn cloud_columns(wide: bool) -> Vec<Column> {
    let mut c = vec![col("usage.col.model", None, false)];
    if wide {
        c.push(col("usage.col.provider", Some(110.), false));
    }
    c.push(col("usage.col.calls", Some(52.), true));
    c.push(col("usage.col.avg", Some(72.), true));
    if wide {
        c.push(col("usage.col.max", Some(72.), true));
    }
    c.push(col(
        "usage.col.tokens",
        Some(if wide { 120. } else { 112. }),
        true,
    ));
    c.push(col("usage.col.cost", Some(76.), true));
    c
}

fn local_columns(wide: bool) -> Vec<Column> {
    let mut c = vec![col("usage.col.model", None, false)];
    if wide {
        c.push(col("usage.col.kind", Some(110.), false));
    }
    c.push(col("usage.col.runs", Some(52.), true));
    c.push(col("usage.col.work", Some(96.), true));
    c.push(col("usage.col.speed", Some(84.), true));
    if wide {
        c.push(col("usage.col.load", Some(72.), true));
    }
    c
}

fn cloud_row(
    m: &ModelSpend,
    provider: SharedString,
    wide: bool,
) -> (SharedString, Vec<SharedString>) {
    let dash = || SharedString::from("—");
    let priced = m.calls - m.unpriced_calls;
    let total: SharedString = if priced == 0 {
        dash()
    } else if m.estimated_calls > 0 || m.unpriced_calls > 0 {
        format!("~{}", format_usd(m.total_usd)).into()
    } else {
        format_usd(m.total_usd).into()
    };
    let mut cells: Vec<SharedString> = vec![m.model.clone().into()];
    if wide {
        cells.push(provider);
    }
    cells.push(m.calls.to_string().into());
    cells.push(m.avg_usd.map(|v| format_usd(v).into()).unwrap_or_else(dash));
    if wide {
        cells.push(m.max_usd.map(|v| format_usd(v).into()).unwrap_or_else(dash));
    }
    cells.push(
        format!(
            "{} / {}",
            format_tokens(m.input_tokens),
            format_tokens(m.output_tokens)
        )
        .into(),
    );
    cells.push(total);
    (
        format!("usage-model-{}-{}", m.provider_id, m.model).into(),
        cells,
    )
}

fn local_row(m: &LocalModelUsage, wide: bool) -> (SharedString, Vec<SharedString>) {
    let dash = || SharedString::from("—");
    let (work, speed): (SharedString, SharedString) = match m.kind {
        LocalKind::Transcription => (
            format_duration(m.audio_ms).into(),
            m.realtime_factor()
                .map(|x| format_speedup(x).into())
                .unwrap_or_else(dash),
        ),
        LocalKind::Llm => (
            format!(
                "{} / {}",
                format_tokens(m.input_tokens),
                format_tokens(m.output_tokens)
            )
            .into(),
            m.generation_tokens_per_second()
                .map(|x| format_tokens_per_second(x).into())
                .unwrap_or_else(dash),
        ),
    };
    let mut cells: Vec<SharedString> = vec![m.model.clone().into()];
    if wide {
        cells.push(t(kind_key(m.kind)));
    }
    cells.push(m.runs.to_string().into());
    cells.push(work);
    cells.push(speed);
    if wide {
        cells.push(
            m.avg_load_ms()
                .map(|ms| format_wait(ms).into())
                .unwrap_or_else(dash),
        );
    }
    (
        format!("usage-local-{}-{}", m.kind.as_str(), m.model).into(),
        cells,
    )
}

impl Render for UsagePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("usage-scroll", self.render_body(window, cx))
    }
}

impl crate::ui::pages::Page for UsagePage {
    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Only a loaded, non-empty report can be cleared.
        let empty = !matches!(&self.report, Some(Ok(r)) if r.calls > 0 || r.local.runs > 0);
        let danger = cx.theme().danger;
        Some(
            h_flex()
                .gap(dp(4.))
                .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
                .child(
                    Button::new("usage-refresh")
                        .ghost()
                        .small()
                        .icon(icon("refresh-cw"))
                        .tooltip(t("usage.refresh"))
                        .loading(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
                .child(
                    Button::new("usage-clear")
                        .ghost()
                        .small()
                        .label(t("usage.clearEllipsis"))
                        .text_color(danger)
                        .loading(self.clearing)
                        .disabled(empty || self.clearing)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_clear(window, cx)),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl UsagePage {
    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let wide = window.viewport_size().width - design::SIDEBAR_W >= WIDE_MIN_W;
        let note = |text: SharedString| {
            div()
                .max_w(design::PROSE_MAX_W)
                .text_size(design::TEXT_SECONDARY)
                .line_height(dp(15.))
                .text_color(tone.text_muted)
                .child(text)
        };
        let header =
            |title: SharedString| div().px(dp(2.)).child(design::section_header(title, cx));
        let clear_error = self.clear_error.clone().map(|e| {
            div()
                .id("usage-clear-error")
                .text_size(design::TEXT_SECONDARY)
                .text_color(cx.theme().danger)
                .child(t_args("usage.clearFailed", &[("error", &e)]))
                .test_support()
        });
        let report = match &self.report {
            None => {
                return page()
                    .id("page-usage")
                    .max_w_full()
                    .child(design::panel(cx).child(design::stat_grid(
                        vec![
                            design::Stat::new(t("usage.total"), "—"),
                            design::Stat::new(t("usage.last30Days"), "—"),
                            design::Stat::new(t("usage.callsTitle"), "—"),
                            design::Stat::new(t("usage.localAudio"), "—"),
                        ],
                        4,
                        cx,
                    )))
                    .child(header(t("usage.cloudByModel")))
                    .child(skeleton_table("usage-loading", &cloud_columns(wide), cx))
                    .test_support()
                    .into_any_element();
            }
            Some(Err(e)) => {
                return page()
                    .id("page-usage")
                    .child(
                        design::notice(cx.theme().danger)
                            .id("usage-error")
                            .flex()
                            .items_center()
                            .gap(dp(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(design::TEXT_BODY)
                                    .child(t_args("usage.loadFailed", &[("error", e)])),
                            )
                            .child(
                                Button::new("usage-retry")
                                    .outline()
                                    .small()
                                    .label(t("common.tryAgain"))
                                    .loading(self.loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                            )
                            .test_support(),
                    )
                    .test_support()
                    .into_any_element();
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
                design::Stat::new(
                    t("usage.localAudio"),
                    format_duration(report.local.audio_ms),
                ),
            ],
            4,
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
        let cloud_rows = report
            .models
            .iter()
            .map(|m| cloud_row(m, provider_label(&m.provider_id), wide))
            .collect();
        let local_rows = report
            .local
            .models
            .iter()
            .map(|m| local_row(m, wide))
            .collect();
        // Spend per feature, in one line under the cloud table.
        let by_feature = (!report.features.is_empty()).then(|| {
            report
                .features
                .iter()
                .map(|f| format!("{} {}", t(feature_key(f.feature)), format_usd(f.total_usd)))
                .collect::<Vec<_>>()
                .join(" · ")
        });
        let local_note = (!report.local.models.is_empty()).then(|| {
            t_args(
                "usage.localSummary",
                &[
                    ("words", &format_tokens(report.local.words)),
                    ("tokens", &format_tokens(report.local.tokens)),
                    ("cost", &format_usd(report.local.cloud_transcription_usd())),
                    ("price", &CLOUD_TRANSCRIPTION_USD_PER_MIN.to_string()),
                ],
            )
        });
        page()
            .id("page-usage")
            .max_w_full()
            .children(clear_error)
            .child(totals)
            .child(
                v_flex()
                    .gap(design::HEADER_GAP)
                    .child(header(t("usage.cloudByModel")))
                    .child(table(
                        "usage-cloud",
                        &cloud_columns(wide),
                        cloud_rows,
                        Some(("usage-empty", t("usage.empty"))),
                        cx,
                    ))
                    .when_some(by_feature, |d, f| {
                        d.child(note(t_args("usage.byFeatureLine", &[("list", &f)])))
                    })
                    .child(note(if estimated {
                        t("usage.estimateNote")
                    } else {
                        t("usage.reportedNote")
                    })),
            )
            .child(
                v_flex()
                    .gap(design::HEADER_GAP)
                    .child(header(t("usage.local")))
                    .child(table(
                        "usage-local",
                        &local_columns(wide),
                        local_rows,
                        Some(("usage-local-empty", t("usage.localEmpty"))),
                        cx,
                    ))
                    .when_some(local_note, |d, n| d.child(note(n))),
            )
            .test_support()
            .into_any_element()
    }
}
