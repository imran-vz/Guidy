//! Recording overlay control.
//!
//! The overlay window itself is drawn by the UI layer; this module decides
//! *when* it shows, in which state, and *where* (cursor monitor, top/bottom
//! offsets), then asks the [`crate::host::Shell`] to place it and emits the
//! same `show-overlay` / `hide-overlay` / `recording-ready` / `mic-level`
//! events the overlay view consumes.

use crate::host::{AppHandle, Monitor, OverlayPlacement, PhysicalPosition, PhysicalSize};
use crate::input;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// Overlay window sizes (logical points). One window is reused for every state
// and resized per state; each size need only be at least as large as the card
// it hosts.
//
// Compact overlay (Minimal / transcribing / processing): the 40h pill animates
// width from 172 to 216 and expands from center, so the window must fit the
// widest state plus a little slack.
pub const OVERLAY_WIDTH: f64 = 256.0;
pub const OVERLAY_HEIGHT: f64 = 50.0;

// Actual is 394x118, just a little extra
pub const OVERLAY_STREAM_WIDTH: f64 = 400.0;
pub const OVERLAY_STREAM_HEIGHT: f64 = 120.0;

/// Overlay window size (logical) for a given UI state.
pub fn overlay_dimensions(state: &str) -> (f64, f64) {
    if state == "streaming" {
        (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT)
    } else {
        (OVERLAY_WIDTH, OVERLAY_HEIGHT)
    }
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

#[cfg(target_os = "macos")]
pub const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(not(target_os = "macos"))]
pub const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
pub const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;
#[cfg(not(target_os = "macos"))]
pub const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

/// No-op kept for call-site parity: the UI creates the overlay window at
/// startup (hidden) and the shell owns it.
pub fn create_recording_overlay(_app_handle: &AppHandle) {}

fn get_monitor_with_cursor(app_handle: &AppHandle) -> Option<Monitor> {
    let monitors = app_handle.available_monitors().unwrap_or_default();
    if let Some(mouse_location) = input::get_cursor_position(app_handle) {
        if let Some(m) = monitor_containing(&monitors, mouse_location) {
            return Some(m);
        }
    }
    app_handle.primary_monitor().ok().flatten()
}

/// The monitor under `mouse_location`. On Windows both the cursor
/// (enigo → GetCursorPos) and monitor bounds are physical pixels; on
/// macOS/Linux enigo returns logical coordinates, so the bounds are scaled down.
fn monitor_containing(monitors: &[Monitor], mouse_location: (i32, i32)) -> Option<Monitor> {
    for monitor in monitors {
        #[cfg(target_os = "windows")]
        if is_mouse_within_monitor(mouse_location, monitor.position(), monitor.size()) {
            return Some(monitor.clone());
        }

        #[cfg(not(target_os = "windows"))]
        {
            let scale = monitor.scale_factor();
            let pos = PhysicalPosition::new(
                (monitor.position().x as f64 / scale) as i32,
                (monitor.position().y as f64 / scale) as i32,
            );
            let size = PhysicalSize::new(
                (monitor.size().width as f64 / scale) as u32,
                (monitor.size().height as f64 / scale) as u32,
            );
            if is_mouse_within_monitor(mouse_location, &pos, &size) {
                return Some(monitor.clone());
            }
        }
    }
    None
}

fn is_mouse_within_monitor(
    mouse_pos: (i32, i32),
    monitor_pos: &PhysicalPosition<i32>,
    monitor_size: &PhysicalSize<u32>,
) -> bool {
    let (mouse_x, mouse_y) = mouse_pos;
    let PhysicalPosition {
        x: monitor_x,
        y: monitor_y,
    } = *monitor_pos;
    let PhysicalSize {
        width: monitor_width,
        height: monitor_height,
    } = *monitor_size;

    mouse_x >= monitor_x
        && mouse_x < (monitor_x + monitor_width as i32)
        && mouse_y >= monitor_y
        && mouse_y < (monitor_y + monitor_height as i32)
}

/// Overlay position in logical coordinates for a window of `width`×`height`
/// on `monitor`.
///
/// On macOS the Bottom anchor uses the work area (visibleFrame) so the overlay
/// tracks the Dock — above it when shown, at the screen edge when hidden. Top
/// and the other platforms use full monitor bounds plus the fixed offsets
/// (work_area is unreliable on Wayland; Windows' offset clears the taskbar).
pub fn overlay_position_on_monitor(
    monitor: &Monitor,
    width: f64,
    height: f64,
    position: OverlayPosition,
) -> (f64, f64) {
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;

    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = match position {
        OverlayPosition::Top => monitor_y + OVERLAY_TOP_OFFSET,
        OverlayPosition::Bottom => {
            #[cfg(target_os = "macos")]
            let bottom = {
                let wa = monitor.work_area();
                (wa.position.y as f64 + wa.size.height as f64) / scale
            };
            #[cfg(not(target_os = "macos"))]
            let bottom = monitor_y + monitor.size().height as f64 / scale;

            bottom - height - OVERLAY_BOTTOM_OFFSET
        }
    };
    (x, y)
}

fn placement_for(app_handle: &AppHandle, state: &str) -> OverlayPlacement {
    let (mut width, mut height) = overlay_dimensions(state);
    #[cfg(target_os = "windows")]
    {
        let text_scale = windows_text_scale_factor();
        width *= text_scale;
        height *= text_scale;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (&mut width, &mut height);
    }
    let position = settings::get_settings(app_handle).overlay_position;
    let (x, y) = get_monitor_with_cursor(app_handle)
        .map(|m| overlay_position_on_monitor(&m, width, height, position))
        .unwrap_or((0.0, 0.0));
    OverlayPlacement {
        state: state.to_string(),
        x,
        y,
        width,
        height,
        top: position == OverlayPosition::Top,
    }
}

#[cfg(target_os = "windows")]
fn windows_text_scale_factor() -> f64 {
    // Absent until the user moves the slider off 100%; stored as a percentage.
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Accessibility")
        .and_then(|key| key.get_value::<u32, _>("TextScaleFactor"))
        .map(|percent| (percent as f64 / 100.0).clamp(1.0, 2.25))
        .unwrap_or(1.0)
}

/// Overlay rectangle in the destination monitor's physical pixels, so nothing
/// is converted through the window's previous-monitor DPI.
#[cfg(target_os = "windows")]
fn windows_overlay_bounds(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    scale: f64,
    text_scale: f64,
    logical_width: f64,
    logical_height: f64,
    overlay_position: OverlayPosition,
) -> (i32, i32, i32, i32) {
    // Grow the window with the text scale; offsets stay DPI-only since the
    // card sits flush against the window's screen-edge side.
    let content_scale = scale * text_scale;
    let width = (logical_width * content_scale).round().max(1.0) as i32;
    let height = (logical_height * content_scale).round().max(1.0) as i32;
    let x = (monitor_position.x as f64 + (monitor_size.width as f64 - width as f64) / 2.0).round()
        as i32;
    let y = match overlay_position {
        OverlayPosition::Top => {
            (monitor_position.y as f64 + OVERLAY_TOP_OFFSET * scale).round() as i32
        }
        OverlayPosition::Bottom => (monitor_position.y as f64 + monitor_size.height as f64
            - height as f64
            - OVERLAY_BOTTOM_OFFSET * scale)
            .round() as i32,
    };

    (x, y, width, height)
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Whether the overlay shows at all is governed by overlay_style; position
    // only chooses Top vs Bottom placement.
    let settings = settings::get_settings(app_handle);
    if settings.overlay_style == OverlayStyle::None {
        return;
    }

    // Monitor/cursor lookups and window geometry run on the main thread.
    let handle = app_handle.clone();
    let state = state.to_string();
    let _ = app_handle.run_on_main_thread(move || show_overlay_state_on_main(&handle, &state));
}

fn show_overlay_state_on_main(app_handle: &AppHandle, state: &str) {
    // Invalidate any delayed hide still in flight from a previous session
    // (see `hide_recording_overlay`).
    OVERLAY_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst);
    let placement = placement_for(app_handle, state);
    log::debug!("overlay '{}': {:?}", state, placement);
    app_handle.shell().show_overlay(placement);
    let _ = app_handle.emit_to("recording_overlay", "show-overlay", state);
}

/// Notify the visible recording overlay that the input stream has delivered its
/// first sample chunk. Audio feedback uses the same backend readiness signal,
/// but this targeted event is skipped when overlays are disabled.
pub fn emit_recording_ready(app_handle: &AppHandle) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Showing the overlay is also queued onto the main thread. Queue readiness
    // there as well so a very fast always-on stream cannot overtake show-overlay
    // and then get reset back to the arming state by the UI.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let _ = handle.emit_to("recording_overlay", "recording-ready", ());
    });
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the larger streaming overlay that displays live transcription text
pub fn show_streaming_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "streaming");
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "transcribing");
}

/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}

/// Updates the overlay window position based on current settings
pub fn update_overlay_position(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || update_overlay_position_on_main(&handle));
}

fn update_overlay_position_on_main(app_handle: &AppHandle) {
    let shell = app_handle.shell();
    // Use the window's current size so centering stays correct whether the
    // overlay is in compact or streaming layout.
    let Some((width, height)) = shell.overlay_size() else {
        return;
    };
    let state = if width >= OVERLAY_STREAM_WIDTH {
        "streaming"
    } else {
        "recording"
    };
    let mut placement = placement_for(app_handle, state);
    placement.width = width;
    placement.height = height;
    if let Some(m) = get_monitor_with_cursor(app_handle) {
        let position = settings::get_settings(app_handle).overlay_position;
        let (x, y) = overlay_position_on_monitor(&m, width, height, position);
        placement.x = x;
        placement.y = y;
    }
    shell.move_overlay(placement);
}

/// Generation counter bumped every time the overlay is shown. The delayed
/// `hide()` below only unmaps the window if no show happened after it was
/// scheduled, so a hide left over from a finished transcription can never
/// take down the overlay of a session that started in the meantime.
static OVERLAY_SHOW_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Hides the recording overlay window with fade-out animation
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    // Always hide the overlay regardless of settings - if setting was changed while recording,
    // we still want to hide it properly
    let scheduled_at = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
    // Emit event to trigger fade-out animation
    let _ = app_handle.emit_to("recording_overlay", "hide-overlay", ());
    // Hide the window after a short delay to allow animation to complete,
    // unless a newer session has shown the overlay again by then.
    let handle = app_handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) != scheduled_at {
            log::debug!("Skipping stale overlay hide: a newer session is showing the overlay");
            return;
        }
        let shell = handle.shell();
        let _ = handle.run_on_main_thread(move || shell.hide_overlay());
    });
}

// Cached "overlay is enabled" flag, kept in sync with overlay_style. Avoids
// reading the settings store on every audio callback (~24 Hz during recording).
// Defaults to false so the audio path doesn't emit until startup populates
// the cache from initial settings.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Update the cached overlay-enabled flag. Called at startup after settings
/// load, and from `change_overlay_style_setting`.
pub fn update_overlay_enabled_cache(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn emit_levels(app_handle: &AppHandle, levels: &[f32]) {
    // Skip emission when the overlay is disabled.
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Throttle to ~30 FPS: the raw audio callback fires far faster than the
    // UI needs.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last = LAST_MIC_LEVEL_EMIT.load(Ordering::Relaxed);
    if now.saturating_sub(last) < EMIT_THROTTLE_MS {
        return;
    }
    LAST_MIC_LEVEL_EMIT.store(now, Ordering::Relaxed);

    let _ = app_handle.emit_to("recording_overlay", "mic-level", levels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_hit_test_uses_half_open_physical_bounds() {
        let position = PhysicalPosition::new(-2560, -200);
        let size = PhysicalSize::new(2560, 1440);

        assert!(is_mouse_within_monitor((-2560, -200), &position, &size));
        assert!(is_mouse_within_monitor((-1, 1239), &position, &size));
        assert!(!is_mouse_within_monitor((0, 0), &position, &size));
        assert!(!is_mouse_within_monitor((-1, 1240), &position, &size));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_cursor_hit_test_does_not_scale_physical_monitor_bounds() {
        let position = PhysicalPosition::new(1920, 0);
        let size = PhysicalSize::new(3840, 2160);
        let cursor = (5000, 1000);

        assert!(is_mouse_within_monitor(cursor, &position, &size));

        // This is the old mixed-coordinate comparison. It excludes a cursor
        // that is visibly inside a secondary display running at 150%.
        let scale = 1.5;
        let logical_position = PhysicalPosition::new(
            (position.x as f64 / scale) as i32,
            (position.y as f64 / scale) as i32,
        );
        let logical_size = PhysicalSize::new(
            (size.width as f64 / scale) as u32,
            (size.height as f64 / scale) as u32,
        );
        assert!(!is_mouse_within_monitor(
            cursor,
            &logical_position,
            &logical_size
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_use_destination_monitor_scale() {
        let monitor_position = PhysicalPosition::new(1920, 0);
        let monitor_size = PhysicalSize::new(3840, 2160);

        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                1.0,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (3648, 2025, 384, 75)
        );
        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                1.0,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Top,
            ),
            (3648, 6, 384, 75)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_support_negative_monitor_origins() {
        assert_eq!(
            windows_overlay_bounds(
                PhysicalPosition::new(-2560, -200),
                PhysicalSize::new(2560, 1440),
                1.25,
                1.0,
                OVERLAY_STREAM_WIDTH,
                OVERLAY_STREAM_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (-1530, 1040, 500, 150)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_grow_with_text_scale_without_moving_the_anchored_edge() {
        let monitor_position = PhysicalPosition::new(-2560, -200);
        let monitor_size = PhysicalSize::new(2560, 1440);

        let (x, y, width, height) = windows_overlay_bounds(
            monitor_position,
            monitor_size,
            1.25,
            1.1,
            OVERLAY_STREAM_WIDTH,
            OVERLAY_STREAM_HEIGHT,
            OverlayPosition::Bottom,
        );
        // 400x120 logical at 1.25 DPI x 1.1 text, still centered horizontally.
        assert_eq!((x, y, width, height), (-1555, 1025, 550, 165));
        // Bottom edge unchanged from the 1.0 case above (1040 + 150).
        assert_eq!(y + height, 1190);

        let (_, top_y, _, _) = windows_overlay_bounds(
            monitor_position,
            monitor_size,
            1.25,
            1.1,
            OVERLAY_STREAM_WIDTH,
            OVERLAY_STREAM_HEIGHT,
            OverlayPosition::Top,
        );
        // Top offset rides the DPI scale alone, so the top edge doesn't move.
        assert_eq!(top_y, -195);
    }

    use crate::host::WorkArea;

    fn monitor(x: i32, y: i32, w: u32, h: u32, scale: f64) -> Monitor {
        Monitor::new(
            PhysicalPosition::new(x, y),
            PhysicalSize::new(w, h),
            scale,
            WorkArea {
                position: PhysicalPosition::new(x, y),
                size: PhysicalSize::new(w, h - (50.0 * scale) as u32),
            },
        )
    }

    #[test]
    fn overlay_is_centered_and_offset_from_the_chosen_edge() {
        let m = monitor(0, 0, 2880, 1800, 2.0);
        let (x, y) =
            overlay_position_on_monitor(&m, OVERLAY_WIDTH, OVERLAY_HEIGHT, OverlayPosition::Top);
        assert_eq!(x, (1440.0 - OVERLAY_WIDTH) / 2.0);
        assert_eq!(y, OVERLAY_TOP_OFFSET);
        let (_, y) =
            overlay_position_on_monitor(&m, OVERLAY_WIDTH, OVERLAY_HEIGHT, OverlayPosition::Bottom);
        #[cfg(target_os = "macos")]
        assert_eq!(y, 850.0 - OVERLAY_HEIGHT - OVERLAY_BOTTOM_OFFSET);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(y, 900.0 - OVERLAY_HEIGHT - OVERLAY_BOTTOM_OFFSET);
    }

    #[test]
    fn streaming_state_uses_the_large_size() {
        assert_eq!(overlay_dimensions("streaming"), (400.0, 120.0));
        for s in ["recording", "transcribing", "processing"] {
            assert_eq!(overlay_dimensions(s), (256.0, 50.0));
        }
    }

    #[test]
    fn cursor_selects_the_secondary_monitor() {
        let a = monitor(0, 0, 1920, 1080, 1.0);
        let b = monitor(1920, 0, 1920, 1080, 1.0);
        let found = monitor_containing(&[a, b.clone()], (2000, 10)).unwrap();
        assert_eq!(found, b);
        assert!(monitor_containing(&[b], (10, 10)).is_none());
    }
}
