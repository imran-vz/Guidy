//! Calls in other apps: a call service holding the microphone, so the floater
//! can offer to take notes. Pure rules over what
//! [`crate::meetings::native::audio_input_apps`] reports, polled by the app.
//!
//! An observation is who holds the microphone now. A known service counts as
//! a call after [`CONFIRM_MS`] (voice-note apps after
//! [`VOICE_NOTE_APP_CONFIRM_MS`]); a browser on an unknown page after
//! [`UNKNOWN_PAGE_CONFIRM_MS`]: voice notes and dictation fields open the
//! microphone for seconds, calls hold it for minutes. The count follows the
//! app, not the page: finding out which page it is mid-run keeps it. Losing
//! the microphone restarts it, and so do observations more than
//! [`MAX_OBSERVATION_GAP_MS`] apart (a sleeping Mac).
//!
//! A call ends once its app has been off the microphone for [`END_GRACE_MS`]
//! (a mute, a device switch). A call the user answered is snoozed until its
//! app has been off the microphone for [`SNOOZE_ABSENT_MS`]. A different app
//! taking the microphone once the first has left is a new call.
//!
//! Pages and hosts are held in memory only and must never be logged: use
//! [`CallSource::log_name`] in logs.

use serde::{Deserialize, Serialize};

use crate::meetings::call_sites;

/// Microphone held this long (ms) by a known service: a call.
pub const CONFIRM_MS: i64 = 2_000;
/// The same for apps that also record voice notes (WhatsApp, Slack). Longer
/// than [`CONFIRM_MS`] to pass over the shortest of those; short enough, with
/// a 1 s poll, to offer within the 5 s the scorecard allows. A voice note past
/// this still gets an offer: the first seconds of the two look the same
/// (`fixtures/calls/voice_note_app_limit.json`).
pub const VOICE_NOTE_APP_CONFIRM_MS: i64 = 4_000;
/// Microphone held this long (ms) by a browser on an unknown page: a call.
/// The microphone may have been taken up to one 1 s poll before it was first
/// seen, so this offers within the 20 s the plan and scorecard allow.
pub const UNKNOWN_PAGE_CONFIRM_MS: i64 = 19_000;
/// Observations further apart than this don't make one run.
pub const MAX_OBSERVATION_GAP_MS: i64 = 10_000;
/// A snoozed app must be off the microphone this long to be offered again.
pub const SNOOZE_ABSENT_MS: i64 = 60_000;
/// Microphone released this long (ms) before the call counts as over.
pub const END_GRACE_MS: i64 = 8_000;

/// Where a call is. Serialized names match the old `CallApp`'s. `Debug` hides
/// a site's host.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallSource {
    Zoom,
    Teams,
    Slack,
    FaceTime,
    Webex,
    Discord,
    WhatsApp,
    #[serde(rename = "google_meet")]
    Meet,
    /// A browser on this host (normalized by [`call_sites::site_host`]).
    BrowserSite(String),
    /// A browser, page not known.
    BrowserUnknown,
}

impl std::fmt::Debug for CallSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.log_name())
    }
}

impl CallSource {
    /// The product name (not translated), or the site's host. For display
    /// only: a host must not reach the logs.
    pub fn name(&self) -> &str {
        match self {
            Self::Zoom => "Zoom",
            Self::Teams => "Microsoft Teams",
            Self::Slack => "Slack",
            Self::FaceTime => "FaceTime",
            Self::Webex => "Webex",
            Self::Discord => "Discord",
            Self::WhatsApp => "WhatsApp",
            Self::Meet => "Google Meet",
            Self::BrowserSite(host) => host.as_str(),
            Self::BrowserUnknown => "Browser",
        }
    }

    /// A name safe to log: never a host.
    pub fn log_name(&self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::Teams => "Microsoft Teams",
            Self::Slack => "Slack",
            Self::FaceTime => "FaceTime",
            Self::Webex => "Webex",
            Self::Discord => "Discord",
            Self::WhatsApp => "WhatsApp",
            Self::Meet => "Google Meet",
            Self::BrowserSite(_) => "website",
            Self::BrowserUnknown => "browser",
        }
    }

    /// How long the microphone must be held before this is a call.
    pub fn confirm_ms(&self) -> i64 {
        match self {
            Self::WhatsApp | Self::Slack => VOICE_NOTE_APP_CONFIRM_MS,
            Self::BrowserSite(host) if !call_sites::is_known_site(host) => UNKNOWN_PAGE_CONFIRM_MS,
            Self::BrowserUnknown => UNKNOWN_PAGE_CONFIRM_MS,
            _ => CONFIRM_MS,
        }
    }

    /// Whose call this is, for the user's Ask / Always / Never choice. A
    /// browser whose page isn't known has none: the choice would cover every
    /// page.
    pub fn provider(&self) -> Option<Provider> {
        let key = match self {
            Self::Zoom => "zoom",
            Self::Teams => "teams",
            Self::Slack => "slack",
            Self::FaceTime => "facetime",
            Self::Webex => "webex",
            Self::Discord => "discord",
            Self::WhatsApp => "whatsapp",
            Self::Meet => "google_meet",
            Self::BrowserSite(host) => host.as_str(),
            Self::BrowserUnknown => return None,
        };
        Some(Provider {
            key: key.to_string(),
            label: self.name().to_string(),
        })
    }

    /// Whether Always may be chosen: known services and call sites only, not
    /// a page that merely held the microphone for a while.
    pub fn can_always_record(&self) -> bool {
        match self {
            Self::BrowserUnknown => false,
            Self::BrowserSite(host) => call_sites::is_known_site(host),
            _ => true,
        }
    }

    /// The user's choice for this source.
    pub fn policy(&self, choices: &[CallProviderChoice]) -> CallPolicy {
        let Some(provider) = self.provider() else {
            return CallPolicy::Ask;
        };
        let chosen = choices
            .iter()
            .find(|c| c.key == provider.key)
            .map(|c| c.policy)
            .unwrap_or_default();
        // A stored Always can't outlive what it may cover.
        if chosen == CallPolicy::Always && !self.can_always_record() {
            return CallPolicy::Ask;
        }
        chosen
    }
}

/// A service or site whose calls share one choice. `Debug` hides it: it may
/// be a site's host.
#[derive(Clone, PartialEq, Eq)]
pub struct Provider {
    pub key: String,
    pub label: String,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<provider>")
    }
}

/// What a detected call leads to.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallPolicy {
    #[default]
    Ask,
    Always,
    Never,
}

/// The user's choice for a provider (the `call_providers` setting). `Debug`
/// hides which: it may be a site's host.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct CallProviderChoice {
    pub key: String,
    pub label: String,
    pub policy: CallPolicy,
}

impl std::fmt::Debug for CallProviderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<provider: {:?}>", self.policy)
    }
}

/// The page a browser shows. In memory only; `Debug` hides it so it can't
/// reach a log by accident.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PageInfo {
    pub host: String,
    pub path: String,
}

impl std::fmt::Debug for PageInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<page>")
    }
}

/// An app capturing from the microphone now. `Debug` leaves out its titles,
/// which can hold a page's address.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct InputApp {
    pub bundle_id: String,
    /// Its windows' titles; only read for browsers ([`title_source`]).
    pub titles: Vec<String>,
    pub pid: i32,
    /// The app behind the process ([`canonical_app`]). Empty: derive it from
    /// `bundle_id`.
    pub canonical: String,
    pub is_browser: bool,
    /// The page, for a browser, when it could be read.
    pub page: Option<PageInfo>,
}

impl std::fmt::Debug for InputApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputApp")
            .field("bundle_id", &self.bundle_id)
            .field("pid", &self.pid)
            .field("canonical", &self.canonical)
            .field("is_browser", &self.is_browser)
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

impl InputApp {
    /// From what Core Audio says: `name` is the process's display name and
    /// `parents` the bundle ids up its parent chain, nearest first.
    pub fn resolved(pid: i32, bundle_id: &str, name: &str, parents: &[String]) -> Self {
        let canonical = canonical_app(bundle_id, name, parents);
        Self {
            bundle_id: bundle_id.to_string(),
            titles: Vec::new(),
            pid,
            is_browser: is_browser_app(&canonical),
            canonical,
            page: None,
        }
    }

    /// The canonical app id, derived when this wasn't built by
    /// [`Self::resolved`].
    pub fn app_id(&self) -> String {
        if self.canonical.is_empty() {
            canonical_app(&self.bundle_id, "", &[])
        } else {
            self.canonical.clone()
        }
    }
}

/// A call in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub source: CallSource,
    /// When it was first seen (unix ms): tells one call from the next.
    pub since: i64,
    /// The canonical app holding the microphone for it.
    pub app_id: String,
}

/// What a newly confirmed call leads to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    Offer(Call),
    /// The provider is set to Always.
    Record(Call),
}

/// (prefix, canonical id): `prefix` or its sub-bundles. Teams, Webex and
/// Discord ids are unverified on hardware (`docs/plans/meetings.md`).
const CALL_APPS: &[(&str, &str)] = &[
    ("us.zoom", "us.zoom.xos"),
    ("com.microsoft.teams2", "com.microsoft.teams2"),
    ("com.microsoft.teams", "com.microsoft.teams2"),
    ("com.tinyspeck.slackmacgap", "com.tinyspeck.slackmacgap"),
    ("com.apple.FaceTime", "com.apple.FaceTime"),
    ("Cisco-Systems.Spark", "Cisco-Systems.Spark"),
    ("com.webex.meetingmanager", "Cisco-Systems.Spark"),
    ("com.hnc.Discord", "com.hnc.Discord"),
    ("net.whatsapp.WhatsApp", "net.whatsapp.WhatsApp"),
    ("desktop.WhatsApp", "net.whatsapp.WhatsApp"),
    ("WhatsApp", "net.whatsapp.WhatsApp"),
];

/// The service a canonical call app is.
fn call_app_source(canonical: &str) -> Option<CallSource> {
    Some(match canonical {
        "us.zoom.xos" => CallSource::Zoom,
        "com.microsoft.teams2" => CallSource::Teams,
        "com.tinyspeck.slackmacgap" => CallSource::Slack,
        "com.apple.FaceTime" => CallSource::FaceTime,
        "Cisco-Systems.Spark" => CallSource::Webex,
        "com.hnc.Discord" => CallSource::Discord,
        "net.whatsapp.WhatsApp" => CallSource::WhatsApp,
        _ => return None,
    })
}

/// Browsers (or the helper process that captures for them) and the app whose
/// windows and tabs say which site is open. WebKit's helpers aren't listed:
/// any app can embed WebKit ([`canonical_app`] tells Safari's by its name).
const BROWSERS: &[(&str, &str)] = &[
    ("com.google.Chrome", "com.google.Chrome"),
    ("com.apple.Safari", "com.apple.Safari"),
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

/// The known app `id` belongs to.
fn known_app(id: &str) -> Option<&'static str> {
    CALL_APPS
        .iter()
        .chain(BROWSERS)
        .find(|(prefix, _)| under(id, prefix))
        .map(|(_, canonical)| *canonical)
}

/// The app behind a process capturing audio. `parents` are the bundle ids up
/// its parent chain, nearest first: Chromium and Electron capture in helpers,
/// some without a bundle id. A WebKit helper is Safari's only if it says so:
/// its display name, or a parent. Unknown apps stay themselves.
pub fn canonical_app(bundle_id: &str, name: &str, parents: &[String]) -> String {
    if under(bundle_id, "com.apple.WebKit") && name.to_ascii_lowercase().starts_with("safari") {
        return "com.apple.Safari".to_string();
    }
    if let Some(known) = std::iter::once(bundle_id)
        .chain(parents.iter().map(String::as_str))
        .find_map(known_app)
    {
        return known.to_string();
    }
    if bundle_id.is_empty() {
        return parents
            .iter()
            .find(|p| !p.is_empty())
            .cloned()
            .unwrap_or_default();
    }
    bundle_id.to_string()
}

/// Whether a canonical app id is a browser.
pub fn is_browser_app(app_id: &str) -> bool {
    BROWSERS.iter().any(|(_, canonical)| *canonical == app_id)
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
/// Weekly sync`, `Google Meet - Brave`), or one that starts with an address
/// on meet.google.com: `meet.google.com.evil.io`, `How to use Google Meet`
/// and `Notes on meet.google.com` do not count.
pub fn is_meet_title(title: &str) -> bool {
    let t = title.trim();
    let on_meet = t.split_whitespace().next().is_some_and(|word| {
        let host = word
            .strip_prefix("https://")
            .or_else(|| word.strip_prefix("http://"))
            .unwrap_or(word);
        let host = host.split(['/', '?', '#']).next().unwrap_or(host);
        call_sites::site_host(host) == "meet.google.com"
    });
    let named = t
        .strip_prefix("Google Meet")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(" -") || rest.starts_with(" –"))
        || t.ends_with("- Google Meet")
        || t.ends_with("| Google Meet");
    on_meet
        || named
        || ["Meet - ", "Meet – ", "Meet — "]
            .iter()
            .any(|p| t.starts_with(p))
}

/// What an app holding the microphone may be: `None` when it can't be a call
/// (a voice recorder, a dictation app).
fn classify_app(app: &InputApp) -> Option<CallSource> {
    let id = app.app_id();
    if let Some(source) = call_app_source(&id) {
        return Some(source);
    }
    if !(app.is_browser || is_browser_app(&id)) {
        return None;
    }
    if let Some(page) = &app.page {
        return Some(call_sites::source_for_page(page));
    }
    if app.titles.iter().any(|t| is_meet_title(t)) {
        return Some(CallSource::Meet);
    }
    Some(CallSource::BrowserUnknown)
}

/// How sure a source is: a known service over a known site over any page.
fn rank(source: &CallSource) -> u8 {
    match source {
        CallSource::BrowserUnknown => 0,
        CallSource::BrowserSite(host) if !call_sites::is_known_site(host) => 1,
        CallSource::BrowserSite(_) | CallSource::Meet => 2,
        _ => 3,
    }
}

/// Everything on the microphone that may be a call, one entry per app, the
/// surest source of each.
fn candidates(apps: &[InputApp]) -> Vec<(String, CallSource)> {
    let mut found: Vec<(String, CallSource)> = Vec::new();
    for app in apps {
        let Some(source) = classify_app(app) else {
            continue;
        };
        let id = app.app_id();
        match found.iter_mut().find(|(known, _)| *known == id) {
            Some(entry) if rank(&source) > rank(&entry.1) => entry.1 = source,
            Some(_) => {}
            None => found.push((id, source)),
        }
    }
    found
}

/// Whether the canonical app `app_id` holds the microphone in `apps`.
pub fn holds_microphone(apps: &[InputApp], app_id: &str) -> bool {
    apps.iter().any(|a| a.app_id() == app_id)
}

/// An app's microphone run on its way to being a call.
#[derive(Clone, Debug)]
struct Pending {
    app_id: String,
    since: i64,
}

/// An answered app, not offered again until it has left the microphone.
#[derive(Clone, Debug)]
struct Snooze {
    app_id: String,
    last_present: i64,
}

/// Follows the microphone from poll to poll.
#[derive(Clone, Debug, Default)]
pub struct CallDetector {
    current: Option<Call>,
    /// The apps seen holding the microphone, each with its own run, so two at
    /// once can't keep restarting each other.
    pending: Vec<Pending>,
    /// When the current call's app was last missing.
    gone_since: Option<i64>,
    /// The current call's app released the microphone since its service was
    /// last seen, so a different service in it is a new call.
    released: bool,
    snoozed: Vec<Snooze>,
    last_observed: Option<i64>,
}

impl CallDetector {
    pub fn current(&self) -> Option<&Call> {
        self.current.as_ref()
    }

    /// Take in what holds the microphone at `now` (unix ms); returns the
    /// call in progress.
    pub fn observe(&mut self, apps: &[InputApp], now: i64) -> Option<Call> {
        self.detect(apps, &[], now);
        self.current.clone()
    }

    /// The clock went backwards, so every time held is in the future: count
    /// from now.
    fn rebase(&mut self, now: i64) {
        self.pending.clear();
        if self.gone_since.is_some() {
            self.gone_since = Some(now);
        }
        for snooze in &mut self.snoozed {
            snooze.last_present = now;
        }
    }

    /// One observation under the user's choices. Returns what a call
    /// confirmed by it leads to; each call is reported once.
    pub fn detect(
        &mut self,
        apps: &[InputApp],
        choices: &[CallProviderChoice],
        now: i64,
    ) -> Option<Detection> {
        let last = self.last_observed.replace(now);
        if last.is_some_and(|last| now < last) {
            self.rebase(now);
        }
        let elapsed = last.map_or(0, |last| now - last);
        let gap = elapsed > MAX_OBSERVATION_GAP_MS;
        // Unwatched for this long (the Mac slept): whatever held the
        // microphone before may be over, though it holds it now.
        if elapsed >= SNOOZE_ABSENT_MS {
            self.current = None;
            self.gone_since = None;
            self.released = false;
        }
        let found = candidates(apps);

        self.snoozed
            .retain(|s| now - s.last_present < SNOOZE_ABSENT_MS);
        for snooze in &mut self.snoozed {
            if holds_microphone(apps, &snooze.app_id) {
                snooze.last_present = now;
            }
        }

        let mut recording = None;
        if let Some(current) = &mut self.current {
            match found.iter().find(|(id, _)| *id == current.app_id) {
                // The microphone was released and taken again for another
                // known service: a new call, though in the same browser.
                Some((_, source))
                    if self.released
                        && rank(source) >= 2
                        && rank(&current.source) >= 2
                        && *source != current.source =>
                {
                    self.current = None;
                    self.gone_since = None;
                    self.released = false;
                }
                Some((_, source)) => {
                    self.gone_since = None;
                    if rank(source) >= 2 {
                        self.released = false;
                    }
                    // Still on the microphone, on whatever page: the same
                    // call. A browser's page found later names it, if that
                    // says more, and the user's choice for that page applies.
                    if rank(source) > rank(&current.source) && rank(&current.source) < 2 {
                        current.source = source.clone();
                        match source.policy(choices) {
                            CallPolicy::Never => self.snooze_current(now),
                            CallPolicy::Always => {
                                recording = self.current.clone().map(Detection::Record);
                            }
                            CallPolicy::Ask => {}
                        }
                    }
                }
                None => {
                    self.released = true;
                    let gone = *self.gone_since.get_or_insert(now);
                    if now - gone >= END_GRACE_MS {
                        self.current = None;
                        self.gone_since = None;
                        self.released = false;
                    }
                }
            }
        }
        // While a call's app stays on the microphone, only a call app taking
        // over from a browser is a new call: not a second browser, not an
        // equal app. Whatever is current now, after the above.
        let current_app = self.current.as_ref().map(|c| c.app_id.clone());
        let staying = current_app
            .as_deref()
            .filter(|id| holds_microphone(apps, id));
        let eligible: Vec<(String, CallSource)> = found
            .into_iter()
            .filter(|(id, source)| {
                Some(id) != current_app.as_ref()
                    && !self.snoozed.iter().any(|s| s.app_id == *id)
                    && source.policy(choices) != CallPolicy::Never
                    && staying.is_none_or(|app| is_browser_app(app) && !is_browser_app(id))
            })
            .collect();
        let pending: Vec<Pending> = eligible
            .iter()
            .map(|(app_id, _)| Pending {
                app_id: app_id.clone(),
                since: self
                    .pending
                    .iter()
                    .find(|p| p.app_id == *app_id && !gap)
                    .map_or(now, |p| p.since),
            })
            .collect();
        self.pending = pending;
        if recording.is_some() {
            return recording;
        }

        // Of those held long enough: the surest, then the longest held.
        let mut confirmed: Option<(String, CallSource, i64)> = None;
        for ((app_id, source), run) in eligible.into_iter().zip(&self.pending) {
            if now - run.since < source.confirm_ms() {
                continue;
            }
            let better = confirmed.as_ref().is_none_or(|(best_id, best, since)| {
                (
                    rank(&source),
                    std::cmp::Reverse(run.since),
                    best_id.as_str(),
                ) > (rank(best), std::cmp::Reverse(*since), app_id.as_str())
            });
            if better {
                confirmed = Some((app_id, source, run.since));
            }
        }
        let (app_id, source, since) = confirmed?;
        self.pending.retain(|p| p.app_id != app_id);
        self.gone_since = None;
        self.released = false;
        // A browser call displaced while it keeps the microphone isn't offered
        // again when the call app leaves.
        if let Some(displaced) = staying.map(str::to_string) {
            self.snooze_app(displaced, now);
        }
        let policy = source.policy(choices);
        let call = Call {
            source,
            since,
            app_id,
        };
        self.current = Some(call.clone());
        Some(match policy {
            CallPolicy::Always => Detection::Record(call),
            _ => Detection::Offer(call),
        })
    }

    /// The user answered the current call (Not now, the offer timed out, a
    /// recording of it stopped while it goes on): no offer for its app until
    /// it has left the microphone for [`SNOOZE_ABSENT_MS`].
    pub fn snooze_current(&mut self, now: i64) {
        if let Some(call) = self.current.take() {
            self.snooze_app(call.app_id, now);
        }
        self.gone_since = None;
        self.released = false;
    }

    /// [`Self::snooze_current`] for an app by its canonical id (a recording's
    /// call may no longer be the current one).
    pub fn snooze_app(&mut self, app_id: String, now: i64) {
        self.pending.retain(|p| p.app_id != app_id);
        self.snoozed.retain(|s| s.app_id != app_id);
        self.snoozed.push(Snooze {
            app_id,
            last_present: now,
        });
    }

    /// Forget everything (the feature was turned off).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// What changed about the call behind a recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceChange {
    /// Its app has been off the microphone for [`END_GRACE_MS`].
    Left,
    /// It came back after [`PresenceChange::Left`].
    Back,
}

/// Notices when the app behind a recording leaves the microphone, so the user
/// can be asked. Reports; never stops anything.
#[derive(Clone, Debug, Default)]
pub struct CallPresence {
    absent_since: Option<i64>,
    left: bool,
}

impl CallPresence {
    pub fn observe(&mut self, present: bool, now: i64) -> Option<PresenceChange> {
        if present {
            self.absent_since = None;
            return std::mem::take(&mut self.left).then_some(PresenceChange::Back);
        }
        // A clock that went backwards counts from now.
        let since = self.absent_since.unwrap_or(now).min(now);
        self.absent_since = Some(since);
        if !self.left && now - since >= END_GRACE_MS {
            self.left = true;
            return Some(PresenceChange::Left);
        }
        None
    }

    pub fn has_left(&self) -> bool {
        self.left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> InputApp {
        InputApp {
            bundle_id: id.into(),
            ..Default::default()
        }
    }

    fn browser(id: &str, titles: &[&str]) -> InputApp {
        InputApp {
            bundle_id: id.into(),
            titles: titles.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    fn classify(apps: &[InputApp]) -> Option<CallSource> {
        let mut found = candidates(apps);
        found.sort_by_key(|(_, source)| std::cmp::Reverse(rank(source)));
        found.into_iter().next().map(|(_, source)| source)
    }

    #[test]
    fn call_apps_are_known_by_bundle() {
        assert_eq!(classify(&[app("us.zoom.xos")]), Some(CallSource::Zoom));
        assert_eq!(
            classify(&[app("net.whatsapp.WhatsApp")]),
            Some(CallSource::WhatsApp)
        );
        assert_eq!(
            classify(&[app("desktop.WhatsApp.helper")]),
            Some(CallSource::WhatsApp)
        );
        assert_eq!(
            classify(&[app("com.microsoft.teams2")]),
            Some(CallSource::Teams)
        );
        assert_eq!(
            classify(&[app("com.microsoft.teams")]),
            Some(CallSource::Teams)
        );
        assert_eq!(
            classify(&[app("com.tinyspeck.slackmacgap.helper")]),
            Some(CallSource::Slack)
        );
        assert_eq!(
            classify(&[app("com.apple.FaceTime")]),
            Some(CallSource::FaceTime)
        );
        assert_eq!(classify(&[app("us.zoomer.app")]), None);
        assert_eq!(classify(&[app("com.apple.VoiceMemos")]), None);
    }

    #[test]
    fn a_browser_is_meet_only_with_a_meet_window() {
        let chrome = browser("com.google.Chrome.helper", &["Inbox - Gmail"]);
        assert_eq!(
            classify(std::slice::from_ref(&chrome)),
            Some(CallSource::BrowserUnknown)
        );
        let meet = browser("com.google.Chrome", &["Inbox", "Meet - abc-defg-hij"]);
        assert_eq!(
            classify(std::slice::from_ref(&meet)),
            Some(CallSource::Meet)
        );
        // Zoom wins over a browser.
        assert_eq!(
            classify(&[meet, app("us.zoom.xos")]),
            Some(CallSource::Zoom)
        );
        assert_eq!(title_source("us.zoom.xos"), None);
        assert_eq!(
            title_source("company.thebrowser.Browser"),
            Some("company.thebrowser.Browser")
        );
    }

    #[test]
    fn webkit_is_safari_only_when_it_says_so() {
        let parents: &[String] = &[];
        assert_eq!(
            canonical_app("com.apple.WebKit.GPU", "Safari Graphics and Media", parents),
            "com.apple.Safari"
        );
        // Another app's embedded WebKit is not Safari.
        let other = canonical_app("com.apple.WebKit.GPU", "Mail Graphics and Media", parents);
        assert_eq!(other, "com.apple.WebKit.GPU");
        assert!(!is_browser_app(&other));
        assert_eq!(classify(&[app("com.apple.WebKit.GPU")]), None);
    }

    #[test]
    fn helpers_resolve_through_their_parents() {
        let parents = vec![String::new(), "com.google.Chrome".to_string()];
        assert_eq!(canonical_app("", "", &parents), "com.google.Chrome");
        let resolved = InputApp::resolved(7, "", "", &parents);
        assert!(resolved.is_browser);
        assert_eq!(resolved.pid, 7);
        // avconferenced has FaceTime added as a parent while it runs.
        let parents = vec!["com.apple.FaceTime".to_string()];
        assert_eq!(
            canonical_app("com.apple.avconferenced", "", &parents),
            "com.apple.FaceTime"
        );
        assert_eq!(
            canonical_app("com.apple.avconferenced", "", &[]),
            "com.apple.avconferenced"
        );
        assert_eq!(canonical_app("", "", &[]), "");
    }

    #[test]
    fn meet_titles() {
        assert!(is_meet_title("Meet - abc-defg-hij"));
        assert!(is_meet_title("Meet – Weekly sync"));
        assert!(is_meet_title("https://meet.google.com/abc-defg-hij"));
        assert!(is_meet_title("Google Meet"));
        assert!(!is_meet_title("Meeting notes - Google Docs"));
        assert!(!is_meet_title("Meetup"));
        assert!(is_meet_title("Google Meet - Google Chrome"));
        assert!(is_meet_title("Weekly sync - Google Meet"));
        // Lookalikes, and pages only about Meet.
        assert!(!is_meet_title(
            "https://meet.google.com.evil.io/abc-defg-hij"
        ));
        assert!(!is_meet_title("How to use Google Meet - YouTube"));
        assert!(!is_meet_title("How to use meet.google.com - Google Docs"));
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
                source: CallSource::Zoom,
                since: 0,
                app_id: "us.zoom.xos".into(),
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
        d.observe(&wa, 2_000);
        let call = d.observe(&wa, VOICE_NOTE_APP_CONFIRM_MS).unwrap();
        assert_eq!(d.observe(&[], 10_000), Some(call.clone()));
        // Back before the grace ran out: the same call.
        assert_eq!(d.observe(&wa, 12_000), Some(call.clone()));
        assert_eq!(d.observe(&[], 20_000), Some(call.clone()));
        assert_eq!(d.observe(&[], 20_000 + END_GRACE_MS), None);
        // The next call is a new one.
        d.observe(&wa, 40_000);
        d.observe(&wa, 42_000);
        let next = d.observe(&wa, 40_000 + VOICE_NOTE_APP_CONFIRM_MS).unwrap();
        assert_eq!(next.since, 40_000);
    }

    #[test]
    fn meet_stays_while_the_browser_keeps_the_mic() {
        let mut d = CallDetector::default();
        let meet = [browser("com.google.Chrome", &["Meet - abc-defg-hij"])];
        let other_tab = [browser("com.google.Chrome", &["Docs"])];
        d.observe(&meet, 0);
        let call = d.observe(&meet, CONFIRM_MS).unwrap();
        assert_eq!(call.source, CallSource::Meet);
        assert_eq!(d.observe(&other_tab, 8_000), Some(call.clone()));
        assert_eq!(d.observe(&other_tab, 16_000), Some(call.clone()));
        assert_eq!(d.observe(&[], 24_000), Some(call.clone()));
        assert_eq!(d.observe(&[], 24_000 + END_GRACE_MS), None);
        // Without a Meet window a browser holding the mic isn't Meet.
        assert_eq!(d.observe(&other_tab, 100_000), None);
        assert_eq!(d.observe(&other_tab, 100_000 + CONFIRM_MS), None);
    }

    #[test]
    fn serialized_names_are_the_old_ones() {
        let name = |s: &CallSource| serde_json::to_string(s).unwrap();
        assert_eq!(name(&CallSource::Zoom), "\"zoom\"");
        assert_eq!(name(&CallSource::Meet), "\"google_meet\"");
        assert_eq!(name(&CallSource::WhatsApp), "\"whats_app\"");
        for old in ["zoom", "google_meet", "whats_app"] {
            let parsed: CallSource = serde_json::from_str(&format!("\"{old}\"")).unwrap();
            assert_eq!(name(&parsed), format!("\"{old}\""));
        }
    }

    #[test]
    fn logs_never_get_a_host() {
        let site = CallSource::BrowserSite("secret.example.com".into());
        assert_eq!(site.name(), "secret.example.com");
        assert!(!site.log_name().contains("secret"));
        let page = PageInfo {
            host: "secret.example.com".into(),
            path: "/room".into(),
        };
        assert!(!format!("{page:?}").contains("secret"));
        assert!(!format!("{site:?}").contains("secret"));
        let call = Call {
            source: site.clone(),
            since: 0,
            app_id: "com.google.Chrome".into(),
        };
        assert!(!format!("{:?}", Detection::Offer(call)).contains("secret"));
        let provider = site.provider().unwrap();
        assert!(!format!("{provider:?}").contains("secret"));
        let choice = CallProviderChoice {
            key: provider.key,
            label: provider.label,
            policy: CallPolicy::Never,
        };
        assert!(!format!("{choice:?}").contains("secret"));
        let app = InputApp {
            titles: vec!["https://secret.example.com/room".into()],
            page: Some(page),
            ..Default::default()
        };
        assert!(!format!("{app:?}").contains("secret"));
    }

    #[test]
    fn choices_decide_what_a_call_leads_to() {
        let choice = |key: &str, policy| CallProviderChoice {
            key: key.into(),
            label: key.into(),
            policy,
        };
        let zoom = [app("us.zoom.xos")];
        let mut d = CallDetector::default();
        let always = [choice("zoom", CallPolicy::Always)];
        d.detect(&zoom, &always, 0);
        assert!(matches!(
            d.detect(&zoom, &always, CONFIRM_MS),
            Some(Detection::Record(_))
        ));
        // Reported once.
        assert_eq!(d.detect(&zoom, &always, CONFIRM_MS + 1_000), None);

        let never = [choice("zoom", CallPolicy::Never)];
        let mut d = CallDetector::default();
        d.detect(&zoom, &never, 0);
        assert_eq!(d.detect(&zoom, &never, CONFIRM_MS), None);
        assert_eq!(d.current(), None);

        // No Always for a page that merely held the microphone.
        let page = CallSource::BrowserSite("example.com".into());
        assert!(!page.can_always_record());
        assert_eq!(
            page.policy(&[choice("example.com", CallPolicy::Always)]),
            CallPolicy::Ask
        );
        assert_eq!(
            page.policy(&[choice("example.com", CallPolicy::Never)]),
            CallPolicy::Never
        );
        assert_eq!(CallSource::BrowserUnknown.provider(), None);
    }

    #[test]
    fn a_clock_going_backwards_counts_from_now() {
        let mut d = CallDetector::default();
        let zoom = [app("us.zoom.xos")];
        d.observe(&zoom, 100_000);
        let call = d.observe(&zoom, 102_000).unwrap();
        d.observe(&[], 104_000);
        // Back to 0: gone for 8 s from here, not until 112 s.
        assert_eq!(d.observe(&[], 0), Some(call));
        assert_eq!(d.observe(&[], END_GRACE_MS), None);

        let mut p = CallPresence::default();
        p.observe(false, 100_000);
        assert_eq!(p.observe(false, 0), None);
        assert_eq!(p.observe(false, END_GRACE_MS), Some(PresenceChange::Left));
    }

    #[test]
    fn presence_reports_leaving_and_coming_back_once() {
        let mut p = CallPresence::default();
        assert_eq!(p.observe(true, 0), None);
        assert_eq!(p.observe(false, 1_000), None);
        assert_eq!(p.observe(false, 1_000 + END_GRACE_MS - 1), None);
        assert_eq!(
            p.observe(false, 1_000 + END_GRACE_MS),
            Some(PresenceChange::Left)
        );
        assert!(p.has_left());
        assert_eq!(p.observe(false, 20_000), None);
        assert_eq!(p.observe(true, 21_000), Some(PresenceChange::Back));
        assert_eq!(p.observe(true, 22_000), None);
        // A short release after that doesn't count.
        assert_eq!(p.observe(false, 23_000), None);
        assert_eq!(p.observe(true, 24_000), None);
    }

    // ----- Fixtures: timed observation sequences (`fixtures/calls/*.json`) --

    #[derive(Deserialize)]
    struct Fixture {
        name: String,
        steps: Vec<Step>,
        offers: Vec<Offer>,
        #[serde(default)]
        choices: Vec<CallProviderChoice>,
    }

    /// Observations every `every` ms from `from` through `to` (once when
    /// `to` is missing); `dismiss` answers the current call after the last.
    #[derive(Deserialize)]
    struct Step {
        from: i64,
        to: Option<i64>,
        #[serde(default = "one_second")]
        every: i64,
        #[serde(default)]
        apps: Vec<FixtureApp>,
        #[serde(default)]
        dismiss: bool,
    }

    fn one_second() -> i64 {
        1_000
    }

    #[derive(Deserialize)]
    struct FixtureApp {
        id: String,
        host: Option<String>,
        #[serde(default)]
        path: String,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct Offer {
        t: i64,
        source: CallSource,
        #[serde(default)]
        record: bool,
    }

    const FIXTURES: &[&str] = &[
        include_str!("../../../../fixtures/calls/voice_note_bursts.json"),
        include_str!("../../../../fixtures/calls/voice_note_app_limit.json"),
        include_str!("../../../../fixtures/calls/always_page_with_interrupted_run.json"),
        include_str!("../../../../fixtures/calls/back_to_back_browser_page_late.json"),
        include_str!("../../../../fixtures/calls/always_page_keeps_other_run.json"),
        include_str!("../../../../fixtures/calls/back_to_back_same_browser.json"),
        include_str!("../../../../fixtures/calls/call_app_after_browser_service.json"),
        include_str!("../../../../fixtures/calls/displaced_browser_not_reoffered.json"),
        include_str!("../../../../fixtures/calls/never_page_keeps_other_run.json"),
        include_str!("../../../../fixtures/calls/generic_site_becomes_meet.json"),
        include_str!("../../../../fixtures/calls/call_app_after_browser.json"),
        include_str!("../../../../fixtures/calls/two_apps_alternating.json"),
        include_str!("../../../../fixtures/calls/page_policy_after_offer.json"),
        include_str!("../../../../fixtures/calls/long_sleep_after_call.json"),
        include_str!("../../../../fixtures/calls/unknown_page_offer.json"),
        include_str!("../../../../fixtures/calls/page_found_mid_run.json"),
        include_str!("../../../../fixtures/calls/unknown_site_found_mid_run.json"),
        include_str!("../../../../fixtures/calls/sleep_gap.json"),
        include_str!("../../../../fixtures/calls/dismissal_snooze.json"),
        include_str!("../../../../fixtures/calls/lookalike_host.json"),
        include_str!("../../../../fixtures/calls/tab_switch.json"),
        include_str!("../../../../fixtures/calls/back_to_back_sources.json"),
        include_str!("../../../../fixtures/calls/back_to_back_same_app.json"),
        include_str!("../../../../fixtures/calls/always_and_never.json"),
    ];

    fn run(fixture: &Fixture) -> Vec<Offer> {
        let mut d = CallDetector::default();
        let mut offers = Vec::new();
        for step in &fixture.steps {
            let apps: Vec<InputApp> = step
                .apps
                .iter()
                .map(|a| InputApp {
                    bundle_id: a.id.clone(),
                    page: a.host.as_ref().map(|host| PageInfo {
                        host: host.clone(),
                        path: a.path.clone(),
                    }),
                    ..Default::default()
                })
                .collect();
            let to = step.to.unwrap_or(step.from);
            let mut t = step.from;
            while t <= to {
                match d.detect(&apps, &fixture.choices, t) {
                    Some(Detection::Offer(c)) => offers.push(Offer {
                        t,
                        source: c.source,
                        record: false,
                    }),
                    Some(Detection::Record(c)) => offers.push(Offer {
                        t,
                        source: c.source,
                        record: true,
                    }),
                    None => {}
                }
                t += step.every;
            }
            if step.dismiss {
                d.snooze_current(to);
            }
        }
        offers
    }

    #[test]
    fn fixtures_give_the_expected_offers() {
        for text in FIXTURES {
            let fixture: Fixture = serde_json::from_str(text).unwrap();
            assert_eq!(run(&fixture), fixture.offers, "{}", fixture.name);
        }
    }
}
