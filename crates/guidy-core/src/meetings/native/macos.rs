//! FFI to `swift/meeting_bridge.swift` (C ABI in `swift/meeting_bridge.h`).

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use std::sync::{mpsc, Arc, RwLock};

use anyhow::{anyhow, Result};

use super::{AuthStatus, CaptureEvent, CaptureMsg, CaptureStreams, RawEvent};
use crate::meetings::types::Track;

type AudioCallback =
    unsafe extern "C" fn(ctx: *mut c_void, stream: i32, samples: *const f32, count: i32, rate: i32);
type EventCallback = unsafe extern "C" fn(ctx: *mut c_void, stream: i32, message: *const c_char);
type NotificationCallback =
    unsafe extern "C" fn(notification_id: *const c_char, action_id: *const c_char);

const STREAM_SYSTEM: i32 = 1;

extern "C" {
    fn guidy_mb_free_string(s: *mut c_char);
    fn guidy_mb_is_supported() -> i32;

    fn guidy_mb_calendar_auth_status() -> i32;
    fn guidy_mb_request_calendar_access() -> i32;
    fn guidy_mb_events_between(
        start_ms: i64,
        end_ms: i64,
        error_out: *mut *mut c_char,
    ) -> *mut c_char;

    fn guidy_mb_capture_start(
        want_mic: i32,
        want_system: i32,
        audio_cb: Option<AudioCallback>,
        event_cb: Option<EventCallback>,
        ctx: *mut c_void,
        error_out: *mut *mut c_char,
    ) -> *mut c_void;
    fn guidy_mb_capture_stop(handle: *mut c_void);
    fn guidy_mb_system_audio_status() -> i32;
    fn guidy_mb_request_system_audio_access() -> i32;

    fn guidy_mb_notifications_available() -> i32;
    fn guidy_mb_notification_auth_status() -> i32;
    fn guidy_mb_request_notification_access() -> i32;
    fn guidy_mb_post_notification(
        notification_id: *const c_char,
        title: *const c_char,
        body: *const c_char,
        actions_json: *const c_char,
    ) -> i32;
    fn guidy_mb_remove_notification(notification_id: *const c_char);
    fn guidy_mb_set_notification_handler(cb: Option<NotificationCallback>);
}

/// Take ownership of a bridge string (may be null).
unsafe fn take_string(s: *mut c_char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let out = CStr::from_ptr(s).to_string_lossy().into_owned();
    guidy_mb_free_string(s);
    Some(out)
}

/// Interior NULs can't cross the C boundary; drop them.
fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).expect("NULs removed")
}

pub(super) fn is_supported() -> bool {
    unsafe { guidy_mb_is_supported() == 1 }
}

// ===== Calendar ============================================================

pub(super) fn calendar_authorization() -> AuthStatus {
    AuthStatus::from_raw(unsafe { guidy_mb_calendar_auth_status() })
}

pub(super) fn request_calendar_access() -> bool {
    if calendar_authorization() == AuthStatus::Granted {
        return true;
    }
    let granted = unsafe { guidy_mb_request_calendar_access() == 1 };
    log::info!("Calendar access request finished: granted={granted}");
    granted
}

pub(super) fn raw_events_between(start_ms: i64, end_ms: i64) -> Result<Vec<RawEvent>> {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { guidy_mb_events_between(start_ms, end_ms, &mut error) };
    let json = unsafe { take_string(json) };
    let error = unsafe { take_string(error) };
    let json = json.ok_or_else(|| {
        anyhow!(
            "reading calendar events failed: {}",
            error.unwrap_or_else(|| "unknown error".into())
        )
    })?;
    Ok(serde_json::from_str(&json)?)
}

// ===== Capture =============================================================

type Ctx = mpsc::Sender<CaptureMsg>;

unsafe extern "C" fn audio_trampoline(
    ctx: *mut c_void,
    stream: i32,
    samples: *const f32,
    count: i32,
    rate: i32,
) {
    if ctx.is_null() || samples.is_null() || count <= 0 {
        return;
    }
    // Runs on an audio thread: copy and hand off, nothing else.
    let tx = &*(ctx as *const Ctx);
    let data = std::slice::from_raw_parts(samples, count as usize).to_vec();
    let _ = tx.send(CaptureMsg::Audio(track(stream), data, rate.max(0) as u32));
}

unsafe extern "C" fn event_trampoline(ctx: *mut c_void, stream: i32, message: *const c_char) {
    if ctx.is_null() || message.is_null() {
        return;
    }
    let tx = &*(ctx as *const Ctx);
    let message = CStr::from_ptr(message).to_string_lossy().into_owned();
    let _ = tx.send(CaptureMsg::Event(CaptureEvent {
        track: track(stream),
        message,
    }));
}

fn track(stream: i32) -> Track {
    if stream == STREAM_SYSTEM {
        Track::System
    } else {
        Track::Mic
    }
}

/// A started Swift capture session plus the channel sender its callbacks use.
pub(super) struct Capture {
    handle: *mut c_void,
    ctx: *mut Ctx,
}

// The handle is an opaque, internally synchronized Swift object; the context
// is a `Sender`, which is `Send + Sync`.
unsafe impl Send for Capture {}

impl Capture {
    pub(super) fn start(streams: CaptureStreams, tx: Ctx) -> Result<Self> {
        let ctx = Box::into_raw(Box::new(tx));
        let mut error: *mut c_char = ptr::null_mut();
        let handle = unsafe {
            guidy_mb_capture_start(
                streams.mic as i32,
                streams.system as i32,
                Some(audio_trampoline),
                Some(event_trampoline),
                ctx as *mut c_void,
                &mut error,
            )
        };
        let error = unsafe { take_string(error) };
        if handle.is_null() {
            // Nothing is running on the Swift side; the context is ours again.
            drop(unsafe { Box::from_raw(ctx) });
            return Err(anyhow!(
                "starting meeting capture failed: {}",
                error.unwrap_or_else(|| "unknown error".into())
            ));
        }
        Ok(Self { handle, ctx })
    }

    pub(super) fn stop(self) {
        drop(self);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // After capture_stop returns no callback runs, so the sender can go;
        // dropping it ends the delivery thread once the queue is drained.
        unsafe {
            guidy_mb_capture_stop(self.handle);
            drop(Box::from_raw(self.ctx));
        }
    }
}

pub(super) fn system_audio_status() -> AuthStatus {
    AuthStatus::from_raw(unsafe { guidy_mb_system_audio_status() })
}

pub(super) fn request_system_audio_access() -> bool {
    let status = AuthStatus::from_raw(unsafe { guidy_mb_request_system_audio_access() });
    log::info!("System audio access request finished: {status:?}");
    status == AuthStatus::Granted
}

// ===== Notifications =======================================================

type Handler = Arc<dyn Fn(&str, &str) + Send + Sync>;

static NOTIFICATION_HANDLER: RwLock<Option<Handler>> = RwLock::new(None);

unsafe extern "C" fn notification_trampoline(id: *const c_char, action: *const c_char) {
    if id.is_null() || action.is_null() {
        return;
    }
    let id = CStr::from_ptr(id).to_string_lossy();
    let action = CStr::from_ptr(action).to_string_lossy();
    let handler = NOTIFICATION_HANDLER
        .read()
        .ok()
        .and_then(|h| h.as_ref().cloned());
    match handler {
        Some(handler) => handler(&id, &action),
        None => log::warn!("Notification action {action} for {id} with no handler set"),
    }
}

fn available_or_log(what: &str) -> bool {
    let ok = notifications_available();
    if !ok {
        log::info!("Notifications unavailable (no bundle identifier; unbundled binary?): {what}");
    }
    ok
}

pub(super) fn notifications_available() -> bool {
    unsafe { guidy_mb_notifications_available() == 1 }
}

pub(super) fn notification_status() -> AuthStatus {
    AuthStatus::from_raw(unsafe { guidy_mb_notification_auth_status() })
}

pub(super) fn request_notification_access() -> bool {
    if !available_or_log("not requesting access") {
        return false;
    }
    let granted = unsafe { guidy_mb_request_notification_access() == 1 };
    log::info!("Notification access request finished: granted={granted}");
    granted
}

pub(super) fn post_notification(
    id: &str,
    title: &str,
    body: &str,
    actions: &[(&str, &str)],
) -> bool {
    if !available_or_log(&format!("not posting {id:?}")) {
        return false;
    }
    let actions_json = serde_json::to_string(
        &actions
            .iter()
            .map(|(a, t)| [*a, *t])
            .collect::<Vec<[&str; 2]>>(),
    )
    .unwrap_or_else(|_| "[]".into());
    let (id, title, body, actions_json) = (
        c_string(id),
        c_string(title),
        c_string(body),
        c_string(&actions_json),
    );
    unsafe {
        guidy_mb_post_notification(
            id.as_ptr(),
            title.as_ptr(),
            body.as_ptr(),
            actions_json.as_ptr(),
        ) == 1
    }
}

pub(super) fn remove_notification(id: &str) {
    if !notifications_available() {
        return;
    }
    let id = c_string(id);
    unsafe { guidy_mb_remove_notification(id.as_ptr()) }
}

#[allow(clippy::type_complexity)]
pub(super) fn set_notification_handler(handler: Box<dyn Fn(&str, &str) + Send + Sync>) {
    if let Ok(mut slot) = NOTIFICATION_HANDLER.write() {
        *slot = Some(Arc::from(handler));
    }
    if !available_or_log("handler stored, but no clicks will arrive") {
        return;
    }
    unsafe { guidy_mb_set_notification_handler(Some(notification_trampoline)) }
}
