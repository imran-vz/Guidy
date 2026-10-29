//! People: a list | detail split view. The list has a search field, "You"
//! (the user's own voice, recorded here so in-person meetings can tell them
//! from the others in the room) pinned at the top, then everyone named in
//! meetings. The detail edits the selected person (name and email save on
//! Enter or when the field loses focus; Esc puts them back), plays their
//! voiceprints, merges them into another person after listening to both,
//! and deletes them (after a confirmation).

use super::{run, toast_error};
use crate::logic::meetings::{format_meeting_date, merge_targets, non_empty, MeetingsEvent};
use crate::playback::Playback;
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp};
use crate::ui::kit::{
    group, icon, page, scroll_page, t, t_args, Dropdown, DropdownOption, LocalizedPlaceholder,
    SettingRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::UserVoice;
use guidy_core::meetings::types::{Person, PersonId, VoiceSample};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Where the "Your voice" recording is.
#[derive(Clone, Copy, PartialEq)]
enum VoiceRecording {
    Idle,
    Starting,
    Recording { since: Instant },
    Saving,
}

/// "Done" is enabled after this much recording (speech is trimmed, so the
/// sample needs some slack over the 8 s minimum).
const MIN_RECORDING: Duration = Duration::from_secs(12);
/// The recording stops by itself after this long.
const MAX_RECORDING: Duration = Duration::from_secs(60);

/// Meeting audio is decoded to 16 kHz mono for playback.
const SAMPLE_RATE: u32 = 16_000;

/// The list's width in narrow windows (otherwise it is the split the
/// Meetings tabs share).
const LIST_W_NARROW: Pixels = dp(184.);

/// What the detail shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeopleSelection {
    /// The user's own voice.
    You,
    Person(PersonId),
}

pub struct PeopleView {
    /// The list | detail split the Meetings page shares with this tab
    /// (its own when shown alone).
    split: Option<Entity<ResizableState>>,
    voice: UserVoice,
    recording: VoiceRecording,
    people: Vec<Person>,
    loading: bool,
    selected: PeopleSelection,
    /// The person picked to merge the selected one into (their voiceprints
    /// show too).
    merge_into: Option<PersonId>,
    /// Voiceprints with the lines they came from, per person (`None`:
    /// loading).
    samples: HashMap<PersonId, Option<Vec<VoiceSample>>>,
    /// Redrawing while a voiceprint plays.
    ticking: bool,
    search: Entity<InputState>,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    list_focus: FocusHandle,
    detail_scroll: ScrollHandle,
    /// The person and the stored name/email last put into the inputs.
    inputs_for: Option<(PersonId, String, Option<String>)>,
    _subs: Vec<Subscription>,
}

impl PeopleView {
    /// Share the Meetings page's list | detail split.
    pub fn with_split(mut self, split: Entity<ResizableState>) -> Self {
        self.split = Some(split);
        self
    }

    fn split(&mut self, cx: &mut Context<Self>) -> Entity<ResizableState> {
        self.split
            .get_or_insert_with(|| cx.new(|_| ResizableState::default()))
            .clone()
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.people.searchPlaceholder",
                window,
                cx,
            )
        });
        let name_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.people.namePlaceholder",
                window,
                cx,
            )
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.people.emailPlaceholder",
                window,
                cx,
            )
        });
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(MeetingsEvent::PeopleChanged) = ev {
                    this.reload(cx);
                }
            }),
            cx.subscribe_in(&name_input, window, Self::on_edit_event),
            cx.subscribe_in(&email_input, window, Self::on_edit_event),
            cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            split: None,
            voice: UserVoice::default(),
            recording: VoiceRecording::Idle,
            people: Vec::new(),
            loading: true,
            selected: PeopleSelection::You,
            merge_into: None,
            samples: HashMap::new(),
            ticking: false,
            search,
            name_input,
            email_input,
            list_focus: cx.focus_handle().tab_stop(true),
            detail_scroll: ScrollHandle::new(),
            inputs_for: None,
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| b.user_voice(),
            |this: &mut Self, r, cx| match r {
                Ok(voice) => this.voice = voice,
                Err(e) => toast_error("meetings.people.voice.loadError", &e, cx),
            },
        );
        run(
            cx,
            |b| b.list_people(),
            |this: &mut Self, r, cx| {
                this.loading = false;
                match r {
                    Ok(mut people) => {
                        people.sort_by_key(|p| p.display_name.to_lowercase());
                        this.people = people;
                    }
                    Err(e) => toast_error("meetings.people.loadError", &e, cx),
                }
            },
        );
    }

    fn start_voice_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording != VoiceRecording::Idle {
            return;
        }
        self.recording = VoiceRecording::Starting;
        cx.notify();
        run(
            cx,
            |b| b.start_voice_recording(),
            |this: &mut Self, r, cx| match r {
                Ok(()) => {
                    this.recording = VoiceRecording::Recording {
                        since: Instant::now(),
                    };
                    this.tick_recording(cx);
                }
                Err(e) => {
                    this.recording = VoiceRecording::Idle;
                    toast_error("meetings.people.voice.recordError", &e, cx);
                }
            },
        );
    }

    /// Redraw the timer while recording; stop at [`MAX_RECORDING`].
    fn tick_recording(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let keep = this
                .update(cx, |this, cx| match this.recording {
                    VoiceRecording::Recording { since } => {
                        if since.elapsed() >= MAX_RECORDING {
                            this.finish_voice_recording(cx);
                            false
                        } else {
                            cx.notify();
                            true
                        }
                    }
                    _ => false,
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    pub fn finish_voice_recording(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.recording, VoiceRecording::Recording { .. }) {
            return;
        }
        self.recording = VoiceRecording::Saving;
        cx.notify();
        run(
            cx,
            |b| b.finish_voice_recording(),
            |this: &mut Self, r, cx| {
                this.recording = VoiceRecording::Idle;
                match r {
                    Ok(voice) => this.voice = voice,
                    Err(e) => toast_error("meetings.people.voice.saveError", &e, cx),
                }
            },
        );
    }

    fn cancel_voice_recording(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.recording, VoiceRecording::Recording { .. }) {
            return;
        }
        self.recording = VoiceRecording::Idle;
        cx.notify();
        run(cx, |b| b.cancel_voice_recording(), |_: &mut Self, _, _| {});
    }

    fn confirm_delete_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("meetings.people.voice.deleteTitle"))
                .description(t("meetings.people.voice.deleteConfirm"))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| {
                        this.voice = UserVoice::default();
                        cx.notify();
                        run(
                            cx,
                            |b| b.delete_user_voice(),
                            |this: &mut Self, r, cx| {
                                if let Err(e) = r {
                                    toast_error("meetings.people.voice.deleteError", &e, cx);
                                    this.reload(cx);
                                }
                            },
                        );
                    })
                    .ok();
                    true
                })
        });
    }

    fn render_voice(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let recorded = self.voice.recorded_at.is_some();
        let text_col = |title: SharedString, line: SharedString| {
            v_flex()
                .min_w_0()
                .flex_1()
                .gap(dp(2.))
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_size(crate::ui::design::TEXT_SECONDARY)
                        .line_height(relative(1.5))
                        .text_color(theme.muted_foreground)
                        .child(line),
                )
        };
        let row = h_flex()
            .id("people-voice")
            .w_full()
            .gap_3()
            .px_3()
            .py_2()
            .justify_between();
        let row = match self.recording {
            VoiceRecording::Recording { since } => {
                let elapsed = since.elapsed();
                let timer = crate::logic::overlay::format_timer(elapsed.as_secs());
                return v_flex()
                    .id("people-voice")
                    .w_full()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_between()
                            .child(text_col(
                                t("meetings.people.voice.recording"),
                                t("meetings.people.voice.readAloud"),
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(crate::theme::MONO_FONT)
                                    .text_sm()
                                    .text_color(theme.danger)
                                    .child(timer),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .px_3()
                            .py_2()
                            .rounded(crate::ui::design::RADIUS_PANEL)
                            .bg(theme.background)
                            .text_size(design::TEXT_BODY)
                            .line_height(relative(1.6))
                            .child(t("meetings.people.voice.passage")),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("people-voice-cancel")
                                    .ghost()
                                    .small()
                                    .label(t("meetings.people.cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_voice_recording(cx)
                                    })),
                            )
                            .child(
                                Button::new("people-voice-done")
                                    .primary()
                                    .small()
                                    .label(t("meetings.people.voice.done"))
                                    .disabled(elapsed < MIN_RECORDING)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.finish_voice_recording(cx)
                                    })),
                            ),
                    )
                    .test_support()
                    .into_any_element();
            }
            VoiceRecording::Starting | VoiceRecording::Saving => row
                .child(text_col(
                    t(if recorded {
                        "meetings.people.voice.recordedTitle"
                    } else {
                        "meetings.people.voice.notRecorded"
                    }),
                    t(if self.recording == VoiceRecording::Saving {
                        "meetings.people.voice.saving"
                    } else {
                        "meetings.people.voice.starting"
                    }),
                ))
                .child(
                    Button::new("people-voice-record")
                        .primary()
                        .small()
                        .loading(true)
                        .label(t("meetings.people.voice.record")),
                ),
            VoiceRecording::Idle if recorded => row
                .child(text_col(
                    t("meetings.people.voice.recordedTitle"),
                    t("meetings.people.voice.recorded"),
                ))
                .child(
                    h_flex()
                        .flex_none()
                        .gap_1()
                        .child(
                            Button::new("people-voice-record")
                                .ghost()
                                .small()
                                .label(t("meetings.people.voice.recordAgain"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.start_voice_recording(cx)),
                                ),
                        )
                        .child(
                            Button::new("people-voice-delete")
                                .ghost()
                                .xsmall()
                                .icon(icon("trash"))
                                .tooltip(t("meetings.people.delete"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_delete_voice(window, cx)
                                })),
                        ),
                ),
            VoiceRecording::Idle => row
                .child(text_col(
                    t("meetings.people.voice.notRecorded"),
                    t("meetings.people.voice.description"),
                ))
                .child(
                    Button::new("people-voice-record")
                        .primary()
                        .small()
                        .icon(icon("mic"))
                        .label(t("meetings.people.voice.record"))
                        .on_click(cx.listener(|this, _, _, cx| this.start_voice_recording(cx))),
                ),
        };
        row.test_support().into_any_element()
    }

    #[cfg(test)]
    pub fn people_count(&self) -> usize {
        self.people.len()
    }

    pub fn selected(&self) -> PeopleSelection {
        self.selected
    }

    /// Show the user's own voice.
    pub fn select_you(&mut self, cx: &mut Context<Self>) {
        if self.selected != PeopleSelection::You {
            self.reset_detail();
            self.selected = PeopleSelection::You;
            self.detail_scroll.set_offset(point(px(0.), px(0.)));
        }
        cx.notify();
    }

    /// Show `id`: their name and email in the form, their voiceprints
    /// loading. Pending edits of the person shown before are saved first.
    pub fn select_person(&mut self, id: PersonId, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected == PeopleSelection::Person(id) {
            return;
        }
        self.save_edit(window, cx);
        self.reset_detail();
        self.selected = PeopleSelection::Person(id);
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
        self.inputs_for = None;
        self.sync_inputs(window, cx);
        self.load_samples(id, cx);
        cx.notify();
    }

    fn select(&mut self, sel: PeopleSelection, window: &mut Window, cx: &mut Context<Self>) {
        match sel {
            PeopleSelection::You => {
                self.save_edit(window, cx);
                self.select_you(cx)
            }
            PeopleSelection::Person(id) => self.select_person(id, window, cx),
        }
    }

    /// The people the search matches, in list order.
    fn shown_people(&self, cx: &App) -> Vec<&Person> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        self.people
            .iter()
            .filter(|p| {
                query.is_empty()
                    || p.display_name.to_lowercase().contains(&query)
                    || p.email
                        .as_deref()
                        .is_some_and(|e| e.to_lowercase().contains(&query))
            })
            .collect()
    }

    /// ↑/↓ through You and the people shown; ⌫ deletes the selected person.
    fn on_list_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if matches!(key, "backspace" | "delete") {
            if let PeopleSelection::Person(id) = self.selected {
                if let Some(p) = self.people.iter().find(|p| p.id == id).cloned() {
                    cx.stop_propagation();
                    self.confirm_delete(&p, window, cx);
                }
            }
            return;
        }
        let delta: isize = match key {
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        cx.stop_propagation();
        let mut order = vec![PeopleSelection::You];
        order.extend(
            self.shown_people(cx)
                .into_iter()
                .map(|p| PeopleSelection::Person(p.id)),
        );
        let i = order.iter().position(|s| *s == self.selected).unwrap_or(0) as isize;
        let next = order[(i + delta).clamp(0, order.len() as isize - 1) as usize];
        self.select(next, window, cx);
    }

    fn on_edit_event(
        &mut self,
        _: &Entity<InputState>,
        ev: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
            self.save_edit(window, cx);
        }
    }

    /// Put the selected person's stored name and email in the form, unless
    /// the user is typing there or they are already in.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PeopleSelection::Person(id) = self.selected else {
            return;
        };
        let Some(p) = self.people.iter().find(|p| p.id == id) else {
            return;
        };
        let stored = (id, p.display_name.clone(), p.email.clone());
        if self.inputs_for.as_ref() == Some(&stored) {
            return;
        }
        let typing = self.name_input.focus_handle(cx).is_focused(window)
            || self.email_input.focus_handle(cx).is_focused(window);
        if typing && self.inputs_for.as_ref().is_some_and(|(p, _, _)| *p == id) {
            return;
        }
        let (name, email) = (stored.1.clone(), stored.2.clone().unwrap_or_default());
        self.name_input
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.email_input
            .update(cx, |s, cx| s.set_value(email, window, cx));
        self.inputs_for = Some(stored);
    }

    /// Esc in the form: the stored name and email come back and the
    /// keyboard goes back to the list.
    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revert_edit(window, cx);
        window.focus(&self.list_focus, cx);
    }

    /// Back to the stored name and email.
    fn revert_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PeopleSelection::Person(id) = self.selected else {
            return;
        };
        if let Some(p) = self.people.iter().find(|p| p.id == id) {
            let (name, email) = (p.display_name.clone(), p.email.clone().unwrap_or_default());
            self.inputs_for = Some((id, p.display_name.clone(), p.email.clone()));
            self.name_input
                .update(cx, |s, cx| s.set_value(name, window, cx));
            self.email_input
                .update(cx, |s, cx| s.set_value(email, window, cx));
        }
        cx.notify();
    }

    #[cfg(test)]
    pub fn type_edit(
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

    /// Test hook: what Enter or leaving the field does.
    #[cfg(test)]
    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_edit(window, cx);
    }

    #[cfg(test)]
    pub fn edit_values(&self, cx: &App) -> (String, String) {
        (
            self.name_input.read(cx).value().to_string(),
            self.email_input.read(cx).value().to_string(),
        )
    }

    /// Save the form when it differs from the stored person. An emptied
    /// name isn't saved: the stored one comes back.
    fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PeopleSelection::Person(id) = self.selected else {
            return;
        };
        let Some(p) = self.people.iter_mut().find(|p| p.id == id) else {
            return;
        };
        let Some(name) = non_empty(&self.name_input.read(cx).value()) else {
            self.revert_edit(window, cx);
            return;
        };
        let email = non_empty(&self.email_input.read(cx).value());
        if name == p.display_name && email == p.email {
            return;
        }
        p.display_name = name.clone();
        p.email = email.clone();
        self.inputs_for = Some((id, name.clone(), email.clone()));
        cx.notify();
        run(
            cx,
            move |b| b.rename_person(id, &name, email.as_deref()),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.people.renameError", &e, cx);
                }
                this.reload(cx);
            },
        );
    }

    /// Forget the detail's merge pick and voiceprints, stopping one that
    /// plays.
    fn reset_detail(&mut self) {
        self.merge_into = None;
        self.samples.clear();
        let player = Playback::global();
        let playing_sample = player
            .state()
            .path
            .is_some_and(|p| p.to_string_lossy().starts_with("voiceprint-"));
        if playing_sample {
            player.stop();
        }
    }

    fn confirm_merge(
        &mut self,
        from: PersonId,
        into: PersonId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = |id| {
            self.people
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.display_name.clone())
                .unwrap_or_default()
        };
        let message = t_args(
            "meetings.people.mergeConfirm",
            &[("from", &name(from)), ("into", &name(into))],
        );
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("meetings.people.mergeTitle"))
                .description(message.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| this.merge(from, into, cx)).ok();
                    true
                })
        });
    }

    pub fn merge(&mut self, from: PersonId, into: PersonId, cx: &mut Context<Self>) {
        self.reset_detail();
        self.people.retain(|p| p.id != from);
        if self.selected == PeopleSelection::Person(from) {
            self.selected = PeopleSelection::Person(into);
            self.load_samples(into, cx);
        }
        cx.notify();
        run(
            cx,
            move |b| b.merge_people(from, into),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.people.mergeError", &e, cx);
                }
                this.reload(cx);
            },
        );
    }

    fn confirm_delete(&mut self, p: &Person, window: &mut Window, cx: &mut Context<Self>) {
        let id = p.id;
        let message = t_args(
            "meetings.people.deleteConfirm",
            &[
                ("name", &p.display_name),
                ("count", &p.voiceprint_count.to_string()),
            ],
        );
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            alert
                .title(t("meetings.people.deleteTitle"))
                .description(message.clone())
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    this.update(cx, |this, cx| this.delete(id, cx)).ok();
                    true
                })
        });
    }

    pub fn delete(&mut self, id: PersonId, cx: &mut Context<Self>) {
        self.people.retain(|p| p.id != id);
        if self.selected == PeopleSelection::Person(id) {
            self.reset_detail();
            self.selected = PeopleSelection::You;
        }
        cx.notify();
        run(
            cx,
            move |b| b.delete_person(id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.people.deleteError", &e, cx);
                    this.reload(cx);
                }
            },
        );
    }

    fn person_name(&self, id: PersonId) -> SharedString {
        self.people
            .iter()
            .find(|p| p.id == id)
            .map(|p| SharedString::from(p.display_name.clone()))
            .unwrap_or_default()
    }

    /// Pick whom to merge into; their voiceprints load next to the other's.
    pub fn pick_merge_target(&mut self, into: PersonId, cx: &mut Context<Self>) {
        if let Some(old) = self.merge_into.replace(into) {
            if PeopleSelection::Person(old) != self.selected {
                self.samples.remove(&old);
            }
        }
        self.load_samples(into, cx);
        cx.notify();
    }

    fn load_samples(&mut self, id: PersonId, cx: &mut Context<Self>) {
        if self.samples.contains_key(&id) {
            return;
        }
        self.samples.insert(id, None);
        run(
            cx,
            move |b| b.voice_samples(id),
            move |this: &mut Self, r, cx| match r {
                Ok(samples) => {
                    // Only while the panel still shows them.
                    if this.samples.contains_key(&id) {
                        this.samples.insert(id, Some(samples));
                    }
                }
                Err(e) => {
                    this.samples.insert(id, Some(Vec::new()));
                    toast_error("meetings.people.samples.loadError", &e, cx);
                }
            },
        );
    }

    fn sample_key(sample: &VoiceSample) -> PathBuf {
        PathBuf::from(format!("voiceprint-{}", sample.voiceprint_id))
    }

    fn play_sample(&mut self, sample: &VoiceSample, cx: &mut Context<Self>) {
        let (Some(meeting_id), Some(seg)) = (sample.meeting_id, sample.segment.as_ref()) else {
            return;
        };
        let player = Playback::global();
        let key = Self::sample_key(sample);
        let st = player.state();
        if st.path.as_ref() == Some(&key) {
            if st.playing {
                player.pause();
            } else {
                player.resume();
            }
            self.ensure_ticking(cx);
            return;
        }
        let (track, t0, t1) = (seg.track, seg.t0_ms, seg.t1_ms);
        run(
            cx,
            move |b| b.segment_audio(meeting_id, track, t0, t1),
            move |this: &mut Self, r, cx| match r {
                Ok(samples) => {
                    Playback::global().play_samples(key, samples, SAMPLE_RATE);
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

    /// One person's voiceprints, each playable where its line's audio is
    /// still kept. `heading`: their name above them (the merge target).
    fn render_samples(
        &self,
        person: PersonId,
        heading: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let list = v_flex()
            .id(SharedString::from(format!("person-samples-{person}")))
            .w_full()
            .px(design::SECTION_X)
            .py(design::ROW_Y)
            .gap(dp(4.))
            .when(heading, |d| {
                d.child(
                    div()
                        .text_size(design::TEXT_SECONDARY)
                        .font_weight(FontWeight::MEDIUM)
                        .child(self.person_name(person)),
                )
            });
        let note = |text: SharedString| {
            div()
                .py(dp(2.))
                .text_size(design::TEXT_SECONDARY)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let Some(Some(samples)) = self.samples.get(&person) else {
            return list
                .child(note(t("common.loading")))
                .test_support()
                .into_any_element();
        };
        if samples.is_empty() {
            return list
                .child(note(t("meetings.people.samples.none")))
                .test_support()
                .into_any_element();
        }
        let st = Playback::global().state();
        let lang = guidy_i18n::language();
        let rows = samples.iter().map(|sample| {
            let playing = st.playing && st.path.as_ref() == Some(&Self::sample_key(sample));
            let playable = sample.has_audio && sample.segment.is_some();
            let line: SharedString = match &sample.segment {
                Some(seg) if !seg.text.trim().is_empty() => {
                    format!("\u{201c}{}\u{201d}", seg.text.trim()).into()
                }
                Some(_) => t("meetings.people.samples.noText"),
                None => t("meetings.people.samples.noLine"),
            };
            let mut place: Vec<String> = Vec::new();
            if let Some(title) = &sample.meeting_title {
                place.push(title.clone());
            }
            if let Some(at) = sample.meeting_started_at {
                place.push(format_meeting_date(at, &lang));
            }
            if let Some(seg) = &sample.segment {
                place.push(super::summary::format_timestamp(seg.t0_ms));
            }
            if sample.segment.is_some() && !sample.has_audio {
                place.push(t("meetings.people.samples.audioDeleted").to_string());
            }
            let s = sample.clone();
            h_flex()
                .id(SharedString::from(format!(
                    "voice-sample-row-{}",
                    sample.voiceprint_id
                )))
                .w_full()
                .gap_2()
                .child(
                    Button::new(SharedString::from(format!(
                        "voice-sample-{}",
                        sample.voiceprint_id
                    )))
                    .ghost()
                    .xsmall()
                    .icon(icon(if playing { "pause" } else { "play" }))
                    .tooltip(t(if playing {
                        "meetings.people.samples.pause"
                    } else {
                        "meetings.people.samples.play"
                    }))
                    .disabled(!playable)
                    .on_click(cx.listener(move |this, _, _, cx| this.play_sample(&s, cx))),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .child(div().text_size(design::TEXT_BODY).truncate().child(line))
                        .child(
                            div()
                                .text_size(design::TEXT_SECONDARY)
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(place.join(" \u{b7} ")),
                        ),
                )
                .test_support()
                .into_any_element()
        });
        list.children(rows).test_support().into_any_element()
    }

    /// Pick whom to merge `id` into, listen to both people's voiceprints,
    /// then merge (after a confirmation).
    fn render_merge(&self, id: PersonId, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity().downgrade();
        let options: Vec<DropdownOption> = merge_targets(&self.people, id)
            .into_iter()
            .map(|p| {
                let opt = DropdownOption::new(p.id.to_string(), p.display_name.clone());
                match &p.email {
                    Some(e) => opt.description(e.clone()),
                    None => opt,
                }
            })
            .collect();
        let into = self.merge_into;
        let mut rows = vec![SettingRow::new(
            SharedString::from(format!("person-merge-into-{id}")),
            t("meetings.people.mergeInto"),
        )
        .description(t("meetings.people.mergeHint"))
        .stacked()
        .control(
            Dropdown::new(SharedString::from(format!("person-merge-target-{id}")))
                .options(options)
                .selected(into.map(|p| p.to_string()))
                .placeholder(t("meetings.people.choosePerson"))
                .min_width(dp(180.))
                .on_select(move |value, _, cx| {
                    let Ok(into) = value.parse::<PersonId>() else {
                        return;
                    };
                    entity
                        .update(cx, |this, cx| this.pick_merge_target(into, cx))
                        .ok();
                }),
        )
        .into_any_element()];
        if let Some(into) = into {
            rows.push(self.render_samples(into, true, cx));
        }
        rows.push(
            h_flex()
                .w_full()
                .px(design::SECTION_X)
                .py(design::ROW_Y)
                .justify_end()
                .child(
                    Button::new(SharedString::from(format!("person-merge-confirm-{id}")))
                        .outline()
                        .small()
                        .icon(icon("merge"))
                        .label(match into {
                            Some(into) => t_args(
                                "meetings.people.mergeIntoName",
                                &[("name", &self.person_name(into))],
                            ),
                            None => t("meetings.people.merge"),
                        })
                        .disabled(into.is_none())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(into) = into {
                                this.confirm_merge(id, into, window, cx);
                            }
                        })),
                )
                .into_any_element(),
        );
        group(Some(t("meetings.people.mergeHeading")), rows, cx)
            .id(SharedString::from(format!("person-merge-panel-{id}")))
            .test_support()
            .into_any_element()
    }

    /// The selected person: name and email, voiceprints, merge, delete.
    fn render_person(&self, p: &Person, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tone = design::tone(cx);
        let id = p.id;
        let count = p.voiceprint_count.to_string();
        let header = v_flex()
            .id("person-detail-header")
            .gap(dp(2.))
            .child(
                div()
                    .text_size(design::TEXT_HEADLINE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(p.display_name.clone()),
            )
            .child(
                div()
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(t_args("meetings.people.voiceprints", &[("count", &count)])),
            )
            .test_support()
            .into_any_element();
        let field = |id: &'static str, title: SharedString, input: &Entity<InputState>| {
            h_flex()
                .id(id)
                .w_full()
                .min_h(design::ROW_MIN_H)
                .gap(dp(12.))
                .px(design::SECTION_X)
                .py(design::ROW_Y)
                .child(
                    div()
                        .flex_none()
                        .w(dp(64.))
                        .text_size(design::TEXT_BODY)
                        .child(title),
                )
                .child(div().flex_1().min_w_0().child(Input::new(input).small()))
                .test_support()
                .into_any_element()
        };
        let form = group(
            Some(t("meetings.people.details")),
            vec![
                field("person-name", t("meetings.people.name"), &self.name_input),
                field(
                    "person-email",
                    t("meetings.people.email"),
                    &self.email_input,
                ),
            ],
            cx,
        )
        .id(SharedString::from(format!("person-{id}-form")))
        .on_action(cx.listener(|this, _: &Escape, window, cx| this.cancel_edit(window, cx)))
        .test_support()
        .into_any_element();
        let voiceprints = group(
            Some(t("meetings.people.voiceprintsHeading")),
            vec![
                div()
                    .px(design::SECTION_X)
                    .pt(design::ROW_Y)
                    .text_size(design::TEXT_SECONDARY)
                    .text_color(tone.text_muted)
                    .child(t("meetings.people.samples.hint"))
                    .into_any_element(),
                self.render_samples(id, false, cx),
            ],
            cx,
        )
        .into_any_element();
        let p_delete = p.clone();
        let delete = h_flex()
            .w_full()
            .child(
                Button::new(SharedString::from(format!("person-delete-{id}")))
                    .outline()
                    .small()
                    .icon(icon("trash"))
                    .label(t("meetings.people.deleteButton"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_delete(&p_delete, window, cx)
                    })),
            )
            .into_any_element();
        let mut out = vec![header, form, voiceprints];
        if self.people.len() > 1 {
            out.push(self.render_merge(id, cx));
        }
        out.push(delete);
        out
    }

    /// A row of the list: icon or nothing, title, a secondary line.
    #[allow(clippy::too_many_arguments)]
    fn list_row(
        &self,
        id: SharedString,
        sel: PeopleSelection,
        title: SharedString,
        line: SharedString,
        lead: Option<&'static str>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = design::tone(cx);
        let active = self.selected == sel;
        let selection = if focused {
            tone.selection_focused
        } else {
            tone.selection
        };
        h_flex()
            .id(id)
            .aria_label(title.clone())
            .flex_none()
            .w_full()
            .gap(dp(8.))
            .px(dp(8.))
            .py(dp(4.))
            .rounded(design::RADIUS_CHIP)
            .when(active, |d| d.bg(selection))
            .when_some(lead, |d, name| {
                d.child(icon(name).small().text_color(if active {
                    tone.accent_text
                } else {
                    tone.text_muted
                }))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(design::TEXT_BODY)
                            .text_color(tone.text)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(tone.text_muted)
                            .truncate()
                            .child(line),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.list_focus, cx);
                this.select(sel, window, cx);
            }))
            .test_support()
            .into_any_element()
    }

    fn render_list(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let focused = self.list_focus.is_focused(window);
        // In the shared split the panel sizes the list.
        let narrow = super::is_narrow(window);
        let you = self.list_row(
            "people-you".into(),
            PeopleSelection::You,
            t("meetings.people.you"),
            t(if self.voice.recorded_at.is_some() {
                "meetings.people.voice.recordedTitle"
            } else {
                "meetings.people.voice.notRecorded"
            }),
            Some("circle-user"),
            focused,
            cx,
        );
        let note = |id: &'static str, text: SharedString| {
            div()
                .id(id)
                .px(dp(8.))
                .py(dp(6.))
                .text_size(design::TEXT_SECONDARY)
                .text_color(tone.text_muted)
                .child(text)
                .test_support()
                .into_any_element()
        };
        let shown: Vec<Person> = self.shown_people(cx).into_iter().cloned().collect();
        let rows: Vec<AnyElement> = if self.loading {
            vec![note("people-loading", t("common.loading"))]
        } else if self.people.is_empty() {
            vec![note("people-empty", t("meetings.people.empty"))]
        } else if shown.is_empty() {
            vec![note("people-no-matches", t("meetings.people.noMatches"))]
        } else {
            shown
                .iter()
                .map(|p| {
                    let count = p.voiceprint_count.to_string();
                    let prints = t_args("meetings.people.voiceprints", &[("count", &count)]);
                    let line: SharedString = match &p.email {
                        Some(e) => format!("{e} · {prints}").into(),
                        None => prints,
                    };
                    self.list_row(
                        SharedString::from(format!("person-{}", p.id)),
                        PeopleSelection::Person(p.id),
                        p.display_name.clone().into(),
                        line,
                        None,
                        focused,
                        cx,
                    )
                })
                .collect()
        };
        v_flex()
            .h_full()
            .min_h_0()
            .map(|d| {
                if narrow {
                    d.flex_none()
                        .w(LIST_W_NARROW)
                        .border_r_1()
                        .border_color(tone.hairline)
                } else {
                    d.size_full()
                }
            })
            .child(
                div().flex_none().p(dp(8.)).child(
                    Input::new(&self.search)
                        .small()
                        .cleanable(true)
                        .prefix(icon("search").xsmall().text_color(tone.text_muted)),
                ),
            )
            .child(
                v_flex()
                    .id("people-list")
                    .track_focus(&self.list_focus)
                    .on_key_down(cx.listener(Self::on_list_key))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(dp(8.))
                    .pb(dp(8.))
                    .gap(px(1.))
                    .child(you)
                    .child(
                        div()
                            .px(dp(8.))
                            .pt(dp(10.))
                            .pb(dp(4.))
                            .child(design::section_header(t("meetings.people.title"), cx)),
                    )
                    .children(rows)
                    .test_support(),
            )
    }
}

impl Render for PeopleView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The selected person may be gone (deleted or merged elsewhere).
        if let PeopleSelection::Person(id) = self.selected {
            if !self.loading && !self.people.iter().any(|p| p.id == id) {
                self.selected = PeopleSelection::You;
                self.reset_detail();
            }
        }
        self.sync_inputs(window, cx);
        let detail: Vec<AnyElement> = match self.selected {
            PeopleSelection::You => vec![
                crate::ui::kit::lede(t("meetings.people.voice.description"), cx).into_any_element(),
                group(
                    Some(t("meetings.people.voice.title")),
                    vec![self.render_voice(cx)],
                    cx,
                )
                .into_any_element(),
            ],
            PeopleSelection::Person(id) => match self.people.iter().find(|p| p.id == id).cloned() {
                Some(p) => self.render_person(&p, cx),
                None => Vec::new(),
            },
        };
        let list = self.render_list(window, cx);
        let detail = div().size_full().min_w_0().min_h_0().child(
            scroll_page("people-detail", page().children(detail)).track_scroll(&self.detail_scroll),
        );
        let narrow = super::is_narrow(window);
        h_flex()
            .id("meetings-people")
            .size_full()
            .min_h_0()
            .min_w_0()
            .items_stretch()
            .map(|d| {
                if narrow {
                    d.child(list).child(div().flex_1().min_w_0().child(detail))
                } else {
                    let split = self.split(cx);
                    d.child(super::split_view("people-split", &split, list, detail, cx))
                }
            })
            .test_support()
    }
}
