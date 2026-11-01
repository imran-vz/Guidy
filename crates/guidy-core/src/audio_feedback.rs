use crate::audio_toolkit::audio::output_device_name;
use crate::host::AppHandle;
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use log::{debug, error, warn};
use rodio::cpal::traits::HostTrait;
use rodio::DeviceSinkBuilder;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;

/// Every theme, Custom included, plays this for `SoundType::Error`.
const ERROR_SOUND_PATH: &str = "resources/mechanical_error.wav";

#[derive(Clone, Copy)]
pub enum SoundType {
    Start,
    Stop,
    Error,
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let sound_file = get_sound_path(settings, sound_type);
    let base_dir = get_sound_base_dir(settings, sound_type);
    match base_dir {
        crate::host::path::BaseDirectory::AppData => {
            crate::portable::resolve_app_data(app, &sound_file).ok()
        }
        _ => app.path().resolve(&sound_file, base_dir).ok(),
    }
}

fn get_sound_path(settings: &AppSettings, sound_type: SoundType) -> String {
    match (settings.sound_theme, sound_type) {
        (_, SoundType::Error) => ERROR_SOUND_PATH.to_string(),
        (SoundTheme::Custom, SoundType::Start) => "custom_start.wav".to_string(),
        (SoundTheme::Custom, SoundType::Stop) => "custom_stop.wav".to_string(),
        (_, SoundType::Start) => settings.sound_theme.to_start_path(),
        (_, SoundType::Stop) => settings.sound_theme.to_stop_path(),
    }
}

fn get_sound_base_dir(
    settings: &AppSettings,
    sound_type: SoundType,
) -> crate::host::path::BaseDirectory {
    match (settings.sound_theme, sound_type) {
        (_, SoundType::Error) => crate::host::path::BaseDirectory::Resource,
        (SoundTheme::Custom, _) => crate::host::path::BaseDirectory::AppData,
        _ => crate::host::path::BaseDirectory::Resource,
    }
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(app, path);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

fn play_sound_async(app: &AppHandle, path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Err(e) = play_sound_at_path(&app_handle, path.as_path()) {
            error!("Failed to play sound '{}': {}", path.display(), e);
        }
    });
}

fn play_sound_blocking(app: &AppHandle, path: &Path) {
    if let Err(e) = play_sound_at_path(app, path) {
        error!("Failed to play sound '{}': {}", path.display(), e);
    }
}

fn play_sound_at_path(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::get_settings(app);
    let volume = settings.audio_feedback_volume;
    let selected_device = settings.selected_output_device.clone();
    play_audio_file(path, selected_device, volume)
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let sink_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            debug!("Using default device");
            DeviceSinkBuilder::from_default_device()?
        } else {
            let host = crate::audio_toolkit::get_cpal_host();
            let found_device = host
                .output_devices()?
                .find(|device| output_device_name(device).as_deref() == Some(&device_name));

            match found_device {
                Some(device) => DeviceSinkBuilder::from_device(device)?,
                None => {
                    warn!("Device '{}' not found, using default device", device_name);
                    DeviceSinkBuilder::from_default_device()?
                }
            }
        }
    } else {
        debug!("Using default device");
        DeviceSinkBuilder::from_default_device()?
    };

    let device_sink = sink_builder.open_stream()?;
    let mixer = device_sink.mixer();

    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let player = rodio::play(mixer, buf_reader)?;
    player.set_volume(volume);
    player.sleep_until_end();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::path::BaseDirectory;
    use crate::host::testing::TestApp;

    fn settings_with_theme(theme: SoundTheme) -> AppSettings {
        let mut settings = settings::get_default_settings();
        settings.sound_theme = theme;
        settings
    }

    #[test]
    fn mechanical_paths_exist_under_assets() {
        let settings = settings_with_theme(SoundTheme::Mechanical);
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for sound_type in [SoundType::Start, SoundType::Stop, SoundType::Error] {
            let path = assets.join(get_sound_path(&settings, sound_type));
            assert!(path.exists(), "{}", path.display());
            assert_eq!(
                get_sound_base_dir(&settings, sound_type),
                BaseDirectory::Resource
            );
        }
    }

    #[test]
    fn error_sound_resolves_to_resource_under_every_theme() {
        let t = TestApp::new();
        for theme in [
            SoundTheme::Mechanical,
            SoundTheme::Marimba,
            SoundTheme::Pop,
            SoundTheme::Custom,
        ] {
            let settings = settings_with_theme(theme);
            assert_eq!(
                get_sound_path(&settings, SoundType::Error),
                ERROR_SOUND_PATH
            );
            assert_eq!(
                get_sound_base_dir(&settings, SoundType::Error),
                BaseDirectory::Resource
            );
            let path = resolve_sound_path(&t.app, &settings, SoundType::Error).unwrap();
            assert!(path.exists(), "{}", path.display());
        }
    }

    #[test]
    fn custom_start_and_stop_stay_in_app_data() {
        let settings = settings_with_theme(SoundTheme::Custom);
        for (sound_type, expected) in [
            (SoundType::Start, "custom_start.wav"),
            (SoundType::Stop, "custom_stop.wav"),
        ] {
            assert_eq!(get_sound_path(&settings, sound_type), expected);
            assert_eq!(
                get_sound_base_dir(&settings, sound_type),
                BaseDirectory::AppData
            );
        }
    }
}
