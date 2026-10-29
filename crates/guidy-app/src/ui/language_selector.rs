//! Recognition-language picker: a searchable combobox (type to filter,
//! ↑/↓ to move, Enter to pick, Esc to close) over the languages the current
//! model supports.

use crate::logic::languages::{available_languages, effective_language, language_label};
use crate::state::AppState;
use crate::ui::design::dp;
use crate::ui::kit::{reset_button, t, SettingRow};
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::h_flex;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::{IndexPath, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// One language in the list: its code and display name.
#[derive(Clone)]
struct Language {
    code: &'static str,
    label: SharedString,
}

impl SearchableListItem for Language {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.code
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        self.label.to_lowercase().contains(&q) || self.code.to_lowercase().starts_with(&q)
    }
}

type LanguageCombo = ComboboxState<SearchableVec<Language>>;

pub struct LanguageSelector {
    state: Entity<AppState>,
    combo: Entity<LanguageCombo>,
    /// (supported languages, detection) the list was built for.
    built_for: (Vec<String>, bool),
    /// The language the combobox shows as selected.
    shown: String,
    _subs: Vec<Subscription>,
}

fn label_for(code: &str) -> SharedString {
    if code == "auto" {
        return t("settings.general.language.auto");
    }
    language_label(code).unwrap_or(code).to_string().into()
}

fn items(supported: &[String], detect: bool) -> Vec<Language> {
    available_languages(supported, detect)
        .into_iter()
        .map(|(code, _)| Language {
            code,
            label: label_for(code),
        })
        .collect()
}

impl LanguageSelector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let (supported, detect) = capabilities(&state, cx);
        let selected = selected_language(&state, cx);
        let list = items(&supported, detect);
        let index = list
            .iter()
            .position(|l| l.code == selected)
            .map(|ix| IndexPath::default().row(ix));
        let combo = cx.new(|cx| {
            ComboboxState::new(
                SearchableVec::new(list),
                index.into_iter().collect(),
                window,
                cx,
            )
            .searchable(true)
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync(window, cx);
                cx.notify();
            }),
            cx.subscribe(&combo, |this, _, ev: &ComboboxEvent<_>, cx| {
                if let ComboboxEvent::Change(values) = ev {
                    if let Some(code) = values.first() {
                        if *code != this.shown {
                            this.shown = code.to_string();
                            let code = code.to_string();
                            this.state.update(cx, |s, cx| {
                                s.update_setting("selected_language", serde_json::json!(code), cx)
                            });
                        }
                    }
                }
            }),
        ];
        Self {
            state,
            combo,
            built_for: (supported, detect),
            shown: selected,
            _subs: subs,
        }
    }

    /// Follow model changes (other languages) and outside setting changes.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caps = capabilities(&self.state, cx);
        let selected = selected_language(&self.state, cx);
        if caps != self.built_for {
            let list = items(&caps.0, caps.1);
            self.combo.update(cx, |c, cx| {
                c.set_items(SearchableVec::new(list), window, cx);
            });
            self.built_for = caps;
            self.shown.clear();
        }
        if selected != self.shown {
            self.shown = selected.clone();
            let code = available_languages(&self.built_for.0, self.built_for.1)
                .into_iter()
                .map(|(c, _)| c)
                .find(|c| *c == selected);
            self.combo.update(cx, |c, cx| match code {
                Some(code) => c.set_selected_values(&[code], window, cx),
                None => c.clear_selection(cx),
            });
        }
    }
}

/// (supported languages, supports detection) of the current model.
fn capabilities(state: &Entity<AppState>, cx: &App) -> (Vec<String>, bool) {
    let st = state.read(cx);
    st.models
        .model(&st.models.current_model)
        .map(|m| (m.supported_languages.clone(), m.supports_language_detection))
        .unwrap_or((Vec::new(), true))
}

/// The language recognition uses: the stored choice, adjusted to what the
/// model supports.
fn selected_language(state: &Entity<AppState>, cx: &App) -> String {
    let intent = state
        .read(cx)
        .app_settings()
        .map(|s| s.selected_language.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "auto".into());
    let (supported, detect) = capabilities(state, cx);
    effective_language(&intent, &supported, detect)
}

impl Render for LanguageSelector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error =
            crate::ui::pages::advanced::save_error(self.state.read(cx), "selected_language");
        SettingRow::new("language-selector", t("settings.general.language.title"))
            .description(t("settings.general.language.descriptionShort"))
            .error(error)
            .control(
                h_flex()
                    .gap_1()
                    .child(reset_button("language-reset", false, |_, cx| {
                        AppState::global(cx)
                            .update(cx, |s, cx| s.reset_setting("selected_language", cx))
                    }))
                    .child(
                        div().id("language-combobox").w(dp(200.)).child(
                            Combobox::new(&self.combo)
                                .small()
                                .menu_width(dp(240.))
                                .menu_max_h(dp(260.))
                                .search_placeholder(t(
                                    "settings.general.language.searchPlaceholder",
                                ))
                                .empty(|_, _| {
                                    div()
                                        .p_2()
                                        .text_sm()
                                        .child(t("settings.general.language.noResults"))
                                }),
                        ),
                    ),
            )
    }
}
