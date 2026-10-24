//! Client tests against a local mock GraphQL server.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::client::{issue_create_input, LinearClient, LinearError, SentDraft};
use super::create::{create_tickets, tests::approved_item, tests::MemStore, InFlight};
use crate::meetings::types::{TicketDraft, TicketState};

#[derive(Debug, Clone)]
struct Request {
    authorization: Option<String>,
    body: Value,
}

impl Request {
    fn query(&self) -> &str {
        self.body["query"].as_str().unwrap_or("")
    }
    fn vars(&self) -> &Value {
        &self.body["variables"]
    }
}

type Handler = Arc<dyn Fn(&Request) -> (u16, Value) + Send + Sync>;

struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Mock {
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().clone()
    }
    fn count(&self, needle: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.query().contains(needle))
            .count()
    }
}

async fn mock(handler: impl Fn(&Request) -> (u16, Value) + Send + Sync + 'static) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/graphql", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let handler: Handler = Arc::new(handler);
    let reqs = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let reqs = reqs.clone();
            tokio::spawn(async move {
                let Some(req) = read_request(&mut sock).await else {
                    return;
                };
                reqs.lock().push(req.clone());
                let (status, body) = handler(&req);
                let body = body.to_string();
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Mock { url, requests }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    };
    let len: usize = header("content-length")?.parse().ok()?;
    while buf.len() < header_end + len {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = serde_json::from_slice(&buf[header_end..header_end + len]).ok()?;
    Some(Request {
        authorization: header("authorization"),
        body,
    })
}

fn ok(data: Value) -> (u16, Value) {
    (200, json!({ "data": data }))
}

fn page(nodes: Value, next: Option<&str>) -> Value {
    json!({ "nodes": nodes, "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next } })
}

#[tokio::test]
async fn viewer_sends_raw_api_key() {
    let m = mock(|_| ok(json!({ "viewer": { "id": "u1", "name": "Imran", "email": "i@x.com" } })))
        .await;
    let c = LinearClient::with_endpoint(" lin_api_123 ", &m.url);
    let v = c.viewer().await.unwrap();
    assert_eq!(v.name, "Imran");
    assert_eq!(v.id, "u1");
    let reqs = m.requests();
    assert_eq!(reqs[0].authorization.as_deref(), Some("lin_api_123"));
    assert!(reqs[0].query().contains("viewer"));
}

#[tokio::test]
async fn teams_paginate_with_cursors() {
    let m = mock(|r| {
        assert_eq!(r.vars()["first"], 250);
        match r.vars()["after"].as_str() {
            None => ok(json!({ "teams": page(json!([{ "id": "t2", "key": "OPS", "name": "Ops" }]), Some("c1")) })),
            Some("c1") => ok(json!({ "teams": page(json!([{ "id": "t1", "key": "ENG", "name": "Engineering" }]), None) })),
            Some(other) => panic!("unexpected cursor {other}"),
        }
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let teams = c.teams().await.unwrap();
    let ids: Vec<_> = teams.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["t1", "t2"]); // sorted by name
    assert_eq!(teams[0].key, "ENG");
    assert_eq!(m.requests().len(), 2);
}

#[tokio::test]
async fn projects_for_team() {
    let m = mock(|r| {
        assert_eq!(r.vars()["teamId"], "t1");
        ok(json!({ "team": { "projects": page(json!([{ "id": "p1", "name": "Launch" }]), None) } }))
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let p = c.projects("t1").await.unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].name, "Launch");
}

#[tokio::test]
async fn users_are_active_only_and_paginated() {
    let m = mock(|r| match r.vars()["after"].as_str() {
        None => ok(json!({ "users": page(json!([
            { "id": "u1", "name": "Ann", "displayName": "ann", "email": "ann@x.com", "active": true },
            { "id": "u2", "name": "Gone", "displayName": "gone", "email": "g@x.com", "active": false }
        ]), Some("next")) })),
        _ => ok(json!({ "users": page(json!([
            { "id": "u3", "name": "Bob", "displayName": "bob", "email": null, "active": true }
        ]), None) })),
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let users = c.users().await.unwrap();
    let ids: Vec<_> = users.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(ids, ["u1", "u3"]);
    assert_eq!(users[0].display_name, "ann");
    assert_eq!(users[1].email, None);
}

#[tokio::test]
async fn labels_merge_team_and_workspace() {
    let m = mock(|r| {
        if r.query().contains("issueLabels") {
            ok(json!({ "issueLabels": page(json!([
                { "id": "l1", "name": "Bug", "isGroup": false },
                { "id": "l3", "name": "Area", "isGroup": true },
                { "id": "l4", "name": "customer", "isGroup": false }
            ]), None) }))
        } else {
            assert_eq!(r.vars()["teamId"], "t1");
            ok(json!({ "team": { "labels": page(json!([
                { "id": "l1", "name": "Bug", "isGroup": false },
                { "id": "l2", "name": "Backend", "isGroup": false }
            ]), None) } }))
        }
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let labels = c.labels("t1").await.unwrap();
    let names: Vec<_> = labels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Backend", "Bug", "customer"]);
}

#[tokio::test]
async fn issue_create_sends_input_and_returns_issue() {
    let m = mock(|r| {
        let input = &r.vars()["input"];
        assert!(r.query().contains("issueCreate"));
        assert_eq!(input["id"], "cid-1");
        assert_eq!(input["teamId"], "t1");
        assert_eq!(input["title"], "Ship it");
        assert_eq!(input["projectId"], "p1");
        assert_eq!(input["assigneeId"], "u1");
        assert_eq!(input["priority"], 2);
        assert_eq!(input["labelIds"], json!(["l1"]));
        assert_eq!(input["dueDate"], "2026-10-20");
        ok(json!({ "issueCreate": { "success": true, "issue": {
            "id": "iss-1", "identifier": "ENG-12", "url": "https://linear.app/a/issue/ENG-12"
        } } }))
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let draft = TicketDraft {
        title: "Ship it".into(),
        description: "desc".into(),
        team_id: Some("t1".into()),
        project_id: Some("p1".into()),
        priority: Some(2),
        label_ids: vec!["l1".into()],
        assignee_id: Some("u1".into()),
        due_date: Some("2026-10-20".into()),
    };
    let (issue, existed) = c
        .create_issue_with_id(&draft, "cid-1", &SentDraft::First)
        .await
        .unwrap();
    assert!(!existed);
    assert_eq!(issue.identifier, "ENG-12");
    assert_eq!(issue.url, "https://linear.app/a/issue/ENG-12");
}

#[tokio::test]
async fn unsuccessful_mutations_are_errors() {
    let m = mock(|r| {
        if r.query().contains("issueUpdate") {
            ok(json!({ "issueUpdate": { "success": false, "issue": null } }))
        } else {
            ok(json!({ "issueCreate": { "success": true } }))
        }
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let err = c
        .update_issue("i", &json!({ "title": "x" }))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        LinearError::GraphQl("the issue was not updated".into())
    );
    let draft = TicketDraft {
        title: "T".into(),
        team_id: Some("t".into()),
        ..Default::default()
    };
    // No issue in the payload; the lookup by id fails too.
    let err = c
        .create_issue_with_id(&draft, "cid", &SentDraft::First)
        .await
        .unwrap_err();
    assert!(matches!(err, LinearError::Decode(_)), "{err:?}");
}

#[test]
fn fill_input_sets_only_non_empty_fields_and_adds_labels() {
    let d = TicketDraft {
        title: "T".into(),
        team_id: Some("t".into()),
        project_id: Some(String::new()),
        label_ids: vec!["l".into()],
        ..Default::default()
    };
    assert_eq!(
        super::client::issue_fill_input(&d).unwrap(),
        json!({ "title": "T", "addedLabelIds": ["l"] })
    );
}

#[test]
fn label_ids_are_trimmed_deduped_and_blanks_dropped_everywhere() {
    let d = TicketDraft {
        title: "T".into(),
        team_id: Some("t".into()),
        label_ids: vec![
            " l1 ".into(),
            "".into(),
            "  ".into(),
            "l2".into(),
            "l1".into(),
        ],
        ..Default::default()
    };
    assert_eq!(
        issue_create_input(&d).unwrap()["labelIds"],
        json!(["l1", "l2"])
    );
    assert_eq!(
        super::client::issue_fill_input(&d).unwrap()["addedLabelIds"],
        json!(["l1", "l2"])
    );
    let sent = TicketDraft {
        label_ids: vec!["l2".into(), "l3 ".into()],
        ..d.clone()
    };
    assert_eq!(
        super::client::issue_changes_input(&sent, &d).unwrap(),
        json!({ "addedLabelIds": ["l1"], "removedLabelIds": ["l3"] })
    );
    let blank = TicketDraft {
        label_ids: vec![" ".into()],
        ..d
    };
    assert!(issue_create_input(&blank)
        .unwrap()
        .get("labelIds")
        .is_none());
}

#[test]
fn issue_input_omits_empty_fields_and_needs_team() {
    let mut d = TicketDraft {
        title: " T ".into(),
        team_id: Some("t".into()),
        project_id: Some(String::new()),
        ..Default::default()
    };
    let v = issue_create_input(&d).unwrap();
    assert_eq!(v, json!({ "teamId": "t", "title": "T" }));
    d.team_id = None;
    assert!(issue_create_input(&d).is_err());
}

#[tokio::test]
async fn http_401_is_unauthorized() {
    let m = mock(|_| (401, json!({ "error": "nope" }))).await;
    let c = LinearClient::with_endpoint("bad", &m.url);
    let e = c.viewer().await.unwrap_err();
    assert_eq!(e, LinearError::Unauthorized);
    assert!(e.to_string().contains("rejected the API key"));
}

#[tokio::test]
async fn graphql_auth_error_is_unauthorized() {
    let m = mock(|_| {
        (
            400,
            json!({ "errors": [{ "message": "Authentication required, not authenticated",
            "extensions": { "code": "AUTHENTICATION_ERROR" } }] }),
        )
    })
    .await;
    let c = LinearClient::with_endpoint("bad", &m.url);
    assert_eq!(c.teams().await.unwrap_err(), LinearError::Unauthorized);
}

#[tokio::test]
async fn graphql_errors_are_mapped() {
    let m = mock(|_| {
        (
            200,
            json!({ "data": null, "errors": [{ "message": "Argument Validation Error",
            "extensions": { "userPresentableMessage": "Team not found" } }] }),
        )
    })
    .await;
    let c = LinearClient::with_endpoint("k", &m.url);
    let e = c.projects("nope").await.unwrap_err();
    assert_eq!(e, LinearError::GraphQl("Team not found".into()));
    assert_eq!(e.to_string(), "Linear error: Team not found");
}

#[tokio::test]
async fn missing_key_fails_without_request() {
    let m = mock(|_| ok(json!({}))).await;
    let c = LinearClient::with_endpoint("  ", &m.url);
    assert_eq!(c.viewer().await.unwrap_err(), LinearError::MissingApiKey);
    assert!(m.requests().is_empty());
}

#[tokio::test]
async fn approving_again_never_creates_twice() {
    let m = mock(|_| {
        ok(json!({ "issueCreate": { "success": true, "issue": {
            "id": "iss-1", "identifier": "ENG-1", "url": "https://linear.app/a/issue/ENG-1"
        } } }))
    })
    .await;
    let client = LinearClient::with_endpoint("k", &m.url);
    let store = MemStore::default();
    store.0.lock().insert(1, approved_item(1));
    let in_flight = InFlight::default();

    let first = create_tickets(&store, &client, &in_flight, &[1]).await;
    assert_eq!(first.results[0].result, Ok("ENG-1".into()));
    assert!(!first.results[0].already_existed);
    assert_eq!(store.0.lock()[&1].ticket_state, TicketState::Created);

    // The user approves again (e.g. a stale UI) and retries.
    store.0.lock().get_mut(&1).unwrap().ticket_state = TicketState::Approved;
    let second = create_tickets(&store, &client, &in_flight, &[1]).await;
    assert_eq!(second.results[0].result, Ok("ENG-1".into()));
    assert!(second.results[0].already_existed);
    assert_eq!(second.used_team_project, None);
    assert_eq!(m.count("issueCreate"), 1);
}

#[tokio::test]
async fn failed_create_is_not_persisted() {
    let m = mock(|_| {
        (
            200,
            json!({ "errors": [{ "message": "assignee is not a team member" }] }),
        )
    })
    .await;
    let client = LinearClient::with_endpoint("k", &m.url);
    let store = MemStore::default();
    store.0.lock().insert(1, approved_item(1));
    let out = create_tickets(&store, &client, &InFlight::default(), &[1]).await;
    let err = out.results[0].result.clone().unwrap_err();
    assert!(err.contains("assignee is not a team member"), "{err}");
    assert_eq!(store.0.lock()[&1].linear_issue_id, None);
    assert_eq!(out.used_team_project, None);
}

// ----- LinearService ---------------------------------------------------------

mod service {
    use super::*;
    use crate::linear::create::TicketStore;
    use crate::linear::service::{KeySource, LinearDefaults, LinearService, MeetingData};
    use crate::linear::CreatedIssue;
    use crate::meetings::types::{
        ActionItem, ActionItemId, Attendee, CalendarEvent, Meeting, MeetingId, MeetingStatus,
        Person,
    };

    #[derive(Default)]
    struct FullStore {
        items: MemStore,
        meeting: Option<Meeting>,
        people: Vec<Person>,
    }

    impl TicketStore for FullStore {
        fn action_item(&self, id: ActionItemId) -> anyhow::Result<Option<ActionItem>> {
            self.items.action_item(id)
        }
        fn begin_create(
            &self,
            id: ActionItemId,
            draft: &TicketDraft,
        ) -> anyhow::Result<(String, crate::linear::SentDraft)> {
            self.items.begin_create(id, draft)
        }
        fn abort_create(&self, id: ActionItemId, client_id: &str) -> anyhow::Result<()> {
            self.items.abort_create(id, client_id)
        }
        fn set_linear_issue(&self, id: ActionItemId, issue: &CreatedIssue) -> anyhow::Result<()> {
            self.items.set_linear_issue(id, issue)
        }
    }

    impl MeetingData for FullStore {
        fn meeting(&self, _id: MeetingId) -> anyhow::Result<Option<Meeting>> {
            Ok(self.meeting.clone())
        }
        fn meeting_action_items(&self, m: MeetingId) -> anyhow::Result<Vec<ActionItem>> {
            let mut v: Vec<_> = self
                .items
                .0
                .lock()
                .values()
                .filter(|i| i.meeting_id == m)
                .cloned()
                .collect();
            v.sort_by_key(|i| i.id);
            Ok(v)
        }
        fn people(&self) -> anyhow::Result<Vec<Person>> {
            Ok(self.people.clone())
        }
        fn save_ticket_draft(&self, id: ActionItemId, draft: &TicketDraft) -> anyhow::Result<()> {
            if let Some(item) = self.items.0.lock().get_mut(&id) {
                item.ticket = draft.clone();
            }
            Ok(())
        }
    }

    fn linear_workspace(r: &Request) -> (u16, Value) {
        let q = r.query();
        if q.contains("viewer") {
            ok(json!({ "viewer": { "id": "u-me", "name": "Imran", "email": "me@x.com" } }))
        } else if q.contains("teams(") {
            ok(json!({ "teams": page(json!([{ "id": "t1", "key": "ENG", "name": "Eng" }]), None) }))
        } else if q.contains("users(") {
            ok(json!({ "users": page(json!([
                { "id": "u-me", "name": "Imran", "displayName": "imran", "email": "me@x.com", "active": true },
                { "id": "u-dana", "name": "Dana", "displayName": "dana", "email": "dana@x.com", "active": true }
            ]), None) }))
        } else {
            (500, json!({}))
        }
    }

    fn service(url: &str, key: Option<&str>) -> LinearService {
        LinearService::new(url, KeySource::Fixed(key.map(Into::into)))
    }

    #[tokio::test]
    async fn lists_are_cached() {
        let m = mock(linear_workspace).await;
        let s = service(&m.url, Some("k"));
        assert_eq!(s.teams().await.unwrap().len(), 1);
        assert_eq!(s.teams().await.unwrap().len(), 1);
        assert_eq!(m.count("teams("), 1);
        s.clear_cache();
        s.teams().await.unwrap();
        assert_eq!(m.count("teams("), 2);
    }

    #[tokio::test]
    async fn missing_key_is_reported() {
        let s = service("http://127.0.0.1:1/graphql", None);
        assert!(!s.has_api_key());
        let e = s.test().await.unwrap_err();
        assert!(e.to_string().contains("No Linear API key"), "{e}");
    }

    fn store_with_items() -> FullStore {
        let mut mine = approved_item(1);
        mine.ticket = Default::default();
        mine.ticket_state = TicketState::Draft;
        let mut dana = mine.clone();
        dana.id = 2;
        dana.owner_is_user = false;
        dana.owner_name = Some("Dana".into());
        dana.owner_person_id = Some(5);
        let mut kept = mine.clone();
        kept.id = 3;
        kept.ticket.title = "Edited by user".into();
        let store = FullStore {
            meeting: Some(Meeting {
                id: 1,
                title: "Weekly sync".into(),
                calendar_event_id: Some("e".into()),
                calendar_event: Some(CalendarEvent {
                    id: "e".into(),
                    title: "Weekly sync".into(),
                    start_at: 0,
                    end_at: 0,
                    meeting_url: None,
                    attendees: vec![Attendee {
                        name: Some("Dana".into()),
                        email: Some("DANA@x.com".into()),
                        is_current_user: false,
                    }],
                }),
                started_at: 1_781_524_800_000,
                ended_at: None,
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: None,
                description: None,
                title_source: Default::default(),
                has_summary: false,
            }),
            people: vec![Person {
                id: 5,
                display_name: "Dana".into(),
                email: None,
                created_at: 0,
                voiceprint_count: 0,
            }],
            ..Default::default()
        };
        for it in [mine, dana, kept] {
            store.items.0.lock().insert(it.id, it);
        }
        store
    }

    #[tokio::test]
    async fn draft_tickets_fills_empty_drafts() {
        let m = mock(linear_workspace).await;
        let s = service(&m.url, Some("k"));
        let store = store_with_items();
        let items = s
            .draft_tickets(&store, 1, &LinearDefaults::default())
            .await
            .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].ticket.assignee_id.as_deref(), Some("u-me"));
        assert_eq!(items[0].ticket.team_id.as_deref(), Some("t1"));
        assert!(items[0].ticket.description.contains("**Weekly sync**"));
        // Dana matched through the calendar attendee's email.
        assert_eq!(items[1].ticket.assignee_id.as_deref(), Some("u-dana"));
        // A draft the user already edited is left alone.
        assert_eq!(items[2].ticket.title, "Edited by user");
        assert_eq!(items[2].ticket.team_id, None);
        // Persisted.
        assert_eq!(
            store.items.0.lock()[&2].ticket.assignee_id.as_deref(),
            Some("u-dana")
        );
        // Nothing left to draft: no more Linear calls.
        let before = m.requests().len();
        s.draft_tickets(&store, 1, &LinearDefaults::default())
            .await
            .unwrap();
        assert_eq!(m.requests().len(), before);
    }

    #[tokio::test]
    async fn draft_tickets_without_key_uses_defaults() {
        let s = service("http://127.0.0.1:1/graphql", None);
        let store = store_with_items();
        let defaults = LinearDefaults {
            team_id: Some("t9".into()),
            project_id: Some("p9".into()),
        };
        let items = s.draft_tickets(&store, 1, &defaults).await.unwrap();
        assert_eq!(items[0].ticket.team_id.as_deref(), Some("t9"));
        assert_eq!(items[0].ticket.project_id.as_deref(), Some("p9"));
        assert_eq!(items[0].ticket.assignee_id, None);
        assert!(!items[0].ticket.title.is_empty());
    }

    #[tokio::test]
    async fn service_create_reports_used_defaults() {
        let m = mock(|_| {
            ok(json!({ "issueCreate": { "success": true, "issue": {
                "id": "i", "identifier": "ENG-7", "url": "u" } } }))
        })
        .await;
        let s = service(&m.url, Some("k"));
        let store = MemStore::default();
        store.0.lock().insert(1, approved_item(1));
        let out = s.create_tickets(&store, &[1]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-7".into()));
        assert_eq!(
            out.used_team_project,
            Some(("team-1".into(), Some("proj-1".into())))
        );
    }
}

// ----- Against the real (in-memory, keyed) meetings store --------------------

mod with_store {
    use super::*;
    use crate::linear::service::{KeySource, LinearDefaults, LinearService};
    use crate::meetings::store::MeetingsStore;
    use crate::meetings::types::{Meeting, MeetingStatus};

    fn store_with_item() -> (MeetingsStore, i64, i64) {
        let store = MeetingsStore::open_in_memory().unwrap();
        let meeting_id = store
            .insert_meeting(&Meeting {
                id: 0,
                title: "Planning".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: 1_781_524_800_000,
                ended_at: None,
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: None,
                description: None,
                title_source: Default::default(),
                has_summary: false,
            })
            .unwrap();
        let mut item = approved_item(0);
        item.meeting_id = meeting_id;
        item.ticket = Default::default();
        item.ticket_state = TicketState::Draft;
        let item = store.insert_action_item(&item).unwrap();
        (store, meeting_id, item.id)
    }

    fn workspace(r: &Request) -> (u16, Value) {
        let q = r.query();
        if q.contains("issueCreate") {
            ok(json!({ "issueCreate": { "success": true, "issue": {
                "id": "iss-1", "identifier": "ENG-1", "url": "https://linear.app/a/issue/ENG-1" } } }))
        } else if q.contains("viewer") {
            ok(json!({ "viewer": { "id": "u-me", "name": "Imran", "email": null } }))
        } else if q.contains("teams(") {
            ok(json!({ "teams": page(json!([{ "id": "t1", "key": "ENG", "name": "Eng" }]), None) }))
        } else {
            ok(json!({ "users": page(json!([]), None) }))
        }
    }

    #[tokio::test]
    async fn draft_approve_create_approve_again() {
        let m = mock(workspace).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, meeting_id, id) = store_with_item();

        let items = s
            .draft_tickets(&store, meeting_id, &LinearDefaults::default())
            .await
            .unwrap();
        assert_eq!(items[0].ticket.team_id.as_deref(), Some("t1"));
        assert_eq!(items[0].ticket.assignee_id.as_deref(), Some("u-me"));
        assert!(items[0]
            .ticket
            .description
            .contains(&format!("guidy://meetings/{meeting_id}?t=0")));

        // Not approved yet: nothing is created.
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert!(out.results[0].result.is_err());
        assert_eq!(m.count("issueCreate"), 0);
        let item_before_create = store.get_action_item(id).unwrap().unwrap();

        store.set_ticket_state(id, TicketState::Approved).unwrap();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-1".into()));
        let saved = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(saved.ticket_state, TicketState::Created);
        assert_eq!(saved.linear_issue_id.as_deref(), Some("iss-1"));
        assert_eq!(
            saved.linear_issue_url.as_deref(),
            Some("https://linear.app/a/issue/ENG-1")
        );

        // The client UUID was sent as the issue id.
        let sent = m
            .requests()
            .into_iter()
            .find(|r| r.query().contains("issueCreate"))
            .unwrap();
        let client_id = store.linear_client_id(id).unwrap().unwrap();
        assert_eq!(sent.vars()["input"]["id"], json!(client_id));
        assert_eq!(client_id.len(), 36);

        // Approving again is refused (the issue exists); a stale full-item
        // save keeps the link; retrying creates nothing new.
        assert!(store.set_ticket_state(id, TicketState::Approved).is_err());
        let mut stale = item_before_create.clone();
        stale.text = "edited after create".into();
        store.update_action_item(&stale).unwrap();
        let after = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(after.linear_issue_id.as_deref(), Some("iss-1"));
        assert_eq!(after.ticket_state, TicketState::Created);
        assert_eq!(after.text, "edited after create");
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert!(out.results[0].already_existed);
        assert_eq!(out.results[0].result, Ok("ENG-1".into()));
        assert_eq!(m.count("issueCreate"), 1);

        // Re-drafting never touches an item that has an issue.
        let items = s
            .draft_tickets(&store, meeting_id, &LinearDefaults::default())
            .await
            .unwrap();
        assert_eq!(items[0].linear_issue_id.as_deref(), Some("iss-1"));
    }
}

// ----- Idempotent creation with a client UUID ---------------------------------

mod idempotent {
    use super::*;
    use crate::linear::service::{KeySource, LinearService};
    use crate::meetings::store::MeetingsStore;
    use crate::meetings::types::{Meeting, MeetingStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn store_with_approved() -> (MeetingsStore, i64) {
        add_approved(MeetingsStore::open_in_memory().unwrap())
    }

    fn add_approved(store: MeetingsStore) -> (MeetingsStore, i64) {
        let meeting_id = store
            .insert_meeting(&Meeting {
                id: 0,
                title: "Planning".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: 0,
                ended_at: None,
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: None,
                description: None,
                title_source: Default::default(),
                has_summary: false,
            })
            .unwrap();
        let mut item = approved_item(0);
        item.meeting_id = meeting_id;
        let item = store.insert_action_item(&item).unwrap();
        (store, item.id)
    }

    /// The issue Linear holds for [`approved_item`]'s unedited draft.
    fn stored_issue(id: &str, identifier: &str, url: &str) -> Value {
        json!({
            "id": id, "identifier": identifier, "url": url,
            "title": "item 0", "description": "", "priority": 0, "dueDate": null,
            "team": { "id": "team-1" }, "project": { "id": "proj-1" },
            "assignee": null, "labels": { "nodes": [] },
        })
    }

    #[tokio::test]
    async fn retry_after_a_lost_response_records_the_existing_issue() {
        // The first create reaches Linear but the response is lost (500);
        // Linear has the issue under our client id. The retry's create is
        // rejected as a duplicate and the lookup by id finds it.
        let created_id: Arc<Mutex<Option<String>>> = Arc::default();
        let creates = Arc::new(AtomicUsize::new(0));
        let (cid, n) = (created_id.clone(), creates.clone());
        let m = mock(move |r| {
            let q = r.query();
            if q.contains("issueCreate") {
                let id = r.vars()["input"]["id"].as_str().unwrap().to_string();
                if n.fetch_add(1, Ordering::SeqCst) == 0 {
                    *cid.lock() = Some(id);
                    (500, json!({}))
                } else {
                    (
                        200,
                        json!({ "errors": [{ "message": "Entity already exists" }] }),
                    )
                }
            } else if q.contains("issue(id:") {
                let id = r.vars()["id"].as_str().unwrap();
                if cid.lock().as_deref() == Some(id) {
                    ok(json!({ "issue": stored_issue(id, "ENG-5", "u5") }))
                } else {
                    (
                        200,
                        json!({ "errors": [{ "message": "Entity not found" }] }),
                    )
                }
            } else {
                (500, json!({}))
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();

        // First attempt: the create fails, and the lookup (the issue does
        // exist on Linear's side here) recovers it right away.
        let first = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(first.results[0].result, Ok("ENG-5".into()));
        assert!(first.results[0].already_existed);
        let saved = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(saved.ticket_state, TicketState::Created);
        assert_eq!(
            saved.linear_issue_id.as_deref(),
            created_id.lock().as_deref()
        );
        assert_eq!(creates.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn crash_after_creating_state_reuses_the_client_id() {
        // A crash after "creating" was persisted: the item has a client id
        // but no issue. The retry sends the same id; Linear says it exists.
        let m = mock(|r| {
            let q = r.query();
            if q.contains("issueCreate") {
                (200, json!({ "errors": [{ "message": "duplicate key value violates unique constraint" }] }))
            } else if q.contains("issue(id:") {
                let id = r.vars()["id"].as_str().unwrap();
                ok(json!({ "issue": stored_issue(id, "ENG-8", "u8") }))
            } else {
                (500, json!({}))
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let ticket = store.get_action_item(id).unwrap().unwrap().ticket;
        let (before, _) = store.ensure_linear_client_id(id, &ticket).unwrap();
        // Idempotent: the same id comes back.
        assert_eq!(
            store.ensure_linear_client_id(id, &ticket).unwrap().0,
            before
        );

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-8".into()));
        assert!(out.results[0].already_existed);
        let sent = m
            .requests()
            .into_iter()
            .find(|r| r.query().contains("issueCreate"))
            .unwrap();
        assert_eq!(sent.vars()["input"]["id"], json!(before));
        assert_eq!(
            store.get_action_item(id).unwrap().unwrap().linear_issue_id,
            Some(before)
        );
    }

    #[tokio::test]
    async fn invalid_draft_gets_no_client_id() {
        let m = mock(|_| (500, json!({}))).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let mut draft = store.get_action_item(id).unwrap().unwrap().ticket;
        draft.team_id = Some(" ".into());
        store.update_ticket_draft(id, &draft).unwrap();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        let err = out.results[0].result.clone().unwrap_err();
        assert!(err.contains("choose a Linear team"), "{err}");
        assert!(m.requests().is_empty());
        assert_eq!(store.linear_client_id(id).unwrap(), None);
    }

    #[tokio::test]
    async fn unreachable_linear_leaves_no_client_id_or_baseline() {
        // Nothing listens on port 1: the connection is refused, so the
        // request was never sent and nothing may be recorded as sent.
        let s = LinearService::new(
            "http://127.0.0.1:1/graphql",
            KeySource::Fixed(Some("k".into())),
        );
        let (store, id) = store_with_approved();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        let err = out.results[0].result.clone().unwrap_err();
        assert!(err.contains("Could not reach Linear"), "{err}");
        assert_eq!(store.linear_client_id(id).unwrap(), None);
        let item = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(item.ticket_state, TicketState::Approved);

        // The next attempt (edited draft) is the first one sent: it is the
        // baseline, so the lost-response retry after it applies no edits.
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let mut edited = item.ticket;
        edited.title = "Sent title".into();
        store.update_ticket_draft(id, &edited).unwrap();
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        lookups_fail.store(false, Ordering::SeqCst);
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert_eq!(m.count("issueUpdate"), 0);
    }

    #[tokio::test]
    async fn a_timeout_after_sending_keeps_the_baseline() {
        let (m, _lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let ticket = store.get_action_item(id).unwrap().unwrap().ticket;
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        let client_id = store.linear_client_id(id).unwrap().unwrap();
        assert_eq!(
            store
                .ensure_linear_client_id(id, &TicketDraft::default())
                .unwrap(),
            (client_id, SentDraft::Earlier(ticket))
        );
    }

    #[tokio::test]
    async fn existing_issue_remembers_its_team_in_linear() {
        // The issue exists (an earlier attempt went through) and was moved to
        // another team/project in Linear: those are remembered, not the
        // draft's.
        let m = mock(|r| {
            let q = r.query();
            if q.contains("issueCreate") {
                (
                    200,
                    json!({ "errors": [{ "message": "Entity already exists" }] }),
                )
            } else if q.contains("issue(id:") {
                let mut issue = stored_issue("", "ENG-5", "u5");
                issue["id"] = r.vars()["id"].clone();
                issue["team"] = json!({ "id": "team-moved" });
                issue["project"] = Value::Null;
                ok(json!({ "issue": issue }))
            } else {
                (500, json!({}))
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert!(out.results[0].already_existed);
        assert!(m.requests()[1].query().contains("team { id }"));
        assert_eq!(out.used_team_project, Some(("team-moved".into(), None)));
    }

    #[tokio::test]
    async fn a_real_failure_keeps_the_client_id_for_the_retry() {
        let m = mock(|r| {
            if r.query().contains("issue(id:") {
                (
                    200,
                    json!({ "errors": [{ "message": "Entity not found" }] }),
                )
            } else {
                (
                    200,
                    json!({ "errors": [{ "message": "assignee is not a team member" }] }),
                )
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        let err = out.results[0].result.clone().unwrap_err();
        assert!(err.contains("assignee is not a team member"), "{err}");
        let item = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(item.linear_issue_id, None);
        assert_eq!(item.ticket_state, TicketState::Approved);
        assert!(store.linear_client_id(id).unwrap().is_some());
    }

    /// Linear for a create that reaches Linear but times out, with a lookup
    /// that fails until the returned flag is cleared. Later creates are
    /// rejected as duplicates; the lookup returns `issue`; updates succeed
    /// when `update_ok`.
    async fn timed_out_create(
        issue: Value,
        update_ok: bool,
    ) -> (Mock, Arc<std::sync::atomic::AtomicBool>) {
        let lookups_fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let fail = lookups_fail.clone();
        let creates = Arc::new(AtomicUsize::new(0));
        let m = mock(move |r| {
            let q = r.query();
            if q.contains("issueCreate") {
                if creates.fetch_add(1, Ordering::SeqCst) == 0 {
                    (504, json!({}))
                } else {
                    (
                        200,
                        json!({ "errors": [{ "message": "Entity already exists" }] }),
                    )
                }
            } else if q.contains("issue(id:") {
                if fail.load(Ordering::SeqCst) {
                    (503, json!({}))
                } else {
                    let mut issue = issue.clone();
                    issue["id"] = r.vars()["id"].clone();
                    ok(json!({ "issue": issue }))
                }
            } else if q.contains("issueUpdate") && update_ok {
                let id = r.vars()["id"].as_str().unwrap();
                ok(json!({ "issueUpdate": { "success": true, "issue": {
                    "id": id, "identifier": "ENG-5", "url": "u5" } } }))
            } else {
                (
                    200,
                    json!({ "errors": [{ "message": "assignee is not a team member" }] }),
                )
            }
        })
        .await;
        (m, lookups_fail)
    }

    /// [`stored_issue`] after changes made in Linear: a teammate assigned
    /// it, an automation added a label and moved it to another project, and
    /// Linear normalised the description.
    fn issue_changed_in_linear() -> Value {
        json!({
            "id": "", "identifier": "ENG-5", "url": "u5",
            "title": "item 0", "description": "Normalised\n", "priority": 0, "dueDate": null,
            "team": { "id": "team-1" }, "project": { "id": "proj-other" },
            "assignee": { "id": "u-teammate" }, "labels": { "nodes": [{ "id": "l-auto" }] },
        })
    }

    fn update_input(m: &Mock) -> Option<Value> {
        m.requests()
            .into_iter()
            .find(|r| r.query().contains("issueUpdate"))
            .map(|r| r.vars()["input"].clone())
    }

    #[tokio::test]
    async fn retry_after_an_edit_applies_only_the_edited_fields() {
        // The first create reaches Linear but times out, and the lookup fails
        // too. The user then edits the draft and retries: the issue that
        // exists gets the user's edits, and nothing else.
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();

        let first = s.create_tickets(&store, &[id]).await.unwrap();
        assert!(first.results[0].result.is_err());
        let client_id = store.linear_client_id(id).unwrap().unwrap();

        let mut edited = store.get_action_item(id).unwrap().unwrap().ticket;
        edited.title = "Edited title".into();
        edited.description = "More detail".into();
        edited.label_ids = vec!["l-mine".into()];
        edited.due_date = Some("2026-11-02".into());
        store.update_ticket_draft(id, &edited).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert!(out.results[0].already_existed);
        assert_eq!(m.count("issueUpdate"), 1);
        let update = m
            .requests()
            .into_iter()
            .find(|r| r.query().contains("issueUpdate"))
            .unwrap();
        assert_eq!(update.vars()["id"], json!(client_id));
        // Only what the user changed; the project and assignee set in Linear
        // and its label are left alone.
        assert_eq!(
            update.vars()["input"],
            json!({
                "title": "Edited title",
                "description": "More detail",
                "dueDate": "2026-11-02",
                "addedLabelIds": ["l-mine"],
            })
        );
        let saved = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(saved.ticket_state, TicketState::Created);
        assert_eq!(saved.linear_issue_identifier.as_deref(), Some("ENG-5"));
    }

    #[tokio::test]
    async fn retry_after_a_restart_applies_only_the_edited_fields() {
        // The create times out and the app quits. After a restart (new
        // service and store instances on the same database) the user edits
        // the draft and retries: only that edit reaches the issue.
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(crate::meetings::store::MEETINGS_DB_FILE);
        let id = {
            let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
            let (store, id) = add_approved(MeetingsStore::open(&path).unwrap());
            let first = s.create_tickets(&store, &[id]).await.unwrap();
            assert!(first.results[0].result.is_err());
            id
        };

        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let store = MeetingsStore::open(&path).unwrap();
        let mut edited = store.get_action_item(id).unwrap().unwrap().ticket;
        edited.title = "Edited after restart".into();
        store.update_ticket_draft(id, &edited).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert!(out.results[0].already_existed);
        assert_eq!(
            update_input(&m),
            Some(json!({ "title": "Edited after restart" }))
        );
        let saved = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(saved.ticket_state, TicketState::Created);
    }

    #[tokio::test]
    async fn unedited_retry_after_a_restart_sends_no_update() {
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(crate::meetings::store::MEETINGS_DB_FILE);
        let id = {
            let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
            let (store, id) = add_approved(MeetingsStore::open(&path).unwrap());
            assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
                .result
                .is_err());
            id
        };
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let store = MeetingsStore::open(&path).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert_eq!(m.count("issueUpdate"), 0);
    }

    #[tokio::test]
    async fn emptied_fields_are_cleared_and_removed_labels_removed() {
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let mut draft = store.get_action_item(id).unwrap().unwrap().ticket;
        draft.label_ids = vec!["l-1".into(), "l-2".into()];
        draft.assignee_id = Some("u-1".into());
        store.update_ticket_draft(id, &draft).unwrap();

        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        draft.label_ids = vec!["l-2".into()];
        draft.project_id = None;
        store.update_ticket_draft(id, &draft).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert_eq!(
            update_input(&m),
            Some(json!({ "projectId": null, "removedLabelIds": ["l-1"] }))
        );
    }

    #[tokio::test]
    async fn unedited_retry_keeps_changes_made_in_linear() {
        // The issue was changed in Linear after the lost create; the draft
        // wasn't edited, so nothing is sent back.
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert!(out.results[0].already_existed);
        assert_eq!(m.count("issueUpdate"), 0);
    }

    #[tokio::test]
    async fn failed_update_of_an_edited_issue_is_reported_and_retryable() {
        let (m, lookups_fail) = timed_out_create(stored_issue("", "ENG-5", "u5"), false).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        let mut edited = store.get_action_item(id).unwrap().unwrap().ticket;
        edited.assignee_id = Some("u-x".into());
        store.update_ticket_draft(id, &edited).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        let err = out.results[0].result.clone().unwrap_err();
        assert!(err.contains("ENG-5 already exists"), "{err}");
        assert!(err.contains("assignee is not a team member"), "{err}");
        assert_eq!(update_input(&m), Some(json!({ "assigneeId": "u-x" })));
        let item = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(item.linear_issue_id, None);
        assert_eq!(item.ticket_state, TicketState::Approved);

        // The next retry still compares against the first attempt's draft.
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert!(out.results[0].result.is_err());
        assert_eq!(m.count("issueUpdate"), 2);
    }

    #[tokio::test]
    async fn unknown_sent_draft_applies_the_drafts_fields_without_clearing() {
        // The first attempt predates recording the sent draft (or its record
        // is unreadable): the retry can't tell what the user edited, so the
        // draft's non-empty fields are applied and its labels added, and
        // what it leaves empty is not cleared in Linear.
        let (m, lookups_fail) = timed_out_create(issue_changed_in_linear(), true).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        store.forget_linear_sent_ticket(id).unwrap();
        let mut edited = store.get_action_item(id).unwrap().unwrap().ticket;
        edited.title = "Edited title".into();
        edited.project_id = None;
        edited.label_ids = vec!["l-mine".into()];
        store.update_ticket_draft(id, &edited).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert!(out.results[0].already_existed);
        assert_eq!(
            update_input(&m),
            Some(json!({
                "title": "Edited title",
                "addedLabelIds": ["l-mine"],
            }))
        );
        let saved = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(saved.ticket_state, TicketState::Created);
    }

    #[tokio::test]
    async fn failed_update_with_an_unknown_sent_draft_is_retryable() {
        let (m, lookups_fail) = timed_out_create(stored_issue("", "ENG-5", "u5"), false).await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        assert!(s.create_tickets(&store, &[id]).await.unwrap().results[0]
            .result
            .is_err());
        store.forget_linear_sent_ticket(id).unwrap();
        lookups_fail.store(false, Ordering::SeqCst);

        let out = s.create_tickets(&store, &[id]).await.unwrap();
        let err = out.results[0].result.clone().unwrap_err();
        assert!(err.contains("ENG-5 already exists"), "{err}");
        let item = store.get_action_item(id).unwrap().unwrap();
        assert_eq!(item.linear_issue_id, None);
        // Still unknown: the next retry applies the draft again.
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert!(out.results[0].result.is_err());
        assert_eq!(m.count("issueUpdate"), 2);
    }

    #[tokio::test]
    async fn existing_issue_without_an_earlier_attempt_is_not_updated() {
        // No earlier attempt is recorded (e.g. it was made before the sent
        // draft was persisted): what this attempt sent is the baseline, so
        // the issue is left as Linear has it.
        let m = mock(|r| {
            let q = r.query();
            if q.contains("issueCreate") {
                (
                    200,
                    json!({ "errors": [{ "message": "Entity already exists" }] }),
                )
            } else if q.contains("issue(id:") {
                let mut issue = issue_changed_in_linear();
                issue["id"] = r.vars()["id"].clone();
                ok(json!({ "issue": issue }))
            } else {
                (500, json!({}))
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let out = s.create_tickets(&store, &[id]).await.unwrap();
        assert_eq!(out.results[0].result, Ok("ENG-5".into()));
        assert_eq!(m.count("issueUpdate"), 0);
    }
}
