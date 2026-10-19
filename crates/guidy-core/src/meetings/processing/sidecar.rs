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
//! Format: `"GDYSIDE2" || nonce (12) || ChaCha20-Poly1305(JSON)`, keyed with
//! the meetings data key. The AAD is `magic || kind (1 byte) || meeting id
//! (i64 LE)`, so a file can't be swapped between meetings or between the
//! cache and the embeddings slot. Files written by the first format
//! (`GDYSIDE1`, AAD = magic only) are not read any more: they are caches, so
//! a checkpoint restarts processing and a missing embeddings file makes
//! naming fall back to the speaker centroid. Files (and their temp files) are
//! created owner-only (0600) on unix.

use std::collections::HashSet;
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

const MAGIC: &[u8; 8] = b"GDYSIDE2";
const NONCE_LEN: usize = 12;

/// Which sidecar a file is (bound into the AAD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarKind {
    /// The [`ProcessingCache`] checkpoint.
    Cache,
    /// Per-segment voiceprints ([`SegmentEmbedding`]s).
    Embeddings,
}

impl SidecarKind {
    fn tag(self) -> u8 {
        match self {
            Self::Cache => 1,
            Self::Embeddings => 2,
        }
    }
}

fn aad(kind: SidecarKind, meeting_id: MeetingId) -> Vec<u8> {
    let mut a = Vec::with_capacity(MAGIC.len() + 9);
    a.extend_from_slice(MAGIC);
    a.push(kind.tag());
    a.extend_from_slice(&meeting_id.to_le_bytes());
    a
}

pub fn seal(
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(kind, meeting_id),
            },
        )
        .map_err(|_| anyhow::anyhow!("sidecar encryption failed"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
    data: &[u8],
) -> Result<Vec<u8>> {
    if data.len() < MAGIC.len() + NONCE_LEN || &data[..MAGIC.len()] != MAGIC {
        bail!("not a Guidy sidecar file (or an old format)");
    }
    let (nonce, ct) = data[MAGIC.len()..].split_at(NONCE_LEN);
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: &aad(kind, meeting_id),
            },
        )
        .map_err(|_| {
            anyhow::anyhow!("sidecar could not be decrypted (wrong key, meeting or damaged)")
        })
}

/// Write `bytes` to `path` creating it owner-only (0600) on unix.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(bytes)?;
    f.sync_all()
}

/// Write `value` encrypted, atomically (temp file + rename).
fn write_at<T: Serialize + ?Sized>(
    path: &Path,
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
    value: &T,
) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let sealed = seal(key, kind, meeting_id, &serde_json::to_vec(value)?)?;
    let tmp = temp_path(path);
    let written = write_private(&tmp, &sealed)
        .with_context(|| format!("write {}", tmp.display()))
        .and_then(|()| {
            std::fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// A temp file next to `path` no other writer uses (`<name>.<pid>-<n>.tmp`):
/// two concurrent writes never share (and tear) one temp file. One a crash
/// leaves behind is never reused: [`delete_sidecars`] and
/// [`sweep_stale_temp_files`] remove it.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.{}-{n}.tmp", std::process::id()))
}

/// Held while a meeting's embeddings sidecar is rewritten from database
/// state: by a processing run from replacing the transcript to writing the
/// new voiceprints, and by a person deletion from finding their segments to
/// dropping them. So a deletion never filters a stale copy over a run's new
/// file (or drops segment ids the new transcript reuses).
pub struct EmbeddingsLock {
    key: (PathBuf, MeetingId),
}

/// The held [`EmbeddingsLock`]s, and the condvar their release signals.
type EmbeddingsLocks = (
    parking_lot::Mutex<HashSet<(PathBuf, MeetingId)>>,
    parking_lot::Condvar,
);

fn embeddings_locks() -> &'static EmbeddingsLocks {
    static LOCKS: std::sync::OnceLock<EmbeddingsLocks> = std::sync::OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

/// Wait for and take meeting `id`'s [`EmbeddingsLock`]. Not reentrant; a
/// caller taking several takes them in ascending meeting order.
pub fn lock_embeddings(meetings_dir: &Path, id: MeetingId) -> EmbeddingsLock {
    let key = (meetings_dir.to_path_buf(), id);
    let (held, freed) = embeddings_locks();
    let mut held = held.lock();
    while held.contains(&key) {
        freed.wait(&mut held);
    }
    held.insert(key.clone());
    EmbeddingsLock { key }
}

impl Drop for EmbeddingsLock {
    fn drop(&mut self) {
        let (held, freed) = embeddings_locks();
        held.lock().remove(&self.key);
        freed.notify_all();
    }
}

/// Read a sidecar. `Ok(None)` when the file doesn't exist.
fn read_at<T: DeserializeOwned>(
    path: &Path,
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
) -> Result<Option<T>> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let plain = open(key, kind, meeting_id, &data).with_context(|| path.display().to_string())?;
    Ok(Some(serde_json::from_slice(&plain).with_context(|| {
        format!("{}: unreadable contents", path.display())
    })?))
}

pub fn write_cache(
    meetings_dir: &Path,
    id: MeetingId,
    key: &[u8; 32],
    cache: &ProcessingCache,
) -> Result<()> {
    write_at(
        &cache_path(meetings_dir, id),
        key,
        SidecarKind::Cache,
        id,
        cache,
    )
}

pub fn read_cache(
    meetings_dir: &Path,
    id: MeetingId,
    key: &[u8; 32],
) -> Result<Option<ProcessingCache>> {
    read_at(&cache_path(meetings_dir, id), key, SidecarKind::Cache, id)
}

pub fn write_embeddings(
    meetings_dir: &Path,
    id: MeetingId,
    key: &[u8; 32],
    embeddings: &[SegmentEmbedding],
) -> Result<()> {
    write_at(
        &embeddings_path(meetings_dir, id),
        key,
        SidecarKind::Embeddings,
        id,
        embeddings,
    )
}

pub fn read_embeddings(
    meetings_dir: &Path,
    id: MeetingId,
    key: &[u8; 32],
) -> Result<Option<Vec<SegmentEmbedding>>> {
    read_at(
        &embeddings_path(meetings_dir, id),
        key,
        SidecarKind::Embeddings,
        id,
    )
}

/// Remove the per-segment voiceprints of `segment_ids` from meeting `id`'s
/// embeddings sidecar (e.g. a deleted person's). Returns how many were
/// removed; a missing sidecar is fine. The caller holds `lock` (taken before
/// it read `segment_ids` from the database).
pub fn drop_segment_embeddings(
    lock: &EmbeddingsLock,
    meetings_dir: &Path,
    id: MeetingId,
    key: &[u8; 32],
    segment_ids: &HashSet<i64>,
) -> Result<usize> {
    debug_assert!(lock.key == (meetings_dir.to_path_buf(), id));
    let Some(mut all) = read_embeddings(meetings_dir, id, key)? else {
        return Ok(0);
    };
    let before = all.len();
    all.retain(|e| !segment_ids.contains(&e.segment_id));
    let removed = before - all.len();
    if removed > 0 {
        write_embeddings(meetings_dir, id, key, &all)?;
    }
    Ok(removed)
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

/// Delete a meeting's embeddings sidecar. The caller holds its
/// [`EmbeddingsLock`].
pub fn delete_embeddings(meetings_dir: &Path, id: MeetingId) -> Result<()> {
    remove(&embeddings_path(meetings_dir, id))
}

/// The sidecar directories (they hold the sidecars and their temp files).
fn sidecar_dirs(meetings_dir: &Path) -> [PathBuf; 2] {
    [
        meetings_dir.join("processing"),
        meetings_dir.join("embeddings"),
    ]
}

/// The files of `dir` (none when it doesn't exist).
fn dir_files(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .collect::<std::io::Result<_>>()
            .with_context(|| format!("list {}", dir.display()))?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("list {}", dir.display())),
    }
}

/// Delete both sidecars of a meeting, and any temp files of them a crash
/// left. Call when the meeting is deleted. Takes the meeting's
/// [`EmbeddingsLock`] (so the caller must not hold it): a person deletion
/// filtering the embeddings sidecar can't write it back afterwards.
pub fn delete_sidecars(meetings_dir: &Path, id: MeetingId) -> Result<()> {
    let _lock = lock_embeddings(meetings_dir, id);
    remove(&cache_path(meetings_dir, id))?;
    remove(&embeddings_path(meetings_dir, id))?;
    let prefix = format!("{id}.bin.");
    for dir in sidecar_dirs(meetings_dir) {
        for file in dir_files(&dir)? {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name.ends_with(".tmp") {
                remove(&file.path())?;
            }
        }
    }
    Ok(())
}

/// Remove sidecar temp files (`*.tmp`) last written more than `max_age`
/// ago: a crash mid-write leaves one, and its unique name is never written
/// again. A write in progress is far younger than any sensible `max_age`.
/// Returns how many were removed; one that can't be is logged and skipped.
pub fn sweep_stale_temp_files(meetings_dir: &Path, max_age: std::time::Duration) -> Result<usize> {
    let mut removed = 0;
    for dir in sidecar_dirs(meetings_dir) {
        for file in dir_files(&dir)? {
            if !file.file_name().to_string_lossy().ends_with(".tmp") {
                continue;
            }
            let stale = file
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > max_age));
            if !stale {
                continue;
            }
            match remove(&file.path()) {
                Ok(()) => removed += 1,
                Err(e) => log::warn!("meetings: {e:#}"),
            }
        }
    }
    Ok(removed)
}

/// Bumped when [`ProcessingCache`] changes incompatibly; an old cache is
/// then discarded and processing restarts.
/// 2: `embeddings` entries keep "skipped" apart from "not looked at" (in 1
/// both were `null`, so a checkpoint past the voiceprint stage never
/// resumed).
pub const CACHE_VERSION: u32 = 2;

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

/// Clustering result over the system units (and the mic units of other
/// people in the room, when the user's voice is recorded).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Clustering {
    /// Per unit (same indices as `units`): local cluster index, `None` for
    /// the user's mic units or system units with no speech.
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
    #[serde(with = "unit_embeddings")]
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

/// [`ProcessingCache::embeddings`] in JSON, where `Some(None)` and `None`
/// would both be `null`: an entry is `null` (not looked at yet), `[]`
/// (skipped) or `[voiceprint]`.
mod unit_embeddings {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    type Entry = Option<Option<Vec<f32>>>;

    pub fn serialize<S: Serializer>(v: &[Entry], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|e| e.as_ref().map(|e| e.iter().collect::<Vec<_>>()))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Entry>, D::Error> {
        let raw = Vec::<Option<Vec<Vec<f32>>>>::deserialize(d)?;
        raw.into_iter()
            .map(|e| match e {
                None => Ok(None),
                Some(mut v) if v.len() <= 1 => Ok(Some(v.pop())),
                Some(_) => Err(serde::de::Error::custom(
                    "a unit has more than one voiceprint",
                )),
            })
            .collect()
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
        // Not looked at, skipped and embedded stay apart.
        cache.embeddings = vec![None, Some(None), Some(Some(vec![0.5, 0.25]))];
        write_cache(dir.path(), 3, &key, &cache).unwrap();
        let back = read_cache(dir.path(), 3, &key).unwrap().unwrap();
        assert_eq!(back, cache);
        assert!(read_cache(dir.path(), 3, &[8u8; 32]).is_err());
        // The plaintext is not on disk.
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"1234"));

        assert!(read_cache(dir.path(), 4, &key).unwrap().is_none());
        write_embeddings(dir.path(), 3, &key, &[]).unwrap();
        delete_sidecars(dir.path(), 3).unwrap();
        assert!(!path.exists() && !embeddings_path(dir.path(), 3).exists());
        delete_sidecars(dir.path(), 3).unwrap();
    }

    /// Temp files a crash left: deleting the meeting removes its own (not
    /// another meeting's, e.g. 1 vs 12).
    #[test]
    fn delete_sidecars_removes_left_over_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        write_cache(dir.path(), 1, &key, &ProcessingCache::new()).unwrap();
        write_embeddings(dir.path(), 1, &key, &[]).unwrap();
        let left = [
            temp_path(&cache_path(dir.path(), 1)),
            temp_path(&embeddings_path(dir.path(), 1)),
        ];
        let other = temp_path(&embeddings_path(dir.path(), 12));
        for f in left.iter().chain([&other]) {
            std::fs::write(f, b"partial").unwrap();
        }
        delete_sidecars(dir.path(), 1).unwrap();
        assert!(left.iter().all(|f| !f.exists()));
        assert!(other.exists());
    }

    #[test]
    fn the_sweep_removes_only_old_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        write_embeddings(dir.path(), 1, &key, &[]).unwrap();
        let old = [
            temp_path(&cache_path(dir.path(), 1)),
            temp_path(&embeddings_path(dir.path(), 2)),
        ];
        let fresh = temp_path(&embeddings_path(dir.path(), 3));
        std::fs::create_dir_all(cache_path(dir.path(), 1).parent().unwrap()).unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        for f in &old {
            std::fs::write(f, b"partial").unwrap();
            std::fs::File::options()
                .write(true)
                .open(f)
                .unwrap()
                .set_modified(two_hours_ago)
                .unwrap();
        }
        std::fs::write(&fresh, b"in progress").unwrap();
        // An old sidecar itself is not a temp file.
        std::fs::File::options()
            .write(true)
            .open(embeddings_path(dir.path(), 1))
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();

        let hour = std::time::Duration::from_secs(3600);
        assert_eq!(sweep_stale_temp_files(dir.path(), hour).unwrap(), 2);
        assert!(old.iter().all(|f| !f.exists()));
        assert!(fresh.exists());
        assert!(embeddings_path(dir.path(), 1).exists());
        // No sidecar directories yet: nothing to do.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(sweep_stale_temp_files(empty.path(), hour).unwrap(), 0);
    }

    /// Deleting the sidecars waits for a holder of the embeddings lock (a
    /// person deletion filtering the file), so it can't be written back.
    #[test]
    fn delete_sidecars_waits_for_the_embeddings_lock() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        write_embeddings(dir.path(), 1, &key, &[]).unwrap();
        let held = lock_embeddings(dir.path(), 1);
        std::thread::scope(|s| {
            let deletion = s.spawn(|| delete_sidecars(dir.path(), 1));
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(!deletion.is_finished());
            // The holder rewrites the file, then lets go.
            write_embeddings(dir.path(), 1, &key, &[]).unwrap();
            drop(held);
            deletion.join().unwrap().unwrap();
        });
        assert!(!embeddings_path(dir.path(), 1).exists());
    }

    #[test]
    fn a_sidecar_is_bound_to_its_meeting_and_kind() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let prints = vec![SegmentEmbedding {
            segment_id: 1,
            duration_ms: 2000,
            embedding: vec![0.5],
        }];
        write_embeddings(dir.path(), 1, &key, &prints).unwrap();
        // Copied into another meeting's slot: refused.
        std::fs::create_dir_all(embeddings_path(dir.path(), 2).parent().unwrap()).unwrap();
        std::fs::copy(
            embeddings_path(dir.path(), 1),
            embeddings_path(dir.path(), 2),
        )
        .unwrap();
        assert!(read_embeddings(dir.path(), 2, &key).is_err());
        // Copied into the cache slot of the same meeting: refused.
        std::fs::create_dir_all(cache_path(dir.path(), 1).parent().unwrap()).unwrap();
        std::fs::copy(embeddings_path(dir.path(), 1), cache_path(dir.path(), 1)).unwrap();
        assert!(read_cache(dir.path(), 1, &key).is_err());
        assert_eq!(read_embeddings(dir.path(), 1, &key).unwrap(), Some(prints));
    }

    #[test]
    fn old_format_files_are_not_read() {
        // A GDYSIDE1 file (AAD = magic only) is treated as unreadable.
        let mut old = b"GDYSIDE1".to_vec();
        old.extend_from_slice(&[0u8; 40]);
        assert!(open(&[0u8; 32], SidecarKind::Cache, 1, &old).is_err());
    }

    #[test]
    fn drop_segment_embeddings_removes_only_the_given_segments() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let e = |id| SegmentEmbedding {
            segment_id: id,
            duration_ms: 1000,
            embedding: vec![id as f32],
        };
        write_embeddings(dir.path(), 5, &key, &[e(1), e(2), e(3)]).unwrap();
        let lock = lock_embeddings(dir.path(), 5);
        let n =
            drop_segment_embeddings(&lock, dir.path(), 5, &key, &HashSet::from([1, 3, 9])).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            read_embeddings(dir.path(), 5, &key).unwrap(),
            Some(vec![e(2)])
        );
        let lock = lock_embeddings(dir.path(), 6);
        assert_eq!(
            drop_segment_embeddings(&lock, dir.path(), 6, &key, &HashSet::from([1])).unwrap(),
            0
        );
    }

    /// Concurrent writers each use their own temp file: every rename lands
    /// a whole file, and no temp file is left behind.
    #[test]
    fn concurrent_writes_never_tear_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let e = |id| SegmentEmbedding {
            segment_id: id,
            duration_ms: 1000,
            embedding: vec![id as f32; 256],
        };
        assert_ne!(
            temp_path(&embeddings_path(dir.path(), 1)),
            temp_path(&embeddings_path(dir.path(), 1))
        );
        std::thread::scope(|s| {
            for t in 0..4 {
                let dir = dir.path();
                s.spawn(move || {
                    for _ in 0..20 {
                        write_embeddings(dir, 1, &key, &[e(t)]).unwrap();
                    }
                });
            }
        });
        let back = read_embeddings(dir.path(), 1, &key).unwrap().unwrap();
        assert_eq!(back.len(), 1);
        let files: Vec<_> = std::fs::read_dir(dir.path().join("embeddings"))
            .unwrap()
            .map(|f| f.unwrap().file_name())
            .collect();
        assert_eq!(files, vec![std::ffi::OsString::from("1.bin")]);
    }

    #[test]
    fn the_embeddings_lock_is_exclusive_per_meeting() {
        let dir = tempfile::tempdir().unwrap();
        let first = lock_embeddings(dir.path(), 1);
        // Another meeting's lock is independent.
        drop(lock_embeddings(dir.path(), 2));
        let taken = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                let _second = lock_embeddings(dir.path(), 1);
                taken.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(!taken.load(std::sync::atomic::Ordering::SeqCst));
            drop(first);
        });
        assert!(taken.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn sidecars_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_cache(dir.path(), 1, &[7u8; 32], &ProcessingCache::new()).unwrap();
        let mode = std::fs::metadata(cache_path(dir.path(), 1))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn rejects_garbage() {
        assert!(open(&[0u8; 32], SidecarKind::Cache, 1, b"short").is_err());
        assert!(open(
            &[0u8; 32],
            SidecarKind::Cache,
            1,
            b"GDYSIDE2............garbage"
        )
        .is_err());
    }
}
