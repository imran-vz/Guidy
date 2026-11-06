//! Meetings page: a full-height destination. The sidebar lists its three
//! views (Meetings, and under it People and Meeting settings, whose
//! categories it lists one level deeper while they are shown); the title
//! bar holds the view's title and the recorder (record, timer, pause,
//! stop, the next calendar event). Meetings is a split view: the
//! list (search, day groups) and the open meeting's detail, each with its
//! own scroll regions; below ~640 pt of content they stack.
//!
//! G1 owns this directory except `summary.rs`, `tickets.rs` and
//! `ai_settings.rs` (G2). See docs/MEETING_NOTES_ARCH.md.

pub mod ai_settings;
pub mod detail;
pub mod emails;
pub mod list;
pub mod people;
pub mod player;
pub mod recorder;
pub mod settings;
pub mod summary;
pub mod tickets;

use crate::logic::meetings::{MeetingsEvent, MeetingsInit};
use crate::logic::settings_store::ToastSpec;
use crate::meetings_backend::{MeetingsBackend, MeetingsError, SharedMeetingsBackend};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::download_control::DownloadControl;
use crate::ui::kit::{fill_page, icon, page, t, t_args};
use detail::{DetailEvent, DetailLayout, MeetingDetail};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::transcription::meeting::{resolve_meeting_model_id, MeetingModelMissing};
use guidy_core::meetings::types::MeetingId;
use list::{MeetingList, MeetingListEvent};
use people::PeopleView;
use recorder::{RecorderBar, RecorderEvent};
use settings::{MeetingSettingsEvent, MeetingSettingsView, SettingsCategory};

/// The meetings backend from app state.
pub(crate) fn meetings_backend(cx: &App) -> SharedMeetingsBackend {
    AppState::global(cx).read(cx).meetings.clone()
}

/// Run `f` against the meetings backend off the main thread, then `done`
/// on the view.
pub(crate) fn run<V, T, F, D>(cx: &mut Context<V>, f: F, done: D)
where
    V: 'static,
    T: Send + 'static,
    F: FnOnce(&dyn MeetingsBackend) -> T + Send + 'static,
    D: FnOnce(&mut V, T, &mut Context<V>) + 'static,
{
    let backend = meetings_backend(cx);
    cx.spawn(async move |this, cx| {
        let result = cx.background_spawn(async move { f(&*backend) }).await;
        this.update(cx, |view, cx| {
            done(view, result, cx);
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Error toast: translated title; known backend errors are localized and
/// anything else is shown raw as the description.
pub(crate) fn toast_error(key: &str, error: &str, cx: &mut App) {
    let e = MeetingsError::parse(error);
    if matches!(e, MeetingsError::ModelMissing { .. }) {
        // The model list may be stale: the download banner reads it.
        AppState::global(cx).update(cx, |s, cx| s.load_models(cx));
    }
    let spec = match e.localized() {
        Some(message) => ToastSpec::error(message, None),
        None => ToastSpec::error(t(key), Some(error.to_string())),
    };
    AppState::global(cx).update(cx, |s, cx| s.toast(spec, cx));
}

/// The meeting transcription model when it isn't downloaded (from the
/// loaded model list and settings; `None` until models load).
pub(crate) fn meeting_model_missing(cx: &App) -> Option<MeetingModelMissing> {
    let state = AppState::global(cx).read(cx);
    if !state.models.initialized || state.models.models.is_empty() {
        return None;
    }
    let settings = state.app_settings()?;
    resolve_meeting_model_id(
        &state.models.models,
        settings.meetings.meeting_model.as_deref(),
        Some(&settings.selected_model),
    )
    .err()
}

/// The downloaded transcription models, the one meetings use now first,
/// then by name.
pub(crate) fn local_model_choices(cx: &App) -> Vec<crate::logic::meetings::LocalModelChoice> {
    let state = AppState::global(cx).read(cx);
    let current = state.app_settings().and_then(|settings| {
        resolve_meeting_model_id(
            &state.models.models,
            settings.meetings.meeting_model.as_deref(),
            Some(&settings.selected_model),
        )
        .ok()
    });
    let mut choices: Vec<crate::logic::meetings::LocalModelChoice> = state
        .models
        .models
        .iter()
        .filter(|m| m.is_downloaded)
        .map(|m| crate::logic::meetings::LocalModelChoice {
            id: m.id.clone(),
            name: crate::ui::kit::model_name(m),
            current: current.as_deref() == Some(m.id.as_str()),
        })
        .collect();
    choices.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| a.name.cmp(&b.name)));
    choices
}

/// The download control for the meeting transcription model `model_id`.
/// It is marked as a meetings download, so finishing it doesn't offer to
/// make it the dictation model.
pub(crate) fn meeting_model_download(model_id: &str, cx: &App) -> DownloadControl {
    let id = model_id.to_string();
    DownloadControl::for_model(guidy_core::downloads::stt_id(model_id), cx).on_start(move |cx| {
        let id = id.clone();
        AppState::global(cx).update(cx, |s, _| {
            s.meeting_model_downloads.insert(id);
        });
    })
}

/// The message for a missing meeting model.
pub(crate) fn model_missing_message(missing: &MeetingModelMissing) -> SharedString {
    let key = if missing.known {
        "meetings.errors.modelMissing"
    } else {
        "meetings.errors.modelUnknown"
    };
    t_args(key, &[("name", &missing.model_name)])
}

/// A notice for a missing meeting model with its download control.
/// `place` keeps element ids unique (`meeting-model-missing-<place>`).
pub(crate) fn render_model_download(
    place: &str,
    missing: &MeetingModelMissing,
    cx: &mut App,
) -> AnyElement {
    render_model_download_with(place, missing, model_missing_message(missing), cx)
}

/// [`render_model_download`] with its own message.
pub(crate) fn render_model_download_with(
    place: &str,
    missing: &MeetingModelMissing,
    message: SharedString,
    cx: &mut App,
) -> AnyElement {
    let warning = cx.theme().warning;
    let action = missing
        .known
        .then(|| meeting_model_download(&missing.model_id, cx));
    design::notice(warning)
        .id(SharedString::from(format!("meeting-model-missing-{place}")))
        .flex()
        .items_center()
        .gap_3()
        .justify_between()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(design::TEXT_BODY)
                .child(message),
        )
        .children(action)
        .test_support()
        .into_any_element()
}

/// Meeting settings from app settings.
pub(crate) fn meeting_settings(cx: &App) -> guidy_core::meetings::settings::MeetingSettings {
    AppState::global(cx)
        .read(cx)
        .app_settings()
        .map(|s| s.meetings.clone())
        .unwrap_or_default()
}

/// "Now" for the meetings UI (day groups, the next event). Snapshots fix
/// it so their dates don't move.
pub struct MeetingsClock(pub i64);
impl Global for MeetingsClock {}

pub(crate) fn now(cx: &App) -> i64 {
    cx.try_global::<MeetingsClock>()
        .map(|c| c.0)
        .unwrap_or_else(guidy_core::meetings::recorder::now_ms)
}

/// Local wall-clock time of a unix-ms timestamp (`HH:MM`).
pub(crate) fn clock_time(at_ms: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingsTab {
    Meetings,
    People,
    Settings,
}

impl MeetingsTab {
    pub fn id(self) -> &'static str {
        match self {
            Self::Meetings => "meetings",
            Self::People => "people",
            Self::Settings => "settings",
        }
    }

    /// The translation key of the tab's name (sidebar row, toolbar title).
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Meetings => "meetings.title",
            Self::People => "meetings.tabs.people",
            Self::Settings => "meetings.tabs.meetingSettings",
        }
    }
}

actions!(
    meetings,
    [
        /// ⌘F: the list's search, or find in the open transcript.
        FindMeetings,
        SelectNextMeeting,
        SelectPreviousMeeting,
        /// Enter in the list: show the selected meeting (stack layout).
        OpenSelectedMeeting,
        /// ⌫: delete the selected meeting (asks first).
        DeleteMeeting,
        /// Space: play or pause the open meeting.
        TogglePlayback,
        /// ⌘R: record, or stop recording.
        ToggleRecording,
        /// ⇧⌘P: pause or resume the recording.
        TogglePauseRecording,
        /// ⌘E: export the open meeting.
        ExportMeeting,
        /// Esc: close find, cancel a rename, clear the search, go back.
        DismissMeetings,
    ]
);

/// The page's key context.
pub const MEETINGS_CONTEXT: &str = "MeetingsPage";

/// Key bindings of the Meetings page (⌘1–⌘9 belong to the sidebar).
pub fn init(cx: &mut App) {
    let page = Some(MEETINGS_CONTEXT);
    let not_typing = Some("MeetingsPage && !Input");
    cx.bind_keys([
        KeyBinding::new("cmd-f", FindMeetings, page),
        KeyBinding::new("down", SelectNextMeeting, Some("MeetingList")),
        KeyBinding::new("up", SelectPreviousMeeting, Some("MeetingList")),
        KeyBinding::new("enter", OpenSelectedMeeting, Some("MeetingList")),
        KeyBinding::new("backspace", DeleteMeeting, not_typing),
        KeyBinding::new("cmd-backspace", DeleteMeeting, not_typing),
        KeyBinding::new("space", TogglePlayback, not_typing),
        KeyBinding::new("cmd-r", ToggleRecording, page),
        KeyBinding::new("cmd-shift-p", TogglePauseRecording, page),
        KeyBinding::new("cmd-e", ExportMeeting, page),
        KeyBinding::new("escape", DismissMeetings, not_typing),
    ]);
}

/// The list's default width and its limits: one for the Meetings, People
/// and Settings tabs, which share the split ([`split_view`]).
pub(crate) const LIST_W: Pixels = dp(240.);
/// The list's default width in a window wider than [`LIST_WIDE_FROM`]:
/// longer titles fit, and the extra room isn't all given to the detail.
const LIST_W_WIDE: Pixels = dp(280.);
const LIST_WIDE_FROM: Pixels = dp(1280.);
pub(crate) const LIST_MIN_W: Pixels = dp(220.);
pub(crate) const LIST_MAX_W: Pixels = dp(400.);
/// Below this content width the list and the detail stack (Meetings), the
/// people list narrows.
pub(crate) const STACK_BELOW: Pixels = dp(640.);
/// From this detail width, notes and transcript sit side by side.
const SIDE_BY_SIDE_FROM: Pixels = dp(880.);
/// Below this detail width, the actions get their own row.
const COMPACT_BELOW: Pixels = dp(640.);
/// Under this transcript width the speaker and time go above each turn.
const NARROW_TRANSCRIPT_BELOW: Pixels = dp(560.);
/// Under this detail width the player gets its own row in the header.
const PLAYER_ROW_BELOW: Pixels = dp(560.);

/// What the Meetings page remembers while the app runs (the window can be
/// closed and opened again): the list width and the meeting shown.
#[derive(Default)]
struct MeetingsMemory {
    selected: Option<MeetingId>,
    /// The width the user dragged the list to (`None`: never dragged).
    list_width: Option<Pixels>,
    /// The list's default width for the window last shown
    /// ([`default_list_width`]).
    default_width: Option<Pixels>,
}
impl Global for MeetingsMemory {}

fn remember(cx: &mut App, f: impl FnOnce(&mut MeetingsMemory)) {
    f(cx.default_global::<MeetingsMemory>());
}

/// Whether the content area (the window less its sidebar) is narrow enough
/// to stack the list and the detail.
pub(crate) fn is_narrow(window: &Window) -> bool {
    window.viewport_size().width - crate::ui::main_window::sidebar_width(window) < STACK_BELOW
}

/// The list's default width for this window: wider above
/// [`LIST_WIDE_FROM`].
fn default_list_width(window: &Window) -> Pixels {
    if window.viewport_size().width > LIST_WIDE_FROM {
        LIST_W_WIDE
    } else {
        LIST_W
    }
}

/// The remembered width, else the default for the window last shown.
fn remembered_list_width(cx: &App) -> Pixels {
    cx.try_global::<MeetingsMemory>()
        .and_then(|m| m.list_width.or(m.default_width))
        .unwrap_or(LIST_W)
}

/// The list's width in `split` now, else the remembered or default one.
pub(crate) fn list_width(split: &Entity<ResizableState>, cx: &App) -> Pixels {
    split
        .read(cx)
        .sizes()
        .first()
        .copied()
        .unwrap_or_else(|| remembered_list_width(cx))
        .clamp(LIST_MIN_W, LIST_MAX_W)
}

/// A list | detail split view. The Meetings, People and Settings tabs of a
/// window all use it with one state (the page's), so the list has the same
/// width, limits and drag on every tab; the width is also remembered for
/// the next window.
pub(crate) fn split_view(
    id: &'static str,
    split: &Entity<ResizableState>,
    list: impl IntoElement,
    detail: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let remembered = remembered_list_width(cx);
    h_resizable(id)
        .with_state(split)
        .on_resize(|state, _, cx| {
            let w = state.read(cx).sizes().first().copied();
            remember(cx, |m| m.list_width = w);
        })
        .child(
            resizable_panel()
                .size(remembered)
                .size_range(LIST_MIN_W..LIST_MAX_W)
                .child(list),
        )
        .child(resizable_panel().child(detail))
        .into_any_element()
}

pub struct MeetingsPage {
    state: Entity<AppState>,
    pub tab: MeetingsTab,
    recorder: Entity<RecorderBar>,
    list: Entity<MeetingList>,
    detail: Option<Entity<MeetingDetail>>,
    people: Entity<PeopleView>,
    settings: Entity<MeetingSettingsView>,
    /// The list | detail split, shared with People and Settings.
    split: Entity<ResizableState>,
    /// The default list width last applied to the split
    /// ([`Self::fit_list_to_window`]).
    list_fitted_to: Option<Pixels>,
    focus: FocusHandle,
    /// Stack layout (narrow): the detail is shown over the list.
    stack_detail: bool,
    /// The meeting remembered from last time, opened once the list has
    /// loaded and still has it.
    restore: Option<MeetingId>,
    /// The first meeting was opened after the list loaded.
    auto_opened: bool,
    _detail_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}

impl MeetingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let list = cx.new(|cx| MeetingList::new(window, cx));
        let recorder = cx.new(RecorderBar::new);
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(MeetingsEvent::Ready) = ev {
                    this.on_ready(cx)
                }
            }),
            cx.subscribe_in(&list, window, Self::on_list_event),
            cx.subscribe_in(&recorder, window, Self::on_recorder_event),
            cx.observe_in(&list, window, |this, list, window, cx| {
                // Once the list has loaded: the remembered meeting if it
                // still exists, else the newest (as Mail and Notes do), so
                // the detail isn't empty. Not in the stack layout, where the
                // list is what's shown.
                if this.auto_opened || this.detail.is_some() {
                    return;
                }
                let (loading, pick) = {
                    let list = list.read(cx);
                    let remembered = this.restore.filter(|id| list.meeting(*id).is_some());
                    (
                        list.is_loading(),
                        remembered.or(list.order().first().copied()),
                    )
                };
                if loading {
                    return;
                }
                this.auto_opened = true;
                this.restore = None;
                if let Some(id) = pick {
                    if !Self::is_stack(window) {
                        let stacked = this.stack_detail;
                        this.open_meeting(id, window, cx);
                        this.stack_detail = stacked;
                    }
                }
            }),
        ];
        let restore = cx.try_global::<MeetingsMemory>().and_then(|m| m.selected);
        let split = cx.new(|_| ResizableState::default());
        let people = cx.new(|cx| PeopleView::new(window, cx).with_split(split.clone()));
        let settings = cx.new(|cx| MeetingSettingsView::new(window, cx).with_split(split.clone()));
        let mut subs = subs;
        subs.push(cx.subscribe(
            &settings,
            |this, _, ev: &MeetingSettingsEvent, cx| match ev {
                MeetingSettingsEvent::OpenYourVoice => {
                    this.tab = MeetingsTab::People;
                    this.people.update(cx, |p, cx| p.select_you(cx));
                    cx.notify();
                }
            },
        ));
        Self {
            state,
            tab: MeetingsTab::Meetings,
            recorder,
            list,
            detail: None,
            people,
            settings,
            split,
            list_fitted_to: None,
            focus: cx.focus_handle(),
            stack_detail: false,
            restore,
            auto_opened: false,
            _detail_sub: None,
            _subs: subs,
        }
    }

    /// The manager came up (startup or retry): reload everything.
    fn on_ready(&mut self, cx: &mut Context<Self>) {
        self.recorder.update(cx, |r, cx| r.reload(cx));
        self.list.update(cx, |l, cx| l.reload(cx));
        self.people.update(cx, |p, cx| p.reload(cx));
        self.settings.update(cx, |s, cx| s.reload_status(cx));
        if let Some(d) = &self.detail {
            d.update(cx, |d, cx| d.load(cx));
        }
        cx.notify();
    }

    fn on_list_event(
        &mut self,
        _: &Entity<MeetingList>,
        ev: &MeetingListEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            MeetingListEvent::Open(id) => {
                self.open_meeting(*id, window, cx);
                self.push_detail(window, cx);
            }
            MeetingListEvent::OpenLine {
                meeting_id,
                segment_id,
            } => {
                self.open_meeting(*meeting_id, window, cx);
                self.stack_detail = true;
                if let Some(detail) = &self.detail {
                    let line = *segment_id;
                    detail.update(cx, |d, cx| d.reveal_when_loaded(line, cx));
                }
            }
            MeetingListEvent::Deleted { id, next } => {
                if self
                    .detail
                    .as_ref()
                    .is_some_and(|d| d.read(cx).meeting_id == *id)
                {
                    // Split view: the next meeting takes its place. Stack:
                    // back to the list.
                    match next {
                        Some(next) if !Self::is_stack(window) => {
                            self.open_meeting(*next, window, cx)
                        }
                        _ => {
                            self.close_meeting(cx);
                            self.focus_list(window, cx);
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    fn on_recorder_event(
        &mut self,
        _: &Entity<RecorderBar>,
        ev: &RecorderEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            RecorderEvent::Changed(started) => {
                self.list.update(cx, |l, cx| l.reload(cx));
                if let Some(id) = started {
                    // Show the live transcript of the new recording.
                    self.tab = MeetingsTab::Meetings;
                    self.open_meeting(*id, window, cx);
                } else if let Some(d) = &self.detail {
                    d.update(cx, |d, cx| d.load(cx));
                }
            }
            RecorderEvent::OpenMeeting(id) => {
                self.tab = MeetingsTab::Meetings;
                self.open_meeting(*id, window, cx);
                self.stack_detail = true;
            }
            RecorderEvent::OpenSetup => self.show_settings(SettingsCategory::Setup, cx),
            RecorderEvent::TurnOn => self.settings.update(cx, |s, cx| s.set_enabled(true, cx)),
        }
        cx.notify();
    }

    fn on_detail_event(
        &mut self,
        _: &Entity<MeetingDetail>,
        ev: &DetailEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            DetailEvent::Back => self.go_back(window, cx),
            DetailEvent::OpenSettings => self.show_settings(SettingsCategory::Linear, cx),
            DetailEvent::Delete(id) => {
                let id = *id;
                self.list
                    .update(cx, |l, cx| l.confirm_delete(id, window, cx));
            }
            DetailEvent::Renamed => self.list.update(cx, |l, cx| l.reload(cx)),
        }
        cx.notify();
    }

    /// Meetings › Settings, at `category`.
    pub fn show_settings(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        self.tab = MeetingsTab::Settings;
        self.settings
            .update(cx, |s, cx| s.show_category(category, cx));
        cx.notify();
    }

    /// Deep link: open a meeting, then seek and play at `t_ms`.
    pub fn open_meeting_at(
        &mut self,
        id: MeetingId,
        t_ms: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab = MeetingsTab::Meetings;
        self.open_meeting(id, window, cx);
        self.stack_detail = true;
        if let (Some(t_ms), Some(detail)) = (t_ms, &self.detail) {
            detail.update(cx, |d, cx| d.seek_when_loaded(t_ms, cx));
        }
    }

    fn render_init(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        match &self.state.read(cx).meetings_init.clone() {
            MeetingsInit::Ready | MeetingsInit::NotStarted => None,
            MeetingsInit::Starting => Some(
                h_flex()
                    .id("meetings-starting")
                    .w_full()
                    .gap_2()
                    .px_3()
                    .py_3()
                    .text_size(design::TEXT_BODY)
                    .text_color(theme.muted_foreground)
                    .child(crate::ui::kit::spinner(
                        "meetings-starting-spinner",
                        icon("loader-circle").xsmall(),
                    ))
                    .child(design::shimmer_text(
                        "meetings-starting-text",
                        t("meetings.init.starting"),
                        cx,
                    ))
                    .test_support()
                    .into_any_element(),
            ),
            MeetingsInit::Failed(e) => Some(self.render_init_error(
                "meetings-init-failed",
                t("meetings.init.failed"),
                None,
                e,
                false,
                cx,
            )),
            MeetingsInit::DataKeyMissing(e) => Some(self.render_init_error(
                "meetings-init-key-missing",
                t("meetings.init.keyMissing.title"),
                Some(t("meetings.init.keyMissing.description")),
                e,
                true,
                cx,
            )),
        }
    }

    /// A failed start: what happened, the error, Retry and, with
    /// `resettable` (only for a key problem: other failures leave the notes
    /// readable), a reset that moves the unreadable meeting data aside
    /// (after a confirmation).
    fn render_init_error(
        &self,
        id: &'static str,
        title: SharedString,
        description: Option<SharedString>,
        error: &str,
        resettable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        design::notice(theme.danger)
            .id(id)
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(design::TEXT_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .when_some(description, |d, text| {
                d.child(div().text_size(design::TEXT_BODY).child(text))
            })
            .child(design::mono(error.to_string()).text_color(theme.muted_foreground))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("meetings-init-retry")
                            .outline()
                            .icon(icon("rotate-ccw"))
                            .label(t("meetings.init.retry"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.update(cx, |s, cx| s.retry_meetings_init(cx))
                            })),
                    )
                    .when(resettable, |row| {
                        row.child(
                            Button::new("meetings-init-reset")
                                .outline()
                                .icon(icon("archive"))
                                .label(t("meetings.init.reset.action"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_reset(window, cx)
                                })),
                        )
                    }),
            )
            .test_support()
            .into_any_element()
    }

    fn confirm_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let state = state.clone();
            alert
                .title(t("meetings.init.reset.title"))
                .description(t("meetings.init.reset.confirm"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    state.update(cx, |s, cx| s.reset_meetings_data(cx)).ok();
                    true
                })
        });
    }

    /// Back from a meeting shown over the list (stack layout).
    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stack_detail = false;
        let focus = self.list.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: MeetingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// Show meeting `id` in the detail (a no-op when it already is, so
    /// its edits, scroll and playback stay).
    /// Doesn't switch tabs: callers that show it do.
    pub fn open_meeting(&mut self, id: MeetingId, window: &mut Window, cx: &mut Context<Self>) {
        self.restore = None;
        self.list.update(cx, |l, cx| l.set_selected(Some(id), cx));
        remember(cx, |m| m.selected = Some(id));
        if self
            .detail
            .as_ref()
            .is_some_and(|d| d.read(cx).meeting_id == id)
        {
            cx.notify();
            return;
        }
        let detail = cx.new(|cx| MeetingDetail::new(id, window, cx));
        self._detail_sub = Some(cx.subscribe_in(&detail, window, Self::on_detail_event));
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn close_meeting(&mut self, cx: &mut Context<Self>) {
        self.detail = None;
        self._detail_sub = None;
        self.stack_detail = false;
        self.list.update(cx, |l, cx| {
            l.set_selected(None, cx);
            l.reload(cx)
        });
        remember(cx, |m| m.selected = None);
        cx.notify();
    }

    pub fn detail(&self) -> Option<&Entity<MeetingDetail>> {
        self.detail.as_ref()
    }

    pub fn list(&self) -> &Entity<MeetingList> {
        &self.list
    }

    #[cfg(test)]
    pub fn people(&self) -> &Entity<PeopleView> {
        &self.people
    }

    /// The meeting settings (the window's sidebar lists their categories).
    pub fn settings_view(&self) -> &Entity<MeetingSettingsView> {
        &self.settings
    }

    #[cfg(test)]
    pub fn recorder(&self) -> &Entity<RecorderBar> {
        &self.recorder
    }

    /// Stack layout is showing the detail.
    pub fn is_stacked_detail(&self) -> bool {
        self.stack_detail && self.detail.is_some()
    }

    /// Focus the meeting list (keyboard selection).
    pub fn focus_list(&self, window: &mut Window, cx: &mut App) {
        let focus = self.list.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }

    // -- keyboard ---------------------------------------------------------------

    /// Stack layout (from the window width).
    fn is_stack(window: &Window) -> bool {
        is_narrow(window)
    }

    fn step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.list.read(cx);
        let order = list.order();
        let current = list.selected();
        if let Some(id) = crate::logic::meetings::step_selection(&order, current, step) {
            let stacked = self.stack_detail;
            self.open_meeting(id, window, cx);
            self.stack_detail = stacked;
        }
    }

    fn on_find(&mut self, _: &FindMeetings, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = MeetingsTab::Meetings;
        let list_focused = self
            .list
            .read(cx)
            .focus_handle()
            .contains_focused(window, cx);
        let detail_shown = !Self::is_stack(window) || self.stack_detail;
        match &self.detail {
            Some(detail) if detail_shown && !list_focused => {
                detail.update(cx, |d, cx| d.open_find(window, cx))
            }
            _ => {
                self.stack_detail = false;
                self.list.update(cx, |l, cx| l.focus_search(window, cx));
            }
        }
        cx.notify();
    }

    fn on_open_selected(
        &mut self,
        _: &OpenSelectedMeeting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.detail.is_some() {
            self.push_detail(window, cx);
        }
    }

    /// Show the detail over the list (stack layout). The list leaves the
    /// screen, so the page takes the focus and its shortcuts keep working.
    fn push_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stack_detail = true;
        if Self::is_stack(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn on_delete(&mut self, _: &DeleteMeeting, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != MeetingsTab::Meetings {
            return;
        }
        if let Some(id) = self.list.read(cx).selected() {
            self.list
                .update(cx, |l, cx| l.confirm_delete(id, window, cx));
        }
    }

    fn on_toggle_playback(&mut self, _: &TogglePlayback, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab != MeetingsTab::Meetings {
            return;
        }
        if let Some(d) = &self.detail {
            d.update(cx, |d, cx| d.toggle_play(cx));
        }
    }

    fn on_export(&mut self, _: &ExportMeeting, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &self.detail {
            d.update(cx, |d, cx| d.export_default(cx));
        }
    }

    /// Esc, innermost first: the detail (find, rename, line selection),
    /// the list (rename, search), then back from the detail (stack).
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &self.detail {
            if d.update(cx, |d, cx| d.dismiss(window, cx)) {
                return;
            }
        }
        let list_done = self.list.update(cx, |l, cx| {
            if l.is_renaming() {
                l.cancel_rename(cx);
                true
            } else {
                l.clear_search(window, cx)
            }
        });
        if list_done {
            return;
        }
        if self.stack_detail {
            self.stack_detail = false;
            self.focus_list(window, cx);
            cx.notify();
        }
    }

    /// Until the user drags it, the list keeps the default width for the
    /// window's size: [`LIST_W`], or [`LIST_W_WIDE`] in a wide window (the
    /// split shared by Meetings, People and Settings follows a resize).
    fn fit_list_to_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = default_list_width(window);
        remember(cx, |m| m.default_width = Some(target));
        let dragged = cx
            .try_global::<MeetingsMemory>()
            .is_some_and(|m| m.list_width.is_some());
        if dragged || is_narrow(window) {
            return;
        }
        // Once per target (the split clamps to its container, so its width
        // may never equal the target exactly).
        let Some(current) = self.split.read(cx).sizes().first().copied() else {
            return;
        };
        if self.list_fitted_to == Some(target) {
            return;
        }
        self.list_fitted_to = Some(target);
        if (current - target).abs() > px(0.5) {
            self.split
                .update(cx, |s, cx| s.resize_panel(0, target, window, cx));
        }
    }

    /// Where the detail goes when no meeting is selected: what to do next.
    fn render_placeholder(&self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let enabled = meeting_settings(cx).enabled;
        let list = self.list.read(cx);
        let (title, body, action): (SharedString, SharedString, Option<AnyElement>) = if !enabled {
            (
                t("meetings.empty.offTitle"),
                t("meetings.empty.offBody"),
                Some(
                    Button::new("meetings-empty-enable")
                        .primary()
                        .label(t("meetings.recorder.turnOn"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.update(cx, |s, cx| s.set_enabled(true, cx))
                        }))
                        .into_any_element(),
                ),
            )
        } else if list.is_empty() {
            // Text only: the toolbar's Record is the one Record.
            (t("meetings.empty.title"), t("meetings.empty.body"), None)
        } else {
            (t("meetings.empty.select"), SharedString::default(), None)
        };
        let glyph = (!body.is_empty()).then(|| {
            icon(if enabled { "notebook-text" } else { "mic-off" })
                .size(dp(32.))
                .text_color(tone.text_muted)
        });
        v_flex()
            .id("meetings-placeholder")
            .size_full()
            .items_center()
            .justify_center()
            .gap(dp(8.))
            .px(design::PAGE_X)
            .children(glyph.map(|g| div().pb(dp(4.)).child(g)))
            .child(
                div()
                    .text_size(design::TEXT_HEADLINE)
                    .line_height(dp(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if body.is_empty() {
                        tone.text_muted
                    } else {
                        tone.text
                    })
                    .child(title),
            )
            .when(!body.is_empty(), |d| {
                d.child(
                    div()
                        .max_w(dp(320.))
                        .text_center()
                        .text_size(design::TEXT_BODY)
                        .line_height(design::LINE_BODY)
                        .text_color(tone.text_muted)
                        .child(body),
                )
            })
            .children(action.map(|a| div().pt(dp(8.)).child(a)))
            .test_support()
            .into_any_element()
    }

    fn render_meetings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content_w =
            window.viewport_size().width - crate::ui::main_window::sidebar_width(window);
        let stack = content_w < STACK_BELOW;
        let list_w = list_width(&self.split, cx);
        let detail_w = if stack { content_w } else { content_w - list_w };
        let side_by_side = !stack && detail_w >= SIDE_BY_SIDE_FROM;
        let transcript_w = if side_by_side {
            detail_w / 2.
        } else {
            detail_w
        };
        let layout = DetailLayout {
            side_by_side,
            stacked: stack,
            player_row: detail_w < PLAYER_ROW_BELOW,
            compact: detail_w < COMPACT_BELOW,
            narrow_transcript: transcript_w < NARROW_TRANSCRIPT_BELOW,
        };
        if let Some(d) = &self.detail {
            d.update(cx, |d, cx| d.set_layout(layout, cx));
        }
        // No meetings at all: one message across the content, not an empty
        // list beside an empty detail (it still has the list's notices).
        if self.detail.is_none()
            && self.list.read(cx).is_empty()
            && !self.list.read(cx).is_searching_now()
        {
            let notice = self.list.update(cx, |l, cx| l.turn_consent(cx));
            return v_flex()
                .size_full()
                .children(notice.map(|n| div().flex_none().px(dp(16.)).pt(dp(12.)).child(n)))
                .child(div().flex_1().min_h_0().child(self.render_placeholder(cx)))
                .into_any_element();
        }
        let detail_pane = match &self.detail {
            Some(d) => d.clone().into_any_element(),
            None => self.render_placeholder(cx),
        };
        if stack {
            return if self.stack_detail && self.detail.is_some() {
                detail_pane
            } else {
                self.list.clone().into_any_element()
            };
        }
        split_view(
            "meetings-split",
            &self.split,
            self.list.clone(),
            detail_pane,
            cx,
        )
    }
}

impl Render for MeetingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fit_list_to_window(window, cx);
        let tone = design::tone(cx);
        let body: AnyElement = if let Some(init) = self.render_init(cx) {
            div()
                .size_full()
                .px(design::PAGE_X)
                .pt(design::PAGE_TOP)
                .child(page().max_w_full().child(init))
                .into_any_element()
        } else {
            match self.tab {
                MeetingsTab::Meetings => self.render_meetings(window, cx),
                MeetingsTab::People => self.people.clone().into_any_element(),
                MeetingsTab::Settings => self.settings.clone().into_any_element(),
            }
        };
        fill_page("page-meetings")
            .key_context(MEETINGS_CONTEXT)
            .track_focus(&self.focus)
            .bg(tone.ink)
            .on_action(cx.listener(Self::on_find))
            .on_action(
                cx.listener(|this, _: &SelectNextMeeting, window, cx| this.step(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectPreviousMeeting, window, cx| {
                    this.step(-1, window, cx)
                }),
            )
            .on_action(cx.listener(Self::on_open_selected))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_toggle_playback))
            .on_action(cx.listener(|this, _: &ToggleRecording, _, cx| {
                this.recorder.update(cx, |r, cx| r.toggle(cx))
            }))
            .on_action(cx.listener(|this, _: &TogglePauseRecording, _, cx| {
                this.recorder.update(cx, |r, cx| r.toggle_pause(cx))
            }))
            .on_action(cx.listener(Self::on_export))
            .on_action(
                cx.listener(|this, _: &DismissMeetings, window, cx| this.dismiss(window, cx)),
            )
            // An Esc a text field didn't use (the search, find).
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                    this.dismiss(window, cx)
                }),
            )
            // A click on something that takes no focus keeps the page's
            // shortcuts working.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.focus.contains_focused(window, cx) {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .child(body)
            .test_support()
    }
}

impl crate::ui::pages::Page for MeetingsPage {
    /// The shown view's name: the sidebar lists Meetings, People and
    /// Meeting settings, and the title says which one this is.
    fn title(&self, cx: &App) -> Option<SharedString> {
        // Meeting settings: the shown category names the page (its sidebar
        // row sits under "Meeting settings"), like a System Settings pane.
        if self.tab == MeetingsTab::Settings {
            return Some(self.settings.read(cx).category().label());
        }
        Some(t(self.tab.label_key()))
    }

    /// A meeting or a person shown over the list (stack layout): Back and
    /// the title, at the toolbar's leading edge, so the detail below keeps
    /// the page's edge for its title, tabs and body. The one Back style of
    /// every list → detail stack.
    fn leading(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.render_init(cx).is_some() {
            return None;
        }
        let back = match self.tab {
            MeetingsTab::Meetings if Self::is_stack(window) && self.is_stacked_detail() => {
                Button::new("meeting-back")
                    .tooltip(t("meetings.detail.back"))
                    .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx)))
            }
            MeetingsTab::People if self.people.read(cx).is_stacked_detail(window, cx) => {
                let people = self.people.clone();
                Button::new("people-back")
                    .tooltip(t("common.back"))
                    .on_click(move |_, window, cx| people.update(cx, |p, cx| p.go_back(window, cx)))
            }
            _ => return None,
        };
        let tone = design::tone(cx);
        Some(
            h_flex()
                .id("meetings-back-title")
                .gap(dp(4.))
                // The glyph, not the button's edge, sits on the page edge.
                .ml(dp(-6.))
                .child(back.ghost().small().icon(icon("chevron-left")))
                .child(
                    div()
                        .id("toolbar-title")
                        .min_w_0()
                        .truncate()
                        .text_size(design::TEXT_TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tone.text)
                        .child(t(self.tab.label_key()))
                        .test_support(),
                )
                .test_support()
                .into_any_element(),
        )
    }

    fn toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.render_init(cx).is_some() {
            return None;
        }
        // The recording's title is on screen when its meeting is the one
        // shown in the detail; elsewhere (People, settings, another
        // meeting) the toolbar keeps it.
        let recording = self.recorder.read(cx).recording_id();
        let shown = self.tab == MeetingsTab::Meetings
            && recording.is_some()
            && self
                .detail
                .as_ref()
                .is_some_and(|d| Some(d.read(cx).meeting_id) == recording)
            && (!Self::is_stack(window) || self.stack_detail);
        self.recorder
            .update(cx, |r, cx| r.set_title_shown(shown, cx));
        Some(self.recorder.clone().into_any_element())
    }
}
