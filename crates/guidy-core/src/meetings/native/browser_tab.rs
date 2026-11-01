//! Which page a browser holding the microphone shows, so a call can be named.
//!
//! Only after the user opted in (`name_browser_calls`): the first query per
//! browser makes macOS ask for the Automation permission. It reads the active
//! tab of the front window, like the dialects in Hex (MIT, see `NOTICE.md`),
//! through `/usr/bin/osascript` on its own thread, one query at a time, killed
//! after [`QUERY_LIMIT`]. Firefox has no such dictionary and is read through
//! Accessibility instead.
//!
//! Background tabs are deliberately not read: one open says nothing about
//! which tab is using the microphone. Pages stay in memory, with no query or
//! fragment, and are never logged or sent anywhere.

use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::meetings::calls::PageInfo;

/// How long one query may take.
pub const QUERY_LIMIT: Duration = Duration::from_secs(2);
/// How often a browser's tab is queried while it holds the microphone.
const QUERY_INTERVAL: Duration = Duration::from_secs(1);
/// A page read this long ago still names the call, when a query fails.
const PAGE_MEMORY: Duration = Duration::from_secs(60);
/// After macOS refuses Automation, how long before asking it again.
const DENIED_BACKOFF: Duration = Duration::from_secs(60);

/// How a browser's scripting dictionary names its active tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// `URL of active tab of front window`.
    Chromium,
    /// `URL of current tab of front window`.
    Safari,
    /// No dictionary: Accessibility's `AXURL`.
    Accessibility,
}

/// The dialect of a canonical browser id.
pub fn dialect(app_id: &str) -> Option<Dialect> {
    Some(match app_id {
        "com.apple.Safari" => Dialect::Safari,
        "com.google.Chrome"
        | "company.thebrowser.Browser"
        | "com.microsoft.edgemac"
        | "com.brave.Browser"
        | "com.vivaldi.Vivaldi"
        | "com.operasoftware.Opera" => Dialect::Chromium,
        "org.mozilla.firefox" => Dialect::Accessibility,
        _ => return None,
    })
}

/// The script reading the active tab's address. `None` for a browser it can't
/// read. It never launches the browser: `tell` would.
pub fn tab_script(app_id: &str) -> Option<String> {
    let tab = match dialect(app_id)? {
        Dialect::Chromium => "active tab",
        Dialect::Safari => "current tab",
        Dialect::Accessibility => return None,
    };
    Some(format!(
        "if application id \"{app_id}\" is running then\n\
         \ttell application id \"{app_id}\"\n\
         \t\tif (count of windows) is 0 then return \"\"\n\
         \t\treturn URL of {tab} of front window\n\
         \tend tell\n\
         end if\n\
         return \"\""
    ))
}

/// The host and path of an address; the query, the fragment, the credentials
/// and the port are dropped here. `None` unless it is a web address.
pub fn parse_page(address: &str) -> Option<PageInfo> {
    let address = address.trim();
    let scheme_end = address.find("://")?;
    if !["http", "https"]
        .iter()
        .any(|s| s.eq_ignore_ascii_case(&address[..scheme_end]))
    {
        return None;
    }
    let rest = &address[scheme_end + 3..];
    let end = rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        return None;
    }
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    Some(PageInfo {
        host: crate::meetings::call_sites::site_host(host),
        path: tail[..path_end].replace('\\', "/"),
    })
}

/// How a query went.
#[derive(Debug, PartialEq, Eq)]
pub enum QueryError {
    /// macOS refused Automation for this browser.
    Denied,
    Failed,
}

/// `Ok(None)`: answered, no web page in front.
pub type QueryResult = Result<Option<PageInfo>, QueryError>;

struct Running {
    app_id: String,
    answer: Receiver<QueryResult>,
}

/// Reads browsers' tabs without ever waiting for one. [`Self::page`] starts a
/// query and returns the newest finished answer.
pub struct TabReader {
    query: fn(&str) -> QueryResult,
    running: Option<Running>,
    started: Option<Instant>,
    denied: Vec<(String, Instant)>,
    /// App id, its page, when it was read.
    memory: Vec<(String, PageInfo, Instant)>,
}

impl TabReader {
    pub fn new(query: fn(&str) -> QueryResult) -> Self {
        Self {
            query,
            running: None,
            started: None,
            denied: Vec::new(),
            memory: Vec::new(),
        }
    }

    /// Only the browsers in `held` still hold the microphone: forget the
    /// others' pages, so their next call starts fresh.
    pub fn retain_apps(&mut self, held: &[String]) {
        self.memory.retain(|(app, _, _)| held.contains(app));
        // An answer on its way for a browser that left says nothing now.
        if self
            .running
            .as_ref()
            .is_some_and(|r| !held.contains(&r.app_id))
        {
            self.running = None;
        }
    }

    fn collect(&mut self, now: Instant) {
        let Some(running) = &self.running else {
            return;
        };
        let answer = match running.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(QueryError::Failed),
        };
        let app_id = running.app_id.clone();
        self.running = None;
        match answer {
            Ok(Some(page)) => {
                self.memory.retain(|(app, _, _)| *app != app_id);
                self.memory.push((app_id, page, now));
            }
            Ok(None) | Err(QueryError::Failed) => {}
            Err(QueryError::Denied) => {
                log::info!("meetings: Automation refused for a browser; naming its calls paused");
                self.denied.retain(|(app, _)| *app != app_id);
                self.denied.push((app_id, now));
            }
        }
    }

    /// The page to judge `app_id`'s call by, if any is known yet.
    pub fn page(&mut self, app_id: &str, now: Instant) -> Option<PageInfo> {
        self.collect(now);
        self.denied
            .retain(|(_, at)| now.saturating_duration_since(*at) < DENIED_BACKOFF);
        let denied = self.denied.iter().any(|(app, _)| app == app_id);
        let due = self
            .started
            .is_none_or(|t| now.saturating_duration_since(t) >= QUERY_INTERVAL);
        if self.running.is_none() && due && !denied {
            self.started = Some(now);
            let (tx, answer) = sync_channel(1);
            let query = self.query;
            let id = app_id.to_string();
            let spawned = std::thread::Builder::new()
                .name("meeting-browser-tab".into())
                .spawn(move || {
                    let _ = tx.send(query(&id));
                });
            match spawned {
                Ok(_) => {
                    self.running = Some(Running {
                        app_id: app_id.to_string(),
                        answer,
                    });
                }
                Err(e) => log::warn!("meetings: could not start a browser tab query: {e}"),
            }
        }
        self.memory
            .iter()
            .find(|(app, _, at)| app == app_id && now.saturating_duration_since(*at) <= PAGE_MEMORY)
            .map(|(_, page, _)| page.clone())
    }
}

/// Runs `script`, killed after [`QUERY_LIMIT`]. Its output and errors aren't
/// logged: they can hold an address.
#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, QueryError> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| QueryError::Failed)?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= QUERY_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(QueryError::Failed);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(QueryError::Failed);
            }
        }
    }
    let output = child.wait_with_output().map_err(|_| QueryError::Failed)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(classify_failure(&String::from_utf8_lossy(&output.stderr)))
}

/// `errAEEventNotPermitted` (-1743) is the user's refusal; anything else may
/// pass.
pub fn classify_failure(stderr: &str) -> QueryError {
    if stderr.contains("-1743") {
        QueryError::Denied
    } else {
        QueryError::Failed
    }
}

/// The real query: the browser's front tab. Blocks up to [`QUERY_LIMIT`].
#[cfg(target_os = "macos")]
pub fn query_front_tab(app_id: &str) -> QueryResult {
    match dialect(app_id) {
        Some(Dialect::Accessibility) => Ok(super::macos::focused_web_url(app_id)
            .as_deref()
            .and_then(parse_page)),
        Some(_) => {
            let Some(script) = tab_script(app_id) else {
                return Ok(None);
            };
            Ok(parse_page(&run_osascript(&script)?))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_read_the_active_tab_of_the_front_window_only() {
        let chrome = tab_script("com.google.Chrome").unwrap();
        assert!(chrome.contains("URL of active tab of front window"));
        assert!(chrome.starts_with("if application id \"com.google.Chrome\" is running then"));
        let safari = tab_script("com.apple.Safari").unwrap();
        assert!(safari.contains("URL of current tab of front window"));
        let arc = tab_script("company.thebrowser.Browser").unwrap();
        assert!(arc.contains("active tab"));
        for script in [&chrome, &safari, &arc] {
            assert!(!script.contains("every tab"));
            assert!(!script.contains("title"));
        }
        assert_eq!(tab_script("org.mozilla.firefox"), None);
        assert_eq!(tab_script("us.zoom.xos"), None);
    }

    #[test]
    fn every_browser_has_a_dialect() {
        for id in [
            "com.google.Chrome",
            "com.apple.Safari",
            "company.thebrowser.Browser",
            "com.microsoft.edgemac",
            "com.brave.Browser",
            "com.vivaldi.Vivaldi",
            "com.operasoftware.Opera",
            "org.mozilla.firefox",
        ] {
            assert!(crate::meetings::calls::is_browser_app(id), "{id}");
            assert!(dialect(id).is_some(), "{id}");
        }
    }

    fn parsed(address: &str) -> Option<(String, String)> {
        parse_page(address).map(|p| (p.host, p.path))
    }

    fn pair(host: &str, path: &str) -> Option<(String, String)> {
        Some((host.to_string(), path.to_string()))
    }

    #[test]
    fn addresses_keep_only_host_and_path() {
        assert_eq!(
            parsed("https://meet.google.com/abc-defg-hij?authuser=0&token=x#frag\n"),
            pair("meet.google.com", "/abc-defg-hij")
        );
        assert_eq!(parsed("HTTP://Example.COM:8080"), pair("example.com", ""));
        assert_eq!(
            parsed("https://www.example.com/a"),
            pair("example.com", "/a")
        );
        assert_eq!(parsed("https://example.com?q=1"), pair("example.com", ""));
    }

    #[test]
    fn tricky_addresses_name_the_real_host() {
        assert_eq!(
            parsed("https://meet.google.com@evil.io/abc-defg-hij"),
            pair("evil.io", "/abc-defg-hij")
        );
        assert_eq!(
            parsed("https://user:pw@example.com/x"),
            pair("example.com", "/x")
        );
        assert_eq!(
            parsed("https://evil.io/meet.google.com/abc-defg-hij"),
            pair("evil.io", "/meet.google.com/abc-defg-hij")
        );
        assert_eq!(
            parsed("https://evil.io?u=https://meet.google.com/x"),
            pair("evil.io", "")
        );
    }

    #[test]
    fn only_web_addresses_are_pages() {
        for address in [
            "",
            "file:///Users/me/a.html",
            "chrome://settings",
            "about:blank",
            "missing value",
            "https://",
        ] {
            assert_eq!(parsed(address), None, "{address}");
        }
    }

    #[test]
    fn refusal_is_told_apart_from_other_failures() {
        assert_eq!(
            classify_failure("execution error: Not authorised to send Apple events. (-1743)"),
            QueryError::Denied
        );
        assert_eq!(classify_failure("timed out"), QueryError::Failed);
    }

    /// Waits for the reader's query thread to answer.
    fn settle(reader: &mut TabReader, now: Instant) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while reader.running.is_some() && Instant::now() < deadline {
            reader.collect(now);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_page_is_remembered_for_a_minute_when_queries_stop_answering() {
        fn query(_: &str) -> QueryResult {
            Ok(parse_page("https://meet.google.com/abc-defg-hij"))
        }
        let t0 = Instant::now();
        let mut reader = TabReader::new(query);
        assert_eq!(reader.page("com.google.Chrome", t0), None);
        settle(&mut reader, t0);
        assert!(reader.page("com.google.Chrome", t0).is_some());
        settle(&mut reader, t0);
        reader.query = |_| Err(QueryError::Failed);
        let later = t0 + Duration::from_secs(30);
        assert!(reader.page("com.google.Chrome", later).is_some());
        settle(&mut reader, later);
        let late = t0 + Duration::from_secs(61);
        assert_eq!(reader.page("com.google.Chrome", late), None);
        settle(&mut reader, late);
        // Another browser doesn't get this one's page.
        let mut reader = TabReader::new(query);
        reader.page("com.google.Chrome", t0);
        settle(&mut reader, t0);
        assert_eq!(
            reader.page("com.apple.Safari", t0 + Duration::from_secs(1)),
            None
        );
        settle(&mut reader, t0);
        // Leaving the microphone forgets the page.
        let held = ["com.apple.Safari".to_string()];
        reader.retain_apps(&held);
        assert!(reader
            .memory
            .iter()
            .all(|(app, _, _)| app != "com.google.Chrome"));
    }

    #[test]
    fn one_query_at_a_time_and_a_refusal_backs_off() {
        fn denied(_: &str) -> QueryResult {
            Err(QueryError::Denied)
        }
        let t0 = Instant::now();
        let mut reader = TabReader::new(denied);
        reader.page("com.google.Chrome", t0);
        assert!(reader.running.is_some());
        settle(&mut reader, t0);
        // Refused: no query for a minute, for that browser.
        assert_eq!(
            reader.page("com.google.Chrome", t0 + Duration::from_secs(5)),
            None
        );
        assert!(reader.running.is_none());
        reader.page("com.google.Chrome", t0 + Duration::from_secs(61));
        assert!(reader.running.is_some());
    }
}
