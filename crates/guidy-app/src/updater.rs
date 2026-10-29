//! Self-update through `cargo-packager-updater` (same minisign-signed
//! `latest.json` manifest shape Tauri's updater used), plus the footer
//! update status state machine.
//!
//! Endpoint and public key are build-time settings (`GUIDY_UPDATE_ENDPOINT`,
//! `GUIDY_UPDATE_PUBKEY`); without them checks report "no update".

use std::sync::Arc;

pub const ENDPOINT: &str = env!("GUIDY_UPDATE_ENDPOINT");
pub const PUBKEY: &str = env!("GUIDY_UPDATE_PUBKEY");

pub fn configured() -> bool {
    !ENDPOINT.is_empty() && !PUBKEY.is_empty()
}

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub version: String,
    /// The raw manifest (used to deep-link portable installers).
    pub raw_manifest: Option<serde_json::Value>,
}

fn updater() -> Result<cargo_packager_updater::Updater, String> {
    let endpoint = ENDPOINT
        .parse()
        .map_err(|e| format!("bad update endpoint: {e}"))?;
    let version = env!("CARGO_PKG_VERSION")
        .parse()
        .map_err(|e| format!("bad version: {e}"))?;
    let config = cargo_packager_updater::Config {
        endpoints: vec![endpoint],
        pubkey: PUBKEY.to_string(),
        windows: None,
    };
    cargo_packager_updater::UpdaterBuilder::new(version, config)
        .build()
        .map_err(|e| e.to_string())
}

fn fetch_manifest() -> Option<serde_json::Value> {
    let url = ENDPOINT.to_string();
    guidy_core::host::async_runtime::block_on(async move {
        let resp = reqwest::get(url).await.ok()?;
        resp.json::<serde_json::Value>().await.ok()
    })
}

/// Blocking check. `Ok(None)` when up to date or unconfigured.
pub fn check() -> Result<Option<AvailableUpdate>, String> {
    if !configured() {
        return Ok(None);
    }
    let update = updater()?.check().map_err(|e| e.to_string())?;
    Ok(update.map(|u| AvailableUpdate {
        version: u.version.clone(),
        raw_manifest: fetch_manifest(),
    }))
}

/// Blocking download + install, reporting (downloaded, total) bytes.
pub fn download_and_install(
    progress: Arc<dyn Fn(u64, Option<u64>) + Send + Sync>,
) -> Result<(), String> {
    let Some(update) = updater()?.check().map_err(|e| e.to_string())? else {
        log::info!("No update available during install attempt");
        return Ok(());
    };
    let downloaded = std::sync::atomic::AtomicU64::new(0);
    update
        .download_and_install_extended(
            |chunk, total| {
                let d = downloaded.fetch_add(chunk as u64, std::sync::atomic::Ordering::Relaxed)
                    + chunk as u64;
                progress(d, total);
            },
            || {},
        )
        .map_err(|e| e.to_string())
}

/// Start the (updated) app again and exit this process.
pub fn relaunch() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// Footer status (UpdateChecker.tsx)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateStatus {
    pub enabled: bool,
    pub checking: bool,
    pub available: bool,
    pub installing: bool,
    /// 0–100
    pub progress: u32,
    pub show_up_to_date: bool,
    pub manual: bool,
    /// The last check or install failed (cleared when the next starts).
    pub failed: Option<UpdateFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateFailure {
    /// The check could not reach the update server; clicking retries.
    Check,
    /// Downloading or installing the update failed; clicking retries.
    Install,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    Install,
    Check,
}

impl UpdateStatus {
    /// i18n key + optional progress arg for the footer text.
    pub fn text_key(&self) -> (&'static str, Option<String>) {
        if !self.enabled {
            return ("footer.updateCheckingDisabled", None);
        }
        if self.installing {
            return if self.progress > 0 && self.progress < 100 {
                ("footer.downloading", Some(format!("{:>3}", self.progress)))
            } else if self.progress == 100 {
                ("footer.installing", None)
            } else {
                ("footer.preparing", None)
            };
        }
        if self.checking {
            return ("footer.checkingUpdates", None);
        }
        match self.failed {
            Some(UpdateFailure::Check) => return ("footer.updateCheckFailed", None),
            Some(UpdateFailure::Install) => return ("footer.updateInstallFailed", None),
            None => {}
        }
        if self.show_up_to_date {
            return ("footer.upToDate", None);
        }
        if self.available {
            return ("footer.updateAvailableShort", None);
        }
        ("footer.checkForUpdates", None)
    }

    pub fn action(&self) -> Option<UpdateAction> {
        if !self.enabled {
            return None;
        }
        if self.available && !self.installing {
            return Some(UpdateAction::Install);
        }
        if !self.checking && !self.installing && !self.available {
            return Some(UpdateAction::Check);
        }
        None
    }

    pub fn clickable(&self) -> bool {
        let disabled = !self.enabled || self.checking || self.installing;
        !disabled && (self.available || (!self.checking && !self.show_up_to_date))
    }

    /// A check finished.
    pub fn finish_check(&mut self, available: bool) {
        self.checking = false;
        self.available = available;
        self.failed = None;
        self.show_up_to_date = !available && self.manual;
        self.manual = false;
    }

    /// A check failed: say so (never "up to date") and offer a retry.
    pub fn fail_check(&mut self) {
        self.checking = false;
        self.failed = Some(UpdateFailure::Check);
        self.show_up_to_date = false;
        self.manual = false;
    }

    /// Installing failed: the update is still available; offer a retry.
    pub fn fail_install(&mut self) {
        self.installing = false;
        self.progress = 0;
        self.failed = Some(UpdateFailure::Install);
    }

    /// The footer text should read as an error.
    pub fn is_error(&self) -> bool {
        self.failed.is_some() && !self.checking && !self.installing
    }

    pub fn progress_percent(downloaded: u64, total: Option<u64>) -> u32 {
        match total {
            Some(t) if t > 0 => ((downloaded as f64 / t as f64) * 100.0).round().min(100.0) as u32,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> UpdateStatus {
        UpdateStatus {
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn disabled_shows_disabled_and_has_no_action() {
        let s = UpdateStatus::default();
        assert_eq!(s.text_key().0, "footer.updateCheckingDisabled");
        assert_eq!(s.action(), None);
        assert!(!s.clickable());
    }

    #[test]
    fn manual_check_shows_up_to_date_then_auto_check_does_not() {
        let mut s = enabled();
        s.manual = true;
        s.checking = true;
        assert_eq!(s.text_key().0, "footer.checkingUpdates");
        assert!(!s.clickable());
        s.finish_check(false);
        assert_eq!(s.text_key().0, "footer.upToDate");
        assert!(!s.clickable());
        let mut s = enabled();
        s.checking = true;
        s.finish_check(false);
        assert_eq!(s.text_key().0, "footer.checkForUpdates");
        assert_eq!(s.action(), Some(UpdateAction::Check));
    }

    #[test]
    fn available_update_installs_with_progress_text() {
        let mut s = enabled();
        s.finish_check(true);
        assert_eq!(s.text_key().0, "footer.updateAvailableShort");
        assert_eq!(s.action(), Some(UpdateAction::Install));
        s.installing = true;
        assert_eq!(s.text_key().0, "footer.preparing");
        s.progress = 42;
        assert_eq!(s.text_key(), ("footer.downloading", Some(" 42".into())));
        s.progress = 100;
        assert_eq!(s.text_key().0, "footer.installing");
        assert_eq!(UpdateStatus::progress_percent(50, Some(200)), 25);
        assert_eq!(UpdateStatus::progress_percent(50, None), 0);
    }

    #[test]
    fn failures_are_reported_and_retryable() {
        let mut s = enabled();
        s.manual = true;
        s.checking = true;
        s.fail_check();
        assert_eq!(s.text_key().0, "footer.updateCheckFailed");
        assert!(s.is_error());
        assert_eq!(s.action(), Some(UpdateAction::Check));
        assert!(s.clickable());
        s.checking = true;
        s.finish_check(true);
        assert!(!s.is_error());
        s.installing = true;
        s.fail_install();
        assert_eq!(s.text_key().0, "footer.updateInstallFailed");
        assert_eq!(s.action(), Some(UpdateAction::Install));
        assert!(s.clickable());
    }

    #[test]
    fn unconfigured_builds_report_no_update() {
        if !configured() {
            assert!(check().unwrap().is_none());
        }
    }
}
