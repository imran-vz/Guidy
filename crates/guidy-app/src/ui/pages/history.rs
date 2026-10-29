//! History page: a master–detail view of transcriptions.
//!
//! The title bar holds Search (⌘F), the Saved filter and Show in Finder.
//! The list (virtualized, newest first, loads pages as it scrolls) sits on
//! the left; the selected transcription's date, actions, selectable text and
//! its one audio player fill the right. In a narrow window the detail
//! expands inline under the selected row instead.
//!
//! Keyboard (list focused): ↑/↓ select, Space plays/pauses, ←/→ skip 5 s,
//! ⌘C copies, ⌫ deletes (undo with ⌘Z or the Undo button for a few
//! seconds), ⌘F searches.

use crate::logic::flows::format_history_date;
use crate::logic::history::{list_splice, matches_filter, preview_line, HistoryList, PAGE_SIZE};
use crate::logic::keyboard::format_key_combination;
use crate::playback::{format_time, wav_duration, Playback};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, t_args, LocalizedPlaceholder};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::history::HistoryEntry;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Below this content width the list and detail stop sitting side by side.
const SPLIT_MIN_W: Pixels = dp(600.);
const LIST_W: Pixels = dp(280.);
/// How long a deleted transcription can be brought back.
const UNDO_FOR: Duration = Duration::from_secs(10);
/// Arrow-key skip in the player.
const SKIP: Duration = Duration::from_secs(5);

/// A deleted entry waiting out its undo period.
struct PendingDelete {
    entry: HistoryEntry,
    _timer: Task<()>,
}

pub struct HistoryPage {
    state: Entity<AppState>,
    list: HistoryList,
    /// Ids shown (search and Saved filter applied), in list order.
    visible: Vec<i64>,
    list_state: ListState,
    selected: Option<i64>,
    search: Entity<InputState>,
    saved_only: bool,
    retrying: HashSet<i64>,
    copied: Option<i64>,
    /// An action on an entry failed (copy, save, re-transcribe, delete).
    action_error: Option<(i64, SharedString)>,
    transcript: Entity<TextareaState>,
    /// (entry, text) the transcript field holds.
    shown: Option<(i64, String)>,
    /// Text to put in the transcript field on the next render (it needs
    /// the window).
    pending_text: Option<String>,
    weak_self: WeakEntity<Self>,
    player: Entity<HistoryPlayer>,
    pending_delete: Option<PendingDelete>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl HistoryPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "settings.history.searchPlaceholder",
                window,
                cx,
            )
        });
        let transcript = cx.new(|cx| TextareaState::new(window, cx));
        let player = cx.new(|cx| HistoryPlayer::new(AppState::global(cx)));
        let list_state = ListState::new(0, ListAlignment::Top, dp(200.));
        let weak = cx.entity().downgrade();
        list_state.set_scroll_handler(move |ev, _, cx| {
            // Near the end: fetch the next page.
            if ev.visible_range.end + 8 >= ev.count {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |this, cx| this.load_more(cx)).ok();
                });
            }
        });
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::History(p) = ev {
                    this.list.apply_event(p);
                    this.refresh_visible(cx);
                }
            }),
            cx.subscribe_in(
                &search,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => {
                        this.refresh_visible(cx);
                        this.load_rest_for_search(cx);
                    }
                    // Enter goes to the results.
                    InputEvent::PressEnter { .. } => window.focus(&this.focus, cx),
                    _ => {}
                },
            ),
            cx.on_release(|this: &mut Self, cx: &mut App| this.commit_pending_delete(cx)),
        ];
        let mut this = Self {
            state,
            list: HistoryList {
                loading: true,
                ..Default::default()
            },
            visible: Vec::new(),
            list_state,
            selected: None,
            search,
            saved_only: false,
            retrying: HashSet::new(),
            copied: None,
            action_error: None,
            transcript,
            shown: None,
            pending_text: None,
            weak_self: cx.entity().downgrade(),
            player,
            pending_delete: None,
            focus: cx.focus_handle().tab_stop(true),
            _subs: subs,
        };
        this.load_page(None, cx);
        this
    }

    // -- Loading -------------------------------------------------------------

    fn load_page(&mut self, cursor: Option<i64>, cx: &mut Context<Self>) {
        let first = cursor.is_none();
        if first {
            self.list.loading = true;
            self.list.error = None;
        } else {
            if self.list.loading_more || self.list.loading {
                return;
            }
            self.list.loading_more = true;
        }
        cx.notify();
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend.get_history_entries(cursor, PAGE_SIZE) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(page) => this.list.apply_page(first, page.entries, page.has_more),
                    Err(e) => {
                        log::error!("Failed to load history entries: {e}");
                        this.list.fail(e);
                    }
                }
                this.refresh_visible(cx);
                this.load_rest_for_search(cx);
            })
            .ok();
        })
        .detach();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.list.has_more && self.list.error.is_none() {
            let cursor = self.list.next_cursor();
            self.load_page(cursor, cx);
        }
    }

    /// Search and the Saved filter look through everything: load the rest.
    fn load_rest_for_search(&mut self, cx: &mut Context<Self>) {
        let filtering = self.saved_only || !self.search.read(cx).value().trim().is_empty();
        if filtering {
            self.load_more(cx);
        }
    }

    /// Recompute the shown ids and splice the list so scrolling stays put.
    fn refresh_visible(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        let next: Vec<i64> = self
            .list
            .entries
            .iter()
            .filter(|e| matches_filter(e, &query, self.saved_only))
            .map(|e| e.id)
            .collect();
        let (range, count) = list_splice(&self.visible, &next);
        if !range.is_empty() || count > 0 {
            self.list_state.splice(range, count);
        }
        self.visible = next;
        // Text can change in place (re-transcribed): measure the rows again.
        if !self.visible.is_empty() {
            self.list_state.remeasure_items(0..self.visible.len());
        }
        if self.selected.is_none_or(|id| !self.visible.contains(&id)) {
            self.selected = self.visible.first().copied();
        }
        self.sync_detail(cx);
        cx.notify();
    }

    fn entry(&self, id: i64) -> Option<&HistoryEntry> {
        self.list.entries.iter().find(|e| e.id == id)
    }

    fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.selected.and_then(|id| self.entry(id))
    }

    /// Point the transcript field and player at the selection.
    fn sync_detail(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.shown = None;
            self.player.update(cx, |p, cx| p.set_entry(None, cx));
            return;
        };
        let text = entry.transcription_text.clone();
        if self.shown.as_ref() != Some(&(entry.id, text.clone())) {
            self.shown = Some((entry.id, text.clone()));
            self.pending_text = Some(text);
        }
        self.player.update(cx, |p, cx| {
            p.set_entry(Some((entry.id, entry.file_name.clone())), cx)
        });
    }

    fn select(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.selected == Some(id) {
            return;
        }
        let old = self
            .selected
            .and_then(|o| self.visible.iter().position(|v| *v == o));
        self.selected = Some(id);
        self.action_error = None;
        if let Some(ix) = self.visible.iter().position(|v| *v == id) {
            self.list_state.scroll_to_reveal_item(ix);
            // The inline (narrow) layout grows the selected row.
            self.list_state.remeasure_items(ix..ix + 1);
        }
        if let Some(ix) = old {
            self.list_state.remeasure_items(ix..ix + 1);
        }
        self.sync_detail(cx);
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.visible.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|id| self.visible.iter().position(|v| *v == id));
        let next = match current {
            Some(ix) => (ix as isize + delta).clamp(0, self.visible.len() as isize - 1) as usize,
            None => 0,
        };
        let id = self.visible[next];
        self.select(id, cx);
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.list.entries.len()
    }

    #[cfg(test)]
    pub fn selected_id(&self) -> Option<i64> {
        self.selected
    }

    #[cfg(test)]
    pub fn select_entry(&mut self, id: i64, cx: &mut Context<Self>) {
        self.select(id, cx);
    }

    #[cfg(test)]
    pub fn search_for(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.search
            .update(cx, |s, cx| s.set_value(text, window, cx));
        self.refresh_visible(cx);
    }

    fn backend(&self, cx: &App) -> crate::backend::SharedBackend {
        self.state.read(cx).backend.clone()
    }

    // -- Actions -------------------------------------------------------------

    fn copy(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(text) = self
            .entry(id)
            .map(|e| e.transcription_text.clone())
            .filter(|t| !t.trim().is_empty())
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        if cx.read_from_clipboard().and_then(|c| c.text()).as_deref() != Some(text.as_str()) {
            self.action_error = Some((id, t("settings.history.copyError")));
            cx.notify();
            return;
        }
        self.copied = Some(id);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            this.update(cx, |this, cx| {
                if this.copied == Some(id) {
                    this.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_saved(&mut self, id: i64, cx: &mut Context<Self>) {
        self.list.toggle_saved(id);
        self.action_error = None;
        self.refresh_visible(cx);
        let backend = self.backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.toggle_history_entry_saved(id) })
                .await;
            if let Err(e) = r {
                this.update(cx, |this, cx| {
                    this.list.toggle_saved(id);
                    this.action_error =
                        Some((id, t_args("settings.history.saveError", &[("error", &e)])));
                    this.refresh_visible(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Remove the entry now and delete it for real once the undo period is
    /// over (or another delete starts, or the page goes away).
    fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        self.commit_pending_delete(cx);
        // The next row (or the previous one at the end) takes the selection.
        if self.selected == Some(id) {
            let ix = self.visible.iter().position(|v| *v == id).unwrap_or(0);
            self.selected = self
                .visible
                .get(ix + 1)
                .or_else(|| ix.checked_sub(1).and_then(|p| self.visible.get(p)))
                .copied();
        }
        self.player.update(cx, |p, cx| p.forget(id, cx));
        self.list.remove(id);
        self.action_error = None;
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            this.update(cx, |this, cx| {
                if this.pending_delete.as_ref().map(|p| p.entry.id) == Some(id) {
                    this.commit_pending_delete(cx);
                    cx.notify();
                }
            })
            .ok();
        });
        self.pending_delete = Some(PendingDelete {
            entry,
            _timer: timer,
        });
        self.refresh_visible(cx);
    }

    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        let id = pending.entry.id;
        self.list.restore(pending.entry);
        self.refresh_visible(cx);
        self.select(id, cx);
    }

    /// Delete the entry waiting for undo now.
    fn commit_pending_delete(&mut self, cx: &mut App) {
        let Some(pending) = self.pending_delete.take() else {
            return;
        };
        let entry = pending.entry;
        let backend = self.state.read(cx).backend.clone();
        let weak = self.weak_self.clone();
        cx.spawn(async move |cx: &mut AsyncApp| {
            let id = entry.id;
            let r = cx
                .background_spawn(async move { backend.delete_history_entry(id) })
                .await;
            if let Err(e) = r {
                log::error!("Failed to delete history entry {id}: {e}");
                weak.update(cx, |this, cx| {
                    this.list.restore(entry);
                    this.action_error = Some((
                        id,
                        t_args("settings.history.deleteFailed", &[("error", &e)]),
                    ));
                    this.refresh_visible(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn retranscribe(&mut self, id: i64, cx: &mut Context<Self>) {
        self.retrying.insert(id);
        self.action_error = None;
        cx.notify();
        let backend = self.backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.retry_history_entry_transcription(id) })
                .await;
            this.update(cx, |this, cx| {
                this.retrying.remove(&id);
                if let Err(e) = r {
                    this.action_error = Some((
                        id,
                        t_args("settings.history.retranscribeFailed", &[("error", &e)]),
                    ));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_recordings_folder(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend(cx);
        cx.background_spawn(async move {
            if let Err(e) = backend.open_recordings_folder() {
                log::error!("Failed to open recordings folder: {e}");
            }
        })
        .detach();
    }

    fn toggle_saved_filter(&mut self, cx: &mut Context<Self>) {
        self.saved_only = !self.saved_only;
        self.refresh_visible(cx);
        self.load_rest_for_search(cx);
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let cmd = m.platform && !m.control && !m.alt;
        let plain = !m.platform && !m.control && !m.alt && !m.shift;
        let selected = self.selected;
        let handled = match ks.key.as_str() {
            "up" if plain => {
                self.move_selection(-1, cx);
                true
            }
            "down" if plain => {
                self.move_selection(1, cx);
                true
            }
            "home" if plain => {
                self.move_selection(isize::MIN / 2, cx);
                true
            }
            "end" if plain => {
                self.move_selection(isize::MAX / 2, cx);
                true
            }
            "space" if plain => {
                self.player.update(cx, |p, cx| p.toggle(cx));
                true
            }
            "left" if plain => {
                self.player.update(cx, |p, cx| p.skip(false, cx));
                true
            }
            "right" if plain => {
                self.player.update(cx, |p, cx| p.skip(true, cx));
                true
            }
            "backspace" | "delete" if plain || cmd => {
                if let Some(id) = selected {
                    self.delete(id, cx);
                }
                true
            }
            "c" if cmd => {
                if let Some(id) = selected {
                    self.copy(id, cx);
                }
                true
            }
            "z" if cmd => {
                self.undo_delete(cx);
                true
            }
            "f" if cmd => {
                let h = self.search.focus_handle(cx);
                window.focus(&h, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }
}

impl crate::ui::pages::Page for HistoryPage {
    fn toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.clone();
        Some(
            h_flex()
                .gap(dp(6.))
                .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
                .child(
                    div()
                        .id("history-search")
                        .test_support()
                        .w(dp(180.))
                        // Esc in search clears it and goes back to the list.
                        .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                            if ev.keystroke.key == "escape" {
                                search.update(cx, |s, cx| s.set_value("", window, cx));
                                window.focus(&this.focus, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            Input::new(&self.search)
                                .small()
                                .cleanable(true)
                                .prefix(
                                    icon("search")
                                        .xsmall()
                                        .text_color(design::tone(cx).text_muted),
                                )
                                .aria_label(t("settings.history.search")),
                        ),
                )
                .child(
                    Button::new("history-saved-filter")
                        .ghost()
                        .small()
                        .icon(icon(if self.saved_only { "star-fill" } else { "star" }))
                        .toggled(self.saved_only)
                        .tooltip(t("settings.history.savedOnly"))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_saved_filter(cx))),
                )
                .child(
                    Button::new("open-recordings")
                        .ghost()
                        .small()
                        .icon(icon("folder-open"))
                        .tooltip(t("settings.history.showInFinder"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_recordings_folder(cx))),
                )
                .into_any_element(),
        )
    }
}

impl Render for HistoryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(text) = self.pending_text.take() {
            self.transcript
                .update(cx, |st, cx| st.set_value(text, window, cx));
        }
        let content_w = window.viewport_size().width - design::SIDEBAR_W;
        let split = content_w >= SPLIT_MIN_W;
        let tone = design::tone(cx);
        let rtl = crate::ui::kit::rtl();
        let body: AnyElement = if self.list.loading && self.list.entries.is_empty() {
            self.render_skeleton(cx).into_any_element()
        } else if let (Some(e), true) = (self.list.error.clone(), self.list.entries.is_empty()) {
            self.render_load_error(e, cx).into_any_element()
        } else if self.list.entries.is_empty() && self.pending_delete.is_none() {
            self.render_empty(cx).into_any_element()
        } else if split {
            h_flex()
                .size_full()
                .items_stretch()
                .when(rtl, |d| d.flex_row_reverse())
                .child(
                    div()
                        .w(LIST_W)
                        .flex_none()
                        .h_full()
                        .when(!rtl, |d| d.border_r_1())
                        .when(rtl, |d| d.border_l_1())
                        .border_color(tone.hairline)
                        .child(self.render_list(false, window, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_detail(window, cx)),
                )
                .into_any_element()
        } else {
            self.render_list(true, window, cx).into_any_element()
        };
        crate::ui::kit::fill_page("page-history")
            .child(body)
            .test_support()
    }
}

impl HistoryPage {
    /// The list pane: rows, the "load more" error, the undo bar.
    fn render_list(&mut self, inline: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tone = design::tone(cx);
        let focused = self.focus.contains_focused(window, cx) || self.focus.is_focused(window);
        let ring = cx.theme().ring;
        let no_matches = self.visible.is_empty() && !self.list.entries.is_empty();
        let list_el = list(
            self.list_state.clone(),
            cx.processor(move |this, ix: usize, window, cx| {
                this.render_row(ix, inline, focused, window, cx)
            }),
        )
        .size_full();
        v_flex()
            .size_full()
            .child(
                div()
                    .id("history-list")
                    .key_context("HistoryList")
                    .track_focus(&self.focus)
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .border_1()
                    .border_color(transparent_black())
                    .focus_visible(move |s| s.border_color(ring))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_action(
                        cx.listener(|this, _: &gpui_kit::component::input::Copy, _, cx| {
                            if let Some(id) = this.selected {
                                this.copy(id, cx);
                            }
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &gpui_kit::component::input::Undo, _, cx| {
                            this.undo_delete(cx)
                        }),
                    )
                    .py(dp(4.))
                    .child(list_el)
                    .when(no_matches, |d| {
                        d.child(
                            div()
                                .id("history-no-matches")
                                .absolute()
                                .top(dp(16.))
                                .w_full()
                                .text_center()
                                .text_size(design::TEXT_BODY)
                                .text_color(tone.text_muted)
                                .child(t("settings.history.noMatches")),
                        )
                    }),
            )
            .when_some(
                self.list
                    .error
                    .clone()
                    .filter(|_| !self.list.entries.is_empty()),
                |d, e| {
                    d.child(
                        h_flex()
                            .flex_none()
                            .gap(dp(6.))
                            .px(dp(10.))
                            .py(dp(6.))
                            .border_t_1()
                            .border_color(tone.hairline)
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(cx.theme().danger)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(t_args("settings.history.loadError", &[("error", &e)])),
                            )
                            .child(
                                Button::new("history-retry-more")
                                    .ghost()
                                    .xsmall()
                                    .label(t("common.tryAgain"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.list.error = None;
                                        this.load_more(cx)
                                    })),
                            ),
                    )
                },
            )
            .when(self.pending_delete.is_some(), |d| {
                d.child(
                    h_flex()
                        .id("history-undo-bar")
                        .flex_none()
                        .gap(dp(6.))
                        .px(dp(10.))
                        .py(dp(4.))
                        .border_t_1()
                        .border_color(tone.hairline)
                        .bg(tone.surface)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(tone.text_muted)
                                .child(t("settings.history.deleted")),
                        )
                        .child(
                            Button::new("history-undo")
                                .ghost()
                                .xsmall()
                                .label(t("settings.history.undo"))
                                .on_click(cx.listener(|this, _, _, cx| this.undo_delete(cx))),
                        )
                        .test_support(),
                )
            })
    }

    fn render_row(
        &mut self,
        ix: usize,
        inline: bool,
        focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(entry) = self.visible.get(ix).and_then(|id| self.entry(*id)).cloned() else {
            return div().into_any_element();
        };
        let tone = design::tone(cx);
        let id = entry.id;
        let selected = self.selected == Some(id);
        let lang = guidy_i18n::language();
        let retrying = self.retrying.contains(&id);
        let preview: SharedString = if retrying {
            t("settings.history.transcribing")
        } else {
            match preview_line(&entry.transcription_text) {
                "" => t("settings.history.noText"),
                line => line.to_string().into(),
            }
        };
        let empty = entry.transcription_text.trim().is_empty() && !retrying;
        let bg = if selected {
            if focused {
                tone.selection_focused
            } else {
                tone.selection
            }
        } else {
            transparent_black()
        };
        let row = v_flex()
            .id(SharedString::from(format!("history-entry-{id}")))
            .w_full()
            .px(dp(8.))
            .py(dp(6.))
            .gap(dp(2.))
            .rounded(design::RADIUS_CHIP)
            .bg(bg)
            .child(
                h_flex()
                    .gap(dp(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(design::TEXT_BODY)
                            .line_height(dp(17.))
                            .when(empty || retrying, |d| d.text_color(tone.text_muted))
                            .child(preview),
                    )
                    .when(entry.saved, |d| {
                        d.child(
                            icon("star-fill")
                                .xsmall()
                                .flex_none()
                                .text_color(tone.accent_text),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(design::TEXT_SECONDARY)
                    .line_height(dp(14.))
                    .text_color(tone.text_muted)
                    .child(format_history_date(entry.timestamp, &lang)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select(id, cx);
                window.focus(&this.focus, cx);
            }))
            .test_support();
        // Narrow window (the list is the page): the row text keeps the
        // standard page inset from the window edge.
        let inset = if inline {
            design::PAGE_X - dp(8.)
        } else {
            dp(6.)
        };
        if inline && selected {
            // Narrow window: the selected row opens into its detail.
            div()
                .w_full()
                .px(inset)
                .py(dp(2.))
                .child(
                    div()
                        .id(SharedString::from(format!("history-entry-{id}")))
                        .w_full()
                        .rounded(design::RADIUS_PANEL)
                        .border_1()
                        .border_color(if focused { tone.accent } else { tone.hairline })
                        .bg(tone.surface)
                        .h(dp(200.))
                        .overflow_hidden()
                        .child(self.render_detail(window, cx))
                        .test_support(),
                )
                .into_any_element()
        } else {
            div().w_full().px(inset).child(row).into_any_element()
        }
    }

    /// Header (date, actions), selectable text, player.
    fn render_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tone = design::tone(cx);
        let Some(entry) = self.selected_entry().cloned() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(t("settings.history.noSelection"))
                .into_any_element();
        };
        let id = entry.id;
        let lang = guidy_i18n::language();
        let retrying = self.retrying.contains(&id);
        let has_text = !entry.transcription_text.trim().is_empty();
        let icon_btn = |name: &'static str, label: SharedString, prefix: &str| {
            Button::new(SharedString::from(format!("{prefix}-{id}")))
                .ghost()
                .small()
                .icon(icon(name))
                .tooltip(label)
        };
        let error = self
            .action_error
            .as_ref()
            .filter(|(e, _)| *e == id)
            .map(|(_, m)| m.clone());
        let header = h_flex()
            .flex_none()
            .gap(dp(8.))
            .px(dp(16.))
            .pt(dp(10.))
            .pb(dp(6.))
            .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(design::TEXT_TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format_history_date(entry.timestamp, &lang)),
            )
            .child(
                h_flex()
                    .gap(dp(2.))
                    .child(
                        icon_btn(
                            if self.copied == Some(id) {
                                "check"
                            } else {
                                "copy"
                            },
                            t("settings.history.copyToClipboard"),
                            "copy",
                        )
                        .disabled(!has_text || retrying)
                        .on_click(cx.listener(move |this, _, _, cx| this.copy(id, cx))),
                    )
                    .child(
                        icon_btn(
                            if entry.saved { "star-fill" } else { "star" },
                            if entry.saved {
                                t("settings.history.unsave")
                            } else {
                                t("settings.history.save")
                            },
                            "star",
                        )
                        .disabled(retrying)
                        .when(entry.saved, |b| b.text_color(tone.accent_text))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_saved(id, cx))),
                    )
                    .child(
                        icon_btn("rotate-ccw", t("settings.history.retranscribe"), "retry")
                            .loading(retrying)
                            .disabled(retrying)
                            .on_click(cx.listener(move |this, _, _, cx| this.retranscribe(id, cx))),
                    )
                    .child(
                        icon_btn("trash", t("settings.history.delete"), "delete")
                            .disabled(retrying)
                            .on_click(cx.listener(move |this, _, _, cx| this.delete(id, cx))),
                    ),
            );
        let text: AnyElement = if retrying {
            div()
                .px(dp(16.))
                .py(dp(6.))
                .child(
                    h_flex().child(design::chip("", true, cx).child(design::shimmer_text(
                        ("transcribing", id as usize),
                        t("settings.history.transcribing"),
                        cx,
                    ))),
                )
                .into_any_element()
        } else if has_text {
            div()
                .id("history-transcript")
                .size_full()
                .px(dp(6.))
                .child(
                    Textarea::new(&self.transcript)
                        .readonly(true)
                        .appearance(false)
                        .h(relative(1.))
                        .aria_label(t("settings.history.transcript")),
                )
                .into_any_element()
        } else {
            div()
                .px(dp(16.))
                .py(dp(6.))
                .text_size(design::TEXT_BODY)
                .text_color(tone.text_muted)
                .child(t("settings.history.transcriptionFailed"))
                .into_any_element()
        };
        v_flex()
            .id("history-detail")
            .size_full()
            .child(header)
            .when_some(error, |d, e| {
                d.child(
                    div()
                        .id("history-action-error")
                        .flex_none()
                        .px(dp(16.))
                        .pb(dp(6.))
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(cx.theme().danger)
                        .child(e),
                )
            })
            .child(div().flex_1().min_h_0().child(text))
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(tone.hairline)
                    .child(self.player.clone()),
            )
            .test_support()
            .into_any_element()
    }

    /// Loading: the list frame with placeholder rows.
    fn render_skeleton(&self, cx: &App) -> impl IntoElement {
        let tone = design::tone(cx);
        let bar = |w: f32, h: f32| {
            div()
                .h(dp(h))
                .w(relative(w))
                .rounded(design::RADIUS_CHIP)
                .bg(tone.raised)
        };
        v_flex()
            .id("history-loading")
            .w(LIST_W)
            .h_full()
            .py(dp(4.))
            .border_r_1()
            .border_color(tone.hairline)
            .children(
                [0.8f32, 0.6, 0.72, 0.5, 0.66, 0.58]
                    .into_iter()
                    .map(move |w| {
                        v_flex()
                            .mx(dp(6.))
                            .px(dp(8.))
                            .py(dp(8.))
                            .gap(dp(6.))
                            .child(bar(w, 10.))
                            .child(bar(0.3, 8.))
                    }),
            )
            .test_support()
    }

    fn render_load_error(&self, error: String, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        v_flex()
            .id("history-error")
            .size_full()
            .items_center()
            .justify_center()
            .gap(dp(8.))
            .px(dp(24.))
            .child(
                div()
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text_muted)
                    .text_center()
                    .child(t_args("settings.history.loadError", &[("error", &error)])),
            )
            .child(
                Button::new("history-retry-load")
                    .outline()
                    .small()
                    .label(t("common.tryAgain"))
                    .on_click(cx.listener(|this, _, _, cx| this.load_page(None, cx))),
            )
            .test_support()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let shortcut = self
            .state
            .read(cx)
            .app_settings()
            .and_then(|s| s.bindings.get("transcribe"))
            .map(|b| format_key_combination(&b.current_binding))
            .unwrap_or_default();
        let filtering = self.saved_only || !self.search.read(cx).value().trim().is_empty();
        v_flex()
            .id("history-empty")
            .size_full()
            .items_center()
            .justify_center()
            .gap(dp(10.))
            .px(dp(24.))
            .child(
                div()
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text_muted)
                    .text_center()
                    .child(if filtering {
                        t("settings.history.noMatches")
                    } else {
                        t_args("settings.history.emptyHint", &[("shortcut", &shortcut)])
                    }),
            )
            .when(!filtering, |d| {
                d.child(
                    Button::new("history-empty-shortcut")
                        .outline()
                        .small()
                        .label(t("settings.history.shortcutSettings"))
                        .on_click(|_, _, cx| {
                            if let Some(main) = crate::windows::main_window_entity(cx) {
                                main.update(cx, |m, cx| {
                                    m.show_section(crate::ui::main_window::Section::General, cx)
                                });
                            }
                        }),
                )
            })
            .test_support()
    }
}

// -- Player -------------------------------------------------------------------

/// The one audio player of the History page, for the selected entry. It
/// redraws itself (not the list) while playing.
pub struct HistoryPlayer {
    state: Entity<AppState>,
    /// (entry id, file name)
    entry: Option<(i64, String)>,
    path: Option<PathBuf>,
    duration: Option<Duration>,
    error: Option<SharedString>,
    ticking: bool,
    scrub_focus: Option<FocusHandle>,
    bar: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl HistoryPlayer {
    fn new(state: Entity<AppState>) -> Self {
        Self {
            state,
            entry: None,
            path: None,
            duration: None,
            error: None,
            ticking: false,
            scrub_focus: None,
            bar: Rc::new(Cell::new(None)),
        }
    }

    fn set_entry(&mut self, entry: Option<(i64, String)>, cx: &mut Context<Self>) {
        if self.entry == entry {
            return;
        }
        // Switching entries stops what this player was playing.
        if self.is_active() {
            Playback::global().stop();
        }
        self.entry = entry.clone();
        self.path = None;
        self.duration = None;
        self.error = None;
        cx.notify();
        let Some((id, file)) = entry else {
            return;
        };
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let path = PathBuf::from(backend.get_audio_file_path(&file)?);
                    if !path.exists() {
                        return Err(String::new());
                    }
                    let duration = wav_duration(&path);
                    Ok((path, duration))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.entry.as_ref().map(|e| e.0) != Some(id) {
                    return;
                }
                match found {
                    Ok((path, duration)) => {
                        this.path = Some(path);
                        this.duration = duration;
                    }
                    Err(e) => {
                        if !e.is_empty() {
                            log::error!("Failed to get audio file path: {e}");
                        }
                        this.error = Some(t("settings.history.audioMissing"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The entry was deleted: stop and drop it.
    fn forget(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.entry.as_ref().map(|e| e.0) == Some(id) {
            if self.is_active() {
                Playback::global().stop();
            }
            self.entry = None;
            self.path = None;
            cx.notify();
        }
    }

    fn is_active(&self) -> bool {
        self.path.is_some() && Playback::global().state().path == self.path
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let player = Playback::global();
        if self.is_active() {
            if player.state().playing {
                player.pause();
            } else {
                player.resume();
            }
        } else {
            player.play(path);
        }
        self.ensure_ticking(cx);
    }

    /// Seek to `fraction` of the recording, starting it if needed.
    fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let (Some(path), Some(duration)) = (self.path.clone(), self.duration) else {
            return;
        };
        let player = Playback::global();
        if !self.is_active() {
            player.play(path);
        }
        player.seek(duration.mul_f32(fraction.clamp(0.0, 1.0)));
        self.ensure_ticking(cx);
    }

    fn skip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(duration) = self.duration.filter(|d| !d.is_zero()) else {
            return;
        };
        let at = if self.is_active() {
            Playback::global().state().position
        } else {
            Duration::ZERO
        };
        let to = if forward {
            (at + SKIP).min(duration)
        } else {
            at.saturating_sub(SKIP)
        };
        self.seek_fraction(to.as_secs_f32() / duration.as_secs_f32(), cx);
    }

    fn ensure_ticking(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let keep = this
                .update(cx, |this, cx| {
                    cx.notify();
                    let playing = this.is_active() && Playback::global().state().playing;
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
}

impl Render for HistoryPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let ring = cx.theme().ring;
        let focus = self
            .scrub_focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let st = Playback::global().state();
        let active = self.is_active();
        let playing = active && st.playing;
        let duration = if active && !st.duration.is_zero() {
            Some(st.duration)
        } else {
            self.duration
        };
        let position = if active { st.position } else { Duration::ZERO };
        let fraction = duration
            .filter(|d| !d.is_zero())
            .map(|d| (position.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let ready = self.path.is_some();
        let bar = self.bar.clone();
        let bar_paint = self.bar.clone();
        let mono = |text: String| {
            design::mono(text)
                .flex_none()
                .min_w(dp(34.))
                .text_color(tone.text_muted)
        };
        let scrubber: AnyElement = match &self.error {
            Some(e) => h_flex()
                .id("history-audio-missing")
                .flex_1()
                .gap(dp(6.))
                .text_size(design::TEXT_SECONDARY)
                .text_color(tone.text_muted)
                .child(icon("circle-alert").xsmall())
                .child(e.clone())
                .test_support()
                .into_any_element(),
            None => div()
                .id("history-seek")
                .track_focus(&focus)
                .flex_1()
                .h(dp(16.))
                .relative()
                .flex()
                .items_center()
                .rounded(design::RADIUS_CHIP)
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .aria_label(t("settings.history.position"))
                .test_support()
                .child(
                    div()
                        .w_full()
                        .h(dp(4.))
                        .rounded(design::RADIUS_TRACK)
                        .bg(tone.raised)
                        .child(
                            div()
                                .h_full()
                                .rounded(design::RADIUS_TRACK)
                                .bg(tone.accent)
                                .w(relative(fraction)),
                        ),
                )
                .when(ready, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(dp(2.))
                            .left(relative(fraction))
                            .ml(dp(-5.))
                            .size(dp(10.))
                            .rounded_full()
                            .bg(tone.surface)
                            .border_1()
                            .border_color(tone.accent),
                    )
                })
                .child(
                    canvas(move |b, _, _| bar_paint.set(Some(b)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .on_mouse_down(MouseButton::Left, {
                    let bar = bar.clone();
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        if let Some(f) = bar_fraction(ev.position, &bar) {
                            this.seek_fraction(f, cx);
                        }
                        if let Some(h) = this.scrub_focus.clone() {
                            window.focus(&h, cx);
                        }
                    })
                })
                .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        if let Some(f) = bar_fraction(ev.position, &bar) {
                            this.seek_fraction(f, cx);
                        }
                    }
                }))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    match ev.keystroke.key.as_str() {
                        "left" => this.skip(false, cx),
                        "right" => this.skip(true, cx),
                        "space" => this.toggle(cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .into_any_element(),
        };
        h_flex()
            .id("history-player")
            .w_full()
            .h(dp(36.))
            .gap(dp(8.))
            .px(dp(12.))
            .when(crate::ui::kit::rtl(), |d| d.flex_row_reverse())
            .child(
                Button::new("history-play")
                    .ghost()
                    .small()
                    .icon(icon(if playing { "pause" } else { "play" }))
                    .tooltip(if playing {
                        t("settings.history.pause")
                    } else {
                        t("settings.history.play")
                    })
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(mono(format_time(position)))
            .child(scrubber)
            .child(mono(
                duration.map(format_time).unwrap_or_else(|| "--:--".into()),
            ))
            .test_support()
    }
}

/// The fraction of the scrubber under `position`.
fn bar_fraction(position: Point<Pixels>, bar: &Cell<Option<Bounds<Pixels>>>) -> Option<f32> {
    let b = bar.get()?;
    (b.size.width > px(0.)).then(|| ((position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0))
}
