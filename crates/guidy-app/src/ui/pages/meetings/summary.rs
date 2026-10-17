//! Meeting summary and action items (plan task 2.3): title, short summary,
//! decisions and topics, all editable; "Generate summary" / "Regenerate
//! with..." (provider + model override); action items with owner, due date
//! and a source quote whose timestamp jumps to the transcript moment.

use crate::meetings_backend::SharedMeetingsBackend;
use crate::state::AppState;
use crate::ui::kit::{group, icon, t, t_args, Dropdown, DropdownOption, SettingRow};
use crate::ui::pages::meetings::ai_settings::{
    error_box, model_label, provider_options, MeetingsLlmChoice,
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use guidy_core::meetings::types::{
    ActionItem, ActionItemId, MeetingId, MeetingSpeaker, Person, StoredSummary,
};
use std::collections::HashMap;
use std::rc::Rc;

/// The summary and action-item editor for `meeting_id`.
pub fn render(meeting_id: MeetingId, window: &mut Window, cx: &mut App) -> AnyElement {
    view(meeting_id, window, cx).into_any_element()
}

/// Like [`render`], with the detail view's "seek the transcript to `t_ms`
/// and play" callback for action-item timestamps.
pub fn render_with_seek(
    meeting_id: MeetingId,
    on_seek: SeekHandler,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let v = view(meeting_id, window, cx);
    v.update(cx, |v, _| v.on_seek = Some(on_seek));
    v.into_any_element()
}

/// The (kept-alive) view for `meeting_id`, e.g. to subscribe to
/// [`SummaryEvent::SeekTo`] or call [`SummaryView::reload`].
pub fn view(meeting_id: MeetingId, window: &mut Window, cx: &mut App) -> Entity<SummaryView> {
    window.use_keyed_state(
        SharedString::from(format!("meeting-summary-{meeting_id}")),
        cx,
        move |window, cx| SummaryView::new(meeting_id, window, cx),
    )
}

/// Called with a transcript time (ms from the recording start) when the user
/// clicks an action item's quote timestamp. The detail view seeks + plays.
pub type SeekHandler = Rc<dyn Fn(i64, &mut Window, &mut App)>;

/// Emitted alongside the [`SeekHandler`] so a parent can also subscribe.
#[derive(Clone, Debug, PartialEq)]
pub enum SummaryEvent {
    SeekTo(i64),
}

/// `1:05` / `1:02:03` for a transcript time in ms.
pub fn format_timestamp(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One entry per non-empty, trimmed line.
pub fn lines_to_vec(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Value of the "me" owner option.
pub const OWNER_ME: &str = "me";
/// Value of the "no owner" option.
pub const OWNER_NONE: &str = "none";

/// Owner picker options: the user, the meeting's named speakers, then the
/// rest of People, then unnamed speakers (by their cluster label).
pub fn owner_options(
    speakers: &[MeetingSpeaker],
    people: &[Person],
    user_name: &str,
) -> Vec<(String, String)> {
    let me = if user_name.trim().is_empty() {
        guidy_i18n::t("meetingsAi.summary.owner.me")
    } else {
        guidy_i18n::t_args("meetingsAi.summary.owner.meNamed", &[("name", user_name)])
    };
    let mut out = vec![
        (
            OWNER_NONE.to_string(),
            guidy_i18n::t("meetingsAi.summary.owner.none"),
        ),
        (OWNER_ME.to_string(), me),
    ];
    let mut seen = std::collections::HashSet::new();
    let in_meeting: Vec<_> = speakers
        .iter()
        .filter(|s| !s.is_user)
        .filter_map(|s| s.person_id)
        .collect();
    let ordered = in_meeting
        .iter()
        .filter_map(|id| people.iter().find(|p| p.id == *id))
        .chain(people.iter());
    for p in ordered {
        if seen.insert(p.id) {
            out.push((format!("person:{}", p.id), p.display_name.clone()));
        }
    }
    for s in speakers
        .iter()
        .filter(|s| !s.is_user && s.person_id.is_none())
    {
        out.push((format!("name:{}", s.cluster_label), s.cluster_label.clone()));
    }
    out
}

/// The owner option value currently selected for `item`.
pub fn owner_value(item: &ActionItem) -> String {
    if item.owner_is_user {
        OWNER_ME.into()
    } else if let Some(id) = item.owner_person_id {
        format!("person:{id}")
    } else if let Some(name) = item.owner_name.as_ref().filter(|n| !n.is_empty()) {
        format!("name:{name}")
    } else {
        OWNER_NONE.into()
    }
}

/// Apply an owner option value to an item.
pub fn apply_owner(item: &mut ActionItem, value: &str, people: &[Person], user_name: &str) {
    item.owner_is_user = false;
    item.owner_person_id = None;
    item.owner_name = None;
    if value == OWNER_ME {
        item.owner_is_user = true;
        item.owner_name = (!user_name.trim().is_empty()).then(|| user_name.to_string());
    } else if let Some(id) = value.strip_prefix("person:").and_then(|v| v.parse().ok()) {
        item.owner_person_id = Some(id);
        item.owner_name = people
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.display_name.clone());
    } else if let Some(name) = value.strip_prefix("name:") {
        item.owner_name = Some(name.to_string());
    }
}

struct ItemInputs {
    text: Entity<InputState>,
    due: Entity<InputState>,
    _subs: Vec<Subscription>,
}

pub struct SummaryView {
    backend: SharedMeetingsBackend,
    apple_available: bool,
    meeting_id: MeetingId,
    loaded: bool,
    summary: Option<StoredSummary>,
    items: Vec<ActionItem>,
    speakers: Vec<MeetingSpeaker>,
    people: Vec<Person>,
    /// A summary/regeneration is running.
    generating: bool,
    error: Option<String>,
    // Summary editor.
    editing: bool,
    title: Entity<InputState>,
    short_summary: Entity<TextareaState>,
    decisions: Entity<TextareaState>,
    topics: Entity<TextareaState>,
    // "Regenerate with..." picker.
    picker_open: bool,
    picker: MeetingsLlmChoice,
    picker_models: HashMap<String, Vec<String>>,
    picker_model_input: Entity<InputState>,
    // Action items.
    item_inputs: HashMap<ActionItemId, ItemInputs>,
    new_item: Entity<InputState>,
    clear_new_item: bool,
    on_seek: Option<SeekHandler>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<SummaryEvent> for SummaryView {}

impl SummaryView {
    pub fn new(meeting_id: MeetingId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let backend = state.read(cx).meetings.clone();
        let apple_available = state.read(cx).backend.check_apple_intelligence_available();
        let title = cx.new(|cx| InputState::new(window, cx));
        let area = |key: &str, window: &mut Window, cx: &mut Context<Self>| {
            let ph = t(key);
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 12)
                    .placeholder(ph)
            })
        };
        let short_summary = area("meetingsAi.summary.shortSummaryPlaceholder", window, cx);
        let decisions = area("meetingsAi.summary.linesPlaceholder", window, cx);
        let topics = area("meetingsAi.summary.linesPlaceholder", window, cx);
        let picker_model_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetingsAi.llm.modelPlaceholder"))
        });
        let new_item = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("meetingsAi.summary.newItemPlaceholder"))
        });
        let subs = vec![
            cx.subscribe(&new_item, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::PressEnter { .. } => this.add_item(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&picker_model_input, |this, input, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.picker.model = input.read(cx).value().trim().to_string();
                    cx.notify();
                }
            }),
        ];
        let mut this = Self {
            backend,
            apple_available,
            meeting_id,
            loaded: false,
            summary: None,
            items: Vec::new(),
            speakers: Vec::new(),
            people: Vec::new(),
            generating: false,
            error: None,
            editing: false,
            title,
            short_summary,
            decisions,
            topics,
            picker_open: false,
            picker: MeetingsLlmChoice::default(),
            picker_models: HashMap::new(),
            picker_model_input,
            item_inputs: HashMap::new(),
            new_item,
            clear_new_item: false,
            on_seek: None,
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    /// Hook for the detail view: seek the transcript to `t_ms` and play.
    pub fn set_on_seek(&mut self, f: impl Fn(i64, &mut Window, &mut App) + 'static) {
        self.on_seek = Some(Rc::new(f));
    }

    pub fn meeting_id(&self) -> MeetingId {
        self.meeting_id
    }

    /// Switch to another meeting (the detail view reuses one entity).
    pub fn set_meeting(&mut self, meeting_id: MeetingId, cx: &mut Context<Self>) {
        if meeting_id == self.meeting_id {
            return;
        }
        self.meeting_id = meeting_id;
        self.loaded = false;
        self.summary = None;
        self.items.clear();
        self.item_inputs.clear();
        self.editing = false;
        self.picker_open = false;
        self.error = None;
        self.reload(cx);
    }

    fn run<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&dyn crate::meetings_backend::MeetingsBackend) -> T + Send + 'static,
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

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let id = self.meeting_id;
        self.run(
            cx,
            move |b| {
                (
                    b.summary(id),
                    b.action_items(id),
                    b.speakers(id),
                    b.list_people(),
                )
            },
            move |this, (summary, items, speakers, people), _| {
                if this.meeting_id != id {
                    return;
                }
                this.loaded = true;
                match summary {
                    Ok(s) => this.summary = s,
                    Err(e) => this.error = Some(e),
                }
                match items {
                    Ok(items) => this.set_items(items),
                    Err(e) => this.error = Some(e),
                }
                this.speakers = speakers.unwrap_or_default();
                this.people = people.unwrap_or_default();
            },
        );
    }

    fn set_items(&mut self, items: Vec<ActionItem>) {
        self.item_inputs
            .retain(|id, _| items.iter().any(|i| i.id == *id));
        self.items = items;
    }

    fn user_name(&self, cx: &App) -> String {
        AppState::global(cx)
            .read(cx)
            .app_settings()
            .map(|s| s.meetings.user_display_name.clone())
            .unwrap_or_default()
    }

    // -- summary --------------------------------------------------------

    /// Generate with the meetings model from settings (`None`) or the
    /// picker's provider/model.
    pub fn generate(
        &mut self,
        provider_override: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        if self.generating {
            return;
        }
        self.generating = true;
        self.error = None;
        self.picker_open = false;
        let id = self.meeting_id;
        self.run(
            cx,
            move |b| {
                let s = b.summarize(id, provider_override);
                (s, b.action_items(id))
            },
            move |this, (summary, items), _| {
                this.generating = false;
                if this.meeting_id != id {
                    return;
                }
                match summary {
                    Ok(s) => {
                        this.summary = Some(s);
                        this.editing = false;
                    }
                    Err(e) => this.error = Some(e),
                }
                if let Ok(items) = items {
                    this.set_items(items);
                }
            },
        );
        cx.notify();
    }

    fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choice = AppState::global(cx)
            .read(cx)
            .app_settings()
            .map(MeetingsLlmChoice::from_settings)
            .unwrap_or_default();
        self.picker_model_input
            .update(cx, |s, cx| s.set_value(choice.model.clone(), window, cx));
        self.picker = choice;
        self.picker_open = true;
        let pid = self.picker.provider_id.clone();
        self.fetch_picker_models(pid, cx);
        cx.notify();
    }

    fn fetch_picker_models(&mut self, provider_id: String, cx: &mut Context<Self>) {
        let pid = provider_id.clone();
        self.run(
            cx,
            move |b| b.llm_models(&pid),
            move |this, r, _| {
                this.picker_models
                    .insert(provider_id, r.unwrap_or_default());
            },
        );
    }

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self
            .summary
            .clone()
            .unwrap_or_else(|| empty_summary(self.meeting_id));
        self.title
            .update(cx, |st, cx| st.set_value(s.title.clone(), window, cx));
        self.short_summary.update(cx, |st, cx| {
            st.set_value(s.short_summary.clone(), window, cx)
        });
        self.decisions.update(cx, |st, cx| {
            st.set_value(s.decisions.join("\n"), window, cx)
        });
        self.topics
            .update(cx, |st, cx| st.set_value(s.topics.join("\n"), window, cx));
        self.editing = true;
        cx.notify();
    }

    /// Test hook: fill the summary editor.
    #[cfg(test)]
    pub fn set_summary_draft(
        &mut self,
        title: &str,
        short: &str,
        decisions: &str,
        topics: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let v = [title, short, decisions, topics].map(str::to_string);
        let [a, b, c, d] = v;
        self.title.update(cx, |s, cx| s.set_value(a, window, cx));
        self.short_summary
            .update(cx, |s, cx| s.set_value(b, window, cx));
        self.decisions
            .update(cx, |s, cx| s.set_value(c, window, cx));
        self.topics.update(cx, |s, cx| s.set_value(d, window, cx));
    }

    fn save_summary(&mut self, cx: &mut Context<Self>) {
        let mut s = self
            .summary
            .clone()
            .unwrap_or_else(|| empty_summary(self.meeting_id));
        s.title = self.title.read(cx).value().trim().to_string();
        s.short_summary = self.short_summary.read(cx).value().trim().to_string();
        s.decisions = lines_to_vec(&self.decisions.read(cx).value());
        s.topics = lines_to_vec(&self.topics.read(cx).value());
        let saved = s.clone();
        self.run(
            cx,
            move |b| b.update_summary(&s),
            move |this, r, _| match r {
                Ok(()) => {
                    this.summary = Some(saved);
                    this.editing = false;
                    this.error = None;
                }
                Err(e) => this.error = Some(e),
            },
        );
    }

    // -- action items ---------------------------------------------------

    fn save_item(&mut self, item: ActionItem, cx: &mut Context<Self>) {
        if let Some(slot) = self.items.iter_mut().find(|i| i.id == item.id) {
            *slot = item.clone();
        }
        self.run(
            cx,
            move |b| b.update_action_item(&item),
            |this, r, _| {
                if let Err(e) = r {
                    this.error = Some(e);
                }
            },
        );
        cx.notify();
    }

    fn save_item_text(&mut self, id: ActionItemId, cx: &mut Context<Self>) {
        let Some(inputs) = self.item_inputs.get(&id) else {
            return;
        };
        let text = inputs.text.read(cx).value().trim().to_string();
        let due = inputs.due.read(cx).value().trim().to_string();
        let Some(mut item) = self.items.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        let due = (!due.is_empty()).then_some(due);
        if text.is_empty() || (item.text == text && item.due == due) {
            return;
        }
        item.text = text;
        item.due = due;
        self.save_item(item, cx);
    }

    /// Test hook: edit an item's text field and save it (as on blur).
    #[cfg(test)]
    pub fn edit_item_text(
        &mut self,
        id: ActionItemId,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(inputs) = self.item_inputs.get(&id) {
            let text = text.to_string();
            inputs
                .text
                .update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.save_item_text(id, cx);
    }

    fn set_owner(&mut self, id: ActionItemId, value: &str, cx: &mut Context<Self>) {
        let Some(mut item) = self.items.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        let user = self.user_name(cx);
        apply_owner(&mut item, value, &self.people, &user);
        self.save_item(item, cx);
    }

    fn add_item(&mut self, cx: &mut Context<Self>) {
        let text = self.new_item.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = self.meeting_id;
        self.run(
            cx,
            move |b| b.add_action_item(id, &text),
            |this, r, cx| match r {
                Ok(item) => {
                    this.items.push(item);
                    this.clear_new_item = true;
                    cx.notify();
                }
                Err(e) => this.error = Some(e),
            },
        );
    }

    fn delete_item(&mut self, id: ActionItemId, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |b| b.delete_action_item(id),
            move |this, r, _| match r {
                Ok(()) => {
                    this.items.retain(|i| i.id != id);
                    this.item_inputs.remove(&id);
                }
                Err(e) => this.error = Some(e),
            },
        );
    }

    fn seek(&mut self, t_ms: i64, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(SummaryEvent::SeekTo(t_ms));
        if let Some(f) = self.on_seek.clone() {
            f(t_ms, window, cx);
        }
    }

    /// Lazily create the text/due inputs for each item (needs a window).
    fn ensure_item_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing: Vec<ActionItem> = self
            .items
            .iter()
            .filter(|i| !self.item_inputs.contains_key(&i.id))
            .cloned()
            .collect();
        for item in missing {
            let id = item.id;
            let text = cx.new(|cx| {
                let mut s = InputState::new(window, cx);
                s.set_value(item.text.clone(), window, cx);
                s
            });
            let due = cx.new(|cx| {
                let mut s =
                    InputState::new(window, cx).placeholder(t("meetingsAi.summary.duePlaceholder"));
                s.set_value(item.due.clone().unwrap_or_default(), window, cx);
                s
            });
            let on_commit = move |this: &mut Self,
                                  _: Entity<InputState>,
                                  ev: &InputEvent,
                                  cx: &mut Context<Self>| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.save_item_text(id, cx);
                }
            };
            let subs = vec![
                cx.subscribe(&text, on_commit),
                cx.subscribe(&due, on_commit),
            ];
            self.item_inputs.insert(
                id,
                ItemInputs {
                    text,
                    due,
                    _subs: subs,
                },
            );
        }
    }

    // -- render ---------------------------------------------------------

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_summary = self.summary.is_some();
        h_flex()
            .w_full()
            .gap_2()
            .justify_end()
            .when(has_summary && !self.editing, |d| {
                d.child(
                    Button::new("summary-edit")
                        .ghost()
                        .small()
                        .icon(icon("pencil"))
                        .label(t("meetingsAi.summary.edit"))
                        .disabled(self.generating)
                        .on_click(cx.listener(|this, _, window, cx| this.start_edit(window, cx))),
                )
            })
            .child(
                Button::new("summary-regenerate-with")
                    .outline()
                    .small()
                    .label(t("meetingsAi.summary.regenerateWith"))
                    .disabled(self.generating)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.picker_open {
                            this.picker_open = false;
                            cx.notify();
                        } else {
                            this.open_picker(window, cx)
                        }
                    })),
            )
            .child(
                Button::new("summary-generate")
                    .primary()
                    .small()
                    .icon(icon("sparkles"))
                    .label(if has_summary {
                        t("meetingsAi.summary.regenerate")
                    } else {
                        t("meetingsAi.summary.generate")
                    })
                    .loading(self.generating)
                    .disabled(self.generating)
                    .on_click(cx.listener(|this, _, _, cx| this.generate(None, cx))),
            )
            .into_any_element()
    }

    fn picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.picker_open {
            return None;
        }
        let st = AppState::global(cx);
        let settings = st.read(cx).app_settings().cloned()?;
        let apple = self.apple_available;
        let pid = self.picker.provider_id.clone();
        let models = self.picker_models.get(&pid).cloned().unwrap_or_default();
        let current = self.picker.model.clone();
        let can_run = !pid.is_empty() && !current.is_empty();
        let weak = cx.entity().downgrade();
        let weak2 = weak.clone();
        let row = SettingRow::new("summary-picker", t("meetingsAi.summary.pickerTitle"))
            .description(t("meetingsAi.summary.pickerDescription"))
            .stacked()
            .control(
                h_flex()
                    .w_full()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Dropdown::new("summary-picker-provider")
                            .options(provider_options(&settings, apple))
                            .selected(Some(pid.clone()))
                            .on_select(move |v, window, cx| {
                                let v = v.to_string();
                                weak.update(cx, |this, cx| {
                                    this.picker.provider_id = v.clone();
                                    this.picker.model = settings_model(cx, &v);
                                    let m = this.picker.model.clone();
                                    this.picker_model_input
                                        .update(cx, |s, cx| s.set_value(m, window, cx));
                                    this.fetch_picker_models(v, cx);
                                    cx.notify();
                                })
                                .ok();
                            }),
                    )
                    .child(
                        div()
                            .w(px(200.))
                            .child(Input::new(&self.picker_model_input).small()),
                    )
                    .child(
                        Dropdown::new("summary-picker-model")
                            .options(
                                models
                                    .iter()
                                    .map(|m| DropdownOption::new(m.clone(), model_label(m)))
                                    .collect(),
                            )
                            .selected(Some(current.clone()))
                            .placeholder(t("meetingsAi.llm.pickModel"))
                            .min_width(px(160.))
                            .on_select(move |v, window, cx| {
                                let v = v.to_string();
                                weak2
                                    .update(cx, |this, cx| {
                                        this.picker.model = v.clone();
                                        this.picker_model_input
                                            .update(cx, |s, cx| s.set_value(v, window, cx));
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    )
                    .child(
                        Button::new("summary-picker-run")
                            .primary()
                            .small()
                            .label(t("meetingsAi.summary.run"))
                            .disabled(!can_run || self.generating)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let o =
                                    (this.picker.provider_id.clone(), this.picker.model.clone());
                                this.generate(Some(o), cx)
                            })),
                    ),
            );
        Some(row.into_any_element())
    }

    fn summary_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        if self.editing {
            let field = |label: SharedString, el: AnyElement| {
                v_flex()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(label))
                    .child(el)
            };
            return vec![div()
                .id("summary-editor")
                .p_3()
                .child(
                    v_flex()
                        .gap_3()
                        .child(field(
                            t("meetingsAi.summary.title"),
                            Input::new(&self.title).small().into_any_element(),
                        ))
                        .child(field(
                            t("meetingsAi.summary.shortSummary"),
                            Textarea::new(&self.short_summary).into_any_element(),
                        ))
                        .child(field(
                            t("meetingsAi.summary.decisions"),
                            Textarea::new(&self.decisions).into_any_element(),
                        ))
                        .child(field(
                            t("meetingsAi.summary.topics"),
                            Textarea::new(&self.topics).into_any_element(),
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(t("meetingsAi.summary.onePerLine")),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("summary-save")
                                        .primary()
                                        .small()
                                        .label(t("meetingsAi.common.save"))
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.save_summary(cx)),
                                        ),
                                )
                                .child(
                                    Button::new("summary-cancel")
                                        .outline()
                                        .small()
                                        .label(t("meetingsAi.common.cancel"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.editing = false;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .test_support()
                .into_any_element()];
        }
        let Some(s) = &self.summary else {
            let text = if !self.loaded {
                t("meetingsAi.common.loading")
            } else if self.generating {
                t("meetingsAi.summary.generating")
            } else {
                t("meetingsAi.summary.empty")
            };
            return vec![div()
                .id("summary-empty")
                .p_3()
                .text_sm()
                .text_color(muted)
                .child(text)
                .test_support()
                .into_any_element()];
        };
        let list = |id: &'static str, title: SharedString, items: &[String]| {
            v_flex()
                .id(id)
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(muted)
                        .child(title),
                )
                .when(items.is_empty(), |d| {
                    d.child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(t("meetingsAi.summary.none")),
                    )
                })
                .children(items.iter().map(|i| {
                    h_flex()
                        .gap_2()
                        .items_start()
                        .text_sm()
                        .child(div().text_color(muted).child("•"))
                        .child(div().flex_1().min_w_0().child(i.clone()))
                }))
        };
        let generated = t_args(
            "meetingsAi.summary.generatedBy",
            &[("model", &s.generated_by)],
        );
        vec![v_flex()
            .id("summary-view")
            .p_3()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(if s.title.is_empty() {
                        t("meetingsAi.summary.untitled")
                    } else {
                        s.title.clone().into()
                    }),
            )
            .when(!s.short_summary.is_empty(), |d| {
                d.child(div().text_sm().child(s.short_summary.clone()))
            })
            .child(list(
                "summary-decisions",
                t("meetingsAi.summary.decisions"),
                &s.decisions,
            ))
            .child(list(
                "summary-topics",
                t("meetingsAi.summary.topics"),
                &s.topics,
            ))
            .when(!s.generated_by.is_empty(), |d| {
                d.child(div().text_xs().text_color(muted).child(generated))
            })
            .test_support()
            .into_any_element()]
    }

    fn item_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let user = self.user_name(cx);
        let owners: Vec<DropdownOption> = owner_options(&self.speakers, &self.people, &user)
            .into_iter()
            .map(|(v, l)| DropdownOption::new(v, l))
            .collect();
        let mut rows: Vec<AnyElement> = Vec::new();
        for item in &self.items {
            let id = item.id;
            let Some(inputs) = self.item_inputs.get(&id) else {
                continue;
            };
            let locked = item.linear_issue_id.is_some();
            let weak = cx.entity().downgrade();
            let quote = (!item.source_quote.is_empty()).then(|| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .text_xs()
                    .text_color(muted)
                    .when_some(item.source_t_ms, |d, t_ms| {
                        d.child(
                            Button::new(SharedString::from(format!("item-seek-{id}")))
                                .link()
                                .xsmall()
                                .label(format_timestamp(t_ms))
                                .tooltip(t("meetingsAi.summary.jumpToMoment"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.seek(t_ms, window, cx)
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .italic()
                            .child(format!("“{}”", item.source_quote)),
                    )
            });
            rows.push(
                v_flex()
                    .id(SharedString::from(format!("action-item-{id}")))
                    .p_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&inputs.text).small().disabled(locked)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("item-delete-{id}")))
                                    .ghost()
                                    .small()
                                    .icon(icon("trash"))
                                    .tooltip(t("meetingsAi.summary.deleteItem"))
                                    .disabled(locked)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.delete_item(id, cx)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t("meetingsAi.summary.owner.title")),
                            )
                            .child(
                                Dropdown::new(SharedString::from(format!("item-owner-{id}")))
                                    .options(owners.clone())
                                    .selected(Some(owner_value(item)))
                                    .min_width(px(180.))
                                    .disabled(locked)
                                    .on_select(move |v, _, cx| {
                                        let v = v.to_string();
                                        weak.update(cx, |this, cx| this.set_owner(id, &v, cx)).ok();
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t("meetingsAi.summary.due")),
                            )
                            .child(
                                div()
                                    .w(px(140.))
                                    .child(Input::new(&inputs.due).small().disabled(locked)),
                            )
                            .when_some(item.linear_issue_identifier.clone(), |d, ident| {
                                d.child(
                                    gpui_kit::component::tag::Tag::primary()
                                        .small()
                                        .child(ident),
                                )
                            }),
                    )
                    .children(quote)
                    .test_support()
                    .into_any_element(),
            );
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .id("action-items-empty")
                    .p_3()
                    .text_sm()
                    .text_color(muted)
                    .child(t("meetingsAi.summary.noItems"))
                    .test_support()
                    .into_any_element(),
            );
        }
        let can_add = !self.new_item.read(cx).value().trim().is_empty();
        rows.push(
            h_flex()
                .id("action-item-add-row")
                .p_3()
                .gap_2()
                .child(div().flex_1().child(Input::new(&self.new_item).small()))
                .child(
                    Button::new("action-item-add")
                        .outline()
                        .small()
                        .icon(icon("plus"))
                        .label(t("meetingsAi.summary.addItem"))
                        .disabled(!can_add)
                        .on_click(cx.listener(|this, _, _, cx| this.add_item(cx))),
                )
                .test_support()
                .into_any_element(),
        );
        rows
    }
}

fn empty_summary(meeting_id: MeetingId) -> StoredSummary {
    StoredSummary {
        meeting_id,
        title: String::new(),
        short_summary: String::new(),
        decisions: Vec::new(),
        topics: Vec::new(),
        generated_by: String::new(),
        generated_at: 0,
    }
}

fn settings_model(cx: &App, provider_id: &str) -> String {
    AppState::global(cx)
        .read(cx)
        .app_settings()
        .and_then(|s| s.meetings.llm_models.get(provider_id).cloned())
        .unwrap_or_default()
}

impl Render for SummaryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.clear_new_item {
            self.clear_new_item = false;
            self.new_item
                .update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.ensure_item_inputs(window, cx);
        let header = self.header(cx);
        let picker = self.picker(cx);
        let summary_rows = self.summary_rows(cx);
        let item_rows = self.item_rows(cx);
        v_flex()
            .id("meeting-summary")
            .w_full()
            .gap_4()
            .child(header)
            .children(picker.map(|p| group(None, vec![p], cx)))
            .when_some(self.error.clone(), |d, e| {
                d.child(error_box("summary-error", e, cx).m_0().test_support())
            })
            .child(group(
                Some(t("meetingsAi.summary.heading")),
                summary_rows,
                cx,
            ))
            .child(group(
                Some(t("meetingsAi.summary.actionItems")),
                item_rows,
                cx,
            ))
            .test_support()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_owner, format_timestamp, lines_to_vec, owner_options, owner_value, OWNER_ME,
        OWNER_NONE,
    };
    use guidy_core::meetings::types::{ActionItem, MeetingSpeaker, Person};

    fn speaker(id: i64, label: &str, person: Option<i64>, is_user: bool) -> MeetingSpeaker {
        MeetingSpeaker {
            id,
            meeting_id: 1,
            cluster_label: label.into(),
            person_id: person,
            match_confidence: None,
            confirmed: true,
            is_user,
        }
    }

    fn person(id: i64, name: &str) -> Person {
        Person {
            id,
            display_name: name.into(),
            email: None,
            created_at: 0,
            voiceprint_count: 0,
        }
    }

    #[test]
    fn timestamps() {
        assert_eq!(format_timestamp(0), "0:00");
        assert_eq!(format_timestamp(65_000), "1:05");
        assert_eq!(format_timestamp(3_723_000), "1:02:03");
    }

    #[test]
    fn lines() {
        assert_eq!(lines_to_vec(" a \n\n b\n"), vec!["a", "b"]);
    }

    #[test]
    fn owner_options_put_meeting_people_first_and_dedupe() {
        guidy_i18n::set_language("en");
        let speakers = vec![
            speaker(1, "me", None, true),
            speaker(2, "Speaker 1", Some(20), false),
            speaker(3, "Speaker 2", None, false),
        ];
        let people = vec![person(10, "Ann"), person(20, "Bob")];
        let opts = owner_options(&speakers, &people, "Imran");
        let values: Vec<_> = opts.iter().map(|o| o.0.as_str()).collect();
        assert_eq!(
            values,
            vec!["none", "me", "person:20", "person:10", "name:Speaker 2"]
        );
    }

    #[test]
    fn apply_and_read_owner_round_trip() {
        let people = vec![person(20, "Bob")];
        let mut item = ActionItem {
            id: 1,
            meeting_id: 1,
            text: "x".into(),
            owner_person_id: None,
            owner_name: None,
            owner_is_user: false,
            due: None,
            source_quote: String::new(),
            source_t_ms: None,
            ticket: Default::default(),
            ticket_state: guidy_core::meetings::types::TicketState::Draft,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        };
        apply_owner(&mut item, "person:20", &people, "Me");
        assert_eq!(item.owner_name.as_deref(), Some("Bob"));
        assert_eq!(owner_value(&item), "person:20");
        apply_owner(&mut item, OWNER_ME, &people, "Me");
        assert!(item.owner_is_user && item.owner_person_id.is_none());
        assert_eq!(owner_value(&item), OWNER_ME);
        apply_owner(&mut item, "name:Speaker 2", &people, "Me");
        assert_eq!(owner_value(&item), "name:Speaker 2");
        apply_owner(&mut item, OWNER_NONE, &people, "Me");
        assert_eq!(owner_value(&item), OWNER_NONE);
    }
}

/// UI tests (GPUI test platform, fake backends).
#[cfg(test)]
mod ui {
    use super::SummaryView;
    use crate::meetings_backend::fake::{action_item_fixture, FakeMeetingsBackend};
    use crate::ui::pages::meetings::tickets::ui::{setup, setup_with};
    use crate::ui_tests::{exists, open, with_window};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext as _, TestAppContext};
    use guidy_core::meetings::types::StoredSummary;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn backend() -> Arc<FakeMeetingsBackend> {
        let mb = FakeMeetingsBackend::new();
        {
            let mut d = mb.data.lock();
            d.ai.summaries.insert(
                1,
                StoredSummary {
                    meeting_id: 1,
                    title: "Weekly sync".into(),
                    short_summary: "We talked.".into(),
                    decisions: vec!["Ship Friday".into()],
                    topics: vec!["Release".into()],
                    generated_by: "guidy_local/qwen3-4b".into(),
                    generated_at: 1,
                },
            );
            d.ai.items = vec![action_item_fixture(7, 1, "send the notes")];
            d.ai.llm_models = vec!["gpt-x".into()];
        }
        mb
    }

    #[gpui_kit::test]
    fn editing_the_summary_calls_update_summary(cx: &mut TestAppContext) {
        let mb = backend();
        setup(cx, mb.clone());
        let (w, view) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        assert!(exists(cx, w, "summary-view"));
        with_window(cx, w, |window, cx| window.click("summary-edit", cx));
        assert!(exists(cx, w, "summary-editor"));
        cx.update_window(w, |_, window, cx| {
            view.update(cx, |v, cx| {
                v.set_summary_draft("Renamed", "Short.", "A\n\n B \n", "T1", window, cx)
            });
        })
        .unwrap();
        with_window(cx, w, |window, cx| window.click("summary-save", cx));
        assert!(
            mb.called("update_summary 1 Renamed decisions=A|B"),
            "{:?}",
            mb.calls()
        );
        assert!(exists(cx, w, "summary-view"));
        let stored = mb.data.lock().ai.summaries.get(&1).cloned().unwrap();
        assert_eq!(stored.topics, vec!["T1".to_string()]);
        assert_eq!(stored.generated_by, "guidy_local/qwen3-4b");
    }

    #[gpui_kit::test]
    fn action_items_edit_add_delete_and_seek(cx: &mut TestAppContext) {
        let mb = backend();
        setup(cx, mb.clone());
        let seeks = Arc::new(Mutex::new(Vec::new()));
        let s2 = seeks.clone();
        let (w, view) = open(cx, move |window, cx| {
            let mut v = SummaryView::new(1, window, cx);
            v.set_on_seek(move |t, _, _| s2.lock().push(t));
            v
        });
        assert!(exists(cx, w, "action-item-7"));
        with_window(cx, w, |window, cx| window.click("item-seek-7", cx));
        assert_eq!(*seeks.lock(), vec![65_000]);

        cx.update_window(w, |_, window, cx| {
            view.update(cx, |v, cx| {
                v.edit_item_text(7, "send the minutes", window, cx)
            });
        })
        .unwrap();
        cx.run_until_parked();
        assert!(mb.called("update_action_item 7 draft send the minutes"));

        with_window(cx, w, |window, cx| window.click("item-delete-7", cx));
        assert!(mb.called("delete_action_item 7"));
        assert!(!exists(cx, w, "action-item-7"));
    }

    #[gpui_kit::test]
    fn regenerate_with_uses_the_picked_provider(cx: &mut TestAppContext) {
        let mb = backend();
        let fb = crate::testing::FakeBackend::new();
        fb.with_settings(|s| {
            s.meetings
                .llm_models
                .insert("guidy_local".into(), "qwen3-4b".into());
        });
        setup_with(cx, fb, mb.clone());
        let (w, _) = open(cx, |window, cx| SummaryView::new(1, window, cx));
        with_window(cx, w, |window, cx| window.click("summary-generate", cx));
        assert!(mb.called("summarize 1 default"), "{:?}", mb.calls());

        with_window(cx, w, |window, cx| {
            window.click("summary-regenerate-with", cx)
        });
        assert!(exists(cx, w, "summary-picker"));
        assert!(mb.called("llm_models guidy_local"));
        with_window(cx, w, |window, cx| window.click("summary-picker-run", cx));
        assert!(
            mb.called("summarize 1 guidy_local/qwen3-4b"),
            "{:?}",
            mb.calls()
        );
        assert!(!exists(cx, w, "summary-picker"));

        mb.data.lock().ai.fail_summarize = Some("model not downloaded".into());
        with_window(cx, w, |window, cx| window.click("summary-generate", cx));
        assert!(exists(cx, w, "summary-error"));
    }
}
