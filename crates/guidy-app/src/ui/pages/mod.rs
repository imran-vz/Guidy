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
pub mod usage;

use gpui_kit::{AnyElement, App, Context, Render, SharedString, Window};

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
