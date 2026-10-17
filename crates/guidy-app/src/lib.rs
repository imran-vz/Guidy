//! Guidy desktop app: GPUI views on top of `guidy-core`.

pub mod assets;
pub mod backend;
pub mod logic;
pub mod meetings_backend;
pub mod playback;
pub mod shell;
pub mod state;
#[cfg(test)]
pub mod testing;
pub mod theme;
pub mod ui;
#[cfg(test)]
mod ui_tests;
pub mod updater;
pub mod windows;

use crate::backend::CoreBackend;
use crate::meetings_backend::CoreMeetingsBackend;
use crate::shell::GpuiShell;
use crate::state::AppState;
use crate::ui::overlay::OverlayState;
use crate::windows::ShellGlobal;
use gpui_kit::*;
use guidy_core::host::{Event as CoreEvent, APP_IDENTIFIER};
use guidy_core::single_instance::{self, Instance};
use guidy_core::{AppHandle, CliArgs};
use std::sync::Arc;

/// Events the recording overlay consumes.
const OVERLAY_EVENTS: &[&str] = &[
    "show-overlay",
    "hide-overlay",
    "recording-ready",
    "mic-level",
    "stream-text-event",
    "stream-phase-event",
    "model-state-changed",
];

/// Keeps the single-instance listener alive for the process lifetime.
struct InstanceGuard(Option<single_instance::PrimaryGuard>);
impl Global for InstanceGuard {}

/// Stop accepting second launches (before relaunching after an update, so
/// the new process becomes primary instead of forwarding to this one).
pub fn release_single_instance(cx: &mut App) {
    if let Some(guard) = cx
        .try_global::<InstanceGuard>()
        .is_some()
        .then(|| cx.global_mut::<InstanceGuard>().0.take())
        .flatten()
    {
        guard.shutdown();
    }
}

pub fn run(cli_args: CliArgs) {
    let app = guidy_core::create_app_handle();
    guidy_core::logging::init(&app.paths().app_log_dir.clone(), false);
    log::info!(
        "Starting {} v{}",
        guidy_core::host::PRODUCT_NAME,
        env!("CARGO_PKG_VERSION")
    );

    // Single instance: a second launch forwards its flags and exits.
    let args: Vec<String> = std::env::args().collect();
    let forward_app = app.clone();
    let guard = match single_instance::acquire(APP_IDENTIFIER, &args, move |forwarded| {
        guidy_core::handle_second_instance(&forward_app, &forwarded)
    }) {
        Ok(Instance::Forwarded) => {
            log::info!("Forwarded arguments to the running instance");
            return;
        }
        Ok(Instance::Primary(g)) => Some(g),
        Err(e) => {
            log::warn!("Single-instance check failed ({e}); continuing");
            None
        }
    };

    let application = gpui_kit::application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::Explicit);
    // macOS: a start-hidden launch with a tray must *launch* as Accessory.
    // Demoting a running Regular app later is unreliable and can leave a
    // Dock icon behind. Showing the window promotes it.
    #[cfg(target_os = "macos")]
    let application = {
        let settings = guidy_core::settings::get_settings(&app);
        let d = guidy_core::startup_decision(
            settings.start_hidden,
            settings.show_tray_icon,
            &cli_args,
            false,
        );
        if d.start_as_accessory {
            log::info!("Starting hidden with tray available: launching as Accessory");
            application.with_windowing(WindowingRequest::Headless)
        } else {
            application
        }
    };
    let reopen_app = app.clone();
    application.on_reopen(move |_cx| guidy_core::handle_reopen(&reopen_app));
    application.run(move |cx| {
        cx.set_global(InstanceGuard(guard));
        start(app, cli_args, cx);
    });
}

fn start(app: AppHandle, cli_args: CliArgs, cx: &mut App) {
    gpui_kit::init(cx);
    theme::init(cx);
    ui::init(cx);
    ui::set_app_menus(cx);

    // Backend → UI plumbing.
    let (shell_tx, shell_rx) = smol::channel::unbounded();
    let shell = Arc::new(GpuiShell::new(shell_tx));
    app.set_shell(shell.clone());
    cx.set_global(ShellGlobal {
        shell: shell.clone(),
        app: app.clone(),
    });
    windows::spawn_shell_loop(shell, shell_rx, cx);
    windows::spawn_appearance_watch(cx);

    // Queue backend events from now on; consumed once state exists.
    let (event_tx, event_rx) = smol::channel::unbounded::<CoreEvent>();
    app.listen_any(move |e| {
        let _ = event_tx.try_send(e.clone());
    });

    // Initial UI language and theme before any window renders (the theme
    // would otherwise flash the system palette until settings load).
    let initial = guidy_core::settings::get_settings(&app);
    state::apply_app_language(&initial.app_language);
    theme::apply_setting(initial.theme, cx);

    let decision = guidy_core::initialize_app(&app, &cli_args, !cli_args.no_tray);

    // AppState runs the startup step machine (onboarding check, then input +
    // shortcut init) whether or not the settings window is ever opened.
    let state = AppState::install_with_meetings(
        Arc::new(CoreBackend::new(app.clone())),
        Arc::new(CoreMeetingsBackend::new(app.clone())),
        cx,
    );
    let overlay = OverlayState::install(cx);
    // Session-wide update checker (automatic check, tray "check for updates").
    ui::status_bar::StatusBar::global(cx);
    spawn_event_loop(event_rx, state, overlay, cx);

    #[cfg(debug_assertions)]
    if std::env::var_os("GUIDY_DEV_OVERLAY_DEMO").is_some() {
        overlay_demo(&app);
    }

    if decision.show_main_window {
        windows::show_main(cx);
    } else {
        #[cfg(target_os = "macos")]
        if decision.start_as_accessory {
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
    }
}

/// Debug builds: replay a streaming session through the real overlay path
/// (window placement + events) without a microphone.
#[cfg(debug_assertions)]
fn overlay_demo(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        use std::time::Duration;
        guidy_core::overlay::update_overlay_enabled_cache(true);
        std::thread::sleep(Duration::from_secs(2));
        guidy_core::overlay::show_streaming_overlay(&app);
        guidy_core::overlay::emit_recording_ready(&app);
        let words = [
            "Guidy", "turns", "your", "voice", "into", "text,", "right", "where", "your", "cursor",
            "is.",
        ];
        for i in 0..words.len() {
            for _ in 0..4 {
                let levels: Vec<f32> = (0..16)
                    .map(|b| (((i * 7 + b * 3) % 11) as f32) / 11.0)
                    .collect();
                guidy_core::overlay::emit_levels(&app, &levels);
                std::thread::sleep(Duration::from_millis(60));
            }
            let committed = words[..i].join(" ");
            let _ = app.emit(
                "stream-text-event",
                serde_json::json!({"committed": committed, "tentative": words[i]}),
            );
        }
    });
}

fn spawn_event_loop(
    rx: smol::channel::Receiver<CoreEvent>,
    state: Entity<AppState>,
    overlay: Entity<OverlayState>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        while let Ok(event) = rx.recv().await {
            cx.update(|cx| dispatch_event(&event, &state, &overlay, cx));
        }
    })
    .detach();
}

/// Route one backend event to the app state and (if relevant) the overlay.
pub fn dispatch_event(
    event: &CoreEvent,
    state: &Entity<AppState>,
    overlay: &Entity<OverlayState>,
    cx: &mut App,
) {
    if OVERLAY_EVENTS.contains(&event.name.as_str()) {
        let (top, style) = state
            .read(cx)
            .app_settings()
            .map(|s| {
                (
                    s.overlay_position == guidy_core::settings::OverlayPosition::Top,
                    s.overlay_style,
                )
            })
            .unwrap_or((false, guidy_core::settings::OverlayStyle::Live));
        overlay.update(cx, |o, cx| {
            o.handle_core_event(&event.name, &event.payload, top, style, cx)
        });
    }
    if event.name != "mic-level" {
        state.update(cx, |s, cx| s.handle_core_event(event, cx));
    }
}
