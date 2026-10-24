//! Local MCP server: lets other apps on this Mac (Claude, Cursor, ...) read
//! the meeting list, summaries and transcripts.
//!
//! Off by default (`meetings.mcp_enabled`). When on, it serves MCP's
//! Streamable HTTP transport (JSON responses, no SSE) at
//! `http://127.0.0.1:<meetings.mcp_port>/mcp`, on loopback only. Browser
//! requests from other origins and foreign `Host` headers are refused, so a
//! web page can't read meetings through DNS rebinding. Read-only: no tool
//! changes anything.
//!
//! Clients that only speak stdio run `guidy --mcp-stdio`, which relays
//! stdin/stdout to the running app's server ([`run_stdio_bridge`]).

pub mod protocol;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use parking_lot::Mutex;
use serde_json::Value;

use crate::host::AppHandle;
use crate::meetings::manager::MeetingsManager;
use crate::meetings::summarize::{self, TranscriptLine};
use crate::meetings::types::{ActionItem, Meeting, MeetingId, StoredSummary};
use protocol::{MeetingsSource, SourceFn};

pub const DEFAULT_PORT: u16 = 47_823;
pub const PATH: &str = "/mcp";
const MAX_BODY: usize = 1 << 20;
const MAX_HEADER_LINES: usize = 100;

/// The endpoint clients connect to.
pub fn endpoint_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// Managed state: the running server, if any.
#[derive(Default)]
pub struct McpServerState(Mutex<Option<Server>>);

/// Start, stop or move the server to match the settings. Cheap and
/// idempotent; call after every meetings settings change.
pub fn sync(app: &AppHandle) {
    let settings = crate::settings::get_settings(app).meetings;
    let Some(state) = app.try_state::<McpServerState>() else {
        return;
    };
    let mut current = state.0.lock();
    let want = settings.mcp_enabled.then_some(settings.mcp_port);
    if current.as_ref().map(|s| s.port) == want {
        return;
    }
    // Dropping stops it.
    *current = None;
    let Some(port) = want else {
        log::info!("mcp: server stopped");
        return;
    };
    let app = app.clone();
    let source: SourceFn = Arc::new(move || {
        app.try_state::<Arc<MeetingsManager>>()
            .map(|m| Arc::new(ManagerSource(m.inner().clone())) as Arc<dyn MeetingsSource>)
    });
    match Server::start(port, source) {
        Ok(server) => {
            log::info!("mcp: serving meetings at {}", endpoint_url(server.port));
            *current = Some(server);
        }
        Err(e) => log::error!("mcp: couldn't start the server on port {port}: {e:#}"),
    }
}

/// Whether the server is running (for the settings UI).
pub fn is_running(app: &AppHandle) -> bool {
    app.try_state::<McpServerState>()
        .is_some_and(|s| s.0.lock().is_some())
}

struct ManagerSource(Arc<MeetingsManager>);

impl MeetingsSource for ManagerSource {
    fn meetings(&self) -> Result<Vec<Meeting>> {
        self.0.list_meetings()
    }
    fn meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        self.0.get_meeting(id)
    }
    fn transcript(&self, id: MeetingId) -> Result<Vec<TranscriptLine>> {
        let user = summarize::user_name(
            &crate::settings::get_settings(self.0.app())
                .meetings
                .user_display_name,
        );
        let names = summarize::speaker_names(&self.0.speakers(id)?, &self.0.list_people()?, &user);
        Ok(summarize::transcript_lines(
            &self.0.segments(id)?,
            &names,
            &user,
        ))
    }
    fn summary(&self, id: MeetingId) -> Result<Option<StoredSummary>> {
        self.0.summary(id)
    }
    fn action_items(&self, id: MeetingId) -> Result<Vec<ActionItem>> {
        self.0.action_items(id)
    }
}

/// A running server. Dropping it stops accepting connections (requests in
/// flight finish on their own threads).
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Bind `127.0.0.1:port` (0 picks a free port) and serve on a thread.
    pub fn start(port: u16, source: SourceFn) -> Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
            .with_context(|| format!("bind 127.0.0.1:{port}"))?;
        let port = listener.local_addr()?.port();
        // Polled, so a stop is noticed without a wake-up connection.
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("mcp-server".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let source = source.clone();
                            let _ = std::thread::Builder::new().name("mcp-conn".into()).spawn(
                                move || {
                                    if let Err(e) = serve_connection(stream, port, &source) {
                                        log::debug!("mcp: connection: {e:#}");
                                    }
                                },
                            );
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            log::warn!("mcp: accept failed: {e}");
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
            })?;
        Ok(Self { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.take(MAX_BODY as u64 + 64 * 1024));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        bail!("bad request line");
    };
    let (method, path) = (
        method.to_string(),
        target.split('?').next().unwrap_or("").to_string(),
    );
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("connection closed in headers");
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADER_LINES {
            bail!("too many headers");
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut req = Request {
        method,
        path,
        headers,
        body: Vec::new(),
    };
    let len: usize = req
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        bail!("body too large");
    }
    req.body.resize(len, 0);
    reader.read_exact(&mut req.body)?;
    Ok(req)
}

/// Loopback hosts only: a DNS-rebound name (evil.example -> 127.0.0.1)
/// still sends its own name.
fn host_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h) || host.eq_ignore_ascii_case(&format!("{h}:{port}")))
}

/// Native clients send no `Origin`; browsers always do. Only pages served
/// from this machine are let through.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    rest.is_some_and(|r| {
        let host = r.rsplit_once(':').map_or(r, |(h, p)| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                r
            }
        });
        matches!(host, "127.0.0.1" | "localhost" | "[::1]")
    })
}

fn respond(mut stream: &TcpStream, status: &str, body: Option<&str>, extra: &[&str]) -> Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for h in extra {
        head.push_str(h);
        head.push_str("\r\n");
    }
    match body {
        Some(b) => head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        )),
        None => head.push_str("Content-Length: 0\r\n\r\n"),
    }
    stream.write_all(head.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn serve_connection(stream: TcpStream, port: u16, source: &SourceFn) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let req = read_request(&stream)?;
    if !host_allowed(req.header("host"), port) || !origin_allowed(req.header("origin")) {
        log::warn!("mcp: refused a request from a foreign host or origin");
        return respond(&stream, "403 Forbidden", None, &[]);
    }
    if req.path != PATH {
        return respond(&stream, "404 Not Found", None, &[]);
    }
    match req.method.as_str() {
        "POST" => {}
        // No server-initiated stream and no sessions to end.
        "GET" | "DELETE" => {
            return respond(&stream, "405 Method Not Allowed", None, &["Allow: POST"])
        }
        _ => return respond(&stream, "405 Method Not Allowed", None, &["Allow: POST"]),
    }
    let message: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            let body =
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#;
            return respond(&stream, "400 Bad Request", Some(body), &[]);
        }
    };
    match protocol::handle(&message, source) {
        Some(reply) => respond(&stream, "200 OK", Some(&reply.to_string()), &[]),
        None => respond(&stream, "202 Accepted", None, &[]),
    }
}

/// POST one message to the local server. `Ok(None)`: accepted, no reply.
pub fn post(port: u16, body: &str) -> Result<Option<String>> {
    let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
        .context("Guidy's MCP server isn't running")?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    write!(
        stream,
        "POST {PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, payload) = response
        .split_once("\r\n\r\n")
        .context("malformed response")?;
    let status = head.split_whitespace().nth(1).unwrap_or("");
    match status {
        "200" => Ok(Some(payload.to_string())),
        "202" => Ok(None),
        s => bail!("the MCP server answered HTTP {s}"),
    }
}

/// `guidy --mcp-stdio`: relay newline-delimited JSON-RPC between stdin/
/// stdout and the running app's server. Returns the exit code.
pub fn run_stdio_bridge(port: u16) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match post(port, &line) {
            Ok(reply) => reply,
            Err(e) => {
                // Requests still get an answer, so the client shows why.
                let id = serde_json::from_str::<Value>(&line)
                    .ok()
                    .and_then(|v| v.get("id").cloned());
                id.map(|id| {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32000,
                            "message": format!(
                                "{e:#}. Open Guidy and turn on Settings › Meetings › MCP server."
                            ),
                        }
                    })
                    .to_string()
                })
            }
        };
        if let Some(reply) = reply {
            if writeln!(stdout, "{reply}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::types::MeetingStatus;
    use serde_json::json;

    struct One;

    impl MeetingsSource for One {
        fn meetings(&self) -> Result<Vec<Meeting>> {
            Ok(vec![Meeting {
                id: 4,
                title: "Standup".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: 0,
                ended_at: None,
                mic_audio_path: None,
                system_audio_path: None,
                status: MeetingStatus::Recorded,
                error: None,
                processing_stage: None,
                description: None,
                title_source: Default::default(),
                has_summary: false,
            }])
        }
        fn meeting(&self, _: MeetingId) -> Result<Option<Meeting>> {
            Ok(None)
        }
        fn transcript(&self, _: MeetingId) -> Result<Vec<TranscriptLine>> {
            Ok(vec![])
        }
        fn summary(&self, _: MeetingId) -> Result<Option<StoredSummary>> {
            Ok(None)
        }
        fn action_items(&self, _: MeetingId) -> Result<Vec<ActionItem>> {
            Ok(vec![])
        }
    }

    fn server() -> Server {
        let src: Arc<dyn MeetingsSource> = Arc::new(One);
        Server::start(0, Arc::new(move || Some(src.clone()))).unwrap()
    }

    fn raw(port: u16, request: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_tools_over_http() {
        let server = server();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                          "params": {"name": "list_meetings"}})
        .to_string();
        let reply: Value =
            serde_json::from_str(&post(server.port(), &body).unwrap().unwrap()).unwrap();
        assert_eq!(
            reply["result"]["structuredContent"]["meetings"][0]["title"],
            "Standup"
        );

        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
        assert_eq!(post(server.port(), &note).unwrap(), None);
    }

    #[test]
    fn refuses_foreign_origins_hosts_and_paths() {
        let server = server();
        let port = server.port();
        let req = |extra: &str, path: &str| {
            format!("POST {path} HTTP/1.1\r\n{extra}Content-Length: 2\r\n\r\n{{}}")
        };
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        assert!(raw(
            port,
            &req(&format!("{host}Origin: https://evil.example\r\n"), PATH)
        )
        .starts_with("HTTP/1.1 403"));
        assert!(raw(port, &req("Host: evil.example\r\n", PATH)).starts_with("HTTP/1.1 403"));
        assert!(raw(port, &req(&host, "/other")).starts_with("HTTP/1.1 404"));
        assert!(
            raw(port, &format!("GET {PATH} HTTP/1.1\r\n{host}\r\n")).starts_with("HTTP/1.1 405")
        );
        // A local page is fine (the body isn't JSON-RPC: invalid request).
        let ok = raw(
            port,
            &req(&format!("{host}Origin: http://localhost:3000\r\n"), PATH),
        );
        assert!(ok.starts_with("HTTP/1.1 202"), "{ok}");
    }

    #[test]
    fn origin_and_host_checks() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://127.0.0.1:8080")));
        assert!(origin_allowed(Some("http://localhost")));
        assert!(!origin_allowed(Some("http://localhost.evil.com")));
        assert!(!origin_allowed(Some("null")));
        assert!(host_allowed(Some("localhost:9"), 9));
        assert!(!host_allowed(Some("localhost:10"), 9));
        assert!(!host_allowed(None, 9));
    }

    #[test]
    fn stopped_server_refuses_connections() {
        let port = {
            let s = server();
            s.port()
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(post(port, "{}").is_err());
    }
}
