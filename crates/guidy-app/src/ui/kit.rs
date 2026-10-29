//! Shared building blocks in the macOS settings language (see
//! `design.rs`): page layouts ([`scroll_page`], [`fill_page`], [`page`]),
//! settings groups and rows, dropdowns, toggles, reset buttons, icons,
//! nested-scroll traps, translated strings.

use crate::ui::design::{self, dp};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

/// Translate in the active UI language.
pub fn t(key: &str) -> SharedString {
    guidy_i18n::t(key).into()
}

pub fn t_args(key: &str, args: &[(&str, &str)]) -> SharedString {
    guidy_i18n::t_args(key, args).into()
}

/// A text field's placeholder that follows the UI language. Fields are
/// made once (in a view's `new`), so a plain `.placeholder(t(..))` would
/// keep the old language; with this, a language switch keeps the pages,
/// and what is typed in them, and only the placeholder changes.
///
/// ```ignore
/// cx.new(|cx| InputState::new(window, cx).localized_placeholder("key", window, cx))
/// ```
pub trait LocalizedPlaceholder: Sized {
    fn localized_placeholder(
        self,
        key: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self;
}

macro_rules! localized_placeholder {
    ($state:ty) => {
        impl LocalizedPlaceholder for $state {
            fn localized_placeholder(
                self,
                key: impl Into<SharedString>,
                window: &mut Window,
                cx: &mut Context<Self>,
            ) -> Self {
                let key: SharedString = key.into();
                let app = crate::state::AppState::global(cx);
                let k = key.clone();
                // Weak on both ends: it goes away with the field.
                cx.subscribe_in(
                    &app,
                    window,
                    move |field, _, ev: &crate::state::AppEvent, window, cx| {
                        if matches!(ev, crate::state::AppEvent::LanguageChanged) {
                            field.set_placeholder(t(&k), window, cx);
                        }
                    },
                )
                .detach();
                self.placeholder(t(&key))
            }
        }
    };
}
localized_placeholder!(gpui_kit::component::input::InputState);
localized_placeholder!(gpui_kit::component::input::TextareaState);

/// A Lucide icon by file name (e.g. `"mic"`).
pub fn icon(name: &str) -> Icon {
    Icon::empty().path(SharedString::from(format!("icons/{name}.svg")))
}

/// A spinning loader icon. `id` must be unique
/// among siblings that animate.
pub fn spinner(id: impl Into<ElementId>, icon: Icon) -> AnimationElement<Icon> {
    icon.with_animation(
        id,
        Animation::new(std::time::Duration::from_millis(800)).repeat(),
        |el, delta| el.transform(Transformation::rotate(percentage(delta))),
    )
}

pub fn muted(cx: &App) -> Hsla {
    cx.theme().muted_foreground
}

/// A settings group: a sentence-case section header over a panel whose
/// rows are separated by hairlines (header → panel 6 pt).
pub fn group(title: Option<SharedString>, rows: Vec<AnyElement>, cx: &App) -> Div {
    let hairline = design::tone(cx).hairline;
    let count = rows.len();
    v_flex()
        .w_full()
        .gap(dp(6.))
        .when_some(title, |this, title| {
            this.child(div().px(dp(2.)).child(design::section_header(title, cx)))
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

/// The heading of a foldable section: a chevron and `title (count)`;
/// clicking it calls `on_toggle`.
pub fn foldable_heading(
    id: impl Into<ElementId>,
    title: SharedString,
    count: usize,
    open: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let ring = cx.theme().ring;
    h_flex()
        .id(id)
        .tab_index(0)
        .gap(dp(4.))
        .px(dp(2.))
        .py(dp(2.))
        .rounded(design::RADIUS_CHIP)
        .border_1()
        .border_color(transparent_black())
        .focus_visible(move |s| s.border_color(ring))
        .text_color(design::tone(cx).text_muted)
        .hover(|s| s.text_color(design::tone(cx).text))
        .child(
            icon(if open {
                "chevron-down"
            } else {
                "chevron-right"
            })
            .xsmall(),
        )
        .child(
            // A section header that takes the row's (hover) color.
            div()
                .text_size(design::TEXT_SECONDARY)
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("{title} ({count})")),
        )
        .on_click(on_toggle)
        .test_support()
}

/// A [`group`] whose heading folds it: a chevron and `count` stay
/// visible while the panel is hidden.
pub fn foldable_group(
    id: impl Into<ElementId>,
    title: SharedString,
    count: usize,
    open: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    rows: Vec<AnyElement>,
    cx: &App,
) -> Div {
    let heading = foldable_heading(id, title, count, open, on_toggle, cx);
    v_flex()
        .w_full()
        .gap(design::HEADER_GAP)
        .child(heading)
        .when(open, |d| d.child(group(None, rows, cx)))
}

/// One setting: title with its description below it on the left, the
/// control on the right (or below, [`SettingRow::stacked`]).
#[derive(IntoElement)]
pub struct SettingRow {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    stacked: bool,
    disabled: bool,
    error: Option<SharedString>,
    control: Option<AnyElement>,
}

/// Whether the UI language reads right to left (rows and toolbars mirror).
pub fn rtl() -> bool {
    guidy_i18n::is_rtl(&guidy_i18n::language())
}

impl SettingRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            stacked: false,
            disabled: false,
            error: None,
            control: None,
        }
    }

    /// An error for this setting (a failed write, an invalid value), shown
    /// under the description in the danger color.
    pub fn error(mut self, error: Option<impl Into<SharedString>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    /// A secondary line under the title (always visible).
    pub fn description(mut self, d: impl Into<SharedString>) -> Self {
        self.description = Some(d.into());
        self
    }

    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn control(mut self, c: impl IntoElement) -> Self {
        self.control = Some(c.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = design::tone(cx).text_muted;
        let danger = cx.theme().danger;
        let rtl = rtl();
        let label = v_flex()
            .gap(px(1.))
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .text_size(design::TEXT_BODY)
                    .line_height(dp(16.))
                    .child(self.title.clone()),
            )
            .when_some(self.description.clone(), |this, d| {
                this.child(
                    div()
                        .text_size(design::TEXT_SECONDARY)
                        .line_height(dp(14.))
                        .text_color(muted)
                        .child(d),
                )
            })
            .when_some(self.error.clone(), |this, e| {
                this.child(
                    div()
                        .id("setting-error")
                        .text_size(design::TEXT_SECONDARY)
                        .line_height(dp(14.))
                        .text_color(danger)
                        .child(e)
                        .test_support(),
                )
            })
            .when(rtl, |d| d.items_end().text_right());
        let control = self.control.unwrap_or_else(|| div().into_any_element());
        let row = if self.stacked {
            v_flex()
                .gap(dp(6.))
                .when(!rtl, |d| d.items_start())
                .when(rtl, |d| d.items_end())
                .child(label)
                .child(div().w_full().child(control))
        } else {
            h_flex()
                .gap(dp(16.))
                .justify_between()
                .when(rtl, |d| d.flex_row_reverse())
                .child(label)
                .child(div().flex_none().child(control))
        };
        div()
            .id(self.id.clone())
            .aria_label(self.title.clone())
            .when_some(self.description, |d, desc| d.aria_description(desc))
            .w_full()
            .min_h(design::ROW_MIN_H)
            .px(design::SECTION_X)
            .py(design::ROW_Y)
            .flex()
            .items_center()
            .when(self.disabled, |d| d.opacity(0.5))
            .child(row.w_full())
            .test_support()
    }
}

/// A dropdown option.
#[derive(Clone, Debug)]
pub struct DropdownOption {
    pub value: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub disabled: bool,
}

impl DropdownOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }

    pub fn description(mut self, d: impl Into<SharedString>) -> Self {
        self.description = Some(d.into());
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
}

type SelectHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type OpenHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A select-like button with a popup menu.
#[derive(IntoElement)]
pub struct Dropdown {
    id: ElementId,
    options: Vec<DropdownOption>,
    selected: Option<SharedString>,
    placeholder: SharedString,
    disabled: bool,
    loading: bool,
    min_width: Pixels,
    menu_width: Option<Pixels>,
    on_select: Option<SelectHandler>,
    on_open: Option<OpenHandler>,
}

impl Dropdown {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
            selected: None,
            placeholder: "Select an option...".into(),
            disabled: false,
            loading: false,
            min_width: dp(200.),
            menu_width: None,
            on_select: None,
            on_open: None,
        }
    }

    /// Called when the menu opens (e.g. refresh a device list).
    pub fn on_open(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    pub fn options(mut self, options: Vec<DropdownOption>) -> Self {
        self.options = options;
        self
    }

    pub fn selected(mut self, value: Option<impl Into<SharedString>>) -> Self {
        self.selected = value.map(Into::into);
        self
    }

    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn loading(mut self, l: bool) -> Self {
        self.loading = l;
        self
    }

    pub fn min_width(mut self, w: Pixels) -> Self {
        self.min_width = w;
        self
    }

    /// Width of the popup menu (defaults to the trigger width).
    pub fn menu_width(mut self, w: Pixels) -> Self {
        self.menu_width = Some(w);
        self
    }

    pub fn on_select(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Dropdown {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let label = self
            .selected
            .as_ref()
            .and_then(|v| self.options.iter().find(|o| &o.value == v))
            .map(|o| o.label.clone())
            .unwrap_or_else(|| self.placeholder.clone());
        let options = self.options.clone();
        let selected = self.selected.clone();
        let handler = self.on_select.clone();
        let trigger_width = self.min_width;
        let menu_width = self.menu_width.unwrap_or(trigger_width).max(trigger_width);
        let empty = options.is_empty();
        let on_open = self.on_open.clone();
        Button::new(self.id)
            .outline()
            .small()
            .label(label)
            .dropdown_caret(true)
            .loading(self.loading)
            .disabled(self.disabled)
            .w(trigger_width)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu.min_w(menu_width).max_h(dp(240.)).scrollable(true);
                if empty {
                    menu = menu.item(PopupMenuItem::label(t("common.noOptionsFound")));
                }
                for opt in &options {
                    let checked = selected.as_ref() == Some(&opt.value);
                    let value = opt.value.clone();
                    let h = handler.clone();
                    let item = match &opt.description {
                        Some(desc) => {
                            let (label, desc) = (opt.label.clone(), desc.clone());
                            PopupMenuItem::element(move |_, cx| {
                                v_flex()
                                    .py_0p5()
                                    .child(div().text_sm().child(label.clone()))
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::TEXT_SECONDARY)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(desc.clone()),
                                    )
                            })
                        }
                        None => PopupMenuItem::new(opt.label.clone()),
                    };
                    menu = menu.item(item.checked(checked).disabled(opt.disabled).on_click(
                        move |_, window, cx| {
                            if let Some(h) = &h {
                                h(&value, window, cx);
                            }
                        },
                    ));
                }
                menu
            })
            .on_open_change(move |open, window, cx| {
                if *open {
                    if let Some(f) = &on_open {
                        f(window, cx);
                    }
                }
            })
    }
}

/// A switch bound to a boolean setting.
pub fn toggle(
    id: impl Into<ElementId>,
    checked: bool,
    disabled: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Switch {
    Switch::new(id)
        .checked(checked)
        .disabled(disabled)
        .on_click(move |v: &bool, window, cx| on_change(*v, window, cx))
}

/// Small ghost button that restores a default. Put it before the control
/// (so controls stay flush with the row's right edge).
pub fn reset_button(
    id: impl Into<ElementId>,
    disabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon("rotate-ccw"))
        .disabled(disabled)
        .on_click(move |_, window, cx| on_click(window, cx))
}

/// Ask before a destructive action: an alert with `title`, `description`,
/// Cancel and an `ok_label` button (red when `destructive`). Enter confirms,
/// Esc cancels. `on_ok` runs on confirm; `on_cancel` when it is dismissed any
/// other way.
#[allow(clippy::too_many_arguments)]
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    title: SharedString,
    description: SharedString,
    ok_label: SharedString,
    destructive: bool,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) {
    use gpui_kit::component::button::ButtonVariant;
    use gpui_kit::component::WindowExt;
    let on_ok = Rc::new(on_ok);
    let on_cancel = Rc::new(on_cancel);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let confirmed = Rc::new(std::cell::Cell::new(false));
        let (ok, done) = (on_ok.clone(), confirmed.clone());
        let (cancel, closed) = (on_cancel.clone(), confirmed.clone());
        alert
            // Like a sheet: just under the title bar, never over it.
            .margin_top(design::TOOLBAR_H + dp(12.))
            .title(title.clone())
            .description(description.clone())
            .show_cancel(true)
            .ok_text(ok_label.clone())
            .cancel_text(t("common.cancel"))
            .when(destructive, |a| a.ok_variant(ButtonVariant::Danger))
            .on_ok(move |_, window, cx| {
                done.set(true);
                ok(window, cx);
                true
            })
            .on_close(move |_, window, cx| {
                if !closed.get() {
                    cancel(window, cx);
                }
            })
    });
}

// -- Page layouts -------------------------------------------------------------
//
// The window shell gives every page a definite height (the area under the
// title bar and banners). A page's root element is one of:
//
// - `scroll_page(id, page()...)` for forms: one vertical scroll region with
//   the page padding inside, content left-aligned in a column no wider than
//   `design::FORM_MAX_W`;
// - `fill_page(id)` for pages that own their regions (split views, lists
//   with pinned headers): it fills the area, and the page puts scroll
//   regions where it needs them, each with its own owner.
//
// A scroll region inside another one uses `ScrollTrap::scroll_trap`.

/// The content column of a form page: left-aligned groups, at most
/// `design::FORM_MAX_W` wide, `design::GROUP_GAP` apart. Put it in a
/// [`scroll_page`].
pub fn page() -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .max_w(design::FORM_MAX_W)
        .gap(design::GROUP_GAP)
}

/// A page that scrolls as a whole (settings forms): fills the page area,
/// scrolls vertically with the scrollbar at the window edge, and pads its
/// content by the page insets. Chain `.track_scroll(&handle)` to drive it.
pub fn scroll_page(id: impl Into<ElementId>, content: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .px(design::PAGE_X)
        .pt(design::PAGE_TOP)
        .pb(design::PAGE_BOTTOM)
        .child(content)
}

/// A page that owns its regions (split views, lists with pinned headers):
/// a column that fills the page area exactly. Children that scroll need
/// `flex_1().min_h_0()` (or a definite height) and their own scroll owner.
pub fn fill_page(id: impl Into<ElementId>) -> Stateful<Div> {
    v_flex()
        .id(id)
        .size_full()
        .min_h_0()
        .min_w_0()
        .overflow_hidden()
}

/// Keeps wheel scrolling inside an inner scroll region (a list in a
/// scrolling page): while the region can scroll, the wheel event stops
/// there instead of also scrolling the page around it.
pub trait ScrollTrap: Sized {
    /// Track `handle` on this scroll region and stop wheel events from
    /// reaching outer scroll regions while it has anything to scroll.
    fn scroll_trap(self, handle: &ScrollHandle) -> Self;
}

impl ScrollTrap for Stateful<Div> {
    fn scroll_trap(self, handle: &ScrollHandle) -> Self {
        let trap = handle.clone();
        // Mouse listeners run before the outer region's scroll listener
        // (`Interactivity::paint`), so stopping here keeps the page still.
        self.track_scroll(handle).on_scroll_wheel(move |_, _, cx| {
            if trap.max_offset().y > px(0.) {
                cx.stop_propagation();
            }
        })
    }
}

/// A short muted paragraph introducing a page or group (one or two
/// sentences; the title bar already names the page).
pub fn lede(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .max_w(design::PROSE_MAX_W)
        .text_size(design::TEXT_SECONDARY)
        .line_height(dp(15.))
        .text_color(design::tone(cx).text_muted)
        .child(text.into())
}

/// A settings slider bound to a numeric setting; writes on release.
pub struct SliderRow {
    key: &'static str,
    /// i18n keys, translated as it renders (so it follows the language).
    title: &'static str,
    description: Option<&'static str>,
    format: fn(f32) -> String,
    with_reset: bool,
    slider: Entity<gpui_kit::component::slider::SliderState>,
    dragging: bool,
    disabled_when: Option<fn(&guidy_core::settings::AppSettings) -> bool>,
    _subs: Vec<Subscription>,
}

impl SliderRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: &'static str,
        title: &'static str,
        description: Option<&'static str>,
        min: f32,
        max: f32,
        step: f32,
        format: fn(f32) -> String,
        with_reset: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        use gpui_kit::component::slider::{SliderEvent, SliderState};
        let state = crate::state::AppState::global(cx);
        let initial = current_f32(&state, key, cx).unwrap_or(min);
        let slider = cx.new(|_| {
            SliderState::new()
                .min(min)
                .max(max)
                .step(step)
                .default_value(initial)
        });
        let subs = vec![
            cx.subscribe(
                &slider,
                |this: &mut Self, _, ev: &SliderEvent, cx| match ev {
                    SliderEvent::Change(_) => {
                        this.dragging = true;
                        cx.notify();
                    }
                    SliderEvent::Release(v) => {
                        this.dragging = false;
                        let value = round_to_step(v.start(), this.slider.read(cx).step_value());
                        let key = this.key;
                        let json = if is_integer_setting(key) {
                            serde_json::json!(value.round() as u64)
                        } else {
                            serde_json::json!(value)
                        };
                        crate::state::AppState::global(cx)
                            .update(cx, |s, cx| s.update_setting(key, json, cx));
                    }
                },
            ),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        Self {
            key,
            title,
            description,
            format,
            with_reset,
            slider,
            dragging: false,
            disabled_when: None,
            _subs: subs,
        }
    }

    pub fn disabled_when(mut self, f: fn(&guidy_core::settings::AppSettings) -> bool) -> Self {
        self.disabled_when = Some(f);
        self
    }
}

fn current_f32(state: &Entity<crate::state::AppState>, key: &str, cx: &App) -> Option<f32> {
    state
        .read(cx)
        .settings
        .get(key)
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
}

fn is_integer_setting(key: &str) -> bool {
    key.ends_with("_ms")
}

pub fn round_to_step(v: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return v;
    }
    ((v / step).round() * step * 1000.0).round() / 1000.0
}

impl Render for SliderRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::slider::Slider;
        let state = crate::state::AppState::global(cx);
        if !self.dragging {
            if let Some(v) = current_f32(&state, self.key, cx) {
                if (self.slider.read(cx).value().start() - v).abs() > f32::EPSILON {
                    self.slider.update(cx, |s, cx| s.set_value(v, window, cx));
                }
            }
        }
        let st = crate::state::AppState::global(cx);
        let st = st.read(cx);
        let disabled = self
            .disabled_when
            .zip(st.app_settings())
            .map(|(f, s)| f(s))
            .unwrap_or(false);
        let updating = st.settings.is_updating(self.key);
        let value = self.slider.read(cx).value().start();
        let key = self.key;
        SettingRow::new(SharedString::from(format!("slider-{key}")), t(self.title))
            .when_some(self.description, |row, key| row.description(t(key)))
            .error(
                st.settings
                    .error_of(key)
                    .map(|e| t_args("settings.saveFailed", &[("error", e)])),
            )
            .disabled(disabled)
            .control(
                h_flex()
                    .gap_2()
                    .w(dp(240.))
                    // Before the control, so controls line up at the row's
                    // right edge with or without one.
                    .when(self.with_reset, |d| {
                        d.child(reset_button(
                            SharedString::from(format!("slider-reset-{key}")),
                            updating || disabled,
                            move |_, cx| {
                                crate::state::AppState::global(cx)
                                    .update(cx, |s, cx| s.reset_setting(key, cx))
                            },
                        ))
                    })
                    .child(
                        tracked_slider(Slider::new(&self.slider).disabled(disabled), cx).flex_1(),
                    )
                    .child(
                        div()
                            .w(dp(48.))
                            .text_right()
                            .text_size(design::TEXT_MONO)
                            .font_family(crate::theme::MONO_FONT)
                            .child((self.format)(value)),
                    ),
            )
    }
}

/// A horizontal slider with a visible unfilled track. gpui-component
/// paints the track as the fill color at 20% opacity (no style hook sets it
/// apart from the fill), which nearly vanishes on white with the yellow
/// accent; a neutral bar drawn behind it gives the track its contrast.
pub fn tracked_slider(slider: gpui_kit::component::slider::Slider, cx: &App) -> Div {
    let track = cx.theme().border;
    div()
        .relative()
        .min_w_0()
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .child(div().w_full().h(rems(0.375)).rounded_full().bg(track)),
        )
        .child(slider)
}

/// Translated model name (`onboarding.models.<id>.name`), else the catalog name.
pub fn model_name(model: &guidy_core::managers::model::ModelInfo) -> String {
    let key = format!("onboarding.models.{}.name", model.id);
    if guidy_i18n::exists(&key) {
        guidy_i18n::t(&key)
    } else {
        model.name.clone()
    }
}

/// Translated model description; custom models use a generic text.
pub fn model_description(model: &guidy_core::managers::model::ModelInfo) -> String {
    if model.is_custom {
        return guidy_i18n::t("onboarding.customModelDescription");
    }
    let key = format!("onboarding.models.{}.description", model.id);
    if guidy_i18n::exists(&key) {
        guidy_i18n::t(&key)
    } else {
        model.description.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::round_to_step;

    #[test]
    fn slider_values_snap_to_step() {
        assert_eq!(round_to_step(0.234, 0.01), 0.23);
        assert_eq!(round_to_step(612.0, 10.0), 610.0);
        assert_eq!(round_to_step(0.5, 0.0), 0.5);
    }
}
