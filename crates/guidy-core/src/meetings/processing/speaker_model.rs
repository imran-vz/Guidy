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

pub const SPEAKER_MODEL_FILE: &str = "wespeaker_en_voxceleb_resnet34_LM.onnx";
pub const SPEAKER_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx";
pub const SPEAKER_MODEL_SHA256: &str =
    "e9848563da86f263117134dfd7ad63c92355b37de492b55e325400c9d9c39012";
pub const SPEAKER_MODEL_SIZE: u64 = 26_530_550;

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

/// Serializes downloads: a second caller waits for the first to finish.
fn download_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
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

pub fn speaker_model_status(app: &AppHandle) -> SpeakerModelStatus {
    if let Some(s @ SpeakerModelStatus::Downloading { .. }) = status_cell().lock().clone() {
        return s;
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

/// The model path, downloading it first if needed. Blocking: call from a
/// worker thread, never from inside the async runtime.
pub fn ensure_speaker_model(app: &AppHandle) -> Result<PathBuf> {
    let path = speaker_model_path(app)?;
    if is_installed(&path) {
        return Ok(path);
    }
    let _guard = download_lock().lock();
    if is_installed(&path) {
        return Ok(path);
    }
    set_status(
        app,
        SpeakerModelStatus::Downloading {
            downloaded: 0,
            total: SPEAKER_MODEL_SIZE,
        },
    );
    let mut last_emit = std::time::Instant::now();
    let result = crate::host::async_runtime::block_on(download_verified(
        SPEAKER_MODEL_URL,
        &path,
        SPEAKER_MODEL_SHA256,
        |downloaded, total| {
            if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
                last_emit = std::time::Instant::now();
                set_status(
                    app,
                    SpeakerModelStatus::Downloading {
                        downloaded,
                        total: total.unwrap_or(SPEAKER_MODEL_SIZE),
                    },
                );
            }
        },
    ));
    match result {
        Ok(()) => {
            set_status(app, SpeakerModelStatus::Ready);
            Ok(path)
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

/// Start downloading the model in the background (no-op when installed).
pub fn start_speaker_model_download(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("speaker-model-download".into())
        .spawn(move || {
            if let Err(e) = ensure_speaker_model(&app) {
                log::warn!("{e:#}");
            }
        });
}

/// Download `url` to `dest` through `dest.part`, verifying its SHA-256 before
/// the final rename. A mismatch deletes the partial file.
pub async fn download_verified(
    url: &str,
    dest: &Path,
    sha256: &str,
    mut on_progress: impl FnMut(u64, Option<u64>),
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
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
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
        let config = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(model.to_string_lossy().into_owned()),
            num_threads: 2,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
