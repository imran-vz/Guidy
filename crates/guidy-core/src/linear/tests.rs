//! Client tests against a local mock GraphQL server.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::client::{issue_create_input, LinearClient, LinearError};
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
    let issue = c.create_issue(&draft).await.unwrap();
    assert_eq!(issue.identifier, "ENG-12");
    assert_eq!(issue.url, "https://linear.app/a/issue/ENG-12");
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
        fn begin_create(&self, id: ActionItemId) -> anyhow::Result<String> {
            self.items.begin_create(id)
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
        let store = MeetingsStore::open_in_memory(&[3u8; 32]).unwrap();
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
        let store = MeetingsStore::open_in_memory(&[4u8; 32]).unwrap();
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
            })
            .unwrap();
        let mut item = approved_item(0);
        item.meeting_id = meeting_id;
        let item = store.insert_action_item(&item).unwrap();
        (store, item.id)
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
                    ok(json!({ "issue": { "id": id, "identifier": "ENG-5", "url": "u5" } }))
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
                ok(json!({ "issue": { "id": id, "identifier": "ENG-8", "url": "u8" } }))
            } else {
                (500, json!({}))
            }
        })
        .await;
        let s = LinearService::new(&m.url, KeySource::Fixed(Some("k".into())));
        let (store, id) = store_with_approved();
        let before = store.ensure_linear_client_id(id).unwrap();
        // Idempotent: the same id comes back.
        assert_eq!(store.ensure_linear_client_id(id).unwrap(), before);

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
}
