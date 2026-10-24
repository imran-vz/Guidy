//! GPUI views.

pub mod banners;
pub mod brand;
pub mod design;
pub mod kit;
pub mod language_selector;
pub mod main_window;
pub mod model_card;
pub mod onboarding;
pub mod overlay;
pub mod pages;
pub mod recording_widget;
pub mod shortcut_input;
pub mod status_bar;
pub mod whats_new;

use gpui_kit::component::input;
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction};

gpui_kit::actions!(guidy, [ToggleDebugMode, CloseWindow, Quit, HideApp]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        // Ctrl/Cmd+Shift+D debug toggle.
        KeyBinding::new("cmd-shift-d", ToggleDebugMode, None),
        KeyBinding::new("ctrl-shift-d", ToggleDebugMode, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", HideApp, None),
    ]);
    // App-level, so Quit/Hide work from the menu bar with no window focused.
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &HideApp, cx| cx.hide());
}

/// The macOS menu bar:
/// app menu with Hide/Quit, and an Edit menu so the system clipboard
/// shortcuts reach text fields.
pub fn set_app_menus(cx: &mut App) {
    let name = guidy_core::host::PRODUCT_NAME;
    cx.set_menus([
        Menu::new(name).items([
            MenuItem::action(format!("Hide {name}"), HideApp),
            MenuItem::separator(),
            MenuItem::action(format!("Quit {name}"), Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
        ]),
    ]);
}
