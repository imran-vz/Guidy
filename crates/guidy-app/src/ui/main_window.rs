//! The settings window: startup step machine, onboarding, and the window
//! shell around the pages: a full-height source-list sidebar (the traffic
//! lights sit over its top), a unified title bar over the content (page
//! title left, page actions right), pinned banners, the page area, the
//! status bar, and global toasts.

use crate::logic::flows::OnboardingStep;
use crate::logic::settings_store::ToastKind;
use crate::state::CoreStatus;
use crate::state::{AppEvent, AppState};
use crate::ui::banners::{AccessibilityBanner, SecureInputBanner};
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use crate::ui::onboarding::{
    migration::ModelMigration, permissions::PermissionsOnboarding, setup::SetupStep, startup,
    welcome::Welcome,
};
use crate::ui::pages::Page;
use crate::ui::pages::{
    about::AboutPage,
    advanced::AdvancedPage,
    debug::DebugPage,
    general::GeneralPage,
    history::HistoryPage,
    meetings::{settings::SettingsCategory, MeetingsPage, MeetingsTab},
    models::ModelsPage,
    post_processing::PostProcessingPage,
    storage::StoragePage,
    usage::UsagePage,
};
use crate::ui::status_bar::StatusBar;
use crate::ui::whats_new::WhatsNewGate;
use crate::ui::{CloseWindow, SelectNextSection, SelectPreviousSection, ToggleDebugMode};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, TitleBar, WindowExt};
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
    Storage,
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
            Section::Storage => "storage",
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
            "storage" => Section::Storage,
            "debug" => Section::Debug,
            "about" => Section::About,
            _ => return None,
        })
    }

    /// The translation key of the section's name.
    pub fn label_key(self) -> &'static str {
        match self {
            Section::General => "sidebar.general",
            Section::History => "sidebar.history",
            Section::Meetings => "meetings.title",
            Section::Models => "sidebar.models",
            Section::Advanced => "sidebar.advanced",
            Section::PostProcessing => "sidebar.postProcessing",
            Section::Usage => "sidebar.usage",
            Section::Storage => "sidebar.storage",
            Section::Debug => "sidebar.debug",
            Section::About => "sidebar.about",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Section::General => "settings",
            Section::History => "rotate-ccw-clock",
            Section::Meetings => "notebook-pen",
            Section::Models => "cpu",
            Section::Advanced => "sliders-horizontal",
            Section::PostProcessing => "wand-sparkles",
            Section::Usage => "chart-column",
            Section::Storage => "hard-drive",
            Section::Debug => "bug",
            Section::About => "info",
        }
    }

    /// The sidebar group the section sits in: a gap separates groups (app,
    /// configuration, data, help), like System Settings. Order is unchanged.
    pub(crate) fn group(self) -> u8 {
        match self {
            Section::General | Section::History | Section::Meetings => 0,
            Section::Models | Section::Advanced | Section::PostProcessing => 1,
            Section::Usage | Section::Storage => 2,
            Section::Debug | Section::About => 3,
        }
    }
}

/// A sidebar row, in visual order: a section, one of the Meetings page's
/// other views (People, Meeting settings) indented under Meetings, or,
/// while Meeting settings is shown, one of its categories indented under
/// it (as System Settings lists a pane's parts). The views are not
/// sections: the window, ⌘ shortcuts and the View menu still know only
/// Meetings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavItem {
    Section(Section),
    MeetingsView(MeetingsTab),
    SettingsCategory(SettingsCategory),
}

impl NavItem {
    fn id(self) -> SharedString {
        match self {
            NavItem::Section(s) => format!("nav-{}", s.id()).into(),
            NavItem::MeetingsView(tab) => format!("nav-meetings-{}", tab.id()).into(),
            // The ids the settings' own category list had.
            NavItem::SettingsCategory(c) => format!("meetings-settings-category-{}", c.id()).into(),
        }
    }

    fn label(self) -> SharedString {
        match self {
            NavItem::Section(s) => t(s.label_key()),
            NavItem::MeetingsView(tab) => t(tab.label_key()),
            NavItem::SettingsCategory(c) => c.label(),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            NavItem::Section(s) => s.icon(),
            NavItem::MeetingsView(MeetingsTab::People) => "users-round",
            NavItem::MeetingsView(_) => "settings-2",
            NavItem::SettingsCategory(c) => c.icon(),
        }
    }

    /// How deep the row sits: sections 0, the Meetings views 1, the
    /// settings categories 2.
    /// Two levels only: Meeting settings is a top-level row in the
    /// settings cluster (its categories one level under it); People stays
    /// under Meetings.
    fn level(self) -> u8 {
        match self {
            NavItem::Section(_) => 0,
            NavItem::MeetingsView(MeetingsTab::Settings) => 0,
            NavItem::MeetingsView(_) => 1,
            NavItem::SettingsCategory(_) => 1,
        }
    }
}

/// The section survives the window being closed (GPUI destroys the window
/// rather than hiding it); `windows` also saves it across launches.
pub struct LastSection(pub Section);
impl Global for LastSection {}

/// The macOS traffic lights' width plus margins: space kept clear at the
/// left of the title bar (they are not drawn in offscreen snapshots).
const TRAFFIC_LIGHTS_W: Pixels = dp(78.);

/// Below this window width the sidebar narrows so the content keeps its
/// room (the minimum window is 720 pt).
const COMPACT_SIDEBAR_BELOW: Pixels = dp(760.);
/// The narrowed sidebar: same 16 pt glyphs and 8 pt gap, less slack
/// (wide enough for "Meeting settings" with a shallower indent).
const SIDEBAR_W_COMPACT: Pixels = dp(172.);

/// The sidebar is in its narrow width.
fn compact_sidebar(window: &Window) -> bool {
    window.viewport_size().width < COMPACT_SIDEBAR_BELOW
}

/// The sidebar's width (and the title bar's sidebar-coloured area) at this
/// window size. Pages that size themselves from the content width
/// subtract this.
pub fn sidebar_width(window: &Window) -> Pixels {
    if compact_sidebar(window) {
        SIDEBAR_W_COMPACT
    } else {
        design::SIDEBAR_W
    }
}

/// A page as the shell shows it: body plus title-bar content.
struct ShownPage {
    body: AnyElement,
    title: Option<SharedString>,
    leading: Option<AnyElement>,
    toolbar: Option<AnyElement>,
}

fn show<P: Page>(page: &Entity<P>, window: &mut Window, cx: &mut App) -> ShownPage {
    let (title, leading, toolbar) = page.update(cx, |p, cx| {
        (p.title(cx), p.leading(window, cx), p.toolbar(window, cx))
    });
    ShownPage {
        body: page.clone().into_any_element(),
        title,
        leading,
        toolbar,
    }
}

#[derive(Default)]
struct Pages {
    general: Option<Entity<GeneralPage>>,
    history: Option<Entity<HistoryPage>>,
    meetings: Option<Entity<MeetingsPage>>,
    models: Option<Entity<ModelsPage>>,
    advanced: Option<Entity<AdvancedPage>>,
    post_processing: Option<Entity<PostProcessingPage>>,
    usage: Option<Entity<UsagePage>>,
    storage: Option<Entity<StoragePage>>,
    debug: Option<Entity<DebugPage>>,
    about: Option<Entity<AboutPage>>,
}

pub struct MainWindow {
    state: Entity<AppState>,
    pub section: Section,
    /// Debug: render an onboarding step with no side effects.
    pub preview: Option<OnboardingStep>,
    pages: Pages,
    /// The onboarding screen shown and its step (and whether a preview):
    /// rebuilt when the step changes, so Back starts a screen afresh.
    onboarding: Option<(OnboardingStep, bool, AnyView)>,
    status_bar: Entity<StatusBar>,
    whats_new: Entity<WhatsNewGate>,
    accessibility_banner: Entity<AccessibilityBanner>,
    secure_banner: Entity<SecureInputBanner>,
    focus: FocusHandle,
    /// The sidebar list (Tab stop; ↑/↓ move the selection).
    sidebar_focus: FocusHandle,
    /// The sidebar's scroll: with Meeting settings' categories listed it
    /// can be taller than a small window.
    sidebar_scroll: ScrollHandle,
    /// Scroll the selected sidebar row into view on the next render (after
    /// ↑/↓).
    reveal_selection: bool,
    /// A `guidy://meetings/<id>` link to open once the Meetings page renders.
    pending_meeting: Option<crate::logic::meetings::MeetingLink>,
    /// Show Meetings › Settings at this category once the Meetings page
    /// renders (deep links: AI; a category row picked before the page).
    pending_meetings_category: Option<SettingsCategory>,
    /// The Meetings view a sidebar row picked before the page existed.
    pending_meetings_tab: Option<MeetingsTab>,
    _subs: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let mut subs = vec![cx.observe(&state, |this, _, cx| this.on_state_changed(cx))];
        subs.push(
            cx.subscribe_in(&state, window, |_, _, event: &AppEvent, window, cx| {
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
                if let AppEvent::ModelReady { id, name } = event {
                    window.push_notification(
                        crate::ui::download_control::model_ready_notification(id, name),
                        cx,
                    );
                }
                if let AppEvent::LanguageChanged = event {
                    // Pages translate as they render, and text field
                    // placeholders follow the language on their own
                    // (`kit::LocalizedPlaceholder`): the pages, and drafts
                    // in them, stay.
                    cx.notify();
                }
            }),
        );
        subs.push(cx.observe_window_appearance(window, |_, window, cx| {
            let setting = AppState::global(cx)
                .read(cx)
                .app_settings()
                .map(|s| s.theme)
                .unwrap_or_else(|| crate::theme::applied_setting(cx));
            gpui_kit::component::Theme::change(
                crate::theme::mode_for(setting, window.appearance()),
                Some(window),
                cx,
            );
            crate::windows::sync_system_appearance(cx);
        }));

        subs.push(cx.observe_window_bounds(window, |_, window, cx| {
            crate::window_state::record(window, cx)
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
            onboarding: None,
            status_bar: StatusBar::global(cx),
            whats_new: cx.new(|cx| WhatsNewGate::new(window, cx)),
            accessibility_banner: cx.new(|cx| AccessibilityBanner::new(window, cx)),
            secure_banner: cx.new(SecureInputBanner::new),
            focus: cx.focus_handle(),
            sidebar_focus: cx.focus_handle().tab_stop(true),
            sidebar_scroll: ScrollHandle::new(),
            reveal_selection: false,
            pending_meeting: None,
            pending_meetings_category: None,
            pending_meetings_tab: None,
            _subs: subs,
        };
        crate::ui::sync_app_menus(cx);
        // Window-level shortcuts (debug toggle, close) need a focused node.
        window.focus(&this.focus, cx);
        // No-op once it has run for this session (normally from startup).
        state.update(cx, |s, cx| s.check_onboarding(cx));
        this
    }

    /// Sidebar sections in order; Meetings only where supported.
    fn visible_sections(&self, cx: &App) -> Vec<&'static str> {
        crate::ui::visible_sections(cx)
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
        // The View menu lists the visible sections.
        crate::ui::sync_app_menus(cx);
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

    /// The onboarding screen shown, if it is a `T` (tests drive it).
    pub fn onboarding_view<T: 'static>(&self) -> Option<Entity<T>> {
        self.onboarding
            .as_ref()
            .and_then(|(_, _, view)| view.clone().downcast::<T>().ok())
    }

    pub fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            cx.set_global(LastSection(section));
        }
        // Spend changes with every cloud call: show the latest on each visit.
        if section == Section::Usage {
            if let Some(page) = &self.pages.usage {
                page.update(cx, |p, cx| p.reload(cx));
            }
        }
        // So do files: measure again on each visit, with no timers.
        if section == Section::Storage {
            if let Some(page) = &self.pages.storage {
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

    /// Meetings › Settings › AI (built-in LLM model downloads live there).
    pub fn open_meetings_settings(&mut self, cx: &mut Context<Self>) {
        if !self.visible_sections(cx).contains(&Section::Meetings.id()) {
            return;
        }
        self.pending_meetings_category = Some(SettingsCategory::Ai);
        self.select_section(Section::Meetings, cx);
    }

    /// The Meetings page, once it has been shown.
    #[cfg(test)]
    pub fn meetings_page(&self) -> Option<Entity<MeetingsPage>> {
        self.pages.meetings.clone()
    }

    /// The sidebar's rows in visual order: the visible sections, with the
    /// Meetings views under Meetings and, while Meeting settings is shown,
    /// its categories under it.
    fn nav_items(&self, cx: &App) -> Vec<NavItem> {
        let settings_shown = self.settings_shown(cx);
        let sections: Vec<Section> = self
            .visible_sections(cx)
            .into_iter()
            .filter_map(Section::from_id)
            .collect();
        let meetings = sections.contains(&Section::Meetings);
        // Meeting settings closes the settings cluster (Models, Advanced,
        // Post Process, Meeting settings).
        let last_setting = sections
            .iter()
            .rposition(|s| s.group() == Section::Models.group());
        let mut items = Vec::new();
        for (i, section) in sections.iter().copied().enumerate() {
            items.push(NavItem::Section(section));
            if section == Section::Meetings {
                items.push(NavItem::MeetingsView(MeetingsTab::People));
            }
            if meetings && Some(i) == last_setting {
                items.push(NavItem::MeetingsView(MeetingsTab::Settings));
                if settings_shown {
                    items.extend(SettingsCategory::ALL.map(NavItem::SettingsCategory));
                }
            }
        }
        items
    }

    /// Meeting settings is the view shown (its categories are listed).
    fn settings_shown(&self, cx: &App) -> bool {
        self.section == Section::Meetings && self.meetings_tab(cx) == MeetingsTab::Settings
    }

    /// The meeting settings category shown (or about to be).
    fn settings_category(&self, cx: &App) -> SettingsCategory {
        self.pending_meetings_category.unwrap_or_else(|| {
            self.pages
                .meetings
                .as_ref()
                .map_or(SettingsCategory::General, |p| {
                    p.read(cx).settings_view().read(cx).category()
                })
        })
    }

    /// The Meetings view shown (or about to be, once the page renders).
    fn meetings_tab(&self, cx: &App) -> MeetingsTab {
        if self.pending_meeting.is_some() {
            MeetingsTab::Meetings
        } else if self.pending_meetings_category.is_some() {
            MeetingsTab::Settings
        } else if let Some(tab) = self.pending_meetings_tab {
            tab
        } else {
            self.pages
                .meetings
                .as_ref()
                .map_or(MeetingsTab::Meetings, |p| p.read(cx).tab)
        }
    }

    /// The sidebar row of what is shown.
    fn current_nav(&self, cx: &App) -> NavItem {
        if self.section != Section::Meetings {
            return NavItem::Section(self.section);
        }
        match self.meetings_tab(cx) {
            MeetingsTab::Meetings => NavItem::Section(Section::Meetings),
            // Meeting settings always shows one of its categories: that
            // row is the selected one.
            MeetingsTab::Settings => NavItem::SettingsCategory(self.settings_category(cx)),
            tab => NavItem::MeetingsView(tab),
        }
    }

    /// Show a sidebar row: a section, or a Meetings view (Meetings itself
    /// shows the meeting list).
    fn select_nav(&mut self, item: NavItem, cx: &mut Context<Self>) {
        if let NavItem::SettingsCategory(category) = item {
            // Exactly what the settings' category list did.
            match &self.pages.meetings {
                Some(page) => page.update(cx, |p, cx| p.show_settings(category, cx)),
                None => self.pending_meetings_category = Some(category),
            }
        } else {
            let tab = match item {
                NavItem::Section(Section::Meetings) => Some(MeetingsTab::Meetings),
                NavItem::MeetingsView(tab) => Some(tab),
                _ => None,
            };
            if let Some(tab) = tab {
                match &self.pages.meetings {
                    Some(page) => page.update(cx, |p, cx| p.set_tab(tab, cx)),
                    None => self.pending_meetings_tab = Some(tab),
                }
            }
        }
        let section = match item {
            NavItem::Section(s) => s,
            NavItem::MeetingsView(_) | NavItem::SettingsCategory(_) => Section::Meetings,
        };
        self.select_section(section, cx);
    }

    /// Select the sidebar row `step` places above (-1) or below (1) the
    /// current one (in visual order, the Meetings views and the settings
    /// categories included), stopping at the ends. While its categories
    /// are listed, Meeting settings is their heading and the keys pass
    /// over it; arriving at it (collapsed) lands on the category next in
    /// visual order: the first from above, the last from below.
    fn step_section(&mut self, step: isize, cx: &mut Context<Self>) {
        let mut items = self.nav_items(cx);
        if self.settings_shown(cx) {
            items.retain(|item| *item != NavItem::MeetingsView(MeetingsTab::Settings));
        }
        let current = self.current_nav(cx);
        let Some(at) = items.iter().position(|item| *item == current) else {
            return;
        };
        let next = at as isize + step;
        if let Some(item) = usize::try_from(next).ok().and_then(|i| items.get(i)) {
            let item = match *item {
                NavItem::MeetingsView(MeetingsTab::Settings) => {
                    NavItem::SettingsCategory(if step > 0 {
                        SettingsCategory::ALL[0]
                    } else {
                        SettingsCategory::ALL[SettingsCategory::ALL.len() - 1]
                    })
                }
                item => item,
            };
            self.select_nav(item, cx);
            // Keep the newly selected row in view (the list can scroll).
            self.reveal_selection = true;
        }
    }

    /// Show `section` if the sidebar lists it (⌘1…⌘9, the View menu).
    pub fn show_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.step(cx) == Some(OnboardingStep::Done)
            && self.preview.is_none()
            && self.visible_sections(cx).contains(&section.id())
        {
            self.select_section(section, cx);
        }
    }

    pub fn start_preview(&mut self, step: OnboardingStep, cx: &mut Context<Self>) {
        self.preview = Some(step);
        cx.notify();
    }

    /// The source list: one row per visible section, with the Meetings
    /// views (People, Meeting settings) indented under Meetings and, while
    /// Meeting settings is shown, its categories one level deeper.
    /// Selection is a neutral fill, ringed while the list has keyboard
    /// focus.
    fn render_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let compact = compact_sidebar(window);
        let focused = self.sidebar_focus.is_focused(window);
        let selection = if focused {
            tone.selection_focused
        } else {
            tone.selection
        };
        let ring = cx.theme().ring;
        let warning = design::status_color(design::Status::Attention, cx);
        let current = self.current_nav(cx);
        let setup_issues = self
            .pages
            .meetings
            .as_ref()
            .map_or(0, |p| p.read(cx).settings_view().read(cx).setup_issues(cx));
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut active_ix = None;
        let mut last_group = None;
        let items = self.nav_items(cx);
        // One set of metrics (26 pt rows, 24 pt categories, 8 pt cluster
        // gaps, no row gaps) whether or not Meeting settings' categories
        // are listed: the global destinations stay in view at 900×600 for a
        // typical sidebar (a debug build with Post Process on scrolls).
        for item in items {
            let group = match item {
                NavItem::Section(section) => section.group(),
                NavItem::MeetingsView(MeetingsTab::People) => Section::Meetings.group(),
                _ => Section::Models.group(),
            };
            if last_group.is_some_and(|g| g != group) {
                // One set of metrics in both states, so rows never shift as
                // the categories open and close.
                let gap = dp(8.);
                rows.push(div().flex_none().h(gap).into_any_element());
            }
            last_group = Some(group);
            let active = item == current;
            if active {
                active_ix = Some(rows.len());
            }
            let level = item.level();
            // Each level's glyph starts at the name of the row above it
            // (narrow: the Meetings views indent less). The categories are
            // a lighter third level: a 14 pt glyph and a 24 pt row; in the
            // narrow sidebar no glyph, the name at Meeting settings' name,
            // so every category name fits.
            let category = matches!(item, NavItem::SettingsCategory(_));
            let pl = match (level, compact) {
                (0, _) => dp(8.),
                (_, false) => dp(34.),
                (_, true) => dp(20.),
            };
            let glyph = true;
            // Every child row (People, the categories) shares People's
            // geometry; categories are only 2 pt shorter so the open list
            // still fits.
            let (slot, glyph_size, gap, height) = if category {
                (dp(18.), dp(16.), dp(8.), dp(24.))
            } else {
                (dp(18.), dp(16.), dp(8.), design::SIDEBAR_ITEM_H)
            };
            let badge = (item == NavItem::SettingsCategory(SettingsCategory::Setup)
                && setup_issues > 0)
                .then(|| {
                    h_flex()
                        .id("meetings-settings-setup-badge")
                        .flex_none()
                        .gap(dp(4.))
                        .child(div().size(dp(6.)).rounded_full().bg(warning))
                        .child(
                            design::mono(setup_issues.to_string())
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted),
                        )
                        .test_support()
                });
            rows.push(
                h_flex()
                    .id(item.id())
                    .aria_label(item.label())
                    .flex_none()
                    .gap(gap)
                    .pr(if compact { dp(4.) } else { dp(8.) })
                    .pl(pl)
                    .h(height)
                    .rounded(design::RADIUS_CHIP)
                    // A 2 pt ring on the selected row while the list has
                    // keyboard focus (reserved, transparent otherwise), so
                    // focus never depends on a fill shift alone.
                    .border_2()
                    .border_color(if active && focused {
                        ring
                    } else {
                        transparent_black()
                    })
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text)
                    .when(active, |d| d.bg(selection).font_weight(FontWeight::MEDIUM))
                    .when(!active, |d| {
                        d.hover(move |s| s.bg(tone.selection.opacity(0.5)))
                    })
                    .when(glyph, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .size(slot)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(item.icon()).size(glyph_size).text_color(if active {
                                    tone.accent_text
                                } else {
                                    tone.text_muted
                                })),
                        )
                    })
                    .child(div().flex_1().min_w_0().truncate().child(item.label()))
                    .children(badge)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.sidebar_focus, cx);
                        this.select_nav(item, cx)
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        if std::mem::take(&mut self.reveal_selection) {
            if let Some(ix) = active_ix {
                self.sidebar_scroll.scroll_to_item(ix);
            }
        }
        let list = v_flex()
            .id("sidebar")
            .track_focus(&self.sidebar_focus)
            .track_scroll(&self.sidebar_scroll)
            .key_context(crate::ui::SIDEBAR_CONTEXT)
            .on_action(
                cx.listener(|this, _: &SelectPreviousSection, _, cx| this.step_section(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNextSection, _, cx| this.step_section(1, cx)))
            .size_full()
            .px(if compact { dp(8.) } else { dp(10.) })
            .pt(dp(4.))
            .pb(dp(10.))
            .gap(px(0.))
            .overflow_y_scroll()
            .children(rows);
        // When rows continue below the fold, a hairline marks the bottom
        // edge (a cue that the list scrolls) without dimming any row.
        let more_below = {
            let max = self.sidebar_scroll.max_offset().y;
            max > px(0.) && -self.sidebar_scroll.offset().y < max - px(1.)
        };
        div()
            .relative()
            .size_full()
            .child(list)
            .when(more_below, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.))
                        .bg(tone.track),
                )
            })
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> ShownPage {
        macro_rules! page {
            ($field:ident, $ty:ty) => {{
                let entity = self
                    .pages
                    .$field
                    .get_or_insert_with(|| cx.new(|cx| <$ty>::new(window, cx)))
                    .clone();
                show(&entity, window, cx)
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
                if let Some(tab) = self.pending_meetings_tab.take() {
                    page.update(cx, |p, cx| p.set_tab(tab, cx));
                }
                if let Some(category) = self.pending_meetings_category.take() {
                    page.update(cx, |p, cx| p.show_settings(category, cx));
                }
                if let Some(link) = self.pending_meeting.take() {
                    page.update(cx, |p, cx| {
                        p.open_meeting_at(link.meeting_id, link.t_ms, window, cx)
                    });
                }
                show(&page, window, cx)
            }
            Section::Models => page!(models, ModelsPage),
            Section::Advanced => page!(advanced, AdvancedPage),
            Section::PostProcessing => page!(post_processing, PostProcessingPage),
            Section::Usage => page!(usage, UsagePage),
            Section::Storage => page!(storage, StoragePage),
            Section::Debug => page!(debug, DebugPage),
            Section::About => page!(about, AboutPage),
        }
    }

    /// The unified title bar of the app: over the sidebar it is empty
    /// sidebar (the traffic lights sit there); over the content it holds the
    /// page title and the page's actions. Dragging moves the window and a
    /// double click zooms it (`TitleBar`).
    #[allow(clippy::too_many_arguments)]
    fn render_title_bar(
        &self,
        title: SharedString,
        leading: Option<AnyElement>,
        toolbar: Option<AnyElement>,
        sidebar: bool,
        sidebar_w: Pixels,
        rtl: bool,
        cx: &App,
    ) -> TitleBar {
        let tone = design::tone(cx);
        let border = cx.theme().sidebar_border;
        // Without the sidebar (onboarding), the traffic lights sit over the
        // content: keep their space clear.
        let clear_lights = !sidebar || rtl;
        let bar = h_flex()
            .id("toolbar")
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(dp(8.))
            .pr(dp(12.))
            .when(clear_lights, |d| d.pl(TRAFFIC_LIGHTS_W))
            .when(!clear_lights, |d| d.pl(design::PAGE_X))
            .when(sidebar, |d| d.border_b_1().border_color(tone.hairline))
            .bg(tone.ink)
            .map(|d| match leading {
                // A page's own switcher takes the title's place; the spacer
                // keeps it put while the toolbar's width changes.
                Some(leading) => d
                    .child(h_flex().flex_none().child(leading))
                    .child(div().flex_1().min_w_0()),
                None => d.child(
                    div()
                        .id("toolbar-title")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(design::TEXT_TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tone.text)
                        .child(title)
                        .test_support(),
                ),
            })
            .when_some(toolbar, |d, actions| {
                d.child(h_flex().flex_none().gap(dp(8.)).child(actions))
            })
            .test_support();
        TitleBar::new()
            .pl_0()
            .h(design::TOOLBAR_H)
            .border_b_0()
            .bg(tone.ink)
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    .when(rtl, |d| d.flex_row_reverse())
                    .when(sidebar, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .w(sidebar_w)
                                .h_full()
                                .bg(tone.frame)
                                .when(!rtl, |d| d.border_r_1())
                                .when(rtl, |d| d.border_l_1())
                                .border_color(border),
                        )
                    })
                    .child(bar),
            )
    }

    fn render_onboarding(
        &mut self,
        step: OnboardingStep,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = matches!(&self.onboarding, Some((s, p, _)) if *s == step && *p == preview);
        if !current {
            let main = cx.entity().downgrade();
            let view: AnyView = match step {
                OnboardingStep::Welcome => cx.new(|cx| Welcome::new(preview, window, cx)).into(),
                OnboardingStep::Permissions => cx
                    .new(|cx| PermissionsOnboarding::new(main, preview, window, cx))
                    .into(),
                OnboardingStep::Setup => cx.new(|cx| SetupStep::new(preview, window, cx)).into(),
                OnboardingStep::Migration => {
                    cx.new(|cx| ModelMigration::new(main, window, cx)).into()
                }
                OnboardingStep::Done => {
                    self.onboarding = None;
                    return div().into_any_element();
                }
            };
            self.onboarding = Some((step, preview, view));
        }
        match &self.onboarding {
            Some((_, _, view)) => view.clone().into_any_element(),
            None => div().into_any_element(),
        }
    }

    /// While the core starts and the startup check runs: the shell of the
    /// screen that follows (the app for a returning user, the assistant for
    /// a new one), or why Guidy couldn't start.
    fn render_startup(&self, rtl: bool, sidebar_w: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let st = self.state.read(cx);
        if let CoreStatus::Failed(e) = &st.startup.core {
            return startup::startup_error(e, cx);
        }
        if !st.startup.expect_app {
            return startup::assistant_skeleton(cx);
        }
        let tone = design::tone(cx);
        let border = cx.theme().sidebar_border;
        h_flex()
            .size_full()
            .items_stretch()
            .when(rtl, |d| d.flex_row_reverse())
            .child(
                div()
                    .flex_none()
                    .w(sidebar_w)
                    .h_full()
                    .bg(tone.frame)
                    .when(!rtl, |d| d.border_r_1())
                    .when(rtl, |d| d.border_l_1())
                    .border_color(border)
                    .child(startup::sidebar_skeleton()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(tone.ink)
                    .child(startup::page_skeleton(cx)),
            )
            .into_any_element()
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
        let sidebar_w = sidebar_width(window);
        let in_app = self.step(cx) == Some(OnboardingStep::Done) && self.preview.is_none();
        // While starting, a returning user sees the app's shell at once:
        // nothing jumps when it is ready.
        let starting_app = {
            let st = self.state.read(cx);
            self.preview.is_none()
                && st.startup.step.is_none()
                && st.startup.expect_app
                && !matches!(st.startup.core, CoreStatus::Failed(_))
        };

        // Onboarding has no title: its screens name themselves.
        let mut title = SharedString::default();
        let mut toolbar = None;
        let mut leading = None;
        let content: AnyElement = if let Some(step) = self.preview {
            let body = self.render_onboarding(step, true, window, cx);
            toolbar = Some(
                Button::new("exit-preview")
                    .outline()
                    .label(t("settings.debug.onboardingPreview.exitButton"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.preview = None;
                        this.onboarding = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            div().size_full().child(body).into_any_element()
        } else {
            match self.step(cx) {
                None => self.render_startup(rtl, sidebar_w, cx),
                Some(OnboardingStep::Done) => {
                    let page = self.render_page(window, cx);
                    title = page.title.unwrap_or_else(|| t(self.section.label_key()));
                    toolbar = page.toolbar;
                    leading = page.leading;
                    let border = cx.theme().sidebar_border;
                    let sidebar = div()
                        .flex_none()
                        .w(sidebar_w)
                        .h_full()
                        .bg(tone.frame)
                        .when(!rtl, |d| d.border_r_1())
                        .when(rtl, |d| d.border_l_1())
                        .border_color(border)
                        .child(self.render_sidebar(window, cx));
                    // Banners stay pinned above the page; the page gets the
                    // rest of the height and owns its scrolling.
                    let main = v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .bg(tone.ink)
                        .child(self.accessibility_banner.clone())
                        .child(self.secure_banner.clone())
                        .children(startup::no_model_banner(
                            cx.entity().downgrade(),
                            self.section,
                            cx,
                        ))
                        .child(
                            div()
                                .id("page-area")
                                .flex_1()
                                .min_h_0()
                                .w_full()
                                .child(page.body),
                        );
                    h_flex()
                        .size_full()
                        .items_stretch()
                        .when(rtl, |d| d.flex_row_reverse())
                        .child(sidebar)
                        .child(main)
                        .into_any_element()
                }
                Some(step) => self.render_onboarding(step, false, window, cx),
            }
        };
        let title_bar = self.render_title_bar(
            title,
            leading,
            toolbar,
            in_app || starting_app,
            sidebar_w,
            rtl,
            cx,
        );

        v_flex()
            .id("main-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tone.ink)
            .text_color(tone.text)
            .font_family(crate::theme::UI_FONT)
            .on_action(cx.listener(Self::toggle_debug_mode))
            .on_action(|_: &CloseWindow, window, cx| crate::windows::close_main(window, cx))
            .child(title_bar)
            .child(div().flex_1().min_h_0().child(content))
            .when(in_app, |d| {
                d.child(self.status_bar.clone())
                    .child(self.whats_new.clone())
            })
            .when(starting_app, |d| {
                // The status bar's place, empty until the app is ready.
                d.child(
                    div()
                        .id("status-bar-skeleton")
                        .flex_none()
                        .h(design::STATUS_BAR_H)
                        .border_t_1()
                        .border_color(cx.theme().sidebar_border)
                        .bg(tone.frame),
                )
            })
    }
}
