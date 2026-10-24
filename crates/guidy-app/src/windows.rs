//! Window lifecycle: the settings window, the recording overlay, and the
//! main-thread loop that applies [`ShellMsg`]s from the backend.
//!
//! GPUI has no per-window hide/show, so "hiding" closes the window and
//! "showing" reopens it; all state lives in models outside the views.

use crate::logic::notch::ScreenInfo;
use crate::shell::{GpuiShell, ShellMsg};
use crate::state::AppState;
use crate::ui::design::dp;
use crate::ui::main_window::MainWindow;
use crate::ui::overlay::OverlayView;
use crate::ui::recording_widget::{RecordingWidgetState, RecordingWidgetView};
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use guidy_core::host::{Monitor, OverlayPlacement, PhysicalPosition, PhysicalSize, WorkArea};
use guidy_core::meetings::settings::RecordingControls;
use guidy_core::AppHandle;
use std::sync::Arc;

/// The main window's smallest design size (scaled by [`dp`]).
pub const MAIN_WIDTH: f32 = 680.;
pub const MAIN_HEIGHT: f32 = 570.;
/// Its design size when it first opens: roomier than the minimum, still
/// inside a 13" display at the default resolution.
const MAIN_DEFAULT_WIDTH: f32 = 800.;
const MAIN_DEFAULT_HEIGHT: f32 = 620.;

#[derive(Default)]
pub struct Windows {
    pub main: Option<AnyWindowHandle>,
    pub main_view: Option<WeakEntity<MainWindow>>,
    pub overlay: Option<(AnyWindowHandle, OverlayPlacement)>,
    pub notch: Option<AnyWindowHandle>,
    pub recording_widget: Option<AnyWindowHandle>,
}

impl Global for Windows {}

pub struct ShellGlobal {
    pub shell: Arc<GpuiShell>,
    pub app: AppHandle,
}

impl Global for ShellGlobal {}

fn windows(cx: &mut App) -> &mut Windows {
    if !cx.has_global::<Windows>() {
        cx.set_global(Windows::default());
    }
    cx.global_mut::<Windows>()
}

fn set_main_visible(visible: bool, cx: &mut App) {
    if let Some(g) = cx.try_global::<ShellGlobal>() {
        g.shell.cache.lock().main_visible = visible;
    }
}

pub fn main_window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(dp(MAIN_DEFAULT_WIDTH), dp(MAIN_DEFAULT_HEIGHT)),
            cx,
        ))),
        window_min_size: Some(size(dp(MAIN_WIDTH), dp(MAIN_HEIGHT))),
        app_id: Some(guidy_core::host::APP_IDENTIFIER.into()),
        ..TitleBar::window_options()
    }
}

/// Open the settings window, or focus it if it's already open.
pub fn show_main(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.set_activation_policy(ActivationPolicy::Regular);

    if let Some(handle) = windows(cx).main {
        if cx
            .update_window(handle, |_, window, _| window.activate_window())
            .is_ok()
        {
            cx.activate(true);
            set_main_visible(true, cx);
            return;
        }
        windows(cx).main = None;
    }

    let options = main_window_options(cx);
    match gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title(guidy_core::host::PRODUCT_NAME);
        window.on_window_should_close(cx, |_, cx| {
            on_main_closed(cx);
            true
        });
        cx.new(|cx| MainWindow::new(window, cx))
    }) {
        Ok((handle, view)) => {
            windows(cx).main = Some(handle);
            windows(cx).main_view = Some(view.downgrade());
            set_main_visible(true, cx);
            cx.activate(true);
        }
        Err(e) => log::error!("Failed to open main window: {e}"),
    }
}

/// The close button hides the app into the tray (macOS: drops the Dock
/// icon when a tray is available).
pub fn on_main_closed(cx: &mut App) {
    windows(cx).main = None;
    windows(cx).main_view = None;
    set_main_visible(false, cx);
    #[cfg(target_os = "macos")]
    if let Some(g) = cx.try_global::<ShellGlobal>() {
        let settings = guidy_core::settings::get_settings(&g.app);
        let no_tray = g
            .app
            .try_state::<guidy_core::CliArgs>()
            .map(|a| a.no_tray)
            .unwrap_or(false);
        if settings.show_tray_icon && !no_tray {
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
    }
}

/// The live settings-window view, if open.
pub fn main_window_entity(cx: &App) -> Option<Entity<MainWindow>> {
    cx.try_global::<Windows>()?.main_view.as_ref()?.upgrade()
}

pub fn hide_main(cx: &mut App) {
    if let Some(handle) = windows(cx).main.take() {
        let _ = cx.update_window(handle, |_, window, _| window.remove_window());
    }
    set_main_visible(false, cx);
}

fn overlay_options(p: &OverlayPlacement, cx: &App) -> WindowOptions {
    let (x, y, display_id) = overlay_origin(p);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(x as f32), px(y as f32)),
            size: size(px(p.width as f32), px(p.height as f32)),
        })),
        display_id,
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(format!("{}.overlay", guidy_core::host::APP_IDENTIFIER)),
        ..Default::default()
    };
    layer_shell_options(options, p, cx)
}

/// Where to open the overlay window. macOS places a window relative to its
/// display (the primary one when none is given), so the overlay goes on the
/// display it was placed for, at coordinates relative to that display.
/// Elsewhere the placement's global coordinates are used as they are.
fn overlay_origin(p: &OverlayPlacement) -> (f64, f64, Option<DisplayId>) {
    #[cfg(target_os = "macos")]
    if let Some(id) = p.display_id {
        let (ox, oy) = p.display_origin;
        return (p.x - ox, p.y - oy, Some(DisplayId::new(id)));
    }
    (p.x, p.y, None)
}

/// Wayland: clients can't place their own windows, so the overlay is a
/// layer-shell surface anchored to the chosen edge, with the same offsets
/// as the other platforms. `GUIDY_NO_LAYER_SHELL=1` opts out.
#[cfg(target_os = "linux")]
fn layer_shell_options(
    mut options: WindowOptions,
    p: &OverlayPlacement,
    cx: &App,
) -> WindowOptions {
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
    use guidy_core::overlay::{OVERLAY_BOTTOM_OFFSET, OVERLAY_TOP_OFFSET};
    if !guidy_core::utils::is_wayland() || guidy_core::env_flag_enabled("GUIDY_NO_LAYER_SHELL") {
        return options;
    }
    let top = cx
        .try_global::<ShellGlobal>()
        .map(|g| guidy_core::settings::get_settings(&g.app).overlay_position)
        .is_some_and(|pos| pos == guidy_core::settings::OverlayPosition::Top);
    let zero = px(0.);
    let (anchor, margin) = if top {
        (
            Anchor::TOP,
            (px(OVERLAY_TOP_OFFSET as f32), zero, zero, zero),
        )
    } else {
        (
            Anchor::BOTTOM,
            (zero, zero, px(OVERLAY_BOTTOM_OFFSET as f32), zero),
        )
    };
    options.kind = WindowKind::LayerShell(LayerShellOptions {
        namespace: format!("{}.overlay", guidy_core::host::APP_IDENTIFIER),
        layer: Layer::Overlay,
        anchor,
        exclusive_zone: None,
        exclusive_edge: None,
        margin: Some(margin),
        keyboard_interactivity: KeyboardInteractivity::None,
    });
    // Size only: the compositor positions the surface from the anchor.
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin: Point::default(),
        size: size(px(p.width as f32), px(p.height as f32)),
    }));
    options
}

#[cfg(not(target_os = "linux"))]
fn layer_shell_options(options: WindowOptions, _: &OverlayPlacement, _: &App) -> WindowOptions {
    options
}

pub fn show_overlay(p: OverlayPlacement, cx: &mut App) {
    if let Some((handle, current)) = windows(cx).overlay.clone() {
        let same_geometry = current.display_id == p.display_id
            && current.x == p.x
            && current.y == p.y
            && current.width == p.width
            && current.height == p.height;
        if same_geometry
            && cx
                .update_window(handle, |_, window, _| window.refresh())
                .is_ok()
        {
            windows(cx).overlay = Some((handle, p));
            return;
        }
        let _ = cx.update_window(handle, |_, window, _| window.remove_window());
        windows(cx).overlay = None;
    }
    match cx.open_window(overlay_options(&p, cx), |window, cx| {
        cx.new(|cx| OverlayView::new(window, cx))
    }) {
        Ok(handle) => {
            let any: AnyWindowHandle = handle.into();
            let _ = cx.update_window(any, |_, window, _| configure_overlay_panel(window));
            windows(cx).overlay = Some((any, p))
        }
        Err(e) => log::error!("Failed to open overlay window: {e}"),
    }
}

/// macOS tweaks for the overlay panel.
///
/// The overlay must stay visible while Guidy is in the background (the user
/// is dictating into another app). GPUI's PopUp windows are NSPanels, which
/// hide on app deactivation by default — turn that off.
///
/// GPUI also creates title-less windows with a titled style mask and keeps
/// the system shadow on transparent windows, so macOS outlines the whole
/// (mostly empty) overlay window around the pill. The pill draws its own
/// border and shadow, so make the panel borderless and shadowless.
fn configure_overlay_panel(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            let view = h.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: a live NSView owned by this window, on the main thread.
            unsafe {
                let ns_window: *mut AnyObject = msg_send![view, window];
                if !ns_window.is_null() {
                    // NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                    let _: () = msg_send![ns_window, setStyleMask: 1usize << 7];
                    let _: () = msg_send![ns_window, setHasShadow: false];
                    let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
                    let _: () = msg_send![ns_window, orderFrontRegardless];
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

pub fn hide_overlay(cx: &mut App) {
    if let Some((handle, _)) = windows(cx).overlay.take() {
        let _ = cx.update_window(handle, |_, window, _| window.remove_window());
    }
}

/// Where the controls of a recording in progress go (a setting).
fn recording_controls(cx: &App) -> RecordingControls {
    AppState::global(cx)
        .read(cx)
        .app_settings()
        .map(|s| s.meetings.recording_controls)
        .unwrap_or_default()
}

/// Show the recording floater while a meeting records, docked at the notch
/// or free as the settings say, and close it when it stops.
pub fn sync_recording_controls(cx: &mut App) {
    let docked = recording_controls(cx) == RecordingControls::Notch;
    if let Some(state) = RecordingWidgetState::global(cx) {
        state.update(cx, |s, cx| {
            if s.docked != docked {
                s.docked = docked;
                cx.notify();
            }
        });
    }
    sync_recording_widget(cx);
}

/// The floater's window: transparent, over the whole display with the
/// notch (else the primary one), above the menu bar so it can dock into
/// the notch. It lets the pointer through except over the floater
/// ([`set_ignores_mouse`]). macOS only.
fn sync_recording_widget(cx: &mut App) {
    let visible = RecordingWidgetState::global(cx).is_some_and(|s| s.read(cx).model.visible());
    if !visible {
        if let Some(handle) = windows(cx).recording_widget.take() {
            let _ = cx.update_window(handle, |_, window, _| window.remove_window());
        }
        return;
    }
    if let Some(handle) = windows(cx).recording_widget {
        if cx
            .update_window(handle, |_, window, _| window.refresh())
            .is_ok()
        {
            return;
        }
        windows(cx).recording_widget = None;
    }
    let Some(screen) = stage_screen() else {
        return;
    };
    if let Some(state) = RecordingWidgetState::global(cx) {
        state.update(cx, |s, cx| {
            s.screen = Some(screen);
            cx.notify();
        });
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(screen.width as f32), px(screen.height as f32)),
        })),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(format!(
            "{}.recording-widget",
            guidy_core::host::APP_IDENTIFIER
        )),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| RecordingWidgetView::new(window, cx))
    }) {
        Ok(handle) => {
            let any: AnyWindowHandle = handle.into();
            let _ = cx.update_window(any, |_, window, _| {
                configure_overlay_panel(window);
                place_panel(
                    window,
                    (screen.x, screen.y, screen.width, screen.height),
                    MENU_BAR_LEVEL,
                );
            });
            windows(cx).recording_widget = Some(any);
        }
        Err(e) => log::error!("Failed to open the recording widget: {e}"),
    }
}

/// A panel frame in Cocoa coordinates: x, y, width, height.
type Rect = (f64, f64, f64, f64);

/// NSMainMenuWindowLevel (24) + 2: above the menu bar.
const MENU_BAR_LEVEL: isize = 26;

/// The display the floater lives on: the one with a notch, else the
/// primary one.
#[cfg(target_os = "macos")]
fn stage_screen() -> Option<ScreenInfo> {
    let screens = macos_screens();
    screens
        .iter()
        .find(|s| s.notch.is_some())
        .or_else(|| screens.first())
        .copied()
}

#[cfg(not(target_os = "macos"))]
fn stage_screen() -> Option<ScreenInfo> {
    None
}

/// The pointer, in Cocoa coordinates (points, origin bottom-left of the
/// primary display).
#[cfg(target_os = "macos")]
pub fn pointer_location() -> Option<(f64, f64)> {
    use cocoa::CGPoint;
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    let class = AnyClass::get(c"NSEvent")?;
    // SAFETY: a class method returning a plain struct, on the main thread.
    let p: CGPoint = unsafe { msg_send![class, mouseLocation] };
    Some((p.x, p.y))
}

#[cfg(not(target_os = "macos"))]
pub fn pointer_location() -> Option<(f64, f64)> {
    None
}

/// The native window behind `window`, for [`set_ignores_mouse`]. Only
/// valid while the window is open.
pub fn native_window(window: &Window) -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = HasWindowHandle::window_handle(window).ok()?;
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            let view = h.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: a live NSView owned by this window, on the main thread.
            let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
            return (!ns_window.is_null()).then_some(ns_window as usize);
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// Let the pointer through the native window (from [`native_window`]) to
/// what is beneath (`ignore`), or take it again. Call it outside GPUI's
/// window updates: AppKit answers with pointer events at once, which GPUI
/// can't take while it is updating that window.
pub fn set_ignores_mouse(ns_window: usize, ignore: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};
        let ns_window = ns_window as *mut AnyObject;
        // SAFETY: an NSWindow the caller holds open, on the main thread.
        unsafe {
            let _: () = msg_send![ns_window, setIgnoresMouseEvents: Bool::new(ignore)];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (ns_window, ignore);
}

#[cfg(target_os = "macos")]
mod cocoa {
    use objc2::encode::{Encode, Encoding};

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NSEdgeInsets {
        pub top: f64,
        pub left: f64,
        pub bottom: f64,
        pub right: f64,
    }

    const POINT: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
    const SIZE: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);

    // SAFETY: `#[repr(C)]` with the fields AppKit declares, in order.
    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding = POINT;
    }
    unsafe impl Encode for CGSize {
        const ENCODING: Encoding = SIZE;
    }
    unsafe impl Encode for CGRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[POINT, SIZE]);
    }
    unsafe impl Encode for NSEdgeInsets {
        const ENCODING: Encoding = Encoding::Struct(
            "NSEdgeInsets",
            &[
                Encoding::Double,
                Encoding::Double,
                Encoding::Double,
                Encoding::Double,
            ],
        );
    }
}

/// Every display (primary first) with its notch, from `NSScreen`.
#[cfg(target_os = "macos")]
fn macos_screens() -> Vec<ScreenInfo> {
    use cocoa::{CGRect, NSEdgeInsets};
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};
    let Some(class) = AnyClass::get(c"NSScreen") else {
        return Vec::new();
    };
    let responds = |obj: *mut AnyObject, sel: Sel| -> bool {
        // SAFETY: `obj` is a live NSScreen.
        let r: Bool = unsafe { msg_send![obj, respondsToSelector: sel] };
        r.as_bool()
    };
    let mut out = Vec::new();
    // SAFETY: AppKit calls on the main thread (GPUI's app thread); the
    // array and screens are owned by AppKit and outlive this call.
    unsafe {
        let screens: *mut AnyObject = msg_send![class, screens];
        if screens.is_null() {
            return out;
        }
        let count: usize = msg_send![screens, count];
        for i in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if screen.is_null() {
                continue;
            }
            let frame: CGRect = msg_send![screen, frame];
            // macOS 12+: the notch areas.
            let (top_inset, notch) = if responds(screen, sel!(safeAreaInsets)) {
                let insets: NSEdgeInsets = msg_send![screen, safeAreaInsets];
                let notch = if insets.top > 0.0 && responds(screen, sel!(auxiliaryTopLeftArea)) {
                    let left: CGRect = msg_send![screen, auxiliaryTopLeftArea];
                    let right: CGRect = msg_send![screen, auxiliaryTopRightArea];
                    let x0 = left.origin.x + left.size.width - frame.origin.x;
                    let x1 = right.origin.x - frame.origin.x;
                    (x1 > x0 && left.size.width > 0.0).then_some((x0, x1))
                } else {
                    None
                };
                (insets.top, notch)
            } else {
                (0.0, None)
            };
            // Without a notch, match the menu bar's height.
            let top_inset = if notch.is_none() {
                let visible: CGRect = msg_send![screen, visibleFrame];
                let bar =
                    (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
                if bar > 0.0 {
                    bar
                } else {
                    top_inset
                }
            } else {
                top_inset
            };
            out.push(ScreenInfo {
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
                notch,
                top_inset,
            });
        }
    }
    out
}

/// Move the panel to `frame` (Cocoa coordinates) at window `level`, on
/// every Space and over full-screen apps.
fn place_panel(window: &Window, frame: Rect, level: isize) {
    #[cfg(target_os = "macos")]
    {
        use cocoa::{CGPoint, CGRect, CGSize};
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            let view = h.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: a live NSView owned by this window, on the main thread.
            unsafe {
                let ns_window: *mut AnyObject = msg_send![view, window];
                if ns_window.is_null() {
                    return;
                }
                let _: () = msg_send![ns_window, setLevel: level];
                // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
                let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
                let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
                let (x, y, width, height) = frame;
                let rect = CGRect {
                    origin: CGPoint { x, y },
                    size: CGSize { width, height },
                };
                let _: () = msg_send![ns_window, setFrame: rect, display: Bool::YES];
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, frame, level);
}

/// Displays in logical points (GPUI exposes no scale factor; placement math
/// works in logical units with scale 1), in global coordinates.
pub fn snapshot_monitors(cx: &App) -> Vec<Monitor> {
    let mut displays = cx.displays();
    if let Some(primary) = cx.primary_display() {
        // Primary first: `primary_monitor()` falls back to the first entry.
        displays.sort_by_key(|d| d.id() != primary.id());
    }
    displays
        .into_iter()
        .map(|d| {
            let id = u64::from(d.id());
            let b = d.bounds();
            let v = d.visible_bounds();
            // GPUI's macOS display bounds start at (0, 0) and its visible
            // bounds are relative to the display; the cursor is in global
            // coordinates, so both are moved to the display's global origin.
            let (ox, oy) = display_origin(id)
                .unwrap_or((f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64));
            let (vx, vy) = if cfg!(target_os = "macos") {
                (
                    ox + f32::from(v.origin.x) as f64,
                    oy + f32::from(v.origin.y) as f64,
                )
            } else {
                (f32::from(v.origin.x) as f64, f32::from(v.origin.y) as f64)
            };
            Monitor::new(
                PhysicalPosition::new(ox as i32, oy as i32),
                PhysicalSize::new(
                    f32::from(b.size.width) as u32,
                    f32::from(b.size.height) as u32,
                ),
                1.0,
                WorkArea {
                    position: PhysicalPosition::new(vx as i32, vy as i32),
                    size: PhysicalSize::new(
                        f32::from(v.size.width) as u32,
                        f32::from(v.size.height) as u32,
                    ),
                },
            )
            .with_id(id)
        })
        .collect()
}

/// A display's top-left corner in global coordinates (points, origin at the
/// primary display's top-left), the space the cursor position is in.
#[cfg(target_os = "macos")]
fn display_origin(id: u64) -> Option<(f64, f64)> {
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[allow(dead_code)] // only the layout matters
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayBounds(display: u32) -> CGRect;
    }
    let id = u32::try_from(id).ok()?;
    // SAFETY: a plain C call; an unknown id yields an empty rect.
    let r = unsafe { CGDisplayBounds(id) };
    Some((r.origin.x, r.origin.y))
}

#[cfg(not(target_os = "macos"))]
fn display_origin(_id: u64) -> Option<(f64, f64)> {
    None
}

fn refresh_cache(shell: &GpuiShell, cx: &App) {
    let mut cache = shell.cache.lock();
    cache.monitors = snapshot_monitors(cx);
    cache.system_dark = Some(matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    ));
}

/// Apply one shell message on the main thread.
pub fn handle_shell_msg(msg: ShellMsg, cx: &mut App) {
    match msg {
        ShellMsg::Run(f) => f(),
        ShellMsg::ShowMain => show_main(cx),
        ShellMsg::HideMain => hide_main(cx),
        ShellMsg::SetTheme(theme) => {
            let setting = match theme {
                Some(guidy_core::host::Theme::Light) => guidy_core::settings::Theme::Light,
                Some(guidy_core::host::Theme::Dark) => guidy_core::settings::Theme::Dark,
                None => guidy_core::settings::Theme::System,
            };
            crate::theme::apply_setting(setting, cx);
        }
        ShellMsg::ShowOverlay(p) | ShellMsg::MoveOverlay(p) => show_overlay(p, cx),
        ShellMsg::HideOverlay => hide_overlay(cx),
        ShellMsg::Accessory(accessory) => {
            #[cfg(target_os = "macos")]
            cx.set_activation_policy(if accessory {
                ActivationPolicy::Accessory
            } else {
                ActivationPolicy::Regular
            });
            #[cfg(not(target_os = "macos"))]
            let _ = accessory;
        }
        ShellMsg::Exit(code) => {
            if code == 0 {
                cx.quit();
            } else {
                std::process::exit(code);
            }
        }
    }
}

/// The OS switched light/dark: refresh the shell's cached appearance first
/// (the tray reads it), then re-theme the UI when following the system and
/// swap the tray icon variant.
pub fn sync_system_appearance(cx: &mut App) {
    let Some((shell, app)) = cx
        .try_global::<ShellGlobal>()
        .map(|g| (g.shell.clone(), g.app.clone()))
    else {
        return;
    };
    refresh_cache(&shell, cx);
    let setting = crate::state::AppState::global(cx)
        .read(cx)
        .app_settings()
        .map(|s| s.theme)
        .unwrap_or(guidy_core::settings::Theme::System);
    crate::theme::apply_setting(setting, cx);
    guidy_core::tray::refresh_tray_icon(&app);
}

/// Follow OS theme changes even with every window hidden; GPUI only reports
/// appearance per open window, so watch it app-wide.
pub fn spawn_appearance_watch(cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last = cx.update(|cx| cx.window_appearance());
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let now = cx.update(|cx| cx.window_appearance());
            if now != last {
                last = now;
                cx.update(sync_system_appearance);
            }
        }
    })
    .detach();
}

/// Drain shell messages on the main thread for the app's lifetime.
pub fn spawn_shell_loop(
    shell: Arc<GpuiShell>,
    rx: smol::channel::Receiver<ShellMsg>,
    cx: &mut App,
) {
    refresh_cache(&shell, cx);
    cx.spawn(async move |cx| {
        while let Ok(msg) = rx.recv().await {
            let shell = shell.clone();
            cx.update(|cx| {
                refresh_cache(&shell, cx);
                handle_shell_msg(msg, cx);
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui_kit's `test` macro would shadow `#[test]`.
    use super::overlay_origin;
    #[cfg(target_os = "macos")]
    use gpui_kit::DisplayId;
    use guidy_core::host::OverlayPlacement;

    #[test]
    fn overlay_opens_on_its_display_relative_to_it() {
        let p = OverlayPlacement {
            state: "streaming".into(),
            x: 1728.0 + 1080.0,
            y: -200.0 + 1305.0,
            width: 400.0,
            height: 120.0,
            top: false,
            display_id: Some(7),
            display_origin: (1728.0, -200.0),
        };
        let (x, y, display) = overlay_origin(&p);
        #[cfg(target_os = "macos")]
        {
            assert_eq!((x, y), (1080.0, 1305.0));
            assert_eq!(display, Some(DisplayId::new(7)));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!((x, y), (p.x, p.y));
            assert_eq!(display, None);
        }

        // Without a known display the global position is used as is.
        let p = OverlayPlacement {
            display_id: None,
            ..p
        };
        assert_eq!(overlay_origin(&p), (p.x, p.y, None));
    }
}
