//! Window lifecycle: the settings window, the recording overlay, and the
//! main-thread loop that applies [`ShellMsg`]s from the backend.
//!
//! GPUI has no per-window hide/show, so "hiding" closes the window and
//! "showing" reopens it; all state lives in models outside the views.

use crate::shell::{GpuiShell, ShellMsg};
use crate::ui::main_window::MainWindow;
use crate::ui::overlay::OverlayView;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use guidy_core::host::{Monitor, OverlayPlacement, PhysicalPosition, PhysicalSize, WorkArea};
use guidy_core::AppHandle;
use std::sync::Arc;

pub const MAIN_WIDTH: f32 = 680.;
pub const MAIN_HEIGHT: f32 = 570.;

#[derive(Default)]
pub struct Windows {
    pub main: Option<AnyWindowHandle>,
    pub main_view: Option<WeakEntity<MainWindow>>,
    pub overlay: Option<(AnyWindowHandle, OverlayPlacement)>,
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
            size(px(MAIN_WIDTH), px(MAIN_HEIGHT)),
            cx,
        ))),
        window_min_size: Some(size(px(MAIN_WIDTH), px(MAIN_HEIGHT))),
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
