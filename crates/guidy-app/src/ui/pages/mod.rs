//! Settings pages.
//!
//! Each page is an entity the main window shows under its unified title
//! bar. The window asks the page for its title-bar content through
//! [`Page`]; the page body picks a layout from `kit` (`scroll_page` for
//! forms, `fill_page` for pages that own their regions).

pub mod about;
pub mod advanced;
pub mod debug;
pub mod general;
pub mod history;
pub mod meetings;
pub mod models;
pub mod post_processing;
pub mod storage;
pub mod usage;

use crate::ui::design::{self, dp};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{div, AnyElement, App, Context, FontWeight, Render, SharedString, Window};

/// A page that couldn't load: a centred block with a 32 pt alert icon, a
/// headline, one sentence and one action (Try Again). `id` names the
/// block (tests look for it); put it in a [`crate::ui::kit::fill_page`].
pub fn error_state(
    id: &'static str,
    title: SharedString,
    message: SharedString,
    action: Button,
    cx: &App,
) -> AnyElement {
    let tone = design::tone(cx);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .px(design::PAGE_X)
        .child(
            v_flex()
                .id(id)
                .max_w(dp(400.))
                .items_center()
                .gap(dp(8.))
                .text_center()
                .child(
                    crate::ui::kit::icon("circle-alert")
                        .size(dp(32.))
                        .text_color(tone.text_muted),
                )
                .child(
                    div()
                        .pt(dp(4.))
                        .text_size(design::TEXT_HEADLINE)
                        .line_height(dp(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tone.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(design::TEXT_BODY)
                        .line_height(design::LINE_BODY)
                        .text_color(tone.text_muted)
                        .child(message),
                )
                .child(div().pt(dp(8.)).child(action))
                .test_support(),
        )
        .into_any_element()
}

/// An error's cause as the sentence under an [`error_state`] headline: the
/// headline already names what failed, so the sentence starts with why
/// ("The data folder can’t be located."), capitalised, with curly
/// apostrophes and a closing period.
pub fn cause_sentence(error: &str) -> SharedString {
    let e = error.trim().replace('\'', "\u{2019}");
    let mut chars = e.chars();
    let mut out: String = match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => return SharedString::default(),
    };
    if !out.ends_with(['.', '!', '?', ')']) {
        out.push('.');
    }
    out.into()
}

/// What the window shell asks of a page entity. Called on every render of
/// the main window, so keep both methods cheap.
pub trait Page: Render + Sized {
    /// The title-bar title. `None` shows the section name.
    fn title(&self, _cx: &App) -> Option<SharedString> {
        None
    }

    /// Content shown in place of the title, at the title bar's left edge
    /// (a page's own view switcher). It stays put whatever the toolbar
    /// shows. `None` shows the title.
    fn leading(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    /// The page's actions, shown at the right end of the title bar (a few
    /// small buttons, a search field). Ids must be unique in the window.
    fn toolbar(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::cause_sentence;

    #[test]
    fn cause_sentence_leads_with_the_cause() {
        assert_eq!(
            cause_sentence("the data folder can't be located").as_ref(),
            "The data folder can\u{2019}t be located."
        );
        assert_eq!(cause_sentence("Locked.").as_ref(), "Locked.");
        assert_eq!(cause_sentence("  ").as_ref(), "");
    }
}
