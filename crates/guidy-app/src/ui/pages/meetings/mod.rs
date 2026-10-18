//! Meetings page: recorder header (start/stop, upcoming calendar events),
//! then tabs: Meetings (list → detail), People, Settings.
//!
//! G1 owns this directory except `summary.rs`, `tickets.rs` and
//! `ai_settings.rs` (G2). See docs/MEETING_NOTES_ARCH.md.

pub mod ai_settings;
pub mod detail;
pub mod list;
pub mod people;
pub mod settings;
pub mod summary;
pub mod tickets;

use crate::logic::meetings::{MeetingsEvent, MeetingsInit};
use crate::logic::settings_store::ToastSpec;
use crate::meetings_backend::{MeetingsBackend, MeetingsError, SharedMeetingsBackend};
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{icon, page, page_header, t, t_args};
use detail::{DetailEvent, MeetingDetail};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable, StyledExt, WindowExt,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::transcription::meeting::{resolve_meeting_model_id, MeetingModelMissing};
use guidy_core::meetings::types::{CalendarEvent, MeetingId, RecorderState};
use list::{MeetingList, MeetingListEvent};
use people::PeopleView;
use settings::MeetingSettingsView;

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
    let configured = state
        .app_settings()
        .and_then(|s| s.meetings.meeting_model.clone());
    resolve_meeting_model_id(&state.models.models, configured.as_deref()).err()
}

/// "Download meeting model" (or its progress) for a missing meeting model.
/// `place` keeps element ids unique (`meeting-model-download-<place>`).
pub(crate) fn render_model_download(
    place: &str,
    missing: &MeetingModelMissing,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let state = AppState::global(cx);
    let st = state.read(cx);
    let id = missing.model_id.clone();
    let downloading = st.models.downloading.contains(&id);
    let pct = st
        .models
        .progress
        .get(&id)
        .map(|p| p.percentage)
        .unwrap_or(0.0);
    let message = if missing.known {
        t_args(
            "meetings.errors.modelMissing",
            &[("name", &missing.model_name)],
        )
    } else {
        t_args(
            "meetings.errors.modelUnknown",
            &[("name", &missing.model_name)],
        )
    };
    let action: Option<AnyElement> = if !missing.known {
        None
    } else if downloading {
        Some(
            h_flex()
                .gap_2()
                .w(px(200.))
                .child(
                    div().flex_1().child(
                        Progress::new(SharedString::from(format!(
                            "meeting-model-progress-{place}"
                        )))
                        .value(pct as f32),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{pct:.0}%")),
                )
                .into_any_element(),
        )
    } else {
        Some(
            Button::new(SharedString::from(format!(
                "meeting-model-download-{place}"
            )))
            .primary()
            .small()
            .icon(icon("download"))
            .label(t("meetings.model.download"))
            .on_click(move |_, _, cx| {
                let id = id.clone();
                AppState::global(cx).update(cx, |s, cx| s.download_meeting_model(&id, cx));
            })
            .into_any_element(),
        )
    };
    h_flex()
        .id(SharedString::from(format!("meeting-model-missing-{place}")))
        .w_full()
        .gap_3()
        .px_3()
        .py_2()
        .justify_between()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.warning.opacity(0.4))
        .bg(theme.warning.opacity(0.08))
        .child(div().flex_1().min_w_0().text_sm().child(message))
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
    fn id(self) -> &'static str {
        match self {
            Self::Meetings => "meetings",
            Self::People => "people",
            Self::Settings => "settings",
        }
    }
}

pub struct MeetingsPage {
    state: Entity<AppState>,
    pub tab: MeetingsTab,
    recorder: RecorderState,
    recorder_busy: bool,
    upcoming: Vec<CalendarEvent>,
    list: Entity<MeetingList>,
    detail: Option<Entity<MeetingDetail>>,
    people: Entity<PeopleView>,
    settings: Entity<MeetingSettingsView>,
    _detail_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}

impl MeetingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let list = cx.new(|cx| MeetingList::new(window, cx));
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| match ev {
                AppEvent::Meetings(MeetingsEvent::RecorderState(rs)) => {
                    this.recorder = rs.clone();
                    this.load_upcoming(cx);
                    cx.notify();
                }
                AppEvent::Meetings(MeetingsEvent::Ready) => this.on_ready(cx),
                _ => {}
            }),
            cx.subscribe_in(
                &list,
                window,
                |this, _, ev: &MeetingListEvent, window, cx| {
                    let MeetingListEvent::Open(id) = ev;
                    this.open_meeting(*id, window, cx);
                },
            ),
        ];
        let mut this = Self {
            state,
            tab: MeetingsTab::Meetings,
            recorder: RecorderState::Idle,
            recorder_busy: false,
            upcoming: Vec::new(),
            list,
            detail: None,
            people: cx.new(|cx| PeopleView::new(window, cx)),
            settings: cx.new(|cx| MeetingSettingsView::new(window, cx)),
            _detail_sub: None,
            _subs: subs,
        };
        this.refresh_recorder(cx);
        this.load_upcoming(cx);
        this
    }

    fn refresh_recorder(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| b.recorder_state(),
            |this: &mut Self, rs, _| this.recorder = rs,
        );
    }

    fn load_upcoming(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| b.upcoming_events(),
            |this: &mut Self, r, _| match r {
                Ok(events) => this.upcoming = events,
                Err(e) => log::warn!("Failed to load upcoming events: {e}"),
            },
        );
    }

    /// The manager came up (startup or retry): reload everything.
    fn on_ready(&mut self, cx: &mut Context<Self>) {
        self.refresh_recorder(cx);
        self.load_upcoming(cx);
        self.list.update(cx, |l, cx| l.reload(cx));
        self.people.update(cx, |p, cx| p.reload(cx));
        self.settings.update(cx, |s, cx| s.reload_status(cx));
        if let Some(d) = &self.detail {
            d.update(cx, |d, cx| d.load(cx));
        }
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
        self.open_meeting(id, window, cx);
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
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(crate::ui::kit::spinner(
                        "meetings-starting-spinner",
                        icon("loader-circle").xsmall(),
                    ))
                    .child(t("meetings.init.starting"))
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
        v_flex()
            .id(id)
            .w_full()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.danger.opacity(0.4))
            .bg(theme.danger.opacity(0.08))
            .child(div().text_sm().font_semibold().child(title))
            .when_some(description, |d, text| d.child(div().text_sm().child(text)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(error.to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("meetings-init-retry")
                            .outline()
                            .small()
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
                                .small()
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

    pub fn set_tab(&mut self, tab: MeetingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub fn open_meeting(&mut self, id: MeetingId, window: &mut Window, cx: &mut Context<Self>) {
        let detail = cx.new(|cx| MeetingDetail::new(id, window, cx));
        self._detail_sub = Some(
            cx.subscribe(&detail, |this, _, ev: &DetailEvent, cx| match ev {
                DetailEvent::Back => this.close_meeting(cx),
                DetailEvent::OpenSettings => {
                    this.close_meeting(cx);
                    this.set_tab(MeetingsTab::Settings, cx);
                }
            }),
        );
        self.detail = Some(detail);
        self.tab = MeetingsTab::Meetings;
        cx.notify();
    }

    pub fn close_meeting(&mut self, cx: &mut Context<Self>) {
        self.detail = None;
        self._detail_sub = None;
        self.list.update(cx, |l, cx| l.reload(cx));
        cx.notify();
    }

    #[cfg(test)]
    pub fn detail(&self) -> Option<&Entity<MeetingDetail>> {
        self.detail.as_ref()
    }

    #[cfg(test)]
    pub fn list(&self) -> &Entity<MeetingList> {
        &self.list
    }

    #[cfg(test)]
    pub fn people(&self) -> &Entity<PeopleView> {
        &self.people
    }

    #[cfg(test)]
    pub fn settings_view(&self) -> &Entity<MeetingSettingsView> {
        &self.settings
    }

    fn after_recorder_change(&mut self, cx: &mut Context<Self>) {
        self.recorder_busy = false;
        self.refresh_recorder(cx);
        self.load_upcoming(cx);
        self.list.update(cx, |l, cx| l.reload(cx));
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.recorder_busy {
            return;
        }
        self.recorder_busy = true;
        cx.notify();
        if matches!(self.recorder, RecorderState::Recording { .. }) {
            run(
                cx,
                |b| b.stop_recording(),
                |this: &mut Self, r, cx| {
                    if let Err(e) = r {
                        toast_error("meetings.recorder.stopError", &e, cx);
                    }
                    this.after_recorder_change(cx);
                },
            );
        } else {
            self.start(None, cx);
        }
    }

    fn start(&mut self, event_id: Option<String>, cx: &mut Context<Self>) {
        self.recorder_busy = true;
        cx.notify();
        run(
            cx,
            move |b| b.start_recording(event_id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.recorder.startError", &e, cx);
                }
                this.after_recorder_change(cx);
            },
        );
    }

    fn render_recorder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let enabled = meeting_settings(cx).enabled;
        let recording = matches!(self.recorder, RecorderState::Recording { .. });
        let status: AnyElement = match &self.recorder {
            RecorderState::Recording {
                title, started_at, ..
            } => v_flex()
                .gap_0p5()
                .min_w_0()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().size(px(8.)).rounded_full().bg(theme.danger))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(t("meetings.recorder.recording")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(clock_time(*started_at)),
                        ),
                )
                .child(
                    div()
                        .id("meeting-recording-title")
                        .text_sm()
                        .truncate()
                        .child(title.clone())
                        .test_support(),
                )
                .into_any_element(),
            RecorderState::Idle => v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().size(px(8.)).rounded_full().bg(theme.muted_foreground))
                        .child(div().text_sm().child(t("meetings.recorder.idle"))),
                )
                .when(!enabled, |d| {
                    d.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t("meetings.recorder.disabled")),
                    )
                })
                .into_any_element(),
        };
        let button = Button::new("meeting-record-toggle")
            .small()
            .loading(self.recorder_busy)
            .disabled(self.recorder_busy || (!recording && !enabled))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx)));
        let button = if recording {
            button
                .danger()
                .icon(icon("square"))
                .label(t("meetings.recorder.stop"))
        } else {
            button
                .primary()
                .icon(icon("mic"))
                .label(t("meetings.recorder.start"))
        };
        h_flex()
            .id("meeting-recorder")
            .w_full()
            .gap_4()
            .justify_between()
            .px_3()
            .py_2()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(if recording {
                theme.danger.opacity(0.4)
            } else {
                theme.border
            })
            .bg(theme.sidebar)
            .child(div().flex_1().min_w_0().child(status))
            .child(button)
            .test_support()
    }

    fn render_upcoming(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if matches!(self.recorder, RecorderState::Recording { .. }) || self.upcoming.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let enabled = meeting_settings(cx).enabled;
        let rows: Vec<AnyElement> = self
            .upcoming
            .iter()
            .enumerate()
            .map(|(i, ev)| {
                let event_id = ev.id.clone();
                h_flex()
                    .id(SharedString::from(format!("upcoming-{i}")))
                    .w_full()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(div().text_sm().truncate().child(ev.title.clone()))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                format!("{} – {}", clock_time(ev.start_at), clock_time(ev.end_at)),
                            )),
                    )
                    .child(
                        Button::new(SharedString::from(format!("upcoming-start-{i}")))
                            .outline()
                            .small()
                            .icon(icon("mic"))
                            .label(t("meetings.upcoming.start"))
                            .disabled(self.recorder_busy || !enabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start(Some(event_id.clone()), cx)
                            })),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        Some(crate::ui::kit::group(Some(t("meetings.upcoming.title")), rows, cx).into_any_element())
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = [
            (
                MeetingsTab::Meetings,
                "meetings.tabs.meetings",
                "notebook-pen",
            ),
            (MeetingsTab::People, "meetings.tabs.people", "users"),
            (MeetingsTab::Settings, "meetings.tabs.settings", "settings"),
        ];
        h_flex()
            .gap_1()
            .children(tabs.into_iter().map(|(tab, key, ic)| {
                Button::new(SharedString::from(format!("meetings-tab-{}", tab.id())))
                    .ghost()
                    .small()
                    .icon(icon(ic))
                    .label(t(key))
                    .selected(self.tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
            }))
    }
}

impl Render for MeetingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(init) = self.render_init(cx) {
            return page()
                .id("page-meetings")
                .child(page_header(
                    t("meetings.title"),
                    Some(t("meetings.description")),
                    cx,
                ))
                .child(init)
                .test_support();
        }
        let body: AnyElement = match self.tab {
            MeetingsTab::Meetings => match &self.detail {
                Some(detail) => detail.clone().into_any_element(),
                None => v_flex()
                    .w_full()
                    .gap_6()
                    .children(self.render_upcoming(cx))
                    .child(self.list.clone())
                    .into_any_element(),
            },
            MeetingsTab::People => self.people.clone().into_any_element(),
            MeetingsTab::Settings => self.settings.clone().into_any_element(),
        };
        page()
            .id("page-meetings")
            .child(page_header(
                t("meetings.title"),
                Some(t("meetings.description")),
                cx,
            ))
            .child(self.render_recorder(cx))
            .child(self.render_tabs(cx))
            .child(body)
            .test_support()
    }
}
