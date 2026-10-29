//! About page: a compact header (icon, name, version · engine · model,
//! Check for Updates), then the author, the What's New toggle, the data and
//! log folders, and acknowledgments. Theme and app language live in
//! General → Appearance.

use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{group, icon, page, t, t_args, toggle, SettingRow};
use crate::ui::pages::advanced::save_error;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::json;
use std::sync::{Arc, OnceLock};

const AUTHOR_SITE: &str = "https://imran.codes";
const AUTHOR_GITHUB: &str = "https://github.com/imran-vz";

pub struct AboutPage {
    state: Entity<AppState>,
    app_dir: Option<Result<String, String>>,
    log_dir: Option<Result<String, String>>,
    _subs: Vec<Subscription>,
}

/// The app icon, 48 pt. Its tile is near-black, so on a dark window it
/// gets a light hairline edge (as macOS draws around dark icons) to keep
/// its shape. The PNG's tile sits 4 pt inside the image (the icon grid).
fn app_icon_tile(cx: &App) -> Div {
    let tone = design::tone(cx);
    div()
        .flex_none()
        .relative()
        .size(dp(48.))
        .child(img(app_icon()).size_full())
        .when(tone.dark, |d| {
            d.child(
                div()
                    .absolute()
                    .top(dp(4.))
                    .left(dp(4.))
                    .size(dp(40.))
                    .rounded(dp(9.))
                    .border_1()
                    .border_color(gpui_kit::white().opacity(0.22)),
            )
        })
}

/// The app icon (decoded once).
fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../../../assets/icons/app/guidy.png").to_vec(),
        ))
    })
    .clone()
}

impl AboutPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        let backend = state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let (app, log) = cx
                .background_spawn(async move {
                    (backend.get_app_dir_path(), backend.get_log_dir_path())
                })
                .await;
            this.update(cx, |this, cx| {
                this.app_dir = Some(app);
                this.log_dir = Some(log);
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            state,
            app_dir: None,
            log_dir: None,
            _subs: subs,
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let st = self.state.read(cx);
        let model = st
            .models
            .models
            .iter()
            .find(|m| m.id == st.models.current_model)
            .map(crate::ui::kit::model_name);
        let mut facts = vec![
            t_args(
                "settings.about.versionLabel",
                &[("version", env!("CARGO_PKG_VERSION"))],
            )
            .to_string(),
            "transcribe.cpp".to_string(),
        ];
        facts.extend(model);
        // Updates are off in builds without an update endpoint.
        let can_check = st.update_checks_locked == Some(false);
        let state = self.state.clone();
        h_flex()
            .id("about-header")
            .w_full()
            .gap(dp(12.))
            .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
            .child(app_icon_tile(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(2.))
                    .child(
                        div()
                            .text_size(design::TEXT_HEADLINE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Guidy"),
                    )
                    .child(
                        div()
                            .id("version-value")
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .truncate()
                            .child(facts.join(" · "))
                            .test_support(),
                    ),
            )
            .child(
                Button::new("check-for-updates")
                    .outline()
                    .small()
                    .label(t("settings.about.checkForUpdates"))
                    .disabled(!can_check)
                    .on_click(move |_, _, cx| {
                        state.update(cx, |_, cx| cx.emit(AppEvent::CheckForUpdates));
                    }),
            )
            .test_support()
    }
}

/// A folder row: the path (or why it can't be shown) with Show in Finder.
fn folder_row(
    id: &'static str,
    button_id: &'static str,
    title: SharedString,
    dir: &Option<Result<String, String>>,
    open: fn(&dyn crate::backend::Backend) -> Result<(), String>,
) -> AnyElement {
    let (path, error): (SharedString, bool) = match dir {
        None => (t("common.loading"), false),
        Some(Ok(p)) => (p.clone().into(), false),
        Some(Err(e)) => (t_args("errors.loadDirectory", &[("error", e)]), true),
    };
    SettingRow::new(id, title)
        .description(path.clone())
        .control(
            Button::new(button_id)
                .outline()
                .small()
                .label(t("settings.about.showInFinder"))
                .disabled(!matches!(dir, Some(Ok(_))))
                .on_click(move |_, _, cx| {
                    let b = AppState::global(cx).read(cx).backend.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = open(&*b) {
                            log::error!("Failed to open folder: {e}");
                        }
                    })
                    .detach();
                }),
        )
        .when(error, |r| r.error(Some(path)))
        .into_any_element()
}

impl Render for AboutPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::kit::scroll_page("about-scroll", self.render_body(window, cx))
    }
}

impl crate::ui::pages::Page for AboutPage {}

impl AboutPage {
    fn render_body(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let st = self.state.read(cx);
        let Some(s) = st.app_settings().cloned() else {
            return page().id("page-about").child(header).into_any_element();
        };
        let tone = design::tone(cx);
        let rows = vec![
            SettingRow::new("author", t("settings.about.author.title"))
                .description(t("settings.about.author.description"))
                .control(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("author-site")
                                .ghost()
                                .small()
                                .icon(icon("globe"))
                                .label("imran.codes")
                                .on_click(|_, _, cx| cx.open_url(AUTHOR_SITE)),
                        )
                        .child(
                            Button::new("author-github")
                                .ghost()
                                .small()
                                .icon(icon("github"))
                                .label("GitHub")
                                .on_click(|_, _, cx| cx.open_url(AUTHOR_GITHUB)),
                        ),
                )
                .into_any_element(),
            SettingRow::new(
                "whats-new-updates",
                t("settings.about.whatsNewUpdates.label"),
            )
            .description(t("settings.about.whatsNewUpdates.description"))
            .error(save_error(st, "show_whats_new_on_update"))
            .control(toggle(
                "whats-new-toggle",
                s.show_whats_new_on_update,
                false,
                |v, _, cx| {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.update_setting("show_whats_new_on_update", json!(v), cx)
                    })
                },
            ))
            .into_any_element(),
            folder_row(
                "app-data-dir",
                "open-app-data",
                t("settings.about.appDataDirectory.title"),
                &self.app_dir,
                |b| b.open_app_data_dir(),
            ),
            folder_row(
                "log-dir",
                "open-log-dir",
                t("settings.debug.logDirectory.title"),
                &self.log_dir,
                |b| b.open_log_dir(),
            ),
        ];
        let ack = vec![
            SettingRow::new("ack-ggml", t("settings.about.acknowledgments.ggml.title"))
                .description(t("settings.about.acknowledgments.ggml.description"))
                .stacked()
                .control(
                    div()
                        .max_w(design::PROSE_MAX_W)
                        .text_size(design::TEXT_SECONDARY)
                        .line_height(dp(15.))
                        .text_color(tone.text_muted)
                        .child(t("settings.about.acknowledgments.ggml.details")),
                )
                .into_any_element(),
        ];
        page()
            .id("page-about")
            .child(header)
            .child(group(None, rows, cx))
            .child(group(
                Some(t("settings.about.acknowledgments.title")),
                ack,
                cx,
            ))
            .test_support()
            .into_any_element()
    }
}
