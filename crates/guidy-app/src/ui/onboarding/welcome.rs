//! Onboarding, step 1: what Guidy is, and which features to set up
//! (each with what it needs).

use super::assistant::{continue_button, row, rows, Assistant};
use crate::logic::flows::OnboardingStep;
use crate::state::AppState;
use crate::ui::design::{self, dp};
use crate::ui::kit::{icon, t};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy)]
enum Feature {
    Dictation,
    Meetings,
}

pub struct Welcome {
    state: Entity<AppState>,
    /// Debug preview: nothing changes.
    preview: bool,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Welcome {
    pub fn new(preview: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            _subs: vec![cx.observe(&state, |_, _, cx| cx.notify())],
            state,
            preview,
            focus,
        }
    }

    fn toggle(&mut self, feature: Feature, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        self.state.update(cx, |s, cx| {
            s.update_onboarding_choices(
                |c| match feature {
                    Feature::Dictation => c.dictation = !c.dictation,
                    Feature::Meetings => c.meetings = !c.meetings,
                },
                cx,
            )
        });
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        if self.preview || !state.startup.choices.any() {
            return;
        }
        let state = self.state.clone();
        cx.defer(move |cx| state.update(cx, |s, cx| s.set_step(OnboardingStep::Permissions, cx)));
    }

    fn feature_row(
        &self,
        feature: Feature,
        checked: bool,
        title: SharedString,
        description: SharedString,
        needs: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match feature {
            Feature::Dictation => "welcome-dictation",
            Feature::Meetings => "welcome-meetings",
        };
        let checkbox = Checkbox::new(SharedString::from(format!("{id}-check")))
            .checked(checked)
            .accessibility_label(title.clone())
            .on_click(cx.listener(move |this, _: &bool, _, cx| {
                cx.stop_propagation();
                this.toggle(feature, cx);
            }));
        row(
            id,
            Some(checkbox.into_any_element()),
            title,
            Some(description),
            Some(design::chip(needs, false, cx).into_any_element()),
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(feature, cx)))
        .test_support()
        .into_any_element()
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.state.read(cx);
        let choices = st.startup.choices.clone();
        let meetings_supported = st.meetings_supported;
        let tone = design::tone(cx);

        let mut features = vec![self.feature_row(
            Feature::Dictation,
            choices.dictation,
            t("onboarding.welcome.dictation.title"),
            t("onboarding.welcome.dictation.description"),
            t("onboarding.welcome.dictation.needs"),
            cx,
        )];
        if meetings_supported {
            features.push(self.feature_row(
                Feature::Meetings,
                choices.meetings,
                t("onboarding.welcome.meetings.title"),
                t("onboarding.welcome.meetings.description"),
                t("onboarding.welcome.meetings.needs"),
                cx,
            ));
        }
        let later = h_flex()
            .id("welcome-later-note")
            .w_full()
            .px(dp(2.))
            .gap(dp(6.))
            .items_start()
            .text_size(design::TEXT_SECONDARY)
            .line_height(dp(15.))
            .text_color(tone.text_muted)
            .child(
                icon("sparkles")
                    .size(dp(12.))
                    .mt(dp(1.))
                    .text_color(tone.text_faint),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(t("onboarding.welcome.postProcessing")),
            );

        let enabled = choices.any() && !self.preview;
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "enter" && !ev.keystroke.modifiers.modified() {
                    this.next(cx);
                }
            }))
            .child(
                Assistant::new("onboarding-welcome", t("onboarding.welcome.title"))
                    .icon("audio-lines")
                    .description(t("onboarding.welcome.description"))
                    .child(rows(Some(t("onboarding.welcome.question")), features, cx))
                    .child(later)
                    .action(continue_button(
                        "welcome-continue",
                        t("onboarding.continue"),
                        enabled,
                        cx.listener(|this, _, _, cx| this.next(cx)),
                    )),
            )
    }
}
