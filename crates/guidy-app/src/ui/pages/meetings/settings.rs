//! Meeting settings: the master switch (requests permissions), the user's
//! name and email, the meeting model, audio retention, calendar
//! notifications, the reminder interval, and the speaker-match thresholds
//! under "Advanced". G2's AI + Linear settings follow.
//!
//! Every change writes the whole nested object through
//! `update_setting("meetings", <MeetingSettings JSON>)`.

use super::ai_settings::AiSettingsView;
use super::{meeting_settings, run, toast_error};
use crate::logic::meetings::{non_empty, parse_retention, retention_value};
use crate::state::AppState;
use crate::ui::kit::{
    group, icon, model_name, t, t_args, toggle, Dropdown, DropdownOption, SettingRow,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::manager::PermissionReport;
use guidy_core::meetings::settings::{MeetingSettings, RETENTION_CHOICES};

const REMINDER_CHOICES: [u32; 5] = [5, 10, 15, 20, 30];

/// Persist a change to the nested meetings settings.
pub fn update_meetings(cx: &mut App, f: impl FnOnce(&mut MeetingSettings)) {
    let mut m = meeting_settings(cx);
    f(&mut m);
    match serde_json::to_value(&m) {
        Ok(json) => AppState::global(cx).update(cx, |s, cx| s.update_setting("meetings", json, cx)),
        Err(e) => log::error!("Failed to serialize meeting settings: {e}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Threshold {
    Match,
    Suggest,
}

pub struct MeetingSettingsView {
    name_input: Entity<InputState>,
    email_input: Entity<InputState>,
    match_slider: Entity<SliderState>,
    suggest_slider: Entity<SliderState>,
    dragging: bool,
    permissions: Option<PermissionReport>,
    requesting: bool,
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
        let email_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("meetings.settings.userEmail.placeholder"))
                .default_value(current.user_email.clone().unwrap_or_default())
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
            cx.subscribe(&email_input, |_, input, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let email = non_empty(&input.read(cx).value());
                    if email != meeting_settings(cx).user_email {
                        update_meetings(cx, |m| m.user_email = email);
                    }
                }
            }),
            cx.subscribe(&match_slider, |this, _, ev: &SliderEvent, cx| {
                this.on_slider(Threshold::Match, ev, cx)
            }),
            cx.subscribe(&suggest_slider, |this, _, ev: &SliderEvent, cx| {
                this.on_slider(Threshold::Suggest, ev, cx)
            }),
        ];
        let this = Self {
            name_input,
            email_input,
            match_slider,
            suggest_slider,
            dragging: false,
            permissions: None,
            requesting: false,
            advanced_open: false,
            ai: cx.new(|cx| AiSettingsView::new(window, cx)),
            _subs: subs,
        };
        if current.enabled {
            run(
                cx,
                |b| b.permission_status(),
                |this: &mut Self, p, _| {
                    if this.permissions.is_none() {
                        this.permissions = Some(p);
                    }
                },
            );
        }
        this
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
        sync_input(
            &self.email_input.clone(),
            m.user_email.clone().unwrap_or_default(),
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
        if !enabled {
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

    fn render_permissions(&self, cx: &App) -> Option<AnyElement> {
        let p = self.permissions.as_ref()?;
        let theme = cx.theme();
        let item = |key: &str, granted: bool| {
            h_flex()
                .gap_1()
                .text_xs()
                .child(
                    icon(if granted { "check" } else { "x" })
                        .xsmall()
                        .text_color(if granted { theme.success } else { theme.danger }),
                )
                .child(t(key))
                .child(div().text_color(theme.muted_foreground).child(if granted {
                    t("meetings.settings.permissions.granted")
                } else {
                    t("meetings.settings.permissions.denied")
                }))
        };
        Some(
            SettingRow::new(
                "meetings-permissions",
                t("meetings.settings.permissions.title"),
            )
            .stacked()
            .control(
                h_flex()
                    .flex_wrap()
                    .gap_x_4()
                    .gap_y_1()
                    .child(item(
                        "meetings.settings.permissions.calendar",
                        p.calendar_granted,
                    ))
                    .child(item(
                        "meetings.settings.permissions.systemAudio",
                        p.system_audio_granted,
                    ))
                    .child(item(
                        "meetings.settings.permissions.microphone",
                        p.microphone_granted,
                    ))
                    .child(item(
                        "meetings.settings.permissions.notifications",
                        p.notifications_granted,
                    )),
            )
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
                    .w(px(260.))
                    .child(div().flex_1().child(Slider::new(slider)))
                    .child(
                        div()
                            .w(px(56.))
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
                .control(
                    div()
                        .w(px(260.))
                        .child(Input::new(&self.name_input).small()),
                )
                .into_any_element(),
            SettingRow::new(
                "meetings-user-email",
                t("meetings.settings.userEmail.title"),
            )
            .description(t("meetings.settings.userEmail.description"))
            .control(
                div()
                    .w(px(260.))
                    .child(Input::new(&self.email_input).small()),
            )
            .into_any_element(),
        ];

        // -- recording ------------------------------------------------------
        let mut model_options = vec![DropdownOption::new(
            "",
            t("meetings.settings.meetingModel.default"),
        )];
        model_options.extend(
            state
                .read(cx)
                .models
                .models
                .iter()
                .filter(|mi| mi.is_downloaded)
                .map(|mi| DropdownOption::new(mi.id.clone(), model_name(mi))),
        );
        let selected_model = m.meeting_model.clone().unwrap_or_default();
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
        let recording = vec![
            SettingRow::new("meetings-model", t("meetings.settings.meetingModel.title"))
                .description(t("meetings.settings.meetingModel.description"))
                .control(
                    Dropdown::new("meetings-model-dropdown")
                        .options(model_options)
                        .selected(Some(selected_model))
                        .min_width(px(220.))
                        .on_select(|v, _, cx| {
                            let v = non_empty(v);
                            update_meetings(cx, |m| m.meeting_model = v);
                        }),
                )
                .into_any_element(),
            SettingRow::new("meetings-retention", t("meetings.settings.retention.title"))
                .description(t("meetings.settings.retention.description"))
                .control(
                    Dropdown::new("meetings-retention-dropdown")
                        .options(retention_options)
                        .selected(Some(retention_value(m.audio_retention_days)))
                        .min_width(px(160.))
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
                "meetings-reminder-interval",
                t("meetings.settings.reminderInterval.title"),
            )
            .description(t("meetings.settings.reminderInterval.description"))
            .control(
                Dropdown::new("meetings-reminder-dropdown")
                    .options(reminder_options)
                    .selected(Some(m.reminder_interval_minutes.to_string()))
                    .min_width(px(160.))
                    .on_select(|v, _, cx| {
                        if let Ok(n) = v.parse::<u32>() {
                            update_meetings(cx, |m| m.reminder_interval_minutes = n);
                        }
                    }),
            )
            .into_any_element(),
        ];

        // -- advanced -------------------------------------------------------
        let advanced_toggle = Button::new("meetings-advanced-toggle")
            .ghost()
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
