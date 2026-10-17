//! Guidy's wordmark: a waveform glyph and the name.

use crate::ui::kit::icon;
use gpui_kit::component::{h_flex, ActiveTheme, Sizable, StyledExt};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn wordmark(size: Pixels, cx: &App) -> impl IntoElement {
    let accent = cx.theme().primary;
    h_flex()
        .gap(size * 0.3)
        .items_center()
        .child(icon("audio-lines").with_size(size).text_color(accent))
        .child(
            div()
                .text_size(size)
                .font_semibold()
                .child(guidy_core::host::PRODUCT_NAME),
        )
}
