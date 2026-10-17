pub mod audio;
pub mod history;
pub mod models;
pub mod transcription;

use crate::host::AppHandle;
use crate::settings::{
    get_settings, update_checks_forced_disabled, update_settings, AppSettings, LogLevel,
};
use crate::utils::cancel_current_operation;
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}
pub fn is_portable() -> bool {
    crate::portable::is_portable()
}
pub fn is_update_checks_locked() -> bool {
    update_checks_forced_disabled()
}
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}
pub fn get_app_settings(app: AppHandle) -> Result<AppSettings, String> {
    Ok(get_settings(&app))
}
pub fn get_default_settings() -> Result<AppSettings, String> {
    Ok(crate::settings::get_default_settings())
}
pub fn get_log_dir_path(app: AppHandle) -> Result<String, String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    Ok(log_dir.to_string_lossy().to_string())
}
pub fn set_log_level(app: AppHandle, level: LogLevel) -> Result<(), String> {
    let log_level: log::Level = level.into();
    // Update the file log level atomic so the filter picks up the new level
    crate::FILE_LOG_LEVEL.store(
        log_level.to_level_filter() as u8,
        std::sync::atomic::Ordering::Relaxed,
    );

    update_settings(&app, |s| s.log_level = level);

    Ok(())
}
pub fn open_recordings_folder(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let recordings_dir = app_data_dir.join("recordings");

    let path = recordings_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open recordings folder: {}", e))?;

    Ok(())
}
pub fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    let path = log_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open log directory: {}", e))?;

    Ok(())
}
pub fn open_app_data_dir(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let path = app_data_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open app data directory: {}", e))?;

    Ok(())
}

/// Check if Apple Intelligence is available on this device.
/// Called by the frontend when the user selects Apple Intelligence provider.
pub fn check_apple_intelligence_available() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        crate::apple_intelligence::check_apple_intelligence_availability()
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

/// Try to initialize Enigo (keyboard/mouse simulation).
/// On macOS, this will return an error if accessibility permissions are not granted.
pub fn initialize_enigo(app: AppHandle) -> Result<(), String> {
    use crate::input::EnigoState;

    // Check if already initialized
    if app.try_state::<EnigoState>().is_some() {
        log::debug!("Enigo already initialized");
        return Ok(());
    }

    // Try to initialize
    match EnigoState::new() {
        Ok(enigo_state) => {
            app.manage(enigo_state);
            log::info!("Enigo initialized successfully after permission grant");
            Ok(())
        }
        Err(e) => {
            if cfg!(target_os = "macos") {
                log::warn!(
                    "Failed to initialize Enigo: {} (accessibility permissions may not be granted)",
                    e
                );
            } else {
                log::warn!("Failed to initialize Enigo: {}", e);
            }
            Err(format!("Failed to initialize input system: {}", e))
        }
    }
}

/// Marker state to track if shortcuts have been initialized.
pub struct ShortcutsInitialized;

/// Initialize keyboard shortcuts.
/// On macOS, this should be called after accessibility permissions are granted.
/// This is idempotent - calling it multiple times is safe.
pub fn initialize_shortcuts(app: AppHandle) -> Result<(), String> {
    // Check if already initialized
    if app.try_state::<ShortcutsInitialized>().is_some() {
        log::debug!("Shortcuts already initialized");
        return Ok(());
    }

    // Initialize shortcuts
    crate::shortcut::init_shortcuts(&app);

    // Mark as initialized before reconciling the macOS Secure Input fallback.
    app.manage(ShortcutsInitialized);
    crate::secure_input::reconcile_fallback(&app);

    log::info!("Shortcuts initialized successfully");
    Ok(())
}

/// App-level commands against a temp app handle (PARITY C-15, C-18).
#[cfg(test)]
mod app_command_tests {
    use super::*;
    use crate::host::testing::TestApp;

    #[test]
    fn paths_and_portable_flag() {
        let t = TestApp::new();
        assert!(!is_portable());
        assert_eq!(
            get_app_dir_path(t.app.clone()).unwrap(),
            t.app.paths().app_data_dir.to_string_lossy()
        );
        assert_eq!(
            get_log_dir_path(t.app.clone()).unwrap(),
            t.app.paths().app_log_dir.to_string_lossy()
        );
    }

    #[test]
    fn set_log_level_updates_file_filter_and_setting() {
        let t = TestApp::new();
        set_log_level(t.app.clone(), LogLevel::Warn).unwrap();
        assert_eq!(
            crate::FILE_LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed),
            log::LevelFilter::Warn as u8
        );
        assert_eq!(
            get_app_settings(t.app.clone()).unwrap().log_level,
            LogLevel::Warn
        );
    }

    #[test]
    fn update_check_trigger_respects_setting() {
        let t = TestApp::new();
        crate::trigger_update_check(t.app.clone()).unwrap();
        let enabled = crate::settings::update_checks_effectively_enabled(
            &crate::settings::get_settings(&t.app),
        );
        assert_eq!(
            t.event_names().contains(&"check-for-updates".to_string()),
            enabled
        );
        crate::settings::update_settings(&t.app, |s| s.update_checks_enabled = false);
        t.take_events();
        crate::trigger_update_check(t.app.clone()).unwrap();
        assert!(!t.event_names().contains(&"check-for-updates".to_string()));
    }

    #[test]
    fn model_unload_timeout_persists() {
        let t = TestApp::new();
        super::transcription::set_model_unload_timeout(
            t.app.clone(),
            crate::settings::ModelUnloadTimeout::Hour1,
        );
        assert_eq!(
            get_app_settings(t.app.clone())
                .unwrap()
                .model_unload_timeout,
            crate::settings::ModelUnloadTimeout::Hour1
        );
    }
}
