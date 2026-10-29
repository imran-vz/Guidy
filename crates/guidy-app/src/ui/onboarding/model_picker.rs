//! "Choose Another…": every speech model in one compact list, filtered by
//! language (the dictation language first) and a search. Used by the setup
//! step and the retired-model migration; it only picks, the caller decides
//! what happens (select now, or download in the background).

use super::assistant::{continue_button, row, rows, trailing_text, Assistant};
use crate::logic::languages::{capability_languages, language_label, unique_capability_languages};
use crate::logic::onboarding::picker_models;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{model_description, model_name, t, t_args, Dropdown, DropdownOption};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{h_flex, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::model::ModelInfo;
use std::rc::Rc;

const ALL_LANGUAGES: &str = "all";

type OnDone = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

pub struct ModelPicker {
    state: Entity<AppState>,
    search: Entity<InputState>,
    /// Language filter (`None`: all languages).
    language: Option<String>,
    pub selected: Option<String>,
    on_done: OnDone,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl ModelPicker {
    /// `on_done(Some(id))` when a model is chosen, `None` on Cancel.
    pub fn new(
        selected: Option<String>,
        language: Option<String>,
        on_done: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        use crate::ui::kit::LocalizedPlaceholder;
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "onboarding.picker.search",
                window,
                cx,
            )
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
        ];
        // A language no model speaks would show an empty list.
        let language = language.filter(|l| {
            let st = state.read(cx);
            !picker_models(&st.models.models, Some(l), "").is_empty()
        });
        Self {
            state,
            search,
            language,
            selected,
            on_done: Rc::new(on_done),
            focus,
            _subs: subs,
        }
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        cx.notify();
    }

    fn done(&mut self, chosen: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let on_done = self.on_done.clone();
        cx.defer_in(window, move |_, window, cx| on_done(chosen, window, cx));
    }

    fn model_row(&self, m: &ModelInfo, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_deref() == Some(m.id.as_str());
        let langs = unique_capability_languages(&m.supported_languages);
        let languages = match langs.as_slice() {
            [one] => t_args(
                "modelSelector.capabilities.languageOnly",
                &[("language", language_label(one).unwrap_or(one))],
            ),
            _ => t_args(
                "modelSelector.capabilities.languageCount",
                &[("total", &langs.len().to_string())],
            ),
        };
        let mut description = model_description(m);
        if !description.is_empty() {
            description.push_str(" · ");
        }
        description.push_str(&languages);
        let id = m.id.clone();
        let radio = Radio::new(SharedString::from(format!("picker-radio-{}", m.id)))
            .checked(selected)
            .on_click(cx.listener({
                let id = id.clone();
                move |this, _: &bool, _, cx| {
                    cx.stop_propagation();
                    this.select(&id, cx);
                }
            }));
        let title = h_flex()
            .gap(dp(6.))
            .child(model_name(m))
            .when(m.is_recommended, |d| {
                d.child(design::chip(t("onboarding.recommended"), true, cx))
            })
            .when(m.is_downloaded, |d| {
                d.child(design::chip(t("onboarding.picker.downloaded"), false, cx))
            });
        let size = if m.is_downloaded {
            String::new()
        } else {
            guidy_core::downloads::format_bytes(m.size_mb * 1024 * 1024)
        };
        let tone = design::tone(cx);
        row(
            SharedString::from(format!("picker-model-{}", m.id)),
            Some(radio.into_any_element()),
            title,
            Some(description.into()),
            Some(trailing_text(size, cx)),
            cx,
        )
        .when(selected, |d| d.bg(tone.selection))
        .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
        .test_support()
        .into_any_element()
    }
}

impl Render for ModelPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let models = st.models.models.clone();
        let query = self.search.read(cx).value().to_string();
        let list: Vec<ModelInfo> = picker_models(&models, self.language.as_deref(), &query)
            .into_iter()
            .cloned()
            .collect();
        let tone = design::tone(cx);

        let mut options = vec![DropdownOption::new(
            ALL_LANGUAGES,
            t("onboarding.picker.allLanguages"),
        )];
        options
            .extend(capability_languages().map(|(code, label)| DropdownOption::new(code, label)));
        let weak = cx.entity().downgrade();
        let filter = h_flex()
            .w_full()
            .gap(dp(8.))
            .child(
                div()
                    .id("picker-search")
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.search).small().cleanable(true))
                    .test_support(),
            )
            .child(
                Dropdown::new("picker-language")
                    .options(options)
                    .selected(Some(
                        self.language
                            .clone()
                            .unwrap_or_else(|| ALL_LANGUAGES.to_string()),
                    ))
                    .min_width(dp(150.))
                    .menu_width(dp(200.))
                    .on_select(move |v, _, cx| {
                        let v = v.to_string();
                        weak.update(cx, |this, cx| {
                            this.language = (v != ALL_LANGUAGES).then_some(v);
                            cx.notify();
                        })
                        .ok();
                    }),
            );

        let body: AnyElement = if list.is_empty() {
            div()
                .id("picker-empty")
                .py(dp(24.))
                .w_full()
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(t("onboarding.picker.empty"))
                .into_any_element()
        } else {
            let items = list.iter().map(|m| self.model_row(m, cx)).collect();
            rows(None, items, cx).into_any_element()
        };

        let chosen = self
            .selected
            .clone()
            .filter(|id| list.iter().any(|m| &m.id == id));
        let has_choice = chosen.is_some();
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                let k = &ev.keystroke;
                if k.modifiers.modified() {
                    return;
                }
                if k.key == "escape" {
                    this.done(None, window, cx);
                } else if k.key == "enter" && this.focus.is_focused(window) {
                    if let Some(id) = this.selected.clone() {
                        this.done(Some(id), window, cx);
                    }
                }
            }))
            .child(
                Assistant::new("model-picker", t("onboarding.picker.title"))
                    .icon("audio-lines")
                    .description(t("onboarding.picker.description"))
                    .child(filter)
                    .child(body)
                    .footer_start(
                        Button::new("picker-cancel")
                            .outline()
                            .label(t("common.cancel"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.done(None, window, cx)),
                            ),
                    )
                    .note(t_args(
                        "onboarding.picker.count",
                        &[("count", &list.len().to_string())],
                    ))
                    .action(continue_button(
                        "picker-use",
                        t("onboarding.picker.use"),
                        has_choice,
                        cx.listener(move |this, _, window, cx| {
                            let chosen = this.selected.clone();
                            if chosen.is_some() {
                                this.done(chosen, window, cx);
                            }
                        }),
                    )),
            )
    }
}
