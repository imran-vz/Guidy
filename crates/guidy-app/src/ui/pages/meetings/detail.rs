//! Meeting detail: header, G2's summary and ticket panes, the speakers
//! with their name pickers, and the transcript grouped by speaker with
//! click-to-play segments.

use super::{meeting_settings, run, toast_error};
use crate::logic::meetings::{
    can_process, duration_minutes, format_meeting_date, format_offset, group_by_speaker,
    needs_confirmation, non_empty, speaker_name, status_key, MeetingsEvent, SpeakerBlock,
};
use crate::playback::Playback;
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{group, icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::{NameSuggestion, PersonRef, SuggestionSource};
use guidy_core::meetings::types::{
    Meeting, MeetingId, MeetingSpeaker, MeetingStatus, Person, ProcessingProgress, Segment,
    SpeakerId,
};
use std::path::PathBuf;
use std::time::Duration;

/// `segment_audio` returns 16 kHz mono.
const SEGMENT_SAMPLE_RATE: u32 = 16_000;

pub enum DetailEvent {
    Back,
    /// The ticket list asked for settings (no Linear key yet).
    OpenSettings,
}

pub struct MeetingDetail {
    pub meeting_id: MeetingId,
    meeting: Option<Meeting>,
    segments: Vec<Segment>,
    speakers: Vec<MeetingSpeaker>,
    people: Vec<Person>,
    loading: bool,
    progress: Option<ProcessingProgress>,
    /// The speaker whose name picker is open.
    picker: Option<SpeakerId>,
    suggestions: Vec<NameSuggestion>,
    suggestions_loading: bool,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    assigning: bool,
    processing_busy: bool,
    ticking: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<DetailEvent> for MeetingDetail {}

impl MeetingDetail {
    pub fn new(meeting_id: MeetingId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetings.speaker.namePlaceholder"))
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetings.speaker.emailPlaceholder"))
        });
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(ev) = ev {
                    this.on_event(ev, cx);
                }
            }),
            cx.subscribe(&name_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::PressEnter { .. } => this.save_new_person(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&email_input, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.save_new_person(cx);
                }
            }),
        ];
        let mut this = Self {
            meeting_id,
            meeting: None,
            segments: Vec::new(),
            speakers: Vec::new(),
            people: Vec::new(),
            loading: true,
            progress: None,
            picker: None,
            suggestions: Vec::new(),
            suggestions_loading: false,
            name_input,
            email_input,
            assigning: false,
            processing_busy: false,
            ticking: false,
            _subs: subs,
        };
        this.load(cx);
        this
    }

    fn on_event(&mut self, ev: &MeetingsEvent, cx: &mut Context<Self>) {
        match ev {
            MeetingsEvent::MeetingChanged(id) if *id == self.meeting_id => self.load(cx),
            MeetingsEvent::PeopleChanged => self.load(cx),
            MeetingsEvent::Progress(p) if p.meeting_id == self.meeting_id => {
                let done = p.fraction >= 1.0;
                self.progress = Some(p.clone());
                if let Some(m) = &mut self.meeting {
                    if !done {
                        m.status = MeetingStatus::Processing;
                    }
                }
                if done {
                    self.load(cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        run(
            cx,
            move |b| -> Result<_, String> {
                let meeting = b.get_meeting(id)?;
                let segments = b.segments(id)?;
                let speakers = b.speakers(id)?;
                let people = b.list_people()?;
                let progress = b.processing_progress(id);
                Ok((meeting, segments, speakers, people, progress))
            },
            |this: &mut Self, r, cx| {
                this.loading = false;
                match r {
                    Ok((meeting, segments, speakers, people, progress)) => {
                        this.meeting = meeting;
                        this.segments = segments;
                        this.speakers = speakers;
                        this.people = people;
                        if progress.is_some() {
                            this.progress = progress;
                        }
                    }
                    Err(e) => toast_error("meetings.detail.loadError", &e, cx),
                }
            },
        );
    }

    #[cfg(test)]
    pub fn speakers(&self) -> &[MeetingSpeaker] {
        &self.speakers
    }

    // -- speaker naming -------------------------------------------------------

    pub fn open_picker(
        &mut self,
        speaker_id: SpeakerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picker = Some(speaker_id);
        self.suggestions.clear();
        self.suggestions_loading = true;
        self.name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.email_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        run(
            cx,
            move |b| b.speaker_suggestions(speaker_id),
            move |this: &mut Self, r, _| {
                if this.picker != Some(speaker_id) {
                    return;
                }
                this.suggestions_loading = false;
                match r {
                    Ok(s) => this.suggestions = s,
                    Err(e) => log::warn!("Failed to load speaker suggestions: {e}"),
                }
            },
        );
    }

    fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        self.suggestions.clear();
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_new_person(
        &mut self,
        name: &str,
        email: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, email) = (name.to_string(), email.to_string());
        self.name_input
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.email_input
            .update(cx, |s, cx| s.set_value(email, window, cx));
    }

    fn save_new_person(&mut self, cx: &mut Context<Self>) {
        let Some(name) = non_empty(&self.name_input.read(cx).value()) else {
            return;
        };
        let email = non_empty(&self.email_input.read(cx).value());
        self.assign(
            PersonRef::New {
                display_name: name,
                email,
            },
            cx,
        );
    }

    fn pick_suggestion(&mut self, s: &NameSuggestion, cx: &mut Context<Self>) {
        let person = match s.person_id {
            Some(id) => PersonRef::Existing(id),
            None => PersonRef::New {
                display_name: s.display_name.clone(),
                email: s.email.clone(),
            },
        };
        self.assign(person, cx);
    }

    fn assign(&mut self, person: PersonRef, cx: &mut Context<Self>) {
        let Some(speaker_id) = self.picker else {
            return;
        };
        if self.assigning {
            return;
        }
        self.assigning = true;
        cx.notify();
        run(
            cx,
            move |b| b.assign_speaker(speaker_id, person),
            |this: &mut Self, r, cx| {
                this.assigning = false;
                match r {
                    Ok(_) => {
                        this.close_picker(cx);
                        this.load(cx);
                    }
                    Err(e) => toast_error("meetings.speaker.assignError", &e, cx),
                }
            },
        );
    }

    fn confirm(&mut self, speaker_id: SpeakerId, cx: &mut Context<Self>) {
        if let Some(s) = self.speakers.iter_mut().find(|s| s.id == speaker_id) {
            s.confirmed = true;
        }
        cx.notify();
        run(
            cx,
            move |b| b.confirm_speaker(speaker_id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.speaker.confirmError", &e, cx);
                }
                this.load(cx);
            },
        );
    }

    fn process(&mut self, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        self.processing_busy = true;
        cx.notify();
        run(
            cx,
            move |b| b.process_meeting(id),
            |this: &mut Self, r, cx| {
                this.processing_busy = false;
                match r {
                    Ok(()) => {
                        if let Some(m) = &mut this.meeting {
                            m.status = MeetingStatus::Processing;
                            m.error = None;
                        }
                    }
                    Err(e) => toast_error("meetings.list.processError", &e, cx),
                }
            },
        );
    }

    // -- G2 panes ---------------------------------------------------------------

    fn render_summary(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let on_seek: super::summary::SeekHandler = std::rc::Rc::new(move |t_ms, _, cx| {
            let _ = weak.update(cx, |this, cx| this.seek_to(t_ms, cx));
        });
        super::summary::render_with_seek(self.meeting_id, on_seek, window, cx)
    }

    fn render_tickets(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tickets = super::tickets::view(self.meeting_id, window, cx);
        let weak = cx.entity().downgrade();
        tickets.update(cx, |v, _| {
            v.set_on_open_settings(move |_, cx| {
                let _ = weak.update(cx, |_, cx| cx.emit(DetailEvent::OpenSettings));
            })
        });
        tickets.into_any_element()
    }

    /// Play the segment containing `t_ms` (or the last one starting before it).
    fn seek_to(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        let seg = self
            .segments
            .iter()
            .find(|s| s.t0_ms <= t_ms && t_ms <= s.t1_ms)
            .or_else(|| self.segments.iter().rev().find(|s| s.t0_ms <= t_ms))
            .or_else(|| self.segments.first())
            .cloned();
        if let Some(seg) = seg {
            self.play(&seg, cx);
        }
    }

    // -- playback -------------------------------------------------------------

    fn clip_key(&self, segment_id: i64) -> PathBuf {
        PathBuf::from(format!("meeting-{}-segment-{segment_id}", self.meeting_id))
    }

    fn play(&mut self, seg: &Segment, cx: &mut Context<Self>) {
        let player = Playback::global();
        let st = player.state();
        let key = self.clip_key(seg.id);
        if st.path.as_ref() == Some(&key) {
            if st.playing {
                player.pause();
            } else {
                player.resume();
            }
            self.ensure_ticking(cx);
            return;
        }
        let (id, track, t0, t1) = (self.meeting_id, seg.track, seg.t0_ms, seg.t1_ms);
        run(
            cx,
            move |b| b.segment_audio(id, track, t0, t1),
            move |this: &mut Self, r, cx| match r {
                Ok(samples) => {
                    Playback::global().play_samples(key, samples, SEGMENT_SAMPLE_RATE);
                    this.ensure_ticking(cx);
                }
                Err(e) => toast_error("meetings.detail.playError", &e, cx),
            },
        );
    }

    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let keep = this
                .update(cx, |this, cx| {
                    cx.notify();
                    let playing = Playback::global().state().playing;
                    if !playing {
                        this.ticking = false;
                    }
                    playing
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    // -- rendering ------------------------------------------------------------

    fn speaker_label(&self, s: &MeetingSpeaker, cx: &App) -> String {
        let settings = meeting_settings(cx);
        speaker_name(s, &self.people, Some(&settings), &t("meetings.detail.you"))
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let back = Button::new("meeting-back")
            .ghost()
            .small()
            .icon(icon("arrow-left"))
            .label(t("meetings.detail.back"))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailEvent::Back)));
        let Some(m) = &self.meeting else {
            return v_flex()
                .gap_2()
                .items_start()
                .child(back)
                .into_any_element();
        };
        let lang = guidy_i18n::language();
        let mut meta = format_meeting_date(m.started_at, &lang);
        if let Some(d) = m.duration_ms() {
            let mins = duration_minutes(d).to_string();
            meta.push_str(" · ");
            meta.push_str(&t_args("meetings.list.minutes", &[("count", &mins)]));
        }
        meta.push_str(" · ");
        meta.push_str(&t(status_key(m.status)));
        let attendees: Vec<String> = m
            .calendar_event
            .as_ref()
            .map(|e| {
                e.attendees
                    .iter()
                    .filter_map(|a| a.name.clone().or_else(|| a.email.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let processing = m.status == MeetingStatus::Processing;
        let fraction = self
            .progress
            .as_ref()
            .map(|p| p.fraction)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let show_process = can_process(m) && m.status != MeetingStatus::Processed;
        v_flex()
            .gap_2()
            .items_start()
            .w_full()
            .child(back)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .id("meeting-detail-title")
                                    .text_lg()
                                    .font_semibold()
                                    .child(m.title.clone())
                                    .test_support(),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(meta),
                            )
                            .when(!attendees.is_empty(), |d| {
                                d.child(div().text_xs().text_color(theme.muted_foreground).child(
                                    format!(
                                        "{}: {}",
                                        t("meetings.detail.attendees"),
                                        attendees.join(", ")
                                    ),
                                ))
                            })
                            .when_some(
                                m.error
                                    .clone()
                                    .filter(|_| m.status == MeetingStatus::Failed),
                                |d, e| d.child(div().text_xs().text_color(theme.danger).child(e)),
                            ),
                    )
                    .when(show_process, |d| {
                        let failed = m.status == MeetingStatus::Failed;
                        d.child(
                            Button::new("meeting-detail-process")
                                .outline()
                                .small()
                                .icon(icon(if failed { "rotate-ccw" } else { "play" }))
                                .label(if failed {
                                    t("meetings.list.retry")
                                } else {
                                    t("meetings.list.process")
                                })
                                .loading(self.processing_busy)
                                .disabled(self.processing_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.process(cx))),
                        )
                    }),
            )
            .when(processing, |d| {
                let stage = self
                    .progress
                    .as_ref()
                    .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                    .unwrap_or_else(|| t("meetings.status.processing"));
                d.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(stage),
                        )
                        .child(div().flex_1().child(
                            Progress::new("meeting-detail-progress").value(fraction * 100.0),
                        )),
                )
            })
            .into_any_element()
    }

    fn render_picker(&self, speaker_id: SpeakerId, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let heading = |key: &str| {
            div()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(t(key))
        };
        let suggestions: AnyElement = if self.suggestions_loading {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("common.loading"))
                .into_any_element()
        } else if self.suggestions.is_empty() {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("meetings.speaker.noSuggestions"))
                .into_any_element()
        } else {
            h_flex()
                .flex_wrap()
                .gap_1()
                .children(self.suggestions.iter().enumerate().map(|(i, s)| {
                    let source = match s.source {
                        SuggestionSource::Attendee => t("meetings.speaker.attendee"),
                        SuggestionSource::KnownPerson => t("meetings.speaker.knownPerson"),
                    };
                    let mut tip = source.to_string();
                    if let Some(e) = &s.email {
                        tip = format!("{tip} · {e}");
                    }
                    if let Some(sim) = s.similarity {
                        tip = format!("{tip} · {:.0}%", sim * 100.0);
                    }
                    let pick = s.clone();
                    Button::new(SharedString::from(format!("speaker-suggestion-{i}")))
                        .outline()
                        .xsmall()
                        .when(s.source == SuggestionSource::Attendee, |b| {
                            b.icon(icon("calendar"))
                        })
                        .label(s.display_name.clone())
                        .tooltip(tip)
                        .disabled(self.assigning)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.pick_suggestion(&pick, cx)),
                        )
                }))
                .into_any_element()
        };
        let can_save = !self.name_input.read(cx).value().trim().is_empty();
        v_flex()
            .id(SharedString::from(format!("speaker-picker-{speaker_id}")))
            .w_full()
            .gap_2()
            .px_3()
            .py_2()
            .bg(theme.background)
            .child(heading("meetings.speaker.suggestions"))
            .child(suggestions)
            .child(heading("meetings.speaker.newPerson"))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.name_input).small()))
                    .child(div().flex_1().child(Input::new(&self.email_input).small()))
                    .child(
                        Button::new("speaker-new-save")
                            .primary()
                            .small()
                            .icon(icon("user-plus"))
                            .label(t("meetings.speaker.save"))
                            .loading(self.assigning)
                            .disabled(!can_save || self.assigning)
                            .on_click(cx.listener(|this, _, _, cx| this.save_new_person(cx))),
                    )
                    .child(
                        Button::new("speaker-picker-cancel")
                            .ghost()
                            .small()
                            .label(t("meetings.speaker.cancel"))
                            .on_click(cx.listener(|this, _, _, cx| this.close_picker(cx))),
                    ),
            )
            .test_support()
            .into_any_element()
    }

    fn render_speakers(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.speakers.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = self
            .speakers
            .iter()
            .map(|s| {
                let sid = s.id;
                let name = self.speaker_label(s, cx);
                let named = s.person_id.is_some() || s.is_user;
                let uncertain = needs_confirmation(s);
                let row = h_flex()
                    .id(SharedString::from(format!("speaker-{sid}")))
                    .w_full()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(div().text_sm().font_semibold().truncate().child(name))
                            .when(named && !s.is_user, |d| {
                                d.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(s.cluster_label.clone()),
                                )
                            })
                            .when(uncertain, |d| {
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("speaker-uncertain-{sid}")))
                                        .px_1p5()
                                        .rounded(theme.radius)
                                        .text_xs()
                                        .text_color(theme.warning)
                                        .bg(theme.warning.opacity(0.12))
                                        .child(match s.match_confidence {
                                            Some(c) => format!(
                                                "{} {:.0}%",
                                                t("meetings.speaker.confirm"),
                                                c * 100.0
                                            ),
                                            None => t("meetings.speaker.confirm").to_string(),
                                        })
                                        .test_support(),
                                )
                            }),
                    )
                    .when(!s.is_user, |d| {
                        d.child(if uncertain {
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "speaker-confirm-{sid}"
                                    )))
                                    .outline()
                                    .xsmall()
                                    .icon(icon("check"))
                                    .label(t("meetings.speaker.confirmAction"))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.confirm(sid, cx)),
                                    ),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "speaker-change-{sid}"
                                    )))
                                    .ghost()
                                    .xsmall()
                                    .label(t("meetings.speaker.change"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_picker(sid, window, cx)
                                    })),
                                )
                                .into_any_element()
                        } else {
                            Button::new(SharedString::from(format!("speaker-name-{sid}")))
                                .ghost()
                                .xsmall()
                                .icon(icon("pencil"))
                                .tooltip(t("meetings.speaker.rename"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_picker(sid, window, cx)
                                }))
                                .into_any_element()
                        })
                    })
                    .test_support();
                v_flex()
                    .w_full()
                    .child(row)
                    .when(self.picker == Some(sid), |d| {
                        d.child(self.render_picker(sid, cx))
                    })
                    .into_any_element()
            })
            .collect();
        Some(group(Some(t("meetings.detail.speakers")), rows, cx).into_any_element())
    }

    fn render_block(&self, i: usize, block: &SpeakerBlock, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let speaker = block
            .speaker_id
            .and_then(|id| self.speakers.iter().find(|s| s.id == id));
        let name = speaker
            .map(|s| self.speaker_label(s, cx))
            .unwrap_or_else(|| "?".into());
        let pickable = speaker.filter(|s| !s.is_user).map(|s| s.id);
        let uncertain = speaker.is_some_and(needs_confirmation);
        let st = Playback::global().state();
        v_flex()
            .id(SharedString::from(format!("block-{i}")))
            .w_full()
            .gap_1()
            .px_3()
            .py_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("block-{i}-speaker")))
                            .text_sm()
                            .font_semibold()
                            .when(pickable.is_some(), |d| {
                                d.cursor_pointer().hover(|s| s.underline())
                            })
                            .child(name)
                            .when_some(pickable, |d, sid| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_picker(sid, window, cx)
                                }))
                            })
                            .test_support(),
                    )
                    .when(uncertain, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.warning)
                                .child(t("meetings.speaker.confirm")),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_family(crate::theme::MONO_FONT)
                            .text_color(theme.muted_foreground)
                            .child(format_offset(block.t0_ms())),
                    ),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .children(block.segments.iter().map(|seg| {
                        let key = self.clip_key(seg.id);
                        let active = st.path.as_ref() == Some(&key);
                        let playing = active && st.playing;
                        let s = seg.clone();
                        h_flex()
                            .id(SharedString::from(format!("segment-{}", seg.id)))
                            .gap_2()
                            .items_start()
                            .px_1()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .when(active, |d| d.bg(theme.primary.opacity(0.1)))
                            .hover(|d| d.bg(theme.accent))
                            .child(
                                div()
                                    .pt_0p5()
                                    .flex_none()
                                    .text_color(if active {
                                        theme.primary
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(icon(if playing { "pause" } else { "play" }).xsmall()),
                            )
                            .child(div().flex_1().text_sm().child(seg.text.clone()))
                            .tooltip({
                                let tip = format!(
                                    "{} · {}",
                                    t("meetings.detail.play"),
                                    format_offset(seg.t0_ms)
                                );
                                move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                        .build(window, cx)
                                }
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.play(&s, cx)))
                            .test_support()
                    })),
            )
            .test_support()
            .into_any_element()
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = if self.loading {
            vec![div()
                .px_4()
                .py_3()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(t("meetings.detail.loading"))
                .into_any_element()]
        } else if self.segments.is_empty() {
            vec![div()
                .id("transcript-empty")
                .px_4()
                .py_3()
                .text_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(t("meetings.detail.empty"))
                .test_support()
                .into_any_element()]
        } else {
            group_by_speaker(&self.segments)
                .iter()
                .enumerate()
                .map(|(i, b)| self.render_block(i, b, cx))
                .collect()
        };
        group(Some(t("meetings.detail.transcript")), rows, cx).into_any_element()
    }
}

impl Render for MeetingDetail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("meeting-detail")
            .w_full()
            .gap_6()
            .child(self.render_header(cx))
            // ---- G2 slot: summary + action items (summary.rs) -------------
            .child(self.render_summary(window, cx))
            // ---- G2 slot: Linear ticket review list (tickets.rs) ----------
            .child(self.render_tickets(window, cx))
            // ---------------------------------------------------------------
            .children(self.render_speakers(cx))
            .child(self.render_transcript(cx))
            .test_support()
    }
}
