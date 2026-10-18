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
use crate::ui::design::{self, Pill};
use crate::ui::kit::{icon, page, page_header, t, t_args};
use detail::{DetailEvent, MeetingDetail};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
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
                .child(div().flex_1().child(design::track(
                    SharedString::from(format!("meeting-model-progress-{place}")),
                    Some((pct as f32) / 100.0),
                    cx,
                )))
                .child(design::mono(format!("{pct:.0}%")).text_color(theme.muted_foreground))
                .into_any_element(),
        )
    } else {
        Some(
            Button::new(SharedString::from(format!(
                "meeting-model-download-{place}"
            )))
            .primary()
            .small()
            .pill()
            .icon(icon("download"))
            .label(t("meetings.model.download"))
            .on_click(move |_, _, cx| {
                let id = id.clone();
                AppState::global(cx).update(cx, |s, cx| s.download_meeting_model(&id, cx));
            })
            .into_any_element(),
        )
    };
    design::notice(theme.warning)
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
                            .secondary()
                            .small()
                            .pill()
                            .icon(icon("rotate-ccw"))
                            .label(t("meetings.init.retry"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.update(cx, |s, cx| s.retry_meetings_init(cx))
                            })),
                    )
                    .when(resettable, |row| {
                        row.child(
                            Button::new("meetings-init-reset")
                                .secondary()
                                .small()
                                .pill()
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
        let tone = design::tone(cx);
        let enabled = meeting_settings(cx).enabled;
        let recording = matches!(self.recorder, RecorderState::Recording { .. });
        let dot = if recording {
            design::signal_dot(cx)
                .with_animation(
                    "meeting-rec-pulse",
                    Animation::new(std::time::Duration::from_millis(1900)).repeat(),
                    |el, delta| el.opacity(1.0 - 0.45 * (delta * std::f32::consts::PI).sin()),
                )
                .into_any_element()
        } else {
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(7.)).rounded_full().bg(tone.text_faint))
                .into_any_element()
        };
        let status: AnyElement = match &self.recorder {
            RecorderState::Recording {
                title, started_at, ..
            } => v_flex()
                .gap(px(3.))
                .min_w_0()
                .child(
                    h_flex()
                        .gap_2()
                        .items_baseline()
                        .child(
                            div()
                                .text_size(design::TEXT_TITLE)
                                .font_weight(FontWeight::MEDIUM)
                                .child(t("meetings.recorder.recording")),
                        )
                        .child(design::mono(clock_time(*started_at)).text_color(tone.accent_text)),
                )
                .child(
                    div()
                        .id("meeting-recording-title")
                        .text_size(px(12.))
                        .text_color(tone.text_muted)
                        .truncate()
                        .child(title.clone())
                        .test_support(),
                )
                .into_any_element(),
            RecorderState::Idle => v_flex()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(design::TEXT_TITLE)
                        .font_weight(FontWeight::MEDIUM)
                        .child(t("meetings.recorder.idle")),
                )
                .when(!enabled, |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(tone.text_muted)
                            .child(t("meetings.recorder.disabled")),
                    )
                })
                .into_any_element(),
        };
        let button = Button::new("meeting-record-toggle")
            .pill()
            .loading(self.recorder_busy)
            .disabled(self.recorder_busy || (!recording && !enabled))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx)));
        let button = if recording {
            button
                .danger()
                .icon(icon("square"))
                .label(t("meetings.recorder.stop"))
        } else {
            // Unavailable until meeting notes are on: neutral, not a dim accent.
            button
                .primary()
                .when(!enabled, |b| b.secondary())
                .icon(icon("mic"))
                .label(t("meetings.recorder.start"))
        };
        design::panel(cx)
            .id("meeting-recorder")
            .when(recording, |d| {
                d.border_color(tone.accent_text.opacity(0.35))
            })
            .child(design::sheen("meeting-rec-sheen", design::RADIUS_PANEL, cx))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(14.))
                    .px(design::SECTION_X)
                    .py(design::SECTION_Y)
                    .child(dot)
                    .child(div().flex_1().min_w_0().child(status))
                    .child(button),
            )
            .test_support()
    }

    fn render_upcoming(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if matches!(self.recorder, RecorderState::Recording { .. }) || self.upcoming.is_empty() {
            return None;
        }
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
                    .px(design::SECTION_X)
                    .py(px(10.))
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(design::TEXT_BODY)
                                    .truncate()
                                    .child(ev.title.clone()),
                            )
                            .child(design::eyebrow(
                                format!("{} – {}", clock_time(ev.start_at), clock_time(ev.end_at)),
                                cx,
                            )),
                    )
                    .child(
                        Button::new(SharedString::from(format!("upcoming-start-{i}")))
                            .secondary()
                            .small()
                            .pill()
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
        h_flex().child(
            design::segmented(cx).children(tabs.into_iter().map(|(tab, key, _)| {
                design::segment(
                    SharedString::from(format!("meetings-tab-{}", tab.id())),
                    t(key),
                    self.tab == tab,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
                .test_support()
            })),
        )
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
