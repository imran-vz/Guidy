//! One download manager for every model kind.
//!
//! Speech-to-text models ([`DownloadKind::Stt`]), the built-in LLMs
//! ([`DownloadKind::Llm`]) and the meeting speaker models (speaker,
//! separation, segmentation) all report here under a stable id
//! (`"<kind>:<key>"`, see [`DownloadKind`]). Changes are emitted as
//! [`DOWNLOADS_CHANGED_EVENT`] with the full list, so one indicator (status
//! bar, tray, Models page) shows them all. The per-kind events
//! (`model-download-*`, `llm-models://*`, `meetings://speaker-model-status`)
//! are still sent by each downloader for existing consumers.
//!
//! # Queue
//!
//! [`request`] (or [`DownloadTracker::enqueue`]) puts a download in the
//! queue: at most [`MAX_CONCURRENT`] run at once, the rest wait as
//! [`DownloadState::Queued`]. Before queueing, the free space of the target
//! volume is checked ([`DownloadError::InsufficientSpace`]). The tracker
//! keeps every queued request, so [`retry`] restarts a failed or cancelled
//! download of any kind, resuming its partial file.
//!
//! # Downloader contract
//!
//! A downloader (the code that moves bytes) calls [`DownloadTracker::begin`]
//! (which returns the cancel token it must honor), then
//! [`progress`](DownloadTracker::progress) /
//! [`verifying`](DownloadTracker::verifying), and ends with
//! [`finish`](DownloadTracker::finish), [`fail`](DownloadTracker::fail) or
//! [`cancelled`](DownloadTracker::cancelled). Code that needs a model right
//! away (meeting processing) may call `begin` directly; that bypasses the
//! queue but still shows up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use hf_hub::api::tokio::CancellationToken;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::host::AppHandle;

/// Emitted with a [`Vec<DownloadItem>`] whenever a download changes.
pub const DOWNLOADS_CHANGED_EVENT: &str = "downloads://changed";

/// At or above this size, a download waits for the user to agree.
/// Smaller ones may start on their own and still show up here.
pub const CONSENT_BYTES: u64 = 10 * 1024 * 1024;

/// Downloads that run at the same time; the rest are queued.
pub const MAX_CONCURRENT: usize = 2;

/// Space kept free on the target volume on top of what downloads need.
pub const SPACE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

/// How often a running download pushes a progress event.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// How often the transfer speed is re-measured.
const RATE_EVERY: Duration = Duration::from_secs(1);

/// What a download is. Its id is `"<prefix>:<key>"`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloadKind {
    /// A speech-to-text model; key = the `ModelManager` model id.
    #[default]
    Stt,
    /// A built-in LLM; key = the `llm::local_models` catalog id.
    Llm,
    /// The standard speaker model (ResNet34); key = `resnet34`.
    Speaker,
    /// A speaker model for telling voices apart; key = `SpeakerModelId`.
    Separation,
    /// The pyannote speaker segmentation model; key = `pyannote`.
    Segmentation,
}

impl DownloadKind {
    pub const ALL: [DownloadKind; 5] = [
        Self::Stt,
        Self::Llm,
        Self::Speaker,
        Self::Separation,
        Self::Segmentation,
    ];

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Llm => "llm",
            Self::Speaker => "speaker",
            Self::Separation => "separation",
            Self::Segmentation => "segmentation",
        }
    }

    /// The tracker id of `key`.
    pub fn id(self, key: &str) -> String {
        format!("{}:{key}", self.prefix())
    }

    /// `(kind, key)` of a tracker id.
    pub fn parse(id: &str) -> Option<(Self, &str)> {
        let (prefix, key) = id.split_once(':')?;
        let kind = Self::ALL.into_iter().find(|k| k.prefix() == prefix)?;
        (!key.is_empty()).then_some((kind, key))
    }

    /// Whether a cancelled or failed download of this kind keeps its
    /// partial file and resumes from it.
    pub fn resumable(self) -> bool {
        true
    }
}

/// Tracker id of a speech-to-text model.
pub fn stt_id(model_id: &str) -> String {
    DownloadKind::Stt.id(model_id)
}

/// Tracker id of a built-in LLM.
pub fn llm_id(catalog_id: &str) -> String {
    DownloadKind::Llm.id(catalog_id)
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    /// Waiting for a free slot.
    Queued,
    Downloading,
    /// All bytes are there; checking the SHA-256.
    Verifying,
    Done,
    Failed,
    /// Stopped by the user. A resumable kind keeps its partial file.
    Cancelled,
}

impl DownloadState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Verifying)
    }

    /// Holds a download slot.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Downloading | Self::Verifying)
    }

    /// The status bar stays up for these.
    pub fn needs_attention(self) -> bool {
        self.is_active() || self == Self::Failed
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: DownloadKind,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    /// Smoothed transfer speed; 0 while unknown.
    #[serde(default)]
    pub bytes_per_sec: u64,
    pub state: DownloadState,
    pub error: Option<String>,
    /// Unix milliseconds (queue order).
    pub started_at_ms: i64,
    pub cancellable: bool,
    /// [`retry`] can start it again (the tracker kept its request).
    #[serde(default)]
    pub retryable: bool,
}

impl DownloadItem {
    /// A fresh item in `state` (its kind from the id).
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        total_bytes: u64,
        state: DownloadState,
    ) -> Self {
        let id = id.into();
        let kind = DownloadKind::parse(&id).map(|(k, _)| k).unwrap_or_default();
        Self {
            id,
            name: name.into(),
            kind,
            total_bytes,
            downloaded_bytes: 0,
            bytes_per_sec: 0,
            state,
            error: None,
            started_at_ms: now_ms(),
            cancellable: true,
            retryable: false,
        }
    }

    /// 0..=100, or `None` when the size isn't known yet.
    pub fn percent(&self) -> Option<u32> {
        (self.total_bytes > 0).then(|| {
            ((self.downloaded_bytes as f64 / self.total_bytes as f64) * 100.0)
                .clamp(0.0, 100.0)
                .floor() as u32
        })
    }

    /// 0.0..=1.0, or `None` when the size isn't known yet.
    pub fn fraction(&self) -> Option<f32> {
        (self.total_bytes > 0).then(|| {
            (self.downloaded_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        })
    }

    /// Bytes still to fetch.
    pub fn remaining_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.downloaded_bytes)
    }

    /// Seconds left at the current speed, when both are known.
    pub fn eta_secs(&self) -> Option<u64> {
        (self.state == DownloadState::Downloading && self.bytes_per_sec > 0 && self.total_bytes > 0)
            .then(|| self.remaining_bytes() / self.bytes_per_sec)
    }
}

/// Totals over a download list (the status bar's one number).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DownloadSummary {
    /// Queued, downloading or verifying.
    pub active: usize,
    pub failed: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

impl DownloadSummary {
    pub fn of(items: &[DownloadItem]) -> Self {
        let mut s = Self::default();
        for item in items {
            if item.state.is_active() {
                s.active += 1;
                s.downloaded_bytes += item.downloaded_bytes.min(item.total_bytes);
                s.total_bytes += item.total_bytes;
            } else if item.state == DownloadState::Failed {
                s.failed += 1;
            }
        }
        s
    }

    /// Overall 0..=100 of the active downloads.
    pub fn percent(&self) -> Option<u32> {
        (self.total_bytes > 0).then(|| {
            (self.downloaded_bytes as f64 / self.total_bytes as f64 * 100.0).floor() as u32
        })
    }
}

/// What to download: built by [`resolve`] for a tracker id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub id: String,
    pub name: String,
    pub total_bytes: u64,
    /// Bytes a partial file already has (resume).
    pub partial_bytes: u64,
    /// Where the file lands, for the free-space check.
    pub dest_dir: Option<PathBuf>,
}

impl DownloadRequest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, total_bytes: u64) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            total_bytes,
            partial_bytes: 0,
            dest_dir: None,
        }
    }

    pub fn partial(mut self, bytes: u64) -> Self {
        self.partial_bytes = bytes;
        self
    }

    pub fn dest_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dest_dir = Some(dir.into());
        self
    }

    pub fn remaining_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.partial_bytes)
    }
}

/// Why a download could not be queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The target volume lacks the space (incl. other running downloads and
    /// [`SPACE_MARGIN_BYTES`]).
    InsufficientSpace {
        needed: u64,
        available: u64,
    },
    /// No model has that id.
    Unknown(String),
    /// Already on disk: nothing to download.
    AlreadyInstalled(String),
    Other(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientSpace { needed, available } => write!(
                f,
                "Not enough disk space: the download needs {} and {} is free.",
                format_bytes(*needed),
                format_bytes(*available)
            ),
            Self::Unknown(id) => write!(f, "Unknown model '{id}'"),
            Self::AlreadyInstalled(id) => write!(f, "'{id}' is already downloaded"),
            Self::Other(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for DownloadError {}

/// "1.2 GB", "740 MB" (decimal units, as Finder shows them).
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    }
}

/// Starts a queued download (non-blocking: spawn the work). The default
/// dispatches on the kind ([`start_download`]); tests inject their own.
pub type Starter = dyn Fn(&AppHandle, &DownloadRequest) -> anyhow::Result<()> + Send + Sync;

/// Free bytes on the volume holding a path.
pub type FreeSpace = dyn Fn(&Path) -> Option<u64> + Send + Sync;

struct Slot {
    item: DownloadItem,
    /// What [`retry`] restarts (queued downloads keep it).
    request: Option<DownloadRequest>,
    token: CancellationToken,
    last_emit: Instant,
    rate_at: Instant,
    rate_bytes: u64,
}

impl Slot {
    fn new(item: DownloadItem, request: Option<DownloadRequest>) -> Self {
        Self {
            item,
            request,
            token: CancellationToken::new(),
            last_emit: Instant::now(),
            rate_at: Instant::now(),
            rate_bytes: 0,
        }
    }

    fn set_running(&mut self) {
        if self.token.is_cancelled() {
            self.token = CancellationToken::new();
        }
        self.item.state = DownloadState::Downloading;
        self.item.error = None;
        self.item.bytes_per_sec = 0;
        self.rate_at = Instant::now();
        self.rate_bytes = self.item.downloaded_bytes;
    }
}

/// Process-wide download list and queue. Production uses [`global`]; tests
/// build their own so they don't share state.
pub struct DownloadTracker {
    slots: Mutex<HashMap<String, Slot>>,
    limit: usize,
    starter: Box<Starter>,
    free_space: Box<FreeSpace>,
}

impl DownloadTracker {
    /// A tracker that starts downloads with [`start_download`].
    pub fn new() -> Self {
        Self::with_starter(MAX_CONCURRENT, start_download)
    }

    pub fn with_starter(
        limit: usize,
        starter: impl Fn(&AppHandle, &DownloadRequest) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            limit: limit.max(1),
            starter: Box::new(starter),
            free_space: Box::new(free_space),
        }
    }

    /// Replace the free-space probe (tests).
    pub fn with_free_space(
        mut self,
        f: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static,
    ) -> Self {
        self.free_space = Box::new(f);
        self
    }

    pub fn list(&self) -> Vec<DownloadItem> {
        let mut items: Vec<_> = self.slots.lock().values().map(|s| s.item.clone()).collect();
        items.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.id.cmp(&b.id)));
        items
    }

    pub fn get(&self, id: &str) -> Option<DownloadItem> {
        self.slots.lock().get(id).map(|s| s.item.clone())
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.slots
            .lock()
            .get(id)
            .is_some_and(|s| s.token.is_cancelled() || s.item.state == DownloadState::Cancelled)
    }

    /// Queue a download. No-op when it is already queued or running. Fails
    /// when the target volume can't hold it (with what's already running).
    pub fn enqueue(&self, app: &AppHandle, request: DownloadRequest) -> Result<(), DownloadError> {
        {
            let slots = self.slots.lock();
            if slots
                .get(&request.id)
                .is_some_and(|s| s.item.state.is_active())
            {
                return Ok(());
            }
            if let Some(dir) = &request.dest_dir {
                if let Some(available) = (self.free_space)(dir) {
                    let others: u64 = slots
                        .values()
                        .filter(|s| s.item.state.is_active())
                        .map(|s| s.item.remaining_bytes())
                        .sum();
                    let needed = request.remaining_bytes();
                    if available < needed + others + SPACE_MARGIN_BYTES {
                        return Err(DownloadError::InsufficientSpace { needed, available });
                    }
                }
            }
        }
        let mut item = DownloadItem::new(
            &request.id,
            &request.name,
            request.total_bytes,
            DownloadState::Queued,
        );
        item.downloaded_bytes = request.partial_bytes.min(request.total_bytes);
        item.retryable = true;
        self.slots
            .lock()
            .insert(request.id.clone(), Slot::new(item, Some(request)));
        self.emit(app);
        self.pump(app);
        Ok(())
    }

    /// Queue a failed or cancelled download again (its partial resumes).
    /// `Unknown` when the tracker has no request for it.
    pub fn retry(&self, app: &AppHandle, id: &str) -> Result<(), DownloadError> {
        let request = {
            let slots = self.slots.lock();
            match slots.get(id) {
                Some(s) if s.item.state.is_active() => return Ok(()),
                Some(s) => s.request.clone().map(|mut r| {
                    r.partial_bytes = s.item.downloaded_bytes.min(r.total_bytes);
                    r
                }),
                None => None,
            }
        };
        match request {
            Some(r) => self.enqueue(app, r),
            None => Err(DownloadError::Unknown(id.to_string())),
        }
    }

    /// Start queued downloads while slots are free.
    fn pump(&self, app: &AppHandle) {
        loop {
            let next = {
                let mut slots = self.slots.lock();
                let running = slots.values().filter(|s| s.item.state.is_running()).count();
                if running >= self.limit {
                    return;
                }
                let next = slots
                    .values()
                    .filter(|s| s.item.state == DownloadState::Queued && s.request.is_some())
                    .min_by(|a, b| {
                        a.item
                            .started_at_ms
                            .cmp(&b.item.started_at_ms)
                            .then(a.item.id.cmp(&b.item.id))
                    })
                    .map(|s| s.item.id.clone());
                let Some(id) = next else { return };
                let slot = slots.get_mut(&id).expect("just found");
                slot.set_running();
                slot.request.clone().expect("queued with a request")
            };
            self.emit(app);
            if let Err(e) = (self.starter)(app, &next) {
                log::warn!("Could not start the download of {}: {e:#}", next.id);
                self.settle(app, &next.id, |item| {
                    item.state = DownloadState::Failed;
                    item.error = Some(format!("{e:#}"));
                });
            }
        }
    }

    /// A downloader starts `id` (or takes over its queued slot). Returns the
    /// token it must stop on. A download already running keeps its progress
    /// and returns the same token.
    pub fn begin(
        &self,
        app: &AppHandle,
        id: impl Into<String>,
        name: impl Into<String>,
        total_bytes: u64,
    ) -> CancellationToken {
        let id = id.into();
        let mut slots = self.slots.lock();
        if let Some(slot) = slots.get_mut(&id) {
            if slot.item.state.is_running() && !slot.token.is_cancelled() {
                return slot.token.clone();
            }
            // Queued (taken over), or settled: start again, keeping the
            // request for a later retry.
            slot.set_running();
            slot.item.name = name.into();
            if total_bytes > 0 {
                slot.item.total_bytes = total_bytes;
            }
            if slot.item.downloaded_bytes > slot.item.total_bytes {
                slot.item.downloaded_bytes = 0;
            }
            let token = slot.token.clone();
            drop(slots);
            self.emit(app);
            return token;
        }
        let mut slot = Slot::new(
            DownloadItem::new(&id, name, total_bytes, DownloadState::Downloading),
            None,
        );
        slot.set_running();
        let token = slot.token.clone();
        slots.insert(id, slot);
        drop(slots);
        self.emit(app);
        token
    }

    /// Byte progress. The stored counts update every call; the event is
    /// throttled to [`PROGRESS_EVERY`].
    pub fn progress(&self, app: &AppHandle, id: &str, downloaded: u64, total: u64) {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return;
        };
        if slot.item.state != DownloadState::Downloading || slot.token.is_cancelled() {
            return;
        }
        slot.item.downloaded_bytes = downloaded;
        if total > 0 {
            slot.item.total_bytes = total;
        }
        let elapsed = slot.rate_at.elapsed();
        if elapsed >= RATE_EVERY {
            let bytes = downloaded.saturating_sub(slot.rate_bytes);
            let now = (bytes as f64 / elapsed.as_secs_f64()) as u64;
            slot.item.bytes_per_sec = if slot.item.bytes_per_sec == 0 {
                now
            } else {
                (slot.item.bytes_per_sec as f64 * 0.7 + now as f64 * 0.3) as u64
            };
            slot.rate_at = Instant::now();
            slot.rate_bytes = downloaded;
        }
        let due = slot.last_emit.elapsed() >= PROGRESS_EVERY;
        if due {
            slot.last_emit = Instant::now();
        }
        drop(slots);
        if due {
            self.emit(app);
        }
    }

    /// All bytes are there; the checksum runs.
    pub fn verifying(&self, app: &AppHandle, id: &str) {
        self.settle_active(app, id, false, |item| {
            item.state = DownloadState::Verifying;
            item.downloaded_bytes = item.total_bytes.max(item.downloaded_bytes);
            item.bytes_per_sec = 0;
        });
    }

    pub fn finish(&self, app: &AppHandle, id: &str) {
        self.settle_active(app, id, true, |item| {
            item.downloaded_bytes = item.total_bytes.max(item.downloaded_bytes);
            item.state = DownloadState::Done;
            item.error = None;
            item.bytes_per_sec = 0;
        });
    }

    pub fn fail(&self, app: &AppHandle, id: &str, error: impl Into<String>) {
        let error = error.into();
        self.settle_active(app, id, true, |item| {
            item.state = DownloadState::Failed;
            item.error = Some(error);
            item.bytes_per_sec = 0;
        });
    }

    /// The downloader stopped because it was cancelled (by its own cancel
    /// path or the tracker's token).
    pub fn cancelled(&self, app: &AppHandle, id: &str) {
        self.settle_active(app, id, true, |item| {
            item.state = DownloadState::Cancelled;
            item.error = None;
            item.bytes_per_sec = 0;
        });
    }

    /// Stop a queued or running download (the user asked). Returns false
    /// when it isn't active.
    pub fn cancel(&self, app: &AppHandle, id: &str) -> bool {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return false;
        };
        if !slot.item.state.is_active() {
            return false;
        }
        slot.token.cancel();
        slot.item.state = DownloadState::Cancelled;
        slot.item.error = None;
        slot.item.bytes_per_sec = 0;
        drop(slots);
        self.emit(app);
        self.pump(app);
        true
    }

    /// Forget a finished, failed or cancelled download (clears it from the
    /// list). Returns false when it is active or unknown.
    pub fn dismiss(&self, app: &AppHandle, id: &str) -> bool {
        let mut slots = self.slots.lock();
        if !slots.get(id).is_some_and(|s| !s.item.state.is_active()) {
            return false;
        }
        slots.remove(id);
        drop(slots);
        self.emit(app);
        true
    }

    /// Settle an active item (`pump` after it when it frees a slot).
    fn settle_active(
        &self,
        app: &AppHandle,
        id: &str,
        frees_slot: bool,
        f: impl FnOnce(&mut DownloadItem),
    ) {
        let mut slots = self.slots.lock();
        let Some(slot) = slots.get_mut(id) else {
            return;
        };
        if !slot.item.state.is_active() {
            return;
        }
        f(&mut slot.item);
        drop(slots);
        self.emit(app);
        if frees_slot {
            self.pump(app);
        }
    }

    fn settle(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut DownloadItem)) {
        if let Some(slot) = self.slots.lock().get_mut(id) {
            f(&mut slot.item);
        }
        self.emit(app);
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(DOWNLOADS_CHANGED_EVENT, self.list());
    }
}

impl Default for DownloadTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global() -> &'static DownloadTracker {
    static T: OnceLock<DownloadTracker> = OnceLock::new();
    T.get_or_init(DownloadTracker::new)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The downloader stopped because it was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadCancelled;

impl std::fmt::Display for DownloadCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}

impl std::error::Error for DownloadCancelled {}

// ---------------------------------------------------------------------------
// Disk space
// ---------------------------------------------------------------------------

/// Free bytes (available to this user) on the volume holding `path`, or on
/// its nearest existing ancestor. `None` when it can't be read.
pub fn free_space(path: &Path) -> Option<u64> {
    let mut dir = path;
    while !dir.exists() {
        dir = dir.parent()?;
    }
    statvfs_free(dir)
}

#[cfg(unix)]
fn statvfs_free(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a valid out-pointer.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    (rc == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(unix))]
fn statvfs_free(_dir: &Path) -> Option<u64> {
    None
}

/// Total size of the files under `dir` (0 when it doesn't exist).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// What models take on disk, and what's left.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskUsage {
    /// Downloaded models of every kind (speech-to-text models in the shared
    /// Hugging Face cache included).
    pub models_bytes: u64,
    /// Free on the app data volume, when known.
    pub free_bytes: Option<u64>,
}

/// [`DiskUsage`] of this app. Walks the model directories: call off the UI
/// thread.
pub fn disk_usage(app: &AppHandle) -> DiskUsage {
    let models_dir = crate::portable::app_data_dir(app)
        .ok()
        .map(|d| d.join("models"));
    let stt = app
        .try_state::<Arc<crate::managers::model::ModelManager>>()
        .map(|mm| mm.downloaded_bytes())
        .unwrap_or(0);
    let other = models_dir
        .as_ref()
        .map(|d| dir_size(&d.join("llm")) + dir_size(&d.join("speaker")))
        .unwrap_or(0);
    DiskUsage {
        models_bytes: stt + other,
        free_bytes: models_dir.as_deref().and_then(free_space),
    }
}

// ---------------------------------------------------------------------------
// Every model kind: catalog, requests, start, delete
// ---------------------------------------------------------------------------

/// A model that isn't speech-to-text (those come from `ModelManager`), with
/// its state on disk: the built-in LLMs and the meeting speaker models.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OtherModel {
    /// Tracker id (`llm:…`, `speaker:resnet34`, `separation:…`,
    /// `segmentation:pyannote`).
    pub id: String,
    pub kind: DownloadKind,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub installed: bool,
    /// Bytes of a paused (resumable) download.
    pub partial_bytes: u64,
    /// The default of its kind (Qwen3 4B, ResNet34).
    pub is_default: bool,
    /// RAM it is recommended for (LLMs).
    pub recommended_min_ram_gb: Option<u32>,
    /// This Mac has that RAM (or there is no minimum).
    pub fits_this_mac: bool,
}

/// The LLMs and speaker models with their state, LLMs first, smallest first
/// within a kind.
pub fn other_models(app: &AppHandle) -> Vec<OtherModel> {
    use crate::meetings::processing::speaker_model as sm;
    use crate::meetings::types::SpeakerModelId;
    let mut out = Vec::new();
    if let Ok(llms) = crate::llm::local_models::LocalModels::new(app) {
        out.extend(llms.list().into_iter().map(|m| OtherModel {
            id: llm_id(&m.id),
            kind: DownloadKind::Llm,
            name: m.name,
            description: m.description,
            size_bytes: m.size_bytes,
            installed: m.downloaded,
            partial_bytes: m.partial_bytes,
            is_default: m.is_default,
            recommended_min_ram_gb: m.recommended_min_ram_gb,
            fits_this_mac: m.fits_this_mac,
        }));
    }
    let partial = |path: anyhow::Result<PathBuf>| {
        path.ok()
            .and_then(|p| sm::partial_path(&p).metadata().ok())
            .map(|m| m.len())
            .unwrap_or(0)
    };
    for m in sm::SEPARATION_MODELS {
        let standard = m.id == SpeakerModelId::Resnet34;
        out.push(OtherModel {
            id: if standard {
                sm::speaker_download_id().to_string()
            } else {
                sm::separation_download_id(m.id)
            },
            kind: if standard {
                DownloadKind::Speaker
            } else {
                DownloadKind::Separation
            },
            name: m.name.to_string(),
            description: String::new(),
            size_bytes: m.size,
            installed: if standard {
                sm::speaker_model_installed(app)
            } else {
                sm::separation_model_installed(app, m.id)
            },
            partial_bytes: partial(sm::separation_model_path(app, m.id)),
            is_default: standard,
            recommended_min_ram_gb: None,
            fits_this_mac: true,
        });
    }
    out.push(OtherModel {
        id: sm::segmentation_download_id().to_string(),
        kind: DownloadKind::Segmentation,
        name: sm::SEGMENTATION_MODEL_NAME.to_string(),
        description: String::new(),
        size_bytes: sm::SEGMENTATION_MODEL_SIZE,
        installed: sm::installed_segmentation_model(app).is_some(),
        partial_bytes: partial(sm::segmentation_model_path(app)),
        is_default: false,
        recommended_min_ram_gb: None,
        fits_this_mac: true,
    });
    out
}

/// The request for a tracker id: its name, size, partial bytes and target
/// directory.
pub fn resolve(app: &AppHandle, id: &str) -> Result<DownloadRequest, DownloadError> {
    use crate::meetings::processing::speaker_model as sm;
    let unknown = || DownloadError::Unknown(id.to_string());
    let (kind, key) = DownloadKind::parse(id).ok_or_else(unknown)?;
    let installed = || DownloadError::AlreadyInstalled(id.to_string());
    match kind {
        DownloadKind::Stt => {
            let mm = app
                .try_state::<Arc<crate::managers::model::ModelManager>>()
                .ok_or_else(|| DownloadError::Other("Model manager not ready".into()))?;
            let info = mm.get_model_info(key).ok_or_else(unknown)?;
            if info.is_downloaded {
                return Err(installed());
            }
            let total = info.size_mb * 1024 * 1024;
            Ok(DownloadRequest::new(id, info.name.clone(), total)
                .partial(info.partial_size.min(total))
                .dest_dir(mm.download_dir(&info)))
        }
        DownloadKind::Llm => {
            let entry = crate::llm::local_models::catalog_entry(key).ok_or_else(unknown)?;
            let models = crate::llm::local_models::LocalModels::new(app)
                .map_err(|e| DownloadError::Other(format!("{e:#}")))?;
            if models.is_downloaded(key) {
                return Err(installed());
            }
            let partial = models
                .list()
                .into_iter()
                .find(|m| m.id == key)
                .map(|m| m.partial_bytes)
                .unwrap_or(0);
            Ok(DownloadRequest::new(id, entry.name, entry.size_bytes)
                .partial(partial)
                .dest_dir(models.dir()))
        }
        DownloadKind::Speaker | DownloadKind::Separation | DownloadKind::Segmentation => {
            let file = sm::download_file_for(id).ok_or_else(unknown)?;
            let dest = file
                .path(app)
                .map_err(|e| DownloadError::Other(format!("{e:#}")))?;
            if file.installed(&dest) {
                return Err(installed());
            }
            let partial = sm::partial_path(&dest)
                .metadata()
                .map(|m| m.len())
                .unwrap_or(0);
            let dir = dest.parent().map(Path::to_path_buf);
            let mut r = DownloadRequest::new(id, file.name, file.size).partial(partial);
            r.dest_dir = dir;
            Ok(r)
        }
    }
}

/// Queue the download of a tracker id (any kind). The UI entry point.
pub fn request(app: &AppHandle, id: &str) -> Result<(), DownloadError> {
    let request = resolve(app, id)?;
    global().enqueue(app, request)
}

/// Start a failed or cancelled download again (resuming its partial), or
/// queue it fresh when the tracker doesn't know it (e.g. after a restart).
pub fn retry(app: &AppHandle, id: &str) -> Result<(), DownloadError> {
    match global().retry(app, id) {
        Err(DownloadError::Unknown(_)) => request(app, id),
        other => other,
    }
}

/// Stop a download of any kind. Returns false when nothing was running.
pub fn cancel(app: &AppHandle, id: &str) -> bool {
    let stopped = global().cancel(app, id);
    // A download started before the tracker knew it (or by an old code
    // path) still stops through its own manager.
    match DownloadKind::parse(id) {
        Some((DownloadKind::Stt, key)) => {
            if let Some(mm) = app.try_state::<Arc<crate::managers::model::ModelManager>>() {
                if mm.is_downloading(key) {
                    let _ = mm.cancel_download(key);
                    return true;
                }
            }
        }
        Some((DownloadKind::Llm, key)) => {
            if let Ok(models) = crate::llm::local_models::LocalModels::new(app) {
                return models.cancel_download(key) || stopped;
            }
        }
        _ => {}
    }
    stopped
}

/// Delete a downloaded LLM or speaker model (and any partial). A running
/// download of it is cancelled first. Speech-to-text models are deleted
/// through `commands::models::delete_model` (it unloads the active one).
/// Blocking.
pub fn delete(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    use crate::meetings::processing::speaker_model as sm;
    let (kind, key) =
        DownloadKind::parse(id).ok_or_else(|| anyhow::anyhow!("unknown model '{id}'"))?;
    global().cancel(app, id);
    match kind {
        DownloadKind::Stt => {
            anyhow::bail!("speech-to-text models are deleted by the model manager")
        }
        DownloadKind::Llm => crate::llm::local_models::LocalModels::new(app)?.delete(key)?,
        DownloadKind::Speaker => sm::delete_speaker_model(app)?,
        DownloadKind::Separation => {
            sm::delete_separation_model(app, crate::meetings::types::SpeakerModelId::parse(key))?
        }
        DownloadKind::Segmentation => sm::delete_segmentation_model(app)?,
    }
    global().dismiss(app, id);
    Ok(())
}

/// The default [`Starter`]: spawn the kind's downloader. The downloader
/// reports to [`global`]; whatever it leaves active when it returns
/// (already on disk, early error) is settled here.
pub fn start_download(app: &AppHandle, request: &DownloadRequest) -> anyhow::Result<()> {
    use crate::meetings::processing::speaker_model as sm;
    let id = request.id.clone();
    let (kind, key) =
        DownloadKind::parse(&id).ok_or_else(|| anyhow::anyhow!("unknown download {id}"))?;
    let key = key.to_string();
    let app = app.clone();
    match kind {
        DownloadKind::Stt => {
            let mm = app
                .try_state::<Arc<crate::managers::model::ModelManager>>()
                .ok_or_else(|| anyhow::anyhow!("model manager not ready"))?
                .inner()
                .clone();
            if mm.is_downloading(&key) {
                // Already running (it reports to the tracker itself).
                return Ok(());
            }
            crate::host::async_runtime::spawn(async move {
                let result = mm.download_model(&key).await;
                if let Err(e) = &result {
                    log::error!("Model download failed for {key}: {e:#}");
                    let _ = app.emit(
                        "model-download-failed",
                        serde_json::json!({ "model_id": &key, "error": format!("{e:#}") }),
                    );
                }
                let done = mm.get_model_info(&key).is_some_and(|m| m.is_downloaded);
                settle_after(&app, &id, result.map(|()| done));
            });
        }
        DownloadKind::Llm => {
            let models = crate::llm::local_models::LocalModels::new(&app)?;
            crate::host::async_runtime::spawn(async move {
                let result = models.download(&key).await;
                settle_after(&app, &id, result);
            });
        }
        DownloadKind::Speaker | DownloadKind::Separation | DownloadKind::Segmentation => {
            let file = sm::download_file_for(&id)
                .ok_or_else(|| anyhow::anyhow!("unknown download {id}"))?;
            std::thread::Builder::new()
                .name("model-download".into())
                .spawn(move || {
                    let result = sm::ensure_download(&app, &file);
                    let result = match result {
                        Ok(_) => Ok(true),
                        Err(e) if e.is::<DownloadCancelled>() => Ok(false),
                        Err(e) => Err(e),
                    };
                    settle_after(&app, &id, result);
                })?;
        }
    }
    Ok(())
}

/// Settle `id` from its downloader's result unless the downloader already
/// did: `Ok(true)` done, `Ok(false)` cancelled.
fn settle_after(app: &AppHandle, id: &str, result: anyhow::Result<bool>) {
    let tracker = global();
    match result {
        Ok(true) => tracker.finish(app, id),
        Ok(false) => tracker.cancelled(app, id),
        Err(e) => tracker.fail(app, id, format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing::TestApp;

    /// A tracker whose starter only records what it was asked to start.
    fn recording(limit: usize) -> (DownloadTracker, Arc<Mutex<Vec<String>>>) {
        let started = Arc::new(Mutex::new(Vec::new()));
        let log = started.clone();
        let t = DownloadTracker::with_starter(limit, move |_, r| {
            log.lock().push(r.id.clone());
            Ok(())
        });
        (t, started)
    }

    fn req(id: &str, total: u64) -> DownloadRequest {
        DownloadRequest::new(id, id.to_uppercase(), total)
    }

    #[test]
    fn ids_round_trip() {
        assert_eq!(stt_id("org/m/x.gguf"), "stt:org/m/x.gguf");
        assert_eq!(
            DownloadKind::parse("stt:org/m/x.gguf"),
            Some((DownloadKind::Stt, "org/m/x.gguf"))
        );
        assert_eq!(
            DownloadKind::parse("separation:titanet_large"),
            Some((DownloadKind::Separation, "titanet_large"))
        );
        assert_eq!(DownloadKind::parse("nope:x"), None);
        assert_eq!(DownloadKind::parse("llm:"), None);
        let item = DownloadItem::new("llm:q", "Q", 10, DownloadState::Queued);
        assert_eq!(item.kind, DownloadKind::Llm);
    }

    #[test]
    fn progress_finish_and_fail_are_recorded_and_emitted() {
        let app = TestApp::new();
        let (t, _) = recording(2);
        let cancel = t.begin(&app.app, "speaker:resnet34", "ResNet34", 1000);
        assert!(!cancel.is_cancelled());
        let item = t.get("speaker:resnet34").unwrap();
        assert_eq!(item.state, DownloadState::Downloading);
        assert_eq!(item.kind, DownloadKind::Speaker);
        assert_eq!(item.percent(), Some(0));

        t.progress(&app.app, "speaker:resnet34", 450, 1000);
        assert_eq!(t.get("speaker:resnet34").unwrap().percent(), Some(45));

        t.verifying(&app.app, "speaker:resnet34");
        assert_eq!(
            t.get("speaker:resnet34").unwrap().state,
            DownloadState::Verifying
        );
        t.finish(&app.app, "speaker:resnet34");
        let done = t.get("speaker:resnet34").unwrap();
        assert_eq!(done.state, DownloadState::Done);
        assert_eq!(done.percent(), Some(100));

        t.begin(&app.app, "segmentation:pyannote", "Segmentation", 100);
        t.fail(&app.app, "segmentation:pyannote", "network");
        let failed = t.get("segmentation:pyannote").unwrap();
        assert_eq!(failed.state, DownloadState::Failed);
        assert_eq!(failed.error.as_deref(), Some("network"));

        let last = app
            .events
            .lock()
            .iter()
            .rev()
            .find(|e| e.name == DOWNLOADS_CHANGED_EVENT)
            .unwrap()
            .payload
            .clone();
        let items: Vec<DownloadItem> = serde_json::from_value(last).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn cancel_stops_a_running_download_and_ignores_a_later_finish() {
        let app = TestApp::new();
        let (t, _) = recording(2);
        let token = t.begin(&app.app, "speaker:resnet34", "ResNet34", 100);
        assert!(t.cancel(&app.app, "speaker:resnet34"));
        assert!(token.is_cancelled());
        assert!(t.is_cancelled("speaker:resnet34"));
        t.finish(&app.app, "speaker:resnet34");
        t.fail(&app.app, "speaker:resnet34", "late");
        assert_eq!(
            t.get("speaker:resnet34").unwrap().state,
            DownloadState::Cancelled
        );
        assert!(!t.cancel(&app.app, "missing"));
    }

    #[test]
    fn a_second_begin_while_running_does_not_reset_progress() {
        let app = TestApp::new();
        let (t, _) = recording(2);
        let first = t.begin(&app.app, "llm:a", "A", 100);
        t.progress(&app.app, "llm:a", 40, 100);
        let second = t.begin(&app.app, "llm:a", "A", 100);
        second.cancel();
        assert!(first.is_cancelled(), "the same token");
        assert_eq!(t.get("llm:a").unwrap().downloaded_bytes, 40);
    }

    #[test]
    fn a_failed_download_can_be_started_again() {
        let app = TestApp::new();
        let (t, _) = recording(2);
        t.begin(&app.app, "llm:a", "A", 10);
        t.fail(&app.app, "llm:a", "nope");
        let token = t.begin(&app.app, "llm:a", "A again", 10);
        assert!(!token.is_cancelled());
        let item = t.get("llm:a").unwrap();
        assert_eq!(item.state, DownloadState::Downloading);
        assert_eq!(item.name, "A again");
        assert!(item.error.is_none());
    }

    #[test]
    fn the_queue_runs_at_most_the_limit_and_starts_the_next_when_one_ends() {
        let app = TestApp::new();
        let (t, started) = recording(2);
        for id in ["stt:a", "stt:b", "llm:c", "speaker:resnet34"] {
            t.enqueue(&app.app, req(id, 100)).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(*started.lock(), ["stt:a", "stt:b"]);
        assert_eq!(t.get("llm:c").unwrap().state, DownloadState::Queued);
        // The downloader takes over its slot; queueing again is a no-op.
        t.begin(&app.app, "stt:a", "A", 100);
        t.enqueue(&app.app, req("stt:a", 100)).unwrap();
        assert_eq!(started.lock().len(), 2);

        t.finish(&app.app, "stt:a");
        assert_eq!(*started.lock(), ["stt:a", "stt:b", "llm:c"]);
        // Cancelling a queued one never starts it; a running one frees a slot.
        assert!(t.cancel(&app.app, "speaker:resnet34"));
        t.cancel(&app.app, "stt:b");
        assert_eq!(started.lock().len(), 3);
        assert_eq!(
            t.get("speaker:resnet34").unwrap().state,
            DownloadState::Cancelled
        );
        let summary = DownloadSummary::of(&t.list());
        assert_eq!(summary.active, 1);
    }

    #[test]
    fn retry_requeues_with_the_partial_and_unknown_ids_say_so() {
        let app = TestApp::new();
        let (t, started) = recording(1);
        t.enqueue(&app.app, req("llm:big", 1000)).unwrap();
        t.begin(&app.app, "llm:big", "Big", 1000);
        t.progress(&app.app, "llm:big", 300, 1000);
        t.fail(&app.app, "llm:big", "reset");
        assert!(t.get("llm:big").unwrap().retryable);
        t.retry(&app.app, "llm:big").unwrap();
        assert_eq!(started.lock().len(), 2);
        let item = t.get("llm:big").unwrap();
        assert_eq!(item.state, DownloadState::Downloading);
        assert_eq!(item.downloaded_bytes, 300, "resumes from the partial");
        assert_eq!(
            t.retry(&app.app, "llm:other"),
            Err(DownloadError::Unknown("llm:other".into()))
        );
    }

    #[test]
    fn a_starter_error_fails_the_item_and_the_queue_moves_on() {
        let app = TestApp::new();
        let t = DownloadTracker::with_starter(1, |_, r| {
            if r.id == "stt:bad" {
                anyhow::bail!("no such model")
            }
            Ok(())
        });
        t.enqueue(&app.app, req("stt:bad", 10)).unwrap();
        t.enqueue(&app.app, req("stt:good", 10)).unwrap();
        let bad = t.get("stt:bad").unwrap();
        assert_eq!(bad.state, DownloadState::Failed);
        assert_eq!(bad.error.as_deref(), Some("no such model"));
        assert_eq!(t.get("stt:good").unwrap().state, DownloadState::Downloading);
    }

    #[test]
    fn a_download_that_does_not_fit_is_refused() {
        let app = TestApp::new();
        let gb = 1_000_000_000;
        let (t, started) = recording(2);
        let t = t.with_free_space(move |_| Some(3 * gb + gb / 5));
        let big = |id: &str, total| req(id, total).dest_dir("/tmp");
        t.enqueue(&app.app, big("llm:a", 2 * gb)).unwrap();
        // 2 GB running + 2 GB + margin > 3.2 GB free.
        let err = t.enqueue(&app.app, big("llm:b", 2 * gb)).unwrap_err();
        assert!(matches!(err, DownloadError::InsufficientSpace { needed, .. } if needed == 2 * gb));
        assert!(err.to_string().contains("2.0 GB"), "{err}");
        // A resumed download only needs what's left.
        t.enqueue(&app.app, big("llm:c", 2 * gb).partial(gb + gb / 2))
            .unwrap();
        assert_eq!(started.lock().len(), 2);
    }

    #[test]
    fn summary_and_eta() {
        let mut a = DownloadItem::new("stt:a", "A", 1000, DownloadState::Downloading);
        a.downloaded_bytes = 250;
        a.bytes_per_sec = 75;
        let mut b = DownloadItem::new("llm:b", "B", 1000, DownloadState::Queued);
        b.downloaded_bytes = 0;
        let c = DownloadItem::new("speaker:resnet34", "C", 10, DownloadState::Failed);
        let s = DownloadSummary::of(&[a.clone(), b, c]);
        assert_eq!((s.active, s.failed), (2, 1));
        assert_eq!(s.percent(), Some(12));
        assert_eq!(a.eta_secs(), Some(10));
        assert_eq!(format_bytes(2_497_281_120), "2.5 GB");
        assert_eq!(format_bytes(26_530_550), "27 MB");
    }

    #[test]
    fn dismiss_only_settled_items() {
        let app = TestApp::new();
        let (t, _) = recording(2);
        t.begin(&app.app, "llm:a", "A", 10);
        assert!(!t.dismiss(&app.app, "llm:a"));
        t.finish(&app.app, "llm:a");
        assert!(t.dismiss(&app.app, "llm:a"));
        assert!(t.get("llm:a").is_none());
    }

    #[test]
    fn free_space_reads_the_nearest_existing_ancestor() {
        let dir = std::env::temp_dir();
        let free = free_space(&dir.join("does/not/exist/yet"));
        if cfg!(unix) {
            assert!(free.is_some_and(|b| b > 0));
        }
    }
}
