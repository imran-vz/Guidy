//! Screens around startup: the skeleton the window draws while the core
//! starts (the app's shell for a returning user, the setup assistant for a
//! new one, so nothing jumps when it is ready), the error shown when the
//! core can't start, and the prompt shown in the app when dictation has no
//! speech model.

use super::assistant::{Assistant, CARD_H, CARD_W};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use crate::ui::main_window::{MainWindow, Section};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Sidebar rows while the app starts.
pub fn sidebar_skeleton() -> impl IntoElement {
    v_flex()
        .id("sidebar-skeleton")
        .size_full()
        .px(dp(16.))
        .pt(dp(8.))
        .gap(dp(14.))
        .children((0..6).map(|i| {
            h_flex()
                .gap(dp(8.))
                .child(Skeleton::new().size(dp(14.)))
                .child(Skeleton::new().h(dp(9.)).w(dp(64. + (i % 3) as f32 * 18.)))
        }))
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
                .max_w(design::FORM_MAX_W)
                .gap(design::GROUP_GAP)
                .child(group(3))
                .child(group(2)),
        )
        .test_support()
}

/// The setup assistant's frame while the app starts.
pub fn assistant_skeleton(cx: &App) -> AnyElement {
    let lines = v_flex()
        .px(dp(20.))
        .pt(dp(20.))
        .pb(dp(24.))
        .gap(dp(14.))
        .child(
            h_flex()
                .gap(dp(12.))
                .child(Skeleton::new().size(dp(36.)))
                .child(
                    v_flex()
                        .gap(dp(8.))
                        .child(Skeleton::new().w(dp(180.)).h(dp(14.)))
                        .child(Skeleton::new().secondary().w(dp(320.)).h(dp(9.))),
                ),
        )
        .child(Skeleton::new().secondary().w_full().h(dp(96.)));
    div()
        .id("assistant-skeleton")
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(dp(24.))
        .pt(dp(4.))
        .pb(dp(24.))
        .child(
            design::panel(cx)
                .max_w(CARD_W)
                .h_full()
                .max_h(CARD_H)
                .child(lines.flex_1().min_h_0())
                .child(
                    div()
                        .h(dp(48.))
                        .border_t_1()
                        .border_color(design::tone(cx).hairline)
                        .bg(design::tone(cx).frame),
                ),
        )
        .test_support()
        .into_any_element()
}

/// The core couldn't start (e.g. the data folder isn't writable): say
/// so, and offer the logs and Quit instead of crashing before any window.
pub fn startup_error(error: &str, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
    Assistant::new("startup-error", t("onboarding.startupError.title"))
        .icon("triangle-alert")
        .description(t("onboarding.startupError.description"))
        .child(
            div()
                .id("startup-error-detail")
                .w_full()
                .p(dp(10.))
                .rounded(design::RADIUS_PANEL)
                .border_1()
                .border_color(tone.hairline)
                .bg(tone.ink)
                .font_family(crate::theme::MONO_FONT)
                .text_size(design::TEXT_SECONDARY)
                .line_height(dp(16.))
                .text_color(cx.theme().danger)
                .child(error.to_string())
                .test_support(),
        )
        .footer_start(
            Button::new("startup-error-logs")
                .outline()
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
        .action(
            Button::new("startup-error-quit")
                .primary()
                .label(t("onboarding.startupError.quit"))
                .on_click(|_, _, cx| cx.quit()),
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
                        .small()
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
