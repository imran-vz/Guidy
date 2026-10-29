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
#[cfg(all(test, target_os = "macos"))]
mod ui_snapshots;
#[cfg(test)]
mod ui_tests;
pub mod updater;
pub mod window_state;
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

/// The single-instance id; a debug build gets one per dev profile.
fn instance_id() -> String {
    #[cfg(debug_assertions)]
    if let Some(dir) = guidy_core::dev_profile_dir() {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut h);
        return format!("{APP_IDENTIFIER}.dev-{:08x}", h.finish() as u32);
    }
    APP_IDENTIFIER.to_string()
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
    let guard = match single_instance::acquire(&instance_id(), &args, move |forwarded| {
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
    // `guidy://` links (CFBundleURLTypes): queued until the UI runs, since a
    // cold launch from a link delivers it before `run`'s callback.
    let (url_tx, url_rx) = smol::channel::unbounded::<String>();
    application.on_open_urls(move |urls| {
        for url in urls {
            let _ = url_tx.try_send(url);
        }
    });
    application.run(move |cx| {
        cx.set_global(InstanceGuard(guard));
        start(app, cli_args, cx);
        spawn_url_loop(url_rx, cx);
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
    windows::watch_displays(cx);
    windows::save_state_on_quit(cx);

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

    // Window first: the core's managers (model registry scan, history
    // database) are built off the main thread meanwhile, and the window
    // draws a skeleton of the screen it will show (the app, or the setup
    // assistant for a new user) instead of a blank frame.
    let early = guidy_core::startup_decision(
        initial.start_hidden,
        initial.show_tray_icon,
        &cli_args,
        false,
    );
    let state = AppState::install_starting(
        Arc::new(CoreBackend::new(app.clone())),
        Arc::new(CoreMeetingsBackend::new(app.clone())),
        initial.onboarding_completed,
        cx,
    );
    let overlay = OverlayState::install(cx);
    crate::ui::recording_widget::RecordingWidgetState::install(cx);
    spawn_event_loop(event_rx, state.clone(), overlay, cx);
    if early.show_main_window {
        windows::show_main(cx);
    } else {
        #[cfg(target_os = "macos")]
        if early.start_as_accessory {
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
    }

    let managers_app = app.clone();
    let managers = cx
        .background_executor()
        .spawn(async move { guidy_core::create_managers(&managers_app) });
    cx.spawn(async move |cx| {
        let managers = managers.await;
        cx.update(|cx| finish_start(app, cli_args, managers, early, state, cx));
    })
    .detach();
}

/// The second half of startup, on the main thread once the managers exist:
/// register them (tray, shortcuts plumbing, meetings), then let the app
/// state load and run the startup check. A core that can't start shows an
/// error window (Show Logs, Quit) instead of crashing before any UI.
fn finish_start(
    app: AppHandle,
    cli_args: CliArgs,
    managers: Result<guidy_core::CoreManagers, String>,
    early: guidy_core::StartupDecision,
    state: Entity<AppState>,
    cx: &mut App,
) {
    let managers = match managers {
        Ok(m) => m,
        Err(e) => {
            log::error!("Guidy couldn't start: {e}");
            state.update(cx, |s, cx| s.core_failed(e, cx));
            windows::show_main(cx);
            return;
        }
    };
    let decision = guidy_core::initialize_app(&app, &cli_args, !cli_args.no_tray, managers);
    state.update(cx, |s, cx| s.core_ready(cx));

    #[cfg(debug_assertions)]
    let call_demo = std::env::var("GUIDY_DEV_RECORDING_WIDGET_DEMO").as_deref() == Ok("call");
    #[cfg(not(debug_assertions))]
    let call_demo = false;
    if !call_demo {
        crate::ui::recording_widget::watch_calls(windows::sync_recording_controls, cx);
    }
    // Session-wide update checker (automatic check, tray "check for updates").
    ui::status_bar::StatusBar::global(cx);

    #[cfg(debug_assertions)]
    if std::env::var_os("GUIDY_DEV_OVERLAY_DEMO").is_some() {
        overlay_demo(&app);
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("GUIDY_DEV_DEMO_MEETING").is_some() {
        demo_meeting(&app, cx);
    }
    #[cfg(debug_assertions)]
    if call_demo {
        // The stopped floater offering notes for a Zoom call (no detection).
        let call = guidy_core::meetings::calls::Call {
            app: guidy_core::meetings::calls::CallApp::Zoom,
            since: guidy_core::meetings::recorder::now_ms(),
        };
        if crate::ui::recording_widget::set_call(Some(call), cx) {
            windows::sync_recording_controls(cx);
        }
    } else if let Ok(mode) = std::env::var("GUIDY_DEV_RECORDING_WIDGET_DEMO") {
        recording_widget_demo(&app, mode == "expanded", cx);
    }

    // Windows: the core can insist on the window (microphone denied).
    if decision.show_main_window && !early.show_main_window {
        windows::show_main(cx);
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

/// Debug builds: add a processed meeting whose transcript a local model
/// garbled (repetition loops), to see the meeting page and the redo flow
/// without recording, and open it. Its audio files don't exist, so a redo
/// itself fails.
#[cfg(debug_assertions)]
fn demo_meeting(app: &AppHandle, cx: &mut App) {
    use guidy_core::meetings::manager::MeetingsManager;
    use guidy_core::meetings::types::*;
    let (tx, rx) = smol::channel::bounded::<MeetingId>(1);
    cx.spawn(async move |cx| {
        if let Ok(id) = rx.recv().await {
            cx.update(|cx| open_url_link(&format!("guidy://meetings/{id}"), cx));
        }
    })
    .detach();
    let app = app.clone();
    std::thread::spawn(move || {
        let manager = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            app.try_state::<Arc<MeetingsManager>>()
        });
        let Some(manager) = manager else {
            log::warn!("demo meeting: meeting notes never started");
            return;
        };
        let store = manager.store();
        if let Some(m) = store
            .list_meetings()
            .ok()
            .and_then(|m| m.into_iter().find(|m| m.title.starts_with("Roadmap")))
        {
            let _ = tx.try_send(m.id);
            return;
        }
        let now = guidy_core::meetings::recorder::now_ms();
        let result = (|| -> anyhow::Result<()> {
            let id = store.insert_meeting(&Meeting {
                id: 0,
                title: "Roadmap review".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: now - 40 * 60_000,
                ended_at: Some(now - 5 * 60_000),
                mic_audio_path: Some("demo-mic.opus".into()),
                system_audio_path: Some("demo-system.opus".into()),
                status: MeetingStatus::Processed,
                error: None,
                processing_stage: Some(ProcessingStage::Done.as_str().into()),
                description: None,
                title_source: Default::default(),
                has_summary: false,
            })?;
            let speaker = |id: SpeakerId, label: &str, is_user: bool| MeetingSpeaker {
                id,
                meeting_id: id,
                cluster_label: label.into(),
                person_id: None,
                match_confidence: None,
                confirmed: is_user,
                is_user,
            };
            let lines = [
                (
                    1,
                    "Okay, so the first thing on the roadmap is the onboarding rework.",
                ),
                (0, "Right. Did we settle on the the the the the timeline?"),
                (1, "Thank you. Thank you. Thank you. Thank you."),
                (0, "Sorry, you cut out there."),
                (
                    1,
                    "I said we ship it in two weeks, after the design review.",
                ),
            ];
            let segments: Vec<Segment> = lines
                .iter()
                .enumerate()
                .map(|(i, (speaker, text))| Segment {
                    id: 0,
                    meeting_id: id,
                    track: if *speaker == 0 {
                        Track::Mic
                    } else {
                        Track::System
                    },
                    t0_ms: i as i64 * 12_000,
                    t1_ms: i as i64 * 12_000 + 9_000,
                    text: (*text).into(),
                    words: Vec::new(),
                    speaker_id: Some(*speaker),
                })
                .collect();
            store.replace_transcript(
                id,
                &[speaker(0, "me", true), speaker(1, "Speaker 1", false)],
                &segments,
            )?;
            store.set_meeting_status(id, MeetingStatus::Processed, None)?;
            let _ = app.emit(events::MEETING_CHANGED, id);
            let _ = tx.try_send(id);
            Ok(())
        })();
        if let Err(e) = result {
            log::warn!("demo meeting: {e:#}");
        }
    });
}

/// Debug builds: replay a meeting recording and its live transcript through
/// the real event path, without capturing audio. `expanded` opens the
/// widget's panel.
#[cfg(debug_assertions)]
fn recording_widget_demo(app: &AppHandle, expanded: bool, cx: &mut App) {
    use guidy_core::meetings::types::{
        events, LiveLine, LiveStatus, LiveUpdate, RecorderState, Track,
    };
    if expanded {
        // After the recording starts (it starts collapsed), as a click would.
        cx.spawn(async |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(3_000))
                .await;
            cx.update(|cx| crate::ui::recording_widget::set_expanded(true, cx));
        })
        .detach();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        use std::time::Duration;
        const ID: i64 = 9_999;
        let now = guidy_core::meetings::recorder::now_ms();
        let live = |status: Option<LiveStatus>, line: Option<LiveLine>| {
            let _ = app.emit(
                events::LIVE_TRANSCRIPT,
                LiveUpdate {
                    meeting_id: ID,
                    status,
                    line,
                },
            );
        };
        std::thread::sleep(Duration::from_secs(2));
        live(Some(LiveStatus::Starting), None);
        let _ = app.emit(
            events::RECORDER_STATE,
            RecorderState::Recording {
                meeting_id: ID,
                title: "Design review: recording widget".into(),
                started_at: now - 754_000,
                scheduled_end_at: None,
                paused_at: None,
                paused_ms: 0,
            },
        );
        std::thread::sleep(Duration::from_millis(1500));
        live(Some(LiveStatus::Live), None);
        let script = [
            (
                Track::System,
                "Okay, can everyone see my screen? This is the new floating recorder.",
            ),
            (
                Track::Mic,
                "Yes, looks good. Is the transcript live or does it wait for the end?",
            ),
            (
                Track::System,
                "It's live. Lines show up about a second after someone speaks,",
            ),
            (
                Track::System,
                "and the proper transcript with names is still made after the meeting.",
            ),
            (
                Track::Mic,
                "Nice. And if the local model gets something wrong, I can redo it in the cloud",
            ),
        ];
        let mut t0 = 0;
        for (i, (track, text)) in script.iter().enumerate() {
            let words: Vec<&str> = text.split(' ').collect();
            let line = |n: usize, tentative: bool| LiveLine {
                id: i as u64,
                track: *track,
                t0_ms: t0,
                t1_ms: t0 + 3_000,
                text: words[..n].join(" "),
                tentative,
            };
            for n in (3..words.len()).step_by(4) {
                live(None, Some(line(n, true)));
                std::thread::sleep(Duration::from_millis(400));
            }
            let last = i + 1 == script.len();
            live(None, Some(line(words.len(), last)));
            t0 += 3_500;
        }
    });
}

/// Handle incoming `guidy://` URLs. Only well-formed meeting links are
/// acted on, and only by navigating (never recording, deleting, ...).
fn spawn_url_loop(rx: smol::channel::Receiver<String>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok(url) = rx.recv().await {
            cx.update(|cx| open_url_link(&url, cx));
        }
    })
    .detach();
}

/// Navigate for one `guidy://` URL; anything not a meeting link is ignored.
pub fn open_url_link(url: &str, cx: &mut App) {
    let Some(link) = crate::logic::meetings::parse_deep_link(url) else {
        log::info!("Ignoring an unsupported guidy:// link");
        return;
    };
    windows::show_main(cx);
    if let Some(main) = windows::main_window_entity(cx) {
        main.update(cx, |m, cx| m.open_meeting_link(link, cx));
    }
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
    if event.name == guidy_core::meetings::types::events::RECORDER_STATE {
        if let Ok(recorder) = serde_json::from_value::<guidy_core::meetings::types::RecorderState>(
            event.payload.clone(),
        ) {
            if let Some(widget) = crate::ui::recording_widget::RecordingWidgetState::global(cx) {
                widget.update(cx, |w, cx| w.set_recorder(recorder, cx));
            }
            windows::sync_recording_controls(cx);
        }
    }
    if event.name == guidy_core::meetings::types::events::LIVE_TRANSCRIPT {
        if let (Some(widget), Ok(update)) = (
            crate::ui::recording_widget::RecordingWidgetState::global(cx),
            serde_json::from_value::<guidy_core::meetings::types::LiveUpdate>(
                event.payload.clone(),
            ),
        ) {
            widget.update(cx, |w, cx| w.apply_live(&update, cx));
        }
    }
    if event.name != "mic-level" {
        state.update(cx, |s, cx| s.handle_core_event(event, cx));
    }
}
