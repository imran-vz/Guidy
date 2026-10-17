//! Async GraphQL client for Linear, authenticated with a personal API key.
//!
//! Personal keys go in the `Authorization` header as is (no `Bearer`). The
//! base URL is overridable so tests can point the client at a local mock.

use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{Label, Project, Team, User};
use crate::meetings::types::TicketDraft;

pub const DEFAULT_ENDPOINT: &str = "https://api.linear.app/graphql";

/// Page size for every paginated query.
pub const PAGE_SIZE: u32 = 250;

/// Safety cap on pages per list (250 * 40 = 10k nodes).
const MAX_PAGES: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearError {
    /// No API key is stored.
    MissingApiKey,
    /// HTTP 401 or a GraphQL authentication error.
    Unauthorized,
    /// HTTP 429 or a GraphQL rate-limit error.
    RateLimited,
    /// GraphQL `errors` (user-presentable message when Linear provides one).
    GraphQl(String),
    /// A non-success HTTP status without a GraphQL error body.
    Http(u16, String),
    /// Connection / timeout errors.
    Network(String),
    /// The response didn't have the expected shape.
    Decode(String),
}

impl fmt::Display for LinearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => write!(f, "No Linear API key. Add one in Settings."),
            Self::Unauthorized => write!(
                f,
                "Linear rejected the API key. Check that it is a valid personal API key."
            ),
            Self::RateLimited => write!(f, "Linear rate limit reached. Try again in a minute."),
            Self::GraphQl(m) => write!(f, "Linear error: {m}"),
            Self::Http(code, body) if body.is_empty() => write!(f, "Linear returned HTTP {code}"),
            Self::Http(code, body) => write!(f, "Linear returned HTTP {code}: {body}"),
            Self::Network(m) => write!(f, "Could not reach Linear: {m}"),
            Self::Decode(m) => write!(f, "Unexpected response from Linear: {m}"),
        }
    }
}

impl std::error::Error for LinearError {}

pub type LinearResult<T> = std::result::Result<T, LinearError>;

/// The authenticated user (`viewer`).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

/// A created issue.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CreatedIssue {
    pub id: String,
    pub identifier: String,
    pub url: String,
}

#[derive(Clone)]
pub struct LinearClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
}

impl fmt::Debug for LinearClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinearClient")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    nodes: Vec<T>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserNode {
    id: String,
    name: String,
    display_name: String,
    email: Option<String>,
    #[serde(default = "yes")]
    active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LabelNode {
    id: String,
    name: String,
    #[serde(default)]
    is_group: bool,
}

fn yes() -> bool {
    true
}

const PAGE_INFO: &str = "pageInfo { hasNextPage endCursor }";

impl LinearClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_endpoint(api_key, DEFAULT_ENDPOINT)
    }

    pub fn with_endpoint(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("Guidy/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            endpoint: endpoint.into(),
            api_key: api_key.into().trim().to_string(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Run one GraphQL operation and return its `data`.
    pub async fn graphql(&self, query: &str, variables: Value) -> LinearResult<Value> {
        if self.api_key.is_empty() {
            return Err(LinearError::MissingApiKey);
        }
        let resp = self
            .http
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, &self.api_key)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|e| LinearError::Network(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LinearError::Unauthorized);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(LinearError::RateLimited);
        }
        let text = resp
            .text()
            .await
            .map_err(|e| LinearError::Network(e.to_string()))?;
        let body: Option<Value> = serde_json::from_str(&text).ok();
        if let Some(err) = body.as_ref().and_then(graphql_error) {
            return Err(err);
        }
        if !status.is_success() {
            let snippet: String = text.chars().take(200).collect();
            return Err(LinearError::Http(
                status.as_u16(),
                snippet.trim().to_string(),
            ));
        }
        let mut body = body.ok_or_else(|| LinearError::Decode("response is not JSON".into()))?;
        match body.get_mut("data").map(Value::take) {
            Some(data) if !data.is_null() => Ok(data),
            _ => Err(LinearError::Decode("response has no data".into())),
        }
    }

    async fn query_at<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
        path: &[&str],
    ) -> LinearResult<T> {
        let data = self.graphql(query, variables).await?;
        let v = pointer(&data, path)?;
        serde_json::from_value(v.clone()).map_err(|e| LinearError::Decode(e.to_string()))
    }

    /// Fetch every node of a connection at `path`, 250 per page. The query
    /// must take `$first: Int!` and `$after: String`.
    async fn paginate<T: DeserializeOwned>(
        &self,
        query: &str,
        mut variables: Value,
        path: &[&str],
    ) -> LinearResult<Vec<T>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_PAGES {
            variables["first"] = json!(PAGE_SIZE);
            variables["after"] = json!(after);
            let page: Connection<T> = self.query_at(query, variables.clone(), path).await?;
            out.extend(page.nodes);
            match (page.page_info.has_next_page, page.page_info.end_cursor) {
                (true, Some(c)) => after = Some(c),
                _ => return Ok(out),
            }
        }
        log::warn!("Linear: stopped paginating {path:?} after {MAX_PAGES} pages");
        Ok(out)
    }

    pub async fn viewer(&self) -> LinearResult<Viewer> {
        self.query_at("query { viewer { id name email } }", json!({}), &["viewer"])
            .await
    }

    pub async fn teams(&self) -> LinearResult<Vec<Team>> {
        let q = format!(
            "query Teams($first: Int!, $after: String) {{ teams(first: $first, after: $after) {{ nodes {{ id key name }} {PAGE_INFO} }} }}"
        );
        let mut teams: Vec<Team> = self.paginate(&q, json!({}), &["teams"]).await?;
        teams.sort_by_key(|t| t.name.to_lowercase());
        Ok(teams)
    }

    pub async fn projects(&self, team_id: &str) -> LinearResult<Vec<Project>> {
        let q = format!(
            "query TeamProjects($teamId: String!, $first: Int!, $after: String) {{ team(id: $teamId) {{ projects(first: $first, after: $after) {{ nodes {{ id name }} {PAGE_INFO} }} }} }}"
        );
        let mut projects: Vec<Project> = self
            .paginate(&q, json!({ "teamId": team_id }), &["team", "projects"])
            .await?;
        projects.sort_by_key(|p| p.name.to_lowercase());
        Ok(projects)
    }

    /// Active users of the workspace.
    pub async fn users(&self) -> LinearResult<Vec<User>> {
        let q = format!(
            "query Users($first: Int!, $after: String) {{ users(first: $first, after: $after) {{ nodes {{ id name displayName email active }} {PAGE_INFO} }} }}"
        );
        let nodes: Vec<UserNode> = self.paginate(&q, json!({}), &["users"]).await?;
        let mut users: Vec<User> = nodes
            .into_iter()
            .filter(|u| u.active)
            .map(|u| User {
                id: u.id,
                name: u.name,
                display_name: u.display_name,
                email: u.email,
            })
            .collect();
        users.sort_by_key(|u| u.name.to_lowercase());
        Ok(users)
    }

    /// The team's labels plus workspace-wide labels. Label groups are left
    /// out (they can't be applied to an issue).
    pub async fn labels(&self, team_id: &str) -> LinearResult<Vec<Label>> {
        let team_q = format!(
            "query TeamLabels($teamId: String!, $first: Int!, $after: String) {{ team(id: $teamId) {{ labels(first: $first, after: $after) {{ nodes {{ id name isGroup }} {PAGE_INFO} }} }} }}"
        );
        let ws_q = format!(
            "query WorkspaceLabels($first: Int!, $after: String) {{ issueLabels(first: $first, after: $after, filter: {{ team: {{ null: true }} }}) {{ nodes {{ id name isGroup }} {PAGE_INFO} }} }}"
        );
        let mut nodes: Vec<LabelNode> = self
            .paginate(&team_q, json!({ "teamId": team_id }), &["team", "labels"])
            .await?;
        nodes.extend(
            self.paginate::<LabelNode>(&ws_q, json!({}), &["issueLabels"])
                .await?,
        );
        let mut seen = std::collections::HashSet::new();
        let mut labels: Vec<Label> = nodes
            .into_iter()
            .filter(|l| !l.is_group && seen.insert(l.id.clone()))
            .map(|l| Label {
                id: l.id,
                name: l.name,
            })
            .collect();
        labels.sort_by_key(|l| l.name.to_lowercase());
        Ok(labels)
    }

    pub async fn create_issue(&self, draft: &TicketDraft) -> LinearResult<CreatedIssue> {
        let input = issue_create_input(draft).map_err(LinearError::GraphQl)?;
        let q = "mutation IssueCreate($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier url } } }";
        let data = self.graphql(q, json!({ "input": input })).await?;
        let payload = pointer(&data, &["issueCreate"])?;
        if payload.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(LinearError::GraphQl("the issue was not created".into()));
        }
        let issue = pointer(payload, &["issue"])?;
        serde_json::from_value(issue.clone()).map_err(|e| LinearError::Decode(e.to_string()))
    }
}

/// Build `IssueCreateInput` from a draft. Empty optional fields are omitted.
pub fn issue_create_input(draft: &TicketDraft) -> std::result::Result<Value, String> {
    let team_id = draft
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("choose a Linear team for this ticket")?;
    let title = draft.title.trim();
    if title.is_empty() {
        return Err("the ticket needs a title".into());
    }
    let mut input = json!({ "teamId": team_id, "title": title });
    let non_empty = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if !draft.description.trim().is_empty() {
        input["description"] = json!(draft.description);
    }
    if let Some(p) = non_empty(&draft.project_id) {
        input["projectId"] = json!(p);
    }
    if let Some(a) = non_empty(&draft.assignee_id) {
        input["assigneeId"] = json!(a);
    }
    if let Some(p) = draft.priority.filter(|p| *p <= 4) {
        input["priority"] = json!(p);
    }
    if !draft.label_ids.is_empty() {
        input["labelIds"] = json!(draft.label_ids);
    }
    if let Some(d) = non_empty(&draft.due_date) {
        input["dueDate"] = json!(d);
    }
    Ok(input)
}

fn pointer<'a>(data: &'a Value, path: &[&str]) -> LinearResult<&'a Value> {
    let mut v = data;
    for key in path {
        v = v
            .get(*key)
            .filter(|v| !v.is_null())
            .ok_or_else(|| LinearError::Decode(format!("missing `{}`", path.join("."))))?;
    }
    Ok(v)
}

/// Map a GraphQL `errors` array to a [`LinearError`], if there is one.
fn graphql_error(body: &Value) -> Option<LinearError> {
    let errors = body.get("errors")?.as_array()?;
    let first = errors.first()?;
    let ext = first.get("extensions");
    let code = ext
        .and_then(|e| e.get("code").or_else(|| e.get("type")))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if code.contains("authentication") || code.contains("unauthenticated") {
        return Some(LinearError::Unauthorized);
    }
    if code.contains("ratelimit") || code.contains("rate limit") {
        return Some(LinearError::RateLimited);
    }
    let msg = ext
        .and_then(|e| e.get("userPresentableMessage"))
        .and_then(Value::as_str)
        .or_else(|| first.get("message").and_then(Value::as_str))
        .unwrap_or("unknown error");
    Some(LinearError::GraphQl(msg.to_string()))
}
