//! Linear ticket review list of the meeting detail view (plan task 3.3).
//!
//! Owner: G2 (docs/MEETING_NOTES_ARCH.md). G1's `detail.rs` calls [`render`]
//! once per frame for the open meeting and places the result below the
//! summary pane, above the transcript.
//!
//! "Draft tickets" fills each action item's draft; every draft is editable
//! (title, description, team, project, priority, labels, assignee) and must
//! be approved explicitly; "Create approved" creates only approved items and
//! shows per-item results. Created items are read-only and link to their
//! issue. Nothing is created without an approval.

use crate::logic::meetings::{is_linear_url, MeetingsEvent};
use crate::meetings_backend::{error_message, MeetingsBackend, SharedMeetingsBackend};
use crate::state::{AppEvent, AppState};
use crate::ui::design::{self, Pill};
use crate::ui::kit::{group, icon, t, t_args, Dropdown, DropdownOption};
use crate::ui::pages::meetings::ai_settings::error_box;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::linear::{Label, Project, Team, User};
use guidy_core::meetings::manager::TicketResult;
use guidy_core::meetings::types::{ActionItem, ActionItemId, MeetingId, TicketState};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The Linear draft-ticket review list for `meeting_id`.
pub fn render(meeting_id: MeetingId, window: &mut Window, cx: &mut App) -> AnyElement {
    view(meeting_id, window, cx).into_any_element()
}

/// The (kept-alive) view for `meeting_id`, e.g. to subscribe to
/// [`TicketsEvent::OpenSettings`] or set [`TicketsView::set_on_open_settings`].
pub fn view(meeting_id: MeetingId, window: &mut Window, cx: &mut App) -> Entity<TicketsView> {
    window.use_keyed_state(
        SharedString::from(format!("meeting-tickets-{meeting_id}")),
        cx,
        move |window, cx| TicketsView::new(meeting_id, window, cx),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum TicketsEvent {
    /// The user asked to open settings (no Linear key yet).
    OpenSettings,
}

type Hook = Rc<dyn Fn(&mut Window, &mut App)>;

/// Linear priorities: (value, i18n key).
const PRIORITIES: [(u8, &str); 5] = [
    (0, "meetingsAi.tickets.priority.none"),
    (1, "meetingsAi.tickets.priority.urgent"),
    (2, "meetingsAi.tickets.priority.high"),
    (3, "meetingsAi.tickets.priority.medium"),
    (4, "meetingsAi.tickets.priority.low"),
];

/// Items shown in the review list: drafted, or already past draft.
pub fn reviewable(items: &[ActionItem]) -> Vec<&ActionItem> {
    items
        .iter()
        .filter(|i| !i.ticket.title.is_empty() || i.ticket_state != TicketState::Draft)
        .collect()
}

/// Ids `create_tickets` gets: approved and not created yet. Never drafts or
/// discarded items.
pub fn approved_ids(items: &[ActionItem]) -> Vec<ActionItemId> {
    items
        .iter()
        .filter(|i| i.ticket_state == TicketState::Approved && i.linear_issue_id.is_none())
        .map(|i| i.id)
        .collect()
}

/// A draft can be approved once it has a title and a team.
pub fn can_approve(item: &ActionItem) -> bool {
    item.ticket_state == TicketState::Draft
        && item.linear_issue_id.is_none()
        && !item.ticket.title.trim().is_empty()
        && item.ticket.team_id.is_some()
}

pub fn is_read_only(item: &ActionItem) -> bool {
    item.linear_issue_id.is_some() || item.ticket_state == TicketState::Created
}

pub struct TicketsView {
    backend: SharedMeetingsBackend,
    meeting_id: MeetingId,
    loaded: bool,
    has_key: bool,
    items: Vec<ActionItem>,
    teams: Vec<Team>,
    users: Vec<User>,
    projects: HashMap<String, Vec<Project>>,
    labels: HashMap<String, Vec<Label>>,
    selected: HashSet<ActionItemId>,
    editing: Option<ActionItemId>,
    edit_title: Entity<InputState>,
    edit_description: Entity<TextareaState>,
    results: HashMap<ActionItemId, TicketResult>,
    drafting: bool,
    creating: bool,
    error: Option<String>,
    on_open_settings: Option<Hook>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<TicketsEvent> for TicketsView {}

impl TicketsView {
    pub fn new(meeting_id: MeetingId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let backend = state.read(cx).meetings.clone();
        let subs = vec![cx.subscribe(&state, |this, _, ev: &AppEvent, cx| match ev {
            AppEvent::Meetings(MeetingsEvent::MeetingChanged(id)) if *id == this.meeting_id => {
                this.reload(cx)
            }
            AppEvent::Meetings(MeetingsEvent::LinearKeyChanged | MeetingsEvent::Ready) => {
                // A new key may see other teams/users.
                this.teams.clear();
                this.users.clear();
                this.projects.clear();
                this.labels.clear();
                this.reload(cx)
            }
            _ => {}
        })];
        let edit_title = cx.new(|cx| InputState::new(window, cx));
        let edit_description = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 12));
        let mut this = Self {
            backend,
            meeting_id,
            loaded: false,
            has_key: false,
            items: Vec::new(),
            teams: Vec::new(),
            users: Vec::new(),
            projects: HashMap::new(),
            labels: HashMap::new(),
            selected: HashSet::new(),
            editing: None,
            edit_title,
            edit_description,
            results: HashMap::new(),
            drafting: false,
            creating: false,
            error: None,
            on_open_settings: None,
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    /// Hook for the meetings page: show the settings tab (Linear key).
    pub fn set_on_open_settings(&mut self, f: impl Fn(&mut Window, &mut App) + 'static) {
        self.on_open_settings = Some(Rc::new(f));
    }

    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&dyn MeetingsBackend) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let r = cx.background_spawn(async move { f(&*backend) }).await;
            this.update(cx, |s, cx| {
                done(s, r, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reload the key state and items. Teams and users are fetched from
    /// Linear only when not loaded yet (or after the key changed).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        let need_lists = self.teams.is_empty();
        self.run(
            cx,
            move |b| {
                let has_key = b.has_linear_api_key();
                let items = b.action_items(id);
                let lists = (has_key && need_lists).then(|| (b.linear_teams(), b.linear_users()));
                (has_key, items, lists)
            },
            move |this, (has_key, items, lists), cx| {
                if this.meeting_id != id {
                    return;
                }
                this.loaded = true;
                this.has_key = has_key;
                if let Some((teams, users)) = lists {
                    match (teams, users) {
                        (Ok(t), Ok(u)) => {
                            this.teams = t;
                            this.users = u;
                        }
                        (Err(e), _) | (_, Err(e)) => this.error = Some(error_message(&e)),
                    }
                }
                match items {
                    Ok(items) => this.set_items(items, cx),
                    Err(e) => this.error = Some(error_message(&e)),
                }
            },
        );
    }

    fn set_items(&mut self, items: Vec<ActionItem>, cx: &mut Context<Self>) {
        self.selected
            .retain(|id| items.iter().any(|i| i.id == *id && !is_read_only(i)));
        let teams: HashSet<String> = items
            .iter()
            .filter_map(|i| i.ticket.team_id.clone())
            .collect();
        self.items = items;
        for team in teams {
            self.load_team(team, cx);
        }
    }

    /// Projects and labels of a team (once).
    fn load_team(&mut self, team_id: String, cx: &mut Context<Self>) {
        if !self.has_key || self.projects.contains_key(&team_id) {
            return;
        }
        self.projects.insert(team_id.clone(), Vec::new());
        let tid = team_id.clone();
        self.run(
            cx,
            move |b| (b.linear_projects(&tid), b.linear_labels(&tid)),
            move |this, (projects, labels), _| {
                this.projects
                    .insert(team_id.clone(), projects.unwrap_or_default());
                this.labels.insert(team_id, labels.unwrap_or_default());
            },
        );
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TicketsEvent::OpenSettings);
        if let Some(f) = self.on_open_settings.clone() {
            f(window, cx);
        }
    }

    fn draft(&mut self, cx: &mut Context<Self>) {
        self.drafting = true;
        self.error = None;
        let id = self.meeting_id;
        self.run(
            cx,
            move |b| b.draft_tickets(id),
            |this, r, cx| {
                this.drafting = false;
                match r {
                    Ok(items) => this.set_items(items, cx),
                    Err(e) => this.error = Some(error_message(&e)),
                }
            },
        );
        cx.notify();
    }

    fn save(&mut self, item: ActionItem, cx: &mut Context<Self>) {
        if is_read_only(&item) {
            return;
        }
        if let Some(slot) = self.items.iter_mut().find(|i| i.id == item.id) {
            *slot = item.clone();
        }
        self.run(
            cx,
            move |b| b.update_action_item(&item),
            |this, r, _| {
                if let Err(e) = r {
                    this.error = Some(error_message(&e));
                }
            },
        );
        cx.notify();
    }

    fn modify(
        &mut self,
        id: ActionItemId,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ActionItem),
    ) {
        let Some(mut item) = self.items.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        if is_read_only(&item) {
            return;
        }
        f(&mut item);
        self.save(item, cx);
    }

    fn set_state(&mut self, id: ActionItemId, state: TicketState, cx: &mut Context<Self>) {
        self.results.remove(&id);
        if state != TicketState::Draft {
            self.selected.remove(&id);
        }
        self.modify(id, cx, |i| i.ticket_state = state);
    }

    fn approve_selected(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<ActionItemId> = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.id) && can_approve(i))
            .map(|i| i.id)
            .collect();
        for id in ids {
            self.set_state(id, TicketState::Approved, cx);
        }
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let ids = approved_ids(&self.items);
        if ids.is_empty() || self.creating {
            return;
        }
        self.creating = true;
        self.error = None;
        let meeting = self.meeting_id;
        self.run(
            cx,
            move |b| (b.create_tickets(&ids), b.action_items(meeting)),
            |this, (results, items), cx| {
                this.creating = false;
                match results {
                    Ok(results) => {
                        for r in results {
                            this.results.insert(r.action_item_id, r);
                        }
                    }
                    Err(e) => this.error = Some(error_message(&e)),
                }
                if let Ok(items) = items {
                    this.set_items(items, cx);
                }
            },
        );
        cx.notify();
    }

    fn start_edit(&mut self, id: ActionItemId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return;
        };
        let (title, desc) = (item.ticket.title.clone(), item.ticket.description.clone());
        self.edit_title
            .update(cx, |s, cx| s.set_value(title, window, cx));
        self.edit_description
            .update(cx, |s, cx| s.set_value(desc, window, cx));
        self.editing = Some(id);
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing.take() else {
            return;
        };
        let title = self.edit_title.read(cx).value().trim().to_string();
        let desc = self.edit_description.read(cx).value().trim().to_string();
        self.modify(id, cx, |i| {
            if !title.is_empty() {
                i.ticket.title = title;
            }
            i.ticket.description = desc;
        });
    }

    /// Test hook: edit a draft's title through the editor.
    #[cfg(test)]
    pub fn edit_title(
        &mut self,
        id: ActionItemId,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_edit(id, window, cx);
        let title = title.to_string();
        self.edit_title
            .update(cx, |s, cx| s.set_value(title, window, cx));
        self.save_edit(cx);
    }

    /// Open a created issue: only `https://linear.app/...` links.
    fn open_issue(&mut self, url: String, cx: &mut Context<Self>) {
        if !is_linear_url(&url) {
            log::warn!("Not opening a ticket link that isn't a Linear URL");
            return;
        }
        self.run(
            cx,
            move |b| b.open_url(&url),
            |this, r, _| {
                if let Err(e) = r {
                    this.error = Some(error_message(&e));
                }
            },
        );
    }

    // -- render ---------------------------------------------------------

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let approved = approved_ids(&self.items).len();
        let selectable = self
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.id) && can_approve(i))
            .count();
        h_flex()
            .w_full()
            .gap_2()
            .justify_end()
            .child(
                Button::new("tickets-draft")
                    .secondary()
                    .pill()
                    .small()
                    .icon(icon("file-pen-line"))
                    .label(t("meetingsAi.tickets.draft"))
                    .loading(self.drafting)
                    .disabled(self.drafting || self.creating)
                    .on_click(cx.listener(|this, _, _, cx| this.draft(cx))),
            )
            .child(
                Button::new("tickets-approve-selected")
                    .secondary()
                    .pill()
                    .small()
                    .icon(icon("check"))
                    .label(t_args(
                        "meetingsAi.tickets.approveSelected",
                        &[("count", &selectable.to_string())],
                    ))
                    .disabled(selectable == 0 || self.creating)
                    .on_click(cx.listener(|this, _, _, cx| this.approve_selected(cx))),
            )
            .child(
                Button::new("tickets-create")
                    .primary()
                    .when(approved == 0 || self.creating, |b| b.secondary())
                    .pill()
                    .small()
                    .icon(icon("send"))
                    .label(t_args(
                        "meetingsAi.tickets.createApproved",
                        &[("count", &approved.to_string())],
                    ))
                    .loading(self.creating)
                    .disabled(approved == 0 || self.creating)
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
            )
            .into_any_element()
    }

    fn created_row(&self, item: &ActionItem, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let id = item.id;
        let ident = item
            .linear_issue_identifier
            .clone()
            .unwrap_or_else(|| guidy_i18n::t("meetingsAi.tickets.issue"));
        let url = item.linear_issue_url.clone().filter(|u| is_linear_url(u));
        let already = self.results.get(&id).is_some_and(|r| r.already_existed);
        h_flex()
            .id(SharedString::from(format!("ticket-{id}")))
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .child(icon("circle-check").small().text_color(cx.theme().success))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(item.ticket.title.clone()))
                    .child(div().text_xs().text_color(muted).child(if already {
                        t("meetingsAi.tickets.alreadyExisted")
                    } else {
                        t("meetingsAi.tickets.created")
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("ticket-issue-{id}")))
                    .link()
                    .small()
                    .label(ident)
                    .icon(icon("external-link"))
                    .disabled(url.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(u) = url.clone() {
                            this.open_issue(u, cx)
                        }
                    })),
            )
            .test_support()
            .into_any_element()
    }

    fn discarded_row(&self, item: &ActionItem, cx: &mut Context<Self>) -> AnyElement {
        let id = item.id;
        h_flex()
            .id(SharedString::from(format!("ticket-{id}")))
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .opacity(0.6)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .line_through()
                    .truncate()
                    .child(item.ticket.title.clone()),
            )
            .child(
                Button::new(SharedString::from(format!("ticket-restore-{id}")))
                    .ghost()
                    .pill()
                    .xsmall()
                    .label(t("meetingsAi.tickets.restore"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_state(id, TicketState::Draft, cx)
                    })),
            )
            .test_support()
            .into_any_element()
    }

    fn draft_row(&self, item: &ActionItem, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let id = item.id;
        let approved = item.ticket_state == TicketState::Approved;
        let team_id = item.ticket.team_id.clone();
        let editing = self.editing == Some(id);

        let header =
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    Checkbox::new(SharedString::from(format!("ticket-select-{id}")))
                        .checked(self.selected.contains(&id))
                        .disabled(approved)
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            if *checked {
                                this.selected.insert(id);
                            } else {
                                this.selected.remove(&id);
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(item.ticket.title.clone()),
                )
                .child(if approved {
                    design::chip(t("meetingsAi.tickets.approved"), true, cx)
                } else {
                    design::chip(t("meetingsAi.tickets.draftState"), false, cx)
                })
                .child(
                    Button::new(SharedString::from(format!("ticket-edit-{id}")))
                        .ghost()
                        .xsmall()
                        .icon(icon("pencil"))
                        .tooltip(t("meetingsAi.tickets.edit"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.editing == Some(id) {
                                this.editing = None;
                                cx.notify();
                            } else {
                                this.start_edit(id, window, cx)
                            }
                        })),
                )
                .child(if approved {
                    Button::new(SharedString::from(format!("ticket-unapprove-{id}")))
                        .secondary()
                        .pill()
                        .xsmall()
                        .label(t("meetingsAi.tickets.unapprove"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_state(id, TicketState::Draft, cx)
                        }))
                } else {
                    Button::new(SharedString::from(format!("ticket-approve-{id}")))
                        .primary()
                        .when(!can_approve(item), |b| b.secondary())
                        .pill()
                        .xsmall()
                        .label(t("meetingsAi.tickets.approve"))
                        .tooltip(if can_approve(item) {
                            t("meetingsAi.tickets.approveTip")
                        } else {
                            t("meetingsAi.tickets.needsTeam")
                        })
                        .disabled(!can_approve(item))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_state(id, TicketState::Approved, cx)
                        }))
                })
                .child(
                    Button::new(SharedString::from(format!("ticket-discard-{id}")))
                        .ghost()
                        .xsmall()
                        .icon(icon("x"))
                        .tooltip(t("meetingsAi.tickets.discard"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_state(id, TicketState::Discarded, cx)
                        })),
                );

        // Field dropdowns.
        let weak = cx.entity().downgrade();
        let field = move |key: &str, el: AnyElement| {
            v_flex()
                .gap_0p5()
                .child(div().text_xs().text_color(muted).child(t(key)))
                .child(el)
        };
        let w = weak.clone();
        let team = Dropdown::new(SharedString::from(format!("ticket-team-{id}")))
            .options(
                self.teams
                    .iter()
                    .map(|t| DropdownOption::new(t.id.clone(), format!("{} · {}", t.key, t.name)))
                    .collect(),
            )
            .selected(team_id.clone())
            .placeholder(t("meetingsAi.tickets.pickTeam"))
            .min_width(px(160.))
            .on_select(move |v, _, cx| {
                let v = v.to_string();
                w.update(cx, |this, cx| {
                    this.load_team(v.clone(), cx);
                    this.modify(id, cx, |i| {
                        if i.ticket.team_id.as_deref() != Some(v.as_str()) {
                            i.ticket.team_id = Some(v);
                            i.ticket.project_id = None;
                            i.ticket.label_ids.clear();
                        }
                    });
                })
                .ok();
            });
        let projects = team_id
            .as_ref()
            .and_then(|t| self.projects.get(t))
            .cloned()
            .unwrap_or_default();
        let w = weak.clone();
        let project = Dropdown::new(SharedString::from(format!("ticket-project-{id}")))
            .options(
                std::iter::once(DropdownOption::new("", t("meetingsAi.tickets.noProject")))
                    .chain(
                        projects
                            .iter()
                            .map(|p| DropdownOption::new(p.id.clone(), p.name.clone())),
                    )
                    .collect(),
            )
            .selected(Some(item.ticket.project_id.clone().unwrap_or_default()))
            .min_width(px(150.))
            .disabled(team_id.is_none())
            .on_select(move |v, _, cx| {
                let v = v.to_string();
                w.update(cx, |this, cx| {
                    this.modify(id, cx, |i| {
                        i.ticket.project_id = (!v.is_empty()).then_some(v)
                    })
                })
                .ok();
            });
        let w = weak.clone();
        let priority = Dropdown::new(SharedString::from(format!("ticket-priority-{id}")))
            .options(
                PRIORITIES
                    .iter()
                    .map(|(v, k)| DropdownOption::new(v.to_string(), t(k)))
                    .collect(),
            )
            .selected(Some(item.ticket.priority.unwrap_or(0).to_string()))
            .min_width(px(110.))
            .on_select(move |v, _, cx| {
                let p: u8 = v.parse().unwrap_or(0);
                w.update(cx, |this, cx| {
                    this.modify(id, cx, |i| i.ticket.priority = (p > 0).then_some(p))
                })
                .ok();
            });
        let w = weak.clone();
        let assignee = Dropdown::new(SharedString::from(format!("ticket-assignee-{id}")))
            .options(
                std::iter::once(DropdownOption::new("", t("meetingsAi.tickets.unassigned")))
                    .chain(self.users.iter().map(|u| {
                        let mut o = DropdownOption::new(u.id.clone(), u.name.clone());
                        if let Some(e) = &u.email {
                            o = o.description(e.clone());
                        }
                        o
                    }))
                    .collect(),
            )
            .selected(Some(item.ticket.assignee_id.clone().unwrap_or_default()))
            .min_width(px(160.))
            .menu_width(px(260.))
            .on_select(move |v, _, cx| {
                let v = v.to_string();
                w.update(cx, |this, cx| {
                    this.modify(id, cx, |i| {
                        i.ticket.assignee_id = (!v.is_empty()).then_some(v)
                    })
                })
                .ok();
            });
        let team_labels = team_id
            .as_ref()
            .and_then(|t| self.labels.get(t))
            .cloned()
            .unwrap_or_default();
        let chosen = item.ticket.label_ids.clone();
        let w = weak.clone();
        let labels = Dropdown::new(SharedString::from(format!("ticket-labels-{id}")))
            .options(
                team_labels
                    .iter()
                    .map(|l| {
                        let mark = if chosen.contains(&l.id) { "✓ " } else { "" };
                        DropdownOption::new(l.id.clone(), format!("{mark}{}", l.name))
                    })
                    .collect(),
            )
            .placeholder(if chosen.is_empty() {
                t("meetingsAi.tickets.addLabel")
            } else {
                t_args(
                    "meetingsAi.tickets.labelCount",
                    &[("count", &chosen.len().to_string())],
                )
            })
            .min_width(px(130.))
            .disabled(team_id.is_none())
            .on_select(move |v, _, cx| {
                let v = v.to_string();
                w.update(cx, |this, cx| {
                    this.modify(id, cx, |i| {
                        if let Some(pos) = i.ticket.label_ids.iter().position(|l| *l == v) {
                            i.ticket.label_ids.remove(pos);
                        } else {
                            i.ticket.label_ids.push(v);
                        }
                    })
                })
                .ok();
            });
        let label_names: Vec<String> = chosen
            .iter()
            .filter_map(|id| team_labels.iter().find(|l| &l.id == id))
            .map(|l| l.name.clone())
            .collect();

        let fields = h_flex()
            .gap_3()
            .flex_wrap()
            .items_end()
            .child(field("meetingsAi.tickets.team", team.into_any_element()))
            .child(field(
                "meetingsAi.tickets.project",
                project.into_any_element(),
            ))
            .child(field(
                "meetingsAi.tickets.priority.title",
                priority.into_any_element(),
            ))
            .child(field(
                "meetingsAi.tickets.assignee",
                assignee.into_any_element(),
            ))
            .child(field(
                "meetingsAi.tickets.labels",
                labels.into_any_element(),
            ))
            .children(label_names.into_iter().map(|n| design::chip(n, false, cx)));

        let body: AnyElement = if editing {
            v_flex()
                .gap_2()
                .child(Input::new(&self.edit_title).small())
                .child(Textarea::new(&self.edit_description))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("ticket-save-{id}")))
                                .primary()
                                .pill()
                                .small()
                                .label(t("meetingsAi.common.save"))
                                .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                        )
                        .child(
                            Button::new(SharedString::from(format!("ticket-cancel-{id}")))
                                .secondary()
                                .pill()
                                .small()
                                .label(t("meetingsAi.common.cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.editing = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(muted)
                .line_clamp(3)
                .child(item.ticket.description.clone())
                .into_any_element()
        };

        let failure = self
            .results
            .get(&id)
            .and_then(|r| r.result.as_ref().err().cloned());

        v_flex()
            .id(SharedString::from(format!("ticket-{id}")))
            .w_full()
            .p_3()
            .gap_2()
            .when(approved, |d| d.bg(theme.primary.opacity(0.04)))
            .child(header)
            .child(body)
            .child(fields)
            .when_some(failure, |d, e| {
                d.child(
                    h_flex()
                        .id(SharedString::from(format!("ticket-error-{id}")))
                        .gap_1()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(icon("circle-alert").xsmall())
                        .child(t_args("meetingsAi.tickets.createFailed", &[("error", &e)])),
                )
            })
            .test_support()
            .into_any_element()
    }

    fn no_key(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .id("tickets-no-key")
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t("meetingsAi.tickets.noKey")),
            )
            .child(
                Button::new("tickets-open-settings")
                    .secondary()
                    .pill()
                    .small()
                    .icon(icon("settings"))
                    .label(t("meetingsAi.tickets.openSettings"))
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
            .test_support()
            .into_any_element()
    }
}

impl Render for TicketsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut rows: Vec<AnyElement> = Vec::new();
        if !self.loaded {
            rows.push(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(muted)
                    .child(t("meetingsAi.common.loading"))
                    .into_any_element(),
            );
        } else if !self.has_key {
            rows.push(self.no_key(cx));
        } else {
            if let Some(e) = self.error.clone() {
                rows.push(
                    error_box("tickets-error", e, cx)
                        .test_support()
                        .into_any_element(),
                );
            }
            let items: Vec<ActionItem> = reviewable(&self.items).into_iter().cloned().collect();
            if items.is_empty() {
                rows.push(
                    div()
                        .id("tickets-empty")
                        .p_3()
                        .text_sm()
                        .text_color(muted)
                        .child(if self.items.is_empty() {
                            t("meetingsAi.tickets.noItems")
                        } else {
                            t("meetingsAi.tickets.notDrafted")
                        })
                        .test_support()
                        .into_any_element(),
                );
            }
            for item in &items {
                rows.push(if is_read_only(item) {
                    self.created_row(item, cx)
                } else if item.ticket_state == TicketState::Discarded {
                    self.discarded_row(item, cx)
                } else {
                    self.draft_row(item, cx)
                });
            }
        }
        let toolbar = (self.loaded && self.has_key).then(|| self.toolbar(cx));
        v_flex()
            .id(SharedString::from(format!(
                "meeting-tickets-{}",
                self.meeting_id
            )))
            .w_full()
            .gap_2()
            .children(toolbar)
            .child(group(Some(t("meetingsAi.tickets.heading")), rows, cx))
            .test_support()
    }
}

#[cfg(test)]
mod tests {
    use super::{approved_ids, can_approve, is_read_only, reviewable};
    use crate::meetings_backend::fake::action_item_fixture;
    use guidy_core::meetings::types::TicketState;

    #[test]
    fn only_approved_uncreated_items_are_created() {
        let mut a = action_item_fixture(1, 1, "a");
        a.ticket_state = TicketState::Approved;
        let b = action_item_fixture(2, 1, "b");
        let mut c = action_item_fixture(3, 1, "c");
        c.ticket_state = TicketState::Discarded;
        let mut d = action_item_fixture(4, 1, "d");
        d.ticket_state = TicketState::Approved;
        d.linear_issue_id = Some("x".into());
        assert_eq!(approved_ids(&[a, b, c, d]), vec![1]);
    }

    #[test]
    fn approval_needs_title_and_team() {
        let mut a = action_item_fixture(1, 1, "a");
        assert!(!can_approve(&a));
        a.ticket.title = "T".into();
        assert!(!can_approve(&a));
        a.ticket.team_id = Some("team".into());
        assert!(can_approve(&a));
        a.linear_issue_id = Some("x".into());
        assert!(!can_approve(&a) && is_read_only(&a));
    }

    #[test]
    fn undrafted_items_are_not_reviewable() {
        let a = action_item_fixture(1, 1, "a");
        let mut b = action_item_fixture(2, 1, "b");
        b.ticket.title = "B".into();
        let items = [a, b];
        let r = reviewable(&items);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, 2);
    }
}

/// UI tests (GPUI test platform, fake backends).
#[cfg(test)]
pub(crate) mod ui {
    use crate::meetings_backend::fake::{action_item_fixture, FakeMeetingsBackend};
    use crate::state::AppState;
    use crate::testing::FakeBackend;
    use crate::ui_tests::{exists, open, with_window};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, Context, Entity, IntoElement, ParentElement, Render, Styled, TestAppContext, Window,
    };
    use guidy_core::meetings::types::{ActionItem, MeetingId, TicketState};
    use std::sync::Arc;

    pub fn setup(cx: &mut TestAppContext, mb: Arc<FakeMeetingsBackend>) -> Entity<AppState> {
        setup_with(cx, FakeBackend::new(), mb)
    }

    pub fn setup_with(
        cx: &mut TestAppContext,
        backend: Arc<FakeBackend>,
        mb: Arc<FakeMeetingsBackend>,
    ) -> Entity<AppState> {
        let state = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            crate::ui::init(cx);
            guidy_i18n::set_language("en");
            let state = AppState::install_with_meetings(backend, mb, cx);
            crate::ui::overlay::OverlayState::install(cx);
            state
        });
        cx.run_until_parked();
        state
    }

    /// Hosts the review list through its public entry point.
    struct Host(MeetingId);
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(super::render(self.0, window, cx))
        }
    }

    fn drafted(id: i64) -> ActionItem {
        let mut i = action_item_fixture(id, 1, &format!("task {id}"));
        i.ticket.title = format!("Ticket {id}");
        i.ticket.team_id = Some("team-1".into());
        i
    }

    fn backend(items: Vec<ActionItem>) -> Arc<FakeMeetingsBackend> {
        let mb = FakeMeetingsBackend::new();
        {
            let mut d = mb.data.lock();
            d.ai.linear_key = Some("lin_api_x".into());
            d.ai.items = items;
        }
        mb
    }

    #[gpui_kit::test]
    fn approve_then_create_sends_only_approved_ids(cx: &mut TestAppContext) {
        let mb = backend(vec![drafted(11), drafted(12), drafted(13), drafted(14)]);
        setup(cx, mb.clone());
        let (w, _) = open(cx, |_, _| Host(1));
        // Nothing approved yet: "Create approved" creates nothing.
        with_window(cx, w, |window, cx| window.click("tickets-create", cx));
        assert!(!mb.called("create_tickets"), "{:?}", mb.calls());

        with_window(cx, w, |window, cx| window.click("ticket-approve-11", cx));
        with_window(cx, w, |window, cx| window.click("ticket-select-12", cx));
        with_window(cx, w, |window, cx| window.click("ticket-select-13", cx));
        with_window(cx, w, |window, cx| window.click("ticket-discard-13", cx));
        with_window(cx, w, |window, cx| {
            window.click("tickets-approve-selected", cx)
        });
        assert!(mb.called("update_action_item 11 approved"));
        assert!(mb.called("update_action_item 12 approved"));
        assert!(mb.called("update_action_item 13 discarded"));
        assert!(!mb.called("update_action_item 14"));

        with_window(cx, w, |window, cx| window.click("tickets-create", cx));
        let creates: Vec<_> = mb
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("create_tickets"))
            .collect();
        assert_eq!(creates, vec!["create_tickets 11,12".to_string()]);
        let d = mb.data.lock();
        let state = |id| d.ai.items.iter().find(|i| i.id == id).unwrap().ticket_state;
        assert_eq!(state(13), TicketState::Discarded);
        assert_eq!(state(14), TicketState::Draft);
    }

    #[gpui_kit::test]
    fn created_items_are_read_only_and_link_to_their_issue(cx: &mut TestAppContext) {
        let mut created = drafted(21);
        created.ticket_state = TicketState::Created;
        created.linear_issue_id = Some("issue-21".into());
        created.linear_issue_identifier = Some("ENG-21".into());
        created.linear_issue_url = Some("https://linear.app/x/issue/ENG-21".into());
        let mb = backend(vec![created, drafted(22)]);
        setup(cx, mb.clone());
        let (w, _) = open(cx, |_, _| Host(1));
        assert!(exists(cx, w, "ticket-issue-21"));
        for id in [
            "ticket-approve-21",
            "ticket-edit-21",
            "ticket-discard-21",
            "ticket-select-21",
            "ticket-team-21",
            "ticket-assignee-21",
        ] {
            assert!(!exists(cx, w, id), "{id} should not be editable");
        }
        assert!(exists(cx, w, "ticket-approve-22"));
        with_window(cx, w, |window, cx| window.click("ticket-issue-21", cx));
        assert!(mb.called("open_url https://linear.app/x/issue/ENG-21"));
        // Approving the other one and creating again only sends that one.
        with_window(cx, w, |window, cx| window.click("ticket-approve-22", cx));
        with_window(cx, w, |window, cx| window.click("tickets-create", cx));
        assert!(mb.called("create_tickets 22"), "{:?}", mb.calls());
        assert!(exists(cx, w, "ticket-issue-22"));
        assert!(!exists(cx, w, "ticket-approve-22"));
    }

    #[gpui_kit::test]
    fn draft_tickets_fills_the_review_list(cx: &mut TestAppContext) {
        let mb = backend(vec![action_item_fixture(31, 1, "write docs")]);
        setup(cx, mb.clone());
        let (w, _) = open(cx, |_, _| Host(1));
        assert!(exists(cx, w, "tickets-empty"));
        with_window(cx, w, |window, cx| window.click("tickets-draft", cx));
        assert!(mb.called("draft_tickets 1"));
        assert!(exists(cx, w, "ticket-approve-31"));
        assert!(exists(cx, w, "ticket-priority-31"));
    }

    #[gpui_kit::test]
    fn without_a_key_it_prompts_for_settings(cx: &mut TestAppContext) {
        let mb = FakeMeetingsBackend::new();
        mb.data.lock().ai.items = vec![drafted(41)];
        setup(cx, mb.clone());
        let (w, _) = open(cx, |_, _| Host(1));
        assert!(exists(cx, w, "tickets-no-key"));
        assert!(!exists(cx, w, "tickets-create"));
        assert!(!exists(cx, w, "ticket-approve-41"));
    }
}
