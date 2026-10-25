//! Calls in other apps: Zoom, Google Meet or WhatsApp holding the microphone,
//! so the floater can offer to take notes. Pure rules over what
//! [`crate::meetings::native::audio_input_apps`] reports, polled by the app.
//!
//! A call is an app of [`CallApp`] capturing from the microphone for
//! [`CONFIRM_MS`] (a short mic test is not a call), and it ends once no such
//! app has captured for [`END_GRACE_MS`] (a mute, a device switch). Google
//! Meet runs in a browser, so a browser counts once one of its windows is
//! titled like a Meet call, then for as long as it keeps the microphone (the
//! user may switch tabs mid-call).

use serde::{Deserialize, Serialize};

/// Microphone held this long (ms) before it counts as a call.
pub const CONFIRM_MS: i64 = 3_000;
/// Microphone released this long (ms) before the call counts as over.
pub const END_GRACE_MS: i64 = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallApp {
    Zoom,
    GoogleMeet,
    WhatsApp,
}

impl CallApp {
    /// The product name (not translated).
    pub fn name(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::GoogleMeet => "Google Meet",
            Self::WhatsApp => "WhatsApp",
        }
    }
}

/// An app capturing from the microphone now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputApp {
    pub bundle_id: String,
    /// Its windows' titles; only read for browsers ([`title_source`]).
    pub titles: Vec<String>,
}

/// A call in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub app: CallApp,
    /// When it was first seen (unix ms): tells one call from the next.
    pub since: i64,
}

const ZOOM: &[&str] = &["us.zoom"];
const WHATSAPP: &[&str] = &["net.whatsapp.WhatsApp", "desktop.WhatsApp", "WhatsApp"];
/// Browsers (or the helper process that captures for them) and the app
/// whose windows say which site is open.
const BROWSERS: &[(&str, &str)] = &[
    ("com.google.Chrome", "com.google.Chrome"),
    ("com.apple.Safari", "com.apple.Safari"),
    // Safari captures in WebKit's GPU process.
    ("com.apple.WebKit", "com.apple.Safari"),
    ("company.thebrowser.Browser", "company.thebrowser.Browser"),
    ("com.microsoft.edgemac", "com.microsoft.edgemac"),
    ("com.brave.Browser", "com.brave.Browser"),
    ("com.vivaldi.Vivaldi", "com.vivaldi.Vivaldi"),
    ("com.operasoftware.Opera", "com.operasoftware.Opera"),
    ("org.mozilla.firefox", "org.mozilla.firefox"),
    ("org.mozilla.plugincontainer", "org.mozilla.firefox"),
];

/// `id` is `prefix` or one of its sub-bundles (`us.zoom.xos`,
/// `com.google.Chrome.helper`).
fn under(id: &str, prefix: &str) -> bool {
    id == prefix
        || id
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

fn is_any(id: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| under(id, p))
}

/// For a browser (or its capture helper), the bundle id whose window titles
/// tell whether Meet is open; `None` for anything else.
pub fn title_source(bundle_id: &str) -> Option<&'static str> {
    BROWSERS
        .iter()
        .find(|(p, _)| under(bundle_id, p))
        .map(|(_, app)| *app)
}

/// A window title of a Google Meet call (`Meet - abc-defg-hij`, `Meet –
/// Weekly sync`), or of a page on meet.google.com.
pub fn is_meet_title(title: &str) -> bool {
    let t = title.trim();
    t.contains("meet.google.com")
        || t.contains("Google Meet")
        || ["Meet - ", "Meet – ", "Meet — "]
            .iter()
            .any(|p| t.starts_with(p))
}

/// The call app holding the microphone, if any. `meeting` keeps a browser
/// counting as Meet while it holds the microphone, whatever its titles.
fn classify(apps: &[InputApp], meeting: bool) -> Option<CallApp> {
    let mut found = None;
    for a in apps {
        let app = if is_any(&a.bundle_id, ZOOM) {
            CallApp::Zoom
        } else if is_any(&a.bundle_id, WHATSAPP) {
            CallApp::WhatsApp
        } else if title_source(&a.bundle_id).is_some()
            && (meeting || a.titles.iter().any(|t| is_meet_title(t)))
        {
            CallApp::GoogleMeet
        } else {
            continue;
        };
        // A dedicated call app wins over a browser.
        if app != CallApp::GoogleMeet {
            return Some(app);
        }
        found = Some(app);
    }
    found
}

/// Follows the call apps' microphone use from poll to poll.
#[derive(Clone, Debug, Default)]
pub struct CallDetector {
    current: Option<Call>,
    /// A call app seen holding the microphone, and since when.
    candidate: Option<Call>,
    /// When the current call's app was last missing.
    gone_since: Option<i64>,
}

impl CallDetector {
    pub fn current(&self) -> Option<Call> {
        self.current
    }

    /// Take in what holds the microphone at `now` (unix ms); returns the
    /// call in progress.
    pub fn observe(&mut self, apps: &[InputApp], now: i64) -> Option<Call> {
        let meeting = self.current.is_some_and(|c| c.app == CallApp::GoogleMeet);
        match classify(apps, meeting) {
            Some(app) if self.current.is_some_and(|c| c.app == app) => {
                self.candidate = None;
                self.gone_since = None;
            }
            Some(app) => {
                let since = match self.candidate {
                    Some(c) if c.app == app => c.since,
                    _ => now,
                };
                self.candidate = Some(Call { app, since });
                if now - since >= CONFIRM_MS {
                    self.current = self.candidate.take();
                    self.gone_since = None;
                } else {
                    self.fade(now);
                }
            }
            None => {
                self.candidate = None;
                self.fade(now);
            }
        }
        self.current
    }

    /// The current call's app isn't holding the microphone at `now`.
    fn fade(&mut self, now: i64) {
        if self.current.is_none() {
            return;
        }
        let gone = *self.gone_since.get_or_insert(now);
        if now - gone >= END_GRACE_MS {
            self.current = None;
            self.gone_since = None;
        }
    }

    /// Forget everything (the feature was turned off).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> InputApp {
        InputApp {
            bundle_id: id.into(),
            titles: vec![],
        }
    }

    fn browser(id: &str, titles: &[&str]) -> InputApp {
        InputApp {
            bundle_id: id.into(),
            titles: titles.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn call_apps_are_known_by_bundle() {
        assert_eq!(classify(&[app("us.zoom.xos")], false), Some(CallApp::Zoom));
        assert_eq!(
            classify(&[app("net.whatsapp.WhatsApp")], false),
            Some(CallApp::WhatsApp)
        );
        assert_eq!(
            classify(&[app("desktop.WhatsApp.helper")], false),
            Some(CallApp::WhatsApp)
        );
        assert_eq!(classify(&[app("us.zoomer.app")], false), None);
        assert_eq!(classify(&[app("com.apple.VoiceMemos")], false), None);
    }

    #[test]
    fn a_browser_is_meet_only_with_a_meet_window() {
        let chrome = browser("com.google.Chrome.helper", &["Inbox - Gmail"]);
        assert_eq!(classify(std::slice::from_ref(&chrome), false), None);
        // Mid-call the user switched tabs: still Meet.
        assert_eq!(classify(&[chrome], true), Some(CallApp::GoogleMeet));
        let meet = browser("com.google.Chrome", &["Inbox", "Meet - abc-defg-hij"]);
        assert_eq!(
            classify(std::slice::from_ref(&meet), false),
            Some(CallApp::GoogleMeet)
        );
        // Zoom wins over a browser.
        assert_eq!(
            classify(&[meet, app("us.zoom.xos")], false),
            Some(CallApp::Zoom)
        );
        assert_eq!(
            title_source("com.apple.WebKit.GPU"),
            Some("com.apple.Safari")
        );
        assert_eq!(title_source("us.zoom.xos"), None);
    }

    #[test]
    fn meet_titles() {
        assert!(is_meet_title("Meet - abc-defg-hij"));
        assert!(is_meet_title("Meet – Weekly sync"));
        assert!(is_meet_title("https://meet.google.com/abc-defg-hij"));
        assert!(is_meet_title("Google Meet"));
        assert!(!is_meet_title("Meeting notes - Google Docs"));
        assert!(!is_meet_title("Meetup"));
    }

    #[test]
    fn a_call_needs_the_mic_for_a_few_seconds() {
        let mut d = CallDetector::default();
        let zoom = [app("us.zoom.xos")];
        assert_eq!(d.observe(&zoom, 0), None);
        assert_eq!(d.observe(&zoom, CONFIRM_MS - 1), None);
        let call = d.observe(&zoom, CONFIRM_MS).unwrap();
        assert_eq!(
            call,
            Call {
                app: CallApp::Zoom,
                since: 0
            }
        );
        // A blip that stopped before then is not a call.
        let mut d = CallDetector::default();
        d.observe(&zoom, 0);
        d.observe(&[], 1_000);
        assert_eq!(d.observe(&zoom, CONFIRM_MS), None);
    }

    #[test]
    fn a_call_survives_a_short_gap_and_ends_after_the_grace() {
        let mut d = CallDetector::default();
        let wa = [app("net.whatsapp.WhatsApp")];
        d.observe(&wa, 0);
        let call = d.observe(&wa, CONFIRM_MS).unwrap();
        assert_eq!(d.observe(&[], 10_000), Some(call));
        // Back before the grace ran out: the same call.
        assert_eq!(d.observe(&wa, 12_000), Some(call));
        assert_eq!(d.observe(&[], 20_000), Some(call));
        assert_eq!(d.observe(&[], 20_000 + END_GRACE_MS), None);
        // The next call is a new one.
        d.observe(&wa, 40_000);
        let next = d.observe(&wa, 40_000 + CONFIRM_MS).unwrap();
        assert_eq!(next.since, 40_000);
    }

    #[test]
    fn meet_stays_while_the_browser_keeps_the_mic() {
        let mut d = CallDetector::default();
        let meet = [browser("com.google.Chrome", &["Meet - abc-defg-hij"])];
        let other_tab = [browser("com.google.Chrome", &["Docs"])];
        d.observe(&meet, 0);
        let call = d.observe(&meet, CONFIRM_MS).unwrap();
        assert_eq!(call.app, CallApp::GoogleMeet);
        assert_eq!(d.observe(&other_tab, 60_000), Some(call));
        assert_eq!(d.observe(&other_tab, 60_000 + END_GRACE_MS), Some(call));
        assert_eq!(d.observe(&[], 70_000), Some(call));
        assert_eq!(d.observe(&[], 70_000 + END_GRACE_MS), None);
        // Without a Meet window a browser holding the mic is no call.
        assert_eq!(d.observe(&other_tab, 100_000), None);
        assert_eq!(d.observe(&other_tab, 100_000 + CONFIRM_MS), None);
    }
}
