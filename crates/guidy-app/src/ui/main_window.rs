//! The settings window (Handy's `App.tsx`): startup step machine, onboarding,
//! sidebar navigation, banners, status bar, and global toasts.

use crate::logic::flows::OnboardingStep;
use crate::logic::settings_store::ToastKind;
use crate::state::{AppEvent, AppState};
use crate::ui::banners::{AccessibilityBanner, SecureInputBanner};
use crate::ui::kit::{icon, t};
use crate::ui::onboarding::{
    migration::ModelMigration, model_picker::ModelPicker, permissions::PermissionsOnboarding,
};
use crate::ui::pages::{
    about::AboutPage, advanced::AdvancedPage, debug::DebugPage, general::GeneralPage,
    history::HistoryPage, models::ModelsPage, post_processing::PostProcessingPage,
};
use crate::ui::status_bar::StatusBar;
use crate::ui::whats_new::WhatsNewGate;
use crate::ui::{CloseWindow, ToggleDebugMode};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    General,
    History,
    Models,
    Advanced,
    PostProcessing,
    Debug,
    About,
}

impl Section {
    pub fn id(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::History => "history",
            Section::Models => "models",
            Section::Advanced => "advanced",
            Section::PostProcessing => "postprocessing",
            Section::Debug => "debug",
            Section::About => "about",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "general" => Section::General,
            "history" => Section::History,
            "models" => Section::Models,
            "advanced" => Section::Advanced,
            "postprocessing" => Section::PostProcessing,
            "debug" => Section::Debug,
            "about" => Section::About,
            _ => return None,
        })
    }

    fn label_key(self) -> &'static str {
        match self {
            Section::General => "sidebar.general",
            Section::History => "sidebar.history",
            Section::Models => "sidebar.models",
            Section::Advanced => "sidebar.advanced",
            Section::PostProcessing => "sidebar.postProcessing",
            Section::Debug => "sidebar.debug",
            Section::About => "sidebar.about",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => "mic",
            Section::History => "clock",
            Section::Models => "cpu",
            Section::Advanced => "cog",
            Section::PostProcessing => "sparkles",
            Section::Debug => "flask-conical",
            Section::About => "info",
        }
    }
}

/// The section survives the window being closed (GPUI destroys it; Handy's
/// webview was only hidden).
struct LastSection(Section);
impl Global for LastSection {}

#[derive(Default)]
struct Pages {
    general: Option<Entity<GeneralPage>>,
    history: Option<Entity<HistoryPage>>,
    models: Option<Entity<ModelsPage>>,
    advanced: Option<Entity<AdvancedPage>>,
    post_processing: Option<Entity<PostProcessingPage>>,
    debug: Option<Entity<DebugPage>>,
    about: Option<Entity<AboutPage>>,
}

pub struct MainWindow {
    state: Entity<AppState>,
    pub section: Section,
    /// Debug: render an onboarding step with no side effects.
    pub preview: Option<OnboardingStep>,
    pages: Pages,
    permissions: Option<Entity<PermissionsOnboarding>>,
    picker: Option<Entity<ModelPicker>>,
    migration: Option<Entity<ModelMigration>>,
    status_bar: Entity<StatusBar>,
    whats_new: Entity<WhatsNewGate>,
    accessibility_banner: Entity<AccessibilityBanner>,
    secure_banner: Entity<SecureInputBanner>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let mut subs = vec![cx.observe(&state, |this, _, cx| this.on_state_changed(cx))];
        subs.push(
            cx.subscribe_in(&state, window, |this, _, event: &AppEvent, window, cx| {
                if let AppEvent::Toast(spec) = event {
                    let kind = match spec.kind {
                        ToastKind::Error => NotificationType::Error,
                        ToastKind::Warning => NotificationType::Warning,
                        ToastKind::Success => NotificationType::Success,
                        ToastKind::Info => NotificationType::Info,
                    };
                    let mut n = Notification::new()
                        .title(spec.title.clone())
                        .with_type(kind);
                    if let Some(d) = &spec.description {
                        n = n.message(d.clone());
                    }
                    window.push_notification(n, cx);
                }
                if let AppEvent::LanguageChanged = event {
                    // Pages cache translated strings (slider titles, input
                    // placeholders): rebuild them in the new language.
                    this.pages = Pages::default();
                    cx.notify();
                }
            }),
        );
        subs.push(cx.observe_window_appearance(window, |_, window, cx| {
            let setting = AppState::global(cx)
                .read(cx)
                .app_settings()
                .map(|s| s.theme)
                .unwrap_or(guidy_core::settings::Theme::System);
            gpui_kit::component::Theme::change(
                crate::theme::mode_for(setting, window.appearance()),
                Some(window),
                cx,
            );
            crate::windows::sync_system_appearance(cx);
        }));

        let section = cx
            .try_global::<LastSection>()
            .map(|s| s.0)
            .unwrap_or(Section::General);
        #[cfg(debug_assertions)]
        let section = std::env::var("GUIDY_DEV_START_SECTION")
            .ok()
            .and_then(|s| Section::from_id(&s))
            .unwrap_or(section);
        let this = Self {
            state: state.clone(),
            section,
            preview: None,
            pages: Pages::default(),
            permissions: None,
            picker: None,
            migration: None,
            status_bar: StatusBar::global(cx),
            whats_new: cx.new(|cx| WhatsNewGate::new(window, cx)),
            accessibility_banner: cx.new(|cx| AccessibilityBanner::new(window, cx)),
            secure_banner: cx.new(SecureInputBanner::new),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            _subs: subs,
        };
        // Window-level shortcuts (debug toggle, close) need a focused node.
        window.focus(&this.focus, cx);
        // No-op once it has run for this session (normally from startup).
        state.update(cx, |s, cx| s.check_onboarding(cx));
        this
    }

    /// The startup step (`None` while the startup check runs).
    pub fn step(&self, cx: &App) -> Option<OnboardingStep> {
        self.state.read(cx).startup.step
    }

    fn on_state_changed(&mut self, cx: &mut Context<Self>) {
        // Keep the selected section valid when it becomes hidden.
        let visible = crate::logic::flows::sidebar_sections(self.state.read(cx).app_settings());
        if !visible.contains(&self.section.id()) {
            self.section = Section::General;
        }
        cx.notify();
    }

    pub fn set_step(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        // Deferred: callers may be inside an AppState callback.
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.set_step(step, cx)));
    }

    pub fn on_permissions_complete(&mut self, cx: &mut Context<Self>) {
        self.accessibility_banner
            .update(cx, |banner, cx| banner.refresh(cx));
        // Deferred: callers may be inside an AppState callback.
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.on_permissions_complete(cx)));
    }

    pub fn on_model_selected(&mut self, cx: &mut Context<Self>) {
        // Deferred: callers may be inside an AppState callback.
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.on_model_selected(cx)));
    }

    pub fn open_model_picker_from_migration(&mut self, cx: &mut Context<Self>) {
        self.set_step(OnboardingStep::Model, cx);
    }

    pub fn back_to_migration(&mut self, cx: &mut Context<Self>) {
        self.set_step(OnboardingStep::Migration, cx);
    }

    pub fn has_migration(&self, cx: &App) -> bool {
        self.state.read(cx).startup.migration_model_id.is_some()
    }

    pub fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            cx.set_global(LastSection(section));
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        cx.notify();
    }

    pub fn start_preview(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        self.preview = Some(step);
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let visible = crate::logic::flows::sidebar_sections(self.state.read(cx).app_settings());
        let (selected_bg, hover_bg, muted, border, panel) = (
            theme.sidebar_accent,
            theme.accent,
            theme.muted_foreground,
            theme.sidebar_border,
            theme.sidebar,
        );
        v_flex()
            .id("sidebar")
            .w(px(168.))
            .h_full()
            .flex_none()
            .bg(panel)
            .border_r_1()
            .border_color(border)
            .p_1p5()
            .gap_0p5()
            .children(
                visible
                    .into_iter()
                    .filter_map(Section::from_id)
                    .map(|section| {
                        let active = section == self.section;
                        h_flex()
                            .id(SharedString::from(format!("nav-{}", section.id())))
                            .aria_label(t(section.label_key()))
                            .gap_2()
                            .px_2()
                            .h(px(28.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_sm()
                            .when(active, |d| d.bg(selected_bg))
                            .when(!active, |d| {
                                d.text_color(muted).hover(move |s| s.bg(hover_bg))
                            })
                            .child(icon(section.icon()).small())
                            .child(div().truncate().child(t(section.label_key())))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select_section(section, cx)),
                            )
                            .test_support()
                    }),
            )
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        macro_rules! page {
            ($field:ident, $ty:ty) => {{
                let entity = self
                    .pages
                    .$field
                    .get_or_insert_with(|| cx.new(|cx| <$ty>::new(window, cx)))
                    .clone();
                entity.into_any_element()
            }};
        }
        match self.section {
            Section::General => page!(general, GeneralPage),
            Section::History => page!(history, HistoryPage),
            Section::Models => page!(models, ModelsPage),
            Section::Advanced => page!(advanced, AdvancedPage),
            Section::PostProcessing => page!(post_processing, PostProcessingPage),
            Section::Debug => page!(debug, DebugPage),
            Section::About => page!(about, AboutPage),
        }
    }

    fn render_onboarding(
        &mut self,
        step: OnboardingStep,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let main = cx.entity().downgrade();
        match step {
            OnboardingStep::Accessibility => self
                .permissions
                .get_or_insert_with(|| {
                    cx.new(|cx| PermissionsOnboarding::new(main, preview, window, cx))
                })
                .clone()
                .into_any_element(),
            OnboardingStep::Model => self
                .picker
                .get_or_insert_with(|| cx.new(|cx| ModelPicker::new(main, preview, window, cx)))
                .clone()
                .into_any_element(),
            OnboardingStep::Migration => self
                .migration
                .get_or_insert_with(|| cx.new(|cx| ModelMigration::new(main, window, cx)))
                .clone()
                .into_any_element(),
            OnboardingStep::Done => div().into_any_element(),
        }
    }

    fn toggle_debug_mode(&mut self, _: &ToggleDebugMode, _: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .state
            .read(cx)
            .app_settings()
            .map(|s| s.debug_mode)
            .unwrap_or(false);
        self.state.update(cx, |s, cx| {
            s.update_setting("debug_mode", serde_json::json!(!current), cx)
        });
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, muted) = (theme.background, theme.foreground, theme.muted_foreground);
        let rtl = guidy_i18n::is_rtl(&guidy_i18n::language());

        let title_bar = TitleBar::new().child(
            h_flex()
                .gap_1p5()
                .text_sm()
                .text_color(muted)
                .child(icon("audio-lines").small())
                .child(guidy_core::host::PRODUCT_NAME),
        );

        let content: AnyElement = if let Some(step) = self.preview {
            let body = self.render_onboarding(step, true, window, cx);
            div()
                .size_full()
                .relative()
                .child(body)
                .child(
                    div().absolute().top_2().right_2().child(
                        Button::new("exit-preview")
                            .outline()
                            .small()
                            .label(t("settings.debug.onboardingPreview.exitButton"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview = None;
                                this.permissions = None;
                                this.picker = None;
                                cx.notify();
                            })),
                    ),
                )
                .into_any_element()
        } else {
            match self.step(cx) {
                None => div().size_full().into_any_element(),
                Some(OnboardingStep::Done) => {
                    let page = self.render_page(window, cx);
                    h_flex()
                        .size_full()
                        .min_h_0()
                        .when(rtl, |d| d.flex_row_reverse())
                        .child(self.render_sidebar(cx))
                        .child(
                            div()
                                .id("page-scroll")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .child(
                                    v_flex()
                                        .w_full()
                                        .min_w_0()
                                        .p_4()
                                        .gap_4()
                                        .items_center()
                                        .child(self.accessibility_banner.clone())
                                        .child(self.secure_banner.clone())
                                        .child(page),
                                ),
                        )
                        .into_any_element()
                }
                Some(step) => self.render_onboarding(step, false, window, cx),
            }
        };

        v_flex()
            .id("main-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(bg)
            .text_color(fg)
            .font_family(crate::theme::UI_FONT)
            .on_action(cx.listener(Self::toggle_debug_mode))
            .on_action(|_: &CloseWindow, window, cx| {
                // Same as the close button: hide into the tray.
                crate::windows::on_main_closed(cx);
                window.remove_window();
            })
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(content))
            .when(
                self.step(cx) == Some(OnboardingStep::Done) && self.preview.is_none(),
                |d| {
                    d.child(self.status_bar.clone())
                        .child(self.whats_new.clone())
                },
            )
    }
}
