//! GGUF catalog for the built-in llama.cpp runtime (plan task 2.2).
//!
//! Models live in `<app_data_dir>/models/llm/`. Downloads reuse the
//! resumable, size- and SHA256-verified transport of the transcription model
//! downloads (`managers::model::download_file`), resume from a `.partial`
//! file, and report progress as [`events::DOWNLOAD_PROGRESS`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use hf_hub::api::tokio::CancellationToken;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::host::AppHandle;
use crate::managers::model::DownloadProgress;

pub mod events {
    /// Payload: [`crate::managers::model::DownloadProgress`] (`model_id` is the
    /// catalog id).
    pub const DOWNLOAD_PROGRESS: &str = "llm-models://download-progress";
    /// Payload: [`super::DownloadEvent`]. Sent when a download starts,
    /// finishes, fails or is cancelled, and when a model is deleted.
    pub const CHANGED: &str = "llm-models://changed";
}

/// One entry of the static catalog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub repo: &'static str,
    pub filename: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
    /// Context the worker loads it with.
    pub n_ctx: u32,
    /// RAM this model is recommended for (None = any supported Mac).
    pub recommended_min_ram_gb: Option<u32>,
    pub is_default: bool,
}

impl CatalogEntry {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.repo, self.filename
        )
    }
}

/// Default first. Sizes and SHA256s are the Hugging Face LFS values.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "qwen3-4b-instruct-2507-q4_k_m",
        name: "Qwen3 4B Instruct",
        description: "Fast, good summaries. Works on any Apple silicon Mac.",
        repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
        filename: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        size_bytes: 2_497_281_120,
        sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
        n_ctx: 16_384,
        recommended_min_ram_gb: None,
        is_default: true,
    },
    CatalogEntry {
        id: "qwen3-8b-q4_k_m",
        name: "Qwen3 8B",
        description: "Better action items and owners. Recommended with 16 GB of memory or more.",
        repo: "Qwen/Qwen3-8B-GGUF",
        filename: "Qwen3-8B-Q4_K_M.gguf",
        size_bytes: 5_027_783_488,
        sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
        n_ctx: 16_384,
        recommended_min_ram_gb: Some(16),
        is_default: false,
    },
];

pub fn catalog_entry(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

pub fn default_entry() -> &'static CatalogEntry {
    CATALOG.iter().find(|e| e.is_default).unwrap_or(&CATALOG[0])
}

/// A catalog model with its local state, for the UI.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub recommended_min_ram_gb: Option<u32>,
    pub is_default: bool,
    /// True when this Mac has the recommended memory (or there is no minimum).
    pub fits_this_mac: bool,
    pub downloaded: bool,
    pub downloading: bool,
    /// Bytes of a paused/in-progress download.
    pub partial_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DownloadEvent {
    pub model_id: String,
    pub state: DownloadState,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Started,
    Completed,
    Cancelled,
    Failed,
    Deleted,
}

/// In-flight downloads (process-wide), so any handle can cancel them.
fn in_flight() -> &'static Mutex<HashMap<String, CancellationToken>> {
    static MAP: OnceLock<Mutex<HashMap<String, CancellationToken>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// Total physical memory in GB, when known.
pub fn system_ram_gb() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        let mut size: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let name = c"hw.memsize";
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                &mut size as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0 && size > 0).then_some((size / (1024 * 1024 * 1024)) as u32)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Access to the local LLM model directory. Cheap to create.
pub struct LocalModels {
    app: Option<AppHandle>,
    dir: PathBuf,
}

impl LocalModels {
    pub fn new(app: &AppHandle) -> Result<Self> {
        let dir = crate::portable::app_data_dir(app)
            .map_err(|e| anyhow!("no app data dir: {e}"))?
            .join("models")
            .join("llm");
        Ok(Self {
            app: Some(app.clone()),
            dir,
        })
    }

    /// For tests: a directory without an app (no events).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            app: None,
            dir: dir.into(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn entry(id: &str) -> Result<&'static CatalogEntry> {
        catalog_entry(id).ok_or_else(|| anyhow!("unknown local model '{id}'"))
    }

    pub fn path_for(&self, entry: &CatalogEntry) -> PathBuf {
        self.dir.join(entry.filename)
    }

    fn partial_for(&self, entry: &CatalogEntry) -> PathBuf {
        self.dir.join(format!("{}.partial", entry.filename))
    }

    /// The catalog with download state.
    pub fn list(&self) -> Vec<LocalModel> {
        let ram = system_ram_gb();
        let downloading = in_flight().lock();
        CATALOG
            .iter()
            .map(|e| LocalModel {
                id: e.id.to_string(),
                name: e.name.to_string(),
                description: e.description.to_string(),
                size_bytes: e.size_bytes,
                recommended_min_ram_gb: e.recommended_min_ram_gb,
                is_default: e.is_default,
                fits_this_mac: match (e.recommended_min_ram_gb, ram) {
                    (Some(min), Some(ram)) => ram >= min,
                    _ => true,
                },
                downloaded: self.is_downloaded(e.id),
                downloading: downloading.contains_key(e.id),
                partial_bytes: self.partial_for(e).metadata().map(|m| m.len()).unwrap_or(0),
            })
            .collect()
    }

    /// Downloaded means the final file exists with the catalog size (it is
    /// only renamed into place after its SHA256 checked out).
    pub fn is_downloaded(&self, id: &str) -> bool {
        catalog_entry(id).is_some_and(|e| {
            self.path_for(e)
                .metadata()
                .is_ok_and(|m| m.len() == e.size_bytes)
        })
    }

    pub fn is_downloading(&self, id: &str) -> bool {
        in_flight().lock().contains_key(id)
    }

    /// Path of a downloaded model.
    pub fn model_path(&self, id: &str) -> Result<PathBuf> {
        let entry = Self::entry(id)?;
        if !self.is_downloaded(id) {
            bail!("{} is not downloaded", entry.name);
        }
        Ok(self.path_for(entry))
    }

    /// The model to use when none is chosen: the default if downloaded, else
    /// any downloaded one.
    pub fn first_available(&self) -> Option<&'static CatalogEntry> {
        let default = default_entry();
        if self.is_downloaded(default.id) {
            return Some(default);
        }
        CATALOG.iter().find(|e| self.is_downloaded(e.id))
    }

    fn emit<S: Serialize>(&self, event: &str, payload: S) {
        if let Some(app) = &self.app {
            let _ = app.emit(event, payload);
        }
    }

    fn emit_state(&self, id: &str, state: DownloadState, error: Option<String>) {
        self.emit(
            events::CHANGED,
            DownloadEvent {
                model_id: id.to_string(),
                state,
                error,
            },
        );
    }

    /// Download (or resume) a catalog model. Returns `Ok(true)` when the
    /// model is ready, `Ok(false)` when cancelled. Transient failures are
    /// retried a few times, resuming from the partial file.
    pub async fn download(&self, id: &str) -> Result<bool> {
        let entry = Self::entry(id)?;
        self.download_from(entry, &entry.url()).await
    }

    async fn download_from(&self, entry: &'static CatalogEntry, url: &str) -> Result<bool> {
        if self.is_downloaded(entry.id) {
            return Ok(true);
        }
        let token = {
            let mut map = in_flight().lock();
            if map.contains_key(entry.id) {
                bail!("{} is already downloading", entry.name);
            }
            let token = CancellationToken::new();
            map.insert(entry.id.to_string(), token.clone());
            token
        };
        self.emit_state(entry.id, DownloadState::Started, None);
        let result = self.download_inner(entry, url, &token).await;
        in_flight().lock().remove(entry.id);
        match &result {
            Ok(true) => self.emit_state(entry.id, DownloadState::Completed, None),
            Ok(false) => self.emit_state(entry.id, DownloadState::Cancelled, None),
            Err(e) => self.emit_state(entry.id, DownloadState::Failed, Some(format!("{e:#}"))),
        }
        result
    }

    async fn download_inner(
        &self,
        entry: &'static CatalogEntry,
        url: &str,
        token: &CancellationToken,
    ) -> Result<bool> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("create {}", self.dir.display()))?;
        let partial = self.partial_for(entry);
        let app = self.app.clone();
        let on_progress = move |p: &DownloadProgress| {
            if let Some(app) = &app {
                let _ = app.emit(events::DOWNLOAD_PROGRESS, p);
            }
        };
        const ATTEMPTS: usize = 3;
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            match crate::managers::model::download_file(
                entry.id,
                url,
                &partial,
                Some(entry.size_bytes),
                Some(entry.sha256),
                token,
                &on_progress,
            )
            .await
            {
                Ok(true) => {
                    std::fs::rename(&partial, self.path_for(entry))
                        .context("finalize the downloaded model")?;
                    log::info!("Downloaded local LLM {}", entry.id);
                    return Ok(true);
                }
                Ok(false) => return Ok(false),
                Err(e) => {
                    log::warn!(
                        "Download of {} failed (attempt {attempt}/{ATTEMPTS}): {e:#}",
                        entry.id
                    );
                    last_err = Some(e);
                    if token.is_cancelled() {
                        return Ok(false);
                    }
                    if attempt < ATTEMPTS {
                        tokio::time::sleep(std::time::Duration::from_secs(2 * attempt as u64))
                            .await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("download failed")))
    }

    /// Start a download in the background (UI entry point). Progress and the
    /// outcome arrive as [`events`].
    pub fn start_download(&self, id: &str) -> Result<()> {
        let entry = Self::entry(id)?;
        if self.is_downloading(id) {
            return Ok(());
        }
        let this = Self {
            app: self.app.clone(),
            dir: self.dir.clone(),
        };
        crate::host::async_runtime::spawn(async move {
            if let Err(e) = this.download(entry.id).await {
                log::error!("Local LLM download failed: {e:#}");
            }
        });
        Ok(())
    }

    /// Cancel an in-flight download (the partial file is kept for resume).
    pub fn cancel_download(&self, id: &str) -> bool {
        match in_flight().lock().get(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Delete a downloaded model and any partial download. Unloads it from
    /// the built-in worker first.
    pub fn delete(&self, id: &str) -> Result<()> {
        let entry = Self::entry(id)?;
        self.cancel_download(id);
        let path = self.path_for(entry);
        super::builtin::BuiltinLlm::global().forget_model(&path);
        for p in [path, self.partial_for(entry)] {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("delete {}", p.display())),
            }
        }
        self.emit_state(id, DownloadState::Deleted, None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn catalog_is_well_formed() {
        assert_eq!(CATALOG.iter().filter(|e| e.is_default).count(), 1);
        assert_eq!(default_entry().id, "qwen3-4b-instruct-2507-q4_k_m");
        for e in CATALOG {
            assert_eq!(e.sha256.len(), 64, "{}", e.id);
            assert!(e.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(e.filename.ends_with(".gguf"));
            assert!(e.size_bytes > 1_000_000_000);
            assert!(e.url().starts_with("https://huggingface.co/"));
            assert!(e.n_ctx >= 4096);
        }
        assert_eq!(
            catalog_entry("qwen3-8b-q4_k_m")
                .unwrap()
                .recommended_min_ram_gb,
            Some(16)
        );
    }

    #[test]
    fn list_reports_download_state() {
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        let list = models.list();
        assert_eq!(list.len(), CATALOG.len());
        assert!(list.iter().all(|m| !m.downloaded && !m.downloading));
        assert!(models.first_available().is_none());
        assert!(models.model_path(default_entry().id).is_err());
        // A wrong-size file is not "downloaded".
        std::fs::write(models.path_for(default_entry()), b"short").unwrap();
        assert!(!models.is_downloaded(default_entry().id));
        models.delete(default_entry().id).unwrap();
        assert!(!models.path_for(default_entry()).exists());
    }

    /// A fake catalog entry served from a local socket, through the shared
    /// verified downloader.
    #[tokio::test]
    async fn download_verifies_and_finalizes() {
        let body: &'static [u8] = b"GGUF fake model bytes for the download test";
        let sha: &'static str = Box::leak(format!("{:x}", Sha256::digest(body)).into_boxed_str());
        let entry: &'static CatalogEntry = Box::leak(Box::new(CatalogEntry {
            id: "test-model",
            name: "Test",
            description: "",
            repo: "",
            filename: "test-model.gguf",
            size_bytes: body.len() as u64,
            sha256: sha,
            n_ctx: 4096,
            recommended_min_ram_gb: None,
            is_default: false,
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/m.gguf", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            s.write_all(head.as_bytes()).await.unwrap();
            s.write_all(body).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        assert!(models.download_from(entry, &url).await.unwrap());
        let path = dir.path().join("test-model.gguf");
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert!(!dir.path().join("test-model.gguf.partial").exists());
        assert!(!in_flight().lock().contains_key("test-model"));
    }
}
