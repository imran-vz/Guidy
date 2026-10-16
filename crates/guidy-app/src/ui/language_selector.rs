//! Searchable recognition-language picker (Handy's `LanguageSelector`).

use crate::logic::languages::{
    available_languages, effective_language, filter_languages, language_label,
};
use crate::state::AppState;
use crate::ui::kit::{reset_button, t, SettingRow};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub struct LanguageSelector {
    state: Entity<AppState>,
    search: Entity<InputState>,
    open: bool,
    _subs: Vec<Subscription>,
}

impl LanguageSelector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("settings.general.language.searchPlaceholder"))
        });
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &search,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => {
                        // Enter picks the first match.
                        if let Some((code, _)) = this.filtered(cx).first().copied() {
                            this.select(code, window, cx);
                        }
                    }
                    _ => {}
                },
            ),
        ];
        Self {
            state,
            search,
            open: false,
            _subs: subs,
        }
    }

    /// (supported languages, supports detection) of the current model.
    fn capabilities(&self, cx: &App) -> (Vec<String>, bool) {
        let st = self.state.read(cx);
        st.models
            .model(&st.models.current_model)
            .map(|m| (m.supported_languages.clone(), m.supports_language_detection))
            .unwrap_or((Vec::new(), true))
    }

    fn filtered(&self, cx: &App) -> Vec<(&'static str, &'static str)> {
        let (supported, detect) = self.capabilities(cx);
        let langs = available_languages(&supported, detect);
        filter_languages(&langs, &self.search.read(cx).value())
    }

    fn select(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        let code = code.to_string();
        self.state.update(cx, |s, cx| {
            s.update_setting("selected_language", serde_json::json!(code), cx)
        });
        self.close(window, cx);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }
}

fn label_for(code: &str) -> SharedString {
    if code == "auto" {
        return t("settings.general.language.auto");
    }
    language_label(code).unwrap_or(code).to_string().into()
}

impl Render for LanguageSelector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let intent = st
            .app_settings()
            .map(|s| s.selected_language.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "auto".into());
        let updating = st.settings.is_updating("selected_language");
        let (supported, detect) = self.capabilities(cx);
        let selected = effective_language(&intent, &supported, detect);
        let filtered = self.filtered(cx);
        let entity = cx.entity().downgrade();
        let search = self.search.clone();
        let muted = cx.theme().muted_foreground;
        let hover = cx.theme().accent;
        let active = cx.theme().sidebar_accent;
        let selected_for_list = selected.clone();

        let popover = Popover::new("language-popover")
            .anchor(Anchor::TopRight)
            .open(self.open)
            .on_open_change({
                let entity = entity.clone();
                move |open, window, cx| {
                    let open = *open;
                    entity
                        .update(cx, |this, cx| {
                            if open {
                                this.open = true;
                                let h = this.search.focus_handle(cx);
                                window.focus(&h, cx);
                                cx.notify();
                            } else {
                                this.close(window, cx);
                            }
                        })
                        .ok();
                }
            })
            .trigger(
                Button::new("language-trigger")
                    .outline()
                    .small()
                    .w(px(200.))
                    .label(label_for(&selected))
                    .dropdown_caret(true)
                    .disabled(updating),
            )
            .content(move |_, _, _| {
                let entity = entity.clone();
                v_flex()
                    .w(px(240.))
                    .gap_1()
                    .child(Input::new(&search).small().cleanable(true))
                    .child(
                        v_flex()
                            .id("language-list")
                            .max_h(px(240.))
                            .overflow_y_scroll()
                            .when(filtered.is_empty(), |d| {
                                d.child(
                                    div()
                                        .p_2()
                                        .text_sm()
                                        .text_color(muted)
                                        .child(t("settings.general.language.noResults")),
                                )
                            })
                            .children(filtered.iter().map(|(code, _)| {
                                let code = *code;
                                let is_sel = code == selected_for_list;
                                let entity = entity.clone();
                                div()
                                    .id(SharedString::from(format!("lang-{code}")))
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.))
                                    .text_sm()
                                    .cursor_pointer()
                                    .when(is_sel, |d| d.bg(active))
                                    .hover(move |s| s.bg(hover))
                                    .child(label_for(code))
                                    .on_click(move |_, window, cx| {
                                        entity
                                            .update(cx, |this, cx| this.select(code, window, cx))
                                            .ok();
                                    })
                            })),
                    )
            });

        SettingRow::new("language-selector", t("settings.general.language.title"))
            .description(t("settings.general.language.description"))
            .control(h_flex().gap_1().child(popover).child(reset_button(
                "language-reset",
                updating,
                |_, cx| {
                    AppState::global(cx)
                        .update(cx, |s, cx| s.reset_setting("selected_language", cx))
                },
            )))
    }
}
