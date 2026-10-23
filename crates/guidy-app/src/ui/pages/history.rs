//! History page: infinite list of transcriptions
//! with copy / star / re-transcribe / delete and an inline audio player.

use crate::logic::flows::format_history_date;
use crate::logic::history::{HistoryList, PAGE_SIZE};
use crate::playback::{format_time, Playback};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{icon, page, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::history::HistoryEntry;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub struct HistoryPage {
    state: Entity<AppState>,
    list: HistoryList,
    retrying: HashSet<i64>,
    copied: Option<i64>,
    /// Entry id → resolved audio path (for the active player).
    audio_paths: std::collections::HashMap<i64, PathBuf>,
    ticking: bool,
    _subs: Vec<Subscription>,
}

impl HistoryPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::History(p) = ev {
                    this.list.apply_event(p);
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            state,
            list: HistoryList {
                loading: true,
                ..Default::default()
            },
            retrying: HashSet::new(),
            copied: None,
            audio_paths: Default::default(),
            ticking: false,
            _subs: subs,
        };
        this.load_page(None, cx);
        this
    }

    fn load_page(&mut self, cursor: Option<i64>, cx: &mut Context<Self>) {
        let first = cursor.is_none();
        if !first {
            if self.list.loading_more {
                return;
            }
            self.list.loading_more = true;
        } else {
            self.list.loading = true;
        }
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
                        this.list.loading = false;
                        this.list.loading_more = false;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.list.entries.len()
    }

    fn backend(&self, cx: &App) -> crate::backend::SharedBackend {
        self.state.read(cx).backend.clone()
    }

    fn toast_error(&self, key: &str, cx: &mut Context<Self>) {
        let msg = t(key);
        self.state.update(cx, |s, cx| s.toast_error(msg, cx));
    }

    fn copy(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(entry.transcription_text.clone()));
        if cx.read_from_clipboard().and_then(|c| c.text()).as_deref()
            != Some(entry.transcription_text.as_str())
        {
            self.toast_error("settings.history.copyError", cx);
            return;
        }
        self.copied = Some(entry.id);
        let id = entry.id;
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
        cx.notify();
        let backend = self.backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.toggle_history_entry_saved(id) })
                .await;
            if r.is_err() {
                this.update(cx, |this, cx| {
                    this.list.toggle_saved(id);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        // The entry's player goes away with it.
        if let Some(path) = self.audio_paths.remove(&id) {
            let player = Playback::global();
            if player.state().path.as_ref() == Some(&path) {
                player.stop();
            }
        }
        self.list.remove(id);
        cx.notify();
        let backend = self.backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.delete_history_entry(id) })
                .await;
            if r.is_err() {
                this.update(cx, |this, cx| {
                    this.toast_error("settings.history.deleteError", cx);
                    this.load_page(None, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn retranscribe(&mut self, id: i64, cx: &mut Context<Self>) {
        self.retrying.insert(id);
        cx.notify();
        let backend = self.backend(cx);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { backend.retry_history_entry_transcription(id) })
                .await;
            this.update(cx, |this, cx| {
                this.retrying.remove(&id);
                if r.is_err() {
                    this.toast_error("settings.history.retranscribeError", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn play(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) {
        let player = Playback::global();
        let st = player.state();
        if let Some(path) = self.audio_paths.get(&entry.id) {
            if st.path.as_ref() == Some(path) {
                if st.playing {
                    player.pause();
                } else {
                    player.resume();
                }
                self.ensure_ticking(cx);
                return;
            }
        }
        let backend = self.backend(cx);
        let (id, file) = (entry.id, entry.file_name.clone());
        cx.spawn(async move |this, cx| {
            let path = cx
                .background_spawn(async move { backend.get_audio_file_path(&file) })
                .await;
            this.update(cx, |this, cx| match path {
                Ok(p) => {
                    let p = PathBuf::from(p);
                    this.audio_paths.insert(id, p.clone());
                    Playback::global().play(p);
                    this.ensure_ticking(cx);
                }
                Err(e) => log::error!("Failed to get audio file path: {e}"),
            })
            .ok();
        })
        .detach();
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
        cx.notify();
    }

    fn render_player(&self, entry: &HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let st = Playback::global().state();
        let active = self
            .audio_paths
            .get(&entry.id)
            .is_some_and(|p| st.path.as_ref() == Some(p));
        let playing = active && st.playing;
        let fraction = if active && !st.duration.is_zero() {
            (st.position.as_secs_f32() / st.duration.as_secs_f32()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
        let bounds_paint = bounds.clone();
        let id = entry.id;
        let duration = st.duration;
        let e = entry.clone();
        h_flex()
            .gap(dp(10.))
            .w_full()
            .px(dp(10.))
            .py(dp(8.))
            .border_t_1()
            .border_color(tone.hairline)
            .child(
                Button::new(SharedString::from(format!("play-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(icon(if playing { "pause" } else { "play" }))
                    .on_click(cx.listener(move |this, _, _, cx| this.play(&e, cx))),
            )
            .child(
                design::mono(if active {
                    format_time(st.position)
                } else {
                    "0:00".into()
                })
                .text_color(tone.text_muted),
            )
            .child(
                div()
                    .id(SharedString::from(format!("seek-{id}")))
                    .flex_1()
                    .h(dp(4.))
                    .rounded_full()
                    .bg(tone.raised)
                    .cursor_pointer()
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(tone.accent)
                            .w(relative(fraction)),
                    )
                    .child(
                        canvas(move |b, _, _| bounds_paint.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let bounds = bounds.clone();
                        move |ev, _, _| {
                            if active {
                                seek_to(ev.position, &bounds, duration);
                            }
                        }
                    })
                    // Drag to scrub, like an HTML range input.
                    .on_mouse_move(move |ev, _, _| {
                        if active && ev.pressed_button == Some(MouseButton::Left) {
                            seek_to(ev.position, &bounds, duration);
                        }
                    }),
            )
            .child(
                design::mono(if active {
                    format_time(st.duration)
                } else {
                    "--:--".into()
                })
                .text_color(tone.text_muted),
            )
    }

    fn render_entry(&self, entry: &HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let id = entry.id;
        let retrying = self.retrying.contains(&id);
        let has_text = !entry.transcription_text.trim().is_empty();
        let lang = guidy_i18n::language();
        let e_copy = entry.clone();
        let icon_btn = |name: &'static str, label: SharedString, suffix: &str| {
            Button::new(SharedString::from(format!("{suffix}-{id}")))
                .ghost()
                .xsmall()
                .icon(icon(name))
                .tooltip(label)
        };
        let tone = design::tone(cx);
        design::panel(cx)
            .id(SharedString::from(format!("history-entry-{id}")))
            .child(
                h_flex()
                    .pl(design::SECTION_X)
                    .pr(dp(10.))
                    .pt(dp(10.))
                    .justify_between()
                    .child(design::eyebrow(
                        format_history_date(entry.timestamp, &lang),
                        cx,
                    ))
                    .child(
                        h_flex()
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
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.copy(&e_copy, cx)),
                                ),
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
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.toggle_saved(id, cx)),
                                ),
                            )
                            .child(
                                icon_btn("rotate-ccw", t("settings.history.retranscribe"), "retry")
                                    .loading(retrying)
                                    .disabled(retrying)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.retranscribe(id, cx)
                                        }),
                                    ),
                            )
                            .child(
                                icon_btn("trash", t("settings.history.delete"), "delete")
                                    .disabled(retrying)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.delete(id, cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .px(design::SECTION_X)
                    .pt(dp(6.))
                    .pb(dp(12.))
                    .text_size(design::TEXT_BODY)
                    .line_height(relative(1.6))
                    .when(retrying, |d| {
                        // Skeleton lines under a passing sheen stand in for
                        // the text that's on its way.
                        let line = |w: f32, i: usize| {
                            div()
                                .relative()
                                .h(dp(8.))
                                .w(relative(w))
                                .rounded_full()
                                .bg(tone.raised)
                                .overflow_hidden()
                                .child(design::sheen(("skeleton", id as usize * 2 + i), dp(4.), cx))
                        };
                        d.child(
                            v_flex()
                                .gap(dp(9.))
                                .child(h_flex().child(design::chip("", true, cx).child(
                                    design::shimmer_text(
                                        ("transcribing", id as usize),
                                        t("settings.history.transcribing"),
                                        cx,
                                    ),
                                )))
                                .child(line(0.72, 0))
                                .child(line(0.48, 1)),
                        )
                    })
                    .when(!retrying && has_text, |d| {
                        d.child(entry.transcription_text.clone())
                    })
                    .when(!retrying && !has_text, |d| {
                        d.text_color(tone.text_muted)
                            .child(t("settings.history.transcriptionFailed"))
                    }),
            )
            .child(self.render_player(entry, cx))
            .test_support()
    }
}

impl Render for HistoryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let notice = |text: SharedString| {
            design::panel(cx)
                .child(
                    div()
                        .px(design::SECTION_X)
                        .py(dp(18.))
                        .text_center()
                        .text_size(design::TEXT_BODY)
                        .text_color(tone.text_muted)
                        .child(text),
                )
                .into_any_element()
        };
        let content: AnyElement = if self.list.loading {
            notice(t("settings.history.loading"))
        } else if self.list.entries.is_empty() {
            design::panel(cx)
                .child(
                    design::blueprint(dp(150.), cx).child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(design::illustration("waveform", dp(220.), dp(96.), cx)),
                    ),
                )
                .child(
                    div()
                        .px(design::SECTION_X)
                        .pb(dp(16.))
                        .text_center()
                        .text_size(design::TEXT_BODY)
                        .text_color(tone.text_muted)
                        .child(t("settings.history.empty")),
                )
                .into_any_element()
        } else {
            let entries = self.list.entries.clone();
            let rows: Vec<AnyElement> = entries
                .iter()
                .map(|e| self.render_entry(e, cx).into_any_element())
                .collect();
            // Sentinel: when it's laid out inside the window, fetch the next page.
            let entity = cx.entity().downgrade();
            let has_more = self.list.has_more;
            let cursor = self.list.next_cursor();
            let sentinel = canvas(
                move |bounds, window, cx| {
                    if has_more && bounds.origin.y < window.viewport_size().height {
                        let entity = entity.clone();
                        cx.defer(move |cx| {
                            entity
                                .update(cx, |this, cx| this.load_page(cursor, cx))
                                .ok();
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .w_full()
            .h(dp(4.));
            v_flex()
                .gap(dp(12.))
                .children(rows)
                .child(sentinel)
                .into_any_element()
        };
        page()
            .id("page-history")
            .child(
                v_flex()
                    .gap(dp(14.))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(crate::ui::kit::crumb_title(
                                t("settings.history.title"),
                                None,
                                design::TEXT_HEADLINE,
                                cx,
                            ))
                            .child(
                                Button::new("open-recordings")
                                    .secondary()
                                    .small()
                                    .pill()
                                    .icon(icon("folder-open"))
                                    .label(t("settings.history.openFolder"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let backend = this.backend(cx);
                                        cx.background_spawn(async move {
                                            if let Err(e) = backend.open_recordings_folder() {
                                                log::error!(
                                                    "Failed to open recordings folder: {e}"
                                                );
                                            }
                                        })
                                        .detach();
                                    })),
                            ),
                    )
                    .child(content),
            )
            .test_support()
    }
}

/// Seek to the fraction of the bar under `position`.
fn seek_to(position: Point<Pixels>, bounds: &Cell<Option<Bounds<Pixels>>>, duration: Duration) {
    if let Some(b) = bounds.get() {
        let x = ((position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
        Playback::global().seek(duration.mul_f32(x));
    }
}
