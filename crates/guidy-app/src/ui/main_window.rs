//! The settings window: startup step machine, onboarding,
//! sidebar navigation, banners, status bar, and global toasts.

use crate::logic::flows::OnboardingStep;
use crate::logic::settings_store::ToastKind;
use crate::state::{AppEvent, AppState};
use crate::ui::banners::{AccessibilityBanner, SecureInputBanner};
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{icon, t};
use crate::ui::onboarding::{
    migration::ModelMigration, model_picker::ModelPicker, permissions::PermissionsOnboarding,
};
use crate::ui::pages::{
    about::AboutPage, advanced::AdvancedPage, debug::DebugPage, general::GeneralPage,
    history::HistoryPage, meetings::MeetingsPage, models::ModelsPage,
    post_processing::PostProcessingPage, usage::UsagePage,
};
use crate::ui::status_bar::StatusBar;
use crate::ui::whats_new::WhatsNewGate;
use crate::ui::{CloseWindow, ToggleDebugMode};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{h_flex, v_flex, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    General,
    History,
    Meetings,
    Models,
    Advanced,
    PostProcessing,
    Usage,
    Debug,
    About,
}

impl Section {
    pub fn id(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::History => "history",
            Section::Meetings => "meetings",
            Section::Models => "models",
            Section::Advanced => "advanced",
            Section::PostProcessing => "postprocessing",
            Section::Usage => "usage",
            Section::Debug => "debug",
            Section::About => "about",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "general" => Section::General,
            "history" => Section::History,
            "meetings" => Section::Meetings,
            "models" => Section::Models,
            "advanced" => Section::Advanced,
            "postprocessing" => Section::PostProcessing,
            "usage" => Section::Usage,
            "debug" => Section::Debug,
            "about" => Section::About,
            _ => return None,
        })
    }

    fn label_key(self) -> &'static str {
        match self {
            Section::General => "sidebar.general",
            Section::History => "sidebar.history",
            Section::Meetings => "meetings.title",
            Section::Models => "sidebar.models",
            Section::Advanced => "sidebar.advanced",
            Section::PostProcessing => "sidebar.postProcessing",
            Section::Usage => "sidebar.usage",
            Section::Debug => "sidebar.debug",
            Section::About => "sidebar.about",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => "mic",
            Section::History => "clock",
            Section::Meetings => "notebook-pen",
            Section::Models => "cpu",
            Section::Advanced => "cog",
            Section::PostProcessing => "sparkles",
            Section::Usage => "coins",
            Section::Debug => "flask-conical",
            Section::About => "info",
        }
    }
}

/// The section survives the window being closed (GPUI destroys the window
/// rather than hiding it).
struct LastSection(Section);
impl Global for LastSection {}

/// The page well's scroll, for pages that scroll to something of theirs
/// (the meeting transcript, from its talk-time chart).
pub struct PageScroll(pub ScrollHandle);
impl Global for PageScroll {}

#[derive(Default)]
struct Pages {
    general: Option<Entity<GeneralPage>>,
    history: Option<Entity<HistoryPage>>,
    meetings: Option<Entity<MeetingsPage>>,
    models: Option<Entity<ModelsPage>>,
    advanced: Option<Entity<AdvancedPage>>,
    post_processing: Option<Entity<PostProcessingPage>>,
    usage: Option<Entity<UsagePage>>,
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
    /// A `guidy://meetings/<id>` link to open once the Meetings page renders.
    pending_meeting: Option<crate::logic::meetings::MeetingLink>,
    /// Show the Meetings > Settings tab once the Meetings page renders.
    pending_meetings_settings: bool,
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
            pending_meeting: None,
            pending_meetings_settings: false,
            _subs: subs,
        };
        cx.set_global(PageScroll(this.scroll.clone()));
        // Window-level shortcuts (debug toggle, close) need a focused node.
        window.focus(&this.focus, cx);
        // No-op once it has run for this session (normally from startup).
        state.update(cx, |s, cx| s.check_onboarding(cx));
        this
    }

    /// Sidebar sections in order; Meetings only where supported.
    fn visible_sections(&self, cx: &App) -> Vec<&'static str> {
        let state = self.state.read(cx);
        crate::logic::meetings::with_meetings_section(
            crate::logic::flows::sidebar_sections(state.app_settings()),
            state.meetings_supported,
        )
    }

    /// The startup step (`None` while the startup check runs).
    pub fn step(&self, cx: &App) -> Option<OnboardingStep> {
        self.state.read(cx).startup.step
    }

    fn on_state_changed(&mut self, cx: &mut Context<Self>) {
        // Keep the selected section valid when it becomes hidden.
        let visible = self.visible_sections(cx);
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
        // Spend changes with every cloud call: show the latest on each visit.
        if section == Section::Usage {
            if let Some(page) = &self.pages.usage {
                page.update(cx, |p, cx| p.reload(cx));
            }
        }
        cx.notify();
    }

    /// Deep link: show the Meetings section and open the meeting (seeking
    /// to the link's time). Ignored where meeting notes aren't available.
    pub fn open_meeting_link(
        &mut self,
        link: crate::logic::meetings::MeetingLink,
        cx: &mut Context<Self>,
    ) {
        if !self.visible_sections(cx).contains(&Section::Meetings.id()) {
            log::info!("Ignoring a meeting link: meeting notes are unavailable");
            return;
        }
        self.pending_meeting = Some(link);
        self.select_section(Section::Meetings, cx);
    }

    /// Meetings > Settings (built-in LLM model downloads live there).
    pub fn open_meetings_settings(&mut self, cx: &mut Context<Self>) {
        if !self.visible_sections(cx).contains(&Section::Meetings.id()) {
            return;
        }
        self.pending_meetings_settings = true;
        self.select_section(Section::Meetings, cx);
    }

    pub fn start_preview(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        self.preview = Some(step);
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let visible = self.visible_sections(cx);
        v_flex()
            .id("sidebar")
            .w(dp(176.))
            .h_full()
            .flex_none()
            .bg(tone.frame)
            .px(dp(8.))
            .pt(dp(4.))
            .gap(dp(2.))
            .children(
                visible
                    .into_iter()
                    .filter_map(Section::from_id)
                    .map(|section| {
                        let active = section == self.section;
                        h_flex()
                            .id(SharedString::from(format!("nav-{}", section.id())))
                            .aria_label(t(section.label_key()))
                            .relative()
                            .gap(dp(10.))
                            .px(dp(10.))
                            .h(dp(32.))
                            .rounded(dp(9.))
                            .cursor_pointer()
                            .text_size(design::TEXT_BODY)
                            .when(active, |d| {
                                d.bg(tone.raised)
                                    .text_color(tone.text)
                                    .shadow_xs()
                                    .child(design::rim_light(cx))
                            })
                            .when(!active, |d| {
                                d.text_color(tone.text_muted).hover(move |s| {
                                    s.bg(tone.raised.opacity(0.55)).text_color(tone.text)
                                })
                            })
                            .child(
                                icon(section.icon())
                                    .small()
                                    .when(active, |i| i.text_color(tone.accent_text)),
                            )
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
            Section::Meetings => {
                let page = self
                    .pages
                    .meetings
                    .get_or_insert_with(|| cx.new(|cx| MeetingsPage::new(window, cx)))
                    .clone();
                if std::mem::take(&mut self.pending_meetings_settings) {
                    page.update(cx, |p, cx| {
                        p.close_meeting(cx);
                        p.set_tab(crate::ui::pages::meetings::MeetingsTab::Settings, cx)
                    });
                }
                if let Some(link) = self.pending_meeting.take() {
                    page.update(cx, |p, cx| {
                        p.open_meeting_at(link.meeting_id, link.t_ms, window, cx)
                    });
                }
                page.into_any_element()
            }
            Section::Models => page!(models, ModelsPage),
            Section::Advanced => page!(advanced, AdvancedPage),
            Section::PostProcessing => page!(post_processing, PostProcessingPage),
            Section::Usage => page!(usage, UsagePage),
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
        let tone = design::tone(cx);
        let rtl = guidy_i18n::is_rtl(&guidy_i18n::language());
        let in_app = self.step(cx) == Some(OnboardingStep::Done) && self.preview.is_none();

        // "Guidy / General": the product, then where you are.
        let title_bar = TitleBar::new()
            .bg(tone.frame)
            .border_color(tone.frame)
            .child(
                h_flex()
                    .gap(dp(8.))
                    .items_center()
                    .text_size(design::TEXT_BODY)
                    .child(icon("audio-lines").small().text_color(tone.accent_text))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tone.text)
                            .child(guidy_core::host::PRODUCT_NAME),
                    )
                    .when(in_app, |d| {
                        d.child(div().text_color(tone.text_faint).child("/")).child(
                            div()
                                .text_color(tone.text_muted)
                                .child(t(self.section.label_key())),
                        )
                    }),
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
                            .secondary()
                            .pill()
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
                    // Pages sit in an ink well inset into the window frame.
                    let well = div()
                        .id("page-scroll")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .when(rtl, |d| d.ml(design::WELL_INSET))
                        .when(!rtl, |d| d.mr(design::WELL_INSET))
                        .rounded(design::RADIUS_WELL)
                        .border_1()
                        .border_color(tone.hairline)
                        .bg(tone.ink)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            v_flex()
                                .w_full()
                                .min_w_0()
                                .px(dp(20.))
                                .pt(dp(20.))
                                .pb(dp(28.))
                                .items_center()
                                .child(self.accessibility_banner.clone())
                                .child(self.secure_banner.clone())
                                .child(page),
                        );
                    h_flex()
                        .size_full()
                        .min_h_0()
                        .bg(tone.frame)
                        .when(rtl, |d| d.flex_row_reverse())
                        .child(self.render_sidebar(cx))
                        .child(well)
                        .into_any_element()
                }
                Some(step) => self.render_onboarding(step, false, window, cx),
            }
        };

        v_flex()
            .id("main-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(if in_app { tone.frame } else { tone.ink })
            .text_color(tone.text)
            .font_family(crate::theme::UI_FONT)
            .on_action(cx.listener(Self::toggle_debug_mode))
            .on_action(|_: &CloseWindow, window, cx| {
                // Same as the close button: hide into the tray.
                crate::windows::on_main_closed(cx);
                window.remove_window();
            })
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(content))
            .when(in_app, |d| {
                d.child(self.status_bar.clone())
                    .child(self.whats_new.clone())
            })
    }
}
