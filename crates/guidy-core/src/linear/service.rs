//! [`LinearService`]: the state behind the Linear section of
//! `MeetingsManager` (API key, cached lists, in-flight guard), plus drafting
//! and creation over a [`MeetingData`] store.
//!
//! Methods are async; the manager's sync methods `block_on` them.

use anyhow::Result;

use super::cache::TtlCache;
use super::client::{LinearClient, LinearError, Viewer, DEFAULT_ENDPOINT};
use super::create::{self, CreateOutcome, InFlight, TicketStore};
use super::draft::{draft_ticket, DraftContext};
use super::{Label, Project, Team, User};
use crate::meetings::types::{
    ActionItem, ActionItemId, Meeting, MeetingId, Person, TicketDraft, TicketState,
};

/// The meetings data drafting reads and writes (implemented by the store).
pub trait MeetingData: TicketStore {
    fn meeting(&self, id: MeetingId) -> Result<Option<Meeting>>;
    fn meeting_action_items(&self, meeting_id: MeetingId) -> Result<Vec<ActionItem>>;
    fn people(&self) -> Result<Vec<Person>>;
    /// Save a generated ticket draft. Must not touch an item whose issue
    /// exists, nor a draft the user filled in meanwhile (both: `Ok`, no-op).
    fn save_ticket_draft(&self, id: ActionItemId, draft: &TicketDraft) -> Result<()>;
}

/// Remembered defaults read from `settings.meetings`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinearDefaults {
    pub team_id: Option<String>,
    pub project_id: Option<String>,
}

/// Where the API key is kept. The real one is the Keychain via
/// [`crate::secrets`]; tests pass a fixed key.
#[derive(Debug, Clone)]
pub enum KeySource {
    Secrets,
    #[cfg(test)]
    Fixed(Option<String>),
}

pub struct LinearService {
    endpoint: String,
    key_source: KeySource,
    viewer: TtlCache<Viewer>,
    teams: TtlCache<Vec<Team>>,
    projects: TtlCache<Vec<Project>>,
    users: TtlCache<Vec<User>>,
    labels: TtlCache<Vec<Label>>,
    in_flight: InFlight,
}

impl Default for LinearService {
    fn default() -> Self {
        Self::new(DEFAULT_ENDPOINT, KeySource::Secrets)
    }
}

/// True when a draft still needs generating.
fn needs_draft(item: &ActionItem) -> bool {
    item.linear_issue_id.is_none()
        && item.ticket_state != TicketState::Discarded
        && item.ticket_state != TicketState::Created
        && item.ticket.title.trim().is_empty()
}

impl LinearService {
    pub fn new(endpoint: impl Into<String>, key_source: KeySource) -> Self {
        Self {
            endpoint: endpoint.into(),
            key_source,
            viewer: TtlCache::default(),
            teams: TtlCache::default(),
            projects: TtlCache::default(),
            users: TtlCache::default(),
            labels: TtlCache::default(),
            in_flight: InFlight::default(),
        }
    }

    fn api_key(&self) -> Result<Option<String>> {
        let key = match &self.key_source {
            KeySource::Secrets => crate::secrets::get(crate::secrets::LINEAR_API_KEY)?,
            #[cfg(test)]
            KeySource::Fixed(k) => k.clone(),
        };
        Ok(key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()))
    }

    fn client(&self) -> Result<LinearClient> {
        let key = self.api_key()?.ok_or(LinearError::MissingApiKey)?;
        Ok(LinearClient::with_endpoint(key, self.endpoint.clone()))
    }

    pub fn clear_cache(&self) {
        self.viewer.clear();
        self.teams.clear();
        self.projects.clear();
        self.users.clear();
        self.labels.clear();
    }

    /// Store the key in the Keychain; an empty key deletes it.
    pub fn set_api_key(&self, key: &str) -> Result<()> {
        let key = key.trim();
        if key.is_empty() {
            crate::secrets::delete(crate::secrets::LINEAR_API_KEY)?;
        } else {
            crate::secrets::set(crate::secrets::LINEAR_API_KEY, key)?;
        }
        self.clear_cache();
        Ok(())
    }

    pub fn has_api_key(&self) -> bool {
        matches!(self.api_key(), Ok(Some(_)))
    }

    /// Check the key against Linear (never cached). Returns the viewer.
    pub async fn test(&self) -> Result<Viewer> {
        let viewer = self.client()?.viewer().await?;
        self.viewer.put("", viewer.clone());
        Ok(viewer)
    }

    pub async fn viewer(&self) -> Result<Viewer> {
        if let Some(v) = self.viewer.get("") {
            return Ok(v);
        }
        self.test().await
    }

    pub async fn teams(&self) -> Result<Vec<Team>> {
        if let Some(v) = self.teams.get("") {
            return Ok(v);
        }
        let v = self.client()?.teams().await?;
        self.teams.put("", v.clone());
        Ok(v)
    }

    pub async fn projects(&self, team_id: &str) -> Result<Vec<Project>> {
        if let Some(v) = self.projects.get(team_id) {
            return Ok(v);
        }
        let v = self.client()?.projects(team_id).await?;
        self.projects.put(team_id, v.clone());
        Ok(v)
    }

    pub async fn users(&self) -> Result<Vec<User>> {
        if let Some(v) = self.users.get("") {
            return Ok(v);
        }
        let v = self.client()?.users().await?;
        self.users.put("", v.clone());
        Ok(v)
    }

    pub async fn labels(&self, team_id: &str) -> Result<Vec<Label>> {
        if let Some(v) = self.labels.get(team_id) {
            return Ok(v);
        }
        let v = self.client()?.labels(team_id).await?;
        self.labels.put(team_id, v.clone());
        Ok(v)
    }

    /// Fill empty ticket drafts of the meeting's action items and save them.
    /// Linear data (viewer, teams, users) is best-effort: without a key or
    /// network the drafts still get a title, description and the remembered
    /// team/project. Returns all of the meeting's items.
    pub async fn draft_tickets<S: MeetingData>(
        &self,
        store: &S,
        meeting_id: MeetingId,
        defaults: &LinearDefaults,
    ) -> Result<Vec<ActionItem>> {
        let items = store.meeting_action_items(meeting_id)?;
        if !items.iter().any(needs_draft) {
            return Ok(items);
        }
        let meeting = store.meeting(meeting_id)?;
        let people = store.people()?;
        let (viewer, teams, users) = if self.has_api_key() {
            let viewer = self.viewer().await;
            let teams = self.teams().await;
            let users = self.users().await;
            for e in [
                viewer.as_ref().err(),
                teams.as_ref().err(),
                users.as_ref().err(),
            ]
            .into_iter()
            .flatten()
            {
                log::warn!("Linear: drafting without Linear data: {e:#}");
            }
            (
                viewer.ok(),
                teams.unwrap_or_default(),
                users.unwrap_or_default(),
            )
        } else {
            (None, Vec::new(), Vec::new())
        };
        let attendees = meeting
            .as_ref()
            .and_then(|m| m.calendar_event.as_ref())
            .map(|e| e.attendees.clone())
            .unwrap_or_default();
        let ctx = DraftContext {
            meeting: meeting.as_ref(),
            people: &people,
            attendees: &attendees,
            linear_users: &users,
            viewer_id: viewer.as_ref().map(|v| v.id.as_str()),
            teams: &teams,
            default_team_id: defaults.team_id.as_deref(),
            default_project_id: defaults.project_id.as_deref(),
        };
        for item in items.iter().filter(|i| needs_draft(i)) {
            store.save_ticket_draft(item.id, &draft_ticket(item, &ctx))?;
        }
        // Re-read: what was actually saved (a concurrent create or edit wins).
        store.meeting_action_items(meeting_id)
    }

    /// Action items whose issue is being created right now.
    pub fn in_flight_ids(&self) -> Vec<ActionItemId> {
        self.in_flight.ids()
    }

    /// Create issues for approved items without one. See [`create`].
    pub async fn create_tickets<S: TicketStore>(
        &self,
        store: &S,
        ids: &[ActionItemId],
    ) -> Result<CreateOutcome> {
        let client = self.client()?;
        Ok(create::create_tickets(store, &client, &self.in_flight, ids).await)
    }
}

impl TicketStore for crate::meetings::store::MeetingsStore {
    fn action_item(&self, id: ActionItemId) -> Result<Option<ActionItem>> {
        self.get_action_item(id)
    }

    fn begin_create(
        &self,
        id: ActionItemId,
        draft: &TicketDraft,
    ) -> Result<(String, super::SentDraft)> {
        self.ensure_linear_client_id(id, draft)
    }

    fn set_linear_issue(&self, id: ActionItemId, issue: &super::CreatedIssue) -> Result<()> {
        crate::meetings::store::MeetingsStore::set_linear_issue(
            self,
            id,
            &issue.id,
            &issue.identifier,
            &issue.url,
        )
    }
}

impl MeetingData for crate::meetings::store::MeetingsStore {
    fn meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        self.get_meeting(id)
    }

    fn meeting_action_items(&self, meeting_id: MeetingId) -> Result<Vec<ActionItem>> {
        self.action_items(meeting_id)
    }

    fn people(&self) -> Result<Vec<Person>> {
        self.list_people()
    }

    fn save_ticket_draft(&self, id: ActionItemId, draft: &TicketDraft) -> Result<()> {
        // Re-read: skip items that got an issue or a user-written draft
        // meanwhile. The store refuses Created items on its own as well.
        match self.get_action_item(id)? {
            Some(current) if needs_draft(&current) => match self.update_ticket_draft(id, draft) {
                Err(e)
                    if e.downcast_ref::<crate::meetings::store::TicketEditError>()
                        .is_some() =>
                {
                    Ok(())
                }
                r => r,
            },
            _ => Ok(()),
        }
    }
}
