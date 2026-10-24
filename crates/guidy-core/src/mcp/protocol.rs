//! MCP JSON-RPC handling and the meeting tools. Pure: everything it reads
//! comes through [`MeetingsSource`], so it is tested with a fake.

use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};

use crate::meetings::summarize::TranscriptLine;
use crate::meetings::types::{ActionItem, Meeting, MeetingId, StoredSummary};

/// Newest first; the first one is answered when a client asks for a version
/// we don't know.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const DEFAULT_LIST_LIMIT: usize = 20;
const MAX_LIST_LIMIT: usize = 200;

/// What the tools read. The real one is the meetings manager
/// ([`super::ManagerSource`]).
pub trait MeetingsSource: Send + Sync {
    /// Every meeting, newest first.
    fn meetings(&self) -> Result<Vec<Meeting>>;
    fn meeting(&self, id: MeetingId) -> Result<Option<Meeting>>;
    /// Speaker-attributed lines in time order.
    fn transcript(&self, id: MeetingId) -> Result<Vec<TranscriptLine>>;
    fn summary(&self, id: MeetingId) -> Result<Option<StoredSummary>>;
    fn action_items(&self, id: MeetingId) -> Result<Vec<ActionItem>>;
}

/// Resolves the source per request: meeting notes may start (or be turned
/// on) after the server did. `None`: not available.
pub type SourceFn = Arc<dyn Fn() -> Option<Arc<dyn MeetingsSource>> + Send + Sync>;

/// Handle one JSON-RPC message (or a batch). `None` when nothing is to be
/// sent back (notifications and responses only).
pub fn handle(message: &Value, source: &SourceFn) -> Option<Value> {
    match message {
        Value::Array(batch) => {
            let out: Vec<Value> = batch.iter().filter_map(|m| handle_one(m, source)).collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        m => handle_one(m, source),
    }
}

fn handle_one(message: &Value, source: &SourceFn) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response (we never send requests) or garbage without an id.
        return message
            .get("id")
            .filter(|_| message.get("result").is_none() && message.get("error").is_none())
            .map(|id| error(id.clone(), -32600, "Invalid request"));
    };
    // Notifications get no answer.
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => success(id, initialize(&params)),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tools() })),
        "tools/call" => match call_tool(&params, source) {
            Ok(result) => success(id, result),
            Err(ToolError::Protocol(code, msg)) => error(id, code, &msg),
        },
        "resources/list" => success(id, json!({ "resources": [] })),
        "prompts/list" => success(id, json!({ "prompts": [] })),
        _ => error(id, -32601, &format!("Method not found: {method}")),
    })
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "guidy",
            "title": "Guidy meeting notes",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Read the user's meeting notes recorded by Guidy: list meetings, \
            then fetch a meeting's details (summary, decisions, action items) or its \
            full speaker-attributed transcript by id. Times are local, ISO 8601.",
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "list_meetings",
            "title": "List meetings",
            "description": "List recorded meetings, newest first, with id, title, start time, \
                duration, status and attendees. Use the id with get_meeting or get_transcript.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Only meetings whose title or attendees contain this text (case-insensitive)."
                    },
                    "since": {
                        "type": "string",
                        "description": "Only meetings starting on or after this date (YYYY-MM-DD, local time)."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_LIST_LIMIT,
                        "description": "Maximum meetings to return (default 20)."
                    }
                }
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "get_meeting",
            "title": "Get meeting",
            "description": "A meeting's details: time, attendees, AI summary, decisions, topics \
                and action items (when it has been summarized).",
            "inputSchema": {
                "type": "object",
                "properties": { "meeting_id": { "type": "integer" } },
                "required": ["meeting_id"]
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "get_transcript",
            "title": "Get transcript",
            "description": "A meeting's full transcript, one \"[mm:ss] Speaker: text\" line per \
                utterance. Empty until the meeting has been processed.",
            "inputSchema": {
                "type": "object",
                "properties": { "meeting_id": { "type": "integer" } },
                "required": ["meeting_id"]
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }
    ])
}

enum ToolError {
    Protocol(i64, String),
}

/// Run a tool. Failures inside a known tool are results with `isError`, so
/// the model sees them; unknown tools and bad params are protocol errors.
fn call_tool(params: &Value, source: &SourceFn) -> Result<Value, ToolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Protocol(-32602, "Missing tool name".into()))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let run: fn(&dyn MeetingsSource, &Value) -> Result<Value> = match name {
        "list_meetings" => list_meetings,
        "get_meeting" => get_meeting,
        "get_transcript" => get_transcript,
        _ => return Err(ToolError::Protocol(-32602, format!("Unknown tool: {name}"))),
    };
    let Some(src) = source() else {
        return Ok(tool_error(
            "Meeting notes aren't available: turn them on in Guidy's settings.",
        ));
    };
    Ok(match run(src.as_ref(), &args) {
        Ok(structured) => {
            let text = match &structured {
                Value::Object(o) if o.contains_key("transcript") => {
                    // Plain text reads far better than escaped JSON lines.
                    let header = json!({
                        "meeting_id": o.get("meeting_id"),
                        "title": o.get("title"),
                    });
                    format!(
                        "{header}\n\n{}",
                        o["transcript"].as_str().unwrap_or_default()
                    )
                }
                v => serde_json::to_string_pretty(v).unwrap_or_default(),
            };
            json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": structured,
                "isError": false,
            })
        }
        Err(e) => tool_error(&format!("{e:#}")),
    })
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn meeting_id_arg(args: &Value) -> Result<MeetingId> {
    let v = &args["meeting_id"];
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("meeting_id (an integer) is required"))
}

fn local_iso(ms: i64) -> Value {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| Value::String(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)))
        .unwrap_or(Value::Null)
}

fn attendees(m: &Meeting) -> Vec<String> {
    m.calendar_event
        .iter()
        .flat_map(|e| &e.attendees)
        .filter_map(|a| a.name.clone().or_else(|| a.email.clone()))
        .filter(|n| !n.trim().is_empty())
        .collect()
}

fn meeting_json(m: &Meeting) -> Value {
    json!({
        "id": m.id,
        "title": m.title,
        "started_at": local_iso(m.started_at),
        "ended_at": m.ended_at.map(local_iso),
        "duration_minutes": m.duration_ms().map(|d| (d as f64 / 60_000.0 * 10.0).round() / 10.0),
        "status": m.status.as_str(),
        "attendees": attendees(m),
    })
}

/// Local midnight of `YYYY-MM-DD`, in unix ms.
fn parse_since(s: &str) -> Result<i64> {
    use chrono::TimeZone;
    let date = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("since must be a date like 2026-03-01"))?;
    let midnight = date.and_hms_opt(0, 0, 0).expect("valid time");
    chrono::Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|t| t.timestamp_millis())
        .ok_or_else(|| anyhow::anyhow!("since is not a valid local date"))
}

fn list_meetings(src: &dyn MeetingsSource, args: &Value) -> Result<Value> {
    let limit = args["limit"]
        .as_u64()
        .map_or(DEFAULT_LIST_LIMIT, |n| n as usize)
        .clamp(1, MAX_LIST_LIMIT);
    let query = args["query"]
        .as_str()
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty());
    let since = args["since"].as_str().map(parse_since).transpose()?;
    let mut meetings = src.meetings()?;
    meetings.sort_by_key(|m| std::cmp::Reverse(m.started_at));
    let matching: Vec<Value> = meetings
        .iter()
        .filter(|m| since.is_none_or(|t| m.started_at >= t))
        .filter(|m| {
            query.as_ref().is_none_or(|q| {
                m.title.to_lowercase().contains(q)
                    || attendees(m).iter().any(|a| a.to_lowercase().contains(q))
            })
        })
        .map(meeting_json)
        .collect();
    let total = matching.len();
    Ok(json!({
        "meetings": matching.into_iter().take(limit).collect::<Vec<_>>(),
        "total": total,
    }))
}

fn get_meeting(src: &dyn MeetingsSource, args: &Value) -> Result<Value> {
    let id = meeting_id_arg(args)?;
    let m = src
        .meeting(id)?
        .ok_or_else(|| anyhow::anyhow!("No meeting with id {id}"))?;
    let mut out = meeting_json(&m);
    let summary = src.summary(id)?;
    out["summary"] = summary.as_ref().map_or(Value::Null, |s| {
        json!({
            "title": s.title,
            "short_summary": s.short_summary,
            "decisions": s.decisions,
            "topics": s.topics,
        })
    });
    out["action_items"] = src
        .action_items(id)?
        .into_iter()
        .map(|a| {
            json!({
                "text": a.text,
                "owner": a.owner_name,
                "owner_is_user": a.owner_is_user,
                "due": a.due,
                "linear_issue": a.linear_issue_url,
            })
        })
        .collect();
    Ok(out)
}

fn get_transcript(src: &dyn MeetingsSource, args: &Value) -> Result<Value> {
    let id = meeting_id_arg(args)?;
    let m = src
        .meeting(id)?
        .ok_or_else(|| anyhow::anyhow!("No meeting with id {id}"))?;
    let lines = src.transcript(id)?;
    let transcript = if lines.is_empty() {
        format!("(no transcript yet: the meeting is {})", m.status.as_str())
    } else {
        lines
            .iter()
            .map(TranscriptLine::render)
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(json!({
        "meeting_id": id,
        "title": m.title,
        "started_at": local_iso(m.started_at),
        "transcript": transcript,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::types::{Attendee, CalendarEvent, MeetingStatus};

    struct Fake(Vec<Meeting>);

    impl MeetingsSource for Fake {
        fn meetings(&self) -> Result<Vec<Meeting>> {
            Ok(self.0.clone())
        }
        fn meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
            Ok(self.0.iter().find(|m| m.id == id).cloned())
        }
        fn transcript(&self, id: MeetingId) -> Result<Vec<TranscriptLine>> {
            Ok(if id == 1 {
                vec![
                    TranscriptLine {
                        t0_ms: 5_000,
                        speaker_id: None,
                        speaker: "Imran".into(),
                        text: "Let's ship it.".into(),
                    },
                    TranscriptLine {
                        t0_ms: 65_000,
                        speaker_id: None,
                        speaker: "Ada".into(),
                        text: "Agreed.".into(),
                    },
                ]
            } else {
                vec![]
            })
        }
        fn summary(&self, _: MeetingId) -> Result<Option<StoredSummary>> {
            Ok(None)
        }
        fn action_items(&self, _: MeetingId) -> Result<Vec<ActionItem>> {
            Ok(vec![])
        }
    }

    fn meeting(id: MeetingId, title: &str, started_at: i64, attendee: &str) -> Meeting {
        Meeting {
            id,
            title: title.into(),
            calendar_event_id: None,
            calendar_event: Some(CalendarEvent {
                id: "e".into(),
                title: title.into(),
                start_at: started_at,
                end_at: started_at,
                meeting_url: None,
                attendees: vec![Attendee {
                    name: Some(attendee.into()),
                    email: None,
                    is_current_user: false,
                }],
            }),
            started_at,
            ended_at: Some(started_at + 30 * 60_000),
            mic_audio_path: None,
            system_audio_path: None,
            status: MeetingStatus::Processed,
            error: None,
            processing_stage: None,
            description: None,
            title_source: Default::default(),
            has_summary: false,
        }
    }

    fn source() -> SourceFn {
        let fake: Arc<dyn MeetingsSource> = Arc::new(Fake(vec![
            meeting(1, "Weekly sync", 1_000_000, "Ada Lovelace"),
            meeting(2, "Design review", 2_000_000, "Grace Hopper"),
        ]));
        Arc::new(move || Some(fake.clone()))
    }

    fn call(method: &str, params: Value) -> Value {
        handle(
            &json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}),
            &source(),
        )
        .expect("a response")
    }

    fn tool(name: &str, args: Value) -> Value {
        call("tools/call", json!({"name": name, "arguments": args}))["result"].clone()
    }

    #[test]
    fn initialize_negotiates_the_version() {
        let r = call("initialize", json!({"protocolVersion": "2025-03-26"}));
        assert_eq!(r["id"], 7);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = call("initialize", json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let n = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert_eq!(handle(&n, &source()), None);
        let r = json!({"jsonrpc": "2.0", "id": 1, "result": {}});
        assert_eq!(handle(&r, &source()), None);
        assert_eq!(handle(&json!([n]), &source()), None);
    }

    #[test]
    fn unknown_methods_and_tools_are_errors() {
        assert_eq!(call("nope", json!({}))["error"]["code"], -32601);
        let r = call("tools/call", json!({"name": "nope"}));
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn lists_the_three_tools() {
        let r = call("tools/list", json!({}));
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list_meetings", "get_meeting", "get_transcript"]);
    }

    #[test]
    fn list_meetings_is_newest_first_and_filters() {
        let r = tool("list_meetings", json!({}));
        assert_eq!(r["isError"], false);
        let ids: Vec<i64> = r["structuredContent"]["meetings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids, [2, 1]);
        assert_eq!(
            r["structuredContent"]["meetings"][0]["duration_minutes"],
            30.0
        );

        let r = tool("list_meetings", json!({"query": "ada"}));
        assert_eq!(r["structuredContent"]["total"], 1);
        assert_eq!(r["structuredContent"]["meetings"][0]["id"], 1);

        let r = tool("list_meetings", json!({"limit": 1}));
        assert_eq!(
            r["structuredContent"]["meetings"].as_array().unwrap().len(),
            1
        );
        assert_eq!(r["structuredContent"]["total"], 2);

        let r = tool("list_meetings", json!({"since": "not a date"}));
        assert_eq!(r["isError"], true);
    }

    #[test]
    fn transcript_is_rendered_as_lines() {
        let r = tool("get_transcript", json!({"meeting_id": 1}));
        let text = r["structuredContent"]["transcript"].as_str().unwrap();
        assert_eq!(text, "[00:05] Imran: Let's ship it.\n[01:05] Ada: Agreed.");
        assert!(r["content"][0]["text"].as_str().unwrap().ends_with(text));
        // String ids work too; a missing meeting is a tool error.
        let r = tool("get_transcript", json!({"meeting_id": "2"}));
        assert!(r["structuredContent"]["transcript"]
            .as_str()
            .unwrap()
            .starts_with("(no transcript yet"));
        assert_eq!(
            tool("get_transcript", json!({"meeting_id": 9}))["isError"],
            true
        );
        assert_eq!(tool("get_transcript", json!({}))["isError"], true);
    }

    #[test]
    fn get_meeting_includes_summary_and_action_items() {
        let r = tool("get_meeting", json!({"meeting_id": 2}));
        let m = &r["structuredContent"];
        assert_eq!(m["title"], "Design review");
        assert_eq!(m["attendees"], json!(["Grace Hopper"]));
        assert!(m["summary"].is_null());
        assert_eq!(m["action_items"], json!([]));
    }

    #[test]
    fn unavailable_meetings_are_a_tool_error() {
        let none: SourceFn = Arc::new(|| None);
        let r = handle(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "list_meetings"}}),
            &none,
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
    }
}
