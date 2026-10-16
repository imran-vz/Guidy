//! About page: language, theme, version, what's-new toggle, data/log dirs,
//! acknowledgments. (Handy's donate/source links are branding and omitted.)

use crate::state::AppState;
use crate::ui::kit::{group, page, path_display, t, toggle, Dropdown, DropdownOption, SettingRow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::json;

pub struct AboutPage {
    state: Entity<AppState>,
    app_dir: Option<String>,
    log_dir: Option<String>,
    _subs: Vec<Subscription>,
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
                this.app_dir = Some(app.unwrap_or_else(|_| guidy_i18n::t("errors.loadDirectory")));
                this.log_dir = Some(log.unwrap_or_else(|_| guidy_i18n::t("errors.loadDirectory")));
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
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let Some(s) = st.app_settings().cloned() else {
            return page().into_any_element();
        };
        let up = |k: &str| st.settings.is_updating(k);
        let language_options = guidy_i18n::supported_languages()
            .into_iter()
            .map(|l| DropdownOption::new(l.code, format!("{} ({})", l.native_name, l.name)))
            .collect();
        let current_lang = guidy_i18n::language();
        let theme_value = serde_json::to_value(s.theme)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string));
        let muted = cx.theme().muted_foreground;

        let rows = vec![
            SettingRow::new("app-language", t("appLanguage.title"))
                .description(t("appLanguage.description"))
                .control(
                    Dropdown::new("app-language-dropdown")
                        .options(language_options)
                        .selected(Some(current_lang))
                        .menu_width(px(260.))
                        .disabled(up("app_language"))
                        .on_select(|v, _, cx| {
                            AppState::global(cx)
                                .update(cx, |s, cx| s.update_setting("app_language", json!(v), cx))
                        }),
                )
                .into_any_element(),
            SettingRow::new("theme", t("theme.title"))
                .description(t("theme.description"))
                .control(
                    Dropdown::new("theme-dropdown")
                        .options(
                            ["system", "light", "dark"]
                                .into_iter()
                                .map(|v| DropdownOption::new(v, t(&format!("theme.options.{v}"))))
                                .collect(),
                        )
                        .selected(theme_value)
                        .disabled(up("theme"))
                        .on_select(|v, _, cx| {
                            AppState::global(cx)
                                .update(cx, |s, cx| s.update_setting("theme", json!(v), cx))
                        }),
                )
                .into_any_element(),
            SettingRow::new("version", t("settings.about.version.title"))
                .description(t("settings.about.version.description"))
                .control(
                    div()
                        .id("version-value")
                        .text_sm()
                        .font_family(crate::theme::MONO_FONT)
                        .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                )
                .into_any_element(),
            SettingRow::new(
                "whats-new-updates",
                t("settings.about.whatsNewUpdates.label"),
            )
            .description(t("settings.about.whatsNewUpdates.description"))
            .control(toggle(
                "whats-new-toggle",
                s.show_whats_new_on_update,
                up("show_whats_new_on_update"),
                |v, _, cx| {
                    AppState::global(cx).update(cx, |s, cx| {
                        s.update_setting("show_whats_new_on_update", json!(v), cx)
                    })
                },
            ))
            .into_any_element(),
            SettingRow::new("app-data-dir", t("settings.about.appDataDirectory.title"))
                .description(t("settings.about.appDataDirectory.description"))
                .stacked()
                .control(path_display(
                    "open-app-data",
                    self.app_dir
                        .clone()
                        .unwrap_or_else(|| guidy_i18n::t("common.loading"))
                        .into(),
                    |_, cx| {
                        let b = AppState::global(cx).read(cx).backend.clone();
                        cx.background_spawn(async move {
                            let _ = b.open_app_data_dir();
                        })
                        .detach();
                    },
                    cx,
                ))
                .into_any_element(),
            SettingRow::new("log-dir", t("settings.debug.logDirectory.title"))
                .description(t("settings.debug.logDirectory.description"))
                .stacked()
                .control(path_display(
                    "open-log-dir",
                    self.log_dir
                        .clone()
                        .unwrap_or_else(|| guidy_i18n::t("common.loading"))
                        .into(),
                    |_, cx| {
                        let b = AppState::global(cx).read(cx).backend.clone();
                        cx.background_spawn(async move {
                            let _ = b.open_log_dir();
                        })
                        .detach();
                    },
                    cx,
                ))
                .into_any_element(),
        ];
        let ack = vec![
            SettingRow::new("ack-ggml", t("settings.about.acknowledgments.ggml.title"))
                .description(t("settings.about.acknowledgments.ggml.description"))
                .inline_description()
                .stacked()
                .control(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(t("settings.about.acknowledgments.ggml.details")),
                )
                .into_any_element(),
        ];
        page()
            .id("page-about")
            .child(group(Some(t("settings.about.title")), rows, cx))
            .child(group(
                Some(t("settings.about.acknowledgments.title")),
                ack,
                cx,
            ))
            .test_support()
            .into_any_element()
    }
}
