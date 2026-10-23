//! Meeting detail: header, G2's summary and ticket panes, the speakers
//! with their name pickers, and the transcript grouped by speaker with
//! click-to-play segments.

use super::{meeting_model_missing, meeting_settings, render_model_download, run, toast_error};
use crate::logic::meetings::{
    can_process, cluster_label, format_meeting_date, format_offset, group_by_speaker,
    needs_confirmation, non_empty, speaker_name, status_key, talk_time, transcript_issue,
    MeetingsEvent, RedoChoice, SpeakerBlock, SpeakerRedoChoice, TalkTime, TranscriptIssue,
};
use crate::playback::Playback;
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp, Pill, Stat};
use crate::ui::kit::{crumb_title, group, icon, t, t_args};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::{NameSuggestion, PersonRef, SuggestionSource};
use guidy_core::meetings::types::{
    CloudTranscriptionOption, DiarizationOptions, LiveStatus, Meeting, MeetingId, MeetingSpeaker,
    MeetingStatus, Person, ProcessingProgress, Segment, SpeakerId, Track, TranscriptionEngine,
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

/// What the name picker names: a whole speaker, or some lines (an unknown
/// speaker's turn, or one line the diarization got wrong).
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    Speaker(SpeakerId),
    Lines(Vec<i64>),
}

pub struct MeetingDetail {
    pub meeting_id: MeetingId,
    meeting: Option<Meeting>,
    segments: Vec<Segment>,
    /// Derived from `segments` on load, for the stats and talk-time chart.
    talk: TalkTime,
    speakers: Vec<MeetingSpeaker>,
    people: Vec<Person>,
    loading: bool,
    progress: Option<ProcessingProgress>,
    /// The speaker whose name picker is open.
    picker: Option<PickTarget>,
    suggestions: Vec<NameSuggestion>,
    suggestions_loading: bool,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    assigning: bool,
    processing_busy: bool,
    cancelling: bool,
    /// Deep link: seek and play here once the transcript has loaded.
    pending_seek: Option<i64>,
    /// Cloud providers the transcript can be redone with.
    cloud_options: Vec<CloudTranscriptionOption>,
    /// What "Redo speakers" can use.
    diarization: DiarizationOptions,
    /// "Copy transcript" just copied (shows a check for a moment).
    transcript_copied: bool,
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
        let mut subs = vec![
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
        // The live transcript while this meeting records.
        if let Some(widget) = crate::ui::recording_widget::RecordingWidgetState::global(cx) {
            subs.push(cx.observe(&widget, |this, w, cx| {
                if w.read(cx).model.live.meeting_id == this.meeting_id {
                    cx.notify();
                }
            }));
        }
        let mut this = Self {
            meeting_id,
            meeting: None,
            segments: Vec::new(),
            talk: talk_time(&[], None),
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
            cancelling: false,
            pending_seek: None,
            cloud_options: Vec::new(),
            diarization: DiarizationOptions::default(),
            transcript_copied: false,
            ticking: false,
            _subs: subs,
        };
        this.load(cx);
        this
    }

    fn on_event(&mut self, ev: &MeetingsEvent, cx: &mut Context<Self>) {
        match ev {
            MeetingsEvent::MeetingChanged(id) if *id == self.meeting_id => self.load(cx),
            MeetingsEvent::PeopleChanged | MeetingsEvent::Ready => self.load(cx),
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
                let cloud = b.cloud_transcription_options();
                let diarization = b.diarization_options();
                Ok((
                    meeting,
                    segments,
                    speakers,
                    people,
                    progress,
                    (cloud, diarization),
                ))
            },
            |this: &mut Self, r, cx| {
                this.loading = false;
                match r {
                    Ok((meeting, segments, speakers, people, progress, (cloud, diarization))) => {
                        this.cloud_options = cloud;
                        this.diarization = diarization;
                        this.talk =
                            talk_time(&segments, meeting.as_ref().and_then(|m| m.duration_ms()));
                        this.meeting = meeting;
                        this.segments = segments;
                        this.speakers = speakers;
                        this.people = people;
                        if progress.is_some() {
                            this.progress = progress;
                        }
                        if !this.segments.is_empty() {
                            if let Some(t_ms) = this.pending_seek.take() {
                                this.seek_to(t_ms, cx);
                            }
                        }
                    }
                    Err(e) => toast_error("meetings.detail.loadError", &e, cx),
                }
            },
        );
    }

    /// Seek and play at `t_ms` now, or once the transcript has loaded.
    pub fn seek_when_loaded(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        if self.loading || self.segments.is_empty() {
            self.pending_seek = Some(t_ms);
        } else {
            self.seek_to(t_ms, cx);
        }
    }

    fn cancel_processing(&mut self, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        self.cancelling = true;
        cx.notify();
        run(
            cx,
            move |b| b.cancel_processing(id),
            |this: &mut Self, r, cx| {
                this.cancelling = false;
                if let Err(e) = r {
                    toast_error("meetings.list.cancelError", &e, cx);
                }
                this.load(cx);
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
        self.picker = Some(PickTarget::Speaker(speaker_id));
        self.suggestions.clear();
        self.suggestions_loading = true;
        self.name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.email_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        let target = PickTarget::Speaker(speaker_id);
        run(
            cx,
            move |b| b.speaker_suggestions(speaker_id),
            move |this: &mut Self, r, _| {
                if this.picker.as_ref() != Some(&target) {
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

    /// Name the speaker of `segment_ids` (one line, or an unknown turn),
    /// suggesting people by the first line's voice.
    pub fn open_line_picker(
        &mut self,
        segment_ids: Vec<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(&first) = segment_ids.first() else {
            return;
        };
        let target = PickTarget::Lines(segment_ids);
        self.picker = Some(target.clone());
        self.suggestions.clear();
        self.suggestions_loading = true;
        self.name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.email_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        run(
            cx,
            move |b| b.segment_suggestions(first),
            move |this: &mut Self, r, _| {
                if this.picker.as_ref() != Some(&target) {
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
        let Some(target) = self.picker.clone() else {
            return;
        };
        if self.assigning {
            return;
        }
        self.assigning = true;
        cx.notify();
        run(
            cx,
            move |b| match target {
                PickTarget::Speaker(speaker_id) => b.assign_speaker(speaker_id, person),
                PickTarget::Lines(ids) => {
                    // A new person is created once, then each line joins them.
                    let mut person = person;
                    let mut id = None;
                    for segment_id in ids {
                        let pid = b.assign_segment(segment_id, person.clone())?;
                        person = PersonRef::Existing(pid);
                        id = Some(pid);
                    }
                    id.ok_or_else(|| "no lines to name".to_string())
                }
            },
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

    /// Whether the transcript can be redone: the meeting is done (or failed)
    /// and its audio is still there.
    fn can_redo(&self) -> bool {
        self.meeting.as_ref().is_some_and(|m| {
            matches!(m.status, MeetingStatus::Processed | MeetingStatus::Failed) && can_process(m)
        })
    }

    /// Transcribe the meeting again with `engine`.
    fn redo(&mut self, engine: TranscriptionEngine, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        self.processing_busy = true;
        cx.notify();
        run(
            cx,
            move |b| b.retranscribe(id, engine),
            |this: &mut Self, r, cx| {
                this.processing_busy = false;
                match r {
                    Ok(()) => {
                        if let Some(m) = &mut this.meeting {
                            m.status = MeetingStatus::Processing;
                            m.error = None;
                        }
                    }
                    Err(e) => toast_error("meetings.redo.error", &e, cx),
                }
            },
        );
    }

    /// The "Redo transcript" choices: the downloaded models, then the
    /// cloud providers.
    pub(crate) fn redo_choices(&self, cx: &App) -> Vec<RedoChoice> {
        let minutes = self
            .meeting
            .as_ref()
            .and_then(|m| m.duration_ms())
            .map(|ms| ms as f64 / 60_000.0);
        crate::logic::meetings::redo_choices(
            &super::local_model_choices(cx),
            &self.cloud_options,
            minutes,
            &t("meetings.redo.noKeyShort"),
        )
    }

    /// Redo with `choice`: right away on this Mac; a cloud provider asks
    /// first, as it uploads the audio and costs money.
    pub(crate) fn choose_redo(
        &mut self,
        choice: RedoChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = choice.engine.clone() else {
            return;
        };
        if !choice.cloud {
            self.redo(engine, cx);
            return;
        }
        let weak = cx.entity().downgrade();
        let description = match &choice.cost {
            Some(cost) => t_args(
                "meetings.redo.confirmCloud",
                &[
                    ("provider", &choice.provider),
                    ("model", &choice.model),
                    ("cost", cost),
                ],
            ),
            None => t_args(
                "meetings.redo.confirmCloudUnpriced",
                &[("provider", &choice.provider), ("model", &choice.model)],
            ),
        };
        let title = t_args(
            "meetings.redo.confirmTitle",
            &[("provider", &choice.provider)],
        );
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            let engine = engine.clone();
            alert
                .title(title.clone())
                .description(description.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    let engine = engine.clone();
                    weak.update(cx, |this, cx| this.redo(engine, cx)).ok();
                    true
                })
        });
    }

    /// The "Redo speakers" choices.
    pub(crate) fn speaker_redo_choices(&self) -> Vec<SpeakerRedoChoice> {
        let minutes = self
            .meeting
            .as_ref()
            .and_then(|m| m.duration_ms())
            .map(|ms| ms as f64 / 60_000.0);
        crate::logic::meetings::speaker_redo_choices(
            &self.diarization,
            minutes,
            &t("meetings.speakersRedo.accurate"),
            &t("meetings.speakersRedo.standard"),
            &t("meetings.speakersRedo.download"),
            &t("meetings.redo.noKeyShort"),
        )
    }

    /// Redo the speakers with `choice` (Deepgram asks first: it uploads
    /// the audio).
    pub(crate) fn choose_speaker_redo(
        &mut self,
        choice: SpeakerRedoChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !choice.enabled {
            return;
        }
        let engine = choice.engine;
        let run_it = move |this: &mut Self, cx: &mut Context<Self>| {
            let id = this.meeting_id;
            this.processing_busy = true;
            cx.notify();
            run(
                cx,
                move |b| b.redo_speakers(id, engine),
                |this: &mut Self, r, cx| {
                    this.processing_busy = false;
                    match r {
                        Ok(()) => {
                            if let Some(m) = &mut this.meeting {
                                m.status = MeetingStatus::Processing;
                            }
                        }
                        Err(e) => toast_error("meetings.speakersRedo.error", &e, cx),
                    }
                },
            );
        };
        if !choice.cloud {
            run_it(self, cx);
            return;
        }
        let weak = cx.entity().downgrade();
        let description = match &choice.cost {
            Some(cost) => t_args("meetings.speakersRedo.confirmDeepgram", &[("cost", cost)]),
            None => t("meetings.speakersRedo.confirmDeepgramUnpriced"),
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let weak = weak.clone();
            alert
                .title(t("meetings.speakersRedo.confirmTitle"))
                .description(description.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    weak.update(cx, |this, cx| run_it(this, cx)).ok();
                    true
                })
        });
    }

    /// "Redo speakers" with the accurate model, and a menu of the others.
    fn speakers_button(&self, cx: &mut Context<Self>) -> DropdownButton {
        let choices = self.speaker_redo_choices();
        let busy = self.processing_busy;
        let main = choices[0].clone();
        let button = Button::new("speakers-redo")
            .ghost()
            .small()
            .pill()
            .icon(icon("users"))
            .label(t("meetings.speakersRedo.action"))
            .tooltip(t_args(
                "meetings.speakersRedo.tooltip",
                &[("model", &main.label)],
            ))
            .disabled(busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.choose_speaker_redo(main.clone(), window, cx)
            }));
        let weak = cx.entity().downgrade();
        DropdownButton::new("speakers-redo-split")
            .button(button)
            .disabled(busy)
            .menu_accessibility_label(t("meetings.redo.more"))
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.label(t("meetings.redo.localTitle"));
                for c in &choices {
                    if c.cloud {
                        menu = menu.separator().label(t("meetings.redo.cloudTitle"));
                    }
                    let choice = c.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(c.label.clone())
                            .disabled(!c.enabled)
                            .on_click(move |_, window, cx| {
                                let choice = choice.clone();
                                weak.update(cx, |this, cx| {
                                    this.choose_speaker_redo(choice, window, cx)
                                })
                                .ok();
                            }),
                    );
                }
                menu
            })
    }

    /// "Redo transcript" with the current meeting model, and a menu of the
    /// other models and the cloud providers. `primary` styles it as the
    /// suggested action.
    fn redo_button(
        &self,
        id: &'static str,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> DropdownButton {
        let choices = self.redo_choices(cx);
        let current = choices.iter().find(|c| c.current && !c.cloud).cloned();
        let busy = self.processing_busy;
        let main = Button::new(id)
            .small()
            .pill()
            .icon(icon("rotate-ccw"))
            .label(t("meetings.redo.action"))
            .map(|b| if primary { b.primary() } else { b.secondary() })
            .when_some(current.clone(), |b, c| {
                b.tooltip(t_args("meetings.redo.tooltip", &[("model", &c.label)]))
            })
            .disabled(busy || current.is_none())
            .when_some(current, |b, c| {
                b.on_click(
                    cx.listener(move |this, _, window, cx| this.choose_redo(c.clone(), window, cx)),
                )
            });
        let weak = cx.entity().downgrade();
        DropdownButton::new(SharedString::from(format!("{id}-split")))
            .button(main)
            .disabled(busy)
            .menu_accessibility_label(t("meetings.redo.more"))
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                let mut section = None;
                for c in &choices {
                    if section != Some(c.cloud) {
                        if section.is_some() {
                            menu = menu.separator();
                        }
                        menu = menu.label(if c.cloud {
                            t("meetings.redo.cloudTitle")
                        } else {
                            t("meetings.redo.localTitle")
                        });
                        section = Some(c.cloud);
                    }
                    let choice = c.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(c.label.clone())
                            .checked(c.current)
                            .disabled(c.engine.is_none())
                            .on_click(move |_, window, cx| {
                                let choice = choice.clone();
                                weak.update(cx, |this, cx| this.choose_redo(choice, window, cx))
                                    .ok();
                            }),
                    );
                }
                menu
            })
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
                    // The player reports an output failure asynchronously.
                    cx.spawn(async move |_, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(300))
                            .await;
                        if let Some(e) = Playback::global().state().error {
                            cx.update(|cx| toast_error("meetings.detail.playError", &e, cx));
                        }
                    })
                    .detach();
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

    /// Speakers along the meeting's timeline, longest talker first; the
    /// user is drawn in the accent.
    fn render_talk_time(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        const MAX_ROWS: usize = 6;
        let talk = &self.talk;
        if talk.total_ms <= 0 || talk.speakers.is_empty() {
            return None;
        }
        let tone = design::tone(cx);
        let total = talk.total_ms as f32;
        let label_w = dp(84.);
        let share_w = dp(40.);
        let rows: Vec<AnyElement> = talk
            .speakers
            .iter()
            .take(MAX_ROWS)
            .map(|row| {
                let speaker = row
                    .speaker_id
                    .and_then(|id| self.speakers.iter().find(|s| s.id == id));
                let is_user = speaker.is_some_and(|s| s.is_user);
                let name = speaker
                    .map(|s| self.speaker_label(s, cx))
                    .unwrap_or_else(|| "?".into());
                let bar = if is_user {
                    tone.accent
                } else {
                    tone.line_art.opacity(0.75)
                };
                h_flex()
                    .h(dp(28.))
                    .gap(dp(12.))
                    .child(
                        div()
                            .w(label_w)
                            .flex_none()
                            .truncate()
                            .text_size(dp(12.5))
                            .text_color(if is_user { tone.accent_text } else { tone.text })
                            .child(name),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(dp(10.))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(dp(4.5))
                                    .h(px(1.))
                                    .bg(tone.hairline),
                            )
                            .children(row.spans.iter().map(move |(t0, t1)| {
                                div()
                                    .absolute()
                                    .top(px(1.))
                                    .h(dp(8.))
                                    .min_w(dp(2.))
                                    .rounded(dp(2.))
                                    .bg(bar)
                                    .left(relative(*t0 as f32 / total))
                                    .w(relative((*t1 - *t0) as f32 / total))
                            })),
                    )
                    .child(
                        design::mono(format!("{:.0}%", row.share * 100.0))
                            .w(share_w)
                            .flex_none()
                            .text_right()
                            .text_color(if is_user {
                                tone.accent_text
                            } else {
                                tone.text_muted
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let minutes = talk.total_ms as f32 / 60_000.0;
        let ticks: Vec<String> = (0..=4)
            .map(|i| format!("{:.0}", minutes * i as f32 / 4.0))
            .collect();
        Some(
            v_flex()
                .w_full()
                .border_t_1()
                .border_color(tone.hairline)
                .child(
                    h_flex()
                        .px(design::SECTION_X)
                        .pt(design::SECTION_Y)
                        .pb(dp(4.))
                        .justify_between()
                        .child(design::eyebrow(t("meetings.detail.talkTime"), cx))
                        .child(design::eyebrow(t("meetings.detail.share"), cx)),
                )
                .child(
                    v_flex()
                        .id("meeting-talk-time")
                        .px(design::SECTION_X)
                        .pb(design::SECTION_Y)
                        .children(rows)
                        .child(
                            h_flex()
                                .mt(dp(4.))
                                .gap(dp(12.))
                                .child(
                                    div()
                                        .w(label_w)
                                        .flex_none()
                                        .child(design::eyebrow(t("meetings.detail.minutes"), cx)),
                                )
                                .child(h_flex().flex_1().justify_between().children(
                                    ticks.into_iter().map(|tick| design::eyebrow(tick, cx)),
                                ))
                                .child(div().w(share_w).flex_none()),
                        )
                        .test_support(),
                )
                .into_any_element(),
        )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tone = design::tone(cx);
        let back = h_flex().child(
            Button::new("meeting-back")
                .ghost()
                .small()
                .pill()
                .icon(icon("arrow-left"))
                .label(t("meetings.detail.back"))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailEvent::Back))),
        );
        let Some(m) = &self.meeting else {
            return v_flex().gap_2().child(back).into_any_element();
        };
        let lang = guidy_i18n::language();
        let date = format_meeting_date(m.started_at, &lang);
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
        let failed = m.status == MeetingStatus::Failed;
        let fraction = self
            .progress
            .as_ref()
            .map(|p| p.fraction)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let show_process = can_process(m) && m.status != MeetingStatus::Processed;
        let model_banner = meeting_model_missing(cx)
            .filter(|_| matches!(m.status, MeetingStatus::Recorded | MeetingStatus::Failed))
            .map(|missing| render_model_download("detail", &missing, cx));

        // Stats: duration, speakers, words and, when the mic track was
        // recorded, how much of the talking was yours.
        let duration_ms = m.duration_ms().unwrap_or(self.talk.total_ms);
        let mut stats = vec![Stat::new(
            t("meetings.detail.duration"),
            format_offset(duration_ms),
        )];
        if !self.segments.is_empty() {
            stats.push(Stat::new(
                t("meetings.detail.speakers"),
                self.speakers.len().to_string(),
            ));
            stats.push(Stat::new(
                t("meetings.detail.words"),
                self.talk.words.to_string(),
            ));
            let user_ids: Vec<_> = self
                .speakers
                .iter()
                .filter(|s| s.is_user)
                .map(|s| s.id)
                .collect();
            if let Some(mine) = self
                .talk
                .speakers
                .iter()
                .find(|s| s.speaker_id.is_some_and(|id| user_ids.contains(&id)))
            {
                stats.push(
                    Stat::new(
                        t("meetings.detail.youSpoke"),
                        format!("{:.0}%", mine.share * 100.0),
                    )
                    .accent(true),
                );
            }
        }
        let columns = stats.len();

        let actions = h_flex()
            .gap(dp(8.))
            .when(show_process, |d| {
                d.child(
                    Button::new("meeting-detail-process")
                        .primary()
                        .when(self.processing_busy, |b| b.secondary())
                        .small()
                        .pill()
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
            })
            .when(self.can_redo(), |d| {
                d.child(self.redo_button("meeting-detail-redo", false, cx))
            })
            .when(processing, |d| {
                d.child(
                    Button::new("meeting-detail-cancel")
                        .secondary()
                        .small()
                        .pill()
                        .icon(icon("circle-x"))
                        .label(t("meetings.list.cancelProcessing"))
                        .loading(self.cancelling)
                        .disabled(self.cancelling)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_processing(cx))),
                )
            });

        let panel = design::panel(cx)
            .child(
                design::section(false, cx)
                    .gap(dp(6.))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_between()
                            .child(
                                div()
                                    .id("meeting-detail-title")
                                    .min_w_0()
                                    .child(crumb_title(
                                        m.title.clone().into(),
                                        Some(date.into()),
                                        design::TEXT_TITLE,
                                        cx,
                                    ))
                                    .test_support(),
                            )
                            .child(
                                design::chip(t(status_key(m.status)), processing, cx)
                                    .when(failed, |d| {
                                        d.bg(theme.danger.opacity(0.12)).text_color(theme.danger)
                                    }),
                            ),
                    )
                    .when(!attendees.is_empty(), |d| {
                        d.child(div().text_size(dp(12.)).text_color(tone.text_muted).child(
                            format!(
                                "{}: {}",
                                t("meetings.detail.attendees"),
                                attendees.join(", ")
                            ),
                        ))
                    })
                    .when_some(m.error.clone().filter(|_| failed), |d, e| {
                        d.child(div().text_size(dp(12.)).text_color(theme.danger).child(e))
                    }),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(tone.hairline)
                    .child(design::stat_grid(stats, columns, cx)),
            )
            .children(self.render_talk_time(cx))
            .when(processing, |d| {
                let stage = self
                    .progress
                    .as_ref()
                    .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                    .unwrap_or_else(|| t("meetings.status.processing"));
                d.child(
                    v_flex()
                        .w_full()
                        .gap(dp(8.))
                        .px(design::SECTION_X)
                        .py(design::SECTION_Y)
                        .border_t_1()
                        .border_color(tone.hairline)
                        .child(
                            div()
                                .font_family(crate::theme::MONO_FONT)
                                .text_size(design::TEXT_MONO)
                                .child(design::shimmer_text(
                                    "meeting-detail-stage",
                                    format!("{stage} · {:.0}%", fraction * 100.0),
                                    cx,
                                )),
                        )
                        .child(design::track("meeting-detail-progress", Some(fraction), cx)),
                )
            })
            .when(show_process || processing || self.can_redo(), |d| {
                d.child(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .px(design::SECTION_X)
                        .py(dp(10.))
                        .border_t_1()
                        .border_color(tone.hairline)
                        .child(actions),
                )
            });

        v_flex()
            .gap(dp(10.))
            .w_full()
            .child(back)
            .child(panel)
            .children(model_banner)
            .into_any_element()
    }

    fn render_picker(&self, id: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let heading = |key: &str| design::eyebrow(t(key), cx);
        let suggestions: AnyElement = if self.suggestions_loading {
            div()
                .text_size(dp(12.))
                .child(design::shimmer_text(
                    "speaker-suggestions-loading",
                    t("common.loading"),
                    cx,
                ))
                .into_any_element()
        } else if self.suggestions.is_empty() {
            div()
                .text_size(dp(12.))
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
                        .secondary()
                        .xsmall()
                        .pill()
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
            .id(id)
            .w_full()
            .gap(dp(8.))
            .px(design::SECTION_X)
            .py(dp(12.))
            .border_t_1()
            .border_color(design::tone(cx).hairline)
            .bg(design::tone(cx).ink)
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
                            .when(!can_save || self.assigning, |b| b.secondary())
                            .small()
                            .pill()
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
                            .pill()
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
                    .pl(design::SECTION_X)
                    .pr(dp(10.))
                    .py(dp(10.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(design::TEXT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .when(s.is_user, |d| d.text_color(design::tone(cx).accent_text))
                                    .child(name),
                            )
                            .when(named && !s.is_user, |d| {
                                d.child(
                                    design::mono(cluster_label(s))
                                        .text_color(theme.muted_foreground),
                                )
                            })
                            .when(uncertain, |d| {
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("speaker-uncertain-{sid}")))
                                        .px(dp(7.))
                                        .py(dp(2.))
                                        .rounded(design::RADIUS_CHIP)
                                        .font_family(crate::theme::MONO_FONT)
                                        .text_size(dp(11.))
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
                                    .secondary()
                                    .xsmall()
                                    .pill()
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
                                    .pill()
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
                    .when(self.picker == Some(PickTarget::Speaker(sid)), |d| {
                        d.child(self.render_picker(format!("speaker-picker-{sid}").into(), cx))
                    })
                    .into_any_element()
            })
            .collect();
        let redo = self
            .meeting
            .as_ref()
            .is_some_and(|m| m.status == MeetingStatus::Processed && can_process(m))
            .then(|| self.speakers_button(cx));
        Some(
            v_flex()
                .w_full()
                .gap(dp(8.))
                .child(
                    h_flex()
                        .w_full()
                        .px(dp(2.))
                        .items_center()
                        .justify_between()
                        .child(design::eyebrow(t("meetings.detail.speakers"), cx))
                        .children(redo),
                )
                .child(group(None, rows, cx))
                .into_any_element(),
        )
    }

    fn render_block(&self, i: usize, block: &SpeakerBlock, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tone = design::tone(cx);
        let speaker = block
            .speaker_id
            .and_then(|id| self.speakers.iter().find(|s| s.id == id));
        let unknown = speaker.is_none();
        let name = speaker
            .map(|s| self.speaker_label(s, cx))
            .unwrap_or_else(|| t("meetings.speaker.unknown").to_string());
        let is_user = speaker.is_some_and(|s| s.is_user);
        let pickable = speaker.filter(|s| !s.is_user).map(|s| s.id);
        let uncertain = speaker.is_some_and(needs_confirmation);
        let st = Playback::global().state();
        let block_ids: Vec<i64> = block.segments.iter().map(|s| s.id).collect();
        // The line picker opens right under the (first) line it names, so it
        // is next to where it was clicked even in a long turn.
        let picker_line = match &self.picker {
            Some(PickTarget::Lines(ids)) => ids.first().copied(),
            _ => None,
        };
        // Who and when on the left, what they said on the right.
        let row = h_flex()
            .id(SharedString::from(format!("block-{i}")))
            .w_full()
            .items_start()
            .gap(dp(12.))
            .pl(design::SECTION_X)
            .pr(dp(10.))
            .py(dp(10.))
            // An unknown voice is highlighted until someone names it.
            .when(unknown, |d| {
                d.bg(theme.warning.opacity(0.07))
                    .border_l_2()
                    .border_color(theme.warning.opacity(0.6))
            })
            .child(
                v_flex()
                    .w(dp(84.))
                    .flex_none()
                    .pt(dp(2.))
                    .gap(dp(2.))
                    .child(
                        div()
                            .id(SharedString::from(format!("block-{i}-speaker")))
                            .text_size(design::TEXT_BODY)
                            .truncate()
                            .text_color(if unknown {
                                theme.warning
                            } else if is_user {
                                tone.accent_text
                            } else {
                                tone.text
                            })
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
                    .child(
                        design::mono(format_offset(block.t0_ms()))
                            .text_size(dp(11.))
                            .text_color(tone.text_faint),
                    )
                    .when(uncertain, |d| {
                        d.child(
                            div()
                                .text_size(dp(11.))
                                .text_color(theme.warning)
                                .child(t("meetings.speaker.confirm")),
                        )
                    })
                    .when(unknown, |d| {
                        let ids = block_ids.clone();
                        d.child(
                            div()
                                .id(SharedString::from(format!("block-{i}-tag")))
                                .mt(dp(4.))
                                .text_size(dp(11.))
                                .text_color(theme.warning)
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(t("meetings.speaker.whoIsThis"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_line_picker(ids.clone(), window, cx)
                                }))
                                .test_support(),
                        )
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(2.))
                    .children(block.segments.iter().map(|seg| {
                        let key = self.clip_key(seg.id);
                        let active = st.error.is_none() && st.path.as_ref() == Some(&key);
                        let playing = active && st.playing;
                        let s = seg.clone();
                        let seg_id = seg.id;
                        let line = h_flex()
                            .id(SharedString::from(format!("segment-{}", seg.id)))
                            .group(SharedString::from(format!("segment-{}", seg.id)))
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_start()
                            .px(dp(6.))
                            .py(px(1.))
                            .rounded(dp(6.))
                            .cursor_pointer()
                            .when(active, |d| d.bg(tone.accent_soft))
                            .hover(move |d| d.bg(tone.raised.opacity(0.5)))
                            .child(
                                div()
                                    .pt(dp(4.))
                                    .flex_none()
                                    .text_color(if active {
                                        tone.accent_text
                                    } else {
                                        tone.text_faint
                                    })
                                    .child(icon(if playing { "pause" } else { "play" }).xsmall()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(design::TEXT_BODY)
                                    .line_height(relative(1.6))
                                    .child(seg.text.clone()),
                            )
                            // "Who said this?": name one line's speaker.
                            .child(
                                div()
                                    .id(SharedString::from(format!("segment-{seg_id}-tag")))
                                    .flex_none()
                                    .mt(dp(3.))
                                    .p(dp(3.))
                                    .rounded(dp(5.))
                                    .text_color(tone.text_faint)
                                    // Faint until the line is hovered.
                                    .opacity(0.35)
                                    .group_hover(format!("segment-{seg_id}"), |s| s.opacity(1.))
                                    .hover(move |s| s.bg(tone.raised).text_color(tone.text))
                                    .child(icon("user-round-pen").xsmall())
                                    .tooltip(|window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(t(
                                            "meetings.speaker.whoSaidThis",
                                        ))
                                        .build(window, cx)
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_line_picker(vec![seg_id], window, cx)
                                    }))
                                    .test_support(),
                            )
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
                            .test_support();
                        v_flex().w_full().min_w_0().child(line).when(
                            picker_line == Some(seg_id),
                            |d| {
                                d.child(div().pt(dp(6.)).pb(dp(10.)).child(
                                    self.render_picker(format!("line-picker-{seg_id}").into(), cx),
                                ))
                            },
                        )
                    })),
            )
            .test_support();
        row.into_any_element()
    }

    /// While this meeting records: what live transcription has heard so far.
    fn render_live_transcript(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.meeting.as_ref()?.status != MeetingStatus::Recording {
            return None;
        }
        let tone = design::tone(cx);
        let widget = crate::ui::recording_widget::RecordingWidgetState::global(cx)?;
        let model = &widget.read(cx).model;
        let live = (model.live.meeting_id == self.meeting_id).then_some(&model.live);
        let rows = live
            .map(|l| crate::logic::recording_widget::rows(&l.lines))
            .unwrap_or_default();
        let hint = div()
            .px(design::SECTION_X)
            .py(dp(12.))
            .text_size(dp(12.))
            .line_height(relative(1.5))
            .text_color(tone.text_muted)
            .child(match live.map(|l| &l.status) {
                Some(LiveStatus::Off) => t("meetings.widget.liveOff"),
                Some(LiveStatus::Unavailable { reason }) => {
                    crate::ui::kit::t_args("meetings.widget.liveUnavailable", &[("reason", reason)])
                }
                _ if rows.is_empty() => t("meetings.widget.liveListening"),
                _ => t("meetings.widget.liveHint"),
            })
            .into_any_element();
        let lines = rows.iter().enumerate().map(|(i, r)| {
            let you = r.track == Track::Mic;
            v_flex()
                .id(("live-transcript-row", i))
                .px(design::SECTION_X)
                .py(dp(10.))
                .gap(dp(4.))
                .child(
                    design::eyebrow(
                        if you {
                            t("meetings.widget.you")
                        } else {
                            t("meetings.widget.others")
                        },
                        cx,
                    )
                    .when(you, |d| d.text_color(tone.accent_text)),
                )
                .child(
                    div()
                        .text_size(design::TEXT_BODY)
                        .line_height(relative(1.55))
                        .child(r.text.clone())
                        .when(!r.tentative.is_empty(), |d| {
                            d.child(div().text_color(tone.text_muted).child(r.tentative.clone()))
                        }),
                )
                .into_any_element()
        });
        let rows: Vec<AnyElement> = std::iter::once(hint).chain(lines).collect();
        Some(
            div()
                .id("meeting-live-transcript")
                .child(group(Some(t("meetings.widget.liveTitle")), rows, cx))
                .test_support()
                .into_any_element(),
        )
    }

    /// A transcript that looks unreliable offers to be redone.
    fn render_quality_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.can_redo() || self.processing_busy {
            return None;
        }
        let issue = transcript_issue(&self.segments)?;
        let tone = design::tone(cx);
        let warning = cx.theme().warning;
        let what = match issue {
            TranscriptIssue::Repetition => t("meetings.redo.issueRepetition"),
            TranscriptIssue::TooFewWords => t("meetings.redo.issueTooFewWords"),
        };
        Some(
            div()
                .id("transcript-quality")
                .p(dp(10.))
                .child(
                    design::notice(warning).child(
                        h_flex()
                            .gap(dp(12.))
                            .items_center()
                            .child(icon("triangle-alert").small().text_color(warning))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(dp(2.))
                                    .text_size(dp(12.5))
                                    .line_height(relative(1.45))
                                    .child(div().text_color(tone.text).child(what))
                                    .child(
                                        div()
                                            .text_color(tone.text_muted)
                                            .child(t("meetings.redo.issueHint")),
                                    ),
                            )
                            .child(self.redo_button("transcript-quality-redo", true, cx)),
                    ),
                )
                .test_support()
                .into_any_element(),
        )
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(live) = self.render_live_transcript(cx) {
            return live;
        }
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = if self.loading {
            vec![div()
                .px_4()
                .py(dp(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(theme.muted_foreground)
                .child(t("meetings.detail.loading"))
                .into_any_element()]
        } else if self.segments.is_empty() {
            vec![div()
                .id("transcript-empty")
                .px_4()
                .py(dp(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(theme.muted_foreground)
                .child(t("meetings.detail.empty"))
                .test_support()
                .into_any_element()]
        } else {
            let notice = self.render_quality_notice(cx);
            notice
                .into_iter()
                .chain(
                    group_by_speaker(&self.segments)
                        .iter()
                        .enumerate()
                        .map(|(i, b)| self.render_block(i, b, cx)),
                )
                .collect()
        };
        let copy = (!self.loading && !self.segments.is_empty()).then(|| {
            Button::new("transcript-copy")
                .ghost()
                .pill()
                .small()
                .icon(icon(if self.transcript_copied {
                    "check"
                } else {
                    "copy"
                }))
                .label(if self.transcript_copied {
                    t("meetings.copy.copied")
                } else {
                    t("meetings.copy.transcript")
                })
                .on_click(cx.listener(|this, _, _, cx| this.copy_transcript(cx)))
        });
        v_flex()
            .w_full()
            .gap(dp(8.))
            .child(
                h_flex()
                    .w_full()
                    .px(dp(2.))
                    .items_center()
                    .justify_between()
                    .child(design::eyebrow(t("meetings.detail.transcript"), cx))
                    .children(copy),
            )
            .child(group(None, rows, cx))
            .into_any_element()
    }

    /// Copy the transcript as `[mm:ss] Speaker: text` lines under the
    /// meeting's title and date.
    fn copy_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(m) = &self.meeting else { return };
        let text = crate::logic::meetings::transcript_text(
            &m.title,
            &format_meeting_date(m.started_at, &guidy_i18n::language()),
            &self.segments,
            &self.speakers,
            &self.people,
            &meeting_settings(cx).user_display_name,
        );
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.transcript_copied = true;
        cx.notify();
        cx.spawn(async |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_500))
                .await;
            this.update(cx, |this, cx| {
                this.transcript_copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for MeetingDetail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("meeting-detail")
            .w_full()
            .gap(dp(22.))
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
