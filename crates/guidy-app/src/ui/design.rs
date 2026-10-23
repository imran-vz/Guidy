//! Guidy's design language: an ink well inside a graphite frame, hairline
//! grids, uppercase mono labels, one signal-yellow accent, line-art
//! illustrations with dashed leaders, and a glassy rim light and sheen.
//!
//! Colors that the stock components need live in `assets/themes/guidy.json`;
//! the extra surface and line tones are here, keyed off the theme mode.

use gpui_kit::component::button::Button;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Duration;

/// Interface scale. Every size below is a design size (as drawn in
/// `docs/design/guidy-mock.html`) times this; the theme's rem sizes in
/// `assets/themes/guidy.json` carry the same factor for the stock components.
pub const SCALE: f32 = 1.2;

/// A design size at the interface [`SCALE`]. Hairlines stay `px(1.)`.
pub const fn dp(design: f32) -> Pixels {
    px(design * SCALE)
}

// Type scale. Body text is the theme's 14 px; these are the other steps.
pub const TEXT_TITLE: Pixels = dp(15.);
pub const TEXT_HEADLINE: Pixels = dp(17.);
pub const TEXT_DISPLAY: Pixels = dp(30.);
pub const TEXT_BODY: Pixels = dp(13.);
pub const TEXT_MONO: Pixels = dp(12.);
pub const TEXT_EYEBROW: Pixels = dp(10.5);

// Spacing and shape.
pub const SECTION_X: Pixels = dp(16.);
pub const SECTION_Y: Pixels = dp(14.);
/// Gap between the window frame and the content well.
pub const WELL_INSET: Pixels = dp(6.);
pub const RADIUS_WELL: Pixels = dp(14.);
pub const RADIUS_PANEL: Pixels = dp(12.);
pub const RADIUS_CHIP: Pixels = dp(5.);
pub const RADIUS_PILL: Pixels = dp(999.);
/// Blueprint grid pitch.
pub const GRID: f32 = 24.;

/// Surface, line and accent tones for the active mode.
#[derive(Clone, Copy)]
pub struct Tone {
    pub dark: bool,
    /// The content well behind pages.
    pub ink: Hsla,
    /// Window chrome: title bar, sidebar, status bar.
    pub frame: Hsla,
    /// Panels inside the well.
    pub surface: Hsla,
    /// Chips, secondary pills, selected nav.
    pub raised: Hsla,
    pub hairline: Hsla,
    pub grid: Hsla,
    /// Crosshair marks and construction lines.
    pub mark: Hsla,
    /// Illustration strokes.
    pub line_art: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    /// Accent fill: buttons, live dots, bars.
    pub accent: Hsla,
    /// Accent used as text or strokes on ink.
    pub accent_text: Hsla,
    /// Tinted accent background: chips, highlighted regions.
    pub accent_soft: Hsla,
    /// Specular top-edge highlight on raised surfaces.
    pub rim: Hsla,
    /// Moving glass highlight.
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
            surface: c(0x121212ff),
            raised: c(0x212121ff),
            hairline: c(0x222222ff),
            grid: c(0xffffff0d),
            mark: c(0xffffff2e),
            line_art: c(0x8a8a8aff),
            text: theme.foreground,
            text_muted: theme.muted_foreground,
            text_faint: c(0x5f5f5fff),
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
            frame: c(0xefefecff),
            surface: c(0xffffffff),
            raised: c(0xebebe7ff),
            hairline: c(0xe5e5e1ff),
            grid: c(0x0000000c),
            mark: c(0x00000029),
            line_art: c(0x8c8c86ff),
            text: theme.foreground,
            text_muted: theme.muted_foreground,
            text_faint: c(0xa3a39dff),
            accent: c(0xffc72cff),
            accent_text: c(0xa87500ff),
            accent_soft: c(0xffc72c33),
            rim: c(0xffffffff),
            sheen: c(0xffffff8c),
        }
    }
}

/// Extra space between eyebrow letters (≈ 0.1em at 10.5 px).
const EYEBROW_TRACKING: Pixels = px(1.);
/// Geist Mono's advance at the eyebrow size, for spaces.
const EYEBROW_SPACE: Pixels = dp(6.3);

/// Uppercase mono label: section names, column heads, metadata. Letters
/// are tracked out like engraved labels; GPUI has no letter spacing, so
/// each character is its own box.
pub fn eyebrow(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    h_flex()
        .flex_none()
        .gap(EYEBROW_TRACKING)
        .overflow_hidden()
        .font_family(crate::theme::MONO_FONT)
        .text_size(TEXT_EYEBROW)
        .line_height(dp(14.))
        .text_color(tone(cx).text_muted)
        .children(text.to_uppercase().chars().map(|c| {
            if c.is_whitespace() {
                div().w(EYEBROW_SPACE).into_any_element()
            } else {
                div()
                    .child(SharedString::from(c.to_string()))
                    .into_any_element()
            }
        }))
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
        .px(dp(7.))
        .py(dp(2.))
        .rounded(RADIUS_CHIP)
        .font_family(crate::theme::MONO_FONT)
        .text_size(dp(11.))
        .line_height(dp(16.))
        .bg(if accent { t.accent_soft } else { t.raised })
        .text_color(if accent { t.accent_text } else { t.text_muted })
        .when(!text.is_empty(), |d| d.child(text))
}

/// A bordered surface with a rim light; sections inside are separated by
/// hairlines (see [`section`]).
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
        .child(rim_light(cx))
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

/// The specular top edge of a glass surface: a 1 px line that fades out
/// towards both corners. Place inside a `relative` parent.
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
/// parent, fading in and out, then rests. GPUI clips to rectangles, so the
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
/// `<name>-accent.svg` in the accent, stacked in that order.
pub fn illustration(name: &str, w: Pixels, h: Pixels, cx: &App) -> Div {
    illustration_state(name, w, h, true, cx)
}

/// [`illustration`] whose highlight can be switched off: inactive, the
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

/// Cells of eyebrow label over mono value, divided by hairlines. Long
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
                    .py(dp(12.))
                    .when(i + 1 < columns, |d| d.border_r_1().border_color(t.hairline));
                match stats.next() {
                    Some(s) => cell.child(
                        v_flex().gap(dp(6.)).child(eyebrow(s.label, cx)).child(
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
    let t = tone(cx);
    div()
        .flex_none()
        .size(dp(14.))
        .rounded_full()
        .bg(t.accent_soft)
        .flex()
        .items_center()
        .justify_center()
        .child(div().size(dp(7.)).rounded_full().bg(t.accent))
}

/// Pill-shaped buttons, as used for actions throughout the app.
pub trait Pill {
    fn pill(self) -> Self;
}

impl Pill for Button {
    fn pill(self) -> Self {
        self.rounded(RADIUS_PILL)
    }
}

/// A tinted notice box (warnings, errors) in `color`.
pub fn notice(color: Hsla) -> Div {
    div()
        .w_full()
        .pl(dp(14.))
        .pr(dp(10.))
        .py(dp(10.))
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
        .p(dp(3.))
        .gap(dp(2.))
        .rounded(RADIUS_PILL)
        .border_1()
        .border_color(t.hairline)
        .bg(t.surface)
}

/// One option in a [`segmented`] control.
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let t = tone(cx);
    div()
        .id(id)
        .px(dp(14.))
        .py(dp(5.))
        .rounded(RADIUS_PILL)
        .text_size(dp(12.))
        .cursor_pointer()
        .when(selected, |d| {
            d.bg(t.raised)
                .text_color(t.text)
                .child(rim_light(cx))
                .relative()
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
        .rounded_full()
        .bg(t.raised)
        .overflow_hidden()
        .when_some(fraction, |d, f| {
            d.child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(t.accent)
                    .w(relative(f.clamp(0.0, 1.0))),
            )
        })
        .child(sheen(id, dp(2.), cx))
}
