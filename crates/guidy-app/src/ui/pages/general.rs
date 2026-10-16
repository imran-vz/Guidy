//! General page: shortcuts, model language settings, sound.

use crate::logic::keyboard::OsType;
use crate::state::AppState;
use crate::ui::kit::{
    group, page, reset_button, t, t_args, toggle, Dropdown, DropdownOption, SettingRow, SliderRow,
};
use crate::ui::language_selector::LanguageSelector;
use crate::ui::shortcut_input::ShortcutInput;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::json;

pub struct GeneralPage {
    state: Entity<AppState>,
    transcribe_shortcut: Entity<ShortcutInput>,
    cancel_shortcut: Option<Entity<ShortcutInput>>,
    language: Entity<LanguageSelector>,
    volume: Entity<SliderRow>,
    /// (device the count was fetched for, channel count)
    channels: (Option<String>, u16),
    _subs: Vec<Subscription>,
}

impl GeneralPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe(&state, |this, _, cx| {
            this.refresh_channels(cx);
            cx.notify();
        })];
        let mut this = Self {
            state,
            transcribe_shortcut: cx.new(|cx| ShortcutInput::new("transcribe", window, cx)),
            // The cancel shortcut stays hidden on Linux (dynamic shortcut
            // instability).
            cancel_shortcut: (OsType::current() != OsType::Linux)
                .then(|| cx.new(|cx| ShortcutInput::new("cancel", window, cx))),
            language: cx.new(|cx| LanguageSelector::new(window, cx)),
            volume: cx.new(|cx| {
                SliderRow::new(
                    "audio_feedback_volume",
                    t("settings.sound.volume.title"),
                    t("settings.sound.volume.description"),
                    0.0,
                    1.0,
                    0.01,
                    |v| format!("{}%", (v * 100.0).round() as i32),
                    false,
                    cx,
                )
                .disabled_when(|s| !s.audio_feedback)
            }),
            channels: (None, 1),
            _subs: subs,
        };
        this.refresh_channels(cx);
        this
    }

    fn selected_microphone(&self, cx: &App) -> String {
        self.state
            .read(cx)
            .app_settings()
            .and_then(|s| s.selected_microphone.clone())
            .unwrap_or_else(|| "default".into())
    }

    /// ChannelSelector: fetch the channel count whenever the device changes.
    fn refresh_channels(&mut self, cx: &mut Context<Self>) {
        let device = self.selected_microphone(cx);
        if self.channels.0.as_deref() == Some(device.as_str()) {
            return;
        }
        self.channels = (Some(device.clone()), 1);
        let backend = self.state.read(cx).backend.clone();
        cx.spawn(async move |this, cx| {
            let name = if device == "Default" {
                "default".to_string()
            } else {
                device.clone()
            };
            let count = cx
                .background_spawn(async move { backend.get_microphone_channels(&name) })
                .await;
            this.update(cx, |this, cx| {
                if this.channels.0.as_deref() == Some(device.as_str()) {
                    if let Ok(n) = count {
                        this.channels.1 = n;
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn set(key: &'static str, value: serde_json::Value, cx: &mut App) {
        AppState::global(cx).update(cx, |s, cx| s.update_setting(key, value, cx));
    }
}

impl Render for GeneralPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let Some(s) = st.app_settings().cloned() else {
            return page().into_any_element();
        };
        let loading = !st.settings_loaded;

        // -- Shortcuts --------------------------------------------------------
        let activation = serde_json::to_value(s.shortcut_activation)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string));
        let activation_options = vec![
            DropdownOption::new(
                "hold_or_toggle",
                t("settings.general.shortcutActivation.options.holdOrToggle"),
            )
            .description(t(
                "settings.general.shortcutActivation.descriptions.hold_or_toggle",
            )),
            DropdownOption::new(
                "push_to_talk",
                t("settings.general.shortcutActivation.options.pushToTalk"),
            )
            .description(t(
                "settings.general.shortcutActivation.descriptions.push_to_talk",
            )),
            DropdownOption::new(
                "toggle",
                t("settings.general.shortcutActivation.options.toggle"),
            )
            .description(t("settings.general.shortcutActivation.descriptions.toggle")),
        ];
        let mut shortcut_rows = vec![
            self.transcribe_shortcut.clone().into_any_element(),
            SettingRow::new(
                "shortcut-activation",
                t("settings.general.shortcutActivation.title"),
            )
            .description(t("settings.general.shortcutActivation.description"))
            .control(
                Dropdown::new("shortcut-activation-dropdown")
                    .options(activation_options)
                    .selected(activation)
                    .menu_width(px(320.))
                    .disabled(st.settings.is_updating("shortcut_activation"))
                    .on_select(|v, _, cx| Self::set("shortcut_activation", json!(v), cx)),
            )
            .into_any_element(),
        ];
        if let Some(cancel) = &self.cancel_shortcut {
            shortcut_rows.push(cancel.clone().into_any_element());
        }

        // -- Model settings card -----------------------------------------------
        let current = st.models.model(&st.models.current_model).cloned();
        let model_card = current.as_ref().and_then(|m| {
            if !(m.supports_language_selection || m.supports_translation) {
                return None;
            }
            let mut rows = Vec::new();
            if m.supports_language_selection {
                rows.push(self.language.clone().into_any_element());
            }
            if m.supports_translation {
                rows.push(
                    SettingRow::new(
                        "translate-to-english",
                        t("settings.advanced.translateToEnglish.label"),
                    )
                    .description(t("settings.advanced.translateToEnglish.description"))
                    .control(toggle(
                        "translate-toggle",
                        s.translate_to_english,
                        st.settings.is_updating("translate_to_english"),
                        |v, _, cx| Self::set("translate_to_english", json!(v), cx),
                    ))
                    .into_any_element(),
                );
            }
            let name_key = format!("onboarding.models.{}.name", m.id);
            let name = if guidy_i18n::exists(&name_key) {
                guidy_i18n::t(&name_key)
            } else {
                m.name.clone()
            };
            Some(group(
                Some(t_args("settings.modelSettings.title", &[("model", &name)])),
                rows,
                cx,
            ))
        });

        // -- Sound ---------------------------------------------------------------
        let mic_options: Vec<DropdownOption> = st
            .audio_devices
            .iter()
            .map(|d| DropdownOption::new(d.name.clone(), d.name.clone()))
            .collect();
        let selected_mic = match s.selected_microphone.as_deref() {
            None | Some("default") => "Default".to_string(),
            Some(m) => m.to_string(),
        };
        let mic_updating = st.settings.is_updating("selected_microphone");
        let no_mics = st.audio_devices.is_empty();
        let mut sound_rows =
            vec![
                SettingRow::new("microphone", t("settings.sound.microphone.title"))
                    .description(t("settings.sound.microphone.description"))
                    .control(
                        h_flex()
                            .gap_1()
                            .child(
                                Dropdown::new("microphone-dropdown")
                                    .options(mic_options)
                                    .selected(Some(selected_mic))
                                    .placeholder(if loading || no_mics {
                                        t("settings.sound.microphone.loading")
                                    } else {
                                        t("settings.sound.microphone.placeholder")
                                    })
                                    .disabled(mic_updating || loading || no_mics)
                                    .on_open(|_, cx| {
                                        AppState::global(cx)
                                            .update(cx, |s, cx| s.refresh_audio_devices(cx))
                                    })
                                    .on_select(|v, _, cx| {
                                        Self::set("selected_microphone", json!(v), cx)
                                    }),
                            )
                            .child(reset_button(
                                "microphone-reset",
                                mic_updating || loading,
                                |_, cx| {
                                    AppState::global(cx).update(cx, |s, cx| {
                                        s.reset_setting("selected_microphone", cx)
                                    })
                                },
                            )),
                    )
                    .into_any_element(),
            ];

        let channel_count = self.channels.1;
        if channel_count > 1 {
            let mut options = vec![DropdownOption::new(
                "average",
                t("settings.sound.channel.average"),
            )];
            for i in 0..channel_count {
                options.push(DropdownOption::new(
                    i.to_string(),
                    t_args(
                        "settings.sound.channel.channel",
                        &[("n", &(i + 1).to_string())],
                    ),
                ));
            }
            let current = match s.selected_channel {
                Some(c) if c < channel_count => c.to_string(),
                _ => "average".to_string(),
            };
            sound_rows.push(
                SettingRow::new("channel", t("settings.sound.channel.title"))
                    .description(t("settings.sound.channel.description"))
                    .control(
                        Dropdown::new("channel-dropdown")
                            .options(options)
                            .selected(Some(current))
                            .disabled(st.settings.is_updating("selected_channel") || loading)
                            .on_select(|v, _, cx| {
                                let channel = if v == "average" {
                                    json!(null)
                                } else {
                                    json!(v.parse::<u16>().ok())
                                };
                                Self::set("selected_channel", channel, cx)
                            }),
                    )
                    .into_any_element(),
            );
        }

        sound_rows.push(
            SettingRow::new(
                "mute-while-recording",
                t("settings.debug.muteWhileRecording.label"),
            )
            .description(t("settings.debug.muteWhileRecording.description"))
            .control(toggle(
                "mute-toggle",
                s.mute_while_recording,
                st.settings.is_updating("mute_while_recording"),
                |v, _, cx| Self::set("mute_while_recording", json!(v), cx),
            ))
            .into_any_element(),
        );
        sound_rows.push(
            SettingRow::new("audio-feedback", t("settings.sound.audioFeedback.label"))
                .description(t("settings.sound.audioFeedback.description"))
                .control(toggle(
                    "audio-feedback-toggle",
                    s.audio_feedback,
                    st.settings.is_updating("audio_feedback"),
                    |v, _, cx| Self::set("audio_feedback", json!(v), cx),
                ))
                .into_any_element(),
        );

        let feedback_off = !s.audio_feedback;
        let out_options: Vec<DropdownOption> = st
            .output_devices
            .iter()
            .map(|d| DropdownOption::new(d.name.clone(), d.name.clone()))
            .collect();
        let selected_out = match s.selected_output_device.as_deref() {
            None | Some("default") => "Default".to_string(),
            Some(m) => m.to_string(),
        };
        let out_updating = st.settings.is_updating("selected_output_device");
        let no_outs = st.output_devices.is_empty();
        sound_rows.push(
            SettingRow::new("output-device", t("settings.sound.outputDevice.title"))
                .description(t("settings.sound.outputDevice.description"))
                .disabled(feedback_off)
                .control(
                    h_flex()
                        .gap_1()
                        .child(
                            Dropdown::new("output-device-dropdown")
                                .options(out_options)
                                .selected(Some(selected_out))
                                .placeholder(if loading || no_outs {
                                    t("settings.sound.outputDevice.loading")
                                } else {
                                    t("settings.sound.outputDevice.placeholder")
                                })
                                .disabled(feedback_off || out_updating || loading || no_outs)
                                .on_open(|_, cx| {
                                    AppState::global(cx)
                                        .update(cx, |s, cx| s.refresh_output_devices(cx))
                                })
                                .on_select(|v, _, cx| {
                                    Self::set("selected_output_device", json!(v), cx)
                                }),
                        )
                        .child(reset_button(
                            "output-device-reset",
                            feedback_off || out_updating || loading,
                            |_, cx| {
                                AppState::global(cx).update(cx, |s, cx| {
                                    s.reset_setting("selected_output_device", cx)
                                })
                            },
                        )),
                )
                .into_any_element(),
        );
        sound_rows.push(self.volume.clone().into_any_element());

        page()
            .id("page-general")
            .child(group(Some(t("settings.general.title")), shortcut_rows, cx))
            .when_some(model_card, |p, card| p.child(card))
            .child(group(Some(t("settings.sound.title")), sound_rows, cx))
            .test_support()
            .into_any_element()
    }
}
