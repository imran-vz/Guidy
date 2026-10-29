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

    fn guidy_mb_audio_input_bundle_ids() -> *mut c_char;
    fn guidy_mb_window_titles(bundle_id: *const c_char) -> *mut c_char;
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
    // Runs on an audio thread: copy and hand off, nothing else. This is the
    // only copy; the Vec is moved from here on (delivery thread, sink, writer).
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

// ===== Calls in other apps =================================================

fn strings(json: Option<String>) -> Vec<String> {
    json.and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

pub(super) fn audio_input_bundle_ids() -> Vec<String> {
    strings(unsafe { take_string(guidy_mb_audio_input_bundle_ids()) })
}

pub(super) fn window_titles(bundle_id: &str) -> Vec<String> {
    let id = c_string(bundle_id);
    strings(unsafe { take_string(guidy_mb_window_titles(id.as_ptr())) })
}

#[cfg(test)]
mod aligner_tests {
    //! The Swift `TimelineAligner` (mic and system tracks on one timeline),
    //! driven through the `guidy_mb_test_aligner_*` hooks.

    use std::ffi::c_void;

    extern "C" {
        fn guidy_mb_test_aligner_new(start_nanos: u64, tolerance_samples: i64) -> *mut c_void;
        fn guidy_mb_test_aligner_audio(
            aligner: *mut c_void,
            samples: *const f32,
            count: i32,
            at_nanos: u64,
            out: *mut f32,
            capacity: i64,
        ) -> i64;
        fn guidy_mb_test_aligner_tick(
            aligner: *mut c_void,
            now_nanos: u64,
            out: *mut f32,
            capacity: i64,
        ) -> i64;
        fn guidy_mb_test_aligner_free(aligner: *mut c_void);
    }

    const START: u64 = 1_000_000_000_000;
    const MS: u64 = 1_000_000;
    const TOLERANCE: i64 = 800; // 50 ms, the bridge default

    /// Everything the aligner emitted so far, as one track.
    struct Aligner {
        raw: *mut c_void,
        track: Vec<f32>,
    }

    impl Aligner {
        fn new() -> Self {
            Self {
                raw: unsafe { guidy_mb_test_aligner_new(START, TOLERANCE) },
                track: Vec::new(),
            }
        }

        /// `count` samples of `value` captured `at_ms` after the origin.
        fn audio(&mut self, at_ms: f64, count: usize, value: f32) -> usize {
            let input = vec![value; count];
            let mut out = vec![0f32; count + 16_000 * 120];
            let n = unsafe {
                guidy_mb_test_aligner_audio(
                    self.raw,
                    input.as_ptr(),
                    count as i32,
                    START + (at_ms * MS as f64) as u64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                )
            } as usize;
            self.track.extend_from_slice(&out[..n]);
            n
        }

        fn tick(&mut self, now_ms: u64) -> usize {
            let mut out = vec![0f32; 16_000 * 120];
            let n = unsafe {
                guidy_mb_test_aligner_tick(
                    self.raw,
                    START + now_ms * MS,
                    out.as_mut_ptr(),
                    out.len() as i64,
                )
            } as usize;
            self.track.extend_from_slice(&out[..n]);
            n
        }
    }

    impl Drop for Aligner {
        fn drop(&mut self) {
            unsafe { guidy_mb_test_aligner_free(self.raw) }
        }
    }

    fn samples_at(ms: u64) -> usize {
        (ms * 16) as usize
    }

    #[test]
    fn late_start_is_padded_with_leading_silence() {
        // The mic took 200 ms to deliver its first buffer.
        let mut a = Aligner::new();
        assert_eq!(a.audio(200.0, 160, 1.0), samples_at(200) + 160);
        assert!(a.track[..samples_at(200)].iter().all(|&s| s == 0.0));
        assert!(a.track[samples_at(200)..].iter().all(|&s| s == 1.0));
    }

    #[test]
    fn small_jitter_is_left_alone() {
        let mut a = Aligner::new();
        a.audio(0.0, 1_600, 1.0);
        // 30 ms late, then 30 ms early: within 50 ms, no padding or trimming.
        assert_eq!(a.audio(130.0, 1_600, 1.0), 1_600);
        assert_eq!(a.audio(170.0, 1_600, 1.0), 1_600);
        assert_eq!(a.track.len(), 4_800);
    }

    #[test]
    fn a_restart_gap_becomes_silence_at_the_right_place() {
        // 100 ms of audio, then the device changed and the mic restarted: the
        // next buffer was captured 2 s after the origin.
        let mut a = Aligner::new();
        a.audio(0.0, 1_600, 1.0);
        a.audio(2_000.0, 1_600, 2.0);
        assert_eq!(a.track.len(), samples_at(2_000) + 1_600);
        assert!(a.track[1_600..samples_at(2_000)].iter().all(|&s| s == 0.0));
        // Sample index / 16 kHz is the capture time.
        assert_eq!(a.track[samples_at(2_000)], 2.0);
    }

    #[test]
    fn overlap_is_trimmed_or_dropped() {
        let mut a = Aligner::new();
        // 0..200 ms.
        a.audio(0.0, 3_200, 1.0);
        // A chunk claiming 0 ms again overlaps completely: dropped.
        assert_eq!(a.audio(0.0, 1_600, 2.0), 0);
        // 50..250 ms: the first 150 ms overlap, only the last 50 ms are kept.
        assert_eq!(a.audio(50.0, 3_200, 3.0), 800);
        assert_eq!(a.track.len(), samples_at(250));
        assert!(a.track[3_200..].iter().all(|&s| s == 3.0));
    }

    #[test]
    fn tick_fills_a_silent_stream_up_to_shortly_before_now() {
        let mut a = Aligner::new();
        // Nothing for 200 ms: not idle long enough yet.
        assert_eq!(a.tick(200), 0);
        // At 1 s: padded up to 900 ms (100 ms left for audio in flight).
        assert_eq!(a.tick(1_000), samples_at(900));
        assert!(a.track.iter().all(|&s| s == 0.0));
        // Audio captured at 950 ms continues the track without a jump.
        assert_eq!(a.audio(950.0, 160, 1.0), 160);
        // Just after audio: no padding.
        assert_eq!(a.tick(1_100), 0);
    }

    #[test]
    fn a_long_drifting_stream_stays_within_tolerance_of_the_wall_clock() {
        // A device clock 0.5% fast and jittery timestamps: after every chunk
        // the track length matches the chunk's end time within tolerance.
        let mut a = Aligner::new();
        let mut t = 0.0f64;
        for i in 0..2_000 {
            let jitter = ((i * 7919) % 11) as f64 - 5.0; // -5..5 ms
            let at = (t + jitter).max(0.0);
            a.audio(at, 1_365, 0.5);
            let end = (at * 16.0) as i64 + 1_365;
            let drift = a.track.len() as i64 - end;
            assert!(drift.abs() <= TOLERANCE + 1_365, "chunk {i}: drift {drift}");
            t += 1_365.0 / 16.0 / 1.005;
        }
        let total_ms = t;
        let expected = (total_ms * 16.0) as i64;
        assert!((a.track.len() as i64 - expected).abs() <= TOLERANCE + 160);
    }
}
