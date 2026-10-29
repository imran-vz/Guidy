//! GPUI views.

pub mod banners;
pub mod brand;
pub mod design;
pub mod download_control;
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

use crate::ui::main_window::Section;
use gpui_kit::component::input;
use gpui_kit::{App, Global, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

gpui_kit::actions!(
    guidy,
    [
        ToggleDebugMode,
        CloseWindow,
        Quit,
        HideApp,
        HideOtherApps,
        ShowAllApps,
        AboutGuidy,
        OpenSettings,
        MinimizeWindow,
        ZoomWindow,
        BringAllToFront,
        OpenHelp,
        /// Sidebar: the section above / below the selected one.
        SelectPreviousSection,
        SelectNextSection,
        ShowGeneral,
        ShowHistory,
        ShowMeetings,
        ShowModels,
        ShowAdvanced,
        ShowPostProcessing,
        ShowUsage,
        ShowDebug,
        ShowAbout,
    ]
);

/// The key context of the main window's sidebar.
pub const SIDEBAR_CONTEXT: &str = "GuidySidebar";

/// Where "Guidy Help" points (set at build time); empty disables the item.
const HELP_URL: &str = env!("GUIDY_HELP_URL");

pub fn init(cx: &mut App) {
    pages::meetings::init(cx);
    cx.bind_keys([
        // Ctrl/Cmd+Shift+D debug toggle.
        KeyBinding::new("cmd-shift-d", ToggleDebugMode, None),
        KeyBinding::new("ctrl-shift-d", ToggleDebugMode, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("cmd-1", ShowGeneral, None),
        KeyBinding::new("cmd-2", ShowHistory, None),
        KeyBinding::new("cmd-3", ShowMeetings, None),
        KeyBinding::new("cmd-4", ShowModels, None),
        KeyBinding::new("cmd-5", ShowAdvanced, None),
        KeyBinding::new("cmd-6", ShowPostProcessing, None),
        KeyBinding::new("cmd-7", ShowUsage, None),
        KeyBinding::new("cmd-8", ShowDebug, None),
        KeyBinding::new("cmd-9", ShowAbout, None),
        KeyBinding::new("up", SelectPreviousSection, Some(SIDEBAR_CONTEXT)),
        KeyBinding::new("down", SelectNextSection, Some(SIDEBAR_CONTEXT)),
    ]);
    // App-level, so they work from the menu bar with no window focused.
    // A window-dispatched action reaches these while its window is being
    // updated, so anything that updates a window is deferred.
    cx.on_action(|_: &Quit, cx| cx.quit());
    // Reached when the focus is in a dialog (the main window's own views
    // handle ⌘W themselves).
    cx.on_action(|_: &CloseWindow, cx| cx.defer(crate::windows::close_active_window));
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(|_: &AboutGuidy, cx| show_section(Section::About, cx));
    cx.on_action(|_: &OpenSettings, cx| show_section(Section::General, cx));
    cx.on_action(|_: &MinimizeWindow, cx| {
        cx.defer(|cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.minimize_window());
            }
        })
    });
    cx.on_action(|_: &ZoomWindow, cx| {
        cx.defer(|cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.zoom_window());
            }
        })
    });
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.on_action(|_: &OpenHelp, cx| {
        if !HELP_URL.is_empty() {
            cx.open_url(HELP_URL);
        }
    });
    cx.on_action(|_: &ShowGeneral, cx| show_section(Section::General, cx));
    cx.on_action(|_: &ShowHistory, cx| show_section(Section::History, cx));
    cx.on_action(|_: &ShowMeetings, cx| show_section(Section::Meetings, cx));
    cx.on_action(|_: &ShowModels, cx| show_section(Section::Models, cx));
    cx.on_action(|_: &ShowAdvanced, cx| show_section(Section::Advanced, cx));
    cx.on_action(|_: &ShowPostProcessing, cx| show_section(Section::PostProcessing, cx));
    cx.on_action(|_: &ShowUsage, cx| show_section(Section::Usage, cx));
    cx.on_action(|_: &ShowDebug, cx| show_section(Section::Debug, cx));
    cx.on_action(|_: &ShowAbout, cx| show_section(Section::About, cx));
}

fn show_section(section: Section, cx: &mut App) {
    cx.defer(move |cx| crate::windows::show_section(section, cx));
}

/// The menu bar as last built: rebuilt only when the visible sections or
/// the UI language change.
#[derive(PartialEq)]
struct MenuKey {
    language: String,
    sections: Vec<&'static str>,
}

struct BuiltMenus(MenuKey);
impl Global for BuiltMenus {}

/// The sidebar sections in order (Meetings only where supported).
pub fn visible_sections(cx: &App) -> Vec<&'static str> {
    match cx.try_global::<crate::state::GlobalAppState>() {
        Some(g) => {
            let state = g.0.read(cx);
            crate::logic::meetings::with_meetings_section(
                crate::logic::flows::sidebar_sections(state.app_settings()),
                state.meetings_supported,
            )
        }
        None => crate::logic::flows::sidebar_sections(None),
    }
}

/// Build the macOS menu bar if the sections or language changed since the
/// last build: the standard app, File, Edit, View, Window and Help menus.
pub fn sync_app_menus(cx: &mut App) {
    let key = MenuKey {
        language: guidy_i18n::language(),
        sections: visible_sections(cx),
    };
    if cx.try_global::<BuiltMenus>().is_some_and(|b| b.0 == key) {
        return;
    }
    set_app_menus_for(&key.sections, cx);
    cx.set_global(BuiltMenus(key));
}

/// Build the macOS menu bar now.
pub fn set_app_menus(cx: &mut App) {
    if let Some(b) = cx.try_global::<BuiltMenus>() {
        let sections = b.0.sections.clone();
        set_app_menus_for(&sections, cx);
    } else {
        sync_app_menus(cx);
    }
}

fn set_app_menus_for(sections: &[&'static str], cx: &mut App) {
    use crate::ui::kit::{t, t_args};
    let name = guidy_core::host::PRODUCT_NAME;
    let app = |key: &str| t_args(key, &[("app", name)]);
    let section_item = |id: &str| -> Option<MenuItem> {
        let section = Section::from_id(id)?;
        let label = t(section.label_key());
        Some(match section {
            Section::General => MenuItem::action(label, ShowGeneral),
            Section::History => MenuItem::action(label, ShowHistory),
            Section::Meetings => MenuItem::action(label, ShowMeetings),
            Section::Models => MenuItem::action(label, ShowModels),
            Section::Advanced => MenuItem::action(label, ShowAdvanced),
            Section::PostProcessing => MenuItem::action(label, ShowPostProcessing),
            Section::Usage => MenuItem::action(label, ShowUsage),
            Section::Debug => MenuItem::action(label, ShowDebug),
            Section::About => MenuItem::action(label, ShowAbout),
        })
    };
    cx.set_menus([
        Menu::new(name).items([
            MenuItem::action(app("menu.about"), AboutGuidy),
            MenuItem::separator(),
            MenuItem::action(t("menu.settings"), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(t("menu.services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(app("menu.hide"), HideApp),
            MenuItem::action(t("menu.hideOthers"), HideOtherApps),
            MenuItem::action(t("menu.showAll"), ShowAllApps),
            MenuItem::separator(),
            MenuItem::action(app("menu.quit"), Quit),
        ]),
        Menu::new(t("menu.file")).items([MenuItem::action(t("menu.closeWindow"), CloseWindow)]),
        Menu::new(t("menu.edit")).items([
            MenuItem::os_action(t("menu.undo"), input::Undo, OsAction::Undo),
            MenuItem::os_action(t("menu.redo"), input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(t("menu.cut"), input::Cut, OsAction::Cut),
            MenuItem::os_action(t("menu.copy"), input::Copy, OsAction::Copy),
            MenuItem::os_action(t("menu.paste"), input::Paste, OsAction::Paste),
            MenuItem::os_action(t("menu.selectAll"), input::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new(t("menu.view")).items(sections.iter().filter_map(|id| section_item(id))),
        // AppKit treats the menu titled "Window" as the window menu; the
        // items below work under any title.
        Menu::new(t("menu.window")).items([
            MenuItem::action(t("menu.minimize"), MinimizeWindow),
            MenuItem::action(t("menu.zoom"), ZoomWindow),
            MenuItem::separator(),
            MenuItem::action(t("menu.bringAllToFront"), BringAllToFront),
        ]),
        Menu::new(t("menu.helpMenu"))
            .items([MenuItem::action(app("menu.appHelp"), OpenHelp).disabled(HELP_URL.is_empty())]),
    ]);
}
