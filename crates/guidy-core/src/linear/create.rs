//! Idempotent ticket creation.
//!
//! Only `Approved` items without a `linear_issue_id` are created. Before the
//! create call, the item's client UUID is persisted ("creating"); it is sent
//! as `IssueCreateInput.id`, so a retry after a timeout or crash reuses it and
//! Linear can't create a second issue (the existing one is looked up,
//! updated if the draft was edited since, and recorded as already existing). The issue is persisted on the item right
//! after each successful create. An in-flight guard stops a double click
//! (two overlapping calls) from creating the same item twice.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::client::{CreatedIssue, LinearClient};
use crate::meetings::manager::TicketResult;
use crate::meetings::types::{ActionItem, ActionItemId, TicketDraft, TicketState};

/// The persistence creation needs (implemented by the meetings store).
pub trait TicketStore {
    fn action_item(&self, id: ActionItemId) -> anyhow::Result<Option<ActionItem>>;
    /// The item's client UUID for `IssueCreateInput.id`, generated and
    /// persisted first if it has none (the recoverable "creating" state).
    fn begin_create(&self, id: ActionItemId) -> anyhow::Result<String>;
    /// Record the created issue on the item (and mark it `Created`).
    fn set_linear_issue(&self, id: ActionItemId, issue: &CreatedIssue) -> anyhow::Result<()>;
}

/// Something that creates Linear issues (the real client, or a test fake).
#[allow(async_fn_in_trait)]
pub trait IssueCreator {
    /// Create the issue with id `client_id`. Returns the issue and whether it
    /// already existed (an earlier attempt with the same id went through).
    async fn create_issue(
        &self,
        draft: &TicketDraft,
        client_id: &str,
    ) -> Result<(CreatedIssue, bool), String>;
}

impl IssueCreator for LinearClient {
    async fn create_issue(
        &self,
        draft: &TicketDraft,
        client_id: &str,
    ) -> Result<(CreatedIssue, bool), String> {
        LinearClient::create_issue_with_id(self, draft, client_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Action items currently being created, per process.
#[derive(Default, Clone)]
pub struct InFlight(Arc<Mutex<HashSet<ActionItemId>>>);

impl InFlight {
    /// The ids currently being created.
    pub fn ids(&self) -> Vec<ActionItemId> {
        self.0.lock().iter().copied().collect()
    }

    /// Claim `id`; `None` if another call holds it.
    pub fn claim(&self, id: ActionItemId) -> Option<InFlightGuard> {
        self.0.lock().insert(id).then(|| InFlightGuard {
            set: self.0.clone(),
            id,
        })
    }
}

pub struct InFlightGuard {
    set: Arc<Mutex<HashSet<ActionItemId>>>,
    id: ActionItemId,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.set.lock().remove(&self.id);
    }
}

/// What a batch did, beyond the per-item results.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CreateOutcome {
    pub results: Vec<TicketResult>,
    /// Team/project of the last issue created in this batch, to remember as
    /// the defaults. `None` when nothing was created.
    pub used_team_project: Option<(String, Option<String>)>,
}

fn existing(item: &ActionItem) -> Option<String> {
    let id = item.linear_issue_id.as_deref()?;
    Some(
        item.linear_issue_identifier
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string()),
    )
}

/// Create issues for `ids`, one at a time, in order.
pub async fn create_tickets<S: TicketStore, C: IssueCreator>(
    store: &S,
    creator: &C,
    in_flight: &InFlight,
    ids: &[ActionItemId],
) -> CreateOutcome {
    let mut out = CreateOutcome::default();
    let mut seen = HashSet::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let mut already_existed = false;
        let result = match in_flight.claim(id) {
            None => Err("This ticket is already being created.".to_string()),
            Some(_guard) => match store.action_item(id) {
                Err(e) => Err(format!("Could not read the action item: {e}")),
                Ok(None) => Err("The action item no longer exists.".to_string()),
                Ok(Some(item)) => {
                    if let Some(identifier) = existing(&item) {
                        already_existed = true;
                        Ok(identifier)
                    } else if item.ticket_state != TicketState::Approved {
                        Err("Approve the ticket before creating it.".to_string())
                    } else {
                        let created = match store.begin_create(id) {
                            Err(e) => Err(format!("Could not save the action item: {e}")),
                            Ok(client_id) => creator.create_issue(&item.ticket, &client_id).await,
                        };
                        match created {
                            Err(e) => Err(e),
                            Ok((issue, existed)) => {
                                already_existed = existed;
                                // Persist before anything else, so retries see it.
                                let saved = store.set_linear_issue(id, &issue);
                                if let Some(team) = item.ticket.team_id.clone() {
                                    out.used_team_project =
                                        Some((team, item.ticket.project_id.clone()));
                                }
                                match saved {
                                    Ok(()) => Ok(issue.identifier),
                                    Err(e) => {
                                        log::error!(
                                            "Linear issue {} created for action item {id} but not saved: {e}",
                                            issue.identifier
                                        );
                                        Err(format!(
                                            "Created {} in Linear but could not save it: {e}",
                                            issue.identifier
                                        ))
                                    }
                                }
                            }
                        }
                    }
                }
            },
        };
        out.results.push(TicketResult {
            action_item_id: id,
            result,
            already_existed,
        });
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// In-memory [`TicketStore`].
    #[derive(Default)]
    pub struct MemStore(pub Mutex<HashMap<ActionItemId, ActionItem>>);

    impl TicketStore for MemStore {
        fn action_item(&self, id: ActionItemId) -> anyhow::Result<Option<ActionItem>> {
            Ok(self.0.lock().get(&id).cloned())
        }
        fn begin_create(&self, id: ActionItemId) -> anyhow::Result<String> {
            // MemStore keeps no client ids: derive a stable one per item.
            Ok(format!("00000000-0000-4000-8000-{id:012}"))
        }
        fn set_linear_issue(&self, id: ActionItemId, issue: &CreatedIssue) -> anyhow::Result<()> {
            let mut items = self.0.lock();
            let it = items
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("missing"))?;
            it.linear_issue_id = Some(issue.id.clone());
            it.linear_issue_identifier = Some(issue.identifier.clone());
            it.linear_issue_url = Some(issue.url.clone());
            it.ticket_state = TicketState::Created;
            Ok(())
        }
    }

    pub fn approved_item(id: ActionItemId) -> ActionItem {
        ActionItem {
            id,
            meeting_id: 1,
            text: format!("item {id}"),
            owner_person_id: None,
            owner_name: None,
            owner_is_user: true,
            due: None,
            source_quote: "q".into(),
            source_t_ms: Some(0),
            ticket: TicketDraft {
                title: format!("item {id}"),
                team_id: Some("team-1".into()),
                project_id: Some("proj-1".into()),
                ..Default::default()
            },
            ticket_state: TicketState::Approved,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        }
    }

    #[derive(Default)]
    struct FakeCreator(AtomicUsize);

    impl IssueCreator for FakeCreator {
        async fn create_issue(
            &self,
            _d: &TicketDraft,
            _client_id: &str,
        ) -> Result<(CreatedIssue, bool), String> {
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok((
                CreatedIssue {
                    id: format!("id-{n}"),
                    identifier: format!("ENG-{n}"),
                    url: format!("https://linear.app/x/issue/ENG-{n}"),
                },
                false,
            ))
        }
    }

    #[tokio::test]
    async fn only_approved_items_without_issue_are_created() {
        let store = MemStore::default();
        let mut draft = approved_item(2);
        draft.ticket_state = TicketState::Draft;
        let mut done = approved_item(3);
        done.linear_issue_id = Some("old".into());
        done.linear_issue_identifier = Some("ENG-9".into());
        for it in [approved_item(1), draft, done] {
            store.0.lock().insert(it.id, it);
        }
        let creator = FakeCreator::default();
        let out = create_tickets(&store, &creator, &InFlight::default(), &[1, 2, 3, 4]).await;
        assert_eq!(creator.0.load(Ordering::SeqCst), 1);
        assert_eq!(out.results[0].result, Ok("ENG-1".into()));
        assert!(!out.results[0].already_existed);
        assert!(out.results[1].result.is_err());
        assert_eq!(out.results[2].result, Ok("ENG-9".into()));
        assert!(out.results[2].already_existed);
        assert!(out.results[3].result.is_err());
        assert_eq!(
            out.used_team_project,
            Some(("team-1".into(), Some("proj-1".into())))
        );
        assert_eq!(
            store.0.lock()[&1].linear_issue_identifier.as_deref(),
            Some("ENG-1")
        );
    }

    #[tokio::test]
    async fn in_flight_item_is_not_created_twice() {
        let store = MemStore::default();
        store.0.lock().insert(1, approved_item(1));
        let creator = FakeCreator::default();
        let in_flight = InFlight::default();
        let held = in_flight.claim(1).unwrap();
        let out = create_tickets(&store, &creator, &in_flight, &[1]).await;
        assert!(out.results[0].result.is_err());
        assert_eq!(creator.0.load(Ordering::SeqCst), 0);
        drop(held);
        let out = create_tickets(&store, &creator, &in_flight, &[1, 1]).await;
        assert_eq!(out.results.len(), 1);
        assert_eq!(creator.0.load(Ordering::SeqCst), 1);
    }
}
