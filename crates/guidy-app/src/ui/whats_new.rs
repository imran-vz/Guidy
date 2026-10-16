//! What's New (Handy's `WhatsNewGate` + `WhatsNewModal`): after an update,
//! show the newest bundled release note the user hasn't seen.

use crate::logic::flows::{
    find_latest_release_note, find_release_note_to_show, is_allowed_note_link,
};
use crate::state::AppState;
use crate::ui::kit::t_args;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::{DialogAction, DialogFooter};
use gpui_kit::component::text::TextView;
use gpui_kit::component::WindowExt;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub struct WhatsNewGate {
    state: Entity<AppState>,
    /// Version dismissed this session (never reopen it).
    dismissed: Option<String>,
    shown: Option<String>,
    _subs: Vec<Subscription>,
}

impl WhatsNewGate {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let subs = vec![cx.observe_in(&state, window, |this, _, window, cx| {
            this.evaluate(window, cx)
        })];
        Self {
            state,
            dismissed: None,
            shown: None,
            _subs: subs,
        }
    }

    fn evaluate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.state.read(cx).app_settings() else {
            return;
        };
        if !settings.show_whats_new_on_update {
            return;
        }
        let notes = crate::assets::release_notes();
        let Some((version, markdown)) = find_release_note_to_show(
            &notes,
            env!("CARGO_PKG_VERSION"),
            &settings.whats_new_last_seen_version,
        )
        .cloned() else {
            return;
        };
        if self.dismissed.as_deref() == Some(&version) || self.shown.as_deref() == Some(&version) {
            return;
        }
        self.shown = Some(version.clone());
        let gate = cx.entity().downgrade();
        open_note_dialog(version, markdown, window, cx, move |version, cx| {
            gate.update(cx, |this, cx| {
                this.dismissed = Some(version.clone());
                this.state.update(cx, |s, cx| {
                    s.update_setting(
                        "whats_new_last_seen_version",
                        serde_json::json!(version),
                        cx,
                    )
                });
            })
            .ok();
        });
    }
}

impl Render for WhatsNewGate {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Open the What's New modal; `on_dismiss(version)` runs when it closes.
pub fn open_note_dialog(
    version: String,
    markdown: String,
    window: &mut Window,
    cx: &mut App,
    on_dismiss: impl Fn(String, &mut App) + 'static,
) {
    let on_dismiss = std::rc::Rc::new(on_dismiss);
    window.open_dialog(cx, move |dialog, _, _| {
        let v1 = version.clone();
        let v2 = version.clone();
        let d1 = on_dismiss.clone();
        let d2 = on_dismiss.clone();
        dialog
            .title(t_args("whatsNew.title", &[("version", &version)]))
            .w(px(520.))
            .child(
                div()
                    .id("whats-new-body")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(
                        TextView::markdown("whats-new-markdown", markdown.clone()).on_link_click(
                            |url, _, _, cx| {
                                if is_allowed_note_link(url) {
                                    cx.open_url(url);
                                }
                            },
                        ),
                    ),
            )
            .footer(
                DialogFooter::new().child(
                    DialogAction::new().child(
                        Button::new("whats-new-close")
                            .primary()
                            .label(crate::ui::kit::t("common.close")),
                    ),
                ),
            )
            .on_ok(move |_, _, cx| {
                d1(v1.clone(), cx);
                true
            })
            .on_cancel(move |_, _, cx| {
                d2(v2.clone(), cx);
                true
            })
    });
}

/// Debug: preview the latest bundled note. Returns false when none exist.
pub fn preview_latest(window: &mut Window, cx: &mut App) -> bool {
    let notes = crate::assets::release_notes();
    let Some((version, markdown)) = find_latest_release_note(&notes).cloned() else {
        return false;
    };
    open_note_dialog(version, markdown, window, cx, |_, _| {});
    true
}
