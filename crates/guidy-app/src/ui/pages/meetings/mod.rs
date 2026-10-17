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

use crate::logic::meetings::MeetingsEvent;
use crate::logic::settings_store::ToastSpec;
use crate::meetings_backend::{MeetingsBackend, SharedMeetingsBackend};
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{icon, page, page_header, t};
use detail::{DetailEvent, MeetingDetail};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable, StyledExt,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
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

/// Error toast: translated title, backend error as the description.
pub(crate) fn toast_error(key: &str, error: &str, cx: &mut App) {
    let spec = ToastSpec::error(t(key), Some(error.to_string()));
    AppState::global(cx).update(cx, |s, cx| s.toast(spec, cx));
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
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(MeetingsEvent::RecorderState(rs)) = ev {
                    this.recorder = rs.clone();
                    this.load_upcoming(cx);
                    cx.notify();
                }
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
        let _ = &self.state;
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
