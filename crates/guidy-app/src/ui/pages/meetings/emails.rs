//! "Email addresses" in the meeting settings: the user's addresses, each
//! with a free-text label ("Work", "Personal"). An attendee with any of them
//! is the user. Changes save through [`update_meetings`].

use super::meeting_settings;
use super::settings::update_meetings;
use crate::logic::meetings::{add_user_email, EmailError};
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t, LocalizedPlaceholder};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

const LABEL_WIDTH: Pixels = dp(120.);

pub struct UserEmailsView {
    /// One label input per stored address, in order.
    labels: Vec<Entity<InputState>>,
    new_label: Entity<InputState>,
    new_email: Entity<InputState>,
    error: Option<EmailError>,
    label_subs: Vec<Subscription>,
    _subs: Vec<Subscription>,
}

impl UserEmailsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let new_label = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.settings.userEmails.labelPlaceholder",
                window,
                cx,
            )
        });
        let new_email = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder(
                "meetings.settings.userEmails.emailPlaceholder",
                window,
                cx,
            )
        });
        let subs = vec![
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(
                &new_email,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.add(window, cx),
                    InputEvent::Change if this.error.is_some() => {
                        this.error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];
        let mut this = Self {
            labels: Vec::new(),
            new_label,
            new_email,
            error: None,
            label_subs: Vec::new(),
            _subs: subs,
        };
        this.sync(window, cx);
        this
    }

    /// Keep one label input per stored address; mirror stored labels into
    /// inputs that aren't being edited.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let emails = meeting_settings(cx).user_emails;
        if self.labels.len() != emails.len() {
            self.labels.clear();
            self.label_subs.clear();
            for (i, e) in emails.iter().enumerate() {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .localized_placeholder("meetings.settings.userEmails.noLabel", window, cx)
                        .default_value(e.label.clone())
                });
                self.label_subs
                    .push(cx.subscribe(&input, move |_, input, ev: &InputEvent, cx| {
                        if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let label = input.read(cx).value().trim().to_string();
                            update_meetings(cx, |m| {
                                if let Some(u) = m.user_emails.get_mut(i) {
                                    u.label = label;
                                }
                            });
                        }
                    }));
                self.labels.push(input);
            }
            return;
        }
        for (input, e) in self.labels.iter().zip(&emails) {
            let focused = input.focus_handle(cx).is_focused(window);
            if !focused && input.read(cx).value().as_ref() != e.label.as_str() {
                let label = e.label.clone();
                input.update(cx, |s, cx| s.set_value(label, window, cx));
            }
        }
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.new_label.read(cx).value().to_string();
        let email = self.new_email.read(cx).value().to_string();
        if email.trim().is_empty() {
            return;
        }
        match add_user_email(&meeting_settings(cx).user_emails, &label, &email) {
            Ok(list) => {
                self.error = None;
                update_meetings(cx, |m| m.user_emails = list);
                for input in [&self.new_label, &self.new_email] {
                    input.update(cx, |s, cx| s.set_value("", window, cx));
                }
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Esc in a label: the stored label comes back.
    fn revert_label(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let stored = meeting_settings(cx)
            .user_emails
            .get(index)
            .map(|e| e.label.clone());
        if let (Some(label), Some(input)) = (stored, self.labels.get(index)) {
            input.update(cx, |s, cx| s.set_value(label, window, cx));
        }
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        update_meetings(cx, |m| {
            if index < m.user_emails.len() {
                m.user_emails.remove(index);
            }
        });
    }

    /// Test hook: type a label and an address, then press Add.
    #[cfg(test)]
    pub fn enter(&mut self, label: &str, email: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (label, email) = (label.to_string(), email.to_string());
        self.new_label
            .update(cx, |s, cx| s.set_value(label, window, cx));
        self.new_email
            .update(cx, |s, cx| s.set_value(email, window, cx));
        self.add(window, cx);
    }

    #[cfg(test)]
    pub fn error(&self) -> Option<EmailError> {
        self.error
    }
}

impl Render for UserEmailsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tone = design::tone(cx);
        let emails = meeting_settings(cx).user_emails;
        let rows = emails
            .iter()
            .zip(&self.labels)
            .enumerate()
            .map(|(i, (e, label))| {
                h_flex()
                    .id(SharedString::from(format!("meetings-user-email-{i}")))
                    .w_full()
                    .gap_2()
                    .on_action(cx.listener(move |this, _: &Escape, window, cx| {
                        this.revert_label(i, window, cx)
                    }))
                    .child(
                        div()
                            .w(LABEL_WIDTH)
                            .flex_none()
                            .child(Input::new(label).small()),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .h(design::CONTROL_H)
                            .px(dp(8.))
                            .rounded(crate::ui::design::RADIUS_CONTROL)
                            .border_1()
                            .border_color(tone.hairline)
                            .bg(tone.ink)
                            .text_size(design::TEXT_BODY)
                            .child(div().min_w_0().truncate().child(e.email.clone())),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "meetings-user-email-remove-{i}"
                        )))
                        .ghost()
                        .small()
                        .icon(icon("x"))
                        .tooltip(t("meetings.settings.userEmails.remove"))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove(i, cx))),
                    )
                    .test_support()
                    .into_any_element()
            });
        let error = self.error.map(|e| {
            div()
                .id("meetings-user-email-error")
                .text_size(design::TEXT_SECONDARY)
                .text_color(cx.theme().danger)
                .child(match e {
                    EmailError::Invalid => t("meetings.settings.userEmails.invalid"),
                    EmailError::Duplicate => t("meetings.settings.userEmails.duplicate"),
                })
                .test_support()
        });
        v_flex()
            .id("meetings-user-emails")
            .w_full()
            .gap(dp(6.))
            .children(rows)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .w(LABEL_WIDTH)
                            .flex_none()
                            .child(Input::new(&self.new_label).small()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.new_email).small()),
                    )
                    .child(
                        Button::new("meetings-user-email-add")
                            .outline()
                            .small()
                            .label(t("meetings.settings.userEmails.add"))
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .children(error)
            .test_support()
    }
}
