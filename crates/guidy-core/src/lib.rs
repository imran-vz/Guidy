//! Guidy backend.
//!
//! The UI layer (the `guidy` app crate)
//! creates an [`host::AppHandle`], installs its [`host::Shell`], and drives
//! startup through [`initialize_app`] / [`run_headless`].

pub mod actions;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod apple_intelligence;
pub mod audio_feedback;
pub mod audio_toolkit;
pub mod autostart;
pub mod catalog;
pub mod chinese_script;
pub mod cli;
pub mod clipboard;
pub mod commands;
pub mod engine_supervisor;
pub mod helpers;
pub mod host;
pub mod input;
pub mod linear;
pub mod llm;
pub mod llm_client;
pub mod logging;
pub mod managers;
pub mod meetings;
pub mod memory;
pub mod overlay;
pub mod paste_tx;
pub mod permissions;
pub mod portable;
pub mod secrets;
pub mod secure_input;
pub mod settings;
pub mod shortcut;
pub mod signal_handle;
pub mod single_instance;
pub mod transcription_coordinator;
pub mod tray;
pub mod tray_i18n;
pub mod utils;

pub use cli::CliArgs;
pub use host::AppHandle;
pub use transcription_coordinator::TranscriptionCoordinator;
pub use utils::env_flag_enabled;

use managers::audio::AudioRecordingManager;
use managers::history::HistoryManager;
use managers::model::ModelManager;
use managers::transcription::TranscriptionManager;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::settings::get_settings;

// Global atomic to store the file log level filter
// We use u8 to store the log::LevelFilter as a number
pub static FILE_LOG_LEVEL: AtomicU8 = AtomicU8::new(log::LevelFilter::Debug as u8);

/// When `true`, log records are also forwarded to the UI via the `log://log`
/// event for the debug page's live log viewer. Gated on debug mode — the live
/// log viewer is its only consumer — so normal runs never broadcast log
/// records (which can include file paths or transcribed text) onto the event
/// bus. Synced at startup and whenever debug mode is toggled (see
/// `shortcut::change_debug_mode_setting`).
pub static LOG_STREAMING: AtomicBool = AtomicBool::new(false);

pub fn level_filter_from_u8(value: u8) -> log::LevelFilter {
    match value {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Trace,
    }
}

/// Process-wide environment set before anything else runs: Metal residency,
/// allocator tuning, portable-mode detection.
pub fn prepare_process() {
    // ggml's Metal residency sets keep model weights wired in memory, which
    // shows up as large "memory used" for an idle app. Advanced users can
    // restore upstream residency behavior with GUIDY_METAL_RESIDENCY=1.
    #[cfg(target_os = "macos")]
    if std::env::var("GUIDY_METAL_RESIDENCY").as_deref() == Ok("1") {
        // ggml treats GGML_METAL_NO_RESIDENCY as presence-based, so remove an
        // inherited value as well when explicitly opting back in.
        std::env::remove_var("GGML_METAL_NO_RESIDENCY");
    } else {
        std::env::set_var("GGML_METAL_NO_RESIDENCY", "1");
    }

    // Pin glibc's dynamic mmap threshold before the first large allocation,
    // so per-dictation transient buffers are returned to the OS on free
    // instead of accumulating in malloc arenas. No-op off Linux/glibc.
    memory::init_allocator();

    // Detect portable mode before anything else
    portable::init();
}

/// `--transcribe-file` / `--list-devices` / `--list-models` run as a
/// standalone one-shot instance.
pub fn is_headless(cli_args: &CliArgs) -> bool {
    cli_args.transcribe_file.is_some() || cli_args.list_devices || cli_args.list_models
}

/// Create the app handle for this process (portable-aware paths).
pub fn create_app_handle() -> AppHandle {
    let mut paths = host::AppPaths::platform_default();
    if let Some(data_dir) = portable::data_dir() {
        paths.app_data_dir = data_dir.clone();
        paths.app_log_dir = data_dir.join("logs");
    }
    AppHandle::new(paths)
}

#[allow(unused_variables)]
fn should_force_show_permissions_window(app: &AppHandle) -> bool {
    #[cfg(target_os = "windows")]
    {
        let model_manager = app.state::<Arc<ModelManager>>();
        let has_downloaded_models = model_manager
            .get_available_models()
            .iter()
            .any(|model| model.is_downloaded);

        if !has_downloaded_models {
            return false;
        }

        let status = commands::audio::get_windows_microphone_permission_status();
        if status.supported && status.overall_access == commands::audio::PermissionAccess::Denied {
            log::info!("Windows microphone permissions are denied; forcing main window visible for onboarding");
            return true;
        }
    }

    false
}

fn initialize_core_logic(app_handle: &AppHandle, build_tray: bool) {
    // Note: Enigo (keyboard/mouse simulation) is NOT initialized here.
    // The UI calls `initialize_enigo` after onboarding completes. This avoids
    // triggering permission dialogs on macOS before the user is ready.

    // Initialize the managers. The audio recorder receives the streaming router
    // explicitly, so always-on microphone startup can wire live-preview frames
    // even before managed state is populated.
    let model_manager =
        Arc::new(ModelManager::new(app_handle).expect("Failed to initialize model manager"));
    let transcription_manager = Arc::new(
        TranscriptionManager::new(app_handle, model_manager.clone())
            .expect("Failed to initialize transcription manager"),
    );
    let recording_manager = Arc::new(
        AudioRecordingManager::new(app_handle, transcription_manager.stream_router())
            .expect("Failed to initialize recording manager"),
    );
    let history_manager =
        Arc::new(HistoryManager::new(app_handle).expect("Failed to initialize history manager"));

    app_handle.manage(recording_manager.clone());
    app_handle.manage(model_manager.clone());
    app_handle.manage(transcription_manager.clone());
    app_handle.manage(history_manager.clone());
    app_handle.manage(tray::TrayState::new());

    // Note: Shortcuts are NOT initialized here either; the UI calls
    // `initialize_shortcuts` after permissions are confirmed (macOS) or after
    // onboarding completes.

    // Set up signal handlers for toggling transcription.
    #[cfg(unix)]
    signal_handle::setup_signal_handler(app_handle.clone());

    if build_tray {
        if let Err(e) = tray::build_tray(app_handle) {
            log::error!("Failed to build tray icon: {}", e);
        }
    }

    // Initialize tray menu with idle state
    tray::update_tray_menu(app_handle);

    // Apply show_tray_icon setting
    let settings = settings::get_settings(app_handle);
    if !settings.show_tray_icon {
        tray::set_tray_visibility(app_handle, false);
    }

    // Refresh tray menu when model state changes
    let app_handle_for_listener = app_handle.clone();
    app_handle.listen("model-state-changed", move |_| {
        tray::update_tray_menu(&app_handle_for_listener);
    });

    // Meeting notes (macOS 14.2+).
    init_meetings(app_handle);

    // Apply the autostart preference (SMAppService login item on macOS 13+,
    // auto-launch elsewhere)
    autostart::apply_autostart(app_handle, settings.autostart_enabled);

    // The recording overlay window is created by the UI at startup (hidden).
    utils::create_recording_overlay(app_handle);
}

/// Register [`meetings::manager::MeetingsManager`] when meeting notes are
/// supported. Opening the encrypted store touches the Keychain, so it runs
/// off the main thread; the UI treats a missing manager as "not available
/// yet". The manager then starts the scheduler (if enabled), recovers
/// recordings interrupted by a crash, and applies audio retention daily.
fn init_meetings(app_handle: &AppHandle) {
    if !meetings::is_supported() {
        return;
    }

    // Tray icon/menu follow the recorder.
    let tray_app = app_handle.clone();
    app_handle.listen(meetings::types::events::RECORDER_STATE, move |_| {
        tray::update_tray_menu(&tray_app);
    });
    // The `meetings.enabled` switch starts/stops the scheduler and shows or
    // hides the tray items.
    let settings_app = app_handle.clone();
    app_handle.listen("settings-changed", move |_| {
        if let Some(m) = settings_app.try_state::<Arc<meetings::manager::MeetingsManager>>() {
            m.sync_scheduler();
        }
        tray::update_tray_menu(&settings_app);
    });

    let app = app_handle.clone();
    let spawned = std::thread::Builder::new()
        .name("meetings-init".into())
        .spawn(
            move || match meetings::manager::MeetingsManager::new(&app) {
                Ok(manager) => {
                    app.manage(manager.clone());
                    manager.start_background();
                    tray::update_tray_menu(&app);
                }
                Err(e) => log::error!("Meeting notes unavailable: {e:#}"),
            },
        );
    if let Err(e) = spawned {
        log::error!("Failed to start meeting notes: {e}");
    }
}

/// What the UI should do once the backend is initialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupDecision {
    pub show_main_window: bool,
    /// macOS: run as an Accessory app (no Dock icon) — only when starting
    /// hidden with a tray available.
    pub start_as_accessory: bool,
}

/// Decide window visibility from settings and CLI flags. If start_hidden but
/// the tray is disabled we must show the window anyway: without a tray icon
/// the dock is the only way back in.
pub fn startup_decision(
    start_hidden_setting: bool,
    show_tray_icon_setting: bool,
    cli_args: &CliArgs,
    force_show: bool,
) -> StartupDecision {
    let should_hide = start_hidden_setting || cli_args.start_hidden;
    let tray_available = show_tray_icon_setting && !cli_args.no_tray;
    let show = force_show || !should_hide || !tray_available;
    StartupDecision {
        show_main_window: show,
        start_as_accessory: !show && should_hide && tray_available,
    }
}

/// Full interactive startup (everything but window creation, which the UI
/// owns). Must run on the main thread.
pub fn initialize_app(
    app_handle: &AppHandle,
    cli_args: &CliArgs,
    build_tray: bool,
) -> StartupDecision {
    app_handle.manage(cli_args.clone());

    let mut settings = get_settings(app_handle);

    // Apply the persisted appearance theme before the window is shown.
    shortcut::apply_window_theme(app_handle, settings.theme);

    // CLI --debug flag overrides debug_mode and log level (runtime-only, not persisted)
    if cli_args.debug {
        settings.debug_mode = true;
        settings.log_level = settings::LogLevel::Trace;
    }

    let file_log_level: log::Level = settings.log_level.into();
    FILE_LOG_LEVEL.store(file_log_level.to_level_filter() as u8, Ordering::Relaxed);
    // Only forward logs to the UI while debug mode is on.
    LOG_STREAMING.store(settings.debug_mode, Ordering::Relaxed);
    logging::attach_app(app_handle);

    app_handle.manage(TranscriptionCoordinator::new(app_handle.clone()));

    initialize_core_logic(app_handle, build_tray);

    // Secure Input monitor (macOS): detects stuck secure input that
    // silently blocks keyed shortcuts, warns the user, and activates
    // the Carbon fallback.
    secure_input::init(app_handle);

    // Populate the overlay-enabled cache from initial settings so the audio
    // path (overlay::emit_levels, ~24 Hz during recording) can do a single
    // atomic load. Kept in sync by shortcut::change_overlay_style_setting.
    overlay::update_overlay_enabled_cache(settings.overlay_style != settings::OverlayStyle::None);

    // Pre-warm GPU/accelerator enumeration on a background thread so the
    // first open of Advanced settings doesn't freeze on Metal shader compiles.
    let devices_app_handle = app_handle.clone();
    std::thread::spawn(move || {
        let tm = devices_app_handle.state::<Arc<TranscriptionManager>>();
        crate::managers::transcription::report_compute_devices(&tm);
        let _ = crate::managers::transcription::get_available_accelerators(&tm);
    });

    // Hide tray icon if --no-tray was passed
    if cli_args.no_tray {
        tray::set_tray_visibility(app_handle, false);
    }

    startup_decision(
        settings.start_hidden,
        settings.show_tray_icon,
        cli_args,
        should_force_show_permissions_window(app_handle),
    )
}

/// Handle the arguments forwarded by a second launch (single instance).
pub fn handle_second_instance(app: &AppHandle, args: &[String]) {
    if args.iter().any(|a| a == "--toggle-transcription") {
        signal_handle::send_transcription_input(app, "transcribe", "CLI");
    } else if args.iter().any(|a| a == "--toggle-post-process") {
        signal_handle::send_transcription_input(app, "transcribe_with_post_process", "CLI");
    } else if args.iter().any(|a| a == "--cancel") {
        crate::utils::cancel_current_operation(app);
    } else {
        // Launched again without remote-control flags: raise the window and
        // recreate a possibly vanished tray icon.
        #[cfg(target_os = "macos")]
        tray::recreate_tray_icon(app);
        app.shell().show_main_window();
    }
}

/// macOS Reopen (Dock click / relaunch from Finder).
pub fn handle_reopen(app: &AppHandle) {
    if !app.shell().is_main_window_visible() {
        #[cfg(target_os = "macos")]
        tray::recreate_tray_icon(app);
    }
    app.shell().show_main_window();
}

/// The main window's close button hides it; with a tray on macOS the Dock
/// icon is hidden too (the app lives in the tray).
pub fn handle_main_window_close(app: &AppHandle) {
    app.shell().hide_main_window();
    #[cfg(target_os = "macos")]
    {
        let settings = get_settings(app);
        let no_tray = app
            .try_state::<CliArgs>()
            .map(|a| a.no_tray)
            .unwrap_or(false);
        if settings.show_tray_icon && !no_tray {
            app.shell().set_activation_policy_accessory(true);
        }
    }
}

pub fn trigger_update_check(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    if !settings::update_checks_effectively_enabled(&settings) {
        return Ok(());
    }
    app.emit("check-for-updates", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn show_main_window_command(app: AppHandle) -> Result<(), String> {
    app.shell().show_main_window();
    Ok(())
}

/// Headless one-shot path: initialize only what transcription needs — the
/// model + transcription managers — then run and return the exit code.
/// Deliberately skips the window, tray, overlay, audio recorder (so it never
/// opens the mic), signal handlers, and autostart.
pub fn run_headless(app_handle: &AppHandle, cli_args: &CliArgs) -> i32 {
    let model_manager =
        Arc::new(ModelManager::new(app_handle).expect("Failed to initialize model manager"));
    let transcription_manager = Arc::new(
        TranscriptionManager::new(app_handle, model_manager.clone())
            .expect("Failed to initialize transcription manager"),
    );
    managers::transcription::report_compute_devices(&transcription_manager);
    app_handle.manage(model_manager);
    app_handle.manage(transcription_manager);

    let code = run_headless_guarded(|| run_headless_transcription(app_handle, cli_args));
    // Drop the loaded engine before exit so its transcription worker is told
    // to shut down cleanly.
    if let Some(tm) = app_handle.try_state::<Arc<TranscriptionManager>>() {
        let _ = tm.unload_model();
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    code
}

/// Convert an unexpected panic on the headless worker into a normal CLI
/// failure.
fn run_headless_guarded<F>(operation: F) -> i32
where
    F: FnOnce() -> i32,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(code) => code,
        Err(payload) => {
            let message = if let Some(message) = payload.downcast_ref::<&str>() {
                (*message).to_string()
            } else if let Some(message) = payload.downcast_ref::<String>() {
                message.clone()
            } else {
                "unknown panic".to_string()
            };
            eprintln!("error: headless transcription panicked: {message}");
            1
        }
    }
}

#[cfg(test)]
mod headless_guard_tests {
    use super::run_headless_guarded;

    #[test]
    fn preserves_normal_exit_codes() {
        assert_eq!(run_headless_guarded(|| 2), 2);
    }

    #[test]
    fn converts_worker_panics_to_runtime_failures() {
        assert_eq!(run_headless_guarded(|| panic!("simulated failure")), 1);
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    fn args() -> CliArgs {
        CliArgs::default()
    }

    #[test]
    fn shows_window_by_default() {
        let d = startup_decision(false, true, &args(), false);
        assert!(d.show_main_window);
        assert!(!d.start_as_accessory);
    }

    #[test]
    fn start_hidden_with_tray_hides_and_runs_as_accessory() {
        let d = startup_decision(true, true, &args(), false);
        assert!(!d.show_main_window);
        assert!(d.start_as_accessory);
        let mut a = args();
        a.start_hidden = true;
        assert!(!startup_decision(false, true, &a, false).show_main_window);
    }

    #[test]
    fn start_hidden_without_tray_still_shows_window() {
        assert!(startup_decision(true, false, &args(), false).show_main_window);
        let mut a = args();
        a.no_tray = true;
        assert!(startup_decision(true, true, &a, false).show_main_window);
    }

    #[test]
    fn permission_onboarding_forces_window() {
        assert!(startup_decision(true, true, &args(), true).show_main_window);
    }
}

fn run_headless_transcription(app: &AppHandle, args: &CliArgs) -> i32 {
    use std::time::Instant;

    // --list-devices: print registered compute devices (with indices) and exit.
    // Useful on multi-GPU machines to discover the index for --device-index.
    if args.list_devices {
        let tm = app.state::<Arc<TranscriptionManager>>();
        let devices = match crate::managers::transcription::describe_compute_devices(&tm) {
            Ok(devices) => devices,
            Err(e) => {
                eprintln!("error: {}", e);
                return 1;
            }
        };
        if devices.is_empty() {
            println!("No transcribe-cpp compute devices registered.");
        } else {
            println!("transcribe-cpp compute devices:");
            for d in &devices {
                println!("  {}", d);
            }
        }
        if args.transcribe_file.is_none() {
            return 0;
        }
    }

    // --list-models: print the model registry (catalog + on-disk + custom) with
    // their ids — the same ids `--model` accepts — then exit. `--json` emits the
    // full ModelInfo array for scripting.
    if args.list_models {
        let model_manager = app.state::<Arc<ModelManager>>();
        let models = model_manager.get_available_models();
        if args.json {
            match serde_json::to_string_pretty(&models) {
                Ok(s) => println!("{}", s),
                Err(e) => {
                    eprintln!("error: failed to serialize models: {}", e);
                    return 1;
                }
            }
        } else if models.is_empty() {
            println!("No models available.");
        } else {
            println!("Available models (✓ = installed):");
            let width = models.iter().map(|m| m.id.len()).max().unwrap_or(0);
            for m in &models {
                let mark = if m.is_downloaded { "✓" } else { " " };
                let rec = if m.is_recommended {
                    "  [recommended]"
                } else {
                    ""
                };
                println!(
                    "  {}  {:<width$}  {}{}",
                    mark,
                    m.id,
                    m.name,
                    rec,
                    width = width
                );
            }
        }
        if args.transcribe_file.is_none() {
            return 0;
        }
    }

    let Some(wav) = args.transcribe_file.clone() else {
        return 0;
    };

    // read_wav_samples reads 16-bit int samples and does no validation; the app
    // only ever saves 16 kHz mono 16-bit PCM, so reject anything else rather than
    // transcribe garbage / mis-time / mis-decode.
    match hound::WavReader::open(&wav) {
        Ok(reader) => {
            let spec = reader.spec();
            if spec.sample_rate != 16_000
                || spec.channels != 1
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
            {
                eprintln!(
                    "error: expected 16 kHz mono 16-bit PCM WAV, got {} Hz / {} ch / {}-bit {:?}",
                    spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
                );
                return 2;
            }
        }
        Err(e) => {
            eprintln!("error: cannot open {}: {}", wav.display(), e);
            return 2;
        }
    }

    let samples = match crate::audio_toolkit::read_wav_samples(&wav) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to read {}: {}", wav.display(), e);
            return 2;
        }
    };
    let audio_secs = samples.len() as f64 / 16_000.0;

    let tm = app.state::<Arc<TranscriptionManager>>();

    let model_id = args
        .model
        .clone()
        .unwrap_or_else(|| get_settings(app).selected_model);
    if model_id.is_empty() {
        eprintln!("error: no model selected (pass --model or pick one in the app)");
        return 2;
    }

    // --device-index hard-selects a compute device by its --list-devices registry
    // index (transcribe-cpp / whisper-family models only; not persisted). Omit it
    // to use the persisted accelerator setting.
    let device_index = args.device_index;
    let requested_device = match device_index {
        Some(idx) => format!("index {}", idx),
        None => "settings".to_string(),
    };

    // Cold load (timed).
    let load_start = Instant::now();
    if let Err(e) = tm.load_model_with_device(&model_id, device_index) {
        eprintln!("error: load_model('{}') failed: {}", model_id, e);
        return 1;
    }
    let load_ms = load_start.elapsed().as_millis() as u64;
    let bound_backend = tm.current_backend();

    let runs = args.repeat.unwrap_or(1).max(1);
    let mut times_ms: Vec<u64> = Vec::new();
    let mut text = String::new();
    for i in 0..runs {
        // If the model's unload-timeout is "Immediately", transcribe() unloads
        // the engine after each run; reload (untimed) so repeats keep working
        // and the inference timing below stays clean.
        if !tm.is_model_loaded() {
            if let Err(e) = tm.load_model_with_device(&model_id, device_index) {
                eprintln!("error: reload before run {} failed: {}", i + 1, e);
                return 1;
            }
        }
        let t = Instant::now();
        match tm.transcribe(samples.clone()) {
            Ok(out) => text = out,
            Err(e) => {
                eprintln!("error: transcribe failed: {}", e);
                return 1;
            }
        }
        times_ms.push(t.elapsed().as_millis() as u64);
    }
    let best_ms = times_ms.iter().copied().min().unwrap_or(0);
    let rtf = if best_ms > 0 {
        audio_secs / (best_ms as f64 / 1000.0)
    } else {
        0.0
    };

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "model": model_id,
                "requested_device": requested_device,
                "bound_backend": bound_backend,
                "audio_secs": audio_secs,
                "load_ms": load_ms,
                "transcribe_ms": times_ms,
                "best_ms": best_ms,
                "rtf": rtf,
                "text": text,
            })
        );
    } else {
        println!(
            "model={} device={} backend={} audio={:.2}s load={}ms best={}ms rtf={:.2}x",
            model_id,
            requested_device,
            bound_backend.as_deref().unwrap_or("?"),
            audio_secs,
            load_ms,
            best_ms,
            rtf,
        );
        println!("text: {}", text);
    }
    0
}
