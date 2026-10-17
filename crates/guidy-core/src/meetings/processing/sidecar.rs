//! Encrypted sidecar files next to `meetings.db`:
//!
//! - `<app_data_dir>/meetings/processing/<id>.bin`: the [`ProcessingCache`]
//!   checkpoint (VAD spans, transcripts, embeddings, clustering) so a
//!   cancelled or crashed run resumes. Deleted once processing is done.
//! - `<app_data_dir>/meetings/embeddings/<id>.bin`: one voiceprint per
//!   embedded system segment of the processed meeting, keyed by the stored
//!   segment id. Naming a speaker copies the longest ones into `voiceprints`.
//!   Deleted with the meeting ([`delete_sidecars`]), never by audio retention.
//!
//! Format: `"GDYSIDE1" || nonce (12) || ChaCha20-Poly1305(JSON)`, keyed with
//! the meetings data key, AAD = the magic.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::cluster::SpeakerMatch;
use super::timeline::RawSegment;
use super::vad::Span;
use crate::meetings::types::{MeetingId, Track};

const MAGIC: &[u8; 8] = b"GDYSIDE1";
const NONCE_LEN: usize = 12;

pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("sidecar encryption failed"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < MAGIC.len() + NONCE_LEN || &data[..MAGIC.len()] != MAGIC {
        bail!("not a Guidy sidecar file");
    }
    let (nonce, ct) = data[MAGIC.len()..].split_at(NONCE_LEN);
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("sidecar could not be decrypted (wrong key or damaged)"))
}

/// Write `value` encrypted, atomically (temp file + rename).
pub fn write<T: Serialize>(path: &Path, key: &[u8; 32], value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let sealed = seal(key, &serde_json::to_vec(value)?)?;
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, sealed).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

/// Read a sidecar. `Ok(None)` when the file doesn't exist.
pub fn read<T: DeserializeOwned>(path: &Path, key: &[u8; 32]) -> Result<Option<T>> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let plain = open(key, &data).with_context(|| path.display().to_string())?;
    Ok(Some(serde_json::from_slice(&plain).with_context(|| {
        format!("{}: unreadable contents", path.display())
    })?))
}

fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("delete {}", path.display())),
    }
}

pub fn cache_path(meetings_dir: &Path, id: MeetingId) -> PathBuf {
    meetings_dir.join("processing").join(format!("{id}.bin"))
}

pub fn embeddings_path(meetings_dir: &Path, id: MeetingId) -> PathBuf {
    meetings_dir.join("embeddings").join(format!("{id}.bin"))
}

pub fn delete_cache(meetings_dir: &Path, id: MeetingId) -> Result<()> {
    remove(&cache_path(meetings_dir, id))
}

/// Delete both sidecars of a meeting. Call when the meeting is deleted.
pub fn delete_sidecars(meetings_dir: &Path, id: MeetingId) -> Result<()> {
    remove(&cache_path(meetings_dir, id))?;
    remove(&embeddings_path(meetings_dir, id))
}

/// Bumped when [`ProcessingCache`] changes incompatibly; an old cache is
/// then discarded and processing restarts.
pub const CACHE_VERSION: u32 = 1;

/// One unit of work: a VAD segment of one track.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    pub track: Track,
    pub span: Span,
}

/// The transcript of one [`Unit`] (words already in meeting time).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct UnitTranscript {
    pub text: String,
    pub words: Vec<crate::meetings::types::Word>,
}

/// Clustering result over the system units.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Clustering {
    /// Per unit (same indices as `units`): local cluster index, `None` for
    /// mic units or system units with no speech.
    pub labels: Vec<Option<usize>>,
    /// Per cluster: centroid voiceprint (`None` when no unit had one).
    pub centroids: Vec<Option<Vec<f32>>>,
    /// Per cluster: the automatic match against known people.
    pub matches: Vec<SpeakerMatch>,
}

/// Everything a run has computed so far. The stage recorded in the database
/// says which parts are complete; partial `transcripts` / `embeddings`
/// entries let a stage itself resume where it stopped.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ProcessingCache {
    pub version: u32,
    /// Track lengths from the decode stage (`None` = no such track).
    pub mic_ms: Option<i64>,
    pub system_ms: Option<i64>,
    /// VAD segments of both tracks, mic first then system, each by time.
    pub units: Vec<Unit>,
    /// Per unit, once transcribed.
    pub transcripts: Vec<Option<UnitTranscript>>,
    /// Per unit, once the voiceprint stage looked at it: `Some(None)` means
    /// skipped (mic, too short, or no speech).
    pub embeddings: Vec<Option<Option<Vec<f32>>>>,
    pub clustering: Option<Clustering>,
}

impl ProcessingCache {
    pub fn new() -> Self {
        Self {
            version: CACHE_VERSION,
            ..Self::default()
        }
    }

    /// The transcribed unit `i` as a segment (`None` until transcribed).
    pub fn raw_segment(&self, i: usize) -> Option<RawSegment> {
        let unit = self.units.get(i)?;
        let t = self.transcripts.get(i)?.as_ref()?;
        Some(RawSegment {
            track: unit.track,
            t0_ms: unit.span.t0_ms,
            t1_ms: unit.span.t1_ms,
            text: t.text.clone(),
            words: t.words.clone(),
        })
    }
}

/// A stored segment's voiceprint (the embeddings sidecar).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SegmentEmbedding {
    pub segment_id: i64,
    pub duration_ms: i64,
    pub embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_the_wrong_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let path = cache_path(dir.path(), 3);
        let mut cache = ProcessingCache::new();
        cache.mic_ms = Some(1234);
        cache.units.push(Unit {
            track: Track::System,
            span: Span::new(1, 2),
        });
        write(&path, &key, &cache).unwrap();
        let back: ProcessingCache = read(&path, &key).unwrap().unwrap();
        assert_eq!(back, cache);
        assert!(read::<ProcessingCache>(&path, &[8u8; 32]).is_err());
        // The plaintext is not on disk.
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"1234"));

        assert!(read::<ProcessingCache>(&cache_path(dir.path(), 4), &key)
            .unwrap()
            .is_none());
        write(
            &embeddings_path(dir.path(), 3),
            &key,
            &Vec::<SegmentEmbedding>::new(),
        )
        .unwrap();
        delete_sidecars(dir.path(), 3).unwrap();
        assert!(!path.exists() && !embeddings_path(dir.path(), 3).exists());
        delete_sidecars(dir.path(), 3).unwrap();
    }

    #[test]
    fn rejects_garbage() {
        assert!(open(&[0u8; 32], b"short").is_err());
        assert!(open(&[0u8; 32], b"GDYSIDE1............garbage").is_err());
    }
}
