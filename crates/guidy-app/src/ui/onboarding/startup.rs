//! Screens around startup: the skeleton the window draws while the core
//! starts (the app's shell for a returning user, the setup assistant for a
//! new one, so nothing jumps when it is ready), the error shown when the
//! core can't start, and the prompt shown in the app when dictation has no
//! speech model.

use super::assistant::{column_page, footer_bar, Assistant, COLUMN_W, COMPACT_BELOW_H};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use crate::ui::main_window::{MainWindow, Section};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// The sidebar while the app starts: its real rows (they are static), in
/// the same clusters and geometry as `MainWindow::render_sidebar`, with
/// Meetings' two views indented under it and nothing selected yet. Only
/// the content beside it is a skeleton, so the sidebar doesn't jump when
/// the app is ready.
pub fn sidebar_skeleton() -> impl IntoElement {
    SidebarSkeleton
}

#[derive(IntoElement)]
struct SidebarSkeleton;

/// A section's glyph and cluster, exactly as the real sidebar draws them.
fn section_look(id: &str) -> (&'static str, u8) {
    Section::from_id(id)
        .map(|s| (s.icon(), s.group()))
        .unwrap_or(("info", 3))
}

impl RenderOnce for SidebarSkeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::ui::pages::meetings::MeetingsTab;
        let tone = design::tone(cx);
        let compact = crate::ui::main_window::sidebar_width(window) < design::SIDEBAR_W;
        // (label, glyph, cluster, indented), in visual order.
        let mut items: Vec<(SharedString, &'static str, u8, bool)> = Vec::new();
        // The real sidebar's order: People under Meetings; Meeting settings
        // a top-level row closing the settings cluster.
        let sections: Vec<Section> = crate::ui::visible_sections(cx)
            .into_iter()
            .filter_map(Section::from_id)
            .collect();
        let meetings = sections.contains(&Section::Meetings);
        let last_setting = sections
            .iter()
            .rposition(|s| s.group() == Section::Models.group());
        for (i, section) in sections.iter().copied().enumerate() {
            let (glyph, cluster) = section_look(section.id());
            items.push((t(section.label_key()), glyph, cluster, false));
            if section == Section::Meetings {
                items.push((
                    t(MeetingsTab::People.label_key()),
                    "users-round",
                    cluster,
                    true,
                ));
            }
            if meetings && Some(i) == last_setting {
                items.push((
                    t(MeetingsTab::Settings.label_key()),
                    "settings-2",
                    cluster,
                    false,
                ));
            }
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last = None;
        for (label, glyph, cluster, indented) in items {
            // The real sidebar's metrics: 8 pt cluster gaps, no row gaps,
            // 26 pt rows with a reserved 2 pt focus border, so nothing moves
            // when the app is ready.
            if last.is_some_and(|c| c != cluster) {
                rows.push(div().flex_none().h(dp(8.)).into_any_element());
            }
            last = Some(cluster);
            rows.push(
                h_flex()
                    .flex_none()
                    .gap(dp(8.))
                    // The real sidebar's insets (narrow: a shallower child
                    // indent and trailing inset, so the labels fit).
                    .pr(if compact { dp(4.) } else { dp(8.) })
                    .pl(match (indented, compact) {
                        (true, false) => dp(34.),
                        (true, true) => dp(20.),
                        (false, _) => dp(8.),
                    })
                    .h(design::SIDEBAR_ITEM_H)
                    .border_2()
                    .border_color(transparent_black())
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text)
                    .child(
                        div()
                            .flex_none()
                            .size(dp(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph).size(dp(16.)).text_color(tone.text_muted)),
                    )
                    .child(div().truncate().child(label))
                    .into_any_element(),
            );
        }
        v_flex()
            .id("sidebar-skeleton")
            .size_full()
            .px(if compact { dp(8.) } else { dp(10.) })
            .pt(dp(4.))
            .pb(dp(10.))
            .gap(px(0.))
            .children(rows)
            .test_support()
    }
}

/// "Starting Guidy…": what the skeletons are waiting for, in words. Static
/// (no spinner or shimmer).
fn starting_label(cx: &App) -> impl IntoElement {
    div()
        .id("startup-starting")
        .flex_none()
        .text_size(design::TEXT_SECONDARY)
        .line_height(design::LINE_SECONDARY)
        .text_color(design::tone(cx).text_muted)
        .child(t("onboarding.starting"))
        .test_support()
}

/// A settings page's rows while the app starts.
pub fn page_skeleton(cx: &App) -> impl IntoElement {
    let hairline = design::tone(cx).hairline;
    let group = |rows: usize| {
        design::panel(cx).children((0..rows).map(move |i| {
            h_flex()
                .h(dp(44.))
                .px(design::SECTION_X)
                .gap(dp(12.))
                .when(i + 1 < rows, |d| d.border_b_1().border_color(hairline))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(dp(6.))
                        .child(Skeleton::new().w(dp(150.)).h(dp(10.)))
                        .child(Skeleton::new().secondary().w(dp(260.)).h(dp(8.))),
                )
                .child(Skeleton::new().w(dp(72.)).h(dp(18.)))
        }))
    };
    v_flex()
        .id("page-skeleton")
        .size_full()
        .px(design::PAGE_X)
        .pt(design::PAGE_TOP)
        .gap(design::GROUP_GAP)
        .child(
            v_flex()
                .w_full()
                .max_w(design::FORM_MAX_W)
                .mx_auto()
                .gap(design::GROUP_GAP)
                .child(starting_label(cx))
                .child(group(3))
                .child(group(2)),
        )
        .test_support()
}

/// The setup assistant's frame while the app starts.
pub fn assistant_skeleton(_cx: &App) -> AnyElement {
    AssistantSkeleton.into_any_element()
}

/// The skeleton as an element: it needs the window height to match the
/// assistant's compact margins.
#[derive(IntoElement)]
struct AssistantSkeleton;

impl RenderOnce for AssistantSkeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        assistant_skeleton_frame(window, cx)
    }
}

fn assistant_skeleton_frame(window: &Window, cx: &App) -> impl IntoElement {
    // The assistant frame's geometry (assistant.rs): same column, header
    // and pinned footer, so nothing moves when the first step appears.
    let compact = window.viewport_size().height < COMPACT_BELOW_H;
    let column = v_flex()
        .w_full()
        .max_w(COLUMN_W)
        // The header's bottom gap (tighter in a short window).
        .gap(if compact { dp(8.) } else { dp(20.) })
        .child(
            h_flex()
                .gap(dp(14.))
                .child(Skeleton::new().size(dp(40.)))
                .child(
                    v_flex()
                        .gap(dp(10.))
                        .child(Skeleton::new().w(dp(180.)).h(dp(16.)))
                        .child(Skeleton::new().secondary().w(dp(320.)).h(dp(10.))),
                ),
        )
        .child(Skeleton::new().secondary().w_full().h(dp(96.)));
    v_flex()
        .id("assistant-skeleton")
        .size_full()
        .bg(design::tone(cx).ink)
        .child(column_page("assistant-skeleton-page", compact, column))
        .child(footer_bar(
            h_flex().w_full().max_w(COLUMN_W).child(starting_label(cx)),
            cx,
        ))
        .test_support()
}

/// The plain words for a startup failure: a headline and one sentence.
/// The core reports which part failed ("Failed to initialize history
/// manager: …"); a failure we recognise is named, anything else keeps the
/// general wording. Presentation only: the raw error is shown below.
fn startup_error_copy(error: &str) -> (SharedString, SharedString) {
    let lower = error.to_ascii_lowercase();
    if lower.starts_with("failed to initialize history manager") {
        let description = if lower.contains("permission denied") || lower.contains("os error 13") {
            t("onboarding.startupError.history.permission")
        } else {
            t("onboarding.startupError.history.description")
        };
        return (t("onboarding.startupError.history.title"), description);
    }
    (
        t("onboarding.startupError.title"),
        t("onboarding.startupError.description"),
    )
}

/// The core couldn't start (e.g. the data folder isn't writable): a plain
/// headline and one sentence that name the cause, then the exact error in
/// a secondary, selectable details box; Show Logs is the visual primary,
/// Quit secondary (instead of crashing before any window).
pub fn startup_error(error: &str, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
    // A fenced block the error can't close early: longer than any run of
    // tildes in it.
    let longest = error.split(|c| c != '~').map(str::len).max().unwrap_or(0);
    let fence = "~".repeat(longest.max(2) + 1);
    let details = format!("{fence}\n{}\n{fence}", error.trim_end());
    let (title, description) = startup_error_copy(error);
    Assistant::new("startup-error", title)
        .icon("triangle-alert")
        .description(description)
        .child(
            v_flex()
                .w_full()
                .gap(design::HEADER_GAP)
                .child(div().px(dp(2.)).child(design::section_header(
                    t("onboarding.startupError.details"),
                    cx,
                )))
                .child(
                    div()
                        .id("startup-error-detail")
                        .w_full()
                        // The code block is the box (selectable, monospace).
                        .text_size(design::TEXT_SECONDARY)
                        .line_height(design::LINE_SECONDARY)
                        .text_color(tone.text_muted)
                        .child(
                            TextView::markdown("startup-error-detail-text", details)
                                .selectable(true)
                                .w_full(),
                        )
                        .test_support(),
                ),
        )
        .action(
            Button::new("startup-error-quit")
                .outline()
                .label(t("onboarding.startupError.quit"))
                .on_click(|_, _, cx| cx.quit()),
        )
        .action(
            Button::new("startup-error-logs")
                .primary()
                .label(t("onboarding.startupError.showLogs"))
                .on_click(|_, _, cx| {
                    let backend = AppState::global(cx).read(cx).backend.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = backend.open_log_dir() {
                            log::warn!("Failed to open the logs folder: {e}");
                        }
                    })
                    .detach();
                }),
        )
        .into_any_element()
}

/// Dictation has no speech model (deleted, or setup was skipped): a banner
/// above the page pointing to the Models page. `None` when not needed:
/// a model is selected or on its way, or the user only uses meeting notes.
pub fn no_model_banner(
    main: WeakEntity<MainWindow>,
    section: Section,
    cx: &App,
) -> Option<AnyElement> {
    let st = AppState::global(cx).read(cx);
    let settings = st.app_settings()?;
    let downloading = st
        .models
        .downloads
        .iter()
        .any(|d| d.kind == guidy_core::downloads::DownloadKind::Stt && d.state.is_active());
    let needed = st.settings_loaded
        && st.models.initialized
        && settings.selected_model.is_empty()
        && st.pending_model.is_none()
        && st.select_on_download.is_empty()
        && !downloading
        && !settings.meetings.enabled;
    if !needed {
        return None;
    }
    let tone = design::tone(cx);
    Some(
        design::notice(tone.accent)
            .id("no-model-banner")
            .flex()
            .flex_row()
            .items_center()
            .gap(dp(10.))
            .mx(design::PAGE_X)
            .mt(dp(12.))
            .w_auto()
            .flex_none()
            .child(icon("mic").small().text_color(tone.accent_text))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(design::TEXT_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("onboarding.noModel.title")),
                    )
                    .child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(t("onboarding.noModel.description")),
                    ),
            )
            .when(section != Section::Models, |d| {
                d.child(
                    Button::new("no-model-choose")
                        .outline()
                        .label(t("onboarding.noModel.action"))
                        .on_click(move |_, _, cx| {
                            main.update(cx, |m, cx| m.select_section(Section::Models, cx))
                                .ok();
                        }),
                )
            })
            .test_support()
            .into_any_element(),
    )
}
