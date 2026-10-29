//! Guidy's design language: a native macOS settings app. Dense and calm:
//! a full-height source-list sidebar, a unified title bar, flat panels
//! separated by hairlines, small corner radii (3–5 pt), sentence-case
//! section headers, one signal-yellow accent used sparingly (selection,
//! focus, primary actions, live states). Sizes are macOS points.
//!
//! The recording floater and the dictation overlay are a separate, heads-up
//! language (capsules, rim light, sheen) that keeps its own [`HUD_SCALE`].
//!
//! Colors that the stock components need live in `assets/themes/guidy.json`;
//! the extra surface and line tones are here, keyed off the theme mode.
//! The redesign spec is `docs/design/REDESIGN.md`.

use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

/// Interface scale: design sizes are macOS points. The theme's rem
/// (`font.size` in `assets/themes/guidy.json`) is 13, so stock components
/// match.
pub const SCALE: f32 = 1.0;

/// A design size at the interface [`SCALE`]. Hairlines stay `px(1.)`.
pub const fn dp(design: f32) -> Pixels {
    px(design * SCALE)
}

/// Scale of the heads-up surfaces (recording floater, dictation overlay,
/// notch geometry): they were drawn at 1.2× and keep that size.
pub const HUD_SCALE: f32 = 1.2;

/// A design size at the [`HUD_SCALE`].
pub const fn hud(design: f32) -> Pixels {
    px(design * HUD_SCALE)
}

// Type scale. Body text is the theme's 13 pt (rem).
/// Body, row titles, controls.
pub const TEXT_BODY: Pixels = dp(13.);
/// Descriptions, metadata, captions, section headers.
pub const TEXT_SECONDARY: Pixels = dp(11.);
/// Toolbar titles and group headings that need weight (use semibold).
pub const TEXT_TITLE: Pixels = dp(13.);
/// Empty states, onboarding headings, About.
pub const TEXT_HEADLINE: Pixels = dp(17.);
/// Onboarding hero only.
pub const TEXT_DISPLAY: Pixels = dp(26.);
/// Numbers, sizes, times, ids.
pub const TEXT_MONO: Pixels = dp(12.);

// HUD type: the same steps at the HUD scale.
pub const HUD_TEXT_BODY: Pixels = hud(13.);
pub const HUD_TEXT_TITLE: Pixels = hud(15.);
pub const HUD_TEXT_MONO: Pixels = hud(12.);
pub const HUD_TEXT_LABEL: Pixels = hud(10.5);
/// The rem the HUD windows render at (stock icon and spacing helpers),
/// set by their views each frame.
pub const HUD_REM: Pixels = hud(14.);

// Spacing (4 pt grid).
/// Horizontal page padding.
pub const PAGE_X: Pixels = dp(20.);
/// Padding above the first group on a page.
pub const PAGE_TOP: Pixels = dp(16.);
/// Padding below the last group on a scrolling page.
pub const PAGE_BOTTOM: Pixels = dp(24.);
/// Between groups on a page.
pub const GROUP_GAP: Pixels = dp(20.);
/// Between a group header and its panel.
pub const HEADER_GAP: Pixels = dp(6.);
/// Horizontal padding of rows and sections inside a panel.
pub const SECTION_X: Pixels = dp(12.);
/// Vertical padding of sections inside a panel.
pub const SECTION_Y: Pixels = dp(10.);
/// Vertical padding of a setting row.
pub const ROW_Y: Pixels = dp(6.);
/// A one-line setting row (with a description it grows to ~44).
pub const ROW_MIN_H: Pixels = dp(32.);

// Widths.
/// Forms (settings groups) are left-aligned and no wider than this.
pub const FORM_MAX_W: Pixels = dp(640.);
/// Long paragraphs.
pub const PROSE_MAX_W: Pixels = dp(560.);
pub const SIDEBAR_W: Pixels = dp(200.);
pub const SIDEBAR_ITEM_H: Pixels = dp(28.);

// Chrome heights.
/// The unified title bar / toolbar.
pub const TOOLBAR_H: Pixels = dp(38.);
pub const STATUS_BAR_H: Pixels = dp(22.);

// Control heights. Stock `Button` sizes: `.small()` ≈ 20, default ≈ 24–26.
pub const CONTROL_H_SMALL: Pixels = dp(20.);
pub const CONTROL_H: Pixels = dp(24.);
/// Onboarding call to action only.
pub const CONTROL_H_LARGE: Pixels = dp(28.);

// Shape. `rounded_full` only for dots, radio marks, switch and slider
// knobs, avatars, HUD capsules.
/// Progress, scrubber and timeline bars: thin capsules, like
/// `NSProgressIndicator`.
pub const RADIUS_TRACK: Pixels = dp(2.);
/// Chips, key caps, list and sidebar selection.
pub const RADIUS_CHIP: Pixels = dp(3.);
/// Buttons, inputs, selects, segmented controls (theme `radius`).
pub const RADIUS_CONTROL: Pixels = dp(4.);
/// Groups, panels, banners, popovers, dialogs (theme `radius.lg`).
pub const RADIUS_PANEL: Pixels = dp(5.);
/// The recording floater's expanded panel.
pub const HUD_RADIUS_PANEL: Pixels = hud(14.);
/// Blueprint grid pitch (onboarding and About illustrations).
pub const GRID: f32 = 24.;

/// Surface, line and accent tones for the active mode.
#[derive(Clone, Copy)]
pub struct Tone {
    pub dark: bool,
    /// The content area behind pages.
    pub ink: Hsla,
    /// Window chrome: sidebar, status bar.
    pub frame: Hsla,
    /// Panels on the content area.
    pub surface: Hsla,
    /// Chips, key caps, secondary fills, hover.
    pub raised: Hsla,
    /// List and sidebar selection while the list is not focused.
    pub selection: Hsla,
    /// Selection while its list has keyboard focus.
    pub selection_focused: Hsla,
    pub hairline: Hsla,
    pub grid: Hsla,
    /// Crosshair marks and construction lines.
    pub mark: Hsla,
    /// Illustration strokes.
    pub line_art: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    /// Tertiary text (≥ 4.5:1 on every surface it is drawn on).
    pub text_faint: Hsla,
    /// Accent fill: buttons, live dots, bars.
    pub accent: Hsla,
    /// Accent used as text or strokes (≥ 4.5:1 on the surfaces).
    pub accent_text: Hsla,
    /// Tinted accent background: chips, highlighted regions.
    pub accent_soft: Hsla,
    /// HUD only: specular top-edge highlight on the floater and overlay.
    pub rim: Hsla,
    /// Moving glass highlight: loading tracks and the HUD.
    pub sheen: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub fn tone(cx: &App) -> Tone {
    let theme = cx.theme();
    if theme.mode.is_dark() {
        Tone {
            dark: true,
            ink: c(0x0d0d0dff),
            frame: c(0x1b1b1bff),
            surface: c(0x141414ff),
            raised: c(0x242424ff),
            selection: c(0xffffff1a),
            selection_focused: c(0xffc72c33),
            hairline: c(0x262626ff),
            grid: c(0xffffff0d),
            mark: c(0xffffff2e),
            line_art: c(0x8a8a8aff),
            text: theme.foreground,
            text_muted: theme.muted_foreground,
            text_faint: c(0x858585ff),
            accent: c(0xffc72cff),
            accent_text: c(0xffc72cff),
            accent_soft: c(0xffc72c1f),
            rim: c(0xffffff1f),
            sheen: c(0xffffff1a),
        }
    } else {
        Tone {
            dark: false,
            ink: c(0xfbfbfaff),
            frame: c(0xf0f0edff),
            surface: c(0xffffffff),
            raised: c(0xebebe7ff),
            selection: c(0x00000017),
            selection_focused: c(0xffc72c4d),
            hairline: c(0xe3e3dfff),
            grid: c(0x0000000c),
            mark: c(0x00000029),
            line_art: c(0x8c8c86ff),
            text: theme.foreground,
            text_muted: theme.muted_foreground,
            text_faint: c(0x6f6f6aff),
            accent: c(0xffc72cff),
            accent_text: c(0x8a6000ff),
            accent_soft: c(0xffc72c33),
            rim: c(0xffffffff),
            sheen: c(0xffffff8c),
        }
    }
}

/// Tones for content drawn on black whatever the mode: the recording
/// floater, which reads as part of the notch.
pub fn tone_on_black() -> Tone {
    Tone {
        dark: true,
        ink: c(0x000000ff),
        frame: c(0x000000ff),
        surface: c(0x000000ff),
        raised: c(0xffffff1f),
        selection: c(0xffffff1f),
        selection_focused: c(0xffc72c33),
        hairline: c(0xffffff1a),
        grid: c(0xffffff0d),
        mark: c(0xffffff2e),
        line_art: c(0x8a8a8aff),
        text: c(0xffffffeb),
        text_muted: c(0xffffff94),
        text_faint: c(0xffffff5c),
        accent: c(0xffc72cff),
        accent_text: c(0xffc72cff),
        accent_soft: c(0xffc72c1f),
        rim: c(0xffffff1f),
        sheen: c(0xffffff1a),
    }
}

/// A section header: sentence case, small, semibold and muted, in the UI
/// font (group titles, column heads, labels over values). Text is shown as
/// given; write it in sentence case.
pub fn section_header(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .min_w_0()
        .truncate()
        .text_size(TEXT_SECONDARY)
        .line_height(dp(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tone(cx).text_muted)
        .child(text.into())
}

/// [`section_header`] at the HUD scale, for labels in the recording
/// floater (speaker names, recording state).
pub fn hud_label(text: impl Into<SharedString>, cx: &App) -> Div {
    section_header(text, cx)
        .text_size(HUD_TEXT_LABEL)
        .line_height(hud(14.))
}

/// Mono value text (numbers, sizes, timings).
pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(crate::theme::MONO_FONT)
        .text_size(TEXT_MONO)
        .child(text.into())
}

/// Small mono tag. `accent` tints it with the signal color. Pass `""` to
/// fill it with your own children (an icon, shimmering text).
pub fn chip(text: impl Into<SharedString>, accent: bool, cx: &App) -> Div {
    let t = tone(cx);
    let text: SharedString = text.into();
    h_flex()
        .flex_none()
        .items_center()
        .gap(dp(4.))
        .px(dp(5.))
        .rounded(RADIUS_CHIP)
        .font_family(crate::theme::MONO_FONT)
        .text_size(TEXT_SECONDARY)
        .line_height(dp(16.))
        .bg(if accent { t.accent_soft } else { t.raised })
        .text_color(if accent { t.accent_text } else { t.text_muted })
        .when(!text.is_empty(), |d| d.child(text))
}

/// A flat bordered surface; sections inside are separated by hairlines
/// (see [`section`]).
pub fn panel(cx: &App) -> Div {
    let t = tone(cx);
    v_flex()
        .relative()
        .w_full()
        .rounded(RADIUS_PANEL)
        .border_1()
        .border_color(t.hairline)
        .bg(t.surface)
        .overflow_hidden()
}

/// Padded block inside a panel; `divided` draws the hairline below it.
pub fn section(divided: bool, cx: &App) -> Div {
    let hairline = tone(cx).hairline;
    v_flex()
        .w_full()
        .px(SECTION_X)
        .py(SECTION_Y)
        .when(divided, |d| d.border_b_1().border_color(hairline))
}

/// HUD only (recording floater, overlay): the specular top edge of a glass
/// surface, a 1 px line that fades out towards both corners. Place inside a
/// `relative` parent. Ordinary surfaces have no rim light.
pub fn rim_light(cx: &App) -> impl IntoElement {
    let rim = tone(cx).rim;
    let clear = rim.opacity(0.);
    h_flex()
        .absolute()
        .top_0()
        .left(dp(14.))
        .right(dp(14.))
        .h(px(1.))
        .child(div().flex_1().h_full().bg(linear_gradient(
            90.,
            linear_color_stop(clear, 0.),
            linear_color_stop(rim, 1.),
        )))
        .child(div().flex_1().h_full().bg(linear_gradient(
            90.,
            linear_color_stop(rim, 0.),
            linear_color_stop(clear, 1.),
        )))
}

/// A slow diagonal glass highlight that glides across its `relative`
/// parent, fading in and out, then rests. Only for loading indicators
/// ([`track`], skeletons) and the HUD, never on ordinary surfaces. GPUI clips to rectangles, so the
/// band stays in a strip inset by the parent's corner `radius` and never
/// reaches rounded corners. Static when the system asks for reduced motion.
pub fn sheen(id: impl Into<ElementId>, radius: Pixels, cx: &App) -> AnyElement {
    if cx.reduce_motion() {
        return div().into_any_element();
    }
    let s = tone(cx).sheen;
    let clear = s.opacity(0.);
    let half = |from: Hsla, to: Hsla| {
        div().flex_1().h_full().bg(linear_gradient(
            100.,
            linear_color_stop(from, 0.),
            linear_color_stop(to, 1.),
        ))
    };
    const WIDTH: f32 = 0.4;
    let band = h_flex()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .w(relative(WIDTH))
        .opacity(0.)
        .child(half(clear, s))
        .child(half(s, clear))
        .with_animation(
            id,
            Animation::new(Duration::from_millis(5200)).repeat(),
            |el, delta| {
                // Glide across in the first 45% of the cycle, then rest. The
                // band stays inside the strip and fades in and out instead
                // of being cut off at an edge.
                let p = (delta / 0.45).min(1.0);
                let eased = p * p * (3.0 - 2.0 * p);
                let fade = (p * std::f32::consts::PI).sin();
                el.left(relative((1.0 - WIDTH) * eased)).opacity(fade)
            },
        );
    div()
        .absolute()
        .top(px(1.))
        .bottom(px(1.))
        .left(radius)
        .right(radius)
        .overflow_hidden()
        .child(band)
        .into_any_element()
}

/// Shimmering text for in-progress states ("Transcribing…").
pub fn shimmer_text(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    cx: &App,
) -> ShimmerText {
    let t = tone(cx);
    ShimmerText::new(text)
        .id(id)
        .duration(Duration::from_millis(2200))
        .spread(0.35)
        .highlight_color(if t.dark {
            t.text
        } else {
            t.text_faint.opacity(0.2)
        })
        .whitespace_nowrap()
}

/// A blueprint stage for the top of a [`panel`]: a fine square grid,
/// crosshair marks near the corners and a fade into the panel at the bottom.
pub fn blueprint(height: Pixels, cx: &App) -> Div {
    let t = tone(cx);
    let (grid, mark, ink) = (t.grid, t.mark, t.surface);
    div()
        .relative()
        .w_full()
        .h(height)
        .flex_none()
        .overflow_hidden()
        .bg(ink)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| paint_grid(bounds, grid, mark, window),
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(relative(0.35))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(ink.opacity(0.), 0.),
                    linear_color_stop(ink, 1.),
                )),
        )
}

fn paint_grid(bounds: Bounds<Pixels>, grid: Hsla, mark: Hsla, window: &mut Window) {
    let (o, s) = (bounds.origin, bounds.size);
    let step = dp(GRID);
    // Center the grid so the middle column lines up with the subject.
    let x0 = o.x + ((s.width / 2.) % step);
    let mut x = x0;
    while x < o.x + s.width {
        window.paint_quad(fill(
            Bounds::new(point(x, o.y), size(px(1.), s.height)),
            grid,
        ));
        x += step;
    }
    let mut y = o.y + ((s.height / 2.) % step);
    while y < o.y + s.height {
        window.paint_quad(fill(
            Bounds::new(point(o.x, y), size(s.width, px(1.))),
            grid,
        ));
        y += step;
    }
    // Crosshairs two cells in from each corner, on grid intersections.
    let inset = step * 2.;
    let left = x0 + step * ((inset - (x0 - o.x)) / step).ceil().max(0.);
    let right = left + step * (((o.x + s.width - inset) - left) / step).floor();
    let top_y0 = o.y + ((s.height / 2.) % step);
    let top = top_y0 + step * ((inset - (top_y0 - o.y)) / step).ceil().max(0.);
    let bottom = top + step * (((o.y + s.height - inset) - top) / step).floor();
    let arm = dp(4.5);
    for (cx_, cy) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        window.paint_quad(fill(
            Bounds::new(point(cx_ - arm, cy), size(arm * 2. + px(1.), px(1.))),
            mark,
        ));
        window.paint_quad(fill(
            Bounds::new(point(cx_, cy - arm), size(px(1.), arm * 2. + px(1.))),
            mark,
        ));
    }
}

/// A dashed horizontal leader line from an illustration to its label.
pub fn leader(accent: bool, cx: &App) -> impl IntoElement {
    let t = tone(cx);
    let color = if accent { t.accent_text } else { t.mark };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let y = bounds.origin.y + bounds.size.height / 2.;
            let mut path = PathBuilder::stroke(px(1.)).dash_array(&[dp(3.), dp(3.)]);
            path.move_to(point(bounds.origin.x, y));
            path.line_to(point(bounds.origin.x + bounds.size.width, y));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .h(px(1.))
}

/// A label hung off a dashed leader, like a callout on a technical drawing.
/// The leader fills the width it is given.
pub fn callout(label: impl Into<SharedString>, accent: bool, cx: &App) -> Div {
    let t = tone(cx);
    h_flex()
        .gap_2()
        .items_center()
        .child(div().flex_1().min_w(dp(16.)).child(leader(accent, cx)))
        .child(
            div()
                .flex_none()
                .font_family(crate::theme::MONO_FONT)
                .text_size(TEXT_MONO)
                .text_color(if accent { t.accent_text } else { t.text_muted })
                .child(label.into()),
        )
}

/// A line-art illustration from `assets/illustrations/`: `<name>.svg` is
/// drawn in the line tone, `<name>-fill.svg` in the soft accent and
/// `<name>-accent.svg` in the accent, stacked in that order. Inactive, the
/// accent layer falls back to the line tone and the fill is dropped.
pub fn illustration_state(name: &str, w: Pixels, h: Pixels, active: bool, cx: &App) -> Div {
    let t = tone(cx);
    let layer = |suffix: &str, color: Hsla| {
        svg()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .path(SharedString::from(format!(
                "illustrations/{name}{suffix}.svg"
            )))
            .text_color(color)
    };
    div()
        .relative()
        .flex_none()
        .w(w)
        .h(h)
        .child(layer("", t.line_art))
        .when(active, |d| d.child(layer("-fill", t.accent_soft)))
        .child(layer(
            "-accent",
            if active { t.accent_text } else { t.line_art },
        ))
}

/// One labeled value in a [`stat_grid`].
pub struct Stat {
    pub label: SharedString,
    pub value: SharedString,
    pub accent: bool,
}

impl Stat {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            accent: false,
        }
    }

    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }
}

/// Cells of section-header label over mono value, divided by hairlines. Long
/// values wrap rather than truncate.
pub fn stat_grid(stats: Vec<Stat>, columns: usize, cx: &App) -> Div {
    let t = tone(cx);
    let columns = columns.max(1);
    let rows = stats.len().div_ceil(columns);
    let mut stats = stats.into_iter();
    v_flex().w_full().children((0..rows).map(|r| {
        // A plain flex row (align: stretch), so every cell is full height
        // and labels line up at the top even when a value wraps.
        div()
            .flex()
            .flex_row()
            .w_full()
            .when(r + 1 < rows, |d| d.border_b_1().border_color(t.hairline))
            .children((0..columns).map(|i| {
                let cell = div()
                    .flex_1()
                    .min_w_0()
                    .px(SECTION_X)
                    .py(SECTION_Y)
                    .when(i + 1 < columns, |d| d.border_r_1().border_color(t.hairline));
                match stats.next() {
                    Some(s) => cell.child(
                        v_flex()
                            .gap(dp(4.))
                            .child(section_header(s.label, cx))
                            .child(
                                mono(s.value)
                                    .line_height(relative(1.4))
                                    .text_color(if s.accent { t.accent_text } else { t.text }),
                            ),
                    ),
                    None => cell,
                }
            }))
    }))
}

/// A dot in the accent with a soft halo, for "live" and "selected" marks.
pub fn signal_dot(cx: &App) -> Div {
    signal_dot_sized(dp(14.), cx)
}

/// [`signal_dot`] at the HUD scale.
pub fn hud_signal_dot(cx: &App) -> Div {
    signal_dot_sized(hud(14.), cx)
}

fn signal_dot_sized(halo: Pixels, cx: &App) -> Div {
    let t = tone(cx);
    div()
        .flex_none()
        .size(halo)
        .rounded_full()
        .bg(t.accent_soft)
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(halo / 2.).rounded_full().bg(t.accent))
}

/// A tinted notice box (warnings, errors, banners) in `color`.
pub fn notice(color: Hsla) -> Div {
    div()
        .w_full()
        .pl(dp(12.))
        .pr(dp(8.))
        .py(dp(8.))
        .rounded(RADIUS_PANEL)
        .border_1()
        .border_color(color.opacity(0.3))
        .bg(color.opacity(0.08))
}

/// The track of a segmented control; fill it with [`segment`]s.
pub fn segmented(cx: &App) -> Div {
    let t = tone(cx);
    h_flex()
        .flex_none()
        .h(CONTROL_H)
        .p(px(2.))
        .gap(px(2.))
        .rounded(RADIUS_CONTROL)
        .border_1()
        .border_color(t.hairline)
        .bg(t.raised.opacity(0.6))
}

/// One option in a [`segmented`] control: keyboard focusable (Tab), and
/// Enter or Space selects it like a click.
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let t = tone(cx);
    let ring = cx.theme().ring;
    h_flex()
        .id(id)
        .tab_index(0)
        .h_full()
        .px(dp(10.))
        .rounded(RADIUS_CHIP)
        .border_1()
        .border_color(transparent_black())
        .text_size(dp(12.))
        .focus_visible(move |s| s.border_color(ring))
        .when(selected, |d| {
            d.bg(t.surface)
                .text_color(t.text)
                .font_weight(FontWeight::MEDIUM)
                .shadow_xs()
        })
        .when(!selected, |d| {
            d.text_color(t.text_muted)
                .hover(move |s| s.text_color(t.text))
        })
        .child(label.into())
}

/// A 4 px progress track: accent fill (or none while indeterminate) under a
/// passing sheen.
pub fn track(id: impl Into<ElementId>, fraction: Option<f32>, cx: &App) -> Div {
    let t = tone(cx);
    div()
        .relative()
        .w_full()
        .h(dp(4.))
        .rounded(RADIUS_TRACK)
        .bg(t.raised)
        .overflow_hidden()
        .when_some(fraction, |d, f| {
            d.child(
                div()
                    .h_full()
                    .rounded(RADIUS_TRACK)
                    .bg(t.accent)
                    .w(relative(f.clamp(0.0, 1.0))),
            )
        })
        .child(sheen(id, dp(2.), cx))
}
