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
    /// The recording's title is already on screen (its meeting is shown
    /// in the detail), so the toolbar leaves it out.
    title_shown: bool,
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
            title_shown: false,
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

    /// A start or stop is in flight.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn recorder(&self) -> &RecorderState {
        &self.recorder
    }

    /// The meeting being recorded, if any.
    pub fn recording_id(&self) -> Option<MeetingId> {
        match &self.recorder {
            RecorderState::Recording { meeting_id, .. } => Some(*meeting_id),
            _ => None,
        }
    }

    /// Whether the recording's meeting is shown in the detail (its title
    /// is then visible there and the toolbar doesn't repeat it).
    pub fn set_title_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.title_shown != shown {
            self.title_shown = shown;
            cx.notify();
        }
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
        // Steady: a live dot never pulses (grey while paused). A soft
        // static halo gives it presence without motion.
        let live = design::status_color(design::Status::Live, cx);
        let dot = div()
            .flex_none()
            .size(dp(12.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .when(!paused, |d| d.bg(live.opacity(0.18)))
            .child(div().size(dp(8.)).rounded_full().bg(if paused {
                tone.text_faint
            } else {
                live
            }))
            .into_any_element();
        h_flex()
            .id("meeting-recorder")
            .gap(dp(8.))
            .child(
                h_flex()
                    .id("meeting-recording-title")
                    .h(design::CONTROL_H)
                    .px(dp(8.))
                    .gap(dp(4.))
                    .rounded(design::RADIUS_CONTROL)
                    .hover(move |s| s.bg(tone.raised))
                    .child(dot)
                    .child(
                        design::mono(elapsed)
                            .id("meeting-recorder-time")
                            .text_size(design::TEXT_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if paused { tone.text_muted } else { tone.text })
                            .test_support(),
                    )
                    // The title only where the detail doesn't show it;
                    // "Paused" always (it's the state, not the title).
                    .when(paused || !self.title_shown, |d| {
                        d.child(
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
                    })
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
                    .size(design::CONTROL_H)
                    .icon(crate::ui::brand::pause_icon(paused))
                    .tooltip(if paused {
                        t("meetings.recorder.resume")
                    } else {
                        t("meetings.recorder.pause")
                    })
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .child(
                // Stop: a bordered push button with a filled stop glyph
                // (red is the live dot's job).
                Button::new("meeting-record-toggle")
                    .outline()
                    .accessibility_label(t("meetings.recorder.stopShort"))
                    .child(glyph(self.busy, false, cx))
                    .child(t("meetings.recorder.stopShort"))
                    .tooltip(t("meetings.recorder.stop"))
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
                        .outline()
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
            .gap(dp(8.))
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
                                .px(dp(8.))
                                .gap(dp(4.))
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
                // Record: a bordered push button with a red dot; the yellow
                // fill is for a region's primary action.
                Button::new("meeting-record-toggle")
                    .outline()
                    .accessibility_label(t("meetings.recorder.record"))
                    .child(glyph(self.busy, true, cx))
                    .child(t("meetings.recorder.record"))
                    .tooltip(t("meetings.recorder.start"))
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

/// The Record (red dot) or Stop (filled stop) glyph, or a spinner while
/// busy.
pub(crate) fn glyph(busy: bool, record: bool, cx: &App) -> AnyElement {
    let tone = design::tone(cx);
    if busy {
        return crate::ui::kit::spinner(
            if record {
                "meeting-record-busy"
            } else {
                "meeting-stop-busy"
            },
            icon("loader-circle").xsmall().text_color(tone.text_muted),
        )
        .into_any_element();
    }
    let mark = if record {
        div()
            .size(dp(8.))
            .rounded_full()
            .bg(design::status_color(design::Status::Live, cx))
            .into_any_element()
    } else {
        icon(crate::ui::brand::STOP_ICON)
            .xsmall()
            .text_color(tone.text)
            .into_any_element()
    };
    div()
        .flex_none()
        .size(dp(12.))
        .flex()
        .items_center()
        .justify_center()
        .child(mark)
        .into_any_element()
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
