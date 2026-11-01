//! Storage page: the disk's capacity with Guidy's share of it, then what
//! Guidy keeps on disk, largest first. Read-only: rows open folders, nothing
//! here deletes anything. Refresh lives in the title bar.

use crate::logic::storage::{bar_fractions, format_size, ordered_rows, BarFractions};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, page, t, t_args};
use crate::ui::main_window::Section;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::storage::{BucketUsage, Category, StorageReport};

const ROW_H: Pixels = dp(32.);
const BAR_H: Pixels = dp(8.);
/// Guidy's segment never draws narrower than this while it holds anything.
const MIN_SEGMENT_W: Pixels = dp(2.);

pub struct StoragePage {
    /// The latest good report: a failed refresh leaves it on screen.
    report: Option<StorageReport>,
    error: Option<String>,
    open_error: Option<String>,
    loading: bool,
}

impl StoragePage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            report: None,
            error: None,
            open_error: None,
            loading: false,
        };
        this.reload(cx);
        this
    }

    /// Measure again; the previous report stays on screen meanwhile. A large
    /// walk is slow, so a second request while one runs is ignored.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend.storage_report() })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(report) => {
                        this.report = Some(report);
                        this.error = None;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_folder(&mut self, category: Category, cx: &mut Context<Self>) {
        self.open_error = None;
        cx.notify();
        let backend = AppState::global(cx).read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend.open_storage_category(category) })
                .await;
            this.update(cx, |this, cx| {
                this.open_error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Stable, for element ids.
fn slug(category: Category) -> &'static str {
    match category {
        Category::Models => "models",
        Category::SharedModels => "shared-models",
        Category::MeetingAudio => "meeting-audio",
        Category::DictationRecordings => "dictation-recordings",
        Category::Databases => "databases",
        Category::MeetingWorkingFiles => "meeting-working-files",
        Category::Configuration => "configuration",
        Category::Cache => "cache",
        Category::Logs => "logs",
        Category::Backups => "backups",
        Category::AppFiles => "app-files",
        Category::Other => "other",
    }
}

fn label_key(category: Category) -> &'static str {
    match category {
        Category::Models => "storage.category.models",
        Category::SharedModels => "storage.category.sharedModels",
        Category::MeetingAudio => "storage.category.meetingAudio",
        Category::DictationRecordings => "storage.category.dictationRecordings",
        Category::Databases => "storage.category.databases",
        Category::MeetingWorkingFiles => "storage.category.meetingWorkingFiles",
        Category::Configuration => "storage.category.configuration",
        Category::Cache => "storage.category.cache",
        Category::Logs => "storage.category.logs",
        Category::Backups => "storage.category.backups",
        Category::AppFiles => "storage.category.appFiles",
        Category::Other => "storage.category.other",
    }
}

/// Guidy, then everything else in use, over a track that reads as free space.
fn disk_bar(fractions: BarFractions, label: SharedString, cx: &App) -> impl IntoElement {
    let tone = design::tone(cx);
    h_flex()
        .id("storage-bar")
        .aria_label(label)
        .w_full()
        .h(BAR_H)
        .rounded(design::RADIUS_TRACK)
        .overflow_hidden()
        .bg(tone.grid)
        .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
        .when(fractions.guidy > 0., |d| {
            d.child(
                div()
                    .h_full()
                    .w(relative(fractions.guidy))
                    .min_w(MIN_SEGMENT_W)
                    .flex_none()
                    .bg(tone.accent),
            )
        })
        .child(
            div()
                .h_full()
                .w(relative(fractions.other_used))
                .min_w_0()
                .bg(tone.text_faint),
        )
        .test_support()
}

impl Render for StoragePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("storage-scroll", self.render_body(cx))
    }
}

impl crate::ui::pages::Page for StoragePage {
    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(
            Button::new("storage-refresh")
                .ghost()
                .small()
                .icon(icon("refresh-cw"))
                .tooltip(t("storage.refresh"))
                .accessibility_label(t("storage.refresh"))
                .loading(self.loading)
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                .into_any_element(),
        )
    }
}

impl StoragePage {
    fn render_summary(&self, report: Option<&StorageReport>, cx: &App) -> Div {
        let tone = design::tone(cx);
        let dash = || SharedString::from("—");
        let disk = report.and_then(|r| r.disk);
        let bytes_or_dash = |bytes: Option<u64>| -> SharedString {
            bytes
                .map(|b| SharedString::from(format_size(b)))
                .unwrap_or_else(dash)
        };
        let guidy: SharedString = match report {
            None => dash(),
            // Something couldn't be read, so this is a lower bound.
            Some(r) if r.error_count > 0 => {
                t_args("storage.atLeast", &[("size", &format_size(r.total_bytes))])
            }
            Some(r) => format_size(r.total_bytes).into(),
        };
        let capacity = bytes_or_dash(disk.map(|d| d.total_bytes));
        let occupied = bytes_or_dash(disk.map(|d| d.used_bytes()));
        let free = bytes_or_dash(disk.map(|d| d.free_bytes));
        let bar_label = t_args(
            "storage.barLabel",
            &[
                ("guidy", &guidy),
                ("occupied", &occupied),
                ("total", &capacity),
            ],
        );
        let fractions = match (report, disk) {
            (Some(r), Some(d)) => bar_fractions(r.same_device_bytes, &d),
            // Not measured yet, or the volume can't say: an empty track.
            _ => BarFractions {
                guidy: 0.,
                other_used: 0.,
                free: 1.,
            },
        };
        design::panel(cx)
            .child(
                div()
                    .px(dp(12.))
                    .py(dp(12.))
                    .border_b_1()
                    .border_color(tone.hairline)
                    .child(disk_bar(fractions, bar_label, cx)),
            )
            .child(design::stat_grid(
                vec![
                    design::Stat::new(t("storage.capacity"), capacity),
                    design::Stat::new(t("storage.occupied"), occupied),
                    design::Stat::new(t("storage.free"), free),
                    design::Stat::new(t("storage.guidy"), guidy).accent(true),
                ],
                4,
                cx,
            ))
    }

    fn render_row(&self, row: BucketUsage, last: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let category = row.category;
        let slug = slug(category);
        h_flex()
            .id(SharedString::from(format!("storage-row-{slug}")))
            .h(ROW_H)
            .px(dp(12.))
            .gap(dp(8.))
            .when(!last, |d| d.border_b_1().border_color(tone.hairline))
            .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text)
                    .child(t(label_key(category))),
            )
            .when(category == Category::SharedModels, |d| {
                d.child(design::chip(t("storage.shared"), false, cx))
            })
            .child(
                div()
                    .flex_none()
                    .font_family(crate::theme::MONO_FONT)
                    .text_size(design::TEXT_MONO)
                    .text_color(tone.text_muted)
                    .child(SharedString::from(format_size(row.bytes))),
            )
            .when(category == Category::Models, |d| {
                d.child(
                    Button::new("storage-manage-models")
                        .ghost()
                        .small()
                        .label(t("storage.manageModels"))
                        .on_click(|_, _, cx| {
                            // This window is being updated: reach it once that ends.
                            cx.defer(|cx| crate::windows::show_section(Section::Models, cx));
                        }),
                )
            })
            .child(
                Button::new(SharedString::from(format!("storage-open-{slug}")))
                    .ghost()
                    .small()
                    .icon(icon("folder-open"))
                    .tooltip(t("storage.openFolder"))
                    .accessibility_label(t_args(
                        "storage.openFolderFor",
                        &[("category", &t(label_key(category)))],
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_folder(category, cx);
                    })),
            )
            .test_support()
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let danger = cx.theme().danger;
        let error = self.error.clone().map(|e| {
            design::notice(danger)
                .id("storage-error")
                .flex()
                .items_center()
                .gap(dp(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(design::TEXT_BODY)
                        .child(t_args("storage.loadFailed", &[("error", &e)])),
                )
                .child(
                    Button::new("storage-retry")
                        .outline()
                        .small()
                        .label(t("common.tryAgain"))
                        .loading(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
                .test_support()
        });
        let open_error = self.open_error.clone().map(|e| {
            div()
                .id("storage-open-error")
                .text_size(design::TEXT_SECONDARY)
                .text_color(danger)
                .child(t_args("storage.openFailed", &[("error", &e)]))
                .test_support()
        });
        let report = self.report.clone();
        let Some(report) = report else {
            // Nothing to show yet: the failure, or the empty frame while the
            // first walk runs.
            return page()
                .id("page-storage")
                .children(error)
                .when(self.error.is_none(), |d| {
                    d.child(self.render_summary(None, cx))
                })
                .test_support()
                .into_any_element();
        };
        let partial = (report.error_count > 0).then(|| {
            let path = report.first_error_path.clone().unwrap_or_default();
            design::notice(cx.theme().warning)
                .id("storage-partial")
                .text_size(design::TEXT_BODY)
                .child(t_args(
                    "storage.partial",
                    &[("count", &report.error_count.to_string()), ("path", &path)],
                ))
                .test_support()
        });
        let rows = ordered_rows(&report.buckets);
        let count = rows.len();
        let row_elements: Vec<_> = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| self.render_row(row, i + 1 == count, cx))
            .collect();
        let caption = |text: SharedString| {
            div()
                .max_w(design::PROSE_MAX_W)
                .text_size(design::TEXT_SECONDARY)
                .line_height(dp(15.))
                .text_color(tone.text_muted)
                .child(text)
        };
        page()
            .id("page-storage")
            .children(error)
            .children(partial)
            .children(open_error)
            .child(self.render_summary(Some(&report), cx))
            .child(
                v_flex()
                    .gap(design::HEADER_GAP)
                    .child(
                        div()
                            .px(dp(2.))
                            .child(design::section_header(t("storage.whatGuidyKeeps"), cx)),
                    )
                    .when(count > 0, |d| {
                        d.child(design::panel(cx).id("storage-rows").children(row_elements))
                    })
                    .child(caption(t("storage.sizeCaption")))
                    .child(caption(t("storage.freeCaption")))
                    .child(caption(t("storage.excludedCaption"))),
            )
            .test_support()
            .into_any_element()
    }
}
