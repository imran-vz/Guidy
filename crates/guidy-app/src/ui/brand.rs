//! Guidy's wordmark (a waveform glyph and the name) and the transport
//! glyphs every recording and playback control shares.

use crate::ui::kit::icon;
use gpui_kit::component::{h_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// The one pause glyph: two filled bars (`assets/icons/pause-fill.svg`).
/// Use it for every pause control (HUD pill and panel, meetings toolbar,
/// players) so pause never reads outlined in one place and solid in another.
pub const PAUSE_ICON: &str = "pause-fill";
/// The matching filled play/resume triangle (`assets/icons/play-fill.svg`).
pub const PLAY_ICON: &str = "play-fill";
/// The filled stop glyph (matches the filled pause and play).
pub const STOP_ICON: &str = "stop-fill";

/// The pause glyph, or the play glyph while `paused` (the action it takes).
pub fn pause_icon(paused: bool) -> Icon {
    icon(if paused { PLAY_ICON } else { PAUSE_ICON })
}

pub fn wordmark(size: Pixels, cx: &App) -> impl IntoElement {
    let accent = cx.theme().primary;
    h_flex()
        .gap(size * 0.3)
        .items_center()
        .child(icon("audio-lines").with_size(size).text_color(accent))
        .child(
            div()
                .text_size(size)
                .font_weight(FontWeight::MEDIUM)
                .child(guidy_core::host::PRODUCT_NAME),
        )
}
