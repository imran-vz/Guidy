//! Meeting settings, laid out like macOS System Settings: a category list
//! on the left ([`SettingsCategory`]) and one pane on the right. Each pane
//! is a single scroll region, except AI Summaries, whose model list is the
//! pane's only flexible scroll region under a pinned provider form (see
//! `ai_settings.rs`). Below ~860 pt the list collapses to a popup button
//! above the pane.
//!
//! Setup is the one place for permissions and required downloads; the
//! category list shows how many of them still need attention.
//!
//! Every change writes only the changed fields through
//! `update_setting("meetings", {"field": value})`; the backend merges the
//! patch into the stored object.

use super::ai_settings::{ActiveSummary, AiSection, AiSettingsView};
use super::emails::UserEmailsView;
use super::{
    local_model_choices, meeting_model_download, meeting_model_missing, meeting_settings,
    meetings_backend, model_missing_message, render_model_download, render_model_download_with,
    run, toast_error,
};
use crate::logic::meetings::{
    meetings_patch, non_empty, parse_retention, retention_value, MeetingsEvent,
    NOTIFICATIONS_SETTINGS_URL, PRIVACY_AUDIO_CAPTURE_URL, PRIVACY_CALENDARS_URL,
    PRIVACY_MICROPHONE_URL,
};
use crate::state::AppEvent;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::download_control::DownloadControl;
use crate::ui::kit::{
    group, icon, model_name, page, scroll_page, t, t_args, toggle, Dropdown, DropdownOption,
    LocalizedPlaceholder, SettingRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::downloads::format_bytes;
use guidy_core::managers::transcription::meeting::{
    configured_meeting_model_missing, DEFAULT_MEETING_MODEL_ID, DEFAULT_MEETING_MODEL_NAME,
};
use guidy_core::meetings::manager::PermissionReport;
use guidy_core::meetings::processing::speaker_model::{
    segmentation_download_id, separation_download_id, separation_model, speaker_download_id,
    SEGMENTATION_MODEL_NAME, SEGMENTATION_MODEL_SIZE,
};
use guidy_core::meetings::processing::{
    speaker_turn_models_bytes, SpeakerModelStatus, SPEAKER_MODEL_NAME, SPEAKER_MODEL_SIZE,
    SPEAKER_MODEL_TRAINED_ON,
};
use guidy_core::meetings::settings::{MeetingSettings, RecordingControls, RETENTION_CHOICES};
use guidy_core::meetings::types::{DiarizationOptions, SpeakerModelId};

const REMINDER_CHOICES: [u32; 5] = [5, 10, 15, 20, 30];

/// The width of every select and slider in the settings panes, so their
/// left edges line up row to row.
const CONTROL_W: Pixels = dp(200.);

/// A pane of the meeting settings, in list order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    /// Permissions and required downloads.
    Setup,
    /// Meeting notes on/off, audio retention.
    General,
    /// Name, email addresses, voice.
    You,
    /// Recording controls, calendar notifications and suggestions, reminders.
    Recording,
    /// Meeting and live transcription models.
    Transcription,
    /// Speaker separation model, cloud speaker redo, match thresholds.
    Speakers,
    /// Where summaries run and which model makes them.
    Ai,
    /// The Linear API key.
    Linear,
    /// Other apps (MCP server).
    Integrations,
}

impl SettingsCategory {
    pub const ALL: [Self; 9] = [
        Self::Setup,
        Self::General,
        Self::You,
        Self::Recording,
        Self::Transcription,
        Self::Speakers,
        Self::Ai,
        Self::Linear,
        Self::Integrations,
    ];

    /// Stable id, used in element ids (`meetings-settings-category-<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::General => "general",
            Self::You => "you",
            Self::Recording => "recording",
            Self::Transcription => "transcription",
            Self::Speakers => "speakers",
            Self::Ai => "ai",
            Self::Linear => "linear",
            Self::Integrations => "integrations",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    fn label(self) -> SharedString {
        t(&format!("meetings.settings.categories.{}", self.id()))
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Setup => "list-checks",
            Self::General => "settings",
            Self::You => "circle-user",
            Self::Recording => "mic",
            Self::Transcription => "captions",
            Self::Speakers => "users",
            Self::Ai => "sparkles",
            Self::Linear => "square-kanban",
            Self::Integrations => "plug",
        }
    }

    /// The category `delta` rows away, clamped to the list.
    fn step(self, delta: isize) -> Self {
        let i = Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as isize;
        Self::ALL[(i + delta).clamp(0, Self::ALL.len() as isize - 1) as usize]
    }
}

/// Requests from the settings to the page around them.
pub enum MeetingSettingsEvent {
    /// Show People with the user's own voice selected.
    OpenYourVoice,
}

impl EventEmitter<MeetingSettingsEvent> for MeetingSettingsView {}

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
    /// Recording needs it (calendar and notifications only add to it).
    fn required(self) -> bool {
        matches!(self, Self::Microphone | Self::SystemAudio)
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
    /// The list | detail split the Meetings page shares with this tab
    /// (its own when shown alone).
    split: Option<Entity<ResizableState>>,
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
    /// Deepgram key for "Redo speakers" in the cloud (never shown back).
    deepgram_key: Entity<InputState>,
    has_deepgram_key: bool,
    /// The user's own voice is recorded (`None`: not loaded yet).
    voice_recorded: Option<bool>,
    ai: Entity<AiSettingsView>,
    category: SettingsCategory,
    /// The category list (↑/↓ while focused).
    list_focus: FocusHandle,
    /// The shown pane's scroll; back to the top on another category.
    pane_scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl MeetingSettingsView {
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
        let current = meeting_settings(cx);
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .localized_placeholder("meetings.settings.userName.placeholder", window, cx)
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
                .localized_placeholder("meetings.settings.deepgram.placeholder", window, cx)
        });
        let match_slider = slider(current.speaker_match_threshold, cx);
        let suggest_slider = slider(current.speaker_suggest_threshold, cx);
        let ai = cx.new(|cx| AiSettingsView::new(window, cx));
        let subs = vec![
            // The Setup count reads the summaries status.
            cx.observe(&ai, |_, _, cx| cx.notify()),
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
                AppEvent::Meetings(MeetingsEvent::DownloadsChanged(_)) => cx.notify(),
                AppEvent::Meetings(MeetingsEvent::PeopleChanged) => this.reload_voice(cx),
                _ => {}
            }),
        ];
        let mut this = Self {
            split: None,
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
            deepgram_key,
            has_deepgram_key: false,
            voice_recorded: None,
            ai,
            category: SettingsCategory::General,
            list_focus: cx.focus_handle().tab_stop(true),
            pane_scroll: ScrollHandle::new(),
            _subs: subs,
        };
        this.reload_status(cx);
        this.reload_voice(cx);
        this
    }

    /// The pane shown.
    pub fn category(&self) -> SettingsCategory {
        self.category
    }

    /// Show `category`'s pane (deep links, the category list, ↑/↓).
    pub fn show_category(&mut self, category: SettingsCategory, cx: &mut Context<Self>) {
        if self.category != category {
            self.category = category;
            self.pane_scroll.set_offset(point(px(0.), px(0.)));
        }
        let section = match category {
            SettingsCategory::Linear => AiSection::Linear,
            _ => AiSection::Summaries,
        };
        self.ai.update(cx, |ai, cx| ai.show(section, cx));
        // Grants may have changed in System Settings meanwhile.
        if category == SettingsCategory::Setup {
            self.reload_status(cx);
        }
        cx.notify();
    }

    /// The AI + Linear settings (shown in the AI Summaries and Linear panes).
    pub fn ai(&self) -> &Entity<AiSettingsView> {
        &self.ai
    }

    fn reload_voice(&mut self, cx: &mut Context<Self>) {
        run(
            cx,
            |b| b.user_voice(),
            |this: &mut Self, r, _| {
                if let Ok(v) = r {
                    this.voice_recorded = Some(v.recorded_at.is_some());
                }
            },
        );
    }

    /// Required things still missing: microphone and system audio access,
    /// the meeting transcription model, the speaker model, and a usable
    /// summaries model. Calendar, notifications and the speaker-turn models
    /// are optional.
    pub fn setup_issues(&self, cx: &App) -> usize {
        let permissions = self.permissions.as_ref().map_or(0, |p| {
            usize::from(!p.microphone_granted) + usize::from(!p.system_audio_granted)
        });
        let meeting_model = usize::from(meeting_model_missing(cx).is_some());
        let speaker = usize::from(matches!(
            self.speaker_model,
            Some(SpeakerModelStatus::Missing | SpeakerModelStatus::Failed { .. })
        ));
        let summaries = usize::from(matches!(
            self.ai.read(cx).active_summary(cx),
            ActiveSummary::NotReady(ref why) if !why.is_empty()
        ));
        permissions + meeting_model + speaker + summaries
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
                            .text_size(design::TEXT_SECONDARY)
                            .text_color(theme.muted_foreground)
                            .child(icon("check").xsmall().text_color(theme.success))
                            .child(t("meetings.settings.deepgram.saved")),
                    )
                    .child(
                        Button::new("meetings-deepgram-remove")
                            .ghost()
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

    /// One row per permission: whether it's granted, whether meetings need
    /// it, and the ways to fix a denial.
    fn render_permissions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(p) = self.permissions.clone() else {
            return vec![loading_row("meetings-permissions-loading", cx)];
        };
        let theme = cx.theme().clone();
        [
            Permission::Microphone,
            Permission::SystemAudio,
            Permission::Calendar,
            Permission::Notifications,
        ]
        .into_iter()
        .map(|perm| {
            let granted = perm.granted(&p);
            let need = t(if perm.required() {
                "meetings.settings.permissions.required"
            } else {
                "meetings.settings.permissions.optional"
            });
            let state = t(if granted {
                "meetings.settings.permissions.granted"
            } else {
                "meetings.settings.permissions.denied"
            });
            let control: AnyElement = if granted {
                status_mark(
                    SharedString::from(format!("meetings-permission-granted-{}", perm.id())),
                    state.clone(),
                    true,
                    cx,
                )
            } else {
                h_flex()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!(
                            "meetings-permission-request-{}",
                            perm.id()
                        )))
                        .outline()
                        .small()
                        .label(t("meetings.settings.permissions.requestAgain"))
                        .disabled(self.requesting)
                        .on_click(cx.listener(|this, _, _, cx| this.request_permissions(cx))),
                    )
                    .when_some(perm.pane(), |d, url| {
                        d.child(
                            Button::new(SharedString::from(format!(
                                "meetings-permission-open-{}",
                                perm.id()
                            )))
                            .ghost()
                            .small()
                            .icon(icon("external-link"))
                            .label(t("meetings.settings.permissions.openSettings"))
                            .on_click(cx.listener(move |this, _, _, cx| this.open_pane(url, cx))),
                        )
                    })
                    .into_any_element()
            };
            let needed = !granted && perm.required();
            SettingRow::new(
                SharedString::from(format!("meetings-permission-{}", perm.id())),
                t(perm.key()),
            )
            .description(if granted {
                need
            } else {
                format!("{need} · {state}").into()
            })
            .control(
                h_flex()
                    .gap_2()
                    .when(needed, |d| {
                        d.child(div().size(dp(6.)).rounded_full().bg(theme.warning))
                    })
                    .child(control),
            )
            .into_any_element()
        })
        .collect()
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
                    .text_size(crate::ui::design::TEXT_SECONDARY)
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
                    .outline()
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
                            .min_width(CONTROL_W)
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

    /// Optional TitaNet + segmentation models. "Not now" on the meetings
    /// page hides the prompt; this row is how to start the download later
    /// (one control per model still missing).
    fn render_turn_models(&self, cx: &mut Context<Self>) -> AnyElement {
        let all_installed = meetings_backend(cx).speaker_turn_models_installed();
        let size = format_bytes(speaker_turn_models_bytes());
        let titanet = separation_model(SpeakerModelId::TitanetLarge);
        let parts = [
            (
                separation_download_id(titanet.id),
                titanet.name,
                titanet.size,
            ),
            (
                segmentation_download_id().to_string(),
                SEGMENTATION_MODEL_NAME,
                SEGMENTATION_MODEL_SIZE,
            ),
        ];
        let tone = design::tone(cx);
        let control: AnyElement = if all_installed {
            status_mark(
                "meetings-turn-models-ready",
                t("meetings.settings.speakerModel.ready"),
                true,
                cx,
            )
        } else {
            let state = AppState::global(cx).read(cx);
            let lines: Vec<AnyElement> = parts
                .into_iter()
                .filter_map(|(id, name, size)| {
                    let known = state.models.other_model(&id);
                    let installed = known.is_some_and(|m| m.installed);
                    let busy = state
                        .models
                        .download(&id)
                        .is_some_and(|d| d.state.is_active());
                    (!installed || busy).then(|| {
                        let control = DownloadControl::new(id)
                            .size(size)
                            .partial(known.map_or(0, |m| m.partial_bytes))
                            .on_start(|cx| {
                                update_meetings(cx, |m| {
                                    m.speaker_turn_models_consent = true;
                                    m.speaker_turn_prompt_dismissed = true;
                                })
                            });
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                div()
                                    .text_size(design::TEXT_SECONDARY)
                                    .text_color(tone.text_muted)
                                    .child(name),
                            )
                            .child(control)
                            .into_any_element()
                    })
                })
                .collect();
            v_flex()
                .items_end()
                .gap(dp(4.))
                .children(lines)
                .into_any_element()
        };
        SettingRow::new(
            "meetings-turn-models",
            t("meetings.settings.turnModels.title"),
        )
        // "Until these are downloaded…" only while they aren't.
        .description(if all_installed {
            t("meetings.settings.turnModels.descriptionReady")
        } else {
            t_args(
                "meetings.settings.turnModels.description",
                &[("size", &size)],
            )
        })
        .control(control)
        .into_any_element()
    }

    /// The standard speaker model (required): its download goes through
    /// the shared queue; the meetings status says when it is usable.
    fn render_speaker_model(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.speaker_model.clone()?;
        let size = format_bytes(SPEAKER_MODEL_SIZE);
        let id = speaker_download_id();
        let partial = AppState::global(cx)
            .read(cx)
            .models
            .other_model(id)
            .map_or(0, |m| m.partial_bytes);
        let control = DownloadControl::new(id)
            .installed(status == SpeakerModelStatus::Ready)
            .size(SPEAKER_MODEL_SIZE)
            .partial(partial);
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
                    .w(CONTROL_W)
                    .child(crate::ui::kit::tracked_slider(Slider::new(slider), cx).flex_1())
                    .child(
                        div()
                            .w(dp(36.))
                            .text_right()
                            .text_size(design::TEXT_SECONDARY)
                            .font_family(crate::theme::MONO_FONT)
                            .child(format!("{value:.2}")),
                    ),
            )
            .into_any_element()
    }
}

// -- panes ----------------------------------------------------------------------

impl MeetingSettingsView {
    /// Permissions, required downloads, and whether summaries are ready:
    /// everything meeting notes need, in one place.
    fn render_setup(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let issues = self.setup_issues(cx);
        let tone = design::tone(cx);
        let summary = crate::ui::kit::lede(
            if issues == 0 {
                t("meetings.settings.setup.ready")
            } else {
                t_args(
                    "meetings.settings.setup.missing",
                    &[("count", &issues.to_string())],
                )
            },
            cx,
        )
        .id("meetings-setup-summary")
        .text_color(tone.text_muted)
        .test_support()
        .into_any_element();

        let mut downloads: Vec<AnyElement> = Vec::new();
        downloads.push(match meeting_model_missing(cx) {
            Some(missing) => SettingRow::new(
                "meetings-setup-meeting-model",
                t("meetings.settings.setup.meetingModel"),
            )
            .description(model_missing_message(&missing))
            .when(missing.known, |row| {
                row.control(meeting_model_download(&missing.model_id, cx))
            })
            .into_any_element(),
            None => {
                let name = local_model_choices(cx)
                    .into_iter()
                    .find(|c| c.current)
                    .map(|c| c.name)
                    .unwrap_or_default();
                SettingRow::new(
                    "meetings-setup-meeting-model",
                    t("meetings.settings.setup.meetingModel"),
                )
                .description(name)
                .control(status_mark(
                    "meetings-setup-meeting-model-ready",
                    t("meetings.settings.setup.downloaded"),
                    true,
                    cx,
                ))
                .into_any_element()
            }
        });
        downloads.extend(self.render_speaker_model(cx));
        downloads.push(self.render_turn_models(cx));

        let (summary_title, ready) = match self.ai.read(cx).active_summary(cx) {
            ActiveSummary::Ready { model, via } => (format!("{model} · {via}"), true),
            ActiveSummary::NotReady(why) => (why, false),
        };
        let summaries = SettingRow::new(
            "meetings-setup-summaries",
            t("meetings.settings.setup.summaries"),
        )
        .description(summary_title)
        .control(
            h_flex()
                .gap_2()
                .when(!ready, |d| {
                    d.child(div().size(dp(6.)).rounded_full().bg(cx.theme().warning))
                })
                .child(
                    Button::new("meetings-setup-summaries-open")
                        .outline()
                        .small()
                        .label(t(if ready {
                            "meetings.settings.setup.change"
                        } else {
                            "meetings.settings.setup.setUp"
                        }))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_category(SettingsCategory::Ai, cx)
                        })),
                ),
        )
        .into_any_element();

        vec![
            summary,
            group(
                Some(t("meetings.settings.permissions.title")),
                self.render_permissions(cx),
                cx,
            )
            .id("meetings-permissions")
            .test_support()
            .into_any_element(),
            group(Some(t("meetings.settings.setup.downloads")), downloads, cx).into_any_element(),
            group(
                Some(t("meetings.settings.setup.summariesGroup")),
                vec![summaries],
                cx,
            )
            .into_any_element(),
        ]
    }

    fn render_general(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let m = meeting_settings(cx);
        let updating = self.updating(cx);
        let this = cx.entity().downgrade();
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
        vec![group(
            None,
            vec![
                SettingRow::new("meetings-enabled", t("meetings.settings.enabled.title"))
                    .description(t("meetings.settings.enabled.description"))
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
                SettingRow::new("meetings-retention", t("meetings.settings.retention.title"))
                    .description(t("meetings.settings.retention.description"))
                    .control(
                        Dropdown::new("meetings-retention-dropdown")
                            .options(retention_options)
                            .selected(Some(retention_value(m.audio_retention_days)))
                            .min_width(CONTROL_W)
                            .on_select(|v, _, cx| {
                                let days = parse_retention(v);
                                update_meetings(cx, |m| m.audio_retention_days = days);
                            }),
                    )
                    .into_any_element(),
            ],
            cx,
        )
        .into_any_element()]
    }

    fn render_you(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let voice = match self.voice_recorded {
            Some(true) => t("meetings.settings.voice.recorded"),
            Some(false) => t("meetings.settings.voice.notRecorded"),
            None => t("common.loading"),
        };
        vec![group(
            None,
            vec![
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
                .stacked()
                .control(self.emails.clone())
                .into_any_element(),
                SettingRow::new("meetings-user-voice", t("meetings.settings.voice.title"))
                    .description(voice)
                    .control(
                        Button::new("meetings-user-voice-open")
                            .outline()
                            .small()
                            .label(t("meetings.settings.voice.open"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(MeetingSettingsEvent::OpenYourVoice)
                            })),
                    )
                    .into_any_element(),
            ],
            cx,
        )
        .into_any_element()]
    }

    fn render_recording(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let m = meeting_settings(cx);
        let updating = self.updating(cx);
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
        let controls_options = vec![
            DropdownOption::new(
                "floating",
                t("meetings.settings.recordingControls.floating"),
            ),
            DropdownOption::new("notch", t("meetings.settings.recordingControls.notch")),
        ];
        let while_recording = vec![
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
                    .min_width(CONTROL_W)
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
                "meetings-reminder-interval",
                t("meetings.settings.reminderInterval.title"),
            )
            .description(t("meetings.settings.reminderInterval.description"))
            .control(
                Dropdown::new("meetings-reminder-dropdown")
                    .options(reminder_options)
                    .selected(Some(m.reminder_interval_minutes.to_string()))
                    .min_width(CONTROL_W)
                    .on_select(|v, _, cx| {
                        if let Ok(n) = v.parse::<u32>() {
                            update_meetings(cx, |m| m.reminder_interval_minutes = n);
                        }
                    }),
            )
            .into_any_element(),
        ];
        let calendar = vec![
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
        ];
        vec![
            group(Some(t("meetings.settings.recording")), while_recording, cx).into_any_element(),
            group(Some(t("meetings.settings.calendarGroup")), calendar, cx).into_any_element(),
        ]
    }

    fn render_transcription(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let m = meeting_settings(cx);
        let updating = self.updating(cx);
        let state = AppState::global(cx);
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
        // dictation model until it is. Nothing usable at all: offered here
        // next to the choice (and in Setup).
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
        let mut meeting =
            vec![
                SettingRow::new("meetings-model", t("meetings.settings.meetingModel.title"))
                    .description(match &dictation_name {
                        Some(name) => t_args(
                            "meetings.settings.meetingModel.usesDictation",
                            &[("name", name)],
                        ),
                        None => t("meetings.settings.meetingModel.noDictation"),
                    })
                    .control(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(design::TEXT_SECONDARY)
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
            meeting.push(
                SettingRow::new(
                    "meetings-model-choice",
                    t("meetings.settings.meetingModel.choice"),
                )
                .description(t("meetings.settings.meetingModel.choiceDescription"))
                .control(
                    Dropdown::new("meetings-model-dropdown")
                        .options(model_options)
                        .selected(Some(selected_model))
                        .min_width(CONTROL_W)
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
        meeting.extend(model_notice.map(|b| {
            div()
                .w_full()
                .px(design::SECTION_X)
                .py(design::ROW_Y)
                .child(b)
                .into_any_element()
        }));

        let mut live = vec![SettingRow::new(
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
        .into_any_element()];
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
            live.push(
                SettingRow::new(
                    "meetings-live-model",
                    t("meetings.settings.liveTranscription.model"),
                )
                .description(t("meetings.settings.liveTranscription.modelDescription"))
                .control(
                    Dropdown::new("meetings-live-model-dropdown")
                        .options(live_options)
                        .selected(Some(
                            m.live_model
                                .clone()
                                .unwrap_or_else(|| LIVE_SAME_MODEL.to_string()),
                        ))
                        .min_width(CONTROL_W)
                        .on_select(|v, _, cx| {
                            let v = (v != LIVE_SAME_MODEL).then(|| v.to_string());
                            update_meetings(cx, |m| m.live_model = v);
                        }),
                )
                .into_any_element(),
            );
        }
        vec![
            group(
                Some(t("meetings.settings.transcription.afterMeeting")),
                meeting,
                cx,
            )
            .into_any_element(),
            group(Some(t("meetings.settings.transcription.live")), live, cx).into_any_element(),
        ]
    }

    fn render_speakers(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut models: Vec<AnyElement> = Vec::new();
        models.extend(self.render_separation_model(cx));
        models.push(self.render_deepgram_key(cx));
        vec![
            group(
                Some(t("meetings.settings.speakers.recognition")),
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
            )
            .into_any_element(),
            group(Some(t("meetings.settings.speakers.separation")), models, cx).into_any_element(),
        ]
    }

    fn render_integrations(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let m = meeting_settings(cx);
        let updating = self.updating(cx);
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
                    h_flex()
                        .id("meetings-mcp-failed")
                        .w_full()
                        .gap_1()
                        .px(design::SECTION_X)
                        .py(design::ROW_Y)
                        .text_size(design::TEXT_SECONDARY)
                        .text_color(cx.theme().danger)
                        .child(icon("circle-alert").xsmall())
                        .child(t_args(
                            "meetings.settings.mcp.failed",
                            &[("port", &m.mcp_port.to_string())],
                        ))
                        .test_support()
                        .into_any_element(),
                );
            }
        }
        vec![group(Some(t("meetings.settings.mcp.group")), mcp, cx).into_any_element()]
    }

    fn updating(&self, cx: &App) -> bool {
        AppState::global(cx)
            .read(cx)
            .settings
            .is_updating("meetings")
    }

    /// The source list of categories; ↑/↓ move the selection while it has
    /// focus, like the window sidebar.
    fn render_categories(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let warning = cx.theme().warning;
        let focused = self.list_focus.is_focused(window);
        let selection = if focused {
            tone.selection_focused
        } else {
            tone.selection
        };
        let issues = self.setup_issues(cx);
        v_flex()
            .id("meetings-settings-categories")
            .track_focus(&self.list_focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let delta = match ev.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                cx.stop_propagation();
                let next = this.category.step(delta);
                this.show_category(next, cx);
            }))
            // Sized by the split the Meetings tabs share.
            .size_full()
            .min_h_0()
            .px(dp(8.))
            .py(dp(8.))
            .gap(px(1.))
            .overflow_y_scroll()
            .children(SettingsCategory::ALL.into_iter().map(|c| {
                let active = c == self.category;
                h_flex()
                    .id(SharedString::from(format!(
                        "meetings-settings-category-{}",
                        c.id()
                    )))
                    .aria_label(c.label())
                    .flex_none()
                    .gap(dp(8.))
                    .px(dp(8.))
                    .h(design::SIDEBAR_ITEM_H)
                    .rounded(design::RADIUS_CHIP)
                    .text_size(design::TEXT_BODY)
                    .text_color(tone.text)
                    .when(active, |d| d.bg(selection))
                    .child(icon(c.icon()).small().text_color(if active {
                        tone.accent_text
                    } else {
                        tone.text_muted
                    }))
                    .child(div().flex_1().min_w_0().truncate().child(c.label()))
                    .when(c == SettingsCategory::Setup && issues > 0, |d| {
                        d.child(
                            h_flex()
                                .id("meetings-settings-setup-badge")
                                .flex_none()
                                .gap(dp(4.))
                                .child(div().size(dp(6.)).rounded_full().bg(warning))
                                .child(
                                    design::mono(issues.to_string())
                                        .text_size(design::TEXT_SECONDARY)
                                        .text_color(tone.text_muted),
                                )
                                .test_support(),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.list_focus, cx);
                        this.show_category(c, cx)
                    }))
                    .test_support()
            }))
            .test_support()
    }

    /// The category list as a popup button, for narrow windows.
    fn render_category_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let issues = self.setup_issues(cx);
        let options = SettingsCategory::ALL
            .into_iter()
            .map(|c| {
                let label = if c == SettingsCategory::Setup && issues > 0 {
                    format!("{} · {issues}", c.label()).into()
                } else {
                    c.label()
                };
                DropdownOption::new(c.id(), label)
            })
            .collect();
        let weak = cx.entity().downgrade();
        h_flex()
            .flex_none()
            .w_full()
            .px(design::PAGE_X)
            .pt(dp(10.))
            .pb(dp(10.))
            .border_b_1()
            .border_color(design::tone(cx).hairline)
            .child(
                Dropdown::new("meetings-settings-category-menu")
                    .options(options)
                    .selected(Some(self.category.id()))
                    .min_width(dp(220.))
                    .on_select(move |v, _, cx| {
                        if let Some(c) = SettingsCategory::from_id(v) {
                            weak.update(cx, |this, cx| this.show_category(c, cx)).ok();
                        }
                    }),
            )
    }

    fn render_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let groups = match self.category {
            // The AI and Linear settings own their pane layout.
            SettingsCategory::Ai | SettingsCategory::Linear => {
                return self.ai.clone().into_any_element()
            }
            SettingsCategory::Setup => self.render_setup(cx),
            SettingsCategory::General => self.render_general(cx),
            SettingsCategory::You => self.render_you(cx),
            SettingsCategory::Recording => self.render_recording(cx),
            SettingsCategory::Transcription => self.render_transcription(cx),
            SettingsCategory::Speakers => self.render_speakers(cx),
            SettingsCategory::Integrations => self.render_integrations(cx),
        };
        scroll_page(
            SharedString::from(format!("meetings-settings-pane-{}", self.category.id())),
            page().children(groups),
        )
        .track_scroll(&self.pane_scroll)
        .test_support()
        .into_any_element()
    }
}

impl Render for MeetingSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Narrow: the category list becomes a popup button above the pane.
        let compact = super::is_narrow(window);
        let pane = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.render_pane(cx));
        if compact {
            v_flex()
                .id("meetings-settings")
                .size_full()
                .min_h_0()
                .min_w_0()
                .child(self.render_category_menu(cx))
                .child(pane)
                .test_support()
                .into_any_element()
        } else {
            let split = self.split(cx);
            h_flex()
                .id("meetings-settings")
                .size_full()
                .min_h_0()
                .min_w_0()
                .items_stretch()
                .child(super::split_view(
                    "meetings-settings-split",
                    &split,
                    self.render_categories(window, cx),
                    pane.size_full(),
                    cx,
                ))
                .test_support()
                .into_any_element()
        }
    }
}

/// A row stand-in while a status loads.
fn loading_row(id: &'static str, cx: &App) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .px(design::SECTION_X)
        .py(design::SECTION_Y)
        .text_size(design::TEXT_SECONDARY)
        .text_color(design::tone(cx).text_muted)
        .child(t("common.loading"))
        .test_support()
        .into_any_element()
}

/// "✓ Granted" / "✓ Downloaded": a quiet ready state for a row's control.
fn status_mark(id: impl Into<ElementId>, label: SharedString, ok: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .gap_1()
        .text_size(design::TEXT_SECONDARY)
        .text_color(theme.muted_foreground)
        .child(
            icon(if ok { "check" } else { "x" })
                .xsmall()
                .text_color(if ok { theme.success } else { theme.danger }),
        )
        .child(label)
        .test_support()
        .into_any_element()
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
                        .xsmall()
                        .label(t("common.copy"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                        }),
                ),
        )
        .into_any_element()
}

/// UI tests (GPUI test platform, fake backends).
#[cfg(test)]
mod ui {
    use super::{MeetingSettingsView, SettingsCategory};
    use crate::meetings_backend::fake::FakeMeetingsBackend;
    use crate::ui::pages::meetings::tickets::ui::setup;
    use crate::ui_tests::{exists, open, with_window};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        px, size, AppContext as _, Bounds, Point, TestAppContext, WindowBounds, WindowOptions,
    };

    fn category(
        cx: &mut TestAppContext,
        view: &gpui_kit::Entity<MeetingSettingsView>,
    ) -> SettingsCategory {
        view.read_with(cx, |v, _| v.category())
    }

    #[gpui_kit::test]
    fn categories_switch_panes_by_click_deep_link_and_keyboard(cx: &mut TestAppContext) {
        setup(cx, FakeMeetingsBackend::new());
        let (w, view) = open(cx, MeetingSettingsView::new);
        assert!(exists(cx, w, "meetings-settings-categories"));
        assert!(exists(cx, w, "meetings-settings-pane-general"));
        assert!(exists(cx, w, "meetings-enabled"));
        assert!(!exists(cx, w, "meetings-linear-key"));

        // A deep link lands on its pane, not at the top of a long form.
        view.update(cx, |v, cx| v.show_category(SettingsCategory::Linear, cx));
        cx.run_until_parked();
        assert!(exists(cx, w, "meetings-linear-key"));
        assert!(!exists(cx, w, "meetings-enabled"));

        with_window(cx, w, |window, cx| {
            window.click("meetings-settings-category-ai", cx)
        });
        assert_eq!(category(cx, &view), SettingsCategory::Ai);
        assert!(exists(cx, w, "meetings-llm-active"));
        assert!(!exists(cx, w, "meetings-linear-key"));

        // The clicked list has focus: ↑/↓ move through it.
        with_window(cx, w, |window, cx| window.press("down", cx));
        assert_eq!(category(cx, &view), SettingsCategory::Linear);
        with_window(cx, w, |window, cx| window.press("up", cx));
        with_window(cx, w, |window, cx| window.press("up", cx));
        assert_eq!(category(cx, &view), SettingsCategory::Speakers);
        assert!(exists(cx, w, "meetings-match-threshold"));
    }

    #[gpui_kit::test]
    fn setup_counts_what_is_missing(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        // Nothing granted (the fake's default).
        setup(cx, mb.clone());
        let (w, view) = open(cx, MeetingSettingsView::new);
        let issues = view.read_with(cx, |v, cx| v.setup_issues(cx));
        assert!(issues >= 2, "microphone and system audio: {issues}");
        assert!(exists(cx, w, "meetings-settings-setup-badge"));
        with_window(cx, w, |window, cx| {
            window.click("meetings-settings-category-setup", cx)
        });
        assert!(exists(cx, w, "meetings-permission-microphone"));
        assert!(exists(cx, w, "meetings-setup-summaries"));
        // Summaries' "Set Up…" goes to the AI pane.
        with_window(cx, w, |window, cx| {
            window.click("meetings-setup-summaries-open", cx)
        });
        assert_eq!(category(cx, &view), SettingsCategory::Ai);

        mb.data.lock().permissions = guidy_core::meetings::manager::PermissionReport {
            calendar_granted: true,
            system_audio_granted: true,
            microphone_granted: true,
            notifications_granted: true,
        };
        view.update(cx, |v, cx| v.reload_status(cx));
        cx.run_until_parked();
        let after = view.read_with(cx, |v, cx| v.setup_issues(cx));
        assert_eq!(after, issues - 2);
    }

    /// In a narrow window the category list is a popup button.
    #[gpui_kit::test]
    fn narrow_windows_get_a_category_menu(cx: &mut TestAppContext) {
        setup(cx, FakeMeetingsBackend::new());
        let (w, _) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: Point::default(),
                            size: size(px(520.), px(440.)),
                        })),
                        ..Default::default()
                    },
                    cx,
                    |window, cx| cx.new(|cx| MeetingSettingsView::new(window, cx)),
                )
            })
            .unwrap();
        cx.run_until_parked();
        assert!(exists(cx, w, "meetings-settings-category-menu"));
        assert!(!exists(cx, w, "meetings-settings-categories"));
    }
}
