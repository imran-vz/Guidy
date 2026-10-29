//! The setup-assistant frame every onboarding screen uses: one card,
//! centered in the window, with an icon + title + one-line description,
//! a scrolling body, and a footer bar holding Back on the left and the
//! step's actions (the default button last) on the right, like a macOS
//! assistant pane.

use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Disableable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// The card's width: an assistant pane, not a page.
pub const CARD_W: Pixels = dp(600.);
/// The card's height when the window has room: every step has the same
/// frame, so nothing moves between steps (the body scrolls, the footer
/// stays).
pub const CARD_H: Pixels = dp(500.);

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

impl RenderOnce for Assistant {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = design::tone(cx);
        let id = self.id.clone();
        let header = h_flex()
            .flex_none()
            .items_start()
            .gap(dp(12.))
            .px(dp(20.))
            .pt(dp(18.))
            .pb(dp(12.))
            .when_some(self.icon, |d, name| d.child(icon_tile(name, cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(3.))
                    .child(
                        div()
                            .text_size(design::TEXT_HEADLINE)
                            .line_height(dp(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title),
                    )
                    .when_some(self.description, |d, text| {
                        d.child(
                            div()
                                .text_size(design::TEXT_BODY)
                                .line_height(dp(18.))
                                .text_color(tone.text_muted)
                                .child(text),
                        )
                    }),
            );
        let body = div()
            .id(SharedString::from(format!("{id}-body")))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .px(dp(20.))
                    .pt(dp(2.))
                    .pb(dp(16.))
                    .gap(dp(14.))
                    .children(self.body),
            );
        let footer = h_flex()
            .flex_none()
            .h(dp(48.))
            .px(dp(16.))
            .gap(dp(8.))
            .border_t_1()
            .border_color(tone.hairline)
            .bg(tone.frame)
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
        div()
            .id(self.id)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(dp(24.))
            .pt(dp(4.))
            .pb(dp(24.))
            .child(
                design::panel(cx)
                    .w_full()
                    .max_w(CARD_W)
                    .h_full()
                    .max_h(CARD_H)
                    .child(header)
                    .child(body)
                    .child(footer),
            )
            .test_support()
    }
}

/// The icon tile of a pane header.
fn icon_tile(name: &'static str, cx: &App) -> Div {
    let tone = design::tone(cx);
    div()
        .flex_none()
        .size(dp(36.))
        .rounded(design::RADIUS_PANEL)
        .bg(tone.accent_soft)
        .text_color(tone.accent_text)
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name).size(dp(20.)))
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
            d.child(div().px(dp(2.)).child(design::section_header(title, cx)))
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
    h_flex()
        .id(id)
        .w_full()
        .items_start()
        .gap(dp(10.))
        .px(design::SECTION_X)
        .py(dp(8.))
        .when_some(leading, |d, l| {
            d.child(div().flex_none().mt(dp(1.)).child(l))
        })
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(dp(2.))
                .child(
                    div()
                        .text_size(design::TEXT_BODY)
                        .line_height(dp(18.))
                        .child(title),
                )
                .when_some(description, |d, text| {
                    d.child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .line_height(dp(15.))
                            .text_color(tone.text_muted)
                            .child(text),
                    )
                }),
        )
        .when_some(trailing, |d, t| {
            d.child(h_flex().flex_none().mt(dp(1.)).gap(dp(8.)).child(t))
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
