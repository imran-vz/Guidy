//! The sherpa-onnx speaker-embedding model: on-demand download (SHA-256
//! verified) into `<app_data_dir>/models/speaker/`, and the extractor.
//!
//! Model: WeSpeaker ResNet34-LM trained on VoxCeleb (English, 16 kHz,
//! 256-dim embeddings, 26.5 MB), from sherpa-onnx's
//! `speaker-recongition-models` release. It is a better fit for English Meet
//! audio than the 3D-Speaker (zh-cn) model.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::SAMPLE_RATE;
use crate::host::AppHandle;
use crate::meetings::types::SpeakerModelId;

/// Display name and training data, shown next to the download button.
pub const SPEAKER_MODEL_NAME: &str = "WeSpeaker ResNet34";
pub const SPEAKER_MODEL_TRAINED_ON: &str = "VoxCeleb, English";
pub const SPEAKER_MODEL_FILE: &str = "wespeaker_en_voxceleb_resnet34_LM.onnx";
pub const SPEAKER_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx";
pub const SPEAKER_MODEL_SHA256: &str =
    "e9848563da86f263117134dfd7ad63c92355b37de492b55e325400c9d9c39012";
pub const SPEAKER_MODEL_SIZE: u64 = 26_530_550;

/// A speaker model that can tell a meeting's voices apart, from the same
/// sherpa-onnx release. Every model's voiceprints live in their own space,
/// so only the standard one (ResNet34) recognizes known people; the others
/// only decide who spoke which line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeparationModel {
    pub id: SpeakerModelId,
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// Threads to run it on (the large ones are several times the work).
    pub threads: i32,
}

macro_rules! release_url {
    ($file:literal) => {
        concat!(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/",
            $file
        )
    };
}

/// Smallest first.
pub const SEPARATION_MODELS: [SeparationModel; 5] = [
    SeparationModel {
        id: SpeakerModelId::Resnet34,
        name: SPEAKER_MODEL_NAME,
        file: SPEAKER_MODEL_FILE,
        url: SPEAKER_MODEL_URL,
        sha256: SPEAKER_MODEL_SHA256,
        size: SPEAKER_MODEL_SIZE,
        threads: 2,
    },
    SeparationModel {
        id: SpeakerModelId::CamPlusPlus,
        name: "WeSpeaker CAM++",
        file: "wespeaker_en_voxceleb_CAM++_LM.onnx",
        url: release_url!("wespeaker_en_voxceleb_CAM%2B%2B_LM.onnx"),
        sha256: "e197af7e9d473030cf486b3124149a19bf37014d0e4485e4c70c483b0ec10cb2",
        size: 29_292_687,
        threads: 2,
    },
    SeparationModel {
        id: SpeakerModelId::Resnet152,
        name: "WeSpeaker ResNet152",
        file: "wespeaker_en_voxceleb_resnet152_LM.onnx",
        url: release_url!("wespeaker_en_voxceleb_resnet152_LM.onnx"),
        sha256: "3e1c2c7a02097bdddc27c1cf7382237399abdc3b5b682e59455830673c6686e7",
        size: 79_158_470,
        threads: 4,
    },
    SeparationModel {
        id: SpeakerModelId::TitanetLarge,
        name: "NVIDIA TitaNet Large",
        file: "nemo_en_titanet_large.onnx",
        url: release_url!("nemo_en_titanet_large.onnx"),
        sha256: "d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77",
        size: 101_405_493,
        // Large on disk, light to run (faster than ResNet34 here).
        threads: 2,
    },
    SeparationModel {
        id: SpeakerModelId::Resnet293,
        name: "WeSpeaker ResNet293",
        file: "wespeaker_en_voxceleb_resnet293_LM.onnx",
        url: release_url!("wespeaker_en_voxceleb_resnet293_LM.onnx"),
        sha256: "f65dbc820e534eef64ae12d1e289e20244d60e60f7f00d7b092092b1c458be2e",
        size: 114_336_527,
        threads: 4,
    },
];

pub fn separation_model(id: SpeakerModelId) -> &'static SeparationModel {
    SEPARATION_MODELS
        .iter()
        .find(|m| m.id == id)
        .expect("every id is in the catalog")
}

/// The speaker model isn't downloaded. It downloads only when the user asks
/// (Meetings > Settings), so processing refuses to start without it.
/// Downcast an `anyhow::Error` to this to point the user there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerModelMissing;

impl std::fmt::Display for SpeakerModelMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "The speaker recognition model isn't downloaded. Download it in Meetings > \
             Settings, then process the meeting again.",
        )
    }
}

impl std::error::Error for SpeakerModelMissing {}

/// Event emitted while the speaker model downloads. Payload:
/// [`SpeakerModelStatus`].
pub const SPEAKER_MODEL_STATUS_EVENT: &str = "meetings://speaker-model-status";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SpeakerModelStatus {
    /// Not on disk; it downloads on first use (or via `download_speaker_model`).
    Missing,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    Ready,
    /// The last download failed; the next use retries.
    Failed {
        error: String,
    },
}

fn status_cell() -> &'static Mutex<Option<SpeakerModelStatus>> {
    static S: OnceLock<Mutex<Option<SpeakerModelStatus>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Serializes downloads of one file: a second caller waits for the first
/// to finish (different files download side by side).
fn file_lock(path: &Path) -> std::sync::Arc<Mutex<()>> {
    static L: OnceLock<Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<Mutex<()>>>>> =
        OnceLock::new();
    L.get_or_init(Default::default)
        .lock()
        .entry(path.to_path_buf())
        .or_default()
        .clone()
}

/// Where a file downloads to before it is verified and renamed into place.
/// A cancelled or failed download keeps it and the next one resumes.
pub fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
}

/// One downloadable speaker-model file (the standard speaker model, a
/// separation model or the segmentation model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    /// Its download-tracker id.
    pub download_id: String,
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

impl ModelFile {
    pub fn path(&self, app: &AppHandle) -> Result<PathBuf> {
        Ok(speaker_model_dir(app)?.join(self.file))
    }

    pub fn installed(&self, path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|m| m.len() == self.size)
    }
}

/// The file behind a download-tracker id (`speaker:resnet34`,
/// `separation:<id>`, `segmentation:pyannote`).
pub fn download_file_for(id: &str) -> Option<ModelFile> {
    if id == speaker_download_id() {
        let m = separation_model(SpeakerModelId::Resnet34);
        return Some(ModelFile {
            download_id: id.to_string(),
            name: m.name,
            file: m.file,
            url: m.url,
            sha256: m.sha256,
            size: m.size,
        });
    }
    if id == segmentation_download_id() {
        return Some(ModelFile {
            download_id: id.to_string(),
            name: SEGMENTATION_MODEL_NAME,
            file: SEGMENTATION_MODEL_FILE,
            url: SEGMENTATION_MODEL_URL,
            sha256: SEGMENTATION_MODEL_SHA256,
            size: SEGMENTATION_MODEL_SIZE,
        });
    }
    let model = SpeakerModelId::parse(id.strip_prefix("separation:")?);
    if model.is_standard() || separation_download_id(model) != id {
        return None;
    }
    let m = separation_model(model);
    Some(ModelFile {
        download_id: id.to_string(),
        name: m.name,
        file: m.file,
        url: m.url,
        sha256: m.sha256,
        size: m.size,
    })
}

/// The file on disk, downloading it first if needed (blocking: call from a
/// worker thread). The standard speaker model also reports
/// [`SPEAKER_MODEL_STATUS_EVENT`].
pub fn ensure_download(app: &AppHandle, file: &ModelFile) -> Result<PathBuf> {
    if file.download_id == speaker_download_id() {
        return ensure_speaker_model(app);
    }
    ensure_file(app, file, &|_, _| {})
}

fn ensure_file(
    app: &AppHandle,
    file: &ModelFile,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<PathBuf> {
    let path = file.path(app)?;
    if file.installed(&path) {
        return Ok(path);
    }
    let lock = file_lock(&path);
    let _guard = lock.lock();
    if file.installed(&path) {
        return Ok(path);
    }
    download_tracked(app, file, &path, on_progress)?;
    Ok(path)
}

/// Download `file` to `dest` through the download tracker: resumable from
/// [`partial_path`], size- and SHA-256-verified, cancelled by the tracker's
/// token ([`crate::downloads::DownloadCancelled`]).
fn download_tracked(
    app: &AppHandle,
    file: &ModelFile,
    dest: &Path,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<()> {
    let tracker = crate::downloads::global();
    let id = file.download_id.as_str();
    let token = tracker.begin(app, id, file.name, file.size);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let partial = partial_path(dest);
    let size = file.size;
    let progress = |p: &crate::managers::model::DownloadProgress| {
        let total = if p.total > 0 { p.total } else { size };
        tracker.progress(app, id, p.downloaded, total);
        on_progress(p.downloaded, total);
    };
    let verifying = || tracker.verifying(app, id);
    let result = crate::host::async_runtime::block_on(crate::managers::model::download_file(
        id,
        file.url,
        &partial,
        Some(size),
        Some(file.sha256),
        &token,
        &progress,
        &verifying,
    ))
    .and_then(|done| {
        if done {
            std::fs::rename(&partial, dest)
                .with_context(|| format!("rename to {}", dest.display()))?;
        }
        Ok(done)
    });
    match result {
        Ok(true) => {
            tracker.finish(app, id);
            Ok(())
        }
        Ok(false) => {
            tracker.cancelled(app, id);
            Err(crate::downloads::DownloadCancelled.into())
        }
        Err(e) => {
            let error = format!("could not download {}: {e:#}", file.name);
            tracker.fail(app, id, &error);
            Err(e).context(error)
        }
    }
}

/// Remove a file, its partial and the pre-resume `.onnx.part` leftover.
fn remove_model_file(path: &Path) -> Result<()> {
    for p in [
        path.to_path_buf(),
        partial_path(path),
        path.with_extension("onnx.part"),
    ] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("delete {}", p.display())),
        }
    }
    Ok(())
}

pub fn speaker_model_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(crate::portable::app_data_dir(app)
        .map_err(|e| anyhow::anyhow!("app data dir: {e}"))?
        .join("models")
        .join("speaker"))
}

pub fn speaker_model_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(speaker_model_dir(app)?.join(SPEAKER_MODEL_FILE))
}

fn is_installed(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == SPEAKER_MODEL_SIZE)
}

/// Whether the speaker model is on disk (no download).
pub fn speaker_model_installed(app: &AppHandle) -> bool {
    speaker_model_path(app).is_ok_and(|p| is_installed(&p))
}

/// The installed speaker model's path, or [`SpeakerModelMissing`]. Never
/// downloads.
pub fn installed_speaker_model(app: &AppHandle) -> Result<PathBuf> {
    let path = speaker_model_path(app)?;
    if !is_installed(&path) {
        return Err(SpeakerModelMissing.into());
    }
    Ok(path)
}

pub fn speaker_model_status(app: &AppHandle) -> SpeakerModelStatus {
    if let Some(s @ SpeakerModelStatus::Downloading { .. }) = status_cell().lock().clone() {
        return s;
    }
    // Queued behind other downloads (or just started): already on its way.
    if let Some(item) = crate::downloads::global()
        .get(speaker_download_id())
        .filter(|i| i.state.is_active())
    {
        return SpeakerModelStatus::Downloading {
            downloaded: item.downloaded_bytes,
            total: item.total_bytes,
        };
    }
    match speaker_model_path(app) {
        Ok(path) if is_installed(&path) => SpeakerModelStatus::Ready,
        _ => status_cell()
            .lock()
            .clone()
            .filter(|s| matches!(s, SpeakerModelStatus::Failed { .. }))
            .unwrap_or(SpeakerModelStatus::Missing),
    }
}

fn set_status(app: &AppHandle, status: SpeakerModelStatus) {
    *status_cell().lock() = Some(status.clone());
    let _ = app.emit(SPEAKER_MODEL_STATUS_EVENT, status);
}

pub fn separation_model_path(app: &AppHandle, id: SpeakerModelId) -> Result<PathBuf> {
    Ok(speaker_model_dir(app)?.join(separation_model(id).file))
}

/// Whether a separation model is on disk.
pub fn separation_model_installed(app: &AppHandle, id: SpeakerModelId) -> bool {
    let size = separation_model(id).size;
    separation_model_path(app, id)
        .is_ok_and(|p| std::fs::metadata(p).is_ok_and(|m| m.len() == size))
}

/// A separation model's path, downloading it first if needed (the user
/// chose it). Blocking, like [`ensure_speaker_model`]; `on_progress` gets
/// `(downloaded, total)` bytes. The standard one goes through
/// [`ensure_speaker_model`] (its download status is shown in settings).
pub fn ensure_separation_model(
    app: &AppHandle,
    id: SpeakerModelId,
    on_progress: impl FnMut(u64, u64) + Send,
) -> Result<PathBuf> {
    if id.is_standard() {
        return ensure_speaker_model(app);
    }
    let file = download_file_for(&separation_download_id(id))
        .ok_or_else(|| anyhow::anyhow!("unknown speaker model {}", id.as_str()))?;
    let on_progress = Mutex::new(on_progress);
    ensure_file(app, &file, &|done, total| (on_progress.lock())(done, total))
}

/// Delete a downloaded separation model (never the standard one, which
/// recognizes people).
pub fn delete_separation_model(app: &AppHandle, id: SpeakerModelId) -> Result<()> {
    if id.is_standard() {
        bail!("the standard speaker model recognizes people and can't be deleted here");
    }
    remove_model_file(&separation_model_path(app, id)?)
}

/// Delete the standard speaker model. Meeting processing then stops with
/// [`SpeakerModelMissing`] until it is downloaded again (the UI warns).
pub fn delete_speaker_model(app: &AppHandle) -> Result<()> {
    let path = speaker_model_path(app)?;
    let lock = file_lock(&path);
    let _guard = lock.lock();
    remove_model_file(&path)?;
    set_status(app, SpeakerModelStatus::Missing);
    Ok(())
}

/// Delete the speaker segmentation model (meetings then find voice changes
/// with voiceprints alone).
pub fn delete_segmentation_model(app: &AppHandle) -> Result<()> {
    let path = segmentation_model_path(app)?;
    let lock = file_lock(&path);
    let _guard = lock.lock();
    remove_model_file(&path)
}

/// The model path, downloading it first if needed. Blocking: call from a
/// worker thread, never from inside the async runtime.
pub fn ensure_speaker_model(app: &AppHandle) -> Result<PathBuf> {
    let file = download_file_for(speaker_download_id()).expect("the standard model");
    let path = file.path(app)?;
    if file.installed(&path) {
        return Ok(path);
    }
    let lock = file_lock(&path);
    let _guard = lock.lock();
    if file.installed(&path) {
        return Ok(path);
    }
    set_status(
        app,
        SpeakerModelStatus::Downloading {
            downloaded: 0,
            total: SPEAKER_MODEL_SIZE,
        },
    );
    let last_emit = Mutex::new(std::time::Instant::now());
    let result = download_tracked(app, &file, &path, &|downloaded, total| {
        let mut last = last_emit.lock();
        if last.elapsed() >= std::time::Duration::from_millis(250) {
            *last = std::time::Instant::now();
            set_status(app, SpeakerModelStatus::Downloading { downloaded, total });
        }
    });
    match result {
        Ok(()) => {
            set_status(app, SpeakerModelStatus::Ready);
            Ok(path)
        }
        Err(e) if e.is::<crate::downloads::DownloadCancelled>() => {
            set_status(app, SpeakerModelStatus::Missing);
            Err(e)
        }
        Err(e) => {
            let error = format!("Could not download the speaker voice model: {e:#}");
            set_status(
                app,
                SpeakerModelStatus::Failed {
                    error: error.clone(),
                },
            );
            bail!(error)
        }
    }
}

/// Queue the model's download (no-op when installed or already queued).
/// Progress arrives as [`SPEAKER_MODEL_STATUS_EVENT`] and in the download
/// tracker.
pub fn start_speaker_model_download(app: &AppHandle) {
    request_download(app, speaker_download_id());
}

/// Queue a download through the tracker, logging why it couldn't be.
fn request_download(app: &AppHandle, id: &str) {
    match crate::downloads::request(app, id) {
        Ok(()) | Err(crate::downloads::DownloadError::AlreadyInstalled(_)) => {}
        Err(e) => {
            log::warn!("could not queue {id}: {e}");
            if id == speaker_download_id() {
                set_status(
                    app,
                    SpeakerModelStatus::Failed {
                        error: e.to_string(),
                    },
                );
            }
        }
    }
}

/// Download `url` to `dest` through `dest.part`, verifying its SHA-256 before
/// the final rename. A mismatch deletes the partial file.
pub async fn download_verified(
    url: &str,
    dest: &Path,
    sha256: &str,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    download_verified_cancellable(url, dest, sha256, on_progress, || false).await
}

/// [`download_verified`] that stops when `cancel` returns true
/// ([`crate::downloads::DownloadCancelled`]), deleting the partial file.
pub async fn download_verified_cancellable(
    url: &str,
    dest: &Path,
    sha256: &str,
    mut on_progress: impl FnMut(u64, Option<u64>),
    cancel: impl Fn() -> bool,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let part = dest.with_extension("onnx.part");
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        bail!("GET {url}: HTTP {}", response.status());
    }
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&part)
        .await
        .with_context(|| format!("create {}", part.display()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut body = response.bytes_stream();
    let stall = std::time::Duration::from_secs(60);
    loop {
        let next = tokio::time::timeout(stall, body.next())
            .await
            .map_err(|_| anyhow::anyhow!("download stalled"))?;
        let Some(chunk) = next else { break };
        let chunk = chunk.context("download interrupted")?;
        if cancel() {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err(crate::downloads::DownloadCancelled.into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    if cancel() {
        let _ = std::fs::remove_file(&part);
        return Err(crate::downloads::DownloadCancelled.into());
    }
    file.flush().await?;
    drop(file);
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&part);
        bail!("checksum mismatch (expected {sha256}, got {actual})");
    }
    std::fs::rename(&part, dest).with_context(|| format!("rename to {}", dest.display()))?;
    Ok(())
}

/// Computes speaker embeddings with sherpa-onnx.
pub struct SpeakerEmbedder {
    extractor: sherpa_onnx::SpeakerEmbeddingExtractor,
}

impl SpeakerEmbedder {
    pub fn new(model: &Path) -> Result<Self> {
        Self::with_threads(model, 2)
    }

    pub fn with_threads(model: &Path, num_threads: i32) -> Result<Self> {
        let config = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(model.to_string_lossy().into_owned()),
            num_threads,
            debug: false,
            provider: Some("cpu".into()),
        };
        let extractor =
            sherpa_onnx::SpeakerEmbeddingExtractor::create(&config).ok_or_else(|| {
                anyhow::anyhow!("could not load the speaker model {}", model.display())
            })?;
        Ok(Self { extractor })
    }

    pub fn dim(&self) -> usize {
        self.extractor.dim() as usize
    }

    /// The embedding of 16 kHz mono `pcm`, or `None` when it is too short
    /// for the model.
    pub fn embed(&self, pcm: &[f32]) -> Result<Option<Vec<f32>>> {
        let stream = self
            .extractor
            .create_stream()
            .ok_or_else(|| anyhow::anyhow!("speaker model: could not create a stream"))?;
        stream.accept_waveform(SAMPLE_RATE as i32, pcm);
        stream.input_finished();
        if !self.extractor.is_ready(&stream) {
            return Ok(None);
        }
        Ok(self.extractor.compute(&stream))
    }
}

/// Speaker segmentation model: pyannote segmentation-3.0 (MIT), exported by
/// sherpa-onnx. It finds where the voice changes inside a line (see
/// [`super::turns`]); it downloads in the background with the speaker
/// model and, until it is there, the voiceprint scan finds changes alone.
pub const SEGMENTATION_MODEL_FILE: &str = "pyannote_segmentation_3_0.onnx";
pub const SEGMENTATION_MODEL_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/9403a6902bb58e3d5ae8c7e77c3422de279db2e0/model.onnx";
pub const SEGMENTATION_MODEL_SHA256: &str =
    "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079";
pub const SEGMENTATION_MODEL_SIZE: u64 = 5_992_913;
pub const SEGMENTATION_MODEL_NAME: &str = "Pyannote Segmentation 3.0";

/// Tracker id of the standard speaker model.
pub fn speaker_download_id() -> &'static str {
    "speaker:resnet34"
}

/// Tracker id of a separation model.
pub fn separation_download_id(id: SpeakerModelId) -> String {
    format!("separation:{}", id.as_str())
}

/// Tracker id of the pyannote segmentation model.
pub fn segmentation_download_id() -> &'static str {
    "segmentation:pyannote"
}

/// TitaNet plus the segmentation model, the pair a meeting uses to split
/// lines where the voice changes.
pub fn speaker_turn_models_bytes() -> u64 {
    separation_model(SpeakerModelId::TitanetLarge).size + SEGMENTATION_MODEL_SIZE
}

/// Both speaker-turn models are on disk.
pub fn speaker_turn_models_installed(app: &AppHandle) -> bool {
    separation_model_installed(app, SpeakerModelId::TitanetLarge)
        && installed_segmentation_model(app).is_some()
}

pub fn segmentation_model_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(speaker_model_dir(app)?.join(SEGMENTATION_MODEL_FILE))
}

/// The segmentation model's path when it is on disk (no download).
pub fn installed_segmentation_model(app: &AppHandle) -> Option<PathBuf> {
    segmentation_model_path(app)
        .ok()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() == SEGMENTATION_MODEL_SIZE))
}

/// Queue the segmentation model's download (no-op when it is there, or
/// already queued).
pub fn start_segmentation_model_download(app: &AppHandle) {
    if installed_segmentation_model(app).is_some() {
        return;
    }
    request_download(app, segmentation_download_id());
}

/// Speaker turns from the segmentation model, through sherpa-onnx's
/// diarizer. Its speaker labels hold within one call only; the pipeline
/// uses them just to find where the voice changes.
pub struct PyannoteSegmenter {
    diarizer: sherpa_onnx::OfflineSpeakerDiarization,
}

impl PyannoteSegmenter {
    /// `segmentation`: the pyannote model; `embedding`: a speaker model the
    /// diarizer links one window's speakers to the next with.
    pub fn new(segmentation: &Path, embedding: &Path) -> Result<Self> {
        let config = sherpa_onnx::OfflineSpeakerDiarizationConfig {
            segmentation: sherpa_onnx::OfflineSpeakerSegmentationModelConfig {
                pyannote: sherpa_onnx::OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(segmentation.to_string_lossy().into_owned()),
                    window_shift_ratio: SEGMENTATION_WINDOW_SHIFT,
                },
                num_threads: 2,
                ..Default::default()
            },
            embedding: sherpa_onnx::SpeakerEmbeddingExtractorConfig {
                model: Some(embedding.to_string_lossy().into_owned()),
                num_threads: 2,
                debug: false,
                provider: Some("cpu".into()),
            },
            // Splitting one voice in two costs only a check (every change
            // is confirmed by voiceprints); merging two hides a change.
            clustering: sherpa_onnx::FastClusteringConfig {
                threshold: SEGMENTATION_CLUSTER_THRESHOLD,
                ..Default::default()
            },
            ..Default::default()
        };
        let diarizer =
            sherpa_onnx::OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
                anyhow::anyhow!(
                    "could not load the speaker segmentation model {}",
                    segmentation.display()
                )
            })?;
        Ok(Self { diarizer })
    }
}

/// Step of the model's 10 s window, as a share of it: 0.25 finds the same
/// changes as sherpa's 0.1 in half the time; 0.5 misplaces some.
const SEGMENTATION_WINDOW_SHIFT: f32 = 0.25;

/// sherpa-onnx's clustering cut for [`PyannoteSegmenter`]: lower makes more
/// speakers.
const SEGMENTATION_CLUSTER_THRESHOLD: f32 = 0.5;

impl super::turns::TurnSegmenter for PyannoteSegmenter {
    fn turns(&mut self, pcm: &[f32]) -> Result<Vec<super::turns::Turn>> {
        let Some(result) = self.diarizer.process(pcm) else {
            bail!("speaker segmentation failed");
        };
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .map(|s| super::turns::Turn {
                t0_ms: (s.start * 1000.0).round() as i64,
                t1_ms: (s.end * 1000.0).round() as i64,
                speaker: s.speaker,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A speaker model file loads and embeds the fixture: halves of one
    /// voice are similar, the voice and silence-free noise aren't. Run with
    /// `GUIDY_TEST_SPEAKER_MODEL=<onnx> cargo test -p guidy-core --lib
    /// speaker_model::tests::embeds -- --ignored --nocapture`
    // IGNORE-REASON: needs a downloaded speaker model.
    #[test]
    #[ignore]
    fn embeds_with_a_real_model() {
        let model =
            std::env::var_os("GUIDY_TEST_SPEAKER_MODEL").expect("set GUIDY_TEST_SPEAKER_MODEL");
        let wav = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/audio/speech-16k-mono.wav"
        );
        let pcm = crate::audio_toolkit::read_wav_samples(wav).unwrap();
        let e = SpeakerEmbedder::new(Path::new(&model)).unwrap();
        let half = pcm.len() / 2;
        let started = std::time::Instant::now();
        let a = e.embed(&pcm[..half]).unwrap().unwrap();
        let b = e.embed(&pcm[half..]).unwrap().unwrap();
        let noise: Vec<f32> = (0..half)
            .map(|i| ((i * 7919 % 1000) as f32 / 1000.0 - 0.5) * 0.2)
            .collect();
        let n = e.embed(&noise).unwrap().unwrap();
        let cos = super::super::cluster::cosine;
        println!(
            "dim {} · same voice {:.2} · voice vs noise {:.2} · {} ms for two halves",
            e.dim(),
            cos(&a, &b),
            cos(&a, &n),
            started.elapsed().as_millis()
        );
        assert!(cos(&a, &b) > cos(&a, &n));
    }

    /// A one-shot HTTP server that answers every request with `body`.
    async fn serve(body: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}/model.onnx")
    }

    #[test]
    fn download_ids_map_to_their_files() {
        assert_eq!(
            download_file_for(speaker_download_id()).unwrap().file,
            SPEAKER_MODEL_FILE
        );
        assert_eq!(
            download_file_for(segmentation_download_id()).unwrap().size,
            SEGMENTATION_MODEL_SIZE
        );
        let titanet = separation_download_id(SpeakerModelId::TitanetLarge);
        assert_eq!(
            download_file_for(&titanet).unwrap().file,
            "nemo_en_titanet_large.onnx"
        );
        assert!(download_file_for("separation:resnet34").is_none());
        assert!(download_file_for("separation:nope").is_none());
        assert_eq!(
            partial_path(Path::new("/a/m.onnx")),
            Path::new("/a/m.onnx.partial")
        );
    }

    #[test]
    fn the_standard_and_segmentation_models_can_be_deleted() {
        let t = crate::host::testing::TestApp::new();
        let path = speaker_model_path(&t.app).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"model").unwrap();
        std::fs::write(partial_path(&path), b"part").unwrap();
        delete_speaker_model(&t.app).unwrap();
        assert!(!path.exists() && !partial_path(&path).exists());
        assert_eq!(speaker_model_status(&t.app), SpeakerModelStatus::Missing);
        let seg = segmentation_model_path(&t.app).unwrap();
        std::fs::write(&seg, b"seg").unwrap();
        delete_segmentation_model(&t.app).unwrap();
        assert!(!seg.exists());
        // Deleting what isn't there is fine.
        delete_segmentation_model(&t.app).unwrap();
        assert!(delete_separation_model(&t.app, SpeakerModelId::Resnet34).is_err());
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        let body: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let sha = format!("{:x}", Sha256::digest(&body));
        let url = serve(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("speaker").join("m.onnx");
        let mut last = 0;
        download_verified(&url, &dest, &sha, |d, _| last = d)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(last, body.len() as u64);
    }

    #[tokio::test]
    async fn checksum_mismatch_leaves_nothing() {
        let url = serve(b"not the model".to_vec()).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.onnx");
        let err = download_verified(&url, &dest, &"0".repeat(64), |_, _| {})
            .await
            .unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        assert!(!dest.exists());
        assert!(!dest.with_extension("onnx.part").exists());
    }
}
