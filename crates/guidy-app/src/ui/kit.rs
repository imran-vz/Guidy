//! Shared building blocks in Zed's visual language: settings rows and
//! groups, dropdowns, toggles, reset buttons, icons, translated strings.

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Icon, Sizable, StyledExt};
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

/// A Lucide icon by file name (e.g. `"mic"`).
pub fn icon(name: &str) -> Icon {
    Icon::empty().path(SharedString::from(format!("icons/{name}.svg")))
}

/// A spinning loader icon (Handy's `animate-spin`). `id` must be unique
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

/// Zed-style group: small muted heading over a bordered surface whose rows
/// are separated by hairlines.
pub fn group(title: Option<SharedString>, rows: Vec<AnyElement>, cx: &App) -> Div {
    let theme = cx.theme();
    let count = rows.len();
    v_flex()
        .w_full()
        .gap_1p5()
        .when_some(title, |this, title| {
            this.child(
                div()
                    .px_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child(title.to_uppercase()),
            )
        })
        .child(
            v_flex()
                .w_full()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .children(rows.into_iter().enumerate().map(move |(i, row)| {
                    div()
                        .w_full()
                        .when(i + 1 < count, |d| d.border_b_1().border_color(theme.border))
                        .child(row)
                })),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DescriptionMode {
    Inline,
    Tooltip,
}

/// One setting: title (+description) on the left, control on the right.
#[derive(IntoElement)]
pub struct SettingRow {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    mode: DescriptionMode,
    stacked: bool,
    disabled: bool,
    control: Option<AnyElement>,
}

impl SettingRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            mode: DescriptionMode::Tooltip,
            stacked: false,
            disabled: false,
            control: None,
        }
    }

    pub fn description(mut self, d: impl Into<SharedString>) -> Self {
        self.description = Some(d.into());
        self
    }

    pub fn inline_description(mut self) -> Self {
        self.mode = DescriptionMode::Inline;
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
        let muted = cx.theme().muted_foreground;
        let tooltip_text = self.description.clone();
        let title = h_flex()
            .gap_1()
            .child(div().text_sm().child(self.title.clone()))
            .when(
                self.mode == DescriptionMode::Tooltip && tooltip_text.is_some(),
                |this| {
                    let text = tooltip_text.clone().unwrap();
                    this.child(
                        div()
                            .id(SharedString::from(format!("{}-info", self.id)))
                            .cursor_pointer()
                            .child(icon("info").xsmall().text_color(muted))
                            .tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            }),
                    )
                },
            );
        let label = v_flex().gap_0p5().min_w_0().flex_1().child(title).when(
            self.mode == DescriptionMode::Inline,
            |this| {
                this.when_some(self.description.clone(), |this, d| {
                    this.child(div().text_xs().text_color(muted).child(d))
                })
            },
        );
        let control = self.control.unwrap_or_else(|| div().into_any_element());
        let row = if self.stacked {
            v_flex()
                .gap_2()
                .items_start()
                .child(label)
                .child(div().w_full().child(control))
        } else {
            h_flex()
                .gap_4()
                .justify_between()
                .child(label)
                .child(div().flex_none().child(control))
        };
        div()
            .id(self.id.clone())
            .aria_label(self.title.clone())
            .when_some(self.description, |d, desc| d.aria_description(desc))
            .w_full()
            .min_h(px(48.))
            .px_3()
            .py_2()
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

/// A select-like button with a popup menu (Handy's `Dropdown`).
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
            min_width: px(200.),
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
                let mut menu = menu.min_w(menu_width).max_h(px(240.)).scrollable(true);
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
                                            .text_xs()
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

/// Small ghost button that restores a default.
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

/// Page container: centered, max 768 px wide.
pub fn page() -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .max_w(px(768.))
        .mx_auto()
        .gap_6()
}

/// Muted section heading used at the top of pages.
pub fn page_header(title: SharedString, description: Option<SharedString>, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_lg().font_semibold().child(title))
        .when_some(description, |this, d| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(d),
            )
        })
}

/// Monospace path display with an Open button (Handy's `PathDisplay`).
pub fn path_display(
    id: impl Into<ElementId>,
    path: SharedString,
    enabled: bool,
    on_open: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    h_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px_2()
                .py_1()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .font_family(crate::theme::MONO_FONT)
                .text_xs()
                .truncate()
                .child(path),
        )
        .child(
            Button::new(id)
                .outline()
                .small()
                .label(t("common.open"))
                .disabled(!enabled)
                .on_click(move |_, window, cx| on_open(window, cx)),
        )
}

/// A settings slider bound to a numeric setting; writes on release.
pub struct SliderRow {
    key: &'static str,
    title: SharedString,
    description: SharedString,
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
        title: SharedString,
        description: SharedString,
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
        SettingRow::new(
            SharedString::from(format!("slider-{key}")),
            self.title.clone(),
        )
        .description(self.description.clone())
        .disabled(disabled)
        .control(
            h_flex()
                .gap_2()
                .w(px(260.))
                .child(
                    div()
                        .flex_1()
                        .child(Slider::new(&self.slider).disabled(disabled)),
                )
                .child(
                    div()
                        .w(px(56.))
                        .text_right()
                        .text_sm()
                        .font_family(crate::theme::MONO_FONT)
                        .child((self.format)(value)),
                )
                .when(self.with_reset, |d| {
                    d.child(reset_button(
                        SharedString::from(format!("slider-reset-{key}")),
                        updating || disabled,
                        move |_, cx| {
                            crate::state::AppState::global(cx)
                                .update(cx, |s, cx| s.reset_setting(key, cx))
                        },
                    ))
                }),
        )
    }
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
