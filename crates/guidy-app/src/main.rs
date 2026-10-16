// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use guidy_core::CliArgs;

fn main() {
    #[cfg(target_os = "windows")]
    {
        // Avoid overlay/capture layer crashes. Set before backend
        // initialization, preserving user overrides.
        if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_none()
            && !guidy_core::env_flag_enabled("GUIDY_KEEP_VULKAN_IMPLICIT_LAYERS")
        {
            std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~");
        }
    }

    // Transcription worker: runs transcribe.cpp in isolation and never touches
    // the UI, the CLI, or single-instance handling. Checked after the env
    // setup above so the worker inherits it.
    if guidy_core::engine_supervisor::is_worker_invocation() {
        std::process::exit(guidy_core::engine_supervisor::run_worker());
    }

    let cli_args = CliArgs::parse();
    guidy_core::prepare_process();

    if guidy_core::is_headless(&cli_args) {
        let app = guidy_core::create_app_handle();
        guidy_core::logging::init(&app.paths().app_log_dir.clone(), true);
        std::process::exit(guidy_core::run_headless(&app, &cli_args));
    }

    guidy_app::run(cli_args);
}
