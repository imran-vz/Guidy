//! GPUI views.

pub mod banners;
pub mod brand;
pub mod kit;
pub mod language_selector;
pub mod main_window;
pub mod model_card;
pub mod onboarding;
pub mod overlay;
pub mod pages;
pub mod shortcut_input;
pub mod status_bar;
pub mod whats_new;

use gpui_kit::{App, KeyBinding};

gpui_kit::actions!(guidy, [ToggleDebugMode, CloseWindow, Quit]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        // Handy's Ctrl/Cmd+Shift+D debug toggle.
        KeyBinding::new("cmd-shift-d", ToggleDebugMode, None),
        KeyBinding::new("ctrl-shift-d", ToggleDebugMode, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}
