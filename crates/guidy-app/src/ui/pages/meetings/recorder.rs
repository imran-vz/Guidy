//! The recorder in the Meetings toolbar: always visible, whatever tab or
//! meeting is shown. Idle it offers Record and the next calendar event;
//! recording it shows a pulsing dot, the time captured (pauses excluded),
//! the meeting's title (opens it), Pause/Resume and Stop. With meeting
//! notes off it offers to turn them on, and anything setup still needs
//! (permissions, models) is one amber "Set Up…" chip.

use super::{meeting_model_missing, meeting_settings, run, toast_error};
use crate::logic::meetings::{next_event, MeetingsEvent};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::PermissionReport;
use guidy_core::meetings::processing::SpeakerModelStatus;
use guidy_core::meetings::recorder::now_ms;
use guidy_core::meetings::types::{format_elapsed, CalendarEvent, MeetingId, RecorderState};
use std::time::Duration;

pub enum RecorderEvent {
    /// A recording started (its meeting) or stopped.
    Changed(Option<MeetingId>),
    /// Show this meeting (the recording's title was clicked).
    OpenMeeting(MeetingId),
    /// Show Settings › Setup.
    OpenSetup,
    /// Turn meeting notes on (they're off).
    TurnOn,
}

pub struct RecorderBar {
    recorder: RecorderState,
    busy: bool,
    upcoming: Vec<CalendarEvent>,
    permissions: Option<PermissionReport>,
    speaker_model: Option<SpeakerModelStatus>,
    ticking: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<RecorderEvent> for RecorderBar {}

impl RecorderBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| {
                let AppEvent::Meetings(ev) = ev else { return };
                match ev {
                    MeetingsEvent::RecorderState(rs) => {
                        this.set_recorder(rs.clone(), cx);
                        this.load_upcoming(cx);
                    }
                    MeetingsEvent::SpeakerModelStatus(s) => {
                        this.speaker_model = Some(s.clone());
                        cx.notify();
                    }
                    MeetingsEvent::Ready => this.reload(cx),
                    _ => {}
                }
            }),
        ];
        let mut this = Self {
            recorder: RecorderState::Idle,
            busy: false,
            upcoming: Vec::new(),
            permissions: None,
            speaker_model: None,
            ticking: false,
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    /// Recorder state, calendar, permissions and the speaker model.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.refresh_recorder(cx);
        self.load_upcoming(cx);
        run(
            cx,
            |b| (b.permission_status(), b.speaker_model_status()),
            |this: &mut Self, (p, s), _| {
                this.permissions = Some(p);
                // Unless an event got here first (newer).
                this.speaker_model.get_or_insert(s);
            },
        );
    }

    pub fn recorder(&self) -> &RecorderState {
        &self.recorder
    }

    fn set_recorder(&mut self, rs: RecorderState, cx: &mut Context<Self>) {
        self.recorder = rs;
        if self.recorder.is_recording() && !self.recorder.is_paused() {
            self.ensure_ticking(cx);
        }
        cx.notify();
    }

    fn refresh_recorder(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| b.recorder_state(),
            |this: &mut Self, rs, cx| this.set_recorder(rs, cx),
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

    /// The elapsed time moves once a second while capturing.
    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let keep = this
                .update(cx, |this, cx| {
                    let keep = this.recorder.is_recording() && !this.recorder.is_paused();
                    if keep {
                        cx.notify();
                    } else {
                        this.ticking = false;
                    }
                    keep
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    /// What setup still needs before meetings record and process: the
    /// microphone and system audio permissions, the meeting model and the
    /// speaker model. Calendar is optional.
    pub fn needs_setup(&self, cx: &App) -> bool {
        if !meeting_settings(cx).enabled {
            return false;
        }
        let permissions = self
            .permissions
            .as_ref()
            .is_some_and(|p| !p.microphone_granted || !p.system_audio_granted);
        let speaker = matches!(
            self.speaker_model,
            Some(SpeakerModelStatus::Missing | SpeakerModelStatus::Failed { .. })
        );
        permissions || speaker || meeting_model_missing(cx).is_some()
    }

    /// Record, or Stop while recording.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.recorder.is_recording() {
            self.busy = true;
            cx.notify();
            run(
                cx,
                |b| b.stop_recording(),
                |this: &mut Self, r, cx| {
                    if let Err(e) = r {
                        toast_error("meetings.recorder.stopError", &e, cx);
                    }
                    this.after_change(None, cx);
                },
            );
        } else if meeting_settings(cx).enabled {
            self.start(None, cx);
        } else {
            cx.emit(RecorderEvent::TurnOn);
        }
    }

    pub fn start(&mut self, event_id: Option<String>, cx: &mut Context<Self>) {
        if self.busy || self.recorder.is_recording() {
            return;
        }
        self.busy = true;
        cx.notify();
        run(
            cx,
            move |b| b.start_recording(event_id),
            |this: &mut Self, r, cx| {
                let id = match r {
                    Ok(id) => Some(id),
                    Err(e) => {
                        toast_error("meetings.recorder.startError", &e, cx);
                        None
                    }
                };
                this.after_change(id, cx);
            },
        );
    }

    /// Pause, or resume a paused recording.
    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if !self.recorder.is_recording() || self.busy {
            return;
        }
        let pause = !self.recorder.is_paused();
        run(
            cx,
            move |b| b.set_recording_paused(pause),
            |this: &mut Self, _, cx| this.refresh_recorder(cx),
        );
    }

    fn after_change(&mut self, started: Option<MeetingId>, cx: &mut Context<Self>) {
        self.busy = false;
        self.refresh_recorder(cx);
        self.load_upcoming(cx);
        cx.emit(RecorderEvent::Changed(started));
    }

    fn render_recording(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let RecorderState::Recording {
            meeting_id, title, ..
        } = &self.recorder
        else {
            return div().into_any_element();
        };
        let paused = self.recorder.is_paused();
        let elapsed = format_elapsed(self.recorder.elapsed_ms(now_ms()).unwrap_or(0));
        let meeting_id = *meeting_id;
        let dot = if paused {
            div()
                .flex_none()
                .size(dp(8.))
                .rounded_full()
                .bg(tone.text_faint)
                .into_any_element()
        } else {
            div()
                .flex_none()
                .size(dp(8.))
                .rounded_full()
                .bg(cx.theme().danger)
                .with_animation(
                    "meeting-rec-pulse",
                    Animation::new(Duration::from_millis(1600)).repeat(),
                    |el, delta| el.opacity(1.0 - 0.55 * (delta * std::f32::consts::PI).sin()),
                )
                .into_any_element()
        };
        h_flex()
            .id("meeting-recorder")
            .gap(dp(6.))
            .child(
                h_flex()
                    .id("meeting-recording-title")
                    .h(design::CONTROL_H)
                    .px(dp(6.))
                    .gap(dp(6.))
                    .rounded(design::RADIUS_CONTROL)
                    .hover(move |s| s.bg(tone.raised))
                    .child(dot)
                    .child(
                        design::mono(elapsed)
                            .id("meeting-recorder-time")
                            .text_color(if paused { tone.text_muted } else { tone.text })
                            .test_support(),
                    )
                    .child(
                        div()
                            .max_w(if narrow { dp(72.) } else { dp(160.) })
                            .truncate()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(if paused {
                                t("meetings.recorder.paused")
                            } else {
                                SharedString::from(title.clone())
                            }),
                    )
                    .tooltip({
                        let tip = t_args("meetings.recorder.show", &[("title", title)]);
                        move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                .build(window, cx)
                        }
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(RecorderEvent::OpenMeeting(meeting_id))
                    }))
                    .test_support(),
            )
            .child(
                Button::new("meeting-record-pause")
                    .ghost()
                    .small()
                    .icon(icon(if paused { "play" } else { "pause" }))
                    .tooltip(if paused {
                        t("meetings.recorder.resume")
                    } else {
                        t("meetings.recorder.pause")
                    })
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .child(
                Button::new("meeting-record-toggle")
                    .danger()
                    .small()
                    .icon(icon("square"))
                    .label(t("meetings.recorder.stopShort"))
                    .tooltip(t("meetings.recorder.stop"))
                    .loading(self.busy)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .test_support()
            .into_any_element()
    }

    fn render_idle(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let enabled = meeting_settings(cx).enabled;
        if !enabled {
            return h_flex()
                .id("meeting-recorder")
                .child(
                    Button::new("meeting-notes-enable")
                        .secondary()
                        .small()
                        .icon(icon("mic"))
                        .label(t("meetings.recorder.turnOn"))
                        .tooltip(t("meetings.recorder.disabled"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(RecorderEvent::TurnOn))),
                )
                .test_support()
                .into_any_element();
        }
        let tone = design::tone(cx);
        // A narrow window keeps the toolbar to the essentials.
        let next = next_event(&self.upcoming, super::now(cx)).filter(|_| !narrow);
        h_flex()
            .id("meeting-recorder")
            .gap(dp(6.))
            .when_some(next, |d, ev| {
                let label = format!("{} · {}", ev.title, super::clock_time(ev.start_at));
                let event_id = ev.id.clone();
                d.child(
                    div()
                        .id("upcoming-0")
                        .child(
                            h_flex()
                                .id("upcoming-start-0")
                                .h(design::CONTROL_H)
                                .max_w(dp(220.))
                                .px(dp(6.))
                                .gap(dp(5.))
                                .rounded(design::RADIUS_CONTROL)
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted)
                                .hover(move |s| s.bg(tone.raised).text_color(tone.text))
                                .child(icon("calendar").xsmall().flex_none())
                                .child(div().min_w_0().truncate().child(label))
                                .tooltip({
                                    let tip = t_args(
                                        "meetings.upcoming.recordEvent",
                                        &[("title", &ev.title)],
                                    );
                                    move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                            .build(window, cx)
                                    }
                                })
                                .when(!self.busy, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| {
                                        this.start(Some(event_id.clone()), cx)
                                    }))
                                })
                                .test_support(),
                        )
                        .test_support(),
                )
            })
            .child(
                Button::new("meeting-record-toggle")
                    .primary()
                    .small()
                    .icon(icon("circle"))
                    .label(t("meetings.recorder.record"))
                    .tooltip(t("meetings.recorder.start"))
                    .loading(self.busy)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .test_support()
            .into_any_element()
    }

    fn render_setup_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.needs_setup(cx) {
            return None;
        }
        let warning = cx.theme().warning;
        Some(
            h_flex()
                .id("meetings-setup")
                .flex_none()
                .h(design::CONTROL_H)
                .px(dp(7.))
                .gap(dp(4.))
                .rounded(design::RADIUS_CONTROL)
                .border_1()
                .border_color(warning.opacity(0.45))
                .bg(warning.opacity(0.12))
                .text_size(dp(12.))
                .text_color(warning)
                .hover(move |s| s.bg(warning.opacity(0.2)))
                .child(icon("triangle-alert").xsmall())
                .child(t("meetings.recorder.setUp"))
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(t("meetings.recorder.setUpHint"))
                        .build(window, cx)
                })
                .on_click(cx.listener(|_, _, _, cx| cx.emit(RecorderEvent::OpenSetup)))
                .test_support()
                .into_any_element(),
        )
    }
}

impl Render for RecorderBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < dp(900.);
        let body = if self.recorder.is_recording() {
            self.render_recording(narrow, cx)
        } else {
            self.render_idle(narrow, cx)
        };
        h_flex()
            .gap(dp(8.))
            .children(self.render_setup_chip(cx))
            .child(body)
    }
}
