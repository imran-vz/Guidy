//! The meeting list pane: a search field pinned on top, the meetings
//! grouped by day (Today, Yesterday, Previous 7 Days…) in a virtualized
//! list, and a footer with the count and "Generate Titles". A whole row
//! selects its meeting; its ⋯ button and right-click menu rename, title,
//! process, cancel and delete. A search lists the matching meetings with
//! their matching lines (a line opens the meeting there).

use super::{meeting_settings, meetings_backend, run, toast_error};
use crate::logic::meetings::{
    apply_progress, can_process, duration_minutes, list_rows, non_empty, progress_label,
    status_key, ListRow, MeetingsEvent,
};
use crate::logic::models::format_model_size;
use crate::meetings_backend::MeetingsError;
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args, LocalizedPlaceholder};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::search::{MatchField, SearchHit, SearchResults, Snippet};
use guidy_core::meetings::types::{Meeting, MeetingId, MeetingStatus, ProcessingProgress};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

pub enum MeetingListEvent {
    /// A row was clicked: show this meeting.
    Open(MeetingId),
    /// Open a meeting at a transcript line (a search hit).
    OpenLine {
        meeting_id: MeetingId,
        segment_id: i64,
    },
    /// The meeting was deleted (after the confirmation). `next` is the
    /// meeting to show instead.
    Deleted {
        id: MeetingId,
        next: Option<MeetingId>,
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
    /// `meetings` as list rows (day headers and meetings).
    rows: Vec<ListRow>,
    list_state: ListState,
    /// Search results scroll on their own.
    results_scroll: ScrollHandle,
    /// The meeting shown in the detail.
    selected: Option<MeetingId>,
    /// Takes ↑ ↓ Enter ⌫ (key context `MeetingList`).
    focus: FocusHandle,
    loading: bool,
    progress: HashMap<MeetingId, ProcessingProgress>,
    renaming: Option<MeetingId>,
    rename_input: Entity<InputState>,
    busy: HashSet<MeetingId>,
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
    /// "Not now" on the speaker-turn download prompt, for this visit.
    turn_prompt_dismissed: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<MeetingListEvent> for MeetingList {}

impl MeetingList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.list.titlePlaceholder",
                window,
                cx,
            )
        });
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder("meetings.search.short", window, cx)
        });
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(ev) = ev {
                    this.on_event(ev, cx);
                }
            }),
            cx.subscribe(&rename_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::PressEnter { .. } => this.save_rename(cx),
                // Clicking away cancels, like Esc.
                InputEvent::Blur => this.cancel_rename(cx),
                _ => {}
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
            rows: Vec::new(),
            list_state: ListState::new(0, ListAlignment::Top, dp(200.)),
            results_scroll: ScrollHandle::new(),
            selected: None,
            focus: cx.focus_handle(),
            loading: true,
            progress: HashMap::new(),
            renaming: None,
            rename_input,
            busy: HashSet::new(),
            search_input,
            query: String::new(),
            search_seq: 0,
            results: None,
            searching: false,
            titling: HashSet::new(),
            bulk: None,
            title_candidates: 0,
            turn_prompt_dismissed: false,
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
            // Bulk titling changes one meeting after another: it updates
            // their rows itself and reloads once it's done.
            MeetingsEvent::MeetingChanged(_) if self.bulk.is_some() => {}
            MeetingsEvent::MeetingChanged(_)
            | MeetingsEvent::RecorderState(_)
            | MeetingsEvent::Ready => self.reload(cx),
            _ => {}
        }
    }

    /// Rebuild the rows from `meetings`, keeping the scroll position.
    fn rebuild_rows(&mut self, cx: &App) {
        let rows = list_rows(&self.meetings, super::now(cx));
        let old = self.list_state.item_count();
        self.list_state.splice(0..old, rows.len());
        self.rows = rows;
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn selected(&self) -> Option<MeetingId> {
        self.selected
    }

    /// Mark `id` as the meeting shown, and scroll its row into view.
    pub fn set_selected(&mut self, id: Option<MeetingId>, cx: &mut Context<Self>) {
        if self.selected == id {
            return;
        }
        self.selected = id;
        self.reveal_selected();
        cx.notify();
    }

    fn reveal_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let row = self.rows.iter().position(|r| match r {
            ListRow::Meeting(i) => self.meetings.get(*i).is_some_and(|m| m.id == id),
            ListRow::Header(_) => false,
        });
        if let Some(ix) = row {
            self.list_state.scroll_to_reveal_item(ix);
        }
    }

    /// The meetings in the order the pane shows them (search hits while
    /// searching), for keyboard selection.
    pub fn order(&self) -> Vec<MeetingId> {
        if self.is_searching() {
            if let Some(r) = &self.results {
                return r
                    .hits
                    .iter()
                    .map(|h| h.meeting_id)
                    .filter(|id| self.meetings.iter().any(|m| m.id == *id))
                    .collect();
            }
            return Vec::new();
        }
        self.meetings.iter().map(|m| m.id).collect()
    }

    pub fn meeting(&self, id: MeetingId) -> Option<&Meeting> {
        self.meetings.iter().find(|m| m.id == id)
    }

    pub fn is_empty(&self) -> bool {
        !self.loading && self.meetings.is_empty()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// ⌘F: focus the search field.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Esc: clear the search. False when there was none.
    pub fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.search_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.set_query(String::new(), cx);
        window.focus(&self.focus, cx);
        true
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
                        this.rebuild_rows(cx);
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

    fn consent_turn_models(&mut self, cx: &mut Context<Self>) {
        super::settings::update_meetings(cx, |m| m.speaker_turn_models_consent = true);
        run(
            cx,
            |b| b.start_speaker_turn_downloads(),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.downloads.startError", &e, cx);
                }
                this.turn_prompt_dismissed = true;
                cx.notify();
            },
        );
    }

    /// Speaker-turn models (~107 MB) download only after the user agrees.
    fn render_turn_consent(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let settings = meeting_settings(cx);
        if self.turn_prompt_dismissed
            || settings.speaker_turn_prompt_dismissed
            || settings.speaker_turn_models_consent
        {
            return None;
        }
        if meetings_backend(cx).speaker_turn_models_installed() {
            return None;
        }
        let bytes = guidy_core::meetings::processing::speaker_turn_models_bytes();
        let size = format_model_size(Some(bytes as f64 / (1024.0 * 1024.0)));
        let tone = design::tone(cx);
        let theme = cx.theme().clone();
        Some(
            design::notice(theme.info)
                .id("speaker-turn-consent")
                .flex()
                .flex_col()
                .gap(dp(6.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(dp(2.))
                        .child(
                            div()
                                .text_size(dp(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(tone.text)
                                .child(t("meetings.turnConsent.title")),
                        )
                        .child(
                            div()
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted)
                                .child(t_args("meetings.turnConsent.body", &[("size", &size)])),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("speaker-turn-not-now")
                                .ghost()
                                .xsmall()
                                .label(t("meetings.turnConsent.notNow"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.turn_prompt_dismissed = true;
                                    super::settings::update_meetings(cx, |m| {
                                        m.speaker_turn_prompt_dismissed = true
                                    });
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("speaker-turn-download")
                                .primary()
                                .xsmall()
                                .icon(icon("download"))
                                .label(t("meetings.turnConsent.download"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.consent_turn_models(cx)),
                                ),
                        ),
                )
                .test_support()
                .into_any_element(),
        )
    }

    #[cfg(test)]
    pub fn meeting_count(&self) -> usize {
        self.meetings.len()
    }

    /// Rename a meeting in its row (the menu's Rename…).
    pub fn start_rename(&mut self, id: MeetingId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.meeting(id).map(|m| m.title.clone()) else {
            return;
        };
        self.renaming = Some(id);
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

    pub fn is_renaming(&self) -> bool {
        self.renaming.is_some()
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            cx.notify();
        }
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

    pub fn process(&mut self, id: MeetingId, cx: &mut Context<Self>) {
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

    pub fn cancel_processing(&mut self, id: MeetingId, cx: &mut Context<Self>) {
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

    /// Delete… : asks first. A meeting still recording can't be deleted.
    pub fn confirm_delete(&mut self, id: MeetingId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.meeting(id) else { return };
        if m.status == MeetingStatus::Recording {
            return;
        }
        let message = t_args("meetings.list.deleteConfirm", &[("title", &m.title)]);
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("meetings.list.deleteTitle"))
                .description(message.clone())
                .ok_text(t("meetings.list.delete"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| this.delete(id, cx)).ok();
                    true
                })
        });
    }

    pub fn delete(&mut self, id: MeetingId, cx: &mut Context<Self>) {
        let next = crate::logic::meetings::after_delete(&self.order(), id);
        self.meetings.retain(|m| m.id != id);
        self.rebuild_rows(cx);
        if self.selected == Some(id) {
            self.selected = None;
        }
        cx.emit(MeetingListEvent::Deleted { id, next });
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

    /// The row menu (⋯ and right click).
    fn row_menu(&self, m: &Meeting, menu: PopupMenu, weak: WeakEntity<Self>) -> PopupMenu {
        let id = m.id;
        let item =
            |label: SharedString, f: fn(&mut Self, MeetingId, &mut Window, &mut Context<Self>)| {
                let weak = weak.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    weak.update(cx, |this, cx| f(this, id, window, cx)).ok();
                })
            };
        let recording = m.status == MeetingStatus::Recording;
        let mut menu = menu
            .item(item(
                t("meetings.list.renameMenu"),
                |this, id, window, cx| this.start_rename(id, window, cx),
            ))
            .item(
                item(t("meetings.titles.generate"), |this, id, _, cx| {
                    this.generate_title(id, cx)
                })
                .disabled(!m.has_summary || self.titling.contains(&id) || self.bulk.is_some()),
            );
        if can_process(m) {
            let failed = m.status == MeetingStatus::Failed;
            let label = if failed || m.status == MeetingStatus::Processed {
                t("meetings.list.retry")
            } else {
                t("meetings.list.process")
            };
            menu = menu.separator().item(
                item(label, |this, id, _, cx| this.process(id, cx))
                    .disabled(self.busy.contains(&id)),
            );
        }
        if m.status == MeetingStatus::Processing {
            menu = menu.separator().item(item(
                t("meetings.list.cancelProcessing"),
                |this, id, _, cx| this.cancel_processing(id, cx),
            ));
        }
        menu.separator().item(
            item(t("meetings.list.deleteMenu"), |this, id, window, cx| {
                this.confirm_delete(id, window, cx)
            })
            .disabled(recording),
        )
    }

    /// The second line of a row: duration and status, or the processing
    /// stage with its progress.
    fn render_status(&self, m: &Meeting, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let tone = design::tone(cx);
        if m.status == MeetingStatus::Processing {
            let p = self.progress.get(&m.id);
            let fraction = p.map(|p| p.fraction).unwrap_or(0.0).clamp(0.0, 1.0);
            let stage = p
                .map(|p| t(&format!("meetings.stage.{}", p.stage.as_str())))
                .unwrap_or_else(|| t("meetings.status.processing"));
            return h_flex()
                .id(SharedString::from(format!("meeting-progress-{}", m.id)))
                .w_full()
                .gap(dp(6.))
                .child(
                    div()
                        .flex_none()
                        .max_w(relative(0.6))
                        .truncate()
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(tone.text_muted)
                        .child(design::shimmer_text(
                            SharedString::from(format!("meeting-stage-{}", m.id)),
                            progress_label(&stage, p),
                            cx,
                        )),
                )
                .child(div().flex_1().min_w(dp(24.)).child(design::track(
                    SharedString::from(format!("meeting-progress-track-{}", m.id)),
                    Some(fraction),
                    cx,
                )))
                .test_support()
                .into_any_element();
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(d) = m.duration_ms() {
            let mins = duration_minutes(d).to_string();
            parts.push(t_args("meetings.list.minutes", &[("count", &mins)]).to_string());
        }
        let failed = m.status == MeetingStatus::Failed;
        let live = m.status == MeetingStatus::Recording;
        let status = (m.status != MeetingStatus::Processed).then(|| t(status_key(m.status)));
        h_flex()
            .w_full()
            .min_w_0()
            .gap(dp(4.))
            .text_size(design::TEXT_SECONDARY)
            .text_color(tone.text_muted)
            .when(live, |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(dp(6.))
                        .rounded_full()
                        .bg(theme.danger),
                )
            })
            .when(!parts.is_empty(), |d| {
                d.child(div().flex_none().child(parts.join(" · ")))
            })
            .when_some(status, |d, status| {
                d.when(!parts.is_empty(), |d| d.child("·")).child(
                    div()
                        .flex_none()
                        .when(failed, |d| d.text_color(theme.danger))
                        .when(live, |d| d.text_color(tone.text))
                        .child(status),
                )
            })
            .when_some(
                m.description.clone().filter(|_| !failed && !live),
                |d, text| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("meeting-description-{}", m.id)))
                            .min_w_0()
                            .truncate()
                            .text_color(tone.text_faint)
                            .child(format!("— {text}"))
                            .test_support(),
                    )
                },
            )
            .when_some(m.error.clone().filter(|_| failed), |d, e| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(tone.text_faint)
                        .child(format!("— {e}")),
                )
            })
            .into_any_element()
    }

    fn render_list_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.rows.get(ix).copied() {
            Some(ListRow::Header(group)) => {
                let tone = design::tone(cx);
                div()
                    .id(SharedString::from(format!("meetings-group-{}", group.id())))
                    .w_full()
                    .px(dp(14.))
                    .pt(if ix == 0 { dp(6.) } else { dp(12.) })
                    .pb(dp(4.))
                    .text_size(design::TEXT_SECONDARY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tone.text_muted)
                    .child(t(group.key()))
                    .test_support()
                    .into_any_element()
            }
            Some(ListRow::Meeting(i)) => match self.meetings.get(i).cloned() {
                Some(m) => self.render_row(&m, window, cx),
                None => div().into_any_element(),
            },
            None => div().into_any_element(),
        }
    }

    fn render_row(&self, m: &Meeting, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let id = m.id;
        let tone = design::tone(cx);
        let selected = self.selected == Some(id);
        let focused = self.focus.contains_focused(window, cx);
        let renaming = self.renaming == Some(id);
        let title: AnyElement = if renaming {
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&self.rename_input).xsmall())
                .on_action(
                    cx.listener(|this, _: &gpui_kit::component::input::Escape, _, cx| {
                        this.cancel_rename(cx)
                    }),
                )
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(format!("meeting-title-{id}")))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(design::TEXT_BODY)
                .font_weight(FontWeight::MEDIUM)
                .child(m.title.clone())
                .test_support()
                .into_any_element()
        };
        let weak = cx.entity().downgrade();
        let menu_meeting = m.clone();
        let menu_weak = weak.clone();
        let more = Button::new(SharedString::from(format!("meeting-menu-{id}")))
            .ghost()
            .xsmall()
            .icon(icon("ellipsis"))
            .tooltip(t("meetings.list.more"))
            .dropdown_menu(move |menu, _, cx| {
                let m = menu_meeting.clone();
                match menu_weak.upgrade() {
                    Some(this) => this.read(cx).row_menu(&m, menu, menu_weak.clone()),
                    None => menu,
                }
            });
        let ctx_meeting = m.clone();
        let ctx_weak = weak.clone();
        let group_id = SharedString::from(format!("meeting-row-{id}"));
        let bg = if selected {
            Some(if focused {
                tone.selection_focused
            } else {
                tone.selection
            })
        } else {
            None
        };
        div()
            .id(SharedString::from(format!("meeting-{id}")))
            .w_full()
            .px(dp(6.))
            .child(
                v_flex()
                    .id(SharedString::from(format!("meeting-open-{id}")))
                    .group(group_id.clone())
                    .w_full()
                    // ~44 pt: a 13 pt title line over an 11 pt status line.
                    .px(dp(8.))
                    .py(dp(5.))
                    .gap(dp(1.))
                    .rounded(design::RADIUS_CHIP)
                    .when_some(bg, |d, bg| d.bg(bg))
                    .when(!selected, |d| {
                        d.hover(move |s| s.bg(tone.raised.opacity(0.5)))
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .h(dp(18.))
                            .line_height(dp(18.))
                            .gap(dp(6.))
                            .child(title)
                            .when(!renaming, |d| {
                                d.child(
                                    div()
                                        .relative()
                                        .flex_none()
                                        .child(
                                            design::mono(super::clock_time(m.started_at))
                                                .text_size(dp(11.))
                                                .text_color(tone.text_muted)
                                                .group_hover(group_id.clone(), |s| s.opacity(0.)),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top(dp(-3.))
                                                .right(dp(-4.))
                                                .opacity(0.)
                                                .group_hover(group_id.clone(), |s| s.opacity(1.))
                                                .child(more),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(dp(15.))
                            .line_height(dp(15.))
                            .child(self.render_status(m, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.renaming != Some(id) {
                            cx.emit(MeetingListEvent::Open(id))
                        }
                    }))
                    .test_support()
                    .context_menu(move |menu, _, cx| {
                        let m = ctx_meeting.clone();
                        match ctx_weak.upgrade() {
                            Some(this) => this.read(cx).row_menu(&m, menu, ctx_weak.clone()),
                            None => menu,
                        }
                    }),
            )
            .test_support()
            .into_any_element()
    }

    /// The footer: how many meetings, and "Generate Titles" (or its
    /// progress and Cancel).
    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let n = self.meetings.len().to_string();
        let action: Option<AnyElement> = if let Some(bulk) = &self.bulk {
            let (done, total) = (bulk.done.to_string(), bulk.total.to_string());
            Some(
                h_flex()
                    .id("meetings-titles-progress")
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .child(design::shimmer_text(
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
                            )),
                    )
                    .child(
                        Button::new("meetings-titles-cancel")
                            .ghost()
                            .xsmall()
                            .label(t("meetings.list.cancel"))
                            .disabled(bulk.cancelled)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_titles(cx))),
                    )
                    .test_support()
                    .into_any_element(),
            )
        } else {
            (self.title_candidates > 0).then(|| {
                let n = self.title_candidates.to_string();
                Button::new("meetings-titles-generate")
                    .ghost()
                    .xsmall()
                    .icon(icon("sparkles"))
                    .label(t_args("meetings.titles.generateAll", &[("count", &n)]))
                    .tooltip(t("meetings.titles.generateAllHint"))
                    .on_click(cx.listener(|this, _, _, cx| this.generate_all_titles(cx)))
                    .into_any_element()
            })
        };
        h_flex()
            .id("meetings-list-footer")
            .flex_none()
            .w_full()
            .h(dp(28.))
            .px(dp(10.))
            .gap_2()
            .justify_between()
            .border_t_1()
            .border_color(tone.hairline)
            .child(
                div()
                    .flex_none()
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(t_args("meetings.list.count", &[("count", &n)])),
            )
            .children(action)
            .test_support()
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
                    .child(design::section_header(
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
            crate::logic::meetings::day_label(m.started_at, super::now(cx), &lang),
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
                .py(dp(10.))
                .when(self.selected == Some(id), |d| d.bg(tone.selection))
                .when(self.selected != Some(id), |d| {
                    d.hover(|s| s.bg(tone.raised))
                })
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
                .child(design::section_header(meta, cx))
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
pub(crate) fn highlighted(text: &str, ranges: &[Range<usize>], cx: &App) -> StyledText {
    highlighted_with(text, ranges, None, cx)
}

/// [`highlighted`], with the range at `current` (find's current match)
/// drawn stronger.
pub(crate) fn highlighted_with(
    text: &str,
    ranges: &[Range<usize>],
    current: Option<usize>,
    cx: &App,
) -> StyledText {
    let tone = design::tone(cx);
    // Like the macOS find highlight: text stays readable on the tint, and
    // the current match is dark text on the accent yellow in both themes.
    let style = HighlightStyle {
        color: Some(tone.text),
        background_color: Some(tone.accent_soft),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let strong = HighlightStyle {
        color: Some(black()),
        background_color: Some(tone.accent),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let ranges: Vec<(Range<usize>, HighlightStyle)> = ranges
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.start < r.end
                && r.end <= text.len()
                && text.is_char_boundary(r.start)
                && text.is_char_boundary(r.end)
        })
        .map(|(i, r)| (r.clone(), if Some(i) == current { strong } else { style }))
        .collect();
    StyledText::new(text.to_string()).with_highlights(ranges)
}

impl Render for MeetingList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        };
        let content: AnyElement = if self.is_searching() {
            div()
                .id("meetings-search-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.results_scroll)
                .child(self.render_results(cx))
                .into_any_element()
        } else if self.loading {
            div()
                .flex_1()
                .min_h_0()
                .child(note("meetings-loading", t("meetings.list.loading")))
                .into_any_element()
        } else if self.meetings.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .child(note("meetings-empty", t("meetings.list.emptyShort")))
                .into_any_element()
        } else {
            let view = cx.entity().downgrade();
            list(self.list_state.clone(), move |ix, window, cx| {
                view.update(cx, |this, cx| this.render_list_row(ix, window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .flex_1()
            .min_h_0()
            .pb(dp(6.))
            .into_any_element()
        };
        v_flex()
            .id("meetings-list")
            .key_context("MeetingList")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .child(
                div().flex_none().px(dp(10.)).pt(dp(8.)).pb(dp(6.)).child(
                    Input::new(&self.search_input)
                        .small()
                        .prefix(icon("search").small().text_color(tone.text_muted))
                        .cleanable(true),
                ),
            )
            .children(
                self.render_turn_consent(cx)
                    .map(|c| div().flex_none().px(dp(10.)).pb(dp(6.)).child(c)),
            )
            .child(content)
            .child(self.render_footer(cx))
            .test_support()
    }
}
