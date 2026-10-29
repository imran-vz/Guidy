//! macOS bridges for meeting notes (docs/MEETING_NOTES_ARCH.md, workstream B):
//! EventKit calendar, the microphone, the Core Audio process tap for system
//! audio, echo cancellation of the mic ([`aec`]), and user notifications.
//!
//! The Swift side lives in `swift/meeting_bridge.swift` (built by
//! `build_meeting_bridge()` in build.rs); [`macos`] holds the FFI. Everything
//! here that does not touch the OS (meeting-link detection, event conversion,
//! the capture hand-off) is plain Rust and unit tested on every platform. On
//! other platforms the API reports "unsupported".
//!
//! Threading:
//! - The permission requests (`request_*`) block until the user answers a
//!   system prompt. Call them from a background thread, never the UI thread.
//! - Capture callbacks run on one dedicated delivery thread per capture (not
//!   the audio threads): the audio threads only copy samples into a channel.
//!   Callbacks may do moderate work (encoding, file writes); if they fall
//!   behind, samples queue up in memory rather than being dropped.
//! - The notification handler runs on whatever thread UserNotifications uses
//!   for its delegate. Keep it short and hand off.

// The conversion and hand-off helpers are only reached from the macOS bridge.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use anyhow::Result;
use serde::Deserialize;
use std::sync::mpsc;

use crate::meetings::manager::PermissionReport;
use crate::meetings::types::{Attendee, CalendarEvent, Track};

mod aec;
#[cfg(target_os = "macos")]
mod macos;

use aec::EchoCanceller;

/// Sample rate of every buffer delivered by [`start_capture`].
pub const CAPTURE_SAMPLE_RATE: u32 = 16_000;

/// Action id reported when the user clicks the notification body.
pub const NOTIFICATION_DEFAULT_ACTION: &str = "default";

/// State of one privacy permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    /// Never asked.
    NotDetermined,
    /// Blocked by policy (MDM, parental controls).
    Restricted,
    Denied,
    Granted,
    /// Calendar only: write-only access, which cannot read events.
    WriteOnly,
    /// The OS gives no way to tell (system audio without the TCC preflight).
    Unknown,
    /// The feature does not exist here (old macOS, other OS, unbundled
    /// binary for notifications).
    Unavailable,
}

impl AuthStatus {
    pub fn is_granted(self) -> bool {
        self == Self::Granted
    }

    fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::NotDetermined,
            1 => Self::Restricted,
            2 => Self::Denied,
            3 => Self::Granted,
            4 => Self::WriteOnly,
            6 => Self::Unavailable,
            _ => Self::Unknown,
        }
    }
}

fn unsupported() -> anyhow::Error {
    anyhow::anyhow!("meeting notes need macOS 14.2 or newer")
}

// ===== Platform support =====================================================

/// True on macOS 14.2+ (Core Audio process taps).
pub fn is_supported() -> bool {
    #[cfg(target_os = "macos")]
    return macos::is_supported();
    #[cfg(not(target_os = "macos"))]
    false
}

// ===== Calendar ============================================================

pub fn calendar_authorization() -> AuthStatus {
    #[cfg(target_os = "macos")]
    return macos::calendar_authorization();
    #[cfg(not(target_os = "macos"))]
    AuthStatus::Unavailable
}

/// Ask for full calendar access (blocking until the user answers). Returns
/// true when access is granted. Uses `requestFullAccessToEvents` on macOS 14+.
pub fn request_calendar_access() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_calendar_access();
    #[cfg(not(target_os = "macos"))]
    false
}

/// Events overlapping `[start_ms, end_ms)` (unix ms) that carry a Meet, Zoom
/// or Teams link, sorted by start. All-day events are skipped.
pub fn events_between(start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
    #[cfg(target_os = "macos")]
    {
        let raw = macos::raw_events_between(start_ms, end_ms)?;
        Ok(meeting_events(raw))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (start_ms, end_ms);
        Err(unsupported())
    }
}

/// Every timed event overlapping `[start_ms, end_ms)`, with or without a
/// meeting link, sorted by start. All-day events are skipped. Used to link a
/// manual start to whatever is happening now (plan 1.3); prompts use
/// [`events_between`], which requires a link.
pub fn timed_events_between(start_ms: i64, end_ms: i64) -> Result<Vec<CalendarEvent>> {
    #[cfg(target_os = "macos")]
    {
        let raw = macos::raw_events_between(start_ms, end_ms)?;
        Ok(timed_events(raw))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (start_ms, end_ms);
        Err(unsupported())
    }
}

/// One event as the Swift bridge reports it, before filtering.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub(crate) struct RawEvent {
    pub external_id: Option<String>,
    pub event_id: Option<String>,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub url: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub attendees: Vec<RawAttendee>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub(crate) struct RawAttendee {
    pub name: Option<String>,
    /// Usually `mailto:someone@example.com`.
    pub url: Option<String>,
    pub is_current_user: bool,
}

/// Keep timed events with a meeting link and convert them.
pub(crate) fn meeting_events(raw: Vec<RawEvent>) -> Vec<CalendarEvent> {
    convert_events(raw, true)
}

/// Keep every timed event (link or not) and convert it.
pub(crate) fn timed_events(raw: Vec<RawEvent>) -> Vec<CalendarEvent> {
    convert_events(raw, false)
}

fn convert_events(raw: Vec<RawEvent>, require_link: bool) -> Vec<CalendarEvent> {
    let mut events: Vec<CalendarEvent> = raw
        .into_iter()
        .filter(|e| !e.all_day)
        .filter_map(|e| {
            let meeting_url = [&e.url, &e.location, &e.notes]
                .into_iter()
                .flatten()
                .find_map(|text| find_meeting_url(text));
            if require_link && meeting_url.is_none() {
                return None;
            }
            let id = event_id(&e)?;
            Some(CalendarEvent {
                id,
                title: e.title.trim().to_string(),
                start_at: e.start_ms,
                end_at: e.end_ms.max(e.start_ms),
                meeting_url,
                attendees: e.attendees.iter().filter_map(attendee).collect(),
            })
        })
        .collect();
    events.sort_by(|a, b| a.start_at.cmp(&b.start_at).then_with(|| a.id.cmp(&b.id)));
    events
}

/// `calendarItemExternalIdentifier` (stable across devices), else
/// `eventIdentifier`, suffixed with the start so recurring occurrences differ:
/// `"<identifier>@<start unix ms>"`.
pub(crate) fn event_id(e: &RawEvent) -> Option<String> {
    let base = [&e.external_id, &e.event_id]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())?;
    Some(format!("{base}@{}", e.start_ms))
}

fn attendee(a: &RawAttendee) -> Option<Attendee> {
    let name = a
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let email = a.url.as_deref().and_then(email_from_participant_url);
    // A name that is just the email adds nothing.
    let name = name.filter(|n| Some(n.as_str()) != email.as_deref());
    if name.is_none() && email.is_none() {
        return None;
    }
    Some(Attendee {
        name,
        email,
        is_current_user: a.is_current_user,
    })
}

/// `mailto:a%40b.com?x=y` -> `a@b.com`. Non-mailto URLs give `None`.
pub(crate) fn email_from_participant_url(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        .map(|_| &url[7..])?;
    let addr = rest.split(['?', '#']).next().unwrap_or("");
    let addr = percent_decode(addr);
    let addr = addr.trim();
    (addr.contains('@') && !addr.contains(char::is_whitespace)).then(|| addr.to_string())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

static MEETING_URL_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    // `\b` keeps "notmeet.google.com" out. The path runs until whitespace or a
    // delimiter commonly wrapped around links in notes (<>, quotes, brackets).
    regex::Regex::new(
        r#"(?xi)
        \b(?:https?://)?
        (?:
            meet\.google\.com/[^\s<>"'()\[\]{}]+
          | (?:[a-z0-9-]+\.)*(?:zoom\.us|zoomgov\.com)/(?:j|w|s|my)/[^\s<>"'()\[\]{}]+
          | teams\.microsoft\.com/l/meetup-join/[^\s<>"'()\[\]{}]+
          | teams\.live\.com/meet/[^\s<>"'()\[\]{}]+
        )"#,
    )
    .expect("meeting url regex")
});

/// The first Google Meet, Zoom or Teams meeting link in `text`, normalized to
/// start with `https://`.
pub fn find_meeting_url(text: &str) -> Option<String> {
    MEETING_URL_RE.find_iter(text).find_map(|m| {
        let url = m
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', '*', '_']);
        // Trimming must leave a link with a path ("meet.google.com/." is not).
        if MEETING_URL_RE.find(url).map(|m| m.len()) != Some(url.len()) {
            return None;
        }
        let lower = url.to_ascii_lowercase();
        Some(if lower.starts_with("https://") {
            url.to_string()
        } else if lower.starts_with("http://") {
            format!("https://{}", &url[7..])
        } else {
            format!("https://{url}")
        })
    })
}

// ===== Capture =============================================================

/// Which streams [`start_capture_streams`] opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureStreams {
    pub mic: bool,
    pub system: bool,
}

impl Default for CaptureStreams {
    fn default() -> Self {
        Self {
            mic: true,
            system: true,
        }
    }
}

/// A stream problem reported while capturing, e.g. a restart after an audio
/// device change. Messages starting with `"error:"` mean the stream stopped
/// delivering audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureEvent {
    pub track: Track,
    pub message: String,
}

impl CaptureEvent {
    pub fn is_error(&self) -> bool {
        self.message.starts_with("error:")
    }
}

/// Called with a block of mono f32 samples (owned: the callback may keep
/// them without copying) and its sample rate.
pub type PcmCallback = Box<dyn FnMut(Vec<f32>, u32) + Send>;

/// Receives captured audio: 16 kHz mono f32 samples plus the sample rate
/// (always [`CAPTURE_SAMPLE_RATE`]). All callbacks of one capture run on its
/// delivery thread, in order, never concurrently.
pub struct CaptureCallbacks {
    pub on_mic: PcmCallback,
    pub on_system: PcmCallback,
    pub on_event: Box<dyn FnMut(CaptureEvent) + Send>,
}

impl CaptureCallbacks {
    /// Callbacks that ignore events.
    pub fn new(
        on_mic: impl FnMut(Vec<f32>, u32) + Send + 'static,
        on_system: impl FnMut(Vec<f32>, u32) + Send + 'static,
    ) -> Self {
        Self {
            on_mic: Box::new(on_mic),
            on_system: Box::new(on_system),
            on_event: Box::new(|e| log::warn!("meeting capture ({:?}): {}", e.track, e.message)),
        }
    }

    pub fn with_events(mut self, on_event: impl FnMut(CaptureEvent) + Send + 'static) -> Self {
        self.on_event = Box::new(on_event);
        self
    }
}

/// What the audio threads hand to the delivery thread.
pub(crate) enum CaptureMsg {
    Audio(Track, Vec<f32>, u32),
    Event(CaptureEvent),
}

/// Runs the callbacks until every sender is gone. With `echo`, the mic
/// reaches `on_mic` echo-cancelled against the system track (a little later
/// than it was captured: each mic frame waits for its system frame).
pub(crate) fn deliver(
    rx: mpsc::Receiver<CaptureMsg>,
    mut callbacks: CaptureCallbacks,
    mut echo: Option<EchoCanceller>,
) {
    while let Ok(msg) = rx.recv() {
        match (msg, echo.as_mut()) {
            (CaptureMsg::Audio(Track::Mic, samples, _), Some(aec)) => {
                cleaned_mic(&mut callbacks, aec.push_mic(&samples))
            }
            (CaptureMsg::Audio(Track::System, samples, rate), Some(aec)) => {
                let cleaned = aec.push_system(&samples);
                (callbacks.on_system)(samples, rate);
                cleaned_mic(&mut callbacks, cleaned);
            }
            (CaptureMsg::Audio(Track::Mic, samples, rate), None) => {
                (callbacks.on_mic)(samples, rate)
            }
            (CaptureMsg::Audio(Track::System, samples, rate), None) => {
                (callbacks.on_system)(samples, rate)
            }
            (CaptureMsg::Event(event), _) => (callbacks.on_event)(event),
        }
    }
    if let Some(aec) = echo.as_mut() {
        cleaned_mic(&mut callbacks, aec.finish());
    }
}

fn cleaned_mic(callbacks: &mut CaptureCallbacks, samples: Vec<f32>) {
    if !samples.is_empty() {
        (callbacks.on_mic)(samples, CAPTURE_SAMPLE_RATE);
    }
}

/// A running capture. [`CaptureHandle::stop`] (or dropping the handle) stops
/// both streams; once it returns no callback is running or will run again.
pub struct CaptureHandle {
    #[cfg(target_os = "macos")]
    inner: Option<macos::Capture>,
    delivery: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    /// Stop capturing and wait for the delivery thread to drain the queued
    /// audio. Idempotent. Calling it from inside a capture callback does not
    /// wait for the drain (it would deadlock).
    pub fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(inner) = self.inner.take() {
            inner.stop();
        }
        if let Some(thread) = self.delivery.take() {
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.delivery.is_some()
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the mic (echo-cancelled) and system-audio streams. Fails, with
/// nothing left running, if either cannot start or mic access is not granted.
///
/// - Mic: AVAudioEngine, the plain input (the built-in mic instead of a
///   Bluetooth headset's). It never touches the output device or other apps'
///   audio. Restarts itself on audio device changes. With the system stream
///   too, the echo of the meeting playing through the speakers is removed
///   from it ([`aec`], AEC3 with the system track as reference).
/// - System: a process tap of all output except Guidy's own process. The
///   stream is continuous and wall-clock aligned: silence is filled in while
///   nothing plays (the tap itself delivers nothing then), and it is rebuilt
///   when the default output device changes. Without the system-audio grant
///   the tap starts but the stream is all silence.
pub fn start_capture(callbacks: CaptureCallbacks) -> Result<CaptureHandle> {
    start_capture_streams(CaptureStreams::default(), callbacks)
}

/// Like [`start_capture`] with a choice of streams.
pub fn start_capture_streams(
    streams: CaptureStreams,
    callbacks: CaptureCallbacks,
) -> Result<CaptureHandle> {
    if !streams.mic && !streams.system {
        anyhow::bail!("no capture stream requested");
    }
    #[cfg(target_os = "macos")]
    {
        if !is_supported() {
            return Err(unsupported());
        }
        // Without the grant the mic would record silence without any error.
        if streams.mic && !crate::permissions::check_microphone_permission() {
            anyhow::bail!("microphone access not granted");
        }
        let (tx, rx) = mpsc::channel();
        let inner = macos::Capture::start(streams, tx)?;
        let echo = (streams.mic && streams.system).then(EchoCanceller::new);
        let delivery = std::thread::Builder::new()
            .name("meeting-capture".into())
            .spawn(move || deliver(rx, callbacks, echo))?;
        Ok(CaptureHandle {
            inner: Some(inner),
            delivery: Some(delivery),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = callbacks;
        Err(unsupported())
    }
}

/// System-audio (process tap) permission. Best effort: macOS has no public
/// query, so this uses the TCC preflight when it can and otherwise returns
/// [`AuthStatus::Unknown`].
pub fn system_audio_status() -> AuthStatus {
    #[cfg(target_os = "macos")]
    return macos::system_audio_status();
    #[cfg(not(target_os = "macos"))]
    AuthStatus::Unavailable
}

/// Make macOS ask for system-audio access (blocking until answered, bounded).
/// There is no public request API: this uses the TCC request when available,
/// else starts a short probe tap, which shows the prompt. Returns true when
/// access is (known to be) granted.
pub fn request_system_audio_access() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_system_audio_access();
    #[cfg(not(target_os = "macos"))]
    false
}

// ===== Notifications =======================================================

/// False when UNUserNotificationCenter can't be used: other OS, or an
/// unbundled binary (`cargo run`) without a bundle identifier.
pub fn notifications_available() -> bool {
    #[cfg(target_os = "macos")]
    return macos::notifications_available();
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn notification_status() -> AuthStatus {
    #[cfg(target_os = "macos")]
    return macos::notification_status();
    #[cfg(not(target_os = "macos"))]
    AuthStatus::Unavailable
}

/// Ask for permission to show alerts with sound (blocking). Returns false
/// when denied or unavailable (logged).
pub fn request_notification_access() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_notification_access();
    #[cfg(not(target_os = "macos"))]
    false
}

/// Ids of the notifications this process posted and hasn't removed, so the
/// manager can withdraw stale ones (calendar prompts, reminders). Entries for
/// notifications the user already dismissed are harmless: removing a missing
/// notification is a no-op.
fn posted_ids() -> &'static parking_lot::Mutex<std::collections::BTreeSet<String>> {
    static POSTED: std::sync::OnceLock<parking_lot::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    POSTED.get_or_init(Default::default)
}

/// Show a notification now. `id` identifies it (posting the same id again
/// replaces it, and the handler receives it back). `actions` are
/// `(action_id, button title)` pairs. Returns false when notifications are
/// unavailable (logged), never panics.
pub fn post_notification(id: &str, title: &str, body: &str, actions: &[(&str, &str)]) -> bool {
    #[cfg(target_os = "macos")]
    let posted = macos::post_notification(id, title, body, actions);
    #[cfg(not(target_os = "macos"))]
    let posted = {
        let _ = (id, title, body, actions);
        false
    };
    if posted {
        posted_ids().lock().insert(id.to_string());
    }
    posted
}

/// Remove a pending or delivered notification.
pub fn remove_notification(id: &str) {
    posted_ids().lock().remove(id);
    #[cfg(target_os = "macos")]
    macos::remove_notification(id);
}

/// Ids of the notifications posted by this process (and not removed since)
/// that start with `prefix`.
pub fn posted_notifications(prefix: &str) -> Vec<String> {
    posted_ids()
        .lock()
        .iter()
        .filter(|id| id.starts_with(prefix))
        .cloned()
        .collect()
}

/// Remove every notification from [`posted_notifications`]`(prefix)`.
pub fn remove_notifications(prefix: &str) {
    for id in posted_notifications(prefix) {
        remove_notification(&id);
    }
}

/// Register the handler for notification clicks: `(notification_id,
/// action_id)`, where `action_id` is one of the ids passed to
/// [`post_notification`] or [`NOTIFICATION_DEFAULT_ACTION`] for a click on the
/// body. Replaces any previous handler. Call it early at startup so a click
/// that activates the app is delivered.
pub fn set_notification_handler(handler: impl Fn(&str, &str) + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    macos::set_notification_handler(Box::new(handler));
    #[cfg(not(target_os = "macos"))]
    let _ = handler;
}

// ===== Calls in other apps =================================================

/// The other apps capturing from the microphone now, with their window
/// titles for browsers ([`crate::meetings::calls::title_source`]: is Meet
/// open?). Empty before macOS 14 and on other platforms. Titles need the
/// Accessibility permission (dictation's paste has it); without it a Meet
/// call in a browser goes unseen.
pub fn audio_input_apps() -> Vec<crate::meetings::calls::InputApp> {
    #[cfg(target_os = "macos")]
    {
        use crate::meetings::calls::{title_source, InputApp};
        macos::audio_input_bundle_ids()
            .into_iter()
            .map(|bundle_id| InputApp {
                titles: title_source(&bundle_id)
                    .map(macos::window_titles)
                    .unwrap_or_default(),
                bundle_id,
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

// ===== MeetingsManager support (permissions, upcoming events) ===============

/// Window of [`upcoming_events`]: from an hour ago to a day ahead.
pub const UPCOMING_PAST_MS: i64 = 3600 * 1000;
pub const UPCOMING_AHEAD_MS: i64 = 24 * 3600 * 1000;

/// Events from now-1h to now+24h that have a meeting link.
pub fn upcoming_events() -> Result<Vec<CalendarEvent>> {
    if !is_supported() {
        return Err(unsupported());
    }
    let now = chrono::Utc::now().timestamp_millis();
    events_between(now - UPCOMING_PAST_MS, now + UPCOMING_AHEAD_MS)
}

/// Current grants, without prompting.
pub fn permission_status() -> PermissionReport {
    if !is_supported() {
        return PermissionReport::default();
    }
    PermissionReport {
        calendar_granted: calendar_authorization().is_granted(),
        system_audio_granted: system_audio_status().is_granted(),
        microphone_granted: crate::permissions::check_microphone_permission(),
        notifications_granted: notification_status().is_granted(),
    }
}

/// Ask for the microphone, calendar, system-audio and notification
/// permissions in turn (each blocks until answered) and report the result.
/// One refusal does not stop the others.
pub fn request_permissions() -> Result<PermissionReport> {
    if !is_supported() {
        return Err(unsupported());
    }
    let microphone_granted = request_microphone_blocking();
    let calendar_granted = request_calendar_access();
    let system_audio_granted = request_system_audio_access();
    let notifications_granted = request_notification_access();
    let report = PermissionReport {
        calendar_granted,
        system_audio_granted,
        microphone_granted,
        notifications_granted,
    };
    log::info!("Meeting notes permissions: {report:?}");
    Ok(report)
}

/// The app's microphone request is fire-and-forget; wait (bounded) for the
/// user's answer so the report is accurate.
fn request_microphone_blocking() -> bool {
    use crate::permissions::{microphone_permission_state, PermissionState};
    if microphone_permission_state() == PermissionState::NotDetermined {
        crate::permissions::request_microphone_permission();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while microphone_permission_state() == PermissionState::NotDetermined
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    crate::permissions::check_microphone_permission()
}

#[cfg(test)]
mod tests;
