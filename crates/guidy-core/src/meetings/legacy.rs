//! One-time conversion of meeting data an earlier version encrypted with the
//! meetings data key (in the Keychain) into plain local data:
//!
//! - `meetings.db` (SQLCipher) is exported to a plain SQLite database;
//! - the encrypted sidecars (`meetings/{processing,embeddings}/<id>.bin`) are
//!   moved into its tables (see [`crate::meetings::processing::sidecar`]);
//! - each version 1 audio track (`meetings/audio/*.gopus`) is rewritten as
//!   version 2, packet for packet (see [`audio_store::convert_legacy_track`]).
//!
//! Every converted copy is checked before it replaces its original, and the
//! originals are moved (never deleted) into `meetings-encrypted-backup-<stamp>/`
//! under the app data dir. Once nothing encrypted is left, the key is kept in
//! the Keychain under `meetings_data_key.backup-<stamp>` and deleted; a
//! [`KEY_RETIRED_MARKER`] file in the backup folder records that.
//!
//! Each step looks at what is on disk, so a run interrupted anywhere (a
//! crash, a full disk) picks up where it stopped on the next start. With no
//! encrypted data and no unfinished backup folder, the Keychain isn't read;
//! only the first bytes of `meetings.db` and of each audio track are.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::meetings::audio_store;
use crate::meetings::processing::sidecar::{self, SegmentEmbedding, SidecarKind};
use crate::meetings::store::{self, MeetingsDbUndecryptable, MeetingsStore, MEETINGS_DB_FILE};
use crate::meetings::types::MeetingId;

/// Prefix of the backup folders (`<prefix><stamp>`) under the app data dir.
pub const BACKUP_PREFIX: &str = "meetings-encrypted-backup-";

/// In a backup folder: the key was retired, the conversion is complete.
pub const KEY_RETIRED_MARKER: &str = "KEY_RETIRED";

/// Suffix of a converted copy before it replaces its original.
const PLAIN_SUFFIX: &str = ".plain";

/// The SQLite file header; SQLCipher encrypts it like every other byte.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// What [`convert_if_needed`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub database: bool,
    pub checkpoints: usize,
    pub embeddings: usize,
    pub tracks: usize,
    /// The backup folder (when anything was converted or retired).
    pub backup: Option<PathBuf>,
    pub key_retired: bool,
}

/// Whether meeting data under `data_dir` is still encrypted (and needs the
/// key): `meetings.db`, a sidecar file, or a version 1 audio track.
pub fn has_encrypted_data(data_dir: &Path) -> Result<bool> {
    Ok(db_is_encrypted(&data_dir.join(MEETINGS_DB_FILE))?
        || !legacy_sidecars(data_dir)?.is_empty()
        || !legacy_tracks(data_dir)?.is_empty())
}

/// Convert whatever encrypted meeting data is left under `data_dir` (see the
/// module docs). `key` is asked for only when there is some; `retire_key`
/// (given the backup stamp) once none is left. A wrong key is
/// [`MeetingsDbUndecryptable`] or [`audio_store::WrongKey`] in the error chain,
/// a missing one whatever `key` returns (`MeetingsDataKeyMissing`).
pub fn convert_if_needed(
    data_dir: &Path,
    key: impl FnOnce() -> Result<[u8; 32]>,
    mut retire_key: impl FnMut(&str) -> Result<bool>,
) -> Result<Report> {
    let mut report = Report::default();
    finish_swaps(data_dir)?;
    if has_encrypted_data(data_dir)? {
        let key = key()?;
        let backup = backup_dir(data_dir)?;
        report.database = convert_database(data_dir, &backup, &key)?;
        (report.checkpoints, report.embeddings) = convert_sidecars(data_dir, &backup, &key)?;
        report.tracks = convert_tracks(data_dir, &backup, &key)?;
        report.backup = Some(backup);
    }
    // Nothing encrypted is left: retire the key of every unfinished backup.
    for backup in unfinished_backups(data_dir)? {
        let stamp = backup
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(BACKUP_PREFIX))
            .context("backup folder name")?
            .to_string();
        report.key_retired |= retire_key(&format!("backup-{stamp}"))?;
        std::fs::write(backup.join(KEY_RETIRED_MARKER), b"")
            .with_context(|| format!("mark {}", backup.display()))?;
        report.backup.get_or_insert(backup);
    }
    if report != Report::default() {
        log::info!("meetings: converted the encrypted meeting data: {report:?}");
    }
    Ok(report)
}

// ----- detection -------------------------------------------------------------

/// An existing, non-empty `meetings.db` that doesn't start with the SQLite
/// header.
fn db_is_encrypted(path: &Path) -> Result<bool> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
    };
    let mut head = Vec::with_capacity(16);
    file.by_ref()
        .take(16)
        .read_to_end(&mut head)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(!head.is_empty() && head.as_slice() != SQLITE_HEADER)
}

/// The files of `dir` (none when it doesn't exist), skipping hidden ones.
fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("list {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("list {}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// The legacy sidecar files: `(kind, meeting id, path)`.
fn legacy_sidecars(data_dir: &Path) -> Result<Vec<(SidecarKind, MeetingId, PathBuf)>> {
    let meetings_dir = data_dir.join("meetings");
    let mut out = Vec::new();
    for kind in [SidecarKind::Cache, SidecarKind::Embeddings] {
        for path in files(&meetings_dir.join(kind.legacy_dir_name()))? {
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".bin"))
                .and_then(|n| n.parse::<MeetingId>().ok());
            if let Some(id) = id {
                out.push((kind, id, path));
            }
        }
    }
    Ok(out)
}

/// The version 1 audio tracks.
fn legacy_tracks(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in files(&audio_store::audio_dir(data_dir))? {
        let is_track =
            path.extension().and_then(|x| x.to_str()) == Some(audio_store::FILE_EXTENSION);
        if is_track && audio_store::is_legacy_track(&path).unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(out)
}

// ----- backup folder -----------------------------------------------------------

/// Backup folders whose key isn't retired yet, oldest first.
fn unfinished_backups(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(data_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("list {}", data_dir.display())),
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(BACKUP_PREFIX))
                && !p.join(KEY_RETIRED_MARKER).exists()
        })
        .collect();
    out.sort();
    Ok(out)
}

/// The unfinished backup folder an interrupted run started, else a new one.
fn backup_dir(data_dir: &Path) -> Result<PathBuf> {
    if let Some(dir) = unfinished_backups(data_dir)?.pop() {
        return Ok(dir);
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut dir = data_dir.join(format!("{BACKUP_PREFIX}{stamp}"));
    let mut n = 1;
    while dir.exists() {
        n += 1;
        dir = data_dir.join(format!("{BACKUP_PREFIX}{stamp}-{n}"));
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// Move `src` to `rel` inside the backup folder (creating its parents).
fn back_up(src: &Path, backup: &Path, rel: &Path) -> Result<()> {
    let mut to = backup.join(rel);
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    // An earlier interrupted run backed up a file of the same name: keep both.
    let mut n = 1;
    while to.exists() {
        n += 1;
        to = backup.join(format!("{}.{n}", rel.display()));
    }
    std::fs::rename(src, &to).with_context(|| format!("move {} to {}", src.display(), to.display()))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("delete {}", path.display())),
    }
}

/// Put back converted copies a run stopped between backing up an original
/// and renaming its copy into place; delete copies that never got that far.
fn finish_swaps(data_dir: &Path) -> Result<()> {
    let mut candidates = vec![data_dir.join(MEETINGS_DB_FILE)];
    candidates.extend(
        files(&audio_store::audio_dir(data_dir))?
            .into_iter()
            .filter_map(|p| {
                p.to_str()
                    .and_then(|s| s.strip_suffix(PLAIN_SUFFIX))
                    .map(PathBuf::from)
            }),
    );
    for original in candidates {
        let copy = with_suffix(&original, PLAIN_SUFFIX);
        if !copy.exists() {
            continue;
        }
        if original.exists() {
            // The original wasn't backed up yet: the copy is redone.
            remove_if_exists(&copy)?;
            remove_if_exists(&with_suffix(&copy, "-journal"))?;
        } else {
            std::fs::rename(&copy, &original)
                .with_context(|| format!("rename {} to {}", copy.display(), original.display()))?;
        }
    }
    Ok(())
}

// ----- the database --------------------------------------------------------------

fn row_counts(conn: &Connection, schema: &str) -> Result<Vec<(String, i64)>> {
    let tables: Vec<String> = conn
        .prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' ORDER BY name"
        ))?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    tables
        .into_iter()
        .map(|t| {
            let n = conn.query_row(&format!("SELECT COUNT(*) FROM {schema}.\"{t}\""), [], |r| {
                r.get(0)
            })?;
            Ok((t, n))
        })
        .collect()
}

/// Export an encrypted `meetings.db` to a plain copy, check it holds the same
/// rows and schema version, back up the original and put the copy in its
/// place. Returns whether there was one to convert.
fn convert_database(data_dir: &Path, backup: &Path, key: &[u8; 32]) -> Result<bool> {
    let db = data_dir.join(MEETINGS_DB_FILE);
    if !db_is_encrypted(&db)? {
        return Ok(false);
    }
    let copy = with_suffix(&db, PLAIN_SUFFIX);
    remove_if_exists(&copy)?;
    {
        let conn = Connection::open(&db).with_context(|| format!("open {}", db.display()))?;
        store::apply_key(&conn, key)?;
        if let Err(e) = conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        }) {
            let wrong_key = e.sqlite_error_code() == Some(rusqlite::ErrorCode::NotADatabase);
            let e = anyhow::Error::new(e);
            return Err(if wrong_key {
                e.context(MeetingsDbUndecryptable)
            } else {
                e.context("read the encrypted meetings.db")
            });
        }
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        store::restrict_file(&copy)?;
        conn.execute(
            "ATTACH DATABASE ?1 AS plaintext KEY ''",
            [copy.to_str().context("non-UTF-8 data path")?],
        )
        .context("create the plain meetings.db")?;
        conn.query_row("SELECT sqlcipher_export('plaintext')", [], |_| Ok(()))
            .context("export meetings.db")?;
        // `sqlcipher_export` doesn't copy the schema version the
        // migrations go by.
        conn.execute_batch(&format!("PRAGMA plaintext.user_version = {version};"))?;
        let (before, after) = (row_counts(&conn, "main")?, row_counts(&conn, "plaintext")?);
        let copied: i64 = conn.query_row("PRAGMA plaintext.user_version", [], |r| r.get(0))?;
        conn.execute_batch("DETACH DATABASE plaintext;")?;
        if before != after || copied != version {
            bail!(
                "the plain copy of meetings.db doesn't match (rows {before:?} vs {after:?}, \
                 version {version} vs {copied})"
            );
        }
    }
    // A plain reader opens it (without a key) and finds it intact.
    {
        let check = Connection::open(&copy)?;
        let ok: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ok != "ok" {
            bail!("the plain copy of meetings.db failed its integrity check: {ok}");
        }
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let f = with_suffix(&db, suffix);
        if f.exists() {
            back_up(
                &f,
                backup,
                Path::new(&format!("{MEETINGS_DB_FILE}{suffix}")),
            )?;
        }
    }
    std::fs::rename(&copy, &db)
        .with_context(|| format!("rename {} to {}", copy.display(), db.display()))?;
    Ok(true)
}

// ----- sidecars --------------------------------------------------------------------

/// Move the legacy sidecars into the (plain) database, then their folders
/// into the backup. A sidecar that can't be read is skipped with a warning:
/// a checkpoint only restarts processing, missing per-segment voiceprints
/// make naming fall back to the speaker's centroid. Returns how many
/// checkpoints and embeddings files were moved in.
fn convert_sidecars(data_dir: &Path, backup: &Path, key: &[u8; 32]) -> Result<(usize, usize)> {
    let found = legacy_sidecars(data_dir)?;
    if found.is_empty() {
        return Ok((0, 0));
    }
    let store = MeetingsStore::open(&data_dir.join(MEETINGS_DB_FILE))?;
    let (mut checkpoints, mut embeddings) = (0, 0);
    for (kind, id, path) in &found {
        let json = match sidecar::read_legacy(path, key, *kind, *id) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("meetings: skipping an unreadable sidecar: {e:#}");
                continue;
            }
        };
        match kind {
            SidecarKind::Cache => {
                let json = String::from_utf8(json).context("checkpoint is not UTF-8")?;
                store.set_processing_checkpoint(*id, &json)?;
                checkpoints += 1;
            }
            SidecarKind::Embeddings => {
                let list: Vec<SegmentEmbedding> = match serde_json::from_slice(&json) {
                    Ok(l) => l,
                    Err(e) => {
                        log::warn!("meetings: skipping {}: {e}", path.display());
                        continue;
                    }
                };
                store.replace_segment_embeddings(*id, &list)?;
                embeddings += 1;
            }
        }
    }
    drop(store);
    let meetings_dir = data_dir.join("meetings");
    for kind in [SidecarKind::Cache, SidecarKind::Embeddings] {
        let dir = meetings_dir.join(kind.legacy_dir_name());
        if dir.exists() {
            back_up(&dir, backup, Path::new(kind.legacy_dir_name()))?;
        }
    }
    Ok((checkpoints, embeddings))
}

// ----- audio -----------------------------------------------------------------------

/// Rewrite each version 1 track as version 2 (checked by
/// [`audio_store::convert_legacy_track`]), back up the original and put the
/// copy in its place. Returns how many were converted.
fn convert_tracks(data_dir: &Path, backup: &Path, key: &[u8; 32]) -> Result<usize> {
    let tracks = legacy_tracks(data_dir)?;
    for track in &tracks {
        let copy = with_suffix(track, PLAIN_SUFFIX);
        remove_if_exists(&copy)?;
        if let Err(e) = audio_store::convert_legacy_track(track, &copy, key) {
            let _ = std::fs::remove_file(&copy);
            return Err(e.context(format!("convert {}", track.display())));
        }
        let name = track.file_name().context("track file name")?;
        back_up(track, backup, &Path::new("audio").join(name))?;
        std::fs::rename(&copy, track)
            .with_context(|| format!("rename {} to {}", copy.display(), track.display()))?;
    }
    Ok(tracks.len())
}

#[cfg(test)]
mod tests;
