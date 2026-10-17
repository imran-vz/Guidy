//! People tab: everyone named in meetings, with voiceprint counts. Edit
//! (name + email), merge into another person, delete (with voiceprints).

use super::{run, toast_error};
use crate::logic::meetings::{merge_targets, non_empty, MeetingsEvent};
use crate::state::{AppEvent, AppState};
use crate::ui::kit::{icon, page_header, t, t_args, Dropdown, DropdownOption};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt, WindowExt,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::types::{Person, PersonId};

pub struct PeopleView {
    people: Vec<Person>,
    loading: bool,
    editing: Option<PersonId>,
    merging: Option<PersonId>,
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
            people: Vec::new(),
            loading: true,
            editing: None,
            merging: None,
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

    #[cfg(test)]
    pub fn people_count(&self) -> usize {
        self.people.len()
    }

    fn start_edit(&mut self, p: &Person, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(p.id);
        self.merging = None;
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
        self.merging = None;
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
                        .small()
                        .label(t("meetings.people.save"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("person-edit-cancel-{id}")))
                        .ghost()
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
                                    .font_semibold()
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
                                        this.merging = if this.merging == Some(id) {
                                            None
                                        } else {
                                            Some(id)
                                        };
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
            .when(merging, |d| {
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
                d.child(
                    h_flex()
                        .id(SharedString::from(format!("person-merge-panel-{id}")))
                        .w_full()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .bg(theme.background)
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
                                .placeholder(t("meetings.people.mergeInto"))
                                .on_select(move |value, window, cx| {
                                    let Ok(into) = value.parse::<PersonId>() else {
                                        return;
                                    };
                                    entity
                                        .update(cx, |this, cx| {
                                            this.confirm_merge(id, into, window, cx)
                                        })
                                        .ok();
                                }),
                        )
                        .test_support(),
                )
            })
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
                .py_3()
                .text_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(t("meetings.people.empty"))
                .test_support()
                .into_any_element()]
        } else {
            let people = self.people.clone();
            people.iter().map(|p| self.render_row(p, cx)).collect()
        };
        v_flex()
            .id("meetings-people")
            .w_full()
            .gap_4()
            .child(page_header(
                t("meetings.people.title"),
                Some(t("meetings.people.description")),
                cx,
            ))
            .child(crate::ui::kit::group(None, rows, cx))
            .test_support()
    }
}
