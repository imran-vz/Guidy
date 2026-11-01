//! Which web pages are calls. Pure: judged from a page's host and path, never
//! the query or fragment (those can hold tokens and are not even kept).
//!
//! A host must match exactly, or as a real subdomain for the wildcard
//! services, so `meet.google.com.evil.io` and `evilwebex.com` are ordinary
//! pages.

use crate::meetings::calls::{CallSource, PageInfo};

/// Lowercase, no trailing dot, no `www.`.
pub fn site_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.strip_prefix("www.") {
        Some(rest) => rest.to_string(),
        None => host,
    }
}

/// The path's segments, without empty ones.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

/// A Meet room: any first segment but the service's own pages.
fn is_meet_room(path: &str) -> bool {
    segments(path).next().is_some_and(|first| {
        !matches!(
            first.to_ascii_lowercase().as_str(),
            "landing" | "new" | "about" | "unsupported"
        )
    })
}

/// Sites whose every page is a call page, named by their own host.
fn is_named_call_host(host: &str) -> bool {
    matches!(host, "meet.jit.si" | "whereby.com")
}

/// The known call service `page` belongs to, if any.
pub fn known_call_page(page: &PageInfo) -> Option<CallSource> {
    let host = site_host(&page.host);
    match host.as_str() {
        "meet.google.com" if is_meet_room(&page.path) => Some(CallSource::Meet),
        "teams.microsoft.com" | "teams.live.com" => Some(CallSource::Teams),
        "app.zoom.us" if segments(&page.path).next() == Some("wc") => Some(CallSource::Zoom),
        "webex.com" => Some(CallSource::Webex),
        h if h.ends_with(".webex.com") => Some(CallSource::Webex),
        h if is_named_call_host(h) => Some(CallSource::BrowserSite(h.to_string())),
        _ => None,
    }
}

/// What a browser holding the microphone on `page` is: a known service, or
/// just the site it is on.
pub fn source_for_page(page: &PageInfo) -> CallSource {
    known_call_page(page).unwrap_or_else(|| CallSource::BrowserSite(site_host(&page.host)))
}

/// A [`CallSource::BrowserSite`] host that is a known call site, not just any
/// page (it counts as a known service: quicker offer, Always allowed).
pub fn is_known_site(host: &str) -> bool {
    is_named_call_host(&site_host(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(host: &str, path: &str) -> PageInfo {
        PageInfo {
            host: host.into(),
            path: path.into(),
        }
    }

    #[test]
    fn meet_needs_a_room_path() {
        let meet = |path| known_call_page(&page("meet.google.com", path));
        assert_eq!(meet("/abc-defg-hij"), Some(CallSource::Meet));
        assert_eq!(meet("/lookup/weekly"), Some(CallSource::Meet));
        assert_eq!(meet("/"), None);
        assert_eq!(meet(""), None);
        assert_eq!(meet("/landing"), None);
        assert_eq!(meet("/new"), None);
    }

    #[test]
    fn known_services_are_named() {
        let of = |host, path| known_call_page(&page(host, path));
        assert_eq!(of("teams.microsoft.com", "/"), Some(CallSource::Teams));
        assert_eq!(of("teams.live.com", "/x"), Some(CallSource::Teams));
        assert_eq!(of("app.zoom.us", "/wc/123/join"), Some(CallSource::Zoom));
        assert_eq!(of("acme.webex.com", "/meet/x"), Some(CallSource::Webex));
        assert_eq!(
            of("meet.jit.si", "/room"),
            Some(CallSource::BrowserSite("meet.jit.si".into()))
        );
        assert_eq!(
            of("WWW.Whereby.com.", "/room"),
            Some(CallSource::BrowserSite("whereby.com".into()))
        );
    }

    #[test]
    fn lookalike_hosts_are_rejected() {
        for (host, path) in [
            ("meet.google.com.evil.io", "/abc-defg-hij"),
            ("evilmeet.google.com", "/abc-defg-hij"),
            ("notmeet.google.com", "/abc-defg-hij"),
            ("teams.microsoft.com.evil.io", "/"),
            ("evilteams.live.com", "/"),
            ("evilwebex.com", "/"),
            ("webex.com.evil.io", "/"),
            ("app.zoom.us.evil.io", "/wc/1"),
            ("meet.jit.si.evil.io", "/room"),
            ("notwhereby.com", "/room"),
            ("example.com", "/meet.google.com/abc-defg-hij"),
        ] {
            assert_eq!(known_call_page(&page(host, path)), None, "{host}{path}");
        }
        // Zoom's site is only a call under /wc.
        assert_eq!(known_call_page(&page("app.zoom.us", "/wcx/1")), None);
        assert_eq!(known_call_page(&page("app.zoom.us", "/profile")), None);
    }

    #[test]
    fn any_other_page_is_only_a_site() {
        assert_eq!(
            source_for_page(&page("www.Example.com", "/a")),
            CallSource::BrowserSite("example.com".into())
        );
        assert!(!is_known_site("example.com"));
        assert!(is_known_site("meet.jit.si"));
    }
}
