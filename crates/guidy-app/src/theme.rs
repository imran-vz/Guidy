//! Guidy Dark / Guidy Light themes, the Geist fonts, and theme switching.

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Window, WindowAppearance};
use guidy_core::settings::Theme as ThemeSetting;

pub const UI_FONT: &str = "Geist";
pub const MONO_FONT: &str = "Geist Mono";
pub const DARK_THEME: &str = "Guidy Dark";
pub const LIGHT_THEME: &str = "Guidy Light";

/// Register fonts and the Guidy theme set; call once after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    if let Err(e) = cx.text_system().add_fonts(crate::assets::fonts()) {
        log::warn!("Failed to register fonts: {e}");
    }
    let json = crate::assets::text("themes/guidy.json").expect("theme json is embedded");
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(&json) {
        log::error!("Failed to load themes: {e}");
        return;
    }
    let registry = ThemeRegistry::global(cx);
    let dark = registry.themes().get(DARK_THEME).cloned();
    let light = registry.themes().get(LIGHT_THEME).cloned();
    Theme::update(cx, |t| {
        if let Some(d) = dark {
            t.dark_theme = d;
        }
        if let Some(l) = light {
            t.light_theme = l;
        }
    });
    apply_mode(mode_for(ThemeSetting::System, cx.window_appearance()), cx);
}

/// The palette for a theme setting given the OS appearance.
pub fn mode_for(setting: ThemeSetting, appearance: WindowAppearance) -> ThemeMode {
    match setting {
        ThemeSetting::Light => ThemeMode::Light,
        ThemeSetting::Dark => ThemeMode::Dark,
        ThemeSetting::System => match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        },
    }
}

pub fn apply_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None::<&mut Window>, cx);
}

/// Apply the user's theme setting now.
pub fn apply_setting(setting: ThemeSetting, cx: &mut App) {
    apply_mode(mode_for(setting, cx.window_appearance()), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_settings_ignore_the_os() {
        assert_eq!(
            mode_for(ThemeSetting::Dark, WindowAppearance::Light),
            ThemeMode::Dark
        );
        assert_eq!(
            mode_for(ThemeSetting::Light, WindowAppearance::Dark),
            ThemeMode::Light
        );
    }

    #[test]
    fn system_follows_the_os() {
        assert_eq!(
            mode_for(ThemeSetting::System, WindowAppearance::VibrantDark),
            ThemeMode::Dark
        );
        assert_eq!(
            mode_for(ThemeSetting::System, WindowAppearance::Light),
            ThemeMode::Light
        );
    }
}
