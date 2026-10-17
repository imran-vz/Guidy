//! Meeting list: title, date, duration, processing status (with live
//! progress), and open / rename / process / delete actions.

use super::{meeting_model_missing, meeting_settings, render_model_download, run, toast_error};
use crate::logic::meetings::{
    apply_progress, can_process, duration_minutes, format_meeting_date, non_empty, status_key,
    MeetingsEvent,
};
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt, WindowExt,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::types::{Meeting, MeetingId, MeetingStatus, ProcessingProgress};
use std::collections::{HashMap, HashSet};

pub enum MeetingListEvent {
    Open(MeetingId),
}

pub struct MeetingList {
    meetings: Vec<Meeting>,
    loading: bool,
    progress: HashMap<MeetingId, ProcessingProgress>,
    renaming: Option<MeetingId>,
    rename_input: Entity<InputState>,
    busy: HashSet<MeetingId>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<MeetingListEvent> for MeetingList {}

impl MeetingList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let rename_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("meetings.list.titlePlaceholder")));
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(ev) = ev {
                    this.on_event(ev, cx);
                }
            }),
            cx.subscribe(&rename_input, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.save_rename(cx);
                }
            }),
        ];
        let mut this = Self {
            meetings: Vec::new(),
            loading: true,
            progress: HashMap::new(),
            renaming: None,
            rename_input,
            busy: HashSet::new(),
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    fn on_event(&mut self, ev: &MeetingsEvent, cx: &mut Context<Self>) {
        match ev {
            MeetingsEvent::Progress(p) => {
                apply_progress(&mut self.meetings, p);
                let done = p.fraction >= 1.0;
                self.progress.insert(p.meeting_id, p.clone());
                if done {
                    self.reload(cx);
                }
                cx.notify();
            }
            MeetingsEvent::MeetingChanged(_)
            | MeetingsEvent::RecorderState(_)
            | MeetingsEvent::Ready => self.reload(cx),
            _ => {}
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| {
                b.list_meetings().map(|meetings| {
                    let progress: Vec<ProcessingProgress> = meetings
                        .iter()
                        .filter(|m| m.status == MeetingStatus::Processing)
                        .filter_map(|m| b.processing_progress(m.id))
                        .collect();
                    (meetings, progress)
                })
            },
            |this: &mut Self, r, cx| {
                this.loading = false;
                match r {
                    Ok((mut meetings, progress)) => {
                        meetings.sort_by_key(|m| std::cmp::Reverse(m.started_at));
                        this.progress.retain(|id, _| {
                            meetings
                                .iter()
                                .any(|m| m.id == *id && m.status == MeetingStatus::Processing)
                        });
                        for p in progress {
                            this.progress.insert(p.meeting_id, p);
                        }
                        this.meetings = meetings;
                    }
                    Err(e) => toast_error("meetings.list.loadError", &e, cx),
                }
            },
        );
    }

    #[cfg(test)]
    pub fn meeting_count(&self) -> usize {
        self.meetings.len()
    }

    fn start_rename(&mut self, m: &Meeting, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = Some(m.id);
        let title = m.title.clone();
        self.rename_input.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_rename(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.rename_input
            .update(cx, |s, cx| s.set_value(text, window, cx));
    }

    fn save_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.take() else {
            return;
        };
        let Some(title) = non_empty(&self.rename_input.read(cx).value()) else {
            cx.notify();
            return;
        };
        if let Some(m) = self.meetings.iter_mut().find(|m| m.id == id) {
            m.title = title.clone();
        }
        cx.notify();
        run(
            cx,
            move |b| b.rename_meeting(id, &title),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.list.renameError", &e, cx);
                    this.reload(cx);
                }
            },
        );
    }

    fn process(&mut self, id: MeetingId, cx: &mut Context<Self>) {
        self.busy.insert(id);
        cx.notify();
        run(
            cx,
            move |b| b.process_meeting(id),
            move |this: &mut Self, r, cx| {
                this.busy.remove(&id);
                match r {
                    Ok(()) => {
                        if let Some(m) = this.meetings.iter_mut().find(|m| m.id == id) {
                            m.status = MeetingStatus::Processing;
                            m.error = None;
                        }
                    }
                    Err(e) => toast_error("meetings.list.processError", &e, cx),
                }
            },
        );
    }

    fn cancel_processing(&mut self, id: MeetingId, cx: &mut Context<Self>) {
        run(
            cx,
            move |b| b.cancel_processing(id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.list.cancelError", &e, cx);
                }
                this.reload(cx);
            },
        );
    }

    fn confirm_delete(&mut self, m: &Meeting, window: &mut Window, cx: &mut Context<Self>) {
        let id = m.id;
        let message = t_args("meetings.list.deleteConfirm", &[("title", &m.title)]);
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("meetings.list.deleteTitle"))
                .description(message.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| this.delete(id, cx)).ok();
                    true
                })
        });
    }

    pub fn delete(&mut self, id: MeetingId, cx: &mut Context<Self>) {
        self.meetings.retain(|m| m.id != id);
        cx.notify();
        run(
            cx,
            move |b| b.delete_meeting(id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.list.deleteError", &e, cx);
                    this.reload(cx);
                }
            },
        );
    }

    fn render_status(&self, m: &Meeting, cx: &App) -> AnyElement {
        let theme = cx.theme();
        if m.status == MeetingStatus::Processing {
            let p = self.progress.get(&m.id);
            let fraction = p.map(|p| p.fraction).unwrap_or(0.0).clamp(0.0, 1.0);
            let stage = p
                .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                .unwrap_or_else(|| t("meetings.status.processing"));
            return h_flex()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .flex_none()
                        .child(stage),
                )
                .child(
                    div().flex_1().child(
                        Progress::new(SharedString::from(format!("meeting-progress-{}", m.id)))
                            .value(fraction * 100.0),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family(crate::theme::MONO_FONT)
                        .text_color(theme.muted_foreground)
                        .child(format!("{:.0}%", fraction * 100.0)),
                )
                .into_any_element();
        }
        let color = match m.status {
            MeetingStatus::Failed => theme.danger,
            MeetingStatus::Recording => theme.danger,
            MeetingStatus::Processed => theme.success,
            _ => theme.muted_foreground,
        };
        h_flex()
            .gap_1p5()
            .child(div().size(px(6.)).rounded_full().bg(color))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t(status_key(m.status))),
            )
            .when_some(
                m.error
                    .clone()
                    .filter(|_| m.status == MeetingStatus::Failed),
                |d, e| d.child(div().text_xs().text_color(theme.danger).truncate().child(e)),
            )
            .into_any_element()
    }

    fn render_row(&self, m: &Meeting, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = m.id;
        let lang = guidy_i18n::language();
        let busy = self.busy.contains(&id);
        let mut meta = format_meeting_date(m.started_at, &lang);
        if let Some(d) = m.duration_ms() {
            let mins = duration_minutes(d).to_string();
            meta.push_str(" · ");
            meta.push_str(&t_args("meetings.list.minutes", &[("count", &mins)]));
        }
        let icon_btn = |name: &'static str, label: SharedString, suffix: &str| {
            Button::new(SharedString::from(format!("meeting-{suffix}-{id}")))
                .ghost()
                .xsmall()
                .icon(icon(name))
                .tooltip(label)
        };
        let renaming = self.renaming == Some(id);
        let title: AnyElement = if renaming {
            h_flex()
                .gap_1()
                .w_full()
                .child(div().flex_1().child(Input::new(&self.rename_input).small()))
                .child(
                    Button::new(SharedString::from(format!("meeting-rename-save-{id}")))
                        .primary()
                        .xsmall()
                        .label(t("meetings.list.save"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_rename(cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("meeting-rename-cancel-{id}")))
                        .ghost()
                        .xsmall()
                        .label(t("meetings.list.cancel"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.renaming = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("meeting-title-{id}")))
                .text_sm()
                .font_semibold()
                .truncate()
                .cursor_pointer()
                .hover(|s| s.underline())
                .child(m.title.clone())
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(MeetingListEvent::Open(id))))
                .into_any_element()
        };
        let processing = m.status == MeetingStatus::Processing;
        let failed = m.status == MeetingStatus::Failed;
        let (m_rename, m_delete) = (m.clone(), m.clone());
        h_flex()
            .id(SharedString::from(format!("meeting-{id}")))
            .w_full()
            .gap_3()
            .px_3()
            .py_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(title)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(meta),
                    )
                    .child(self.render_status(m, cx)),
            )
            .when(!renaming, |d| {
                d.child(
                    h_flex()
                        .flex_none()
                        .child(
                            icon_btn("chevron-right", t("meetings.list.open"), "open").on_click(
                                cx.listener(move |_, _, _, cx| cx.emit(MeetingListEvent::Open(id))),
                            ),
                        )
                        .child(
                            icon_btn("pencil", t("meetings.list.rename"), "rename").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.start_rename(&m_rename, window, cx)
                                }),
                            ),
                        )
                        .when(processing, |d| {
                            d.child(
                                icon_btn("circle-x", t("meetings.list.cancelProcessing"), "cancel")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.cancel_processing(id, cx)
                                    })),
                            )
                        })
                        .when(can_process(m), |d| {
                            let (ic, label) = if failed || m.status == MeetingStatus::Processed {
                                ("rotate-ccw", t("meetings.list.retry"))
                            } else {
                                ("play", t("meetings.list.process"))
                            };
                            d.child(
                                icon_btn(ic, label, "process")
                                    .loading(busy)
                                    .disabled(busy)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.process(id, cx)),
                                    ),
                            )
                        })
                        .child(
                            icon_btn("trash", t("meetings.list.delete"), "delete")
                                .disabled(m.status == MeetingStatus::Recording)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_delete(&m_delete, window, cx)
                                })),
                        ),
                )
            })
            .test_support()
            .into_any_element()
    }
}

impl Render for MeetingList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let needs_model = meeting_settings(cx).enabled
            || self
                .meetings
                .iter()
                .any(|m| matches!(m.status, MeetingStatus::Recorded | MeetingStatus::Failed));
        let model_banner = meeting_model_missing(cx)
            .filter(|_| needs_model)
            .map(|m| render_model_download("list", &m, cx));
        let content: AnyElement = if self.loading {
            div()
                .px_4()
                .py_3()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(t("meetings.list.loading"))
                .into_any_element()
        } else if self.meetings.is_empty() {
            div()
                .id("meetings-empty")
                .px_4()
                .py_3()
                .text_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(t("meetings.list.empty"))
                .test_support()
                .into_any_element()
        } else {
            let meetings = self.meetings.clone();
            let count = meetings.len();
            v_flex()
                .children(meetings.iter().enumerate().map(|(i, m)| {
                    div()
                        .when(i + 1 < count, |d| d.border_b_1().border_color(theme.border))
                        .child(self.render_row(m, cx))
                }))
                .into_any_element()
        };
        v_flex()
            .id("meetings-list")
            .w_full()
            .gap_1p5()
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child(t("meetings.list.title").to_uppercase()),
            )
            .children(model_banner)
            .child(
                div()
                    .w_full()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(content),
            )
            .test_support()
    }
}
