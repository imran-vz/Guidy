//! Meeting list: title, description, date, duration, processing status
//! (with live progress), and open / rename / title / process / delete
//! actions. A search box above it searches titles, summaries and
//! transcripts; "Generate titles" titles every summarized meeting.

use super::{
    meeting_model_missing, meeting_settings, meetings_backend, render_model_download, run,
    toast_error,
};
use crate::logic::meetings::{
    apply_progress, can_process, duration_minutes, non_empty, progress_label, status_key,
    MeetingsEvent,
};
use crate::logic::models::format_model_size;
use crate::meetings_backend::{error_message, MeetingsError};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::processing::{SpeakerModelStatus, SPEAKER_MODEL_SIZE};
use guidy_core::meetings::recorder::now_ms;
use guidy_core::meetings::search::{MatchField, SearchHit, SearchResults, Snippet};
use guidy_core::meetings::types::{Meeting, MeetingId, MeetingStatus, ProcessingProgress};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

pub enum MeetingListEvent {
    Open(MeetingId),
    /// Open a meeting at a transcript line (a search hit).
    OpenLine {
        meeting_id: MeetingId,
        segment_id: i64,
    },
}

/// Most search hits shown.
const SEARCH_LIMIT: usize = 50;
/// Typing pause before searching.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);

/// "Generate titles" in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BulkTitles {
    pub done: usize,
    pub total: usize,
    /// Cancel was pressed: stops after the meeting being titled.
    pub cancelled: bool,
}

pub struct MeetingList {
    meetings: Vec<Meeting>,
    loading: bool,
    progress: HashMap<MeetingId, ProcessingProgress>,
    renaming: Option<MeetingId>,
    rename_input: Entity<InputState>,
    busy: HashSet<MeetingId>,
    /// The speaker model's state: recordings aren't processed without it.
    speaker_model: Option<SpeakerModelStatus>,
    search_input: Entity<InputState>,
    /// The query the shown results are for ("" = no search: the list).
    query: String,
    /// Bumped per keystroke: a slower, older search's results are dropped.
    search_seq: u64,
    results: Option<SearchResults>,
    searching: bool,
    /// Meetings whose title is being generated.
    titling: HashSet<MeetingId>,
    bulk: Option<BulkTitles>,
    /// How many meetings "Generate titles" would title.
    title_candidates: usize,
    _subs: Vec<Subscription>,
}

impl EventEmitter<MeetingListEvent> for MeetingList {}

impl MeetingList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let rename_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("meetings.list.titlePlaceholder")));
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("meetings.search.placeholder")));
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
            cx.subscribe(&search_input, |this, input, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    let q = input.read(cx).value().to_string();
                    this.set_query(q, cx);
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
            speaker_model: None,
            search_input,
            query: String::new(),
            search_seq: 0,
            results: None,
            searching: false,
            titling: HashSet::new(),
            bulk: None,
            title_candidates: 0,
            _subs: subs,
        };
        this.reload(cx);
        run(
            cx,
            |b| b.speaker_model_status(),
            // Unless an event or a click got here first (newer).
            |this: &mut Self, s, _| {
                this.speaker_model.get_or_insert(s);
            },
        );
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
            // Bulk titling changes one meeting after another: it updates
            // their rows itself and reloads once it's done.
            MeetingsEvent::MeetingChanged(_) if self.bulk.is_some() => {}
            MeetingsEvent::MeetingChanged(_)
            | MeetingsEvent::RecorderState(_)
            | MeetingsEvent::Ready => self.reload(cx),
            MeetingsEvent::SpeakerModelStatus(s) => {
                self.speaker_model = Some(s.clone());
                cx.notify();
            }
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
                    let candidates = b.title_candidates().map(|c| c.len()).unwrap_or(0);
                    (meetings, progress, candidates)
                })
            },
            |this: &mut Self, r, cx| {
                this.loading = false;
                match r {
                    Ok((mut meetings, progress, candidates)) => {
                        this.title_candidates = candidates;
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
        // A changed meeting can change what a search finds.
        if !self.query.trim().is_empty() {
            self.search(Duration::ZERO, cx);
        }
    }

    // -- search -------------------------------------------------------------

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if query == self.query {
            return;
        }
        self.query = query;
        if self.query.trim().is_empty() {
            self.search_seq += 1;
            self.results = None;
            self.searching = false;
            cx.notify();
            return;
        }
        self.search(SEARCH_DEBOUNCE, cx);
    }

    #[cfg(test)]
    pub fn type_search(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.search_input
            .update(cx, |s, cx| s.set_value(text.clone(), window, cx));
        self.set_query(text, cx);
    }

    #[cfg(test)]
    pub fn results(&self) -> Option<&SearchResults> {
        self.results.as_ref()
    }

    /// Search for the current query after `delay`, unless the query changes
    /// meanwhile.
    fn search(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.search_seq += 1;
        let seq = self.search_seq;
        let query = self.query.clone();
        self.searching = true;
        cx.notify();
        let backend = meetings_backend(cx);
        cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
                let current = this.update(cx, |this, _| this.search_seq == seq);
                if !matches!(current, Ok(true)) {
                    return;
                }
            }
            let r = cx
                .background_spawn(async move { backend.search_meetings(&query, SEARCH_LIMIT) })
                .await;
            this.update(cx, |this, cx| {
                if this.search_seq != seq {
                    return;
                }
                this.searching = false;
                match r {
                    Ok(results) => this.results = Some(results),
                    Err(e) => {
                        this.results = Some(SearchResults::default());
                        toast_error("meetings.search.error", &e, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -- titles -------------------------------------------------------------

    /// Put a meeting the backend returned into the list.
    fn replace_meeting(&mut self, meeting: Meeting) {
        if let Some(m) = self.meetings.iter_mut().find(|m| m.id == meeting.id) {
            *m = meeting;
        }
    }

    pub fn generate_title(&mut self, id: MeetingId, cx: &mut Context<Self>) {
        if !self.titling.insert(id) {
            return;
        }
        cx.notify();
        run(
            cx,
            move |b| b.generate_title(id, false),
            move |this: &mut Self, r, cx| {
                this.titling.remove(&id);
                match r {
                    Ok(outcome) => this.replace_meeting(outcome.meeting),
                    Err(e) if MeetingsError::parse(&e) == MeetingsError::Cancelled => {}
                    Err(e) => toast_error("meetings.titles.error", &e, cx),
                }
                this.reload(cx);
            },
        );
    }

    /// Title every meeting that needs it, one after another.
    pub fn generate_all_titles(&mut self, cx: &mut Context<Self>) {
        if self.bulk.is_some() {
            return;
        }
        self.bulk = Some(BulkTitles::default());
        cx.notify();
        let backend = meetings_backend(cx);
        cx.spawn(async move |this, cx| {
            let b = backend.clone();
            let ids = match cx
                .background_spawn(async move { b.title_candidates() })
                .await
            {
                Ok(ids) => ids,
                Err(e) => {
                    this.update(cx, |this, cx| {
                        this.bulk = None;
                        toast_error("meetings.titles.error", &e, cx);
                    })
                    .ok();
                    return;
                }
            };
            let total = ids.len();
            let mut titled = 0;
            let mut failure = None;
            for id in ids {
                let go = this.update(cx, |this, cx| {
                    let Some(bulk) = &mut this.bulk else {
                        return false;
                    };
                    bulk.total = total;
                    if bulk.cancelled {
                        return false;
                    }
                    this.titling.insert(id);
                    cx.notify();
                    true
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
                let b = backend.clone();
                let r = cx
                    .background_spawn(async move { b.generate_title(id, true) })
                    .await;
                let go = this.update(cx, |this, cx| {
                    this.titling.remove(&id);
                    if let Some(bulk) = &mut this.bulk {
                        bulk.done += 1;
                    }
                    cx.notify();
                    match r {
                        Ok(outcome) => {
                            titled += usize::from(outcome.generated);
                            this.replace_meeting(outcome.meeting);
                            true
                        }
                        // Stopped, or no summary any more: go on.
                        Err(e) => match MeetingsError::parse(&e) {
                            MeetingsError::Cancelled => false,
                            MeetingsError::TitleNeedsSummary => true,
                            // The model failed: it would for the rest too.
                            _ => {
                                failure = Some(e);
                                false
                            }
                        },
                    }
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                this.bulk = None;
                match failure {
                    Some(e) => toast_error("meetings.titles.error", &e, cx),
                    None if titled > 0 => {
                        let n = titled.to_string();
                        let spec = crate::logic::settings_store::ToastSpec {
                            kind: crate::logic::settings_store::ToastKind::Success,
                            title: t_args("meetings.titles.done", &[("count", &n)]).into(),
                            description: None,
                        };
                        crate::state::AppState::global(cx).update(cx, |s, cx| s.toast(spec, cx));
                    }
                    None => {}
                }
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn cancel_titles(&mut self, cx: &mut Context<Self>) {
        if let Some(bulk) = &mut self.bulk {
            bulk.cancelled = true;
        }
        cx.notify();
        run(cx, |b| b.cancel_title_generation(), |_: &mut Self, _, _| {});
    }

    #[cfg(test)]
    pub fn bulk(&self) -> Option<&BulkTitles> {
        self.bulk.as_ref()
    }

    fn download_speaker_model(&mut self, cx: &mut Context<Self>) {
        self.speaker_model = Some(SpeakerModelStatus::Downloading {
            downloaded: 0,
            total: 0,
        });
        cx.notify();
        run(
            cx,
            |b| b.download_speaker_model(),
            |this: &mut Self, r, _| {
                if let Err(e) = r {
                    this.speaker_model = Some(SpeakerModelStatus::Failed {
                        error: error_message(&e),
                    });
                }
            },
        );
    }

    /// "Download the speaker model" while it's missing: without it nothing
    /// is processed (it downloads only when asked).
    fn render_speaker_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.speaker_model.as_ref()?;
        let tone = design::tone(cx);
        let failed = matches!(status, SpeakerModelStatus::Failed { .. });
        let action: AnyElement = match status {
            SpeakerModelStatus::Ready => return None,
            SpeakerModelStatus::Downloading { downloaded, total } => {
                let pct = if *total > 0 {
                    (*downloaded as f64 / *total as f64 * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                design::mono(format!("{pct:.0}%"))
                    .text_color(tone.text_muted)
                    .into_any_element()
            }
            SpeakerModelStatus::Missing | SpeakerModelStatus::Failed { .. } => {
                let size = format_model_size(Some(SPEAKER_MODEL_SIZE as f64 / (1024.0 * 1024.0)));
                Button::new("meetings-list-speaker-model-download")
                    .primary()
                    .small()
                    .pill()
                    .icon(icon(if failed { "rotate-ccw" } else { "download" }))
                    .label(if failed {
                        t("meetings.settings.speakerModel.retry")
                    } else {
                        t_args(
                            "meetings.settings.speakerModel.download",
                            &[("size", &size)],
                        )
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.download_speaker_model(cx)))
                    .into_any_element()
            }
        };
        let message = match status {
            SpeakerModelStatus::Failed { error } => {
                t_args("meetings.settings.speakerModel.failed", &[("error", error)])
            }
            _ => t("meetings.list.speakerModelMissing"),
        };
        let theme = cx.theme().clone();
        Some(
            design::notice(theme.warning)
                .id("meetings-speaker-model-missing-list")
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
                .child(action)
                .test_support()
                .into_any_element(),
        )
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
        let tone = design::tone(cx);
        if m.status == MeetingStatus::Processing {
            let p = self.progress.get(&m.id);
            let fraction = p.map(|p| p.fraction).unwrap_or(0.0).clamp(0.0, 1.0);
            let stage = p
                .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                .unwrap_or_else(|| t("meetings.status.processing"));
            return v_flex()
                .w_full()
                .gap(dp(8.))
                .child(
                    h_flex().child(design::chip("", true, cx).child(design::shimmer_text(
                        SharedString::from(format!("meeting-stage-{}", m.id)),
                        progress_label(&stage, p),
                        cx,
                    ))),
                )
                .child(design::track(
                    SharedString::from(format!("meeting-progress-{}", m.id)),
                    Some(fraction),
                    cx,
                ))
                .into_any_element();
        }
        let failed = m.status == MeetingStatus::Failed;
        let live = m.status == MeetingStatus::Recording;
        h_flex()
            .gap(dp(8.))
            .min_w_0()
            .child(
                design::chip(t(status_key(m.status)), live, cx).when(failed, |d| {
                    d.bg(theme.danger.opacity(0.12)).text_color(theme.danger)
                }),
            )
            .when_some(m.error.clone().filter(|_| failed), |d, e| {
                d.child(
                    div()
                        .min_w_0()
                        .text_size(dp(12.))
                        .text_color(tone.text_muted)
                        .truncate()
                        .child(e),
                )
            })
            .into_any_element()
    }

    fn render_row(&self, m: &Meeting, cx: &mut Context<Self>) -> AnyElement {
        let id = m.id;
        let lang = guidy_i18n::language();
        let busy = self.busy.contains(&id);
        let mut meta = format!(
            "{} · {}",
            crate::logic::meetings::day_label(m.started_at, now_ms(), &lang),
            super::clock_time(m.started_at)
        );
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
                        .pill()
                        .label(t("meetings.list.save"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_rename(cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("meeting-rename-cancel-{id}")))
                        .ghost()
                        .xsmall()
                        .pill()
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
                .text_size(design::TEXT_BODY)
                .font_weight(FontWeight::MEDIUM)
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
            .pl(design::SECTION_X)
            .pr(dp(10.))
            .py(dp(12.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(6.))
                    .child(title)
                    .when_some(m.description.clone().filter(|_| !renaming), |d, text| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("meeting-description-{id}")))
                                .text_size(dp(12.5))
                                .text_color(design::tone(cx).text_muted)
                                .line_clamp(2)
                                .child(text)
                                .test_support(),
                        )
                    })
                    .child(design::eyebrow(meta, cx))
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
                        .child({
                            let titling = self.titling.contains(&id);
                            icon_btn(
                                "sparkles",
                                if m.has_summary {
                                    t("meetings.titles.generate")
                                } else {
                                    t("meetings.titles.needsSummary")
                                },
                                "title",
                            )
                            .loading(titling)
                            .disabled(titling || !m.has_summary || self.bulk.is_some())
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.generate_title(id, cx)),
                            )
                        })
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

    /// "Generate titles", or its progress and Cancel.
    fn render_bulk_titles(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tone = design::tone(cx);
        if let Some(bulk) = &self.bulk {
            let (done, total) = (bulk.done.to_string(), bulk.total.to_string());
            return Some(
                h_flex()
                    .id("meetings-titles-progress")
                    .gap_2()
                    .items_center()
                    .child(div().text_size(dp(12.)).text_color(tone.text_muted).child(
                        design::shimmer_text(
                            "meetings-titles-progress-text",
                            if bulk.total == 0 {
                                t("meetings.titles.starting")
                            } else {
                                t_args(
                                    "meetings.titles.progress",
                                    &[("done", &done), ("total", &total)],
                                )
                            },
                            cx,
                        ),
                    ))
                    .child(
                        Button::new("meetings-titles-cancel")
                            .ghost()
                            .xsmall()
                            .pill()
                            .label(t("meetings.list.cancel"))
                            .disabled(bulk.cancelled)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_titles(cx))),
                    )
                    .test_support()
                    .into_any_element(),
            );
        }
        (self.title_candidates > 0).then(|| {
            let n = self.title_candidates.to_string();
            Button::new("meetings-titles-generate")
                .ghost()
                .small()
                .pill()
                .icon(icon("sparkles"))
                .label(t_args("meetings.titles.generateAll", &[("count", &n)]))
                .tooltip(t("meetings.titles.generateAllHint"))
                .on_click(cx.listener(|this, _, _, cx| this.generate_all_titles(cx)))
                .into_any_element()
        })
    }

    fn render_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let note = |text: SharedString| {
            div()
                .px(design::SECTION_X)
                .py(dp(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(text)
        };
        let Some(results) = &self.results else {
            return note(t("meetings.search.searching"))
                .id("meetings-search-pending")
                .test_support()
                .into_any_element();
        };
        let hits: Vec<&SearchHit> = results
            .hits
            .iter()
            .filter(|h| self.meetings.iter().any(|m| m.id == h.meeting_id))
            .collect();
        if hits.is_empty() {
            return note(t_args(
                "meetings.search.empty",
                &[("query", self.query.trim())],
            ))
            .id("meetings-search-empty")
            .test_support()
            .into_any_element();
        }
        let mut notes: Vec<SharedString> = Vec::new();
        if !results.corrections.is_empty() {
            let words: Vec<&str> = results.corrections.iter().map(|c| c.1.as_str()).collect();
            notes.push(t_args(
                "meetings.search.corrected",
                &[("words", &words.join(", "))],
            ));
        }
        if results.partial {
            notes.push(t("meetings.search.partial"));
        }
        let count = hits.len();
        let n = count.to_string();
        v_flex()
            .id("meetings-search-results")
            .child(
                h_flex()
                    .px(design::SECTION_X)
                    .py(dp(10.))
                    .gap_3()
                    .items_center()
                    .border_b_1()
                    .border_color(tone.hairline)
                    .child(design::eyebrow(
                        t_args("meetings.search.count", &[("count", &n)]),
                        cx,
                    ))
                    .when(self.searching, |d| {
                        d.child(crate::ui::kit::spinner(
                            "meetings-search-spinner",
                            icon("loader-circle").xsmall(),
                        ))
                    })
                    .children(notes.into_iter().map(|text| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(dp(12.))
                            .text_color(tone.text_faint)
                            .child(text)
                    })),
            )
            .children(hits.iter().enumerate().map(|(i, hit)| {
                div()
                    .when(i + 1 < count, |d| {
                        d.border_b_1().border_color(tone.hairline)
                    })
                    .children(self.render_hit(hit, cx))
            }))
            .test_support()
            .into_any_element()
    }

    fn render_hit(&self, hit: &SearchHit, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.meetings.iter().find(|m| m.id == hit.meeting_id)?;
        let id = m.id;
        let tone = design::tone(cx);
        let lang = guidy_i18n::language();
        let mut meta = format!(
            "{} · {}",
            crate::logic::meetings::day_label(m.started_at, now_ms(), &lang),
            super::clock_time(m.started_at)
        );
        if hit.transcript_hits > 1 {
            let n = hit.transcript_hits.to_string();
            meta.push_str(" · ");
            meta.push_str(&t_args("meetings.search.mentions", &[("count", &n)]));
        }
        let title: AnyElement = match &hit.title {
            Some(sn) => div()
                .child(highlighted(&sn.text, &sn.highlights, cx))
                .into_any_element(),
            None => div().child(m.title.clone()).into_any_element(),
        };
        let shows_description = hit
            .snippets
            .iter()
            .any(|s| s.field == MatchField::Description);
        let snippets: Vec<AnyElement> = hit
            .snippets
            .iter()
            .enumerate()
            .map(|(i, sn)| self.render_snippet(id, i, sn, cx))
            .collect();
        Some(
            v_flex()
                .id(SharedString::from(format!("search-hit-{id}")))
                .w_full()
                .gap(dp(6.))
                .px(design::SECTION_X)
                .py(dp(12.))
                .cursor_pointer()
                .hover(|s| s.bg(tone.raised))
                .child(
                    div()
                        .text_size(design::TEXT_BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(title),
                )
                .when_some(
                    m.description.clone().filter(|_| !shows_description),
                    |d, text| {
                        d.child(
                            div()
                                .text_size(dp(12.5))
                                .text_color(tone.text_muted)
                                .truncate()
                                .child(text),
                        )
                    },
                )
                .child(design::eyebrow(meta, cx))
                .children(snippets)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(MeetingListEvent::Open(id))))
                .test_support()
                .into_any_element(),
        )
    }

    fn render_snippet(
        &self,
        meeting_id: MeetingId,
        i: usize,
        sn: &Snippet,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let label: SharedString = match sn.field {
            MatchField::Transcript => sn
                .t_ms
                .map(super::summary::format_timestamp)
                .unwrap_or_default()
                .into(),
            MatchField::Summary => t("meetings.search.inSummary"),
            MatchField::Description => t("meetings.search.inDescription"),
            MatchField::Title => t("meetings.search.inTitle"),
        };
        let line = sn.segment_id.filter(|_| sn.field == MatchField::Transcript);
        h_flex()
            .id(SharedString::from(format!("search-hit-{meeting_id}-{i}")))
            .w_full()
            .gap_2()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .min_w(dp(44.))
                    .child(design::chip(label, line.is_some(), cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(dp(12.5))
                    .text_color(tone.text_muted)
                    .line_clamp(2)
                    .child(highlighted(&sn.text, &sn.highlights, cx)),
            )
            .when_some(line, |d, segment_id| {
                d.on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(MeetingListEvent::OpenLine {
                        meeting_id,
                        segment_id,
                    });
                }))
            })
            .test_support()
            .into_any_element()
    }
}

/// `text` with `ranges` (search matches) emphasized.
fn highlighted(text: &str, ranges: &[Range<usize>], cx: &App) -> StyledText {
    let tone = design::tone(cx);
    let style = HighlightStyle {
        color: Some(tone.accent_text),
        background_color: Some(tone.accent_soft),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let ranges: Vec<(Range<usize>, HighlightStyle)> = ranges
        .iter()
        .filter(|r| {
            r.start < r.end
                && r.end <= text.len()
                && text.is_char_boundary(r.start)
                && text.is_char_boundary(r.end)
        })
        .map(|r| (r.clone(), style))
        .collect();
    StyledText::new(text.to_string()).with_highlights(ranges)
}

impl Render for MeetingList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let needs_model = meeting_settings(cx).enabled
            || self
                .meetings
                .iter()
                .any(|m| matches!(m.status, MeetingStatus::Recorded | MeetingStatus::Failed));
        let model_banner = meeting_model_missing(cx)
            .filter(|_| needs_model)
            .map(|m| render_model_download("list", &m, cx));
        let speaker_banner = needs_model
            .then(|| self.render_speaker_banner(cx))
            .flatten();
        let notice = |text: SharedString| {
            div()
                .px(design::SECTION_X)
                .py(dp(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(text)
        };
        let searching = !self.query.trim().is_empty();
        let content: AnyElement = if searching {
            self.render_results(cx)
        } else if self.loading {
            notice(t("meetings.list.loading")).into_any_element()
        } else if self.meetings.is_empty() {
            notice(t("meetings.list.empty"))
                .id("meetings-empty")
                .test_support()
                .into_any_element()
        } else {
            let meetings = self.meetings.clone();
            let count = meetings.len();
            v_flex()
                .children(meetings.iter().enumerate().map(|(i, m)| {
                    div()
                        .when(i + 1 < count, |d| {
                            d.border_b_1().border_color(tone.hairline)
                        })
                        .child(self.render_row(m, cx))
                }))
                .into_any_element()
        };
        v_flex()
            .id("meetings-list")
            .w_full()
            .gap(dp(8.))
            .child(
                h_flex()
                    .w_full()
                    .min_h(dp(28.))
                    .px(dp(2.))
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(design::eyebrow(t("meetings.list.title"), cx))
                    .children(self.render_bulk_titles(cx)),
            )
            .child(
                Input::new(&self.search_input)
                    .prefix(icon("search").small().text_color(tone.text_muted))
                    .cleanable(true),
            )
            .children(model_banner)
            .children(speaker_banner)
            .child(design::panel(cx).child(content))
            .test_support()
    }
}
