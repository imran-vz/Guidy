//! Meeting settings: the master switch (requests permissions), the user's
//! name and email, the meeting model, audio retention, calendar
//! notifications, the reminder interval, and the speaker-match thresholds
//! under "Advanced". G2's AI + Linear settings follow.
//!
//! Every change writes only the changed fields through
//! `update_setting("meetings", {"field": value})`; the backend merges the
//! patch into the stored object.

use super::ai_settings::AiSettingsView;
use super::emails::UserEmailsView;
use super::{
    meeting_model_missing, meeting_settings, meetings_backend, render_model_download,
    render_model_download_with, run, toast_error,
};
use crate::logic::meetings::{
    meetings_patch, non_empty, parse_retention, retention_value, MeetingsEvent,
    NOTIFICATIONS_SETTINGS_URL, PRIVACY_AUDIO_CAPTURE_URL, PRIVACY_CALENDARS_URL,
    PRIVACY_MICROPHONE_URL,
};
use crate::logic::models::format_model_size;
use crate::meetings_backend::error_message;
use crate::state::AppEvent;
use crate::state::AppState;
use crate::ui::design::{self, dp, Pill};
use crate::ui::kit::{
    group, icon, model_name, t, t_args, toggle, Dropdown, DropdownOption, SettingRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::managers::transcription::meeting::{
    configured_meeting_model_missing, DEFAULT_MEETING_MODEL_ID, DEFAULT_MEETING_MODEL_NAME,
};
use guidy_core::meetings::manager::PermissionReport;
use guidy_core::meetings::processing::{
    speaker_turn_models_bytes, SpeakerModelStatus, SPEAKER_MODEL_NAME, SPEAKER_MODEL_SIZE,
    SPEAKER_MODEL_TRAINED_ON,
};
use guidy_core::meetings::settings::{MeetingSettings, RecordingControls, RETENTION_CHOICES};
use guidy_core::meetings::types::{DiarizationOptions, SpeakerModelId};

const REMINDER_CHOICES: [u32; 5] = [5, 10, 15, 20, 30];

/// Persist a change to the nested meetings settings: only the fields `f`
/// changes are sent.
pub fn update_meetings(cx: &mut App, f: impl FnOnce(&mut MeetingSettings)) {
    let before = meeting_settings(cx);
    let mut after = before.clone();
    f(&mut after);
    let patch = meetings_patch(&before, &after);
    if patch.as_object().is_some_and(|p| !p.is_empty()) {
        AppState::global(cx).update(cx, |s, cx| s.update_setting("meetings", patch, cx));
    }
}

/// Which permission a row is about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Permission {
    Calendar,
    SystemAudio,
    Microphone,
    Notifications,
}

impl Permission {
    fn id(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::SystemAudio => "system-audio",
            Self::Microphone => "microphone",
            Self::Notifications => "notifications",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Calendar => "meetings.settings.permissions.calendar",
            Self::SystemAudio => "meetings.settings.permissions.systemAudio",
            Self::Microphone => "meetings.settings.permissions.microphone",
            Self::Notifications => "meetings.settings.permissions.notifications",
        }
    }
    /// The System Settings pane where a denial can be changed.
    fn pane(self) -> Option<&'static str> {
        match self {
            Self::Calendar => Some(PRIVACY_CALENDARS_URL),
            Self::SystemAudio => Some(PRIVACY_AUDIO_CAPTURE_URL),
            Self::Microphone => Some(PRIVACY_MICROPHONE_URL),
            Self::Notifications => Some(NOTIFICATIONS_SETTINGS_URL),
        }
    }
    fn granted(self, p: &PermissionReport) -> bool {
        match self {
            Self::Calendar => p.calendar_granted,
            Self::SystemAudio => p.system_audio_granted,
            Self::Microphone => p.microphone_granted,
            Self::Notifications => p.notifications_granted,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Threshold {
    Match,
    Suggest,
}

/// The live model dropdown's "same as meetings" value (not a model id).
const LIVE_SAME_MODEL: &str = "__same__";

pub struct MeetingSettingsView {
    name_input: Entity<InputState>,
    emails: Entity<UserEmailsView>,
    match_slider: Entity<SliderState>,
    suggest_slider: Entity<SliderState>,
    dragging: bool,
    permissions: Option<PermissionReport>,
    requesting: bool,
    speaker_model: Option<SpeakerModelStatus>,
    /// The speaker models for telling voices apart.
    diarization: Option<DiarizationOptions>,
    /// The speaker model being downloaded from its row.
    downloading_separation: Option<SpeakerModelId>,
    /// This view started the speaker-turn download.
    turn_busy: bool,
    /// A speaker-turn download is in the shared tracker.
    turn_downloading: bool,
    /// Deepgram key for "Redo speakers" in the cloud (never shown back).
    deepgram_key: Entity<InputState>,
    has_deepgram_key: bool,
    advanced_open: bool,
    ai: Entity<AiSettingsView>,
    _subs: Vec<Subscription>,
}

impl MeetingSettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let current = meeting_settings(cx);
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("meetings.settings.userName.placeholder"))
                .default_value(current.user_display_name.clone())
        });
        let slider = |v: f32, cx: &mut Context<Self>| {
            cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(1.0)
                    .step(0.01)
                    .default_value(v)
            })
        };
        let deepgram_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("meetings.settings.deepgram.placeholder"))
        });
        let match_slider = slider(current.speaker_match_threshold, cx);
        let suggest_slider = slider(current.speaker_suggest_threshold, cx);
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync(window, cx);
                cx.notify();
            }),
            cx.subscribe(&name_input, |_, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let name = input.read(cx).value().trim().to_string();
                    if name != meeting_settings(cx).user_display_name {
                        update_meetings(cx, |m| m.user_display_name = name);
                    }
                }
            }),
            cx.subscribe_in(
                &deepgram_key,
                window,
                |this, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let key = input.read(cx).value().trim().to_string();
                        if !key.is_empty() {
                            this.save_deepgram_key(key, window, cx);
                        }
                    }
                },
            ),
            cx.subscribe(&match_slider, |this, _, ev: &SliderEvent, cx| {
                this.on_slider(Threshold::Match, ev, cx)
            }),
            cx.subscribe(&suggest_slider, |this, _, ev: &SliderEvent, cx| {
                this.on_slider(Threshold::Suggest, ev, cx)
            }),
            cx.subscribe(&state, |this, _, ev: &AppEvent, cx| match ev {
                AppEvent::Meetings(MeetingsEvent::SpeakerModelStatus(s)) => {
                    this.speaker_model = Some(s.clone());
                    cx.notify();
                }
                AppEvent::Meetings(MeetingsEvent::DownloadsChanged(items)) => {
                    this.turn_downloading = items.iter().any(|d| {
                        d.state.is_active()
                            && (d.id.starts_with("separation:")
                                || d.id.starts_with("segmentation:"))
                    });
                    cx.notify();
                }
                _ => {}
            }),
        ];
        let mut this = Self {
            name_input,
            emails: cx.new(|cx| UserEmailsView::new(window, cx)),
            match_slider,
            suggest_slider,
            dragging: false,
            permissions: None,
            requesting: false,
            speaker_model: None,
            diarization: None,
            downloading_separation: None,
            turn_busy: false,
            turn_downloading: false,
            deepgram_key,
            has_deepgram_key: false,
            advanced_open: false,
            ai: cx.new(|cx| AiSettingsView::new(window, cx)),
            _subs: subs,
        };
        this.reload_status(cx);
        this
    }

    /// Permission grants and the speaker model's state (no prompts).
    pub fn reload_status(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| {
                (
                    b.permission_status(),
                    b.speaker_model_status(),
                    b.diarization_options(),
                )
            },
            |this: &mut Self, (p, speaker, diarization), _| {
                if !this.requesting {
                    this.permissions = Some(p);
                }
                this.speaker_model = Some(speaker);
                this.has_deepgram_key = diarization.deepgram_has_key;
                this.diarization = Some(diarization);
            },
        );
    }

    /// Save (`key` non-empty) or remove the Deepgram key; the input is
    /// cleared either way (keys are never shown back).
    fn save_deepgram_key(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        self.deepgram_key
            .update(cx, |s, cx| s.set_value("", window, cx));
        let saved = !key.is_empty();
        run(
            cx,
            move |b| b.set_deepgram_api_key(&key),
            move |this: &mut Self, r, cx| match r {
                Ok(()) => this.has_deepgram_key = saved,
                Err(e) => toast_error("meetings.settings.deepgram.error", &e, cx),
            },
        );
    }

    fn render_deepgram_key(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        SettingRow::new(
            "meetings-deepgram-key",
            t("meetings.settings.deepgram.title"),
        )
        .description(t("meetings.settings.deepgram.description"))
        .inline_description()
        .stacked()
        .control(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&self.deepgram_key).small().mask_toggle()),
                )
                .when(self.has_deepgram_key, |d| {
                    d.child(
                        h_flex()
                            .gap_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(icon("check").xsmall().text_color(theme.success))
                            .child(t("meetings.settings.deepgram.saved")),
                    )
                    .child(
                        Button::new("meetings-deepgram-remove")
                            .ghost()
                            .pill()
                            .small()
                            .label(t("meetings.settings.deepgram.remove"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_deepgram_key(String::new(), window, cx)
                            })),
                    )
                }),
        )
        .into_any_element()
    }

    fn request_permissions(&mut self, cx: &mut Context<Self>) {
        if self.requesting {
            return;
        }
        self.requesting = true;
        cx.notify();
        run(
            cx,
            |b| b.request_permissions(),
            |this: &mut Self, r, cx| {
                this.requesting = false;
                match r {
                    Ok(p) => this.permissions = Some(p),
                    Err(e) => toast_error("meetings.settings.permissions.error", &e, cx),
                }
            },
        );
    }

    fn open_pane(&mut self, url: &'static str, cx: &mut Context<Self>) {
        run(
            cx,
            move |b| b.open_url(url),
            |_: &mut Self, r, _| {
                if let Err(e) = r {
                    log::warn!("Failed to open System Settings: {e}");
                }
            },
        );
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
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    this.speaker_model = Some(SpeakerModelStatus::Failed {
                        error: error_message(&e),
                    });
                    cx.notify();
                }
            },
        );
    }

    /// Mirror stored values into inputs and sliders (not while editing).
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = meeting_settings(cx);
        let sync_input = |input: &Entity<InputState>,
                          value: String,
                          window: &mut Window,
                          cx: &mut Context<Self>| {
            let focused = input.focus_handle(cx).is_focused(window);
            if !focused && input.read(cx).value().as_ref() != value.as_str() {
                input.update(cx, |s, cx| s.set_value(value, window, cx));
            }
        };
        sync_input(
            &self.name_input.clone(),
            m.user_display_name.clone(),
            window,
            cx,
        );
        if !self.dragging {
            for (slider, v) in [
                (self.match_slider.clone(), m.speaker_match_threshold),
                (self.suggest_slider.clone(), m.speaker_suggest_threshold),
            ] {
                if (slider.read(cx).value().start() - v).abs() > f32::EPSILON {
                    slider.update(cx, |s, cx| s.set_value(v, window, cx));
                }
            }
        }
    }

    fn on_slider(&mut self, which: Threshold, ev: &SliderEvent, cx: &mut Context<Self>) {
        match ev {
            SliderEvent::Change(_) => {
                self.dragging = true;
                cx.notify();
            }
            SliderEvent::Release(v) => {
                self.dragging = false;
                let value = crate::ui::kit::round_to_step(v.start(), 0.01);
                update_meetings(cx, |m| match which {
                    Threshold::Match => m.speaker_match_threshold = value,
                    Threshold::Suggest => m.speaker_suggest_threshold = value,
                });
            }
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        update_meetings(cx, |m| m.enabled = enabled);
        // The speaker model downloads only when asked (its row's button).
        if enabled {
            self.request_permissions(cx);
        }
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.permissions.clone()?;
        let theme = cx.theme().clone();
        let rows: Vec<AnyElement> = [
            Permission::Calendar,
            Permission::SystemAudio,
            Permission::Microphone,
            Permission::Notifications,
        ]
        .into_iter()
        .map(|perm| {
            let granted = perm.granted(&p);
            h_flex()
                .id(SharedString::from(format!(
                    "meetings-permission-{}",
                    perm.id()
                )))
                .w_full()
                .gap_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .child(
                            icon(if granted { "check" } else { "x" })
                                .xsmall()
                                .text_color(if granted { theme.success } else { theme.danger }),
                        )
                        .child(t(perm.key()))
                        .child(div().text_color(theme.muted_foreground).child(if granted {
                            t("meetings.settings.permissions.granted")
                        } else {
                            t("meetings.settings.permissions.denied")
                        })),
                )
                .when(!granted, |d| {
                    d.child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "meetings-permission-request-{}",
                                    perm.id()
                                )))
                                .ghost()
                                .pill()
                                .xsmall()
                                .label(t("meetings.settings.permissions.requestAgain"))
                                .disabled(self.requesting)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.request_permissions(cx)),
                                ),
                            )
                            .when_some(perm.pane(), |d, url| {
                                d.child(
                                    Button::new(SharedString::from(format!(
                                        "meetings-permission-open-{}",
                                        perm.id()
                                    )))
                                    .ghost()
                                    .pill()
                                    .xsmall()
                                    .icon(icon("external-link"))
                                    .label(t("meetings.settings.permissions.openSettings"))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.open_pane(url, cx)),
                                    ),
                                )
                            }),
                    )
                })
                .test_support()
                .into_any_element()
        })
        .collect();
        Some(
            SettingRow::new(
                "meetings-permissions",
                t("meetings.settings.permissions.title"),
            )
            .stacked()
            .control(v_flex().w_full().gap_1().children(rows))
            .into_any_element(),
        )
    }

    /// Use `id` to tell voices apart, downloading it first when needed.
    pub fn choose_separation_model(&mut self, id: SpeakerModelId, cx: &mut Context<Self>) {
        update_meetings(cx, |m| m.speaker_separation_model = id.as_str().to_string());
        let installed = self
            .diarization
            .as_ref()
            .and_then(|d| d.models.iter().find(|m| m.id == id))
            .is_some_and(|m| m.installed);
        if !installed && !id.is_standard() && self.downloading_separation.is_none() {
            self.downloading_separation = Some(id);
            cx.notify();
            run(
                cx,
                move |b| b.download_separation_model(id),
                |this: &mut Self, r, cx| {
                    this.downloading_separation = None;
                    if let Err(e) = r {
                        toast_error("meetings.settings.separation.downloadError", &e, cx);
                    }
                    this.reload_status(cx);
                },
            );
        }
    }

    /// Delete a downloaded speaker model; meetings set to it go back to the
    /// standard one.
    fn delete_separation_model(&mut self, id: SpeakerModelId, cx: &mut Context<Self>) {
        if SpeakerModelId::parse(&meeting_settings(cx).speaker_separation_model) == id {
            update_meetings(cx, |m| m.speaker_separation_model = String::new());
        }
        run(
            cx,
            move |b| b.delete_separation_model(id),
            |this: &mut Self, r, cx| {
                if let Err(e) = r {
                    toast_error("meetings.settings.separation.deleteError", &e, cx);
                }
                this.reload_status(cx);
            },
        );
    }

    /// Which speaker model tells voices apart: a size and accuracy
    /// trade-off for the user to make.
    fn render_separation_model(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let options = self.diarization.as_ref().filter(|d| !d.models.is_empty())?;
        let theme = cx.theme().clone();
        let chosen = SpeakerModelId::parse(&meeting_settings(cx).speaker_separation_model);
        let items: Vec<DropdownOption> = options
            .models
            .iter()
            .map(|m| {
                let state = if m.installed {
                    t("meetings.settings.separation.downloaded")
                } else {
                    t("meetings.settings.separation.notDownloaded")
                };
                DropdownOption::new(m.id.as_str(), format!("{} · {} MB", m.name, m.size_mb))
                    .description(format!(
                        "{} {}",
                        t(&format!(
                            "meetings.settings.separation.about.{}",
                            m.id.as_str()
                        )),
                        state
                    ))
            })
            .collect();
        let current = options.models.iter().find(|m| m.id == chosen);
        let downloading = self.downloading_separation;
        let deletable =
            current.filter(|m| m.installed && !m.id.is_standard() && downloading.is_none());
        let status: Option<AnyElement> = match (downloading, current) {
            (Some(_), _) => Some(
                h_flex()
                    .id("meetings-separation-downloading")
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(crate::ui::kit::spinner(
                        "meetings-separation-spinner",
                        icon("loader-circle").xsmall(),
                    ))
                    .child(t("meetings.settings.separation.downloading"))
                    .test_support()
                    .into_any_element(),
            ),
            (None, Some(m)) if !m.installed && !m.id.is_standard() => Some(
                Button::new("meetings-separation-download")
                    .primary()
                    .pill()
                    .small()
                    .icon(icon("download"))
                    .label(t_args(
                        "meetings.settings.speakerModel.download",
                        &[("size", &format!("{} MB", m.size_mb))],
                    ))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.choose_separation_model(chosen, cx)),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };
        Some(
            SettingRow::new(
                "meetings-separation-model",
                t("meetings.settings.separation.title"),
            )
            .description(t("meetings.settings.separation.description"))
            .control(
                h_flex()
                    .gap_2()
                    .children(status)
                    .when_some(deletable, |d, m| {
                        let id = m.id;
                        d.child(
                            Button::new("meetings-separation-delete")
                                .ghost()
                                .xsmall()
                                .icon(icon("trash"))
                                .tooltip(t_args(
                                    "meetings.settings.separation.delete",
                                    &[("size", &format!("{} MB", m.size_mb))],
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.delete_separation_model(id, cx)
                                })),
                        )
                    })
                    .child(
                        Dropdown::new("meetings-separation-dropdown")
                            .options(items)
                            .selected(Some(chosen.as_str()))
                            .min_width(dp(240.))
                            .menu_width(dp(340.))
                            .disabled(downloading.is_some())
                            .on_select({
                                let weak = cx.entity().downgrade();
                                move |v, _, cx| {
                                    let id = SpeakerModelId::parse(v);
                                    weak.update(cx, |this, cx| {
                                        this.choose_separation_model(id, cx)
                                    })
                                    .ok();
                                }
                            }),
                    ),
            )
            .into_any_element(),
        )
    }

    fn download_turn_models(&mut self, cx: &mut Context<Self>) {
        update_meetings(cx, |m| {
            m.speaker_turn_models_consent = true;
            m.speaker_turn_prompt_dismissed = true;
        });
        self.turn_busy = true;
        cx.notify();
        run(
            cx,
            |b| b.start_speaker_turn_downloads(),
            |this: &mut Self, r, cx| {
                this.turn_busy = false;
                if let Err(e) = r {
                    toast_error("meetings.downloads.startError", &e, cx);
                }
                cx.notify();
            },
        );
    }

    /// Optional TitaNet + segmentation models. "Not now" on the meetings
    /// page hides the prompt; this row is how to start the download later.
    fn render_turn_models(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed = meetings_backend(cx).speaker_turn_models_installed();
        let bytes = speaker_turn_models_bytes();
        let size = format_model_size(Some(bytes as f64 / (1024.0 * 1024.0)));
        let theme = cx.theme().clone();
        let control: AnyElement = if installed {
            h_flex()
                .id("meetings-turn-models-ready")
                .gap_1()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(icon("check").xsmall().text_color(theme.success))
                .child(t("meetings.settings.turnModels.ready"))
                .test_support()
                .into_any_element()
        } else if self.turn_busy || self.turn_downloading {
            h_flex()
                .id("meetings-turn-models-downloading")
                .gap_1()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(crate::ui::kit::spinner(
                    "meetings-turn-models-spinner",
                    icon("loader-circle").xsmall(),
                ))
                .child(t("meetings.settings.turnModels.downloading"))
                .test_support()
                .into_any_element()
        } else {
            Button::new("meetings-turn-models-download")
                .primary()
                .pill()
                .small()
                .icon(icon("download"))
                .label(t_args(
                    "meetings.settings.turnModels.download",
                    &[("size", &size)],
                ))
                .on_click(cx.listener(|this, _, _, cx| this.download_turn_models(cx)))
                .into_any_element()
        };
        SettingRow::new(
            "meetings-turn-models",
            t("meetings.settings.turnModels.title"),
        )
        .description(t_args(
            "meetings.settings.turnModels.description",
            &[("size", &size)],
        ))
        .inline_description()
        .control(control)
        .into_any_element()
    }

    fn render_speaker_model(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.speaker_model.clone()?;
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let size = format_model_size(Some(SPEAKER_MODEL_SIZE as f64 / (1024.0 * 1024.0)));
        let control: AnyElement = match &status {
            SpeakerModelStatus::Ready => h_flex()
                .gap_1()
                .text_sm()
                .text_color(muted)
                .child(icon("check").xsmall().text_color(theme.success))
                .child(t("meetings.settings.speakerModel.ready"))
                .into_any_element(),
            SpeakerModelStatus::Downloading { downloaded, total } => {
                let fraction = if *total > 0 {
                    (*downloaded as f64 / *total as f64).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                h_flex()
                    .id("meetings-speaker-model-progress")
                    .gap_2()
                    .w(dp(220.))
                    .child(div().flex_1().child(design::track(
                        "meetings-speaker-model-bar",
                        Some((fraction * 100.0) / 100.0),
                        cx,
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{:.0}%", fraction * 100.0)),
                    )
                    .test_support()
                    .into_any_element()
            }
            SpeakerModelStatus::Missing | SpeakerModelStatus::Failed { .. } => {
                let failed = matches!(status, SpeakerModelStatus::Failed { .. });
                Button::new("meetings-speaker-model-download")
                    .primary()
                    .pill()
                    .small()
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
        let description = match &status {
            SpeakerModelStatus::Failed { error } => {
                t_args("meetings.settings.speakerModel.failed", &[("error", error)])
            }
            _ => t_args(
                "meetings.settings.speakerModel.description",
                &[
                    ("name", SPEAKER_MODEL_NAME),
                    ("trainedOn", SPEAKER_MODEL_TRAINED_ON),
                    ("size", &size),
                ],
            ),
        };
        Some(
            SettingRow::new(
                "meetings-speaker-model",
                t("meetings.settings.speakerModel.title"),
            )
            .description(description)
            .inline_description()
            .control(control)
            .into_any_element(),
        )
    }

    fn slider_row(
        &self,
        id: &'static str,
        key: &str,
        slider: &Entity<SliderState>,
        cx: &App,
    ) -> AnyElement {
        let value = slider.read(cx).value().start();
        SettingRow::new(id, t(&format!("{key}.title")))
            .description(t(&format!("{key}.description")))
            .control(
                h_flex()
                    .gap_2()
                    .w(dp(260.))
                    .child(div().flex_1().child(Slider::new(slider)))
                    .child(
                        div()
                            .w(dp(56.))
                            .text_right()
                            .text_sm()
                            .font_family(crate::theme::MONO_FONT)
                            .child(format!("{value:.2}")),
                    ),
            )
            .into_any_element()
    }
}

impl Render for MeetingSettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = meeting_settings(cx);
        let state = AppState::global(cx);
        let updating = state.read(cx).settings.is_updating("meetings");

        // -- general --------------------------------------------------------
        let this = cx.entity().downgrade();
        let mut general =
            vec![
                SettingRow::new("meetings-enabled", t("meetings.settings.enabled.title"))
                    .description(t("meetings.settings.enabled.description"))
                    .inline_description()
                    .control(
                        h_flex()
                            .gap_2()
                            .when(self.requesting, |d| {
                                d.child(crate::ui::kit::spinner(
                                    "meetings-permissions-spinner",
                                    icon("loader-circle").xsmall(),
                                ))
                            })
                            .child(toggle(
                                "meetings-enabled-toggle",
                                m.enabled,
                                updating,
                                move |v, _, cx| {
                                    this.update(cx, |this, cx| this.set_enabled(v, cx)).ok();
                                },
                            )),
                    )
                    .into_any_element(),
            ];
        general.extend(self.render_permissions(cx));

        // -- you ------------------------------------------------------------
        let you = vec![
            SettingRow::new("meetings-user-name", t("meetings.settings.userName.title"))
                .description(t("meetings.settings.userName.description"))
                .stacked()
                .control(div().w_full().child(Input::new(&self.name_input).small()))
                .into_any_element(),
            SettingRow::new(
                "meetings-user-email",
                t("meetings.settings.userEmails.title"),
            )
            .description(t("meetings.settings.userEmails.description"))
            .inline_description()
            .stacked()
            .control(self.emails.clone())
            .into_any_element(),
        ];

        // -- recording ------------------------------------------------------
        let models = state.read(cx).models.models.clone();
        let dictation_id = state
            .read(cx)
            .app_settings()
            .map(|s| s.selected_model.clone())
            .unwrap_or_default();
        let dictation_name = models
            .iter()
            .find(|mi| mi.id == dictation_id && mi.is_downloaded)
            .map(model_name);
        let separate = m
            .meeting_model
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty());
        // Turning "Use a different model" on picks Parakeet v3 (word-level
        // timings), else another downloaded model.
        let first_choice = models
            .iter()
            .find(|mi| mi.id == DEFAULT_MEETING_MODEL_ID)
            .or_else(|| {
                models
                    .iter()
                    .find(|mi| mi.is_downloaded && mi.id != dictation_id)
            })
            .map(|mi| mi.id.clone())
            .unwrap_or_else(|| DEFAULT_MEETING_MODEL_ID.to_string());
        // Parakeet v3 is offered even before it is downloaded.
        let mut model_options: Vec<DropdownOption> = models
            .iter()
            .filter(|mi| mi.id == DEFAULT_MEETING_MODEL_ID && !mi.is_downloaded)
            .map(|mi| {
                DropdownOption::new(
                    mi.id.clone(),
                    t_args(
                        "meetings.settings.meetingModel.notDownloaded",
                        &[("name", DEFAULT_MEETING_MODEL_NAME)],
                    ),
                )
            })
            .collect();
        model_options.extend(
            models
                .iter()
                .filter(|mi| mi.is_downloaded)
                .map(|mi| DropdownOption::new(mi.id.clone(), model_name(mi))),
        );
        let selected_model = m.meeting_model.clone().unwrap_or_default();
        // A chosen model that isn't downloaded: meetings fall back to the
        // dictation model until it is. Nothing usable at all: the banner.
        let chosen_missing = configured_meeting_model_missing(&models, m.meeting_model.as_deref());
        let none_usable = meeting_model_missing(cx);
        let model_notice = match (&chosen_missing, &none_usable) {
            (Some(missing), None) => {
                let key = if missing.known {
                    "meetings.settings.meetingModel.chosenMissing"
                } else {
                    "meetings.settings.meetingModel.chosenUnknown"
                };
                let message = t_args(key, &[("name", &missing.model_name)]);
                Some(render_model_download_with("settings", missing, message, cx))
            }
            (_, Some(missing)) => Some(render_model_download("settings", missing, cx)),
            (None, None) => None,
        };
        let retention_options: Vec<DropdownOption> = RETENTION_CHOICES
            .iter()
            .map(|d| {
                let label = match d {
                    Some(3) => t("meetings.settings.retention.days3"),
                    Some(7) => t("meetings.settings.retention.days7"),
                    Some(30) => t("meetings.settings.retention.days30"),
                    Some(n) => format!("{n}").into(),
                    None => t("meetings.settings.retention.forever"),
                };
                DropdownOption::new(retention_value(*d), label)
            })
            .collect();
        let mut reminder_choices: Vec<u32> = REMINDER_CHOICES.to_vec();
        if !reminder_choices.contains(&m.reminder_interval_minutes) {
            reminder_choices.push(m.reminder_interval_minutes);
            reminder_choices.sort_unstable();
        }
        let reminder_options: Vec<DropdownOption> = reminder_choices
            .iter()
            .map(|n| {
                DropdownOption::new(
                    n.to_string(),
                    t_args(
                        "meetings.settings.reminderInterval.minutes",
                        &[("count", &n.to_string())],
                    ),
                )
            })
            .collect();
        let mut recording =
            vec![
                SettingRow::new("meetings-model", t("meetings.settings.meetingModel.title"))
                    .description(match &dictation_name {
                        Some(name) => t_args(
                            "meetings.settings.meetingModel.usesDictation",
                            &[("name", name)],
                        ),
                        None => t("meetings.settings.meetingModel.noDictation"),
                    })
                    .inline_description()
                    .control(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(dp(12.))
                                    .text_color(design::tone(cx).text_muted)
                                    .child(t("meetings.settings.meetingModel.separate")),
                            )
                            .child(toggle(
                                "meetings-model-separate-toggle",
                                separate,
                                updating,
                                move |on, _, cx| {
                                    let choice = on.then(|| first_choice.clone());
                                    update_meetings(cx, |m| m.meeting_model = choice);
                                },
                            )),
                    )
                    .into_any_element(),
            ];
        if separate {
            recording.push(
                SettingRow::new(
                    "meetings-model-choice",
                    t("meetings.settings.meetingModel.choice"),
                )
                .description(t("meetings.settings.meetingModel.choiceDescription"))
                .inline_description()
                .control(
                    Dropdown::new("meetings-model-dropdown")
                        .options(model_options)
                        .selected(Some(selected_model))
                        .min_width(dp(220.))
                        .on_select(|v, _, cx| {
                            let v = non_empty(v);
                            if v.is_some() {
                                update_meetings(cx, |m| m.meeting_model = v);
                            }
                        }),
                )
                .into_any_element(),
            );
        }
        recording.extend(model_notice.map(|b| {
            div()
                .w_full()
                .px(design::SECTION_X)
                .pb(dp(10.))
                .child(b)
                .into_any_element()
        }));
        let controls_options = vec![
            DropdownOption::new(
                "floating",
                t("meetings.settings.recordingControls.floating"),
            ),
            DropdownOption::new("notch", t("meetings.settings.recordingControls.notch")),
        ];
        recording.extend([
            SettingRow::new(
                "meetings-recording-controls",
                t("meetings.settings.recordingControls.title"),
            )
            .description(t("meetings.settings.recordingControls.description"))
            .control(
                Dropdown::new("meetings-recording-controls-dropdown")
                    .options(controls_options)
                    .selected(Some(match m.recording_controls {
                        RecordingControls::Floating => "floating",
                        RecordingControls::Notch => "notch",
                    }))
                    .min_width(dp(200.))
                    .on_select(|v, _, cx| {
                        let controls = if v == "notch" {
                            RecordingControls::Notch
                        } else {
                            RecordingControls::Floating
                        };
                        update_meetings(cx, |m| m.recording_controls = controls);
                        // A recording in progress switches over now.
                        cx.defer(crate::windows::sync_recording_controls);
                    }),
            )
            .into_any_element(),
            SettingRow::new(
                "meetings-live-transcription",
                t("meetings.settings.liveTranscription.title"),
            )
            .description(t("meetings.settings.liveTranscription.description"))
            .control(toggle(
                "meetings-live-transcription-toggle",
                m.live_transcription,
                updating,
                |v, _, cx| update_meetings(cx, |m| m.live_transcription = v),
            ))
            .into_any_element(),
        ]);
        if m.live_transcription {
            // Any downloaded model; a chosen one deleted since still shows.
            let mut live_options = vec![DropdownOption::new(
                LIVE_SAME_MODEL,
                t("meetings.settings.liveTranscription.sameModel"),
            )];
            live_options.extend(
                models
                    .iter()
                    .filter(|mi| mi.is_downloaded || m.live_model.as_deref() == Some(&mi.id))
                    .map(|mi| DropdownOption::new(mi.id.clone(), model_name(mi))),
            );
            recording.push(
                SettingRow::new(
                    "meetings-live-model",
                    t("meetings.settings.liveTranscription.model"),
                )
                .description(t("meetings.settings.liveTranscription.modelDescription"))
                .inline_description()
                .control(
                    Dropdown::new("meetings-live-model-dropdown")
                        .options(live_options)
                        .selected(Some(
                            m.live_model
                                .clone()
                                .unwrap_or_else(|| LIVE_SAME_MODEL.to_string()),
                        ))
                        .min_width(dp(220.))
                        .on_select(|v, _, cx| {
                            let v = (v != LIVE_SAME_MODEL).then(|| v.to_string());
                            update_meetings(cx, |m| m.live_model = v);
                        }),
                )
                .into_any_element(),
            );
        }
        recording.extend([
            SettingRow::new("meetings-retention", t("meetings.settings.retention.title"))
                .description(t("meetings.settings.retention.description"))
                .control(
                    Dropdown::new("meetings-retention-dropdown")
                        .options(retention_options)
                        .selected(Some(retention_value(m.audio_retention_days)))
                        .min_width(dp(160.))
                        .on_select(|v, _, cx| {
                            let days = parse_retention(v);
                            update_meetings(cx, |m| m.audio_retention_days = days);
                        }),
                )
                .into_any_element(),
            SettingRow::new(
                "meetings-calendar-notifications",
                t("meetings.settings.calendarNotifications.title"),
            )
            .description(t("meetings.settings.calendarNotifications.description"))
            .control(toggle(
                "meetings-calendar-notifications-toggle",
                m.calendar_notifications,
                updating,
                |v, _, cx| update_meetings(cx, |m| m.calendar_notifications = v),
            ))
            .into_any_element(),
            SettingRow::new(
                "meetings-call-suggestions",
                t("meetings.settings.callSuggestions.title"),
            )
            .description(t("meetings.settings.callSuggestions.description"))
            .control(toggle(
                "meetings-call-suggestions-toggle",
                m.suggest_notes_on_calls,
                updating,
                |v, _, cx| update_meetings(cx, |m| m.suggest_notes_on_calls = v),
            ))
            .into_any_element(),
            SettingRow::new(
                "meetings-calendar-titles",
                t("meetings.settings.calendarTitles.title"),
            )
            .description(t("meetings.settings.calendarTitles.description"))
            .control(toggle(
                "meetings-calendar-titles-toggle",
                m.prefer_calendar_titles,
                updating,
                |v, _, cx| update_meetings(cx, |m| m.prefer_calendar_titles = v),
            ))
            .into_any_element(),
            SettingRow::new(
                "meetings-reminder-interval",
                t("meetings.settings.reminderInterval.title"),
            )
            .description(t("meetings.settings.reminderInterval.description"))
            .control(
                Dropdown::new("meetings-reminder-dropdown")
                    .options(reminder_options)
                    .selected(Some(m.reminder_interval_minutes.to_string()))
                    .min_width(dp(160.))
                    .on_select(|v, _, cx| {
                        if let Ok(n) = v.parse::<u32>() {
                            update_meetings(cx, |m| m.reminder_interval_minutes = n);
                        }
                    }),
            )
            .into_any_element(),
        ]);
        recording.extend(self.render_speaker_model(cx));
        recording.push(self.render_turn_models(cx));
        recording.extend(self.render_separation_model(cx));
        recording.push(self.render_deepgram_key(cx));

        // -- other apps (MCP) -----------------------------------------------
        let mut mcp = vec![
            SettingRow::new("meetings-mcp", t("meetings.settings.mcp.title"))
                .description(t("meetings.settings.mcp.description"))
                .control(toggle(
                    "meetings-mcp-toggle",
                    m.mcp_enabled,
                    updating,
                    |v, _, cx| update_meetings(cx, |m| m.mcp_enabled = v),
                ))
                .into_any_element(),
        ];
        if m.mcp_enabled {
            let running = cx
                .try_global::<crate::windows::ShellGlobal>()
                .is_some_and(|g| guidy_core::mcp::is_running(&g.app));
            if running || updating {
                let exe = std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "guidy".into());
                mcp.push(copy_row(
                    "meetings-mcp-url",
                    t("meetings.settings.mcp.url"),
                    guidy_core::mcp::endpoint_url(m.mcp_port),
                    cx,
                ));
                mcp.push(copy_row(
                    "meetings-mcp-stdio",
                    t("meetings.settings.mcp.stdio"),
                    format!("\"{exe}\" --mcp-stdio"),
                    cx,
                ));
            } else {
                mcp.push(
                    div()
                        .w_full()
                        .px(design::SECTION_X)
                        .pb(dp(10.))
                        .text_size(dp(12.))
                        .text_color(cx.theme().danger)
                        .child(t_args(
                            "meetings.settings.mcp.failed",
                            &[("port", &m.mcp_port.to_string())],
                        ))
                        .into_any_element(),
                );
            }
        }

        // -- advanced -------------------------------------------------------
        let advanced_toggle = Button::new("meetings-advanced-toggle")
            .ghost()
            .pill()
            .small()
            .icon(icon(if self.advanced_open {
                "chevron-down"
            } else {
                "chevron-right"
            }))
            .label(if self.advanced_open {
                t("meetings.settings.advanced.hide")
            } else {
                t("meetings.settings.advanced.show")
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.advanced_open = !this.advanced_open;
                cx.notify();
            }));

        v_flex()
            .id("meetings-settings")
            .w_full()
            .gap_6()
            .child(group(Some(t("meetings.settings.general")), general, cx))
            .child(group(Some(t("meetings.settings.you")), you, cx))
            .child(group(Some(t("meetings.settings.recording")), recording, cx))
            .child(group(Some(t("meetings.settings.mcp.group")), mcp, cx))
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(advanced_toggle)
                    .when(self.advanced_open, |d| {
                        d.child(group(
                            Some(t("meetings.settings.advanced.title")),
                            vec![
                                self.slider_row(
                                    "meetings-match-threshold",
                                    "meetings.settings.advanced.matchThreshold",
                                    &self.match_slider,
                                    cx,
                                ),
                                self.slider_row(
                                    "meetings-suggest-threshold",
                                    "meetings.settings.advanced.suggestThreshold",
                                    &self.suggest_slider,
                                    cx,
                                ),
                            ],
                            cx,
                        ))
                    }),
            )
            // ---- G2: AI + Linear settings (ai_settings.rs) ----------------
            .child(self.ai.clone())
            .test_support()
    }
}

/// A labelled value in mono with a Copy button (the MCP connection details).
fn copy_row(id: &'static str, title: SharedString, value: String, cx: &App) -> AnyElement {
    let muted = design::tone(cx).text_muted;
    SettingRow::new(id, title)
        .stacked()
        .control(
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .font_family(crate::theme::MONO_FONT)
                        .text_size(dp(12.))
                        .text_color(muted)
                        .child(value.clone()),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-copy")))
                        .secondary()
                        .pill()
                        .xsmall()
                        .label(t("common.copy"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                        }),
                ),
        )
        .into_any_element()
}
