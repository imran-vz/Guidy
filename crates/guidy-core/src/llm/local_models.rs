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
use parking_lot::{Condvar, Mutex};
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
    /// Commit the file is downloaded from (40 hex digits). Pinned, unlike
    /// `main`, so an upstream re-upload can't break the size and SHA256
    /// checks below.
    pub revision: &'static str,
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
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.filename
        )
    }
}

/// Default first. Sizes and SHA256s are the Hugging Face LFS values of the
/// file at `revision`.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "qwen3-4b-instruct-2507-q4_k_m",
        name: "Qwen3 4B Instruct",
        description: "Fast, good summaries. Works on any Apple silicon Mac.",
        repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
        revision: "a06e946bb6b655725eafa393f4a9745d460374c9",
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
        revision: "7c41481f57cb95916b40956ab2f0b139b296d974",
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

/// An in-flight download.
struct InFlight {
    token: CancellationToken,
    /// Deleted while still running (the delete stopped waiting for it): its
    /// end removes the partial it may have written since, instead of keeping
    /// it for resume.
    deleted: bool,
}

impl InFlight {
    fn new(token: CancellationToken) -> Self {
        Self {
            token,
            deleted: false,
        }
    }
}

/// In-flight downloads (process-wide), so any handle can cancel them.
fn in_flight() -> &'static Mutex<HashMap<String, InFlight>> {
    static MAP: OnceLock<Mutex<HashMap<String, InFlight>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// Signalled (with the [`in_flight`] lock) whenever a download ends, for a
/// delete waiting on it.
static DOWNLOAD_ENDED: Condvar = Condvar::new();

/// Serializes a download's finalization (cancel check, rename, outcome
/// event) against [`LocalModels::delete`], so a delete can't be undone by a
/// download that was already verifying when it ran.
static FINALIZE: Mutex<()> = Mutex::new(());

/// How long a delete waits for a cancelled download to wind down (it may be
/// mid-SHA256 over several GB) before removing the files regardless.
const DELETE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

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
            // The download tracker's token, so cancelling there (status bar,
            // Models page) stops this transfer too.
            let token = match &self.app {
                Some(app) => crate::downloads::global().begin(
                    app,
                    crate::downloads::llm_id(entry.id),
                    entry.name,
                    entry.size_bytes,
                ),
                None => CancellationToken::new(),
            };
            map.insert(entry.id.to_string(), InFlight::new(token.clone()));
            token
        };
        self.emit_state(entry.id, DownloadState::Started, None);
        let verified = self.download_inner(entry, url, &token).await;
        let result = self.finish(entry, &token, verified);
        if let Some(app) = &self.app {
            let tracker = crate::downloads::global();
            let id = crate::downloads::llm_id(entry.id);
            match &result {
                Ok(true) => tracker.finish(app, &id),
                Ok(false) => tracker.cancelled(app, &id),
                Err(e) => tracker.fail(app, &id, format!("{e:#}")),
            }
        }
        result
    }

    /// Move a verified download into place unless it was cancelled (or
    /// deleted) meanwhile, then report the outcome. A download deleted while
    /// it ran removes its partial. Runs under [`FINALIZE`].
    fn finish(
        &self,
        entry: &'static CatalogEntry,
        token: &CancellationToken,
        verified: Result<bool>,
    ) -> Result<bool> {
        let _finalize = FINALIZE.lock();
        let result = match verified {
            Ok(true) if token.is_cancelled() => Ok(false),
            Ok(true) => std::fs::rename(self.partial_for(entry), self.path_for(entry))
                .context("finalize the downloaded model")
                .map(|()| {
                    log::info!("Downloaded local LLM {}", entry.id);
                    true
                }),
            other => other,
        };
        let deleted = {
            let mut map = in_flight().lock();
            let deleted = map.remove(entry.id).is_some_and(|d| d.deleted);
            DOWNLOAD_ENDED.notify_all();
            deleted
        };
        if deleted {
            let partial = self.partial_for(entry);
            match std::fs::remove_file(&partial) {
                Ok(()) => log::info!("Removed the partial of deleted model {}", entry.id),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("Could not remove {}: {e}", partial.display()),
            }
        }
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
        let tracker_id = crate::downloads::llm_id(entry.id);
        let on_progress = {
            let app = app.clone();
            let tracker_id = tracker_id.clone();
            move |p: &DownloadProgress| {
                if let Some(app) = &app {
                    crate::downloads::global().progress(app, &tracker_id, p.downloaded, p.total);
                    let _ = app.emit(events::DOWNLOAD_PROGRESS, p);
                }
            }
        };
        let on_verifying = move || {
            if let Some(app) = &app {
                crate::downloads::global().verifying(app, &tracker_id);
            }
        };
        const ATTEMPTS: usize = 3;
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            // Cancelled (or deleted) during the backoff: don't resume.
            if token.is_cancelled() {
                return Ok(false);
            }
            match crate::managers::model::download_file(
                entry.id,
                url,
                &partial,
                Some(entry.size_bytes),
                Some(entry.sha256),
                token,
                &on_progress,
                &on_verifying,
            )
            .await
            {
                // Verified in `partial`; `finish` renames it.
                Ok(done) => return Ok(done),
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

    /// Start a download in the background. Progress and the outcome arrive
    /// as [`events`] and in the download tracker. With an app it goes
    /// through the tracker's queue (and its free-space check).
    pub fn start_download(&self, id: &str) -> Result<()> {
        let entry = Self::entry(id)?;
        if self.is_downloading(id) {
            return Ok(());
        }
        if let Some(app) = &self.app {
            return match crate::downloads::request(app, &crate::downloads::llm_id(entry.id)) {
                Ok(()) | Err(crate::downloads::DownloadError::AlreadyInstalled(_)) => Ok(()),
                Err(e) => Err(anyhow!(e)),
            };
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
    /// Its token is the download tracker's, so the tracker sees it stop.
    pub fn cancel_download(&self, id: &str) -> bool {
        // A queued download isn't in flight yet: only the tracker has it.
        let queued = self.app.as_ref().is_some_and(|app| {
            crate::downloads::global().cancel(app, &crate::downloads::llm_id(id))
        });
        match in_flight().lock().get(id) {
            Some(download) => {
                download.token.cancel();
                true
            }
            None => queued,
        }
    }

    /// Delete a downloaded model and any partial download. Cancels an
    /// in-flight download and waits (bounded) for it to finish, and unloads
    /// the model from the built-in worker first. Blocks.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.delete_entry(Self::entry(id)?, DELETE_WAIT)
    }

    fn delete_entry(&self, entry: &'static CatalogEntry, wait: std::time::Duration) -> Result<()> {
        let id = entry.id;
        {
            let mut map = in_flight().lock();
            if let Some(download) = map.get(id) {
                download.token.cancel();
                DOWNLOAD_ENDED.wait_while_for(&mut map, |map| map.contains_key(id), wait);
                if let Some(download) = map.get_mut(id) {
                    // Still stuck (e.g. on the network). Its finish sees the
                    // cancelled token under FINALIZE and won't rename, and
                    // removes any partial it wrote after the files below.
                    log::warn!("Deleting {id} while its cancelled download is still running");
                    download.deleted = true;
                }
            }
        }
        let path = self.path_for(entry);
        super::builtin::BuiltinLlm::global().forget_model(&path);
        let _finalize = FINALIZE.lock();
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
            assert_eq!(e.revision.len(), 40, "{}", e.id);
            assert!(e.revision.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(
                e.url().contains(&format!("/resolve/{}/", e.revision)),
                "{}",
                e.url()
            );
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
            revision: "",
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

    fn fake_entry(id: &'static str, body: &[u8]) -> &'static CatalogEntry {
        let sha: &'static str = Box::leak(format!("{:x}", Sha256::digest(body)).into_boxed_str());
        Box::leak(Box::new(CatalogEntry {
            id,
            name: "Test",
            description: "",
            repo: "",
            revision: "",
            filename: Box::leak(format!("{id}.gguf").into_boxed_str()),
            size_bytes: body.len() as u64,
            sha256: sha,
            n_ctx: 4096,
            recommended_min_ram_gb: None,
            is_default: false,
        }))
    }

    /// A delete landing while a download is past verification (about to
    /// rename) waits for it, and the download never renames afterwards.
    #[test]
    fn delete_waits_for_a_finishing_download_and_wins() {
        let body = b"verified fake model bytes";
        let entry = fake_entry("test-model-delete-race", body);
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        // The download has verified the partial and is about to finish.
        std::fs::write(models.partial_for(entry), body).unwrap();
        let token = CancellationToken::new();
        in_flight()
            .lock()
            .insert(entry.id.to_string(), InFlight::new(token.clone()));

        let deleter = {
            let dir = dir.path().to_path_buf();
            std::thread::spawn(move || {
                LocalModels::at(dir).delete_entry(entry, std::time::Duration::from_secs(10))
            })
        };
        while !token.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Delete is waiting on the download, not racing past it.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!deleter.is_finished());
        assert!(models.partial_for(entry).exists());

        // The download finishes: cancelled, so no rename (and no Completed).
        assert!(!models.finish(entry, &token, Ok(true)).unwrap());
        deleter.join().unwrap().unwrap();
        assert!(!models.path_for(entry).exists());
        assert!(!models.partial_for(entry).exists());
        assert!(!models.is_downloading(entry.id));
    }

    /// A download that finished first is simply deleted afterwards.
    #[test]
    fn delete_after_a_finished_download_removes_it() {
        let body = b"verified fake model bytes";
        let entry = fake_entry("test-model-delete-after", body);
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        std::fs::write(models.partial_for(entry), body).unwrap();
        let token = CancellationToken::new();
        in_flight()
            .lock()
            .insert(entry.id.to_string(), InFlight::new(token.clone()));
        assert!(models.finish(entry, &token, Ok(true)).unwrap());
        assert!(models.path_for(entry).exists());
        models
            .delete_entry(entry, std::time::Duration::from_secs(10))
            .unwrap();
        assert!(!models.path_for(entry).exists());
        assert!(!token.is_cancelled());
    }

    /// A delete that gives up waiting on a stuck download removes the files
    /// and marks it deleted, so the partial the download writes before it
    /// ends is removed too (not kept for resume).
    #[test]
    fn delete_timing_out_on_a_stuck_download_removes_its_late_partial() {
        let body = b"verified fake model bytes";
        let entry = fake_entry("test-model-delete-stuck", body);
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::at(dir.path());
        std::fs::write(models.partial_for(entry), &body[..4]).unwrap();
        let token = CancellationToken::new();
        in_flight()
            .lock()
            .insert(entry.id.to_string(), InFlight::new(token.clone()));

        models
            .delete_entry(entry, std::time::Duration::from_millis(50))
            .unwrap();
        assert!(token.is_cancelled());
        assert!(!models.partial_for(entry).exists());
        assert!(in_flight().lock()[entry.id].deleted);

        // The cancelled download writes once more, then ends.
        std::fs::write(models.partial_for(entry), &body[..8]).unwrap();
        assert!(!models.finish(entry, &token, Ok(false)).unwrap());
        assert!(!models.partial_for(entry).exists());
        assert!(!models.path_for(entry).exists());
        assert!(!models.is_downloading(entry.id));
    }

    /// A delete during a real (fake-served) transfer cancels it and is woken
    /// when it ends; the partial is gone and nothing is left in flight.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_cancels_a_running_download_and_waits_for_it() {
        let body: &'static [u8] = b"GGUF fake model bytes for the delete-while-downloading test";
        let entry = fake_entry("test-model-delete-running", body);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/m.gguf", listener.local_addr().unwrap());
        let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            s.write_all(head.as_bytes()).await.unwrap();
            // Half the body, then hold the connection open.
            s.write_all(&body[..body.len() / 2]).await.unwrap();
            s.flush().await.unwrap();
            let _ = sent_tx.send(());
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            drop(s);
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let download =
            tokio::spawn(async move { LocalModels::at(path).download_from(entry, &url).await });
        sent_rx.await.unwrap();
        let models = LocalModels::at(dir.path());
        while !models.partial_for(entry).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let path = dir.path().to_path_buf();
        tokio::task::spawn_blocking(move || {
            LocalModels::at(path).delete_entry(entry, std::time::Duration::from_secs(10))
        })
        .await
        .unwrap()
        .unwrap();
        // Woken by the download's end, not by the timeout.
        assert!(!models.is_downloading(entry.id));
        assert!(!download.await.unwrap().unwrap());
        assert!(!models.partial_for(entry).exists());
        assert!(!models.path_for(entry).exists());
    }
}
