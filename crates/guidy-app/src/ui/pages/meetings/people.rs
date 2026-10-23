//! People tab: the user's own voice (recorded here, so in-person meetings
//! can tell them from the others in the room), and everyone named in
//! meetings, with voiceprint counts. Edit (name + email), merge into another
//! person (after listening to both people's voiceprints), delete (with
//! voiceprints).

use super::{run, toast_error};
use crate::logic::meetings::{format_meeting_date, merge_targets, non_empty, MeetingsEvent};
use crate::playback::Playback;
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{icon, t, t_args, Dropdown, DropdownOption};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
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

pub struct PeopleView {
    voice: UserVoice,
    recording: VoiceRecording,
    people: Vec<Person>,
    loading: bool,
    editing: Option<PersonId>,
    merging: Option<PersonId>,
    /// The person picked to merge into (their voiceprints show too).
    merge_into: Option<PersonId>,
    /// Voiceprints with the lines they came from, per person (`None`:
    /// loading).
    samples: HashMap<PersonId, Option<Vec<VoiceSample>>>,
    /// Redrawing while a voiceprint plays.
    ticking: bool,
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl PeopleView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetings.people.namePlaceholder"))
        });
        let email_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetings.people.emailPlaceholder"))
        });
        let subs = vec![
            cx.subscribe_in(&state, window, |this, _, ev: &AppEvent, _, cx| {
                if let AppEvent::Meetings(MeetingsEvent::PeopleChanged) = ev {
                    this.reload(cx);
                }
            }),
            cx.subscribe(&name_input, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.save_edit(cx);
                }
            }),
            cx.subscribe(&email_input, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.save_edit(cx);
                }
            }),
        ];
        let mut this = Self {
            voice: UserVoice::default(),
            recording: VoiceRecording::Idle,
            people: Vec::new(),
            loading: true,
            editing: None,
            merging: None,
            merge_into: None,
            samples: HashMap::new(),
            ticking: false,
            name_input,
            email_input,
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
                        .text_xs()
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
                            .rounded(dp(8.))
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
                                    .pill()
                                    .small()
                                    .label(t("meetings.people.cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_voice_recording(cx)
                                    })),
                            )
                            .child(
                                Button::new("people-voice-done")
                                    .primary()
                                    .pill()
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
                        .pill()
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
                                .pill()
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
                        .pill()
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

    fn start_edit(&mut self, p: &Person, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(p.id);
        self.close_merge();
        let (name, email) = (p.display_name.clone(), p.email.clone().unwrap_or_default());
        self.name_input.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
        });
        self.email_input
            .update(cx, |s, cx| s.set_value(email, window, cx));
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

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        let Some(name) = non_empty(&self.name_input.read(cx).value()) else {
            return;
        };
        let email = non_empty(&self.email_input.read(cx).value());
        self.editing = None;
        if let Some(p) = self.people.iter_mut().find(|p| p.id == id) {
            p.display_name = name.clone();
            p.email = email.clone();
        }
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
        self.close_merge();
        self.people.retain(|p| p.id != from);
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

    /// Open the merge panel of `id`, loading their voiceprints.
    fn open_merge(&mut self, id: PersonId, cx: &mut Context<Self>) {
        self.close_merge();
        self.merging = Some(id);
        self.load_samples(id, cx);
    }

    fn close_merge(&mut self) {
        self.merging = None;
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

    /// Pick whom to merge into; their voiceprints load next to the other's.
    pub fn pick_merge_target(&mut self, into: PersonId, cx: &mut Context<Self>) {
        self.merge_into = Some(into);
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
    /// still kept.
    fn render_samples(&self, person: PersonId, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let header = div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .child(self.person_name(person));
        let list = v_flex()
            .id(SharedString::from(format!("person-samples-{person}")))
            .w_full()
            .gap(dp(4.))
            .child(header);
        let Some(Some(samples)) = self.samples.get(&person) else {
            return list
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t("common.loading")),
                )
                .test_support()
                .into_any_element();
        };
        if samples.is_empty() {
            return list
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t("meetings.people.samples.none")),
                )
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
                        .child(div().text_xs().truncate().child(line))
                        .child(
                            div()
                                .text_xs()
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

    /// Pick whom to merge into, listen to both people's voiceprints, then
    /// merge.
    fn render_merge_panel(&self, id: PersonId, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
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
        v_flex()
            .id(SharedString::from(format!("person-merge-panel-{id}")))
            .w_full()
            .gap_3()
            .px_3()
            .py_2()
            .bg(theme.background)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t("meetings.people.mergeHint")),
                    )
                    .child(
                        Dropdown::new(SharedString::from(format!("person-merge-target-{id}")))
                            .options(options)
                            .selected(into.map(|p| p.to_string()))
                            .placeholder(t("meetings.people.mergeInto"))
                            .on_select(move |value, _, cx| {
                                let Ok(into) = value.parse::<PersonId>() else {
                                    return;
                                };
                                entity
                                    .update(cx, |this, cx| this.pick_merge_target(into, cx))
                                    .ok();
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t("meetings.people.samples.hint")),
            )
            .child(self.render_samples(id, cx))
            .when_some(into, |d, into| d.child(self.render_samples(into, cx)))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new(SharedString::from(format!("person-merge-cancel-{id}")))
                            .ghost()
                            .pill()
                            .small()
                            .label(t("meetings.people.cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_merge();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("person-merge-confirm-{id}")))
                            .primary()
                            .pill()
                            .small()
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
                    ),
            )
            .test_support()
            .into_any_element()
    }

    fn render_row(&self, p: &Person, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = p.id;
        let icon_btn = |name: &'static str, label: SharedString, suffix: &str| {
            Button::new(SharedString::from(format!("person-{suffix}-{id}")))
                .ghost()
                .xsmall()
                .icon(icon(name))
                .tooltip(label)
        };
        if self.editing == Some(id) {
            return h_flex()
                .id(SharedString::from(format!("person-{id}")))
                .w_full()
                .gap_2()
                .px_3()
                .py_2()
                .child(div().flex_1().child(Input::new(&self.name_input).small()))
                .child(div().flex_1().child(Input::new(&self.email_input).small()))
                .child(
                    Button::new(SharedString::from(format!("person-save-{id}")))
                        .primary()
                        .pill()
                        .small()
                        .label(t("meetings.people.save"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("person-edit-cancel-{id}")))
                        .ghost()
                        .pill()
                        .small()
                        .label(t("meetings.people.cancel"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.editing = None;
                            cx.notify();
                        })),
                )
                .test_support()
                .into_any_element();
        }
        let count = p.voiceprint_count.to_string();
        let (p_edit, p_delete) = (p.clone(), p.clone());
        let can_merge = self.people.len() > 1;
        let merging = self.merging == Some(id);
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .id(SharedString::from(format!("person-{id}")))
                    .w_full()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(p.display_name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(match &p.email {
                                        Some(e) => format!(
                                            "{e} · {}",
                                            t_args(
                                                "meetings.people.voiceprints",
                                                &[("count", &count)]
                                            )
                                        ),
                                        None => t_args(
                                            "meetings.people.voiceprints",
                                            &[("count", &count)],
                                        )
                                        .to_string(),
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .child(
                                icon_btn("pencil", t("meetings.people.edit"), "edit").on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.start_edit(&p_edit, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                icon_btn("merge", t("meetings.people.merge"), "merge")
                                    .disabled(!can_merge)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.merging == Some(id) {
                                            this.close_merge();
                                        } else {
                                            this.open_merge(id, cx);
                                        }
                                        this.editing = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_btn("trash", t("meetings.people.delete"), "delete").on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.confirm_delete(&p_delete, window, cx)
                                    }),
                                ),
                            ),
                    )
                    .test_support(),
            )
            .when(merging, |d| d.child(self.render_merge_panel(id, cx)))
            .into_any_element()
    }
}

impl Render for PeopleView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = if self.loading {
            vec![div()
                .px_4()
                .py_3()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(t("common.loading"))
                .into_any_element()]
        } else if self.people.is_empty() {
            vec![div()
                .id("people-empty")
                .px_4()
                .py(dp(18.))
                .text_center()
                .text_size(design::TEXT_BODY)
                .text_color(theme.muted_foreground)
                .child(t("meetings.people.empty"))
                .test_support()
                .into_any_element()]
        } else {
            let people = self.people.clone();
            people.iter().map(|p| self.render_row(p, cx)).collect()
        };
        // A tab under the Meetings headline: eyebrow and a quiet line, not a
        // second headline.
        v_flex()
            .id("meetings-people")
            .w_full()
            .gap(dp(8.))
            .child(crate::ui::kit::group(
                Some(t("meetings.people.voice.title")),
                vec![self.render_voice(cx)],
                cx,
            ))
            .child(
                div()
                    .px(dp(2.))
                    .pt(dp(8.))
                    .child(design::eyebrow(t("meetings.people.title"), cx)),
            )
            .child(
                div()
                    .px(dp(2.))
                    .text_size(dp(12.))
                    .line_height(relative(1.5))
                    .text_color(theme.muted_foreground)
                    .child(t("meetings.people.description")),
            )
            .child(crate::ui::kit::group(None, rows, cx))
            .test_support()
    }
}
