//! The setup-assistant frame every onboarding screen uses: the whole
//! window, no card. A centered column holds an icon + display title +
//! one-line description and the step's body (scrolling when it is long);
//! the footer is pinned to the window bottom under a full-width hairline,
//! with Back on the left and the step's actions (the default button last)
//! on the right, aligned with the column, like a macOS assistant.

use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

/// The column's width: an assistant pane, not a page.
pub const COLUMN_W: Pixels = dp(580.);
/// Below this window height the frame's margins tighten.
pub const COMPACT_BELOW_H: Pixels = dp(560.);
/// The column's minimum top margin (a calm start, not a page edge).
const TOP: Pixels = dp(48.);
const TOP_COMPACT: Pixels = dp(16.);
/// The footer's height (hairline included).
pub const FOOTER_H: Pixels = dp(56.);
/// The display title (20–26 pt by the design rules).
const TITLE_SIZE: Pixels = dp(22.);

/// The assistant frame. Build it, then add the body with `child`.
#[derive(IntoElement)]
pub struct Assistant {
    id: ElementId,
    icon: Option<&'static str>,
    title: SharedString,
    description: Option<SharedString>,
    body: Vec<AnyElement>,
    footer_start: Vec<AnyElement>,
    note: Option<AnyElement>,
    actions: Vec<AnyElement>,
}

impl Assistant {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            title: title.into(),
            description: None,
            body: Vec::new(),
            footer_start: Vec::new(),
            note: None,
            actions: Vec::new(),
        }
    }

    /// A Lucide icon shown in a tile left of the title.
    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Left end of the footer (Back).
    pub fn footer_start(mut self, el: impl IntoElement) -> Self {
        self.footer_start.push(el.into_any_element());
        self
    }

    /// Muted text in the footer, between Back and the actions.
    pub fn note(mut self, el: impl IntoElement) -> Self {
        self.note = Some(el.into_any_element());
        self
    }

    /// Right end of the footer; add the default button last.
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.actions.push(el.into_any_element());
        self
    }
}

impl ParentElement for Assistant {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

/// The page around a column: the column centered horizontally, with an
/// optical vertical center (a third of the spare height above, two
/// thirds below) and at least the top margin, so a short step is not a
/// top-heavy page and a long one starts at a calm margin.
pub fn column_page(
    id: impl Into<ElementId>,
    compact: bool,
    column: impl IntoElement,
) -> Stateful<Div> {
    v_flex()
        .id(id)
        .flex_1()
        .min_h_0()
        .w_full()
        .items_center()
        .px(design::PAGE_X)
        .pt(if compact { TOP_COMPACT } else { TOP })
        .child(div().flex_grow(1.).flex_shrink(1.))
        .child(column)
        .child(div().flex_grow(2.).flex_shrink(1.))
}

/// The footer bar pinned to the window bottom: a full-width hairline, its
/// content aligned with the column.
pub fn footer_bar(content: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .w_full()
        .h(FOOTER_H)
        .justify_center()
        .px(design::PAGE_X)
        .border_t_1()
        .border_color(design::tone(cx).hairline)
        .child(content)
}

impl RenderOnce for Assistant {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let id = self.id.clone();
        // A short window (the minimum is 480 pt) tightens the frame so the
        // body keeps the room.
        let compact = window.viewport_size().height < COMPACT_BELOW_H;
        // The display title is the one place the brand face is used.
        let header = h_flex()
            .flex_none()
            .w_full()
            .items_start()
            .gap(dp(14.))
            .pb(if compact { dp(8.) } else { dp(20.) })
            .when_some(self.icon, |d, name| d.child(icon_tile(name, cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(4.))
                    .child(
                        div()
                            // One UI family: the system font (Geist stays
                            // for the brand mark only).
                            .font_family(crate::theme::UI_FONT)
                            .text_size(TITLE_SIZE)
                            .line_height(dp(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title),
                    )
                    .when_some(self.description, |d, text| {
                        d.child(
                            div()
                                .text_size(design::TEXT_READING)
                                .line_height(dp(20.))
                                .text_color(tone.text_muted)
                                .child(text),
                        )
                    }),
            );
        // The window's content tone, the same as the title bar above.
        let surface = tone.ink;
        // The body is as tall as its content and shrinks (and scrolls)
        // when the window is shorter than that.
        let body = div()
            .id(SharedString::from(format!("{id}-body")))
            .flex_initial()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .pt(dp(2.))
                    .pb(dp(16.))
                    .gap(if compact { dp(12.) } else { dp(20.) })
                    .children(self.body),
            );
        // A visible lower scroll edge: content running into the footer
        // fades out instead of being cut at a line (over the bottom padding
        // it is invisible). Not interactive, so the wheel reaches the body.
        let body = v_flex()
            .relative()
            .flex_initial()
            .min_h_0()
            .w_full()
            .child(body)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(dp(16.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(surface.opacity(0.), 0.),
                        linear_color_stop(surface, 1.),
                    )),
            );
        let column = v_flex()
            .flex_initial()
            .min_h_0()
            .w_full()
            .max_w(COLUMN_W)
            .child(header)
            .child(body);
        let footer = h_flex()
            .w_full()
            .max_w(COLUMN_W)
            .gap(dp(8.))
            .children(self.footer_start)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .when_some(self.note, |d, note| d.child(note)),
            )
            .children(self.actions);
        v_flex()
            .id(self.id)
            .size_full()
            .bg(surface)
            .child(column_page(
                SharedString::from(format!("{id}-page")),
                compact,
                column,
            ))
            .child(footer_bar(footer, cx))
            .test_support()
    }
}

/// The icon tile of a pane header: neutral, the glyph in the accent ink.
fn icon_tile(name: &'static str, cx: &App) -> Div {
    let tone = design::tone(cx);
    div()
        .flex_none()
        .size(dp(40.))
        .mt(dp(-2.))
        .rounded(design::RADIUS_PANEL)
        .bg(tone.raised)
        .text_color(tone.accent_text)
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name).size(dp(22.)))
}

/// The footer's Back button.
pub fn back_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .outline()
        .label(t("common.back"))
        .on_click(on_click)
}

/// A footer action that is not the visual primary (Set Up Later, Quit,
/// Continue Without while a grant is the recommended action).
pub fn secondary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .outline()
        .label(label.into())
        .disabled(!enabled)
        .on_click(on_click)
}

/// The footer's default button.
pub fn continue_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .primary()
        .label(label.into())
        .disabled(!enabled)
        .on_click(on_click)
}

/// A group of rows in a pane body: a sentence-case header over a bordered
/// panel whose rows are divided by hairlines.
pub fn rows(title: Option<SharedString>, rows: Vec<AnyElement>, cx: &App) -> Div {
    let hairline = design::tone(cx).hairline;
    let count = rows.len();
    v_flex()
        .w_full()
        .gap(design::HEADER_GAP)
        .when_some(title, |d, title| {
            d.child(div().px(dp(2.)).child(design::group_header(title, cx)))
        })
        .child(
            design::panel(cx).children(rows.into_iter().enumerate().map(move |(i, row)| {
                div()
                    .w_full()
                    .when(i + 1 < count, |d| d.border_b_1().border_color(hairline))
                    .child(row)
            })),
        )
}

/// The leading slot of a [`row`]: icons (16 pt) and checks/radios (14 pt
/// with their 3 pt focus frame) are centered in it, so every row's text
/// starts at one x.
pub const LEAD_SLOT: Pixels = dp(20.);
/// Between the leading slot and the text.
pub const LEAD_GAP: Pixels = dp(8.);
/// Where a row's text starts (panel inset + slot + gap): content stacked
/// under a row's title (a description, a form) lines up with it.
pub const TEXT_INSET: Pixels = dp(16. + 20. + 8.);

/// A row's leading icon or mark in its [`LEAD_SLOT`]: as tall as the
/// title's line, so the glyph or mark centers on it.
pub fn lead_slot(el: impl IntoElement) -> Div {
    div()
        .flex_none()
        .w(LEAD_SLOT)
        .h(design::LINE_BODY)
        .flex()
        .items_center()
        .justify_center()
        .child(el)
}

/// One row: an optional leading control or icon, a title with a
/// description below it, and trailing content (a size, a chip, a button).
pub fn row(
    id: impl Into<ElementId>,
    leading: Option<AnyElement>,
    title: impl IntoElement,
    description: Option<SharedString>,
    trailing: Option<AnyElement>,
    cx: &App,
) -> Stateful<Div> {
    let tone = design::tone(cx);
    // A one-line row centers its title on a taller trailing control (a
    // button, a segmented control); with a description the title line
    // leads and the trailing content sits beside it.
    let one_line = description.is_none();
    h_flex()
        .id(id)
        .w_full()
        .map(|d| {
            if one_line {
                d.items_center()
            } else {
                d.items_start()
            }
        })
        .gap(LEAD_GAP)
        .px(design::SECTION_X)
        .py(design::ROW_Y)
        .when_some(leading, |d, l| d.child(lead_slot(l)))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(dp(2.))
                .child(
                    div()
                        .text_size(design::TEXT_BODY)
                        .line_height(design::LINE_BODY)
                        .child(title),
                )
                .when_some(description, |d, text| {
                    d.child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .line_height(design::LINE_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(text),
                    )
                }),
        )
        .when_some(trailing, |d, t| {
            d.child(
                h_flex()
                    .flex_none()
                    .when(!one_line, |d| d.mt(dp(1.)))
                    .gap(dp(8.))
                    .child(t),
            )
        })
}

/// A leading icon for a [`row`].
pub fn row_icon(name: &'static str, cx: &App) -> AnyElement {
    icon(name)
        .size(dp(16.))
        .text_color(design::tone(cx).text_muted)
        .into_any_element()
}

/// Mono size / status text at the end of a row.
pub fn trailing_text(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    design::mono(text)
        .text_size(design::TEXT_SECONDARY)
        .text_color(design::tone(cx).text_muted)
        .into_any_element()
}

/// Called with the state an [`AccentMark`] is asked to take.
type MarkHandler = Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// What an [`AccentMark`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Check,
    Radio,
}

/// A checkbox or radio drawn in the accent ink (`tone.accent_text`), the
/// one look for checks and radios across the app (the AI settings use the
/// same marks). The stock components fill with the bright accent, which is
/// too faint as a mark on white. Behaviour (focus, Space, role, label) is
/// gpui-base's; a 2 pt ring-coloured edge shows keyboard focus and is
/// transparent at rest, so nothing moves.
#[derive(IntoElement)]
pub struct AccentMark {
    id: ElementId,
    kind: MarkKind,
    checked: bool,
    disabled: bool,
    label: Option<SharedString>,
    on_click: Option<MarkHandler>,
}

impl AccentMark {
    pub fn new(id: impl Into<ElementId>, kind: MarkKind) -> Self {
        Self {
            id: id.into(),
            kind,
            checked: false,
            disabled: false,
            label: None,
            on_click: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Called with the requested state (always `true` for a radio).
    pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for AccentMark {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let ring = if focused {
            cx.theme().ring
        } else {
            gpui_kit::transparent_black()
        };
        let radio = self.kind == MarkKind::Radio;
        let mark = div()
            .flex_none()
            .size(dp(14.))
            .flex()
            .items_center()
            .justify_center()
            .when(radio, |d| d.rounded_full())
            .when(!radio, |d| d.rounded(design::RADIUS_CHIP))
            .map(|d| match (radio, self.checked) {
                (true, true) => d.border_4().border_color(tone.accent_text),
                (false, true) => d
                    .bg(tone.accent_text)
                    .child(icon("check").size(dp(10.)).text_color(tone.surface)),
                _ => d.border_1().border_color(tone.text_faint),
            })
            .when(self.disabled, |d| d.opacity(0.5));
        let frame = div()
            .flex_none()
            .p(dp(1.))
            .border_2()
            .border_color(ring)
            .when(radio, |d| d.rounded_full())
            .when(!radio, |d| d.rounded(design::RADIUS_CONTROL))
            .child(mark);
        let on_click = self.on_click;
        match self.kind {
            MarkKind::Check => gpui_kit::base::Checkbox::new(self.id)
                .checked(self.checked)
                .disabled(self.disabled)
                .track_focus(&focus)
                .when_some(self.label, |d, l| d.accessibility_label(l))
                .when_some(on_click, |d, f| {
                    d.on_change(move |state, _, window, cx| {
                        window.prevent_default();
                        f(
                            &(state == gpui_kit::base::CheckboxState::Checked),
                            window,
                            cx,
                        )
                    })
                })
                .flex_none()
                .child(frame)
                .into_any_element(),
            MarkKind::Radio => gpui_kit::base::Radio::new(self.id)
                .checked(self.checked)
                .disabled(self.disabled)
                .track_focus(&focus)
                .when_some(self.label, |d, l| d.accessibility_label(l))
                .when_some(on_click, |d, f| {
                    d.on_change(move |v, _, window, cx| {
                        window.prevent_default();
                        f(&v, window, cx)
                    })
                })
                .flex_none()
                .child(frame)
                .into_any_element(),
        }
    }
}
