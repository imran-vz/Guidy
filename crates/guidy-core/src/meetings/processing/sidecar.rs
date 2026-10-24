//! A meeting's processing state beside its transcript, in `meetings.db`:
//!
//! - the [`ProcessingCache`] checkpoint (VAD spans, transcripts, embeddings,
//!   clustering) so a cancelled or crashed run resumes. Deleted once
//!   processing is done.
//! - one voiceprint per embedded segment of the processed meeting
//!   ([`SegmentEmbedding`], keyed by the stored segment id). Naming a speaker
//!   copies the longest ones into `voiceprints`. Deleted with the meeting,
//!   never by audio retention.
//!
//! Both go with their meeting (foreign keys). Earlier versions kept them in
//! encrypted files (`meetings/{processing,embeddings}/<id>.bin`,
//! `"GDYSIDE2" || nonce (12) || ChaCha20-Poly1305(JSON)` keyed with the
//! meetings data key, AAD = `magic || kind || meeting id`); [`read_legacy`]
//! decrypts one for [`crate::meetings::legacy`] to move into the database.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};

use super::cluster::SpeakerMatch;
use super::timeline::RawSegment;
use super::vad::Span;
use crate::meetings::store::MeetingsStore;
pub use crate::meetings::types::SegmentEmbedding;
use crate::meetings::types::{MeetingId, PersonId, Track};

/// Held while a meeting's per-segment voiceprints are rewritten from
/// database state: by a processing run from replacing the transcript to
/// writing the new voiceprints, and by a person deletion from finding their
/// segments to dropping them. So a deletion never filters a stale copy over
/// a run's new voiceprints (or drops segment ids the new transcript reuses).
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

/// Wait for and take meeting `id`'s [`EmbeddingsLock`] (per meetings
/// directory, i.e. per database). Not reentrant; a caller taking several
/// takes them in ascending meeting order.
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

pub fn write_cache(store: &MeetingsStore, id: MeetingId, cache: &ProcessingCache) -> Result<()> {
    store.set_processing_checkpoint(id, &serde_json::to_string(cache)?)
}

/// Meeting `id`'s checkpoint; `Ok(None)` when it has none.
pub fn read_cache(store: &MeetingsStore, id: MeetingId) -> Result<Option<ProcessingCache>> {
    store
        .processing_checkpoint(id)?
        .map(|json| {
            serde_json::from_str(&json)
                .with_context(|| format!("meeting {id}: unreadable processing checkpoint"))
        })
        .transpose()
}

pub fn has_cache(store: &MeetingsStore, id: MeetingId) -> Result<bool> {
    Ok(store.processing_checkpoint(id)?.is_some())
}

pub fn delete_cache(store: &MeetingsStore, id: MeetingId) -> Result<()> {
    store.delete_processing_checkpoint(id)
}

/// Replace meeting `id`'s per-segment voiceprints.
pub fn write_embeddings(
    store: &MeetingsStore,
    id: MeetingId,
    embeddings: &[SegmentEmbedding],
) -> Result<()> {
    store.replace_segment_embeddings(id, embeddings)
}

/// Meeting `id`'s per-segment voiceprints; `Ok(None)` when it has none.
pub fn read_embeddings(
    store: &MeetingsStore,
    id: MeetingId,
) -> Result<Option<Vec<SegmentEmbedding>>> {
    let all = store.segment_embeddings(id)?;
    Ok((!all.is_empty()).then_some(all))
}

pub fn has_embeddings(store: &MeetingsStore, id: MeetingId) -> Result<bool> {
    Ok(!store.segment_embeddings(id)?.is_empty())
}

/// Remove the per-segment voiceprints of `segment_ids` from meeting `id`
/// (e.g. a deleted person's). Returns how many were removed. The caller
/// holds `lock` (taken before it read `segment_ids` from the database).
pub fn drop_segment_embeddings(
    lock: &EmbeddingsLock,
    store: &MeetingsStore,
    id: MeetingId,
    segment_ids: &HashSet<i64>,
) -> Result<usize> {
    debug_assert!(lock.key.1 == id);
    store.delete_segment_embeddings(id, Some(segment_ids))
}

/// Delete meeting `id`'s per-segment voiceprints. The caller holds its
/// [`EmbeddingsLock`].
pub fn delete_embeddings(store: &MeetingsStore, id: MeetingId) -> Result<()> {
    store.delete_segment_embeddings(id, None).map(|_| ())
}

/// Delete a meeting's checkpoint and per-segment voiceprints. Takes the
/// meeting's [`EmbeddingsLock`] (so the caller must not hold it): a person
/// deletion filtering the voiceprints can't write them back afterwards.
pub fn delete_sidecars(meetings_dir: &Path, store: &MeetingsStore, id: MeetingId) -> Result<()> {
    let _lock = lock_embeddings(meetings_dir, id);
    store.delete_processing_checkpoint(id)?;
    store.delete_segment_embeddings(id, None).map(|_| ())
}

// ----- legacy encrypted files ---------------------------------------------

const LEGACY_MAGIC: &[u8; 8] = b"GDYSIDE2";
const LEGACY_NONCE_LEN: usize = 12;

/// Which legacy sidecar a file is (bound into its AAD).
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

    /// The directory (under `meetings/`) its legacy files were in.
    pub fn legacy_dir_name(self) -> &'static str {
        match self {
            Self::Cache => "processing",
            Self::Embeddings => "embeddings",
        }
    }
}

fn legacy_aad(kind: SidecarKind, meeting_id: MeetingId) -> Vec<u8> {
    let mut a = Vec::with_capacity(LEGACY_MAGIC.len() + 9);
    a.extend_from_slice(LEGACY_MAGIC);
    a.push(kind.tag());
    a.extend_from_slice(&meeting_id.to_le_bytes());
    a
}

/// Where a legacy sidecar of meeting `id` was.
pub fn legacy_path(meetings_dir: &Path, kind: SidecarKind, id: MeetingId) -> PathBuf {
    meetings_dir
        .join(kind.legacy_dir_name())
        .join(format!("{id}.bin"))
}

/// The JSON inside a legacy encrypted sidecar of meeting `meeting_id`.
pub fn read_legacy(
    path: &Path,
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
) -> Result<Vec<u8>> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if data.len() < LEGACY_MAGIC.len() + LEGACY_NONCE_LEN
        || &data[..LEGACY_MAGIC.len()] != LEGACY_MAGIC
    {
        bail!(
            "{}: not a Guidy sidecar file (or an old format)",
            path.display()
        );
    }
    let (nonce, ct) = data[LEGACY_MAGIC.len()..].split_at(LEGACY_NONCE_LEN);
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: &legacy_aad(kind, meeting_id),
            },
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "{}: could not be decrypted (wrong key, meeting or damaged)",
                path.display()
            )
        })
}

/// A legacy encrypted sidecar of `json` (tests: files as earlier versions
/// wrote them).
#[cfg(test)]
pub(crate) fn seal_legacy(
    key: &[u8; 32],
    kind: SidecarKind,
    meeting_id: MeetingId,
    json: &[u8],
) -> Vec<u8> {
    let nonce = [5u8; LEGACY_NONCE_LEN];
    let ct = ChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: json,
                aad: &legacy_aad(kind, meeting_id),
            },
        )
        .expect("encrypt");
    [LEGACY_MAGIC.as_slice(), &nonce, &ct].concat()
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
    /// Per unit: the known person its own voiceprint clearly sounds like
    /// (see [`super::cluster::match_line`]); empty in older checkpoints.
    #[serde(default)]
    pub voices: Vec<Option<PersonId>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_meeting() -> (MeetingsStore, MeetingId) {
        let s = MeetingsStore::open_in_memory().unwrap();
        let id = s
            .insert_meeting(&crate::meetings::types::Meeting {
                id: 0,
                title: "m".into(),
                calendar_event_id: None,
                calendar_event: None,
                started_at: 0,
                ended_at: None,
                mic_audio_path: None,
                system_audio_path: None,
                status: crate::meetings::types::MeetingStatus::Processed,
                error: None,
                processing_stage: None,
                description: None,
                title_source: Default::default(),
                has_summary: false,
            })
            .unwrap();
        (s, id)
    }

    fn emb(segment_id: i64) -> SegmentEmbedding {
        SegmentEmbedding {
            segment_id,
            duration_ms: 1000 + segment_id,
            embedding: vec![segment_id as f32, 0.5, -1.25],
        }
    }

    #[test]
    fn the_checkpoint_round_trips_and_goes_with_its_meeting() {
        let (s, id) = store_with_meeting();
        assert!(read_cache(&s, id).unwrap().is_none());
        let mut cache = ProcessingCache::new();
        cache.mic_ms = Some(1234);
        write_cache(&s, id, &cache).unwrap();
        assert!(has_cache(&s, id).unwrap());
        assert_eq!(read_cache(&s, id).unwrap().unwrap().mic_ms, Some(1234));
        delete_cache(&s, id).unwrap();
        assert!(!has_cache(&s, id).unwrap());

        write_cache(&s, id, &cache).unwrap();
        s.delete_meeting(id).unwrap();
        assert!(!has_cache(&s, id).unwrap());
        // A meeting deleted meanwhile gets none.
        write_cache(&s, id, &cache).unwrap();
        assert!(!has_cache(&s, id).unwrap());
    }

    #[test]
    fn per_segment_voiceprints_are_replaced_and_dropped() {
        let (s, id) = store_with_meeting();
        let dir = tempfile::tempdir().unwrap();
        assert!(read_embeddings(&s, id).unwrap().is_none());
        write_embeddings(&s, id, &[emb(1), emb(2), emb(3)]).unwrap();
        write_embeddings(&s, id, &[emb(2), emb(3), emb(4)]).unwrap();
        assert_eq!(
            read_embeddings(&s, id).unwrap().unwrap(),
            [emb(2), emb(3), emb(4)]
        );
        let lock = lock_embeddings(dir.path(), id);
        let gone = drop_segment_embeddings(&lock, &s, id, &HashSet::from([3, 9])).unwrap();
        assert_eq!(gone, 1);
        drop(lock);
        assert_eq!(read_embeddings(&s, id).unwrap().unwrap(), [emb(2), emb(4)]);
        delete_sidecars(dir.path(), &s, id).unwrap();
        assert!(!has_embeddings(&s, id).unwrap());
        write_embeddings(&s, id, &[emb(1)]).unwrap();
        s.delete_meeting(id).unwrap();
        assert!(!has_embeddings(&s, id).unwrap());
    }

    #[test]
    fn delete_sidecars_waits_for_the_embeddings_lock() {
        let (s, id) = store_with_meeting();
        let dir = tempfile::tempdir().unwrap();
        write_embeddings(&s, id, &[emb(1)]).unwrap();
        let lock = lock_embeddings(dir.path(), id);
        std::thread::scope(|scope| {
            let deletion = scope.spawn(|| delete_sidecars(dir.path(), &s, id));
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(!deletion.is_finished());
            drop(lock);
            deletion.join().unwrap().unwrap();
        });
        assert!(!has_embeddings(&s, id).unwrap());
    }

    #[test]
    fn the_embeddings_lock_is_exclusive_per_meeting() {
        let dir = tempfile::tempdir().unwrap();
        let a = lock_embeddings(dir.path(), 1);
        // Another meeting's lock is free.
        drop(lock_embeddings(dir.path(), 2));
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| drop(lock_embeddings(dir.path(), 1)));
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(!waiting.is_finished());
            drop(a);
            waiting.join().unwrap();
        });
    }

    #[test]
    fn a_legacy_sidecar_is_bound_to_its_key_meeting_and_kind() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let path = legacy_path(dir.path(), SidecarKind::Embeddings, 3);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, seal_legacy(&key, SidecarKind::Embeddings, 3, b"[]")).unwrap();
        assert_eq!(
            read_legacy(&path, &key, SidecarKind::Embeddings, 3).unwrap(),
            b"[]"
        );
        assert!(read_legacy(&path, &[8u8; 32], SidecarKind::Embeddings, 3).is_err());
        assert!(read_legacy(&path, &key, SidecarKind::Embeddings, 4).is_err());
        assert!(read_legacy(&path, &key, SidecarKind::Cache, 3).is_err());
        std::fs::write(&path, b"garbage").unwrap();
        assert!(read_legacy(&path, &key, SidecarKind::Embeddings, 3).is_err());
    }
}
