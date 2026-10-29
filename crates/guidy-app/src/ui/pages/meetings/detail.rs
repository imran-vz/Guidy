//! Meeting detail: a pinned header (title with inline rename, when and
//! how long, the actions: Process, Redo, Copy, Export, Delete, and an
//! info popover), thin notices, the player, then the notes (summary,
//! action items, Linear tickets) and the transcript, side by side when
//! there is room, each in its own scroll region. The transcript is
//! virtualized by speaker turn; its speakers strip names voices, and ⌘F
//! finds words in it.

use super::ai_settings::SummaryChoice;
use super::player::{MeetingPlayer, PlayerEvent};
use super::summary::{SummarySection, SummaryView};
use super::tickets::TicketsView;
use super::{meeting_model_missing, meeting_settings, render_model_download, run, toast_error};
use crate::logic::meetings::{
    can_process, cluster_label, export_file_name, export_markdown, find_in_segments,
    format_meeting_date, format_offset, group_by_speaker, insert_provisional, line_near,
    line_range, needs_confirmation, non_empty, progress_label, redo_diff, select_lines,
    speaker_name, status_key, talk_time, transcript_issue, LineClick, MeetingsEvent, RedoChoice,
    RedoDiff, SpeakerBlock, SpeakerRedoChoice, SpeakerShift, TalkTime, TranscriptIssue,
};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp, Stat};
use crate::ui::kit::{icon, t, t_args, LocalizedPlaceholder};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Selectable, Sizable, WindowExt,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::{NameSuggestion, PersonRef, SuggestionSource};
use guidy_core::meetings::types::{
    CloudTranscriptionOption, DiarizationOptions, LiveStatus, Meeting, MeetingId, MeetingSpeaker,
    MeetingStatus, Person, ProcessingProgress, ProvisionalLine, Segment, SpeakerId, Track,
    TranscriptionEngine,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// After the user scrolls the transcript, how long it doesn't follow the
/// playhead.
pub(crate) const FOLLOW_PAUSE: Duration = Duration::from_secs(5);

/// Whether the transcript should follow the playhead: while it plays, and
/// not for [`FOLLOW_PAUSE`] after the user scrolled it.
pub(crate) fn follows_playhead(playing: bool, scrolled_at: Option<Instant>, now: Instant) -> bool {
    playing && scrolled_at.is_none_or(|at| now.saturating_duration_since(at) >= FOLLOW_PAUSE)
}

pub enum DetailEvent {
    /// Back to the list (stack layout).
    Back,
    /// The ticket list asked for settings (no Linear key yet).
    OpenSettings,
    /// Delete… was chosen: the page confirms and deletes.
    Delete(MeetingId),
    /// The title changed here: the list shows it.
    Renamed,
}

/// Notes is the meeting write-up. Transcript is the lines and speakers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Notes,
    Transcript,
}

/// The notes column's parts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesTab {
    Summary,
    ActionItems,
    Tickets,
}

/// How the page lays the detail out (it knows the width).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DetailLayout {
    /// Notes and transcript side by side.
    pub side_by_side: bool,
    /// The list is hidden (stack): show a back button.
    pub show_back: bool,
    /// Narrow: the actions get their own row under the title.
    pub compact: bool,
}

/// What Export writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportKind {
    /// Notes and transcript, Markdown.
    All,
    /// The notes, Markdown.
    Notes,
    /// The transcript, plain text.
    Transcript,
}

/// What the name picker names: a whole speaker, or some lines (an unknown
/// speaker's turn, or one line the diarization got wrong).
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    Speaker(SpeakerId),
    Lines(Vec<i64>),
}

/// The transcript a redo started from.
struct RedoSnapshot {
    segments: Vec<Segment>,
    speakers: Vec<MeetingSpeaker>,
    duration_ms: Option<i64>,
}

pub struct MeetingDetail {
    pub meeting_id: MeetingId,
    meeting: Option<Meeting>,
    segments: Vec<Segment>,
    /// `segments` grouped into speaker turns: the transcript list's items.
    blocks: Vec<SpeakerBlock>,
    transcript_list: ListState,
    /// Scroll of the notes column, and of the live / streamed transcript.
    notes_scroll: ScrollHandle,
    lines_scroll: ScrollHandle,
    speakers_scroll: ScrollHandle,
    /// Derived from `segments` on load, for the stats and talk-time chart.
    talk: TalkTime,
    speakers: Vec<MeetingSpeaker>,
    people: Vec<Person>,
    loading: bool,
    progress: Option<ProcessingProgress>,
    /// The speaker whose name picker is open.
    picker: Option<PickTarget>,
    /// The line a line picker opens under (default: its first line).
    picker_at: Option<i64>,
    /// Lines selected to tag together, in transcript order.
    selection: Vec<i64>,
    /// Where a Shift-click range starts.
    anchor: Option<i64>,
    /// The line a drag started on (mouse down, no modifiers).
    drag_start: Option<i64>,
    /// The drag reached another line: the click it ends in doesn't play.
    dragged: bool,
    /// Takes Esc / Enter for the selection.
    focus: FocusHandle,
    suggestions: Vec<NameSuggestion>,
    suggestions_loading: bool,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    assigning: bool,
    processing_busy: bool,
    cancelling: bool,
    /// Deep link: seek and play here once the transcript has loaded.
    pending_seek: Option<i64>,
    /// A search hit: scroll to this line once the transcript has loaded.
    pending_reveal: Option<i64>,
    /// The line just revealed, highlighted for a moment.
    flash: Option<i64>,
    /// Cloud providers the transcript can be redone with.
    cloud_options: Vec<CloudTranscriptionOption>,
    /// What "Redo speakers" can use.
    diarization: DiarizationOptions,
    /// "Copy transcript" just copied (shows a check for a moment).
    transcript_copied: bool,
    /// The header copy just copied the notes.
    notes_copied: bool,
    /// Notes or the transcript (when they don't fit side by side).
    detail_tab: DetailTab,
    notes_tab: NotesTab,
    layout: DetailLayout,
    /// The speakers panel (names, confirmations, talk time) is open. It
    /// opens by itself when a speaker still needs a name.
    speakers_open: bool,
    /// `speakers_open` was set from the first load.
    speakers_settled: bool,
    /// The first load hasn't arrived yet (it picks the starting tab).
    loading_first: bool,
    /// The title and description are being generated.
    titling: bool,
    /// The title is being edited in place.
    renaming: bool,
    title_input: Entity<InputState>,
    summary: Entity<SummaryView>,
    tickets: Entity<TicketsView>,
    player: Entity<MeetingPlayer>,
    /// The line under the playhead (once something played).
    active_line: Option<i64>,
    /// When the user last scrolled the transcript (wheel/trackpad).
    scrolled_at: Rc<Cell<Option<Instant>>>,
    /// ⌘F in the transcript.
    find_open: bool,
    find_input: Entity<InputState>,
    find_matches: Vec<(i64, Vec<Range<usize>>)>,
    /// Index into the flattened matches (line, range).
    find_current: usize,
    /// Lines announced while this meeting is still transcribing, in time order.
    provisional: Vec<ProvisionalLine>,
    /// Transcript and speakers at the moment a redo started.
    before_redo: Option<RedoSnapshot>,
    /// Shown after that redo finishes, until dismissed.
    what_changed: Option<RedoDiff>,
    /// What changed shows its details.
    what_changed_open: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<DetailEvent> for MeetingDetail {}

impl MeetingDetail {
    pub fn new(meeting_id: MeetingId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let name_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.speaker.namePlaceholder",
                window,
                cx,
            )
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.speaker.emailPlaceholder",
                window,
                cx,
            )
        });
        let title_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.list.titlePlaceholder",
                window,
                cx,
            )
        });
        let find_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.find.placeholder",
                window,
                cx,
            )
        });
        // Kept for the meeting's lifetime, so unsaved edits, drafts and
        // the player survive switching tabs and columns.
        let summary = cx.new(|cx| SummaryView::new(meeting_id, window, cx));
        let tickets = cx.new(|cx| TicketsView::new(meeting_id, window, cx));
        let player = cx.new(|_| MeetingPlayer::new(meeting_id));
        let weak = cx.entity().downgrade();
        summary.update(cx, |v, _| {
            v.set_section(SummarySection::Notes);
            v.set_on_seek(move |t_ms, _, cx| {
                let _ = weak.update(cx, |this, cx| this.seek_to(t_ms, cx));
            });
        });
        let weak = cx.entity().downgrade();
        tickets.update(cx, |v, _| {
            v.set_embedded(true);
            v.set_on_open_settings(move |_, cx| {
                let _ = weak.update(cx, |_, cx| cx.emit(DetailEvent::OpenSettings));
            })
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
            cx.subscribe(&title_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::PressEnter { .. } => this.save_rename(cx),
                InputEvent::Blur => this.cancel_rename(cx),
                _ => {}
            }),
            cx.subscribe(&find_input, |this, input, ev: &InputEvent, cx| match ev {
                InputEvent::Change => {
                    let q = input.read(cx).value().to_string();
                    this.set_find(&q, cx);
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.find_step(if *shift { -1 } else { 1 }, cx)
                }
                _ => {}
            }),
            cx.subscribe(&player, |this, _, ev: &PlayerEvent, cx| {
                let PlayerEvent::Moved(t_ms) = *ev;
                this.follow_playhead(t_ms, cx);
            }),
            cx.observe(&summary, |_, _, cx| cx.notify()),
            cx.observe(&tickets, |_, _, cx| cx.notify()),
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
            blocks: Vec::new(),
            transcript_list: ListState::new(0, ListAlignment::Top, dp(400.)),
            notes_scroll: ScrollHandle::new(),
            lines_scroll: ScrollHandle::new(),
            speakers_scroll: ScrollHandle::new(),
            talk: talk_time(&[], None),
            speakers: Vec::new(),
            people: Vec::new(),
            loading: true,
            progress: None,
            picker: None,
            picker_at: None,
            selection: Vec::new(),
            anchor: None,
            drag_start: None,
            dragged: false,
            focus: cx.focus_handle(),
            suggestions: Vec::new(),
            suggestions_loading: false,
            name_input,
            email_input,
            assigning: false,
            processing_busy: false,
            cancelling: false,
            pending_seek: None,
            pending_reveal: None,
            flash: None,
            cloud_options: Vec::new(),
            diarization: DiarizationOptions::default(),
            transcript_copied: false,
            notes_copied: false,
            detail_tab: DetailTab::Notes,
            notes_tab: NotesTab::Summary,
            layout: DetailLayout::default(),
            speakers_open: false,
            speakers_settled: false,
            loading_first: true,
            titling: false,
            renaming: false,
            title_input,
            summary,
            tickets,
            player,
            active_line: None,
            scrolled_at: Rc::new(Cell::new(None)),
            find_open: false,
            find_input,
            find_matches: Vec::new(),
            find_current: 0,
            provisional: Vec::new(),
            before_redo: None,
            what_changed: None,
            what_changed_open: false,
            _subs: subs,
        };
        // A wheel/trackpad scroll of the transcript pauses following the
        // playhead for a moment (the list calls this only for user scrolls).
        let scrolled_at = this.scrolled_at.clone();
        this.transcript_list.set_scroll_handler(move |_, _, cx| {
            scrolled_at.set(Some(cx.background_executor().now()))
        });
        this.load(cx);
        this
    }

    fn on_event(&mut self, ev: &MeetingsEvent, cx: &mut Context<Self>) {
        match ev {
            MeetingsEvent::MeetingChanged(id) if *id == self.meeting_id => self.load(cx),
            MeetingsEvent::PeopleChanged | MeetingsEvent::Ready => self.load(cx),
            MeetingsEvent::ProvisionalLine(line) if line.meeting_id == self.meeting_id => {
                self.push_provisional(line.clone());
                cx.notify();
            }
            MeetingsEvent::Progress(p) if p.meeting_id == self.meeting_id => {
                let done = p.fraction >= 1.0;
                self.progress = Some(p.clone());
                // The first progress of a run that already has a transcript
                // is the "before" of What changed.
                if !done && self.before_redo.is_none() && !self.segments.is_empty() {
                    let duration_ms = self.meeting.as_ref().and_then(|m| m.duration_ms());
                    self.before_redo = Some(RedoSnapshot {
                        segments: self.segments.clone(),
                        speakers: self.speakers.clone(),
                        duration_ms,
                    });
                }
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

    /// A finished redo becomes the What changed card. A load that arrives
    /// while the run is still going keeps the snapshot.
    fn settle_redo(&mut self) {
        let Some(snap) = self.before_redo.take() else {
            return;
        };
        let processing = self
            .meeting
            .as_ref()
            .is_some_and(|m| m.status == MeetingStatus::Processing);
        if processing {
            self.before_redo = Some(snap);
            return;
        }
        let you = t("meetings.detail.you").to_string();
        let diff = redo_diff(
            &snap.segments,
            &self.segments,
            &snap.speakers,
            &self.speakers,
            &self.people,
            &you,
            snap.duration_ms,
        );
        if !diff.is_empty() {
            self.what_changed = Some(diff);
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
                        // A finished run replaces the streamed lines. While a
                        // run is still going, keep them (a redo keeps the old
                        // transcript too, dimmed until the first new line).
                        let processing = this
                            .meeting
                            .as_ref()
                            .is_some_and(|m| m.status == MeetingStatus::Processing);
                        if !processing {
                            this.provisional.clear();
                        }
                        this.speakers = speakers;
                        this.people = people;
                        this.rebuild_blocks();
                        this.settle_redo();
                        if progress.is_some() {
                            this.progress = progress;
                        }
                        this.sync_player(cx);
                        if this.loading_first {
                            // Live or being transcribed: the lines are what
                            // there is to see.
                            this.loading_first = false;
                            if this.meeting.as_ref().is_some_and(|m| {
                                matches!(
                                    m.status,
                                    MeetingStatus::Recording | MeetingStatus::Processing
                                )
                            }) {
                                this.detail_tab = DetailTab::Transcript;
                            }
                        }
                        if !this.speakers_settled && !this.speakers.is_empty() {
                            // Open when a voice still needs a name.
                            this.speakers_settled = true;
                            this.speakers_open = this.pending_speakers() > 0;
                        }
                        if this.find_open {
                            let q = this.find_input.read(cx).value().to_string();
                            this.set_find(&q, cx);
                        }
                        if !this.segments.is_empty() {
                            if let Some(t_ms) = this.pending_seek.take() {
                                this.seek_to(t_ms, cx);
                            }
                            if let Some(line) = this.pending_reveal.take() {
                                if this.segments.iter().any(|s| s.id == line) {
                                    this.reveal_line(line, cx);
                                }
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

    /// Scroll to (and flash) a transcript line now, or once it has loaded.
    pub fn reveal_when_loaded(&mut self, segment_id: i64, cx: &mut Context<Self>) {
        if self.segments.iter().any(|s| s.id == segment_id) {
            self.reveal_line(segment_id, cx);
        } else {
            self.pending_reveal = Some(segment_id);
        }
    }

    fn generate_title(&mut self, cx: &mut Context<Self>) {
        if self.titling {
            return;
        }
        let id = self.meeting_id;
        self.titling = true;
        cx.notify();
        run(
            cx,
            move |b| b.generate_title(id, false),
            |this: &mut Self, r, cx| {
                this.titling = false;
                match r {
                    Ok(outcome) => this.meeting = Some(outcome.meeting),
                    Err(e)
                        if crate::meetings_backend::MeetingsError::parse(&e)
                            == crate::meetings_backend::MeetingsError::Cancelled => {}
                    Err(e) => toast_error("meetings.titles.error", &e, cx),
                }
            },
        );
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

    #[cfg(test)]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    #[cfg(test)]
    pub fn redo_state(&self) -> (bool, Option<usize>) {
        (
            self.before_redo.is_some(),
            self.what_changed.as_ref().map(|d| d.lines_changed),
        )
    }

    // -- speaker naming -------------------------------------------------------

    pub fn open_picker(
        &mut self,
        speaker_id: SpeakerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picker = Some(PickTarget::Speaker(speaker_id));
        self.picker_at = None;
        // The picker opens under the speaker's row.
        self.speakers_open = true;
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
        self.picker_at = None;
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
        self.picker_at = None;
        self.suggestions.clear();
        cx.notify();
    }

    // -- selecting lines --------------------------------------------------------

    #[cfg(test)]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The lines selected to tag together.
    pub fn selection(&self) -> &[i64] {
        &self.selection
    }

    /// Every line's id, in transcript order.
    fn line_order(&self) -> Vec<i64> {
        self.segments.iter().map(|s| s.id).collect()
    }

    /// A click on a line: Shift selects a range, Command one more line, a
    /// plain click plays it (and drops the selection).
    pub fn click_line(&mut self, seg: &Segment, click: LineClick, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.dragged) {
            // The end of a drag across lines, not a click.
            return;
        }
        let (selection, anchor) = select_lines(
            &self.line_order(),
            &self.selection,
            self.anchor,
            seg.id,
            click,
        );
        let changed = selection != self.selection;
        self.selection = selection;
        self.anchor = anchor;
        if changed && self.picker_at.is_some() {
            self.close_picker(cx);
        }
        if click == LineClick::Plain {
            self.play(seg, cx);
        }
        cx.notify();
    }

    /// The mouse moved over line `over`, the button held since a drag
    /// started: select from the start to it.
    fn drag_over(&mut self, over: i64, pressed: bool, cx: &mut Context<Self>) {
        let Some(start) = self.drag_start else {
            return;
        };
        if !pressed {
            // Let go somewhere else.
            self.drag_start = None;
            cx.notify();
            return;
        }
        if over == start && !self.dragged {
            return;
        }
        let range = line_range(&self.line_order(), start, over);
        if range != self.selection {
            self.selection = range;
            self.anchor = Some(start);
            self.dragged = true;
            if self.picker_at.is_some() {
                self.close_picker(cx);
            }
            cx.notify();
        }
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        if self.picker_at.is_some() {
            self.close_picker(cx);
        }
        cx.notify();
    }

    /// Name the speaker of the selected lines; the picker opens under the
    /// last one (where the selection ended).
    pub fn tag_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&last) = self.selection.last() else {
            return;
        };
        self.open_line_picker(self.selection.clone(), window, cx);
        self.picker_at = Some(last);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            return;
        }
        match ev.keystroke.key.as_str() {
            "escape" => {
                self.clear_selection(cx);
                cx.stop_propagation();
            }
            "enter" if self.picker.is_none() => {
                self.tag_selection(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
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
                        this.selection.clear();
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

    pub fn select_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        let shown = tab == DetailTab::Transcript && self.detail_tab != tab;
        self.detail_tab = tab;
        if shown {
            self.show_playhead(cx);
        }
        cx.notify();
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

    /// "Redo" runs the transcript again with the current model. The menu
    /// also offers the other transcript models, a speaker redo, and a new
    /// summary. `primary` styles it as the suggested action.
    fn redo_button(
        &self,
        id: &'static str,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> DropdownButton {
        let choices = self.redo_choices(cx);
        let current = choices.iter().find(|c| c.current && !c.cloud).cloned();
        let busy = self.processing_busy;
        let icon_only = !primary && self.layout.compact;
        let main = Button::new(id)
            .small()
            .icon(icon("rotate-ccw"))
            .when(!icon_only, |b| b.label(t("meetings.redo.menu")))
            .when(icon_only && current.is_none(), |b| {
                b.tooltip(t("meetings.redo.menu"))
            })
            // In the header it sits with the other ghost toolbar buttons.
            .map(|b| if primary { b.primary() } else { b.ghost() })
            .when_some(current.clone(), |b, c| {
                b.tooltip(t_args("meetings.redo.tooltip", &[("model", &c.label)]))
            })
            // No local model: the main action can't run. The menu stays
            // enabled (only `busy` disables it) so speakers and the summary
            // are still reachable.
            .disabled(busy || current.is_none())
            .when_some(current, |b, c| {
                b.on_click(
                    cx.listener(move |this, _, window, cx| this.choose_redo(c.clone(), window, cx)),
                )
            });
        let weak = cx.entity().downgrade();
        let speakers = self.speaker_redo_choices();
        let (summary_choices, has_summary) = {
            let v = self.summary.read(cx);
            (v.provider_choices(), v.has_summary())
        };
        let offer_speakers = self
            .meeting
            .as_ref()
            .is_some_and(|m| m.status == MeetingStatus::Processed && can_process(m));
        DropdownButton::new(SharedString::from(format!("{id}-split")))
            .button(main)
            .disabled(busy)
            .menu_accessibility_label(t("meetings.redo.more"))
            .dropdown_menu(move |menu, window, cx| {
                let mut menu = menu.label(t("meetings.redo.action"));
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
                if offer_speakers {
                    let speakers = speakers.clone();
                    let weak = weak.clone();
                    menu = menu.separator().submenu(
                        t("meetings.speakersRedo.action"),
                        window,
                        cx,
                        move |menu, _, _| {
                            let mut menu = menu;
                            for c in &speakers {
                                if c.cloud {
                                    menu = menu.separator().label(t("meetings.redo.cloudTitle"));
                                }
                                let choice = c.clone();
                                let weak = weak.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(c.label.clone())
                                        .checked(c.current)
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
                        },
                    );
                }
                if has_summary {
                    let choices = summary_choices.clone();
                    let weak = weak.clone();
                    menu = menu.separator().submenu(
                        t("meetingsAi.summary.regenerate"),
                        window,
                        cx,
                        move |menu, _window, _cx| {
                            let mut local: Vec<_> =
                                choices.iter().filter(|c| c.local).cloned().collect();
                            let mut cloud: Vec<_> =
                                choices.iter().filter(|c| !c.local).cloned().collect();
                            let current_first = |rows: &mut Vec<SummaryChoice>| {
                                rows.sort_by(|a, b| {
                                    b.current.cmp(&a.current).then(a.label.cmp(&b.label))
                                });
                            };
                            current_first(&mut local);
                            current_first(&mut cloud);
                            let mut menu = menu;
                            let mut first = true;
                            for (title, rows) in [
                                (t("meetings.redo.localTitle"), local),
                                (t("meetings.redo.cloudTitle"), cloud),
                            ] {
                                if rows.is_empty() {
                                    continue;
                                }
                                if !first {
                                    menu = menu.separator();
                                }
                                first = false;
                                menu = menu.label(title);
                                for c in rows {
                                    let run = (c.provider_id.clone(), c.model.clone());
                                    let weak = weak.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(c.label.clone())
                                            .checked(c.current)
                                            .on_click(move |_, _, cx| {
                                                let run = run.clone();
                                                weak.update(cx, |this, cx| {
                                                    this.summary.update(cx, |v, cx| {
                                                        v.generate(Some(run), cx)
                                                    });
                                                })
                                                .ok();
                                            }),
                                    );
                                }
                            }
                            if !first {
                                menu = menu.separator();
                            }
                            let weak = weak.clone();
                            menu.item(
                                PopupMenuItem::new(t("meetingsAi.summary.otherModel")).on_click(
                                    move |_, window, cx| {
                                        weak.update(cx, |this, cx| {
                                            this.notes_tab = NotesTab::Summary;
                                            this.detail_tab = DetailTab::Notes;
                                            this.summary
                                                .update(cx, |v, cx| v.open_picker(window, cx));
                                        })
                                        .ok();
                                    },
                                ),
                            )
                        },
                    );
                }
                menu
            })
    }

    // -- layout, player, reveal ------------------------------------------------

    /// The page tells the detail how much room it has.
    pub fn set_layout(&mut self, layout: DetailLayout, cx: &mut Context<Self>) {
        if self.layout != layout {
            let transcript_shown = layout.side_by_side && !self.layout.side_by_side;
            self.layout = layout;
            if transcript_shown {
                self.show_playhead(cx);
            }
            cx.notify();
        }
    }

    pub fn layout(&self) -> DetailLayout {
        self.layout
    }

    pub fn notes_tab(&self) -> NotesTab {
        self.notes_tab
    }

    pub fn select_notes_tab(&mut self, tab: NotesTab, cx: &mut Context<Self>) {
        self.notes_tab = tab;
        self.detail_tab = DetailTab::Notes;
        cx.notify();
    }

    /// The summary view (kept for the meeting's lifetime).
    pub fn summary_view(&self) -> &Entity<SummaryView> {
        &self.summary
    }

    #[cfg(test)]
    pub fn player(&self) -> &Entity<MeetingPlayer> {
        &self.player
    }

    /// Speakers still waiting for a name or a confirmation.
    fn pending_speakers(&self) -> usize {
        self.speakers
            .iter()
            .filter(|s| !s.is_user && (s.person_id.is_none() || needs_confirmation(s)))
            .count()
    }

    /// Group the lines into turns and hand the list its new item count.
    fn rebuild_blocks(&mut self) {
        self.blocks = group_by_speaker(&self.segments);
        let old = self.transcript_list.item_count();
        self.transcript_list.splice(0..old, self.blocks.len());
    }

    /// The meeting's audio can be played: it is still on disk and the
    /// meeting is not being recorded.
    fn has_audio(&self) -> bool {
        self.meeting.as_ref().is_some_and(|m| {
            m.status != MeetingStatus::Recording
                && (m.mic_audio_path.is_some() || m.system_audio_path.is_some())
                && m.duration_ms().unwrap_or(0) > 0
        })
    }

    fn sync_player(&mut self, cx: &mut Context<Self>) {
        let duration = self
            .meeting
            .as_ref()
            .and_then(|m| m.duration_ms())
            .unwrap_or(self.talk.total_ms);
        self.player.update(cx, |p, cx| p.set_duration(duration, cx));
    }

    /// Space: play or pause the meeting.
    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if !self.has_audio() {
            return;
        }
        self.player.update(cx, |p, cx| p.toggle(cx));
    }

    /// The line at `t_ms`: the one containing it, else the last one that
    /// started before it.
    fn line_at(&self, t_ms: i64) -> Option<&Segment> {
        self.segments
            .iter()
            .find(|s| s.t0_ms <= t_ms && t_ms < s.t1_ms)
            .or_else(|| self.segments.iter().rev().find(|s| s.t0_ms <= t_ms))
    }

    fn block_of(&self, seg_id: i64) -> Option<usize> {
        self.blocks
            .iter()
            .position(|b| b.segments.iter().any(|s| s.id == seg_id))
    }

    /// The playhead moved: mark its line, and keep it in view (unless the
    /// user scrolled the transcript a moment ago).
    fn follow_playhead(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        let line = self.line_at(t_ms).map(|s| s.id);
        if line == self.active_line {
            return;
        }
        let old_block = self.active_line.and_then(|id| self.block_of(id));
        self.active_line = line;
        if let Some(ix) = line.and_then(|id| self.block_of(id)) {
            if Some(ix) != old_block && self.follows_playhead(cx) {
                self.transcript_list.scroll_to_reveal_item(ix);
            }
        }
        cx.notify();
    }

    fn follows_playhead(&self, cx: &App) -> bool {
        follows_playhead(
            self.player.read(cx).is_playing(),
            self.scrolled_at.get(),
            cx.background_executor().now(),
        )
    }

    /// The transcript came into view (its tab, or the side-by-side
    /// column) while audio plays: bring the playing turn to the top. The
    /// list may not have measured its rows while hidden, so this scrolls
    /// to the turn rather than computing a minimal reveal.
    fn show_playhead(&mut self, cx: &mut Context<Self>) {
        if !self.follows_playhead(cx) {
            return;
        }
        if let Some(ix) = self.active_line.and_then(|id| self.block_of(id)) {
            self.transcript_list.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// The transcript's first visible turn (tests).
    #[cfg(test)]
    pub fn transcript_top(&self) -> usize {
        self.transcript_list.logical_scroll_top().item_ix
    }

    /// The turn holding the playing line (tests).
    #[cfg(test)]
    pub fn active_block(&self) -> Option<usize> {
        self.active_line.and_then(|id| self.block_of(id))
    }

    /// Record a user scroll of the transcript (tests; the list's scroll
    /// handler does this for real wheel events).
    #[cfg(test)]
    pub fn note_user_scroll(&self, cx: &App) {
        self.scrolled_at.set(Some(cx.background_executor().now()));
    }

    /// End the pause after a user scroll (tests; [`follows_playhead`] has
    /// the timing).
    #[cfg(test)]
    pub fn expire_user_scroll(&self) {
        self.scrolled_at.set(None);
    }

    /// Play from `t_ms` (an action item's quote, a deep link) and show
    /// that line.
    fn seek_to(&mut self, t_ms: i64, cx: &mut Context<Self>) {
        if self.has_audio() {
            self.player.update(cx, |p, cx| p.play_at(t_ms, cx));
        }
        if let Some(id) = self.line_at(t_ms).map(|s| s.id) {
            self.reveal_line(id, cx);
        }
    }

    /// A plain click on a line: play from it, or pause/resume when it is
    /// the line playing.
    fn play(&mut self, seg: &Segment, cx: &mut Context<Self>) {
        if !self.has_audio() {
            return;
        }
        let playing_here = self.active_line == Some(seg.id) && self.player.read(cx).is_playing();
        if playing_here {
            self.player.update(cx, |p, cx| p.pause(cx));
        } else {
            let t0 = seg.t0_ms;
            self.player.update(cx, |p, cx| p.play_at(t0, cx));
        }
        self.active_line = Some(seg.id);
        cx.notify();
    }

    /// Scroll the transcript to the line and highlight it for a moment.
    /// Shows the transcript first when it is a tab.
    fn reveal_line(&mut self, seg_id: i64, cx: &mut Context<Self>) {
        if !self.layout.side_by_side {
            self.detail_tab = DetailTab::Transcript;
        }
        if let Some(ix) = self.block_of(seg_id) {
            self.transcript_list.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
        self.flash = Some(seg_id);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_600))
                .await;
            this.update(cx, |this, cx| {
                if this.flash == Some(seg_id) {
                    this.flash = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // -- find -------------------------------------------------------------------

    /// ⌘F: open the find bar over the transcript.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.layout.side_by_side {
            self.detail_tab = DetailTab::Transcript;
        }
        self.find_open = true;
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub fn is_finding(&self) -> bool {
        self.find_open
    }

    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.find_matches.clear();
        self.find_current = 0;
        self.find_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_find(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.find_input
            .update(cx, |s, cx| s.set_value(text.clone(), window, cx));
        self.set_find(&text, cx);
    }

    #[cfg(test)]
    pub fn find_count(&self) -> usize {
        self.find_total()
    }

    fn find_total(&self) -> usize {
        self.find_matches.iter().map(|(_, r)| r.len()).sum()
    }

    fn set_find(&mut self, query: &str, cx: &mut Context<Self>) {
        self.find_matches = find_in_segments(&self.segments, query);
        self.find_current = 0;
        self.reveal_find(cx);
        cx.notify();
    }

    /// The current match: its line and its index among that line's ranges.
    fn find_at(&self, n: usize) -> Option<(i64, usize)> {
        let mut n = n;
        for (id, ranges) in &self.find_matches {
            if n < ranges.len() {
                return Some((*id, n));
            }
            n -= ranges.len();
        }
        None
    }

    fn find_step(&mut self, step: isize, cx: &mut Context<Self>) {
        let total = self.find_total();
        if total == 0 {
            return;
        }
        self.find_current = (self.find_current as isize + step).rem_euclid(total as isize) as usize;
        self.reveal_find(cx);
        cx.notify();
    }

    fn reveal_find(&mut self, _cx: &mut Context<Self>) {
        if let Some(ix) = self
            .find_at(self.find_current)
            .and_then(|(id, _)| self.block_of(id))
        {
            self.transcript_list.scroll_to_reveal_item(ix);
        }
    }

    // -- rename, delete, export -------------------------------------------------

    /// Edit the title in place.
    pub fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.meeting.as_ref().map(|m| m.title.clone()) else {
            return;
        };
        self.renaming = true;
        self.title_input.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_title(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.title_input
            .update(cx, |s, cx| s.set_value(text, window, cx));
    }

    pub fn is_renaming(&self) -> bool {
        self.renaming
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.renaming) {
            cx.notify();
        }
    }

    pub fn save_rename(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.renaming) {
            return;
        }
        cx.notify();
        let Some(title) = non_empty(&self.title_input.read(cx).value()) else {
            return;
        };
        if let Some(m) = &mut self.meeting {
            if m.title == title {
                return;
            }
            m.title = title.clone();
        }
        let id = self.meeting_id;
        run(
            cx,
            move |b| b.rename_meeting(id, &title),
            |this: &mut Self, r, cx| match r {
                Ok(()) => cx.emit(DetailEvent::Renamed),
                Err(e) => {
                    toast_error("meetings.list.renameError", &e, cx);
                    this.load(cx);
                }
            },
        );
    }

    /// Delete… (the page asks first).
    pub fn request_delete(&mut self, cx: &mut Context<Self>) {
        if self
            .meeting
            .as_ref()
            .is_some_and(|m| m.status != MeetingStatus::Recording)
        {
            cx.emit(DetailEvent::Delete(self.meeting_id));
        }
    }

    fn can_export(&self, cx: &App) -> bool {
        self.can_copy() || self.has_notes(cx)
    }

    /// The text Export writes for `kind`, if there is any.
    pub(crate) fn export_text(&self, kind: ExportKind, cx: &App) -> Option<String> {
        let m = self.meeting.as_ref()?;
        let date = format_meeting_date(m.started_at, &guidy_i18n::language());
        let transcript = (!self.segments.is_empty()).then(|| {
            crate::logic::meetings::transcript_text(
                &m.title,
                &date,
                &self.segments,
                &self.speakers,
                &self.people,
                &meeting_settings(cx).user_display_name,
            )
        });
        let notes = self.summary.read(cx).markdown(&date, true, cx);
        match kind {
            ExportKind::Transcript => transcript,
            ExportKind::Notes => notes,
            ExportKind::All => (notes.is_some() || transcript.is_some())
                .then(|| export_markdown(notes.as_deref(), transcript.as_deref())),
        }
    }

    /// Export…: a save panel, then the file.
    pub fn export(&mut self, kind: ExportKind, cx: &mut Context<Self>) {
        let Some(text) = self.export_text(kind, cx) else {
            return;
        };
        let title = self
            .meeting
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_default();
        let ext = if kind == ExportKind::Transcript {
            "txt"
        } else {
            "md"
        };
        let name = export_file_name(&title, ext);
        let dir = dirs_home().join("Documents");
        let dir = if dir.is_dir() { dir } else { dirs_home() };
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let r = cx
                .background_spawn(async move { std::fs::write(&path, text) })
                .await;
            if let Err(e) = r {
                cx.update(|cx| toast_error("meetings.export.error", &e.to_string(), cx));
            }
        })
        .detach();
    }

    /// Esc inside the detail: close find, cancel a rename, close the name
    /// picker, drop the line selection. False when there was nothing.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.find_open {
            self.close_find(window, cx);
        } else if self.renaming {
            self.cancel_rename(cx);
        } else if self.picker.is_some() {
            self.close_picker(cx);
        } else if !self.selection.is_empty() {
            self.clear_selection(cx);
        } else {
            return false;
        }
        true
    }

    /// ⌘E: everything there is, as Markdown.
    pub fn export_default(&mut self, cx: &mut Context<Self>) {
        self.export(ExportKind::All, cx);
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
            .enumerate()
            .map(|(i, row)| {
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
                let speaker_id = row.speaker_id;
                let lane = Rc::new(Cell::new(None::<Bounds<Pixels>>));
                let lane_paint = lane.clone();
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
                            .id(SharedString::from(format!("talk-lane-{i}")))
                            .relative()
                            .flex_1()
                            .h(dp(10.))
                            .child(
                                canvas(move |b, _, _| lane_paint.set(Some(b)), |_, _, _, _| {})
                                    .absolute()
                                    .size_full(),
                            )
                            // A click on (or just by) one of their marks
                            // goes to that line in the transcript.
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    let Some(b) = lane.get() else { return };
                                    let w = f32::from(b.size.width).max(1.);
                                    let x = f32::from(ev.position.x - b.left()).clamp(0., w);
                                    let t_ms = (x / w * total) as i64;
                                    let slack_ms = (6. / w * total) as i64;
                                    if let Some(id) =
                                        line_near(&this.segments, speaker_id, t_ms, slack_ms)
                                    {
                                        this.reveal_line(id, cx);
                                    }
                                }),
                            )
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
                                    .rounded(design::RADIUS_TRACK)
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
                .child(
                    h_flex()
                        .px(design::SECTION_X)
                        .pt(design::SECTION_Y)
                        .pb(dp(4.))
                        .justify_between()
                        .child(design::section_header(t("meetings.detail.talkTime"), cx))
                        .child(design::section_header(t("meetings.detail.share"), cx)),
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
                                .child(div().w(label_w).flex_none().child(design::section_header(
                                    t("meetings.detail.minutes"),
                                    cx,
                                )))
                                .child(
                                    h_flex().flex_1().justify_between().children(
                                        ticks
                                            .into_iter()
                                            .map(|tick| design::section_header(tick, cx)),
                                    ),
                                )
                                .child(div().w(share_w).flex_none()),
                        )
                        .test_support(),
                )
                .into_any_element(),
        )
    }

    /// Duration, speakers, words and, when the mic track was recorded,
    /// how much of the talking was yours.
    fn stats(&self, m: &Meeting) -> Vec<(SharedString, String, bool)> {
        let duration_ms = m.duration_ms().unwrap_or(self.talk.total_ms);
        let mut stats = vec![(
            t("meetings.detail.duration"),
            format_offset(duration_ms),
            false,
        )];
        if !self.segments.is_empty() {
            stats.push((
                t("meetings.detail.speakers"),
                self.speakers.len().to_string(),
                false,
            ));
            stats.push((
                t("meetings.detail.words"),
                self.talk.words.to_string(),
                false,
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
                stats.push((
                    t("meetings.detail.youSpoke"),
                    format!("{:.0}%", mine.share * 100.0),
                    true,
                ));
            }
        }
        stats
    }

    /// The meta line under the title: when, how long, how many speakers.
    fn meta_line(&self, m: &Meeting) -> String {
        let lang = guidy_i18n::language();
        let mut parts = vec![format_meeting_date(m.started_at, &lang)];
        if let Some(d) = m.duration_ms() {
            let mins = crate::logic::meetings::duration_minutes(d).to_string();
            parts.push(t_args("meetings.list.minutes", &[("count", &mins)]).to_string());
        }
        if !self.speakers.is_empty() {
            let n = self.speakers.len().to_string();
            parts.push(t_args("meetings.detail.speakerCount", &[("count", &n)]).to_string());
        }
        parts.join(" · ")
    }

    /// ⓘ: description, attendees and the numbers, in a popover.
    fn info_button(&self, m: &Meeting, cx: &mut Context<Self>) -> impl IntoElement {
        let description = m.description.clone();
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
        let stats = self.stats(m);
        let _ = cx;
        Popover::new("meeting-info")
            .anchor(Anchor::TopRight)
            .trigger(
                Button::new("meeting-detail-info")
                    .ghost()
                    .small()
                    .icon(icon("info"))
                    .tooltip(t("meetings.detail.info")),
            )
            .content(move |_, _, cx| {
                let tone = design::tone(cx);
                let columns = stats.len();
                let stats: Vec<Stat> = stats
                    .iter()
                    .map(|(l, v, accent)| Stat::new(l.clone(), v.clone()).accent(*accent))
                    .collect();
                v_flex()
                    .id("meeting-info-content")
                    .w(dp(320.))
                    .gap(dp(10.))
                    .when_some(description.clone(), |d, text| {
                        d.child(
                            div()
                                .id("meeting-info-description")
                                .text_size(design::TEXT_BODY)
                                .text_color(tone.text)
                                .child(text)
                                .test_support(),
                        )
                    })
                    .when(!attendees.is_empty(), |d| {
                        d.child(
                            v_flex()
                                .id("meeting-detail-attendees")
                                .gap(dp(4.))
                                .child(design::section_header(
                                    format!(
                                        "{} ({})",
                                        t("meetings.detail.attendees"),
                                        attendees.len()
                                    ),
                                    cx,
                                ))
                                .child(
                                    div()
                                        .text_size(design::TEXT_SECONDARY)
                                        .text_color(tone.text_muted)
                                        .child(attendees.join(", ")),
                                )
                                .test_support(),
                        )
                    })
                    .child(design::panel(cx).child(design::stat_grid(stats, columns.max(1), cx)))
                    .test_support()
            })
    }

    fn export_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let notes = self.has_notes(cx);
        let transcript = !self.segments.is_empty();
        Button::new("meeting-detail-export")
            .ghost()
            .small()
            .icon(icon("share"))
            .tooltip(t("meetings.export.title"))
            .disabled(!self.can_export(cx))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let item = |label: SharedString, kind: ExportKind, enabled: bool| {
                    let weak = weak.clone();
                    PopupMenuItem::new(label)
                        .disabled(!enabled)
                        .on_click(move |_, _, cx| {
                            weak.update(cx, |this, cx| this.export(kind, cx)).ok();
                        })
                };
                menu.item(item(
                    t("meetings.export.all"),
                    ExportKind::All,
                    notes || transcript,
                ))
                .separator()
                .item(item(t("meetings.export.notes"), ExportKind::Notes, notes))
                .item(item(
                    t("meetings.export.transcript"),
                    ExportKind::Transcript,
                    transcript,
                ))
            })
    }

    fn render_actions(&self, m: &Meeting, cx: &mut Context<Self>) -> impl IntoElement {
        let processing = m.status == MeetingStatus::Processing;
        let failed = m.status == MeetingStatus::Failed;
        let show_process = can_process(m) && m.status != MeetingStatus::Processed;
        h_flex()
            .id("meeting-detail-actions")
            .flex_none()
            .gap(dp(4.))
            .when(show_process, |d| {
                d.child(
                    Button::new("meeting-detail-process")
                        .primary()
                        .when(self.processing_busy, |b| b.secondary())
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
            })
            .when(processing, |d| {
                d.child(
                    Button::new("meeting-detail-cancel")
                        .secondary()
                        .small()
                        .icon(icon("circle-x"))
                        .label(t("meetings.detail.cancelShort"))
                        .tooltip(t("meetings.list.cancelProcessing"))
                        .loading(self.cancelling)
                        .disabled(self.cancelling)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_processing(cx))),
                )
            })
            .when(self.can_redo(), |d| {
                d.child(self.redo_button("meeting-detail-redo", false, cx))
            })
            .when(self.can_copy(), |d| d.child(self.copy_button(cx)))
            .child(self.export_button(cx))
            .child(
                Button::new("meeting-detail-generate-title")
                    .ghost()
                    .small()
                    .icon(icon("sparkles"))
                    .tooltip(if m.has_summary {
                        t("meetings.titles.generate")
                    } else {
                        t("meetings.titles.needsSummary")
                    })
                    .loading(self.titling)
                    .disabled(self.titling || !m.has_summary)
                    .on_click(cx.listener(|this, _, _, cx| this.generate_title(cx))),
            )
            .child(
                Button::new("meeting-detail-delete")
                    .ghost()
                    .small()
                    .icon(icon("trash"))
                    .tooltip(t("meetings.list.deleteMenu"))
                    .disabled(m.status == MeetingStatus::Recording)
                    .on_click(cx.listener(|this, _, _, cx| this.request_delete(cx))),
            )
            .child(self.info_button(m, cx))
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tone = design::tone(cx);
        let back = self.layout.show_back.then(|| {
            Button::new("meeting-back")
                .ghost()
                .small()
                .icon(icon("chevron-left"))
                .tooltip(t("meetings.detail.back"))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailEvent::Back)))
        });
        let Some(m) = self.meeting.clone() else {
            return h_flex()
                .id("meeting-detail-header")
                .flex_none()
                .h(dp(52.))
                .px(dp(12.))
                .border_b_1()
                .border_color(tone.hairline)
                .children(back)
                .into_any_element();
        };
        let processing = m.status == MeetingStatus::Processing;
        let failed = m.status == MeetingStatus::Failed;
        let title: AnyElement =
            if self.renaming {
                div()
                    .id("meeting-title-edit")
                    .flex_1()
                    .min_w_0()
                    .max_w(dp(420.))
                    .child(Input::new(&self.title_input).small())
                    .on_action(cx.listener(
                        |this, _: &gpui_kit::component::input::Escape, _, cx| {
                            this.cancel_rename(cx)
                        },
                    ))
                    .test_support()
                    .into_any_element()
            } else {
                h_flex()
                    .id("meeting-detail-title")
                    .group("meeting-detail-title")
                    .min_w_0()
                    .gap(dp(4.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(dp(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tone.text)
                            .child(m.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .opacity(0.)
                            .group_hover("meeting-detail-title", |s| s.opacity(1.))
                            .child(
                                Button::new("meeting-detail-rename")
                                    .ghost()
                                    .xsmall()
                                    .icon(icon("pencil"))
                                    .tooltip(t("meetings.list.rename"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_rename(window, cx)
                                    })),
                            ),
                    )
                    .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                        if ev.click_count() >= 2 {
                            this.start_rename(window, cx)
                        }
                    }))
                    .test_support()
                    .into_any_element()
            };
        let chip = (m.status != MeetingStatus::Processed).then(|| {
            design::chip(t(status_key(m.status)), processing, cx).when(failed, |d| {
                d.bg(theme.danger.opacity(0.12)).text_color(theme.danger)
            })
        });
        let title_row = h_flex()
            .w_full()
            .gap(dp(6.))
            .children(back)
            .child(title)
            .children(chip)
            .child(div().flex_1())
            .child(self.render_actions(&m, cx));
        let meta = h_flex()
            .w_full()
            .min_w_0()
            .gap(dp(6.))
            .when(self.layout.show_back, |d| d.pl(dp(30.)))
            .child(
                div()
                    .id("meeting-detail-meta")
                    .flex_none()
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(self.meta_line(&m))
                    .test_support(),
            )
            .when_some(m.error.clone().filter(|_| failed), |d, e| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(theme.danger)
                        .child(e),
                )
            })
            .when(!failed, |d| {
                d.when_some(m.description.clone(), |d, text| {
                    d.child(
                        div()
                            .id("meeting-detail-description")
                            .min_w_0()
                            .truncate()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_faint)
                            .child(format!("— {text}"))
                            .test_support(),
                    )
                })
            });
        let progress = processing.then(|| {
            let fraction = self
                .progress
                .as_ref()
                .map(|p| p.fraction)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let stage = self
                .progress
                .as_ref()
                .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                .unwrap_or_else(|| t("meetings.status.processing"));
            h_flex()
                .w_full()
                .gap(dp(10.))
                .pt(dp(2.))
                .child(
                    div()
                        .flex_none()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(tone.text_muted)
                        .child(design::shimmer_text(
                            "meeting-detail-stage",
                            progress_label(&stage, self.progress.as_ref()),
                            cx,
                        )),
                )
                .child(div().flex_1().child(design::track(
                    "meeting-detail-progress",
                    Some(fraction),
                    cx,
                )))
        });
        // The player is the header's last row (not a bar of its own), so
        // the header stays short at the minimum window size.
        let player = self.has_audio().then(|| self.player.clone());
        v_flex()
            .id("meeting-detail-header")
            .flex_none()
            .w_full()
            .px(dp(14.))
            .pt(dp(8.))
            .pb(if player.is_some() { dp(2.) } else { dp(8.) })
            .gap(dp(2.))
            .border_b_1()
            .border_color(tone.hairline)
            .child(title_row)
            .child(meta)
            .children(progress)
            .children(player)
            .test_support()
            .into_any_element()
    }

    fn render_picker(&self, id: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let heading = |key: &str| design::section_header(t(key), cx);
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
                    .when(self.picker == Some(PickTarget::Speaker(sid)), |d| {
                        d.child(self.render_picker(format!("speaker-picker-{sid}").into(), cx))
                    })
                    .into_any_element()
            })
            .collect();
        let open = self.speakers_open;
        let pending = self.pending_speakers();
        let tone = design::tone(cx);
        let warning = cx.theme().warning;
        // One chip per speaker with their share of the talking, longest
        // talker first; a voice that needs a name is amber.
        let share = |id: SpeakerId| {
            self.talk
                .speakers
                .iter()
                .find(|s| s.speaker_id == Some(id))
                .map(|s| s.share)
                .unwrap_or(0.0)
        };
        let mut order: Vec<&MeetingSpeaker> = self.speakers.iter().collect();
        order.sort_by(|a, b| {
            share(b.id)
                .partial_cmp(&share(a.id))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let chips: Vec<AnyElement> = order
            .iter()
            .map(|s| {
                let sid = s.id;
                let needs = !s.is_user && (s.person_id.is_none() || needs_confirmation(s));
                let pct = format!("{:.0}%", share(sid) * 100.0);
                h_flex()
                    .id(SharedString::from(format!("speaker-chip-{sid}")))
                    .flex_none()
                    .h(dp(20.))
                    .px(dp(6.))
                    .gap(dp(4.))
                    .rounded(design::RADIUS_CHIP)
                    .text_size(design::TEXT_SECONDARY)
                    .when(needs, |d| d.bg(warning.opacity(0.12)).text_color(warning))
                    .when(!needs, |d| {
                        d.bg(tone.raised).text_color(if s.is_user {
                            tone.accent_text
                        } else {
                            tone.text
                        })
                    })
                    .child(
                        div()
                            .max_w(dp(120.))
                            .truncate()
                            .child(self.speaker_label(s, cx)),
                    )
                    .child(
                        div()
                            .font_family(crate::theme::MONO_FONT)
                            .text_color(if needs { warning } else { tone.text_muted })
                            .child(pct),
                    )
                    .when(!s.is_user, |d| {
                        d.hover(move |s| s.opacity(0.85)).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.speakers_open = true;
                                this.open_picker(sid, window, cx)
                            },
                        ))
                    })
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let strip = h_flex()
            .id("meeting-speakers")
            .w_full()
            .flex_none()
            .px(dp(12.))
            .py(dp(6.))
            .gap(dp(6.))
            .border_b_1()
            .border_color(tone.hairline)
            .child(
                Button::new("meeting-speakers-fold")
                    .ghost()
                    .xsmall()
                    .icon(icon(if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    }))
                    .label(if pending > 0 {
                        t_args(
                            "meetings.detail.speakersUnnamed",
                            &[("count", &pending.to_string())],
                        )
                    } else {
                        t("meetings.detail.speakers")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.speakers_open = !this.speakers_open;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(dp(4.))
                    .overflow_hidden()
                    .children(chips),
            )
            .test_support();
        let panel = open.then(|| {
            div()
                .id("meeting-speakers-panel")
                .flex_none()
                .w_full()
                .max_h(dp(280.))
                .overflow_y_scroll()
                .track_scroll(&self.speakers_scroll)
                .border_b_1()
                .border_color(tone.hairline)
                .bg(tone.surface)
                .child(v_flex().w_full().children(rows))
                .children(
                    self.render_talk_time(cx)
                        .map(|chart| div().border_t_1().border_color(tone.hairline).child(chart)),
                )
                .test_support()
        });
        Some(
            v_flex()
                .w_full()
                .flex_none()
                .child(strip)
                .children(panel)
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
        let player_playing = self.player.read(cx).is_playing();
        let find_current = self.find_at(self.find_current);
        let block_ids: Vec<i64> = block.segments.iter().map(|s| s.id).collect();
        // The line picker opens right under the (first) line it names, so it
        // is next to where it was clicked even in a long turn.
        let picker_line = match &self.picker {
            Some(PickTarget::Lines(ids)) => self.picker_at.or(ids.first().copied()),
            _ => None,
        };
        // The selection's bar goes under its last line, unless the picker
        // already opened there; not mid-drag, where it would push the lines
        // being dragged over down.
        let bar_line = self
            .selection
            .last()
            .copied()
            .filter(|_| picker_line.is_none() && self.drag_start.is_none());
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
                        let active = self.active_line == Some(seg.id);
                        let playing = active && player_playing;
                        let found = self
                            .find_matches
                            .iter()
                            .find(|(id, _)| *id == seg.id)
                            .map(|(_, r)| r.clone());
                        let current = find_current.filter(|(id, _)| *id == seg.id).map(|(_, n)| n);
                        let s = seg.clone();
                        let seg_id = seg.id;
                        let selected = self.selection.contains(&seg_id);
                        let line = h_flex()
                            .id(SharedString::from(format!("segment-{}", seg.id)))
                            .group(SharedString::from(format!("segment-{}", seg.id)))
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_start()
                            .px(dp(6.))
                            .py(px(1.))
                            .rounded(design::RADIUS_CHIP)
                            .relative()
                            .when(active, |d| d.bg(tone.accent_soft))
                            .when(
                                !active
                                    && self
                                        .what_changed
                                        .as_ref()
                                        .is_some_and(|d| d.changed_lines.contains(&seg_id)),
                                |d| d.bg(tone.accent_soft),
                            )
                            .when(self.flash == Some(seg_id) && !selected, |d| {
                                d.bg(tone.accent.opacity(0.18))
                            })
                            .when(selected, |d| {
                                d.bg(tone.accent.opacity(0.18))
                                    .border_l_2()
                                    .border_color(tone.accent)
                            })
                            .when(!selected, |d| {
                                d.hover(move |d| d.bg(tone.raised.opacity(0.5)))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                    window.focus(&this.focus, cx);
                                    let m = ev.modifiers;
                                    this.dragged = false;
                                    this.drag_start =
                                        (!m.shift && !m.platform && !m.control).then_some(seg_id);
                                }),
                            )
                            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                                this.drag_over(
                                    seg_id,
                                    ev.pressed_button == Some(MouseButton::Left),
                                    cx,
                                )
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    // Released on another line: the drag
                                    // ends on it, even before a move got there.
                                    if this.drag_start.is_some_and(|s| s != seg_id) {
                                        this.drag_over(seg_id, true, cx);
                                    }
                                    this.drag_start = None;
                                    cx.notify();
                                }),
                            )
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
                                    .map(|d| match &found {
                                        Some(ranges) => d.child(super::list::highlighted_with(
                                            &seg.text, ranges, current, cx,
                                        )),
                                        None => d.child(seg.text.clone()),
                                    }),
                            )
                            // "Who said this?": name one line's speaker.
                            .child(
                                div()
                                    .id(SharedString::from(format!("segment-{seg_id}-tag")))
                                    .flex_none()
                                    .mt(dp(3.))
                                    .p(dp(3.))
                                    .rounded(crate::ui::design::RADIUS_PANEL)
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
                            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                                let m = ev.modifiers();
                                let click = if m.shift {
                                    LineClick::Range
                                } else if m.platform || m.control {
                                    LineClick::Toggle
                                } else {
                                    LineClick::Plain
                                };
                                this.click_line(&s, click, cx)
                            }))
                            .test_support();
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .child(line)
                            .when(picker_line == Some(seg_id), |d| {
                                d.child(div().pt(dp(6.)).pb(dp(10.)).child(
                                    self.render_picker(format!("line-picker-{seg_id}").into(), cx),
                                ))
                            })
                            .when(bar_line == Some(seg_id), |d| {
                                d.child(self.render_selection_bar(cx))
                            })
                    })),
            )
            .test_support();
        row.into_any_element()
    }

    /// Under the selected lines: how many, and "Set speaker" for all of
    /// them at once.
    fn render_selection_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let n = self.selection.len();
        h_flex()
            .id("selection-bar")
            .w_full()
            .mt(dp(4.))
            .mb(dp(6.))
            .px(dp(10.))
            .py(dp(6.))
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::RADIUS_PANEL)
            .bg(tone.raised)
            .child(
                div()
                    .text_size(dp(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(t_args(
                        "meetings.selection.count",
                        &[("count", &n.to_string())],
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(dp(11.))
                    .text_color(tone.text_faint)
                    .child(t("meetings.selection.keys")),
            )
            .child(
                Button::new("selection-clear")
                    .ghost()
                    .small()
                    .label(t("meetings.selection.clear"))
                    .on_click(cx.listener(|this, _, _, cx| this.clear_selection(cx))),
            )
            .child(
                Button::new("selection-tag")
                    .primary()
                    .small()
                    .icon(icon("user-round-pen"))
                    .label(t("meetings.selection.setSpeaker"))
                    .on_click(cx.listener(|this, _, window, cx| this.tag_selection(window, cx))),
            )
            .test_support()
            .into_any_element()
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
                    design::section_header(
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
                        // A turn with only tentative words has no settled
                        // text: no empty line above them.
                        .when(!r.text.is_empty(), |d| d.child(div().child(r.text.clone())))
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
                .w_full()
                .children(rows)
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
                .flex_none()
                .px(dp(12.))
                .pt(dp(8.))
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

    /// Insert a streamed line in time order. The same span updates in place
    /// (a language pass would, if it announced; the pipeline announces once).
    /// Cloud batches finish one track before the other, so lines can arrive
    /// out of order; [`insert_provisional`] still places them on the clock.
    fn push_provisional(&mut self, line: ProvisionalLine) {
        insert_provisional(&mut self.provisional, line);
    }

    fn render_provisional(&self, line: &ProvisionalLine, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        h_flex()
            .id(SharedString::from(format!(
                "provisional-{}-{}",
                line.track.as_str(),
                line.t0_ms
            )))
            .w_full()
            .gap(dp(8.))
            .px_4()
            .py(dp(6.))
            .text_size(design::TEXT_BODY)
            .text_color(tone.text_muted)
            .child(
                div()
                    .w(dp(52.))
                    .text_color(tone.text_faint)
                    .child(format_offset(line.t0_ms)),
            )
            .child(div().child(t("meetings.detail.speakerPending")))
            .child(div().flex_1().min_w_0().child(line.text.clone()))
            .test_support()
            .into_any_element()
    }

    /// The transcript column's tools: find and copy.
    fn transcript_tools(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_lines = !self.loading && !self.segments.is_empty();
        h_flex().flex_none().gap(dp(2.)).when(has_lines, |d| {
            d.child(
                Button::new("transcript-find")
                    .ghost()
                    .xsmall()
                    .icon(icon("search"))
                    .tooltip(t("meetings.find.tooltip"))
                    .selected(self.find_open)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.find_open {
                            this.close_find(window, cx)
                        } else {
                            this.open_find(window, cx)
                        }
                    })),
            )
            .child(
                Button::new("transcript-copy")
                    .ghost()
                    .xsmall()
                    .icon(icon(if self.transcript_copied {
                        "check"
                    } else {
                        "copy"
                    }))
                    .tooltip(if self.transcript_copied {
                        t("meetings.copy.copied")
                    } else {
                        t("meetings.copy.transcript")
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.copy_transcript(cx))),
            )
        })
    }

    /// Find in the transcript: the field, "3 of 12", previous / next, Done.
    fn render_find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.find_open {
            return None;
        }
        let tone = design::tone(cx);
        let total = self.find_total();
        let query_empty = self.find_input.read(cx).value().trim().is_empty();
        let count = if query_empty {
            SharedString::default()
        } else if total == 0 {
            t("meetings.find.none")
        } else {
            t_args(
                "meetings.find.count",
                &[
                    ("current", &(self.find_current + 1).to_string()),
                    ("total", &total.to_string()),
                ],
            )
        };
        Some(
            h_flex()
                .id("transcript-find-bar")
                .flex_none()
                .w_full()
                .px(dp(10.))
                .py(dp(6.))
                .gap(dp(6.))
                .border_b_1()
                .border_color(tone.hairline)
                .bg(tone.surface)
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.find_input)
                            .xsmall()
                            .prefix(icon("search").xsmall().text_color(tone.text_muted)),
                    ),
                )
                .on_action(cx.listener(
                    |this, _: &gpui_kit::component::input::Escape, window, cx| {
                        this.close_find(window, cx)
                    },
                ))
                .child(
                    div()
                        .id("transcript-find-count")
                        .flex_none()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(tone.text_muted)
                        .child(count)
                        .test_support(),
                )
                .child(
                    Button::new("transcript-find-prev")
                        .ghost()
                        .xsmall()
                        .icon(icon("chevron-up"))
                        .tooltip(t("meetings.find.previous"))
                        .disabled(total == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(-1, cx))),
                )
                .child(
                    Button::new("transcript-find-next")
                        .ghost()
                        .xsmall()
                        .icon(icon("chevron-down"))
                        .tooltip(t("meetings.find.next"))
                        .disabled(total == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(1, cx))),
                )
                .child(
                    Button::new("transcript-find-done")
                        .ghost()
                        .xsmall()
                        .label(t("meetings.find.done"))
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .test_support()
                .into_any_element(),
        )
    }

    /// The transcript below its header: find bar, speakers, then the lines
    /// in their own (virtualized) scroll region.
    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let note = |id: &'static str, text: SharedString| {
            div()
                .id(id)
                .px(dp(16.))
                .py(dp(24.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(text)
                .test_support()
                .into_any_element()
        };
        let scroll = |id: &'static str, child: AnyElement, handle: &ScrollHandle| {
            div()
                .id(id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(handle)
                .child(child)
                .into_any_element()
        };
        if let Some(live) = self.render_live_transcript(cx) {
            return scroll("meeting-live-scroll", live, &self.lines_scroll);
        }
        let processing = self
            .meeting
            .as_ref()
            .is_some_and(|m| m.status == MeetingStatus::Processing);
        // A redo keeps the stored transcript, dimmed, until the first new
        // line arrives. Then the streamed lines replace it until merge.
        let show_provisional = processing && !self.provisional.is_empty();
        let dim_old = processing && !self.segments.is_empty() && self.provisional.is_empty();
        let lines: AnyElement = if self.loading {
            note("transcript-loading", t("meetings.detail.loading"))
        } else if show_provisional {
            scroll(
                "transcript-provisional",
                v_flex()
                    .w_full()
                    .py(dp(4.))
                    .children(
                        self.provisional
                            .iter()
                            .map(|line| self.render_provisional(line, cx)),
                    )
                    .into_any_element(),
                &self.lines_scroll,
            )
        } else if self.segments.is_empty() {
            note(
                "transcript-empty",
                if processing {
                    t("meetings.detail.transcribing")
                } else {
                    t("meetings.detail.empty")
                },
            )
        } else {
            let view = cx.entity().downgrade();
            div()
                .id("transcript-lines")
                .flex_1()
                .min_h_0()
                .w_full()
                .when(dim_old, |d| d.opacity(0.45))
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::on_key_down))
                // A drag let go outside the transcript ends there.
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        if this.drag_start.take().is_some() {
                            cx.notify();
                        }
                    }),
                )
                .child(
                    list(self.transcript_list.clone(), move |ix, _, cx| {
                        view.update(cx, |this, cx| match this.blocks.get(ix).cloned() {
                            Some(block) => div()
                                .w_full()
                                .border_b_1()
                                .border_color(design::tone(cx).hairline)
                                .child(this.render_block(ix, &block, cx))
                                .into_any_element(),
                            None => div().into_any_element(),
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                    })
                    .size_full(),
                )
                .test_support()
                .into_any_element()
        };
        v_flex()
            .id("meeting-transcript")
            .size_full()
            .min_h_0()
            .children(self.render_find_bar(cx))
            .children(self.render_speakers(cx))
            .child(lines)
            .test_support()
            .into_any_element()
    }

    fn can_copy(&self) -> bool {
        !self.segments.is_empty() || self.meeting.as_ref().is_some_and(|m| m.has_summary)
    }

    /// The summary view has notes loaded, or the meeting record says it
    /// has a summary that has not arrived in the view yet.
    fn has_notes(&self, cx: &App) -> bool {
        self.meeting.as_ref().is_some_and(|m| m.has_summary) || self.summary.read(cx).has_summary()
    }

    /// Notes when they exist. With only a transcript, the default copies
    /// that instead. Nothing is marked copied unless text was written.
    fn copy_notes(&mut self, cx: &mut Context<Self>) {
        let view_has = self.summary.read(cx).has_summary();
        if view_has {
            self.summary.update(cx, |v, cx| v.copy(cx));
            self.flash_copied(cx);
            return;
        }
        // The meeting has a summary the view has not loaded. Don't claim
        // the copy, and don't fall through to the transcript.
        if self.meeting.as_ref().is_some_and(|m| m.has_summary) {
            return;
        }
        if self.write_transcript(cx) {
            self.flash_copied(cx);
        }
    }

    fn flash_copied(&mut self, cx: &mut Context<Self>) {
        self.notes_copied = true;
        cx.notify();
        cx.spawn(async |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_500))
                .await;
            this.update(cx, |this, cx| {
                this.notes_copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn copy_button(&self, cx: &mut Context<Self>) -> DropdownButton {
        let copied = self.notes_copied;
        let notes = self.has_notes(cx);
        let label = if copied {
            t("meetings.copy.copied")
        } else if notes {
            t("meetings.copy.notes")
        } else {
            t("meetings.copy.transcript")
        };
        let icon_only = self.layout.compact;
        let main = Button::new("summary-copy")
            .ghost()
            .small()
            .icon(icon(if copied { "check" } else { "copy" }))
            .map(|b| {
                if icon_only {
                    b.tooltip(label)
                } else {
                    b.label(label)
                }
            })
            .on_click(cx.listener(|this, _, _, cx| this.copy_notes(cx)));
        let weak = cx.entity().downgrade();
        DropdownButton::new("meeting-copy-split")
            .button(main)
            .menu_accessibility_label(t("meetings.copy.more"))
            .dropdown_menu(move |menu, _, _| {
                let weak_transcript = weak.clone();
                let weak_summary = weak.clone();
                menu.item(PopupMenuItem::new(t("meetings.copy.transcript")).on_click(
                    move |_, _, cx| {
                        weak_transcript
                            .update(cx, |this, cx| this.copy_transcript(cx))
                            .ok();
                    },
                ))
                .item(
                    PopupMenuItem::new(t("meetings.copy.summaryOnly")).on_click(move |_, _, cx| {
                        weak_summary
                            .update(cx, |this, cx| {
                                this.summary.update(cx, |v, cx| v.copy_summary_only(cx));
                            })
                            .ok();
                    }),
                )
            })
    }

    fn render_what_changed(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let diff = self.what_changed.as_ref()?;
        let tone = design::tone(cx);
        let count = diff.lines_changed.to_string();
        let line_count = t_args("meetings.changed.lines", &[("count", &count)]);
        let shift_key = |shift: &SpeakerShift| match shift {
            SpeakerShift::Renamed { from, to } => format!("rename-{from}-{to}"),
            SpeakerShift::Merged { from, into } => format!("merge-{from}-{into}"),
            SpeakerShift::Moved { from, to, .. } => format!("move-{from}-{to}"),
            SpeakerShift::Added { label } => format!("added-{label}"),
            SpeakerShift::Removed { label } => format!("removed-{label}"),
        };
        let shifts: Vec<_> = diff
            .speakers
            .iter()
            .map(|shift| {
                let text = match shift {
                    SpeakerShift::Renamed { from, to } => {
                        t_args("meetings.changed.renamed", &[("from", from), ("to", to)])
                    }
                    SpeakerShift::Merged { from, into } => {
                        t_args("meetings.changed.merged", &[("from", from), ("into", into)])
                    }
                    SpeakerShift::Moved { count, from, to } => {
                        let count_s = count.to_string();
                        t_args(
                            "meetings.changed.moved",
                            &[("count", &count_s), ("from", from), ("to", to)],
                        )
                    }
                    SpeakerShift::Added { label } => {
                        t_args("meetings.changed.added", &[("label", label)])
                    }
                    SpeakerShift::Removed { label } => {
                        t_args("meetings.changed.removed", &[("label", label)])
                    }
                };
                div()
                    .id(SharedString::from(format!(
                        "what-changed-shift-{}",
                        shift_key(shift)
                    )))
                    .text_size(dp(12.5))
                    .text_color(tone.text)
                    .child(text)
                    .test_support()
            })
            .collect::<Vec<_>>();
        let by = |share_of: fn(&crate::logic::meetings::ShareShift) -> f32| {
            let mut order: Vec<_> = diff.shares.iter().filter(|s| share_of(s) > 0.0).collect();
            order.sort_by(|a, b| {
                share_of(b)
                    .partial_cmp(&share_of(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.label.cmp(&b.label))
            });
            order
        };
        let before_order = by(|s| s.before);
        let after_order = by(|s| s.after);
        let mut rows: Vec<_> = diff.shares.iter().collect();
        rows.sort_by(|a, b| {
            b.after
                .partial_cmp(&a.after)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.label.cmp(&b.label))
        });
        let bars: Vec<_> = rows
            .iter()
            .map(|share| {
                let before = share.before;
                let after = share.after;
                let label = share.label.clone();
                let id = SharedString::from(format!("what-changed-bar-{label}"));
                let before_pct = (before * 100.0).round() as i32;
                let after_pct = (after * 100.0).round() as i32;
                let before_s = before_pct.to_string();
                let after_s = after_pct.to_string();
                let pct = t_args(
                    "meetings.changed.share",
                    &[("before", &before_s), ("after", &after_s)],
                );
                let before_i = before_order.iter().position(|s| s.label == label);
                let after_i = after_order.iter().position(|s| s.label == label);
                let rank = match (before_i, after_i) {
                    (Some(b), Some(a)) if b != a => {
                        let delta = b as i32 - a as i32;
                        let n = delta.unsigned_abs().to_string();
                        Some(if delta > 0 {
                            t_args("meetings.changed.rankUp", &[("n", &n)])
                        } else {
                            t_args("meetings.changed.rankDown", &[("n", &n)])
                        })
                    }
                    _ => None,
                };
                h_flex()
                    .id(SharedString::from(format!("what-changed-share-{label}")))
                    .w_full()
                    .gap(dp(8.))
                    .items_center()
                    .child(
                        div()
                            .w(dp(120.))
                            .flex_none()
                            .min_w_0()
                            .text_size(dp(12.))
                            .text_color(tone.text)
                            .child(label),
                    )
                    // Same slot on every row, empty when the rank didn't move,
                    // so the bars start at the same x.
                    .child(
                        div()
                            .w(dp(32.))
                            .flex_none()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(dp(11.))
                            .text_right()
                            .text_color(tone.text_muted)
                            .child(rank.unwrap_or_default()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(dp(8.))
                            .rounded_full()
                            .bg(tone.raised)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .rounded_full()
                                    .bg(tone.accent)
                                    .with_animation(
                                        id,
                                        Animation::new(Duration::from_millis(700)),
                                        move |el, delta| {
                                            let w = before + (after - before) * delta;
                                            el.w(relative(w.clamp(0.0, 1.0)))
                                        },
                                    ),
                            )
                            .when(before > 0.01, |track| {
                                track.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .h_full()
                                        .w(px(2.))
                                        .bg(tone.text.opacity(0.45))
                                        .left(relative(before.clamp(0.0, 1.0))),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w(dp(88.))
                            .flex_none()
                            .text_size(dp(11.))
                            .text_color(tone.text_muted)
                            .child(pct),
                    )
                    .test_support()
            })
            .collect();
        let open = self.what_changed_open;
        let accent = tone.accent_text;
        Some(
            div()
                .id("what-changed")
                .flex_none()
                .px(dp(12.))
                .pt(dp(8.))
                .child(
                    design::notice(accent).child(
                        v_flex()
                            .w_full()
                            .gap(dp(6.))
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap(dp(8.))
                                    .child(icon("git-compare").xsmall().text_color(accent))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(dp(12.))
                                            .text_color(tone.text)
                                            .child(format!(
                                                "{} · {}",
                                                t("meetings.changed.title"),
                                                line_count
                                            )),
                                    )
                                    .child(
                                        Button::new("what-changed-details")
                                            .ghost()
                                            .xsmall()
                                            .label(if open {
                                                t("meetings.changed.hide")
                                            } else {
                                                t("meetings.changed.details")
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.what_changed_open = !this.what_changed_open;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("what-changed-dismiss")
                                            .ghost()
                                            .xsmall()
                                            .icon(icon("x"))
                                            .tooltip(t("meetings.changed.dismiss"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.what_changed = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(open, |d| d.children(shifts).children(bars)),
                    ),
                )
                .test_support()
                .into_any_element(),
        )
    }

    /// The segmented control over the notes column; with `transcript`
    /// (one column) Transcript is its last option.
    fn render_tabs(&self, transcript: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.summary.read(cx).item_count();
        let tickets = self.tickets.read(cx).count();
        let notes = self.detail_tab == DetailTab::Notes || self.layout.side_by_side;
        let with_count = |key: &str, n: usize| {
            if n == 0 {
                t(key)
            } else {
                SharedString::from(format!("{} {n}", t(key)))
            }
        };
        let mut tabs: Vec<(&'static str, SharedString, bool, Option<NotesTab>)> = vec![
            (
                "notes",
                t("meetings.detail.notesTab"),
                notes && self.notes_tab == NotesTab::Summary,
                Some(NotesTab::Summary),
            ),
            (
                "items",
                with_count("meetings.detail.itemsTab", items),
                notes && self.notes_tab == NotesTab::ActionItems,
                Some(NotesTab::ActionItems),
            ),
            (
                "tickets",
                with_count("meetings.detail.ticketsTab", tickets),
                notes && self.notes_tab == NotesTab::Tickets,
                Some(NotesTab::Tickets),
            ),
        ];
        if transcript {
            tabs.push((
                "transcript",
                t("meetings.detail.transcript"),
                self.detail_tab == DetailTab::Transcript,
                None,
            ));
        }
        design::segmented(cx).children(tabs.into_iter().map(|(id, label, selected, tab)| {
            design::segment(
                SharedString::from(format!("meeting-detail-tab-{id}")),
                label,
                selected,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                match tab {
                    Some(tab) => {
                        if this.notes_tab != tab {
                            this.notes_scroll.set_offset(point(px(0.), px(0.)));
                        }
                        this.notes_tab = tab;
                        this.detail_tab = DetailTab::Notes;
                    }
                    None => this.select_tab(DetailTab::Transcript, cx),
                }
                cx.notify();
            }))
            .test_support()
        }))
    }

    /// A column's header bar.
    fn column_header(&self, id: &'static str, cx: &App) -> Stateful<Div> {
        let tone = design::tone(cx);
        h_flex()
            .id(id)
            .flex_none()
            .w_full()
            .h(dp(36.))
            .px(dp(10.))
            .gap(dp(6.))
            .border_b_1()
            .border_color(tone.hairline)
    }

    /// The notes column's content in its own scroll region.
    fn render_notes(&self, cx: &mut Context<Self>) -> AnyElement {
        let body: AnyElement = match self.notes_tab {
            NotesTab::Summary | NotesTab::ActionItems => self.summary.clone().into_any_element(),
            NotesTab::Tickets => self.tickets.clone().into_any_element(),
        };
        let _ = cx;
        div()
            .id("meeting-notes-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.notes_scroll)
            .child(
                div()
                    .w_full()
                    .px(dp(14.))
                    .pt(dp(12.))
                    .pb(dp(20.))
                    .child(body),
            )
            .into_any_element()
    }

    /// Copy the transcript as `[mm:ss] Speaker: text` lines under the
    /// meeting's title and date. Returns whether anything was written.
    fn write_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(m) = &self.meeting else { return false };
        if self.segments.is_empty() {
            return false;
        }
        let text = crate::logic::meetings::transcript_text(
            &m.title,
            &format_meeting_date(m.started_at, &guidy_i18n::language()),
            &self.segments,
            &self.speakers,
            &self.people,
            &meeting_settings(cx).user_display_name,
        );
        if text.trim().is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Copy the transcript as `[mm:ss] Speaker: text` lines under the
    /// meeting's title and date.
    fn copy_transcript(&mut self, cx: &mut Context<Self>) {
        if !self.write_transcript(cx) {
            return;
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let side = self.layout.side_by_side;
        let section = match self.notes_tab {
            NotesTab::ActionItems => SummarySection::ActionItems,
            _ => SummarySection::Notes,
        };
        self.summary.update(cx, |v, _| v.set_section(section));
        let model_banner = self
            .meeting
            .as_ref()
            .filter(|m| matches!(m.status, MeetingStatus::Recorded | MeetingStatus::Failed))
            .and_then(|_| meeting_model_missing(cx))
            .map(|missing| {
                div()
                    .flex_none()
                    .px(dp(12.))
                    .pt(dp(8.))
                    .child(render_model_download("detail", &missing, cx))
            });
        let body: AnyElement = if side {
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_stretch()
                .child(
                    v_flex()
                        .id("meeting-notes-column")
                        .flex_1()
                        .flex_basis(px(0.))
                        .min_w_0()
                        .h_full()
                        .border_r_1()
                        .border_color(tone.hairline)
                        .child(
                            self.column_header("meeting-notes-header", cx)
                                .child(self.render_tabs(false, cx)),
                        )
                        .child(self.render_notes(cx)),
                )
                .child(
                    v_flex()
                        .id("meeting-transcript-column")
                        .flex_1()
                        .flex_basis(px(0.))
                        .min_w_0()
                        .h_full()
                        .child(
                            self.column_header("meeting-transcript-header", cx)
                                .child(
                                    div()
                                        .text_size(design::TEXT_BODY)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(tone.text)
                                        .child(t("meetings.detail.transcript")),
                                )
                                .child(div().flex_1())
                                .child(self.transcript_tools(cx)),
                        )
                        .child(self.render_transcript(cx)),
                )
                .into_any_element()
        } else {
            let transcript = self.detail_tab == DetailTab::Transcript;
            v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    self.column_header("meeting-notes-header", cx)
                        .child(self.render_tabs(true, cx))
                        .child(div().flex_1())
                        .when(transcript, |d| d.child(self.transcript_tools(cx))),
                )
                .child(if transcript {
                    self.render_transcript(cx)
                } else {
                    self.render_notes(cx)
                })
                .into_any_element()
        };
        v_flex()
            .id("meeting-detail")
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(self.render_header(cx))
            .children(self.render_what_changed(cx))
            .children(self.render_quality_notice(cx))
            .children(model_banner)
            .child(body)
            .test_support()
    }
}

/// The user's home directory (the save panel starts in its Documents).
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}
