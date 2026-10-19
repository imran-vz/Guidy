//! Encrypted `meetings.db` (SQLCipher). See docs/MEETING_NOTES_ARCH.md.
//!
//! The database is keyed with the 32-byte meetings data key from
//! [`crate::secrets::meetings_data_key`] (raw-key syntax, so SQLCipher skips
//! its passphrase KDF). Schema changes go through `rusqlite_migration`, like
//! `managers/history.rs`: append to [`MIGRATIONS`], never edit a shipped entry.
//!
//! Foreign keys:
//! - deleting a meeting deletes its segments, speakers, summary and action
//!   items; its voiceprints stay (`source_meeting_id` becomes NULL);
//! - deleting a person deletes their voiceprints and clears speaker links and
//!   action-item owners that pointed at them, and the `person_merges` rows
//!   that resolve people merged into them.
//!
//! Lists are ordered: meetings newest first, segments by time, speakers by id,
//! people by name, action items by id.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use rusqlite_migration::{Migrations, M};

use crate::linear::SentDraft;
use crate::meetings::types::*;

static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE people (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        display_name TEXT NOT NULL,
        email TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX people_email ON people(email COLLATE NOCASE);

    CREATE TABLE meetings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL,
        calendar_event_id TEXT,
        calendar_event TEXT,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        mic_audio_path TEXT,
        system_audio_path TEXT,
        status TEXT NOT NULL,
        error TEXT,
        processing_stage TEXT
    );
    CREATE INDEX meetings_started_at ON meetings(started_at);

    CREATE TABLE meeting_speakers (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        cluster_label TEXT NOT NULL,
        person_id INTEGER REFERENCES people(id) ON DELETE SET NULL,
        match_confidence REAL,
        confirmed INTEGER NOT NULL DEFAULT 0,
        is_user INTEGER NOT NULL DEFAULT 0,
        embedding BLOB
    );
    CREATE INDEX meeting_speakers_meeting ON meeting_speakers(meeting_id);
    CREATE INDEX meeting_speakers_person ON meeting_speakers(person_id);

    CREATE TABLE meeting_segments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        track TEXT NOT NULL,
        t0_ms INTEGER NOT NULL,
        t1_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        words TEXT NOT NULL DEFAULT '[]',
        speaker_id INTEGER REFERENCES meeting_speakers(id) ON DELETE SET NULL
    );
    CREATE INDEX meeting_segments_meeting ON meeting_segments(meeting_id, t0_ms);
    CREATE INDEX meeting_segments_speaker ON meeting_segments(speaker_id);

    CREATE TABLE voiceprints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
        embedding BLOB NOT NULL,
        source_meeting_id INTEGER REFERENCES meetings(id) ON DELETE SET NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX voiceprints_person ON voiceprints(person_id);
    CREATE INDEX voiceprints_meeting ON voiceprints(source_meeting_id);

    CREATE TABLE meeting_summaries (
        meeting_id INTEGER PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
        title TEXT NOT NULL,
        short_summary TEXT NOT NULL,
        decisions TEXT NOT NULL DEFAULT '[]',
        topics TEXT NOT NULL DEFAULT '[]',
        generated_by TEXT NOT NULL,
        generated_at INTEGER NOT NULL
    );

    CREATE TABLE action_items (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        text TEXT NOT NULL,
        owner_person_id INTEGER REFERENCES people(id) ON DELETE SET NULL,
        owner_name TEXT,
        owner_is_user INTEGER NOT NULL DEFAULT 0,
        due TEXT,
        source_quote TEXT NOT NULL DEFAULT '',
        source_t_ms INTEGER,
        ticket TEXT NOT NULL DEFAULT '{}',
        ticket_state TEXT NOT NULL DEFAULT 'draft',
        linear_issue_id TEXT,
        linear_issue_identifier TEXT,
        linear_issue_url TEXT
    );
    CREATE INDEX action_items_meeting ON action_items(meeting_id);
    CREATE INDEX action_items_owner ON action_items(owner_person_id);",
    ),
    // 2: action-item provenance and idempotent Linear creation.
    // - `manual`: added by the user (never replaced by a regenerated summary);
    // - `user_edited`: the user changed it (also kept on regenerate);
    // - `linear_client_id`: the UUID sent as `IssueCreateInput.id`, written
    //   before the create call so a retry (or a crash) can't duplicate.
    M::up(
        "ALTER TABLE action_items ADD COLUMN manual INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE action_items ADD COLUMN user_edited INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE action_items ADD COLUMN linear_client_id TEXT;
    UPDATE action_items SET manual = 1 WHERE trim(source_quote) = '';",
    ),
    // 3: the status and error a meeting had before it was queued for
    // processing, so a cancel (or a failed resume after a restart) can put
    // them back. NULL while not processing, and for a meeting a crash left
    // in `processing` before this column existed.
    M::up(
        "ALTER TABLE meetings ADD COLUMN status_before_processing TEXT;
    ALTER TABLE meetings ADD COLUMN error_before_processing TEXT;",
    ),
    // 4: the processing stage it had then too, so a cancelled reprocess
    // gets its `done` stage back. NULL like the columns of 3.
    M::up("ALTER TABLE meetings ADD COLUMN stage_before_processing TEXT;"),
    // 5: `linear_sent_ticket`: the ticket draft (JSON) the first create
    // attempt with `linear_client_id` sent, so a retry (also after a
    // restart) applies only the user's later edits to an issue that
    // attempt created. Cleared once the issue is recorded.
    M::up("ALTER TABLE action_items ADD COLUMN linear_sent_ticket TEXT;"),
    // 6: `person_merges`: whom each merged-away person went into, so a
    // speaker matched to them before the merge (a processing checkpoint)
    // links the merged person. A row goes with its target person; a chain
    // of merges is kept flat (every row points at a live person).
    M::up(
        "CREATE TABLE person_merges (
        from_id INTEGER PRIMARY KEY,
        into_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE
    );
    CREATE INDEX person_merges_into ON person_merges(into_id);",
    ),
    // 7: `user_voiceprints`: the user's own voice, recorded in People. Not a
    // person: with it, the mic track is diarized too (in-person meetings),
    // and only the speech that sounds like it is labelled as the user.
    M::up(
        "CREATE TABLE user_voiceprints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        embedding BLOB NOT NULL,
        created_at INTEGER NOT NULL
    );",
    ),
    // 8: the tokens and cost of the cloud LLM calls that produced a summary
    // (NULL `llm_calls`: a local model, or made before this column).
    M::up(
        "ALTER TABLE meeting_summaries ADD COLUMN llm_calls INTEGER;
    ALTER TABLE meeting_summaries ADD COLUMN input_tokens INTEGER;
    ALTER TABLE meeting_summaries ADD COLUMN output_tokens INTEGER;
    ALTER TABLE meeting_summaries ADD COLUMN cost_usd REAL;
    ALTER TABLE meeting_summaries ADD COLUMN cost_estimated INTEGER NOT NULL DEFAULT 0;",
    ),
];

/// File name of the database inside the app data dir.
pub const MEETINGS_DB_FILE: &str = "meetings.db";

/// Encode an embedding as little-endian f32 bytes (the `voiceprints.embedding`
/// BLOB format).
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Decode a little-endian f32 BLOB. Trailing bytes (not a multiple of 4) are
/// ignored.
pub fn decode_embedding(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

/// Parse a JSON column, mapping errors into rusqlite's error type so it can
/// be used inside row mappers.
fn from_json<T: serde::de::DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn bad_value(idx: usize, what: &str, v: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        idx,
        rusqlite::types::Type::Text,
        format!("unknown {what} '{v}'").into(),
    )
}

const MEETING_COLS: &str = "id, title, calendar_event_id, calendar_event, started_at, ended_at, \
     mic_audio_path, system_audio_path, status, error, processing_stage";

fn map_meeting(row: &Row<'_>) -> rusqlite::Result<Meeting> {
    let calendar_event: Option<String> = row.get(3)?;
    let status: String = row.get(8)?;
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        calendar_event_id: row.get(2)?,
        calendar_event: calendar_event
            .as_deref()
            .map(|s| from_json(3, s))
            .transpose()?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        mic_audio_path: row.get(6)?,
        system_audio_path: row.get(7)?,
        status: MeetingStatus::parse(&status).ok_or_else(|| bad_value(8, "status", &status))?,
        error: row.get(9)?,
        processing_stage: row.get(10)?,
    })
}

const SEGMENT_COLS: &str = "id, meeting_id, track, t0_ms, t1_ms, text, words, speaker_id";

fn map_segment(row: &Row<'_>) -> rusqlite::Result<Segment> {
    let track: String = row.get(2)?;
    let words: String = row.get(6)?;
    Ok(Segment {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        track: Track::parse(&track).ok_or_else(|| bad_value(2, "track", &track))?,
        t0_ms: row.get(3)?,
        t1_ms: row.get(4)?,
        text: row.get(5)?,
        words: from_json(6, &words)?,
        speaker_id: row.get(7)?,
    })
}

const SPEAKER_COLS: &str =
    "id, meeting_id, cluster_label, person_id, match_confidence, confirmed, is_user";

fn map_speaker(row: &Row<'_>) -> rusqlite::Result<MeetingSpeaker> {
    Ok(MeetingSpeaker {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        cluster_label: row.get(2)?,
        person_id: row.get(3)?,
        match_confidence: row.get::<_, Option<f64>>(4)?.map(|v| v as f32),
        confirmed: row.get(5)?,
        is_user: row.get(6)?,
    })
}

const PERSON_SELECT: &str = "SELECT p.id, p.display_name, p.email, p.created_at, \
     (SELECT COUNT(*) FROM voiceprints v WHERE v.person_id = p.id) FROM people p";

fn map_person(row: &Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get(0)?,
        display_name: row.get(1)?,
        email: row.get(2)?,
        created_at: row.get(3)?,
        voiceprint_count: row.get(4)?,
    })
}

const VOICEPRINT_COLS: &str = "id, person_id, embedding, source_meeting_id, created_at";

fn map_voiceprint(row: &Row<'_>) -> rusqlite::Result<Voiceprint> {
    let blob: Vec<u8> = row.get(2)?;
    Ok(Voiceprint {
        id: row.get(0)?,
        person_id: row.get(1)?,
        embedding: decode_embedding(&blob),
        source_meeting_id: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn map_summary(row: &Row<'_>) -> rusqlite::Result<StoredSummary> {
    let decisions: String = row.get(3)?;
    let topics: String = row.get(4)?;
    Ok(StoredSummary {
        meeting_id: row.get(0)?,
        title: row.get(1)?,
        short_summary: row.get(2)?,
        decisions: from_json(3, &decisions)?,
        topics: from_json(4, &topics)?,
        generated_by: row.get(5)?,
        generated_at: row.get(6)?,
        usage: row
            .get::<_, Option<i64>>(7)?
            .map(|calls| -> rusqlite::Result<_> {
                Ok(crate::llm::usage::MeterTotals {
                    calls: calls as u32,
                    input_tokens: row.get::<_, Option<i64>>(8)?.unwrap_or(0) as u64,
                    output_tokens: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
                    cost_usd: row.get(10)?,
                    estimated: row.get(11)?,
                })
            })
            .transpose()?,
    })
}

const ACTION_COLS: &str = "id, meeting_id, text, owner_person_id, owner_name, owner_is_user, due, \
     source_quote, source_t_ms, ticket, ticket_state, linear_issue_id, linear_issue_identifier, \
     linear_issue_url";

fn map_action_item(row: &Row<'_>) -> rusqlite::Result<ActionItem> {
    let ticket: String = row.get(9)?;
    let state: String = row.get(10)?;
    Ok(ActionItem {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        text: row.get(2)?,
        owner_person_id: row.get(3)?,
        owner_name: row.get(4)?,
        owner_is_user: row.get(5)?,
        due: row.get(6)?,
        source_quote: row.get(7)?,
        source_t_ms: row.get(8)?,
        ticket: from_json(9, &ticket)?,
        ticket_state: TicketState::parse(&state)
            .ok_or_else(|| bad_value(10, "ticket state", &state))?,
        linear_issue_id: row.get(11)?,
        linear_issue_identifier: row.get(12)?,
        linear_issue_url: row.get(13)?,
    })
}

/// Insert an action item. Items without a source quote are always manual
/// (the summarizer drops quote-less items).
fn insert_action_item_tx(
    conn: &Connection,
    item: &ActionItem,
    manual: bool,
) -> Result<ActionItemId> {
    let manual = manual || item.source_quote.trim().is_empty();
    conn.execute(
        "INSERT INTO action_items (meeting_id, text, owner_person_id, owner_name, owner_is_user,
            due, source_quote, source_t_ms, ticket, ticket_state, linear_issue_id,
            linear_issue_identifier, linear_issue_url, manual)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            item.meeting_id,
            item.text,
            item.owner_person_id,
            item.owner_name,
            item.owner_is_user,
            item.due,
            item.source_quote,
            item.source_t_ms,
            to_json(&item.ticket)?,
            item.ticket_state.as_str(),
            item.linear_issue_id,
            item.linear_issue_identifier,
            item.linear_issue_url,
            manual,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// A random RFC 4122 version-4 UUID (lowercase, hyphenated).
pub fn new_uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Create `path` (if missing) and restrict it to the owner (0600) on unix.
pub(crate) fn restrict_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("create {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Who person `id` is now: themselves while they exist, else whom they were
/// merged into (see [`MeetingsStore::merge_people`]), else `None` (deleted).
fn resolve_person_tx(conn: &Connection, id: PersonId) -> Result<Option<PersonId>> {
    Ok(conn
        .query_row(
            "SELECT id FROM people WHERE id = ?1
             UNION ALL SELECT into_id FROM person_merges WHERE from_id = ?1
             LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Segments attributed to `person_id` (through their meeting speakers),
/// grouped by meeting: `meeting id -> segment ids`.
fn person_segments_tx(
    conn: &Connection,
    person_id: PersonId,
) -> Result<HashMap<MeetingId, Vec<i64>>> {
    let mut stmt = conn.prepare(
        "SELECT g.meeting_id, g.id FROM meeting_segments g
         JOIN meeting_speakers s ON s.id = g.speaker_id
         WHERE s.person_id = ?1 ORDER BY g.meeting_id, g.id",
    )?;
    let mut out: HashMap<MeetingId, Vec<i64>> = HashMap::new();
    let rows = stmt.query_map([person_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    for row in rows {
        let (m, seg): (MeetingId, i64) = row?;
        out.entry(m).or_default().push(seg);
    }
    Ok(out)
}

fn delete_person_tx(conn: &Connection, id: PersonId) -> Result<bool> {
    conn.execute(
        "UPDATE meeting_speakers SET embedding = NULL WHERE person_id = ?1",
        [id],
    )?;
    Ok(conn.execute("DELETE FROM people WHERE id = ?1", [id])? > 0)
}

/// What [`MeetingsStore::delete_person_within`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonDeletion {
    /// Deleted (or already gone). Their segments, by meeting, as they were
    /// just before.
    Deleted(HashMap<MeetingId, Vec<i64>>),
    /// They are (now) in meetings outside the given set: nothing changed.
    /// Every meeting they are in.
    InMoreMeetings(BTreeSet<MeetingId>),
}

/// Why an action-item ticket change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketEditError {
    /// The Linear issue exists: its draft and state are frozen.
    AlreadyCreated,
    /// `Created` is only set by recording the issue ([`MeetingsStore::set_linear_issue`]).
    CannotSetCreated,
}

impl std::fmt::Display for TicketEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyCreated => f.write_str("the Linear issue for this item already exists"),
            Self::CannotSetCreated => {
                f.write_str("an item becomes Created only when its issue is recorded")
            }
        }
    }
}

impl std::error::Error for TicketEditError {}

/// `meetings.db` exists but the data key doesn't decrypt it (SQLite reads
/// it as "not a database"): the key is wrong. Other open failures (a busy
/// or locked file, I/O, a migration error) are not this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingsDbUndecryptable;

impl std::fmt::Display for MeetingsDbUndecryptable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("meetings.db could not be decrypted (wrong key?)")
    }
}

impl std::error::Error for MeetingsDbUndecryptable {}

/// The encrypted meetings database. Cheap to share as `Arc<MeetingsStore>`;
/// every call takes the connection lock for its duration.
pub struct MeetingsStore {
    conn: Mutex<Connection>,
}

impl MeetingsStore {
    /// Open (creating if needed) the database at `path`, keyed with `key`,
    /// and migrate it. Fails with [`MeetingsDbUndecryptable`] (in the error
    /// chain) if the file exists but `key` is wrong.
    pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        // Owner-only before SQLite opens it; its -journal/-wal files copy the
        // database file's mode.
        restrict_file(path)?;
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::init(conn, key)
    }

    /// A keyed in-memory database (tests).
    pub fn open_in_memory(key: &[u8; 32]) -> Result<Self> {
        Self::init(Connection::open_in_memory()?, key)
    }

    fn init(mut conn: Connection, key: &[u8; 32]) -> Result<Self> {
        apply_key(&conn, key)?;
        // Reading the schema is the first access that decrypts a page; a wrong
        // key fails here with "file is not a database".
        if let Err(e) = conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        }) {
            let undecryptable = e.sqlite_error_code() == Some(rusqlite::ErrorCode::NotADatabase);
            let e = anyhow::Error::new(e);
            return Err(if undecryptable {
                e.context(MeetingsDbUndecryptable)
            } else {
                e.context("read meetings.db")
            });
        }
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid meetings migrations");
        migrations
            .to_latest(&mut conn)
            .context("migrate meetings.db")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with_tx<R>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R>) -> Result<R> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let r = f(&tx)?;
        tx.commit()?;
        Ok(r)
    }

    // ----- meetings ---------------------------------------------------------

    /// Insert a meeting (its `id` is ignored). Returns the new id.
    pub fn insert_meeting(&self, m: &Meeting) -> Result<MeetingId> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO meetings (title, calendar_event_id, calendar_event, started_at, ended_at,
                mic_audio_path, system_audio_path, status, error, processing_stage)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                m.title,
                m.calendar_event_id,
                m.calendar_event.as_ref().map(to_json).transpose()?,
                m.started_at,
                m.ended_at,
                m.mic_audio_path,
                m.system_audio_path,
                m.status.as_str(),
                m.error,
                m.processing_stage,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Overwrite every column of the meeting row `m.id`. Tests only: a stale
    /// copy undoes concurrent changes. Use the targeted setters.
    #[cfg(test)]
    pub fn update_meeting(&self, m: &Meeting) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET title = ?2, calendar_event_id = ?3, calendar_event = ?4,
                started_at = ?5, ended_at = ?6, mic_audio_path = ?7, system_audio_path = ?8,
                status = ?9, error = ?10, processing_stage = ?11
             WHERE id = ?1",
            params![
                m.id,
                m.title,
                m.calendar_event_id,
                m.calendar_event.as_ref().map(to_json).transpose()?,
                m.started_at,
                m.ended_at,
                m.mic_audio_path,
                m.system_audio_path,
                m.status.as_str(),
                m.error,
                m.processing_stage,
            ],
        )?;
        expect_one(n, "meeting", m.id)
    }

    pub fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("SELECT {MEETING_COLS} FROM meetings WHERE id = ?1"),
                [id],
                map_meeting,
            )
            .optional()?)
    }

    /// All meetings, newest first.
    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        self.query_meetings(
            &format!("SELECT {MEETING_COLS} FROM meetings ORDER BY started_at DESC, id DESC"),
            [],
        )
    }

    /// Meetings in any of `statuses`, oldest first (e.g. to resume processing
    /// or to recover `Recording` rows left by a crash).
    pub fn meetings_with_status(&self, statuses: &[MeetingStatus]) -> Result<Vec<Meeting>> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let list = statuses
            .iter()
            .map(|s| format!("'{}'", s.as_str()))
            .collect::<Vec<_>>()
            .join(",");
        self.query_meetings(
            &format!(
                "SELECT {MEETING_COLS} FROM meetings WHERE status IN ({list})
                 ORDER BY started_at, id"
            ),
            [],
        )
    }

    /// Meetings that still have audio (either track) and ended before `ts`
    /// (unix ms), oldest first. Meetings still recording are never returned.
    pub fn meetings_with_audio_older_than(&self, ts: i64) -> Result<Vec<Meeting>> {
        self.query_meetings(
            &format!(
                "SELECT {MEETING_COLS} FROM meetings
                 WHERE (mic_audio_path IS NOT NULL OR system_audio_path IS NOT NULL)
                   AND status != 'recording'
                   AND COALESCE(ended_at, started_at) < ?1
                 ORDER BY started_at, id"
            ),
            [ts],
        )
    }

    /// Every non-null track path, of every meeting (retention's orphan
    /// sweep; reads only the two path columns).
    pub fn audio_paths(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT mic_audio_path FROM meetings WHERE mic_audio_path IS NOT NULL
             UNION ALL
             SELECT system_audio_path FROM meetings WHERE system_audio_path IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn query_meetings(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Meeting>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(p, map_meeting)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_meeting_title(&self, id: MeetingId, title: &str) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        expect_one(n, "meeting", id)
    }

    /// Set the status and error message (pass `None` to clear the error).
    pub fn set_meeting_status(
        &self,
        id: MeetingId,
        status: MeetingStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, status.as_str(), error],
        )?;
        expect_one(n, "meeting", id)
    }

    /// Record the last completed processing stage (`None` restarts from the
    /// beginning).
    pub fn set_processing_stage(
        &self,
        id: MeetingId,
        stage: Option<ProcessingStage>,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET processing_stage = ?2 WHERE id = ?1",
            params![id, stage.map(|s| s.as_str())],
        )?;
        expect_one(n, "meeting", id)
    }

    /// [`Self::set_processing_stage`], only while the meeting is
    /// `Processing`: a run that was cancelled (its meeting's status already
    /// restored) can't move the stage afterwards. Returns whether it changed.
    pub fn set_processing_stage_if_processing(
        &self,
        id: MeetingId,
        stage: Option<ProcessingStage>,
    ) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE meetings SET processing_stage = ?2 WHERE id = ?1 AND status = 'processing'",
            params![id, stage.map(|s| s.as_str())],
        )? > 0)
    }

    /// Set both audio paths (relative to `<app_data_dir>/meetings/audio/`).
    /// `None` marks a track as deleted.
    pub fn set_audio_paths(
        &self,
        id: MeetingId,
        mic: Option<&str>,
        system: Option<&str>,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET mic_audio_path = ?2, system_audio_path = ?3 WHERE id = ?1",
            params![id, mic, system],
        )?;
        expect_one(n, "meeting", id)
    }

    /// End a recording: `ended_at`, status `Recorded` and `error`, only while
    /// the row is still `Recording` (title and paths are untouched). Returns
    /// false when it wasn't recording.
    pub fn finish_recording(
        &self,
        id: MeetingId,
        ended_at: i64,
        error: Option<&str>,
    ) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE meetings SET ended_at = ?2, status = 'recorded', error = ?3
             WHERE id = ?1 AND status = 'recording'",
            params![id, ended_at, error],
        )? > 0)
    }

    /// Queue for processing: status `Processing` (error cleared) unless the
    /// meeting is recording or, with `needs_audio`, has no track left.
    /// Returns false (and changes nothing) otherwise. One statement, so it
    /// is atomic with [`Self::clear_audio_paths_if_processed`]: retention
    /// either sees the meeting `Processing` and keeps its audio, or clears
    /// the audio first and this refuses.
    ///
    /// The previous status, error and stage are kept for
    /// [`Self::restore_status_before_processing`]; a meeting already
    /// `Processing` (resumed after a crash) keeps the ones saved before.
    pub fn mark_processing(&self, id: MeetingId, needs_audio: bool) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE meetings SET
                status_before_processing = CASE WHEN status = 'processing'
                    THEN status_before_processing ELSE status END,
                error_before_processing = CASE WHEN status = 'processing'
                    THEN error_before_processing ELSE error END,
                stage_before_processing = CASE WHEN status = 'processing'
                    THEN stage_before_processing ELSE processing_stage END,
                status = 'processing', error = NULL
             WHERE id = ?1 AND status != 'recording'
               AND (NOT ?2 OR mic_audio_path IS NOT NULL OR system_audio_path IS NOT NULL)",
            params![id, needs_audio],
        )? > 0)
    }

    /// The status a `Processing` meeting had before [`Self::mark_processing`]
    /// (`None` when not processing, or for a crash leftover from before it
    /// was recorded).
    pub fn status_before_processing(&self, id: MeetingId) -> Result<Option<MeetingStatus>> {
        let v: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT status_before_processing FROM meetings
                 WHERE id = ?1 AND status = 'processing'",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(v.as_deref().and_then(MeetingStatus::parse))
    }

    /// Undo [`Self::mark_processing`] (processing cancelled, or it could not
    /// be resumed): back to the saved status and error, or `Recorded` (no
    /// error) when none was saved. Only while the meeting is `Processing`;
    /// returns whether it changed.
    ///
    /// A meeting that was `Processed` also gets its stage back (`done`), so
    /// a cancelled reprocess leaves it complete: retention can delete its
    /// audio, and processing it again starts over instead of resuming the
    /// cancelled run's partial checkpoint. Any other keeps the stage the run
    /// reached, so processing it again resumes.
    pub fn restore_status_before_processing(&self, id: MeetingId) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE meetings SET
                status = CASE WHEN status_before_processing IN ('recorded', 'processed', 'failed')
                    THEN status_before_processing ELSE 'recorded' END,
                error = CASE WHEN status_before_processing IN ('recorded', 'processed', 'failed')
                    THEN error_before_processing END,
                processing_stage = CASE WHEN status_before_processing = 'processed'
                    THEN stage_before_processing ELSE processing_stage END,
                status_before_processing = NULL, error_before_processing = NULL,
                stage_before_processing = NULL
             WHERE id = ?1 AND status = 'processing'",
            [id],
        )? > 0)
    }

    /// Record a processing outcome: status and error, dropping the status
    /// saved by [`Self::mark_processing`].
    pub fn finish_processing(
        &self,
        id: MeetingId,
        status: MeetingStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET status = ?2, error = ?3,
                status_before_processing = NULL, error_before_processing = NULL,
                stage_before_processing = NULL
             WHERE id = ?1",
            params![id, status.as_str(), error],
        )?;
        expect_one(n, "meeting", id)
    }

    /// Audio retention: clear both track paths only when the meeting is fully
    /// processed and neither recording nor processing. Returns false (and
    /// changes nothing) otherwise.
    pub fn clear_audio_paths_if_processed(&self, id: MeetingId) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE meetings SET mic_audio_path = NULL, system_audio_path = NULL
             WHERE id = ?1 AND status NOT IN ('recording', 'processing')
               AND processing_stage = 'done'",
            [id],
        )? > 0)
    }

    /// Link (or unlink) a calendar event, storing its snapshot (tests).
    #[cfg(test)]
    pub fn set_calendar_event(&self, id: MeetingId, event: Option<&CalendarEvent>) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meetings SET calendar_event_id = ?2, calendar_event = ?3 WHERE id = ?1",
            params![
                id,
                event.map(|e| e.id.clone()),
                event.map(to_json).transpose()?
            ],
        )?;
        expect_one(n, "meeting", id)
    }

    /// Delete a meeting with its segments, speakers, summary and action items.
    /// Voiceprints recorded from it stay. Returns false if it didn't exist.
    pub fn delete_meeting(&self, id: MeetingId) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .execute("DELETE FROM meetings WHERE id = ?1", [id])?
            > 0)
    }

    // ----- transcript: speakers + segments ---------------------------------

    /// Replace a meeting's speakers and segments in one transaction.
    ///
    /// `speakers[i].id` is a caller-chosen local id (e.g. the index) and each
    /// segment's `speaker_id` refers to one of those local ids; both are
    /// remapped to the new row ids. `meeting_id` fields are ignored. Returns
    /// the stored speakers and segments.
    ///
    /// A speaker's `person_id` is resolved in the same transaction (so a
    /// concurrent deletion or merge can't slip in between): a person merged
    /// away links whom they were merged into; a deleted one is dropped, and
    /// that speaker is stored as an unknown one (no match, unconfirmed).
    /// E.g. a processing checkpoint matched speakers before the change.
    pub fn replace_transcript(
        &self,
        meeting_id: MeetingId,
        speakers: &[MeetingSpeaker],
        segments: &[Segment],
    ) -> Result<(Vec<MeetingSpeaker>, Vec<Segment>)> {
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM meeting_segments WHERE meeting_id = ?1",
                [meeting_id],
            )?;
            tx.execute(
                "DELETE FROM meeting_speakers WHERE meeting_id = ?1",
                [meeting_id],
            )?;
            let mut ids: HashMap<SpeakerId, SpeakerId> = HashMap::new();
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO meeting_speakers (meeting_id, cluster_label, person_id,
                        match_confidence, confirmed, is_user)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for s in speakers {
                    let person = match s.person_id {
                        Some(p) => resolve_person_tx(tx, p)?,
                        None => None,
                    };
                    let (confidence, confirmed) = if person.is_none() && s.person_id.is_some() {
                        (None, false)
                    } else {
                        (s.match_confidence, s.confirmed)
                    };
                    stmt.execute(params![
                        meeting_id,
                        s.cluster_label,
                        person,
                        confidence.map(f64::from),
                        confirmed,
                        s.is_user,
                    ])?;
                    if ids.insert(s.id, tx.last_insert_rowid()).is_some() {
                        bail!("duplicate local speaker id {}", s.id);
                    }
                }
            }
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO meeting_segments (meeting_id, track, t0_ms, t1_ms, text, words,
                        speaker_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for seg in segments {
                    let speaker = seg
                        .speaker_id
                        .map(|local| {
                            ids.get(&local).copied().ok_or_else(|| {
                                anyhow!("segment refers to unknown local speaker id {local}")
                            })
                        })
                        .transpose()?;
                    stmt.execute(params![
                        meeting_id,
                        seg.track.as_str(),
                        seg.t0_ms,
                        seg.t1_ms,
                        seg.text,
                        to_json(&seg.words)?,
                        speaker,
                    ])?;
                }
            }
            Ok(())
        })?;
        Ok((self.speakers(meeting_id)?, self.segments(meeting_id)?))
    }

    /// A meeting's segments, by time.
    pub fn segments(&self, meeting_id: MeetingId) -> Result<Vec<Segment>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SEGMENT_COLS} FROM meeting_segments WHERE meeting_id = ?1
             ORDER BY t0_ms, id"
        ))?;
        let rows = stmt.query_map([meeting_id], map_segment)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A meeting's speakers, by id.
    pub fn speakers(&self, meeting_id: MeetingId) -> Result<Vec<MeetingSpeaker>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SPEAKER_COLS} FROM meeting_speakers WHERE meeting_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([meeting_id], map_speaker)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_speaker(&self, id: SpeakerId) -> Result<Option<MeetingSpeaker>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("SELECT {SPEAKER_COLS} FROM meeting_speakers WHERE id = ?1"),
                [id],
                map_speaker,
            )
            .optional()?)
    }

    /// Overwrite a speaker's label, person, confidence and flags.
    pub fn update_speaker(&self, s: &MeetingSpeaker) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meeting_speakers SET cluster_label = ?2, person_id = ?3,
                match_confidence = ?4, confirmed = ?5, is_user = ?6
             WHERE id = ?1",
            params![
                s.id,
                s.cluster_label,
                s.person_id,
                s.match_confidence.map(f64::from),
                s.confirmed,
                s.is_user
            ],
        )?;
        expect_one(n, "speaker", s.id)
    }

    /// Store a speaker's cluster centroid voiceprint (used to suggest names
    /// and to copy into `voiceprints` when the speaker is named).
    pub fn set_speaker_embedding(&self, id: SpeakerId, embedding: Option<&[f32]>) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE meeting_speakers SET embedding = ?2 WHERE id = ?1",
            params![id, embedding.map(encode_embedding)],
        )?;
        expect_one(n, "speaker", id)
    }

    pub fn speaker_embedding(&self, id: SpeakerId) -> Result<Option<Vec<f32>>> {
        let blob: Option<Option<Vec<u8>>> = self
            .conn
            .lock()
            .query_row(
                "SELECT embedding FROM meeting_speakers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(blob.flatten().map(|b| decode_embedding(&b)))
    }

    // ----- people -----------------------------------------------------------

    pub fn insert_person(&self, display_name: &str, email: Option<&str>) -> Result<Person> {
        let id = {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO people (display_name, email, created_at) VALUES (?1, ?2, ?3)",
                params![display_name, email, now_ms()],
            )?;
            conn.last_insert_rowid()
        };
        self.get_person(id)?
            .ok_or_else(|| anyhow!("person {id} vanished after insert"))
    }

    pub fn get_person(&self, id: PersonId) -> Result<Option<Person>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("{PERSON_SELECT} WHERE p.id = ?1"),
                [id],
                map_person,
            )
            .optional()?)
    }

    /// First person with this email (case-insensitive).
    pub fn find_person_by_email(&self, email: &str) -> Result<Option<Person>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("{PERSON_SELECT} WHERE p.email = ?1 COLLATE NOCASE ORDER BY p.id LIMIT 1"),
                [email],
                map_person,
            )
            .optional()?)
    }

    /// Everyone, by name, with their voiceprint counts.
    pub fn list_people(&self) -> Result<Vec<Person>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "{PERSON_SELECT} ORDER BY p.display_name COLLATE NOCASE, p.id"
        ))?;
        let rows = stmt.query_map([], map_person)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn update_person(
        &self,
        id: PersonId,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE people SET display_name = ?2, email = ?3 WHERE id = ?1",
            params![id, display_name, email],
        )?;
        expect_one(n, "person", id)
    }

    /// Delete a person and their voiceprints; speaker links and action-item
    /// owners pointing at them are cleared, and so are the cluster centroids
    /// (`meeting_speakers.embedding`) of the speakers linked to them, which
    /// are their voice too. Returns false if they didn't exist.
    ///
    /// Per-segment voiceprints live in the encrypted embeddings sidecars;
    /// [`Self::delete_person_within`] also returns the segments whose ones to
    /// drop.
    pub fn delete_person(&self, id: PersonId) -> Result<bool> {
        self.with_tx(|tx| delete_person_tx(tx, id))
    }

    /// [`Self::delete_person`], only if every meeting they are in (has a
    /// segment of theirs) is one of `meetings`, checked in the same
    /// transaction: a caller holding those meetings' sidecar locks knows no
    /// other meeting has segments of theirs when they are gone.
    pub fn delete_person_within(
        &self,
        id: PersonId,
        meetings: &BTreeSet<MeetingId>,
    ) -> Result<PersonDeletion> {
        self.with_tx(|tx| {
            let segments = person_segments_tx(tx, id)?;
            if !segments.keys().all(|m| meetings.contains(m)) {
                return Ok(PersonDeletion::InMoreMeetings(
                    segments.into_keys().collect(),
                ));
            }
            delete_person_tx(tx, id)?;
            Ok(PersonDeletion::Deleted(segments))
        })
    }

    /// Segments attributed to `person_id` (through their meeting speakers),
    /// grouped by meeting: `meeting id -> segment ids`.
    pub fn person_segments(&self, person_id: PersonId) -> Result<HashMap<MeetingId, Vec<i64>>> {
        person_segments_tx(&self.conn.lock(), person_id)
    }

    /// Move voiceprints, speaker links and action-item owners from `from` to
    /// `into` (which keeps its name; it takes `from`'s email if it has none),
    /// then delete `from`. One transaction. `from` is recorded as merged
    /// into `into` (see [`Self::replace_transcript`]).
    pub fn merge_people(&self, from: PersonId, into: PersonId) -> Result<()> {
        if from == into {
            bail!("cannot merge a person into themselves");
        }
        self.with_tx(|tx| {
            let exists = |id: PersonId| -> Result<bool> {
                Ok(tx
                    .query_row("SELECT 1 FROM people WHERE id = ?1", [id], |_| Ok(()))
                    .optional()?
                    .is_some())
            };
            if !exists(from)? {
                bail!("person {from} not found");
            }
            if !exists(into)? {
                bail!("person {into} not found");
            }
            tx.execute(
                "UPDATE voiceprints SET person_id = ?2 WHERE person_id = ?1",
                params![from, into],
            )?;
            tx.execute(
                "UPDATE meeting_speakers SET person_id = ?2 WHERE person_id = ?1",
                params![from, into],
            )?;
            tx.execute(
                "UPDATE action_items SET owner_person_id = ?2 WHERE owner_person_id = ?1",
                params![from, into],
            )?;
            tx.execute(
                "UPDATE people SET email = (SELECT email FROM people WHERE id = ?1)
                 WHERE id = ?2 AND (email IS NULL OR email = '')",
                params![from, into],
            )?;
            // Before `from` goes (its rows would cascade): those merged into
            // `from` now resolve to `into`.
            tx.execute(
                "UPDATE person_merges SET into_id = ?2 WHERE into_id = ?1",
                params![from, into],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO person_merges (from_id, into_id) VALUES (?1, ?2)",
                params![from, into],
            )?;
            tx.execute("DELETE FROM people WHERE id = ?1", [from])?;
            Ok(())
        })
    }

    // ----- voiceprints ------------------------------------------------------

    pub fn add_voiceprint(
        &self,
        person_id: PersonId,
        embedding: &[f32],
        source_meeting_id: Option<MeetingId>,
    ) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO voiceprints (person_id, embedding, source_meeting_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                person_id,
                encode_embedding(embedding),
                source_meeting_id,
                now_ms()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Every stored voiceprint (for matching), by id.
    pub fn voiceprints(&self) -> Result<Vec<Voiceprint>> {
        self.query_voiceprints(
            &format!("SELECT {VOICEPRINT_COLS} FROM voiceprints ORDER BY id"),
            [],
        )
    }

    pub fn voiceprints_for_person(&self, person_id: PersonId) -> Result<Vec<Voiceprint>> {
        self.query_voiceprints(
            &format!("SELECT {VOICEPRINT_COLS} FROM voiceprints WHERE person_id = ?1 ORDER BY id"),
            [person_id],
        )
    }

    fn query_voiceprints(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Voiceprint>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(p, map_voiceprint)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_voiceprint(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .execute("DELETE FROM voiceprints WHERE id = ?1", [id])?
            > 0)
    }

    // ----- the user's voice -------------------------------------------------

    /// Replace the user's recorded voiceprints with `prints`.
    pub fn replace_user_voiceprints(&self, prints: &[Vec<f32>]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM user_voiceprints", [])?;
        let created_at = now_ms();
        for p in prints {
            tx.execute(
                "INSERT INTO user_voiceprints (embedding, created_at) VALUES (?1, ?2)",
                params![encode_embedding(p), created_at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The user's recorded voiceprints, by id (empty when none was recorded).
    pub fn user_voiceprints(&self) -> Result<Vec<Vec<f32>>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT embedding FROM user_voiceprints ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        Ok(rows
            .map(|b| b.map(|b| decode_embedding(&b)))
            .collect::<rusqlite::Result<_>>()?)
    }

    /// When the user's voice was recorded (ms), `None` when it wasn't.
    pub fn user_voice_recorded_at(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .lock()
            .query_row("SELECT MAX(created_at) FROM user_voiceprints", [], |r| {
                r.get(0)
            })?)
    }

    // ----- summaries --------------------------------------------------------

    pub fn summary(&self, meeting_id: MeetingId) -> Result<Option<StoredSummary>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT meeting_id, title, short_summary, decisions, topics, generated_by,
                    generated_at, llm_calls, input_tokens, output_tokens, cost_usd,
                    cost_estimated
                 FROM meeting_summaries WHERE meeting_id = ?1",
                [meeting_id],
                map_summary,
            )
            .optional()?)
    }

    /// Insert or replace the meeting's summary.
    pub fn put_summary(&self, s: &StoredSummary) -> Result<()> {
        put_summary_tx(&self.conn.lock(), s)
    }

    /// Store a regenerated summary: [`Self::put_summary`] and
    /// [`Self::replace_unlinked_action_items`] in one transaction, so a
    /// failure leaves both the old summary and the old items in place.
    /// Returns all the meeting's items.
    pub fn put_summary_and_action_items(
        &self,
        s: &StoredSummary,
        items: &[ActionItem],
        keep: &[ActionItemId],
        is_duplicate: impl Fn(&ActionItem, &ActionItem) -> bool,
    ) -> Result<Vec<ActionItem>> {
        self.with_tx(|tx| {
            put_summary_tx(tx, s)?;
            replace_unlinked_action_items_tx(tx, s.meeting_id, items, keep, is_duplicate)
        })?;
        self.action_items(s.meeting_id)
    }

    // ----- action items -----------------------------------------------------

    /// A meeting's action items, by id (creation order).
    pub fn action_items(&self, meeting_id: MeetingId) -> Result<Vec<ActionItem>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {ACTION_COLS} FROM action_items WHERE meeting_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([meeting_id], map_action_item)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_action_item(&self, id: ActionItemId) -> Result<Option<ActionItem>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("SELECT {ACTION_COLS} FROM action_items WHERE id = ?1"),
                [id],
                map_action_item,
            )
            .optional()?)
    }

    /// Insert an action item (its `id` is ignored). Returns the stored item.
    /// An item without a source quote is stored as manual.
    pub fn insert_action_item(&self, item: &ActionItem) -> Result<ActionItem> {
        let id = insert_action_item_tx(&self.conn.lock(), item, false)?;
        self.get_action_item(id)?
            .ok_or_else(|| anyhow!("action item {id} vanished after insert"))
    }

    /// Insert an item the user added by hand. Manual items are never replaced
    /// by a regenerated summary.
    pub fn insert_manual_action_item(&self, item: &ActionItem) -> Result<ActionItem> {
        let id = insert_action_item_tx(&self.conn.lock(), item, true)?;
        self.get_action_item(id)?
            .ok_or_else(|| anyhow!("action item {id} vanished after insert"))
    }

    /// Save the user-editable item fields of `item.id`: text, owner (person,
    /// name, is-user), due date, source quote and its timestamp. The ticket
    /// draft, ticket state and Linear link are never touched here (see
    /// [`Self::update_ticket_draft`], [`Self::set_ticket_state`],
    /// [`Self::set_linear_issue`]), so a stale copy can't unlink an issue.
    /// Marks the item user-edited when something changed.
    pub fn update_action_item(&self, item: &ActionItem) -> Result<()> {
        self.with_tx(|tx| {
            let current = tx
                .query_row(
                    &format!("SELECT {ACTION_COLS} FROM action_items WHERE id = ?1"),
                    [item.id],
                    map_action_item,
                )
                .optional()?
                .ok_or_else(|| anyhow!("action item {} not found", item.id))?;
            let changed = current.text != item.text
                || current.owner_person_id != item.owner_person_id
                || current.owner_name != item.owner_name
                || current.owner_is_user != item.owner_is_user
                || current.due != item.due
                || current.source_quote != item.source_quote
                || current.source_t_ms != item.source_t_ms;
            if !changed {
                return Ok(());
            }
            tx.execute(
                "UPDATE action_items SET text = ?2, owner_person_id = ?3, owner_name = ?4,
                    owner_is_user = ?5, due = ?6, source_quote = ?7, source_t_ms = ?8,
                    user_edited = 1
                 WHERE id = ?1",
                params![
                    item.id,
                    item.text,
                    item.owner_person_id,
                    item.owner_name,
                    item.owner_is_user,
                    item.due,
                    item.source_quote,
                    item.source_t_ms,
                ],
            )?;
            Ok(())
        })
    }

    /// Replace the ticket draft. Refused ([`TicketEditError::AlreadyCreated`])
    /// once the item has a Linear issue.
    pub fn update_ticket_draft(&self, id: ActionItemId, draft: &TicketDraft) -> Result<()> {
        let json = to_json(draft)?;
        self.with_tx(|tx| {
            ensure_not_created(tx, id)?;
            tx.execute(
                "UPDATE action_items SET ticket = ?2 WHERE id = ?1",
                params![id, json],
            )?;
            Ok(())
        })
    }

    /// Set the ticket state (draft / approved / discarded). Refused for an
    /// item whose issue exists, and for `Created` itself (only
    /// [`Self::set_linear_issue`] sets that).
    pub fn set_ticket_state(&self, id: ActionItemId, state: TicketState) -> Result<()> {
        if state == TicketState::Created {
            return Err(TicketEditError::CannotSetCreated.into());
        }
        self.with_tx(|tx| {
            ensure_not_created(tx, id)?;
            tx.execute(
                "UPDATE action_items SET ticket_state = ?2 WHERE id = ?1",
                params![id, state.as_str()],
            )?;
            Ok(())
        })
    }

    /// Mark an item as changed by the user (kept when the summary is
    /// regenerated).
    pub fn mark_action_item_user_edited(&self, id: ActionItemId) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE action_items SET user_edited = 1 WHERE id = ?1",
            [id],
        )?;
        expect_one(n, "action item", id)
    }

    /// The item's provenance flags: `(manual, user_edited)`.
    pub fn action_item_flags(&self, id: ActionItemId) -> Result<Option<(bool, bool)>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT manual, user_edited FROM action_items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// The UUID this item's Linear issue is (or will be) created with,
    /// generating and persisting one first if needed. Persisting it before
    /// the create call is the "creating" state: a crash or timeout after it
    /// leaves an id that a retry reuses, so Linear rejects the duplicate.
    /// Call it only right before sending, with a draft that passed
    /// validation; if the request then never reaches Linear, undo it with
    /// [`Self::drop_unsent_linear_client_id`].
    ///
    /// Also returns what an earlier attempt with this id sent. Without an
    /// earlier attempt, `draft` (what this attempt sends) is recorded as
    /// that and [`SentDraft::First`] returned. Only the first attempt's draft
    /// is kept (a later failed one may not have reached Linear). An id
    /// without a readable sent draft (an attempt before migration 5, or a
    /// corrupt record) gives [`SentDraft::Unknown`], every time: `draft` is
    /// not what that attempt sent, so it is not recorded as such.
    pub fn ensure_linear_client_id(
        &self,
        id: ActionItemId,
        draft: &TicketDraft,
    ) -> Result<(String, SentDraft)> {
        let json = to_json(draft)?;
        self.with_tx(|tx| {
            let current: Option<(Option<String>, Option<String>)> = tx
                .query_row(
                    "SELECT linear_client_id, linear_sent_ticket FROM action_items WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((client_id, sent)) = current else {
                bail!("action item {id} not found");
            };
            if let Some(client_id) = client_id.filter(|c| !c.is_empty()) {
                let sent = match sent.map(|s| serde_json::from_str::<TicketDraft>(&s)) {
                    Some(Ok(d)) => SentDraft::Earlier(d),
                    Some(Err(e)) => {
                        log::warn!("action item {id}: unreadable sent ticket ({e})");
                        SentDraft::Unknown
                    }
                    None => SentDraft::Unknown,
                };
                return Ok((client_id, sent));
            }
            let client_id = new_uuid_v4();
            tx.execute(
                "UPDATE action_items SET linear_client_id = ?2, linear_sent_ticket = ?3
                 WHERE id = ?1",
                params![id, client_id, json],
            )?;
            Ok((client_id, SentDraft::First))
        })
    }

    /// Undo an [`Self::ensure_linear_client_id`] that generated `client_id`
    /// when the request it was for never reached Linear: drop the client id
    /// and the recorded sent draft, so the item isn't kept as "creating" and
    /// the unsent draft isn't a baseline. A no-op if the item has another
    /// client id or an issue by now.
    pub fn drop_unsent_linear_client_id(&self, id: ActionItemId, client_id: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE action_items SET linear_client_id = NULL, linear_sent_ticket = NULL
             WHERE id = ?1 AND linear_client_id = ?2 AND linear_issue_id IS NULL",
            params![id, client_id],
        )?;
        Ok(())
    }

    /// Test hook: run raw SQL (e.g. a `TEMP TRIGGER` that makes a later
    /// statement fail).
    #[cfg(test)]
    pub(crate) fn execute_batch_for_tests(&self, sql: &str) -> Result<()> {
        Ok(self.conn.lock().execute_batch(sql)?)
    }

    /// Test hook: drop the item's sent draft, as an attempt made before
    /// migration 5 left it.
    #[cfg(test)]
    pub(crate) fn forget_linear_sent_ticket(&self, id: ActionItemId) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE action_items SET linear_sent_ticket = NULL WHERE id = ?1",
            [id],
        )?;
        expect_one(n, "action item", id)
    }

    pub fn linear_client_id(&self, id: ActionItemId) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT linear_client_id FROM action_items WHERE id = ?1",
                [id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn delete_action_item(&self, id: ActionItemId) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .execute("DELETE FROM action_items WHERE id = ?1", [id])?
            > 0)
    }

    /// Replace the meeting's regenerable action items with `items` (their
    /// `id`/`meeting_id` are ignored), in one transaction. Returns all items.
    ///
    /// Kept (never deleted here): items with a Linear issue, items whose
    /// creation was attempted (`linear_client_id` set), `Approved` items,
    /// manual items, items the user edited, and the ids in `keep` (e.g. the
    /// in-flight create guard). New items that `is_duplicate` of a kept item
    /// are skipped.
    pub fn replace_unlinked_action_items(
        &self,
        meeting_id: MeetingId,
        items: &[ActionItem],
        keep: &[ActionItemId],
        is_duplicate: impl Fn(&ActionItem, &ActionItem) -> bool,
    ) -> Result<Vec<ActionItem>> {
        self.with_tx(|tx| {
            replace_unlinked_action_items_tx(tx, meeting_id, items, keep, is_duplicate)
        })?;
        self.action_items(meeting_id)
    }

    /// Record the created Linear issue on an item and mark it `Created`.
    pub fn set_linear_issue(
        &self,
        id: ActionItemId,
        issue_id: &str,
        identifier: &str,
        url: &str,
    ) -> Result<()> {
        let n = self.conn.lock().execute(
            "UPDATE action_items SET linear_issue_id = ?2, linear_issue_identifier = ?3,
                linear_issue_url = ?4, ticket_state = ?5, linear_sent_ticket = NULL
             WHERE id = ?1",
            params![id, issue_id, identifier, url, TicketState::Created.as_str()],
        )?;
        expect_one(n, "action item", id)
    }
}

/// SQL truth value: the item must survive a regenerated summary.
const PRESERVED: &str = "(linear_issue_id IS NOT NULL OR linear_client_id IS NOT NULL \
     OR ticket_state IN ('approved', 'created') OR manual != 0 OR user_edited != 0)";
const ACTION_COL_COUNT: usize = 14;

/// [`MeetingsStore::replace_unlinked_action_items`] inside `tx`.
fn replace_unlinked_action_items_tx(
    tx: &Transaction<'_>,
    meeting_id: MeetingId,
    items: &[ActionItem],
    keep: &[ActionItemId],
    is_duplicate: impl Fn(&ActionItem, &ActionItem) -> bool,
) -> Result<()> {
    let kept: Vec<ActionItem> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT {ACTION_COLS}, {PRESERVED} FROM action_items
             WHERE meeting_id = ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map([meeting_id], |r| {
            Ok((map_action_item(r)?, r.get::<_, bool>(ACTION_COL_COUNT)?))
        })?;
        let mut kept = Vec::new();
        for row in rows {
            let (item, preserved) = row?;
            if preserved || keep.contains(&item.id) {
                kept.push(item);
            } else {
                tx.execute("DELETE FROM action_items WHERE id = ?1", [item.id])?;
            }
        }
        kept
    };
    for item in items {
        let mut item = item.clone();
        item.meeting_id = meeting_id;
        // The owner was picked from a people snapshot taken before the (slow)
        // summary ran; follow merges and drop deleted people so the FK holds.
        if let Some(owner) = item.owner_person_id {
            item.owner_person_id = resolve_person_tx(tx, owner)?;
        }
        if kept.iter().any(|k| is_duplicate(k, &item)) {
            continue;
        }
        insert_action_item_tx(tx, &item, false)?;
    }
    Ok(())
}

fn put_summary_tx(conn: &Connection, s: &StoredSummary) -> Result<()> {
    conn.execute(
        "INSERT INTO meeting_summaries (meeting_id, title, short_summary, decisions, topics,
            generated_by, generated_at, llm_calls, input_tokens, output_tokens, cost_usd,
            cost_estimated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(meeting_id) DO UPDATE SET title = excluded.title,
            short_summary = excluded.short_summary, decisions = excluded.decisions,
            topics = excluded.topics, generated_by = excluded.generated_by,
            generated_at = excluded.generated_at, llm_calls = excluded.llm_calls,
            input_tokens = excluded.input_tokens, output_tokens = excluded.output_tokens,
            cost_usd = excluded.cost_usd, cost_estimated = excluded.cost_estimated",
        params![
            s.meeting_id,
            s.title,
            s.short_summary,
            to_json(&s.decisions)?,
            to_json(&s.topics)?,
            s.generated_by,
            s.generated_at,
            s.usage.map(|u| u.calls as i64),
            s.usage.map(|u| u.input_tokens as i64),
            s.usage.map(|u| u.output_tokens as i64),
            s.usage.and_then(|u| u.cost_usd),
            s.usage.is_some_and(|u| u.estimated),
        ],
    )?;
    Ok(())
}

fn ensure_not_created(tx: &Transaction<'_>, id: ActionItemId) -> Result<()> {
    let row: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT ticket_state, linear_issue_id FROM action_items WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => bail!("action item {id} not found"),
        Some((state, issue)) if state == TicketState::Created.as_str() || issue.is_some() => {
            Err(TicketEditError::AlreadyCreated.into())
        }
        Some(_) => Ok(()),
    }
}

/// Apply the SQLCipher key as a raw 256-bit key (`x'<64 hex>'`).
fn apply_key(conn: &Connection, key: &[u8; 32]) -> Result<()> {
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .context("set the meetings.db key")?;
    Ok(())
}

fn expect_one(n: usize, what: &str, id: i64) -> Result<()> {
    if n == 0 {
        bail!("{what} {id} not found");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];

    fn store() -> MeetingsStore {
        MeetingsStore::open_in_memory(&KEY).unwrap()
    }

    fn meeting(title: &str, started_at: i64) -> Meeting {
        Meeting {
            id: 0,
            title: title.into(),
            calendar_event_id: None,
            calendar_event: None,
            started_at,
            ended_at: Some(started_at + 60_000),
            mic_audio_path: Some(format!("{title}-mic.opus.enc")),
            system_audio_path: Some(format!("{title}-sys.opus.enc")),
            status: MeetingStatus::Recorded,
            error: None,
            processing_stage: None,
        }
    }

    fn speaker(local: i64, label: &str, person: Option<PersonId>) -> MeetingSpeaker {
        MeetingSpeaker {
            id: local,
            meeting_id: 0,
            cluster_label: label.into(),
            person_id: person,
            match_confidence: Some(0.5),
            confirmed: false,
            is_user: label == "me",
        }
    }

    fn segment(t0: i64, speaker: Option<i64>, track: Track) -> Segment {
        Segment {
            id: 0,
            meeting_id: 0,
            track,
            t0_ms: t0,
            t1_ms: t0 + 1000,
            text: format!("at {t0}"),
            words: vec![Word {
                t0_ms: t0,
                t1_ms: t0 + 500,
                text: "at".into(),
            }],
            speaker_id: speaker,
        }
    }

    fn item(meeting_id: MeetingId, text: &str) -> ActionItem {
        ActionItem {
            id: 0,
            meeting_id,
            text: text.into(),
            owner_person_id: None,
            owner_name: Some("me".into()),
            owner_is_user: true,
            due: Some("2026-11-01".into()),
            source_quote: "I'll do it".into(),
            source_t_ms: Some(1234),
            ticket: TicketDraft {
                title: text.into(),
                priority: Some(2),
                label_ids: vec!["l1".into()],
                ..Default::default()
            },
            ticket_state: TicketState::Draft,
            linear_issue_id: None,
            linear_issue_identifier: None,
            linear_issue_url: None,
        }
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn store_is_send_and_sync() {
        assert_send_sync::<MeetingsStore>();
    }

    #[test]
    fn meeting_round_trip_and_updates() {
        let s = store();
        let mut m = meeting("Weekly", 1_000);
        m.calendar_event = Some(CalendarEvent {
            id: "evt-1".into(),
            title: "Weekly".into(),
            start_at: 1_000,
            end_at: 2_000,
            meeting_url: Some("https://meet.google.com/abc".into()),
            attendees: vec![Attendee {
                name: Some("Ann".into()),
                email: Some("ann@example.com".into()),
                is_current_user: false,
            }],
        });
        m.calendar_event_id = Some("evt-1".into());
        let id = s.insert_meeting(&m).unwrap();
        m.id = id;
        assert_eq!(s.get_meeting(id).unwrap(), Some(m.clone()));

        s.set_meeting_title(id, "Renamed").unwrap();
        s.set_meeting_status(id, MeetingStatus::Failed, Some("boom"))
            .unwrap();
        s.set_processing_stage(id, Some(ProcessingStage::Vad))
            .unwrap();
        s.set_audio_paths(id, None, Some("x")).unwrap();
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.title, "Renamed");
        assert_eq!(got.status, MeetingStatus::Failed);
        assert_eq!(got.error.as_deref(), Some("boom"));
        assert_eq!(got.processing_stage.as_deref(), Some("vad"));
        assert_eq!(got.mic_audio_path, None);
        assert_eq!(got.system_audio_path.as_deref(), Some("x"));

        s.set_calendar_event(id, None).unwrap();
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.calendar_event, None);
        assert_eq!(got.calendar_event_id, None);

        let mut full = got.clone();
        full.title = "Full".into();
        full.status = MeetingStatus::Processed;
        s.update_meeting(&full).unwrap();
        assert_eq!(s.get_meeting(id).unwrap(), Some(full));

        assert!(s.set_meeting_title(999, "x").is_err());
        assert_eq!(s.get_meeting(999).unwrap(), None);
    }

    #[test]
    fn list_and_status_queries() {
        let s = store();
        let a = s.insert_meeting(&meeting("a", 100)).unwrap();
        let b = s.insert_meeting(&meeting("b", 300)).unwrap();
        let mut rec = meeting("c", 200);
        rec.status = MeetingStatus::Recording;
        rec.ended_at = None;
        let c = s.insert_meeting(&rec).unwrap();

        let ids: Vec<_> = s.list_meetings().unwrap().iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![b, c, a]);

        let rec: Vec<_> = s
            .meetings_with_status(&[MeetingStatus::Recording])
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(rec, vec![c]);
        assert!(s.meetings_with_status(&[]).unwrap().is_empty());
    }

    #[test]
    fn audio_retention_query() {
        let s = store();
        // ended at 60_100
        let old = s.insert_meeting(&meeting("old", 100)).unwrap();
        // ended at 1_060_000
        let new = s.insert_meeting(&meeting("new", 1_000_000)).unwrap();
        let mut gone = meeting("gone", 50);
        gone.mic_audio_path = None;
        gone.system_audio_path = None;
        s.insert_meeting(&gone).unwrap();
        let mut rec = meeting("rec", 10);
        rec.status = MeetingStatus::Recording;
        rec.ended_at = None;
        s.insert_meeting(&rec).unwrap();

        let ids = |ts| -> Vec<MeetingId> {
            s.meetings_with_audio_older_than(ts)
                .unwrap()
                .iter()
                .map(|m| m.id)
                .collect()
        };
        assert_eq!(ids(500_000), vec![old]);
        assert_eq!(ids(i64::MAX), vec![old, new]);
        assert!(ids(0).is_empty());

        let mut paths = s.audio_paths().unwrap();
        paths.sort();
        assert_eq!(
            paths,
            [
                "new-mic.opus.enc",
                "new-sys.opus.enc",
                "old-mic.opus.enc",
                "old-sys.opus.enc",
                "rec-mic.opus.enc",
                "rec-sys.opus.enc"
            ]
        );
    }

    #[test]
    fn replace_transcript_maps_local_speaker_ids() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let (sp, seg) = s
            .replace_transcript(
                m,
                &[speaker(0, "me", None), speaker(1, "Speaker 1", None)],
                &[
                    segment(2000, Some(1), Track::System),
                    segment(0, Some(0), Track::Mic),
                    segment(5000, None, Track::System),
                ],
            )
            .unwrap();
        assert_eq!(sp.len(), 2);
        assert!(sp[0].is_user);
        assert_eq!(sp[1].match_confidence, Some(0.5));
        assert_eq!(seg.len(), 3);
        assert_eq!(seg[0].t0_ms, 0);
        assert_eq!(seg[0].speaker_id, Some(sp[0].id));
        assert_eq!(seg[0].track, Track::Mic);
        assert_eq!(seg[1].speaker_id, Some(sp[1].id));
        assert_eq!(seg[1].words[0].text, "at");
        assert_eq!(seg[2].speaker_id, None);
        assert_eq!(s.segments(m).unwrap(), seg);
        assert_eq!(s.speakers(m).unwrap(), sp);

        // Replacing again drops the old rows.
        let (sp2, seg2) = s
            .replace_transcript(
                m,
                &[speaker(5, "Speaker 1", None)],
                &[segment(1, Some(5), Track::System)],
            )
            .unwrap();
        assert_eq!(sp2.len(), 1);
        assert_eq!(seg2.len(), 1);
        assert_eq!(s.get_speaker(sp[0].id).unwrap(), None);

        // A bad reference rolls the whole thing back.
        assert!(s
            .replace_transcript(m, &[], &[segment(1, Some(42), Track::System)])
            .is_err());
        assert_eq!(s.segments(m).unwrap(), seg2);
        assert_eq!(s.speakers(m).unwrap(), sp2);
    }

    #[test]
    fn speaker_updates_and_embedding() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let p = s.insert_person("Ann", None).unwrap();
        let (sp, _) = s
            .replace_transcript(m, &[speaker(0, "Speaker 1", None)], &[])
            .unwrap();
        let mut sp = sp[0].clone();
        sp.person_id = Some(p.id);
        sp.confirmed = true;
        sp.match_confidence = None;
        s.update_speaker(&sp).unwrap();
        assert_eq!(s.get_speaker(sp.id).unwrap(), Some(sp.clone()));

        assert_eq!(s.speaker_embedding(sp.id).unwrap(), None);
        s.set_speaker_embedding(sp.id, Some(&[0.25, -1.5])).unwrap();
        assert_eq!(s.speaker_embedding(sp.id).unwrap(), Some(vec![0.25, -1.5]));
        assert_eq!(s.speaker_embedding(12345).unwrap(), None);
    }

    #[test]
    fn people_crud_and_counts() {
        let s = store();
        let bob = s.insert_person("bob", Some("Bob@Example.com")).unwrap();
        let ann = s.insert_person("Ann", None).unwrap();
        assert_eq!(bob.voiceprint_count, 0);
        s.add_voiceprint(bob.id, &[1.0, 2.0], None).unwrap();
        s.add_voiceprint(bob.id, &[3.0], None).unwrap();

        let people = s.list_people().unwrap();
        assert_eq!(
            people
                .iter()
                .map(|p| p.display_name.as_str())
                .collect::<Vec<_>>(),
            vec!["Ann", "bob"]
        );
        assert_eq!(people[1].voiceprint_count, 2);

        assert_eq!(
            s.find_person_by_email("bob@example.com")
                .unwrap()
                .map(|p| p.id),
            Some(bob.id)
        );
        s.update_person(ann.id, "Ann B", Some("ann@x.io")).unwrap();
        let a = s.get_person(ann.id).unwrap().unwrap();
        assert_eq!(a.display_name, "Ann B");
        assert_eq!(a.email.as_deref(), Some("ann@x.io"));
        assert!(s.update_person(999, "x", None).is_err());
    }

    #[test]
    fn deleting_a_person_removes_voiceprints_and_clears_links() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let p = s.insert_person("Ann", None).unwrap();
        let keep = s.insert_person("Bob", None).unwrap();
        s.add_voiceprint(p.id, &[1.0], Some(m)).unwrap();
        s.add_voiceprint(keep.id, &[2.0], Some(m)).unwrap();
        s.replace_transcript(
            m,
            &[speaker(0, "Speaker 1", Some(p.id))],
            &[segment(0, Some(0), Track::System)],
        )
        .unwrap();
        let mut it = item(m, "do it");
        it.owner_person_id = Some(p.id);
        let it = s.insert_action_item(&it).unwrap();

        assert!(s.delete_person(p.id).unwrap());
        assert!(!s.delete_person(p.id).unwrap());

        assert!(s.voiceprints_for_person(p.id).unwrap().is_empty());
        assert_eq!(s.voiceprints().unwrap().len(), 1);
        assert_eq!(s.speakers(m).unwrap()[0].person_id, None);
        assert_eq!(s.segments(m).unwrap().len(), 1);
        assert_eq!(
            s.get_action_item(it.id).unwrap().unwrap().owner_person_id,
            None
        );
    }

    #[test]
    fn deleting_a_meeting_keeps_voiceprints() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let other = s.insert_meeting(&meeting("o", 0)).unwrap();
        let p = s.insert_person("Ann", None).unwrap();
        let vp = s.add_voiceprint(p.id, &[0.5, 0.25], Some(m)).unwrap();
        s.replace_transcript(
            m,
            &[speaker(0, "Speaker 1", Some(p.id))],
            &[segment(0, Some(0), Track::System)],
        )
        .unwrap();
        s.put_summary(&StoredSummary {
            meeting_id: m,
            title: "t".into(),
            short_summary: "s".into(),
            decisions: vec![],
            topics: vec![],
            generated_by: "x/y".into(),
            generated_at: 1,
            usage: None,
        })
        .unwrap();
        s.insert_action_item(&item(m, "a")).unwrap();
        s.insert_action_item(&item(other, "b")).unwrap();

        assert!(s.delete_meeting(m).unwrap());

        assert_eq!(s.get_meeting(m).unwrap(), None);
        assert!(s.segments(m).unwrap().is_empty());
        assert!(s.speakers(m).unwrap().is_empty());
        assert_eq!(s.summary(m).unwrap(), None);
        assert!(s.action_items(m).unwrap().is_empty());
        assert_eq!(s.action_items(other).unwrap().len(), 1);
        let vps = s.voiceprints().unwrap();
        assert_eq!(vps.len(), 1);
        assert_eq!(vps[0].id, vp);
        assert_eq!(vps[0].source_meeting_id, None);
        assert_eq!(vps[0].embedding, vec![0.5, 0.25]);
    }

    #[test]
    fn merge_people_moves_everything() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let from = s.insert_person("Annie", Some("ann@x.io")).unwrap();
        let into = s.insert_person("Ann", None).unwrap();
        s.add_voiceprint(from.id, &[1.0], Some(m)).unwrap();
        s.add_voiceprint(into.id, &[2.0], Some(m)).unwrap();
        s.replace_transcript(m, &[speaker(0, "Speaker 1", Some(from.id))], &[])
            .unwrap();
        let mut it = item(m, "x");
        it.owner_person_id = Some(from.id);
        let it = s.insert_action_item(&it).unwrap();

        s.merge_people(from.id, into.id).unwrap();

        assert_eq!(s.get_person(from.id).unwrap(), None);
        let into_p = s.get_person(into.id).unwrap().unwrap();
        assert_eq!(into_p.display_name, "Ann");
        assert_eq!(into_p.email.as_deref(), Some("ann@x.io"));
        assert_eq!(into_p.voiceprint_count, 2);
        assert_eq!(s.speakers(m).unwrap()[0].person_id, Some(into.id));
        assert_eq!(
            s.get_action_item(it.id).unwrap().unwrap().owner_person_id,
            Some(into.id)
        );

        assert!(s.merge_people(into.id, into.id).is_err());
        assert!(s.merge_people(from.id, into.id).is_err());
    }

    /// Speakers matched to people merged away or deleted since (a
    /// processing checkpoint) are stored linked to the merge target, or
    /// unknown: never a dangling person id.
    #[test]
    fn replace_transcript_resolves_merged_and_deleted_people() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_person("A", None).unwrap();
        let b = s.insert_person("B", None).unwrap();
        let c = s.insert_person("C", None).unwrap();
        let gone = s.insert_person("Gone", None).unwrap();
        let mut confirmed = speaker(3, "Speaker 3", Some(gone.id));
        confirmed.confirmed = true;
        s.merge_people(a.id, b.id).unwrap();
        // A chain of merges resolves to the last target.
        s.merge_people(b.id, c.id).unwrap();
        assert!(s.delete_person(gone.id).unwrap());

        let (sp, _) = s
            .replace_transcript(
                m,
                &[
                    speaker(1, "Speaker 1", Some(a.id)),
                    speaker(2, "Speaker 2", Some(b.id)),
                    confirmed,
                    speaker(4, "Speaker 4", None),
                ],
                &[],
            )
            .unwrap();
        let got: Vec<_> = sp
            .iter()
            .map(|s| (s.person_id, s.match_confidence, s.confirmed))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some(c.id), Some(0.5), false),
                (Some(c.id), Some(0.5), false),
                (None, None, false),
                (None, Some(0.5), false),
            ]
        );

        // Deleting the target forgets the merges into it.
        assert!(s.delete_person(c.id).unwrap());
        let (sp, _) = s
            .replace_transcript(m, &[speaker(1, "Speaker 1", Some(a.id))], &[])
            .unwrap();
        assert_eq!(sp[0].person_id, None);
    }

    #[test]
    fn delete_person_within_refuses_people_in_other_meetings() {
        let s = store();
        let m1 = s.insert_meeting(&meeting("one", 0)).unwrap();
        let m2 = s.insert_meeting(&meeting("two", 0)).unwrap();
        let ann = s.insert_person("Ann", None).unwrap();
        for m in [m1, m2] {
            s.replace_transcript(
                m,
                &[speaker(0, "Speaker 1", Some(ann.id))],
                &[segment(0, Some(0), Track::System)],
            )
            .unwrap();
        }
        assert_eq!(
            s.delete_person_within(ann.id, &BTreeSet::from([m1]))
                .unwrap(),
            PersonDeletion::InMoreMeetings(BTreeSet::from([m1, m2]))
        );
        assert!(s.get_person(ann.id).unwrap().is_some());
        let PersonDeletion::Deleted(segments) = s
            .delete_person_within(ann.id, &BTreeSet::from([m1, m2]))
            .unwrap()
        else {
            panic!("not deleted");
        };
        assert_eq!(segments.len(), 2);
        assert!(s.get_person(ann.id).unwrap().is_none());
    }

    #[test]
    fn voiceprint_blob_is_little_endian_f32() {
        assert_eq!(encode_embedding(&[1.0]), vec![0, 0, 0x80, 0x3f]);
        let v = vec![0.1f32, -2.5, f32::MAX];
        assert_eq!(decode_embedding(&encode_embedding(&v)), v);

        let s = store();
        let p = s.insert_person("Ann", None).unwrap();
        let id = s.add_voiceprint(p.id, &v, None).unwrap();
        let got = s.voiceprints_for_person(p.id).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, id);
        assert_eq!(got[0].embedding, v);
        assert!(s.delete_voiceprint(id).unwrap());
        assert!(s.voiceprints().unwrap().is_empty());
    }

    #[test]
    fn summary_upsert() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        assert_eq!(s.summary(m).unwrap(), None);
        let mut sum = StoredSummary {
            meeting_id: m,
            title: "T".into(),
            short_summary: "S".into(),
            decisions: vec!["d1".into()],
            topics: vec!["t1".into(), "t2".into()],
            generated_by: "guidy_local/qwen".into(),
            generated_at: 5,
            usage: None,
        };
        s.put_summary(&sum).unwrap();
        assert_eq!(s.summary(m).unwrap(), Some(sum.clone()));
        sum.short_summary = "S2".into();
        sum.decisions.clear();
        s.put_summary(&sum).unwrap();
        assert_eq!(s.summary(m).unwrap(), Some(sum));
    }

    #[test]
    fn summary_and_items_are_replaced_atomically() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let old = StoredSummary {
            meeting_id: m,
            title: "Old".into(),
            short_summary: "S".into(),
            decisions: vec![],
            topics: vec![],
            generated_by: "a".into(),
            generated_at: 1,
            usage: None,
        };
        let items = s
            .put_summary_and_action_items(&old, &[item(m, "old item")], &[], |_, _| false)
            .unwrap();
        assert_eq!(items.len(), 1);

        // The second item's insert fails (a test trigger) after the summary
        // and the old items were rewritten.
        s.execute_batch_for_tests(
            "CREATE TEMP TRIGGER fail_bad BEFORE INSERT ON action_items
             WHEN NEW.text = 'bad' BEGIN SELECT RAISE(ABORT, 'boom'); END",
        )
        .unwrap();
        let new = StoredSummary {
            title: "New".into(),
            generated_at: 2,
            ..old.clone()
        };
        let bad = item(m, "bad");
        assert!(s
            .put_summary_and_action_items(&new, &[item(m, "new item"), bad], &[], |_, _| false)
            .is_err());
        assert_eq!(s.summary(m).unwrap(), Some(old));
        assert_eq!(s.action_items(m).unwrap(), items);

        let after = s
            .put_summary_and_action_items(&new, &[item(m, "new item")], &[], |_, _| false)
            .unwrap();
        assert_eq!(s.summary(m).unwrap(), Some(new));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].text, "new item");
    }

    /// Owners picked before a long summary run follow merges and drop
    /// deleted people instead of failing the summary on the foreign key.
    #[test]
    fn summary_items_resolve_merged_and_deleted_owners() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let merged = s.insert_person("Annie", None).unwrap();
        let into = s.insert_person("Ann", None).unwrap();
        let gone = s.insert_person("Bob", None).unwrap();
        let kept = s.insert_person("Cy", None).unwrap();
        s.merge_people(merged.id, into.id).unwrap();
        assert!(s.delete_person(gone.id).unwrap());

        let sum = StoredSummary {
            meeting_id: m,
            title: "T".into(),
            short_summary: "S".into(),
            decisions: vec![],
            topics: vec![],
            generated_by: "a".into(),
            generated_at: 1,
            usage: None,
        };
        let owned = |text: &str, owner: PersonId| ActionItem {
            owner_person_id: Some(owner),
            ..item(m, text)
        };
        let items = s
            .put_summary_and_action_items(
                &sum,
                &[
                    owned("merged", merged.id),
                    owned("gone", gone.id),
                    owned("kept", kept.id),
                    owned("missing", 999_999),
                ],
                &[],
                |_, _| false,
            )
            .unwrap();
        let owners: Vec<_> = items
            .iter()
            .map(|i| (i.text.as_str(), i.owner_person_id))
            .collect();
        assert_eq!(
            owners,
            vec![
                ("merged", Some(into.id)),
                ("gone", None),
                ("kept", Some(kept.id)),
                ("missing", None),
            ]
        );
        assert_eq!(s.summary(m).unwrap(), Some(sum));
    }

    #[test]
    fn action_item_crud_and_linear_link() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "first")).unwrap();
        assert_eq!(a.text, "first");
        assert_eq!(a.ticket.label_ids, vec!["l1".to_string()]);
        assert_eq!(a.ticket_state, TicketState::Draft);
        assert_eq!(s.action_item_flags(a.id).unwrap(), Some((false, false)));

        let mut edited = a.clone();
        edited.text = "edited".into();
        s.update_action_item(&edited).unwrap();
        let mut ticket = a.ticket.clone();
        ticket.team_id = Some("team".into());
        s.update_ticket_draft(a.id, &ticket).unwrap();
        s.set_ticket_state(a.id, TicketState::Approved).unwrap();
        let got = s.get_action_item(a.id).unwrap().unwrap();
        assert_eq!(got.text, "edited");
        assert_eq!(got.ticket, ticket);
        assert_eq!(got.ticket_state, TicketState::Approved);
        assert_eq!(s.action_item_flags(a.id).unwrap(), Some((false, true)));
        assert!(s.set_ticket_state(a.id, TicketState::Created).is_err());

        s.set_linear_issue(a.id, "uuid-1", "ENG-1", "https://linear.app/x/ENG-1")
            .unwrap();
        let linked = s.get_action_item(a.id).unwrap().unwrap();
        assert_eq!(linked.ticket_state, TicketState::Created);
        assert_eq!(linked.linear_issue_identifier.as_deref(), Some("ENG-1"));

        let b = s.insert_action_item(&item(m, "second")).unwrap();
        let all = s
            .replace_unlinked_action_items(
                m,
                &[item(999, "new1"), item(999, "new2")],
                &[],
                |_, _| false,
            )
            .unwrap();
        let texts: Vec<_> = all.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, vec!["edited", "new1", "new2"]);
        assert!(all.iter().all(|i| i.meeting_id == m));
        assert_eq!(s.get_action_item(b.id).unwrap(), None);

        assert!(s.delete_action_item(a.id).unwrap());
        assert_eq!(s.action_items(m).unwrap().len(), 2);
        assert!(s.set_linear_issue(a.id, "x", "y", "z").is_err());
    }

    #[test]
    fn stale_item_save_never_touches_ticket_or_linear_link() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "do it")).unwrap();
        // The UI holds this copy while the ticket gets created.
        let stale = a.clone();
        s.set_ticket_state(a.id, TicketState::Approved).unwrap();
        s.set_linear_issue(a.id, "iss", "ENG-1", "u").unwrap();

        let mut edit = stale.clone();
        edit.text = "do it well".into();
        edit.ticket = TicketDraft::default();
        edit.ticket_state = TicketState::Draft;
        s.update_action_item(&edit).unwrap();
        let got = s.get_action_item(a.id).unwrap().unwrap();
        assert_eq!(got.text, "do it well");
        assert_eq!(got.ticket_state, TicketState::Created);
        assert_eq!(got.linear_issue_id.as_deref(), Some("iss"));
        assert_eq!(got.linear_issue_identifier.as_deref(), Some("ENG-1"));
        assert_eq!(got.ticket, a.ticket);

        // The ticket of a Created item is frozen.
        let e = s
            .update_ticket_draft(a.id, &TicketDraft::default())
            .unwrap_err();
        assert_eq!(
            e.downcast_ref::<TicketEditError>(),
            Some(&TicketEditError::AlreadyCreated)
        );
        assert!(s.set_ticket_state(a.id, TicketState::Draft).is_err());
        assert!(s.set_ticket_state(a.id, TicketState::Discarded).is_err());
        assert!(s
            .update_ticket_draft(4242, &TicketDraft::default())
            .is_err());
    }

    #[test]
    fn regenerating_keeps_approved_manual_edited_creating_and_guarded_items() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let approved = s.insert_action_item(&item(m, "approved")).unwrap();
        s.set_ticket_state(approved.id, TicketState::Approved)
            .unwrap();
        let mut manual = item(m, "manual");
        manual.source_quote = String::new();
        let manual = s.insert_manual_action_item(&manual).unwrap();
        let mut quoteless = item(m, "quoteless");
        quoteless.source_quote = "  ".into();
        let quoteless = s.insert_action_item(&quoteless).unwrap();
        let edited = s.insert_action_item(&item(m, "edited")).unwrap();
        let mut e = edited.clone();
        e.owner_name = Some("Dana".into());
        s.update_action_item(&e).unwrap();
        let creating = s.insert_action_item(&item(m, "creating")).unwrap();
        s.ensure_linear_client_id(creating.id, &creating.ticket)
            .unwrap();
        let guarded = s.insert_action_item(&item(m, "guarded")).unwrap();
        let plain = s.insert_action_item(&item(m, "plain")).unwrap();
        // Saving an unchanged copy is not an edit.
        s.update_action_item(&plain).unwrap();
        assert_eq!(s.action_item_flags(plain.id).unwrap(), Some((false, false)));
        assert_eq!(s.action_item_flags(manual.id).unwrap(), Some((true, false)));
        assert_eq!(
            s.action_item_flags(quoteless.id).unwrap(),
            Some((true, false))
        );

        let all = s
            .replace_unlinked_action_items(
                m,
                &[item(0, "approved"), item(0, "fresh")],
                &[guarded.id],
                |a, b| a.text == b.text,
            )
            .unwrap();
        let texts: Vec<_> = all.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "approved",
                "manual",
                "quoteless",
                "edited",
                "creating",
                "guarded",
                "fresh"
            ]
        );
        assert_eq!(s.get_action_item(plain.id).unwrap(), None);
    }

    #[test]
    fn linear_client_id_is_a_stable_v4_uuid() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "x")).unwrap();
        assert_eq!(s.linear_client_id(a.id).unwrap(), None);
        let (id, sent) = s.ensure_linear_client_id(a.id, &a.ticket).unwrap();
        assert_eq!(sent, SentDraft::First);
        assert_eq!(s.ensure_linear_client_id(a.id, &a.ticket).unwrap().0, id);
        assert_eq!(s.linear_client_id(a.id).unwrap().as_deref(), Some(&*id));
        assert_eq!(id.len(), 36);
        let parts: Vec<_> = id.split('-').map(str::len).collect();
        assert_eq!(parts, vec![8, 4, 4, 4, 12]);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(new_uuid_v4(), new_uuid_v4());
        assert!(s
            .ensure_linear_client_id(999, &TicketDraft::default())
            .is_err());
    }

    #[test]
    fn first_sent_ticket_is_kept_until_the_issue_is_recorded() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "x")).unwrap();
        let first = TicketDraft {
            title: "First".into(),
            ..Default::default()
        };
        let edited = TicketDraft {
            title: "Edited".into(),
            ..Default::default()
        };
        let (id, sent) = s.ensure_linear_client_id(a.id, &first).unwrap();
        assert_eq!(sent, SentDraft::First);
        // A retry with an edited draft gets the first attempt's draft back,
        // and keeps it.
        for _ in 0..2 {
            let (again, sent) = s.ensure_linear_client_id(a.id, &edited).unwrap();
            assert_eq!(again, id);
            assert_eq!(sent, SentDraft::Earlier(first.clone()));
        }
        s.set_linear_issue(a.id, "iss", "ENG-1", "u").unwrap();
        let sent: Option<String> = s
            .conn
            .lock()
            .query_row(
                "SELECT linear_sent_ticket FROM action_items WHERE id = ?1",
                [a.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sent, None);
    }

    #[test]
    fn unsent_client_id_is_dropped_with_its_sent_ticket() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "x")).unwrap();
        let first = TicketDraft {
            title: "Never sent".into(),
            ..Default::default()
        };
        let (id, _) = s.ensure_linear_client_id(a.id, &first).unwrap();
        // Another id (or an issue) by now: left alone.
        s.drop_unsent_linear_client_id(a.id, "other").unwrap();
        assert_eq!(s.linear_client_id(a.id).unwrap().as_deref(), Some(&*id));
        s.drop_unsent_linear_client_id(a.id, &id).unwrap();
        assert_eq!(s.linear_client_id(a.id).unwrap(), None);
        // The next attempt starts over: its draft is the baseline.
        let next = TicketDraft {
            title: "Sent".into(),
            ..Default::default()
        };
        let (id2, sent) = s.ensure_linear_client_id(a.id, &next).unwrap();
        assert_ne!(id2, id);
        assert_eq!(sent, SentDraft::First);
        assert_eq!(
            s.ensure_linear_client_id(a.id, &first).unwrap().1,
            SentDraft::Earlier(next)
        );
        s.set_linear_issue(a.id, "iss", "ENG-1", "u").unwrap();
        s.drop_unsent_linear_client_id(a.id, &id2).unwrap();
        assert_eq!(s.linear_client_id(a.id).unwrap().as_deref(), Some(&*id2));
    }

    #[test]
    fn unreadable_sent_ticket_is_unknown_and_kept() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let a = s.insert_action_item(&item(m, "x")).unwrap();
        let (id, _) = s.ensure_linear_client_id(a.id, &a.ticket).unwrap();
        s.conn
            .lock()
            .execute(
                "UPDATE action_items SET linear_sent_ticket = 'not json' WHERE id = ?1",
                [a.id],
            )
            .unwrap();
        let edited = TicketDraft {
            title: "Edited".into(),
            ..Default::default()
        };
        assert_eq!(
            s.ensure_linear_client_id(a.id, &edited).unwrap(),
            (id, SentDraft::Unknown)
        );
        let sent: Option<String> = s
            .conn
            .lock()
            .query_row(
                "SELECT linear_sent_ticket FROM action_items WHERE id = ?1",
                [a.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sent.as_deref(), Some("not json"));
    }

    #[test]
    fn targeted_meeting_updates_are_guarded_by_status() {
        let s = store();
        let mut rec = meeting("r", 0);
        rec.status = MeetingStatus::Recording;
        rec.ended_at = None;
        let id = s.insert_meeting(&rec).unwrap();
        s.set_meeting_title(id, "Renamed").unwrap();
        assert!(s.finish_recording(id, 500, Some("mic track: x")).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.title, "Renamed");
        assert_eq!(got.status, MeetingStatus::Recorded);
        assert_eq!(got.ended_at, Some(500));
        assert_eq!(got.error.as_deref(), Some("mic track: x"));
        // Not recording any more: a second finish changes nothing.
        assert!(!s.finish_recording(id, 900, None).unwrap());
        assert_eq!(s.get_meeting(id).unwrap().unwrap().ended_at, Some(500));

        // Retention clear: only once fully processed and idle.
        assert!(!s.clear_audio_paths_if_processed(id).unwrap());
        s.set_processing_stage(id, Some(ProcessingStage::Done))
            .unwrap();
        s.set_meeting_status(id, MeetingStatus::Processing, None)
            .unwrap();
        assert!(!s.clear_audio_paths_if_processed(id).unwrap());
        s.set_meeting_status(id, MeetingStatus::Processed, None)
            .unwrap();
        assert!(s.clear_audio_paths_if_processed(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!((got.mic_audio_path, got.system_audio_path), (None, None));
        assert_eq!(got.title, "Renamed");
    }

    #[test]
    fn marking_processing_is_guarded_by_status_and_audio() {
        let s = store();
        let mut rec = meeting("r", 0);
        rec.status = MeetingStatus::Recording;
        let recording = s.insert_meeting(&rec).unwrap();
        assert!(!s.mark_processing(recording, false).unwrap());
        assert_eq!(
            s.get_meeting(recording).unwrap().unwrap().status,
            MeetingStatus::Recording
        );

        let mut failed = meeting("f", 0);
        failed.status = MeetingStatus::Failed;
        failed.error = Some("boom".into());
        let id = s.insert_meeting(&failed).unwrap();
        assert!(s.mark_processing(id, true).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!((got.status, got.error), (MeetingStatus::Processing, None));

        // Processed and its audio cleared by retention: refused when the run
        // needs audio, allowed when it resumes past it.
        s.set_meeting_status(id, MeetingStatus::Processed, None)
            .unwrap();
        s.set_processing_stage(id, Some(ProcessingStage::Done))
            .unwrap();
        assert!(s.clear_audio_paths_if_processed(id).unwrap());
        assert!(!s.mark_processing(id, true).unwrap());
        assert_eq!(
            s.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Processed
        );
        assert!(s.mark_processing(id, false).unwrap());
        assert!(!s.mark_processing(999, false).unwrap());
    }

    #[test]
    fn the_status_before_processing_is_kept_until_it_ends() {
        let s = store();
        let mut done = meeting("p", 0);
        done.status = MeetingStatus::Processed;
        let id = s.insert_meeting(&done).unwrap();
        assert_eq!(s.status_before_processing(id).unwrap(), None);
        assert!(s.mark_processing(id, false).unwrap());
        assert_eq!(
            s.status_before_processing(id).unwrap(),
            Some(MeetingStatus::Processed)
        );
        // Marked again (a resume): the saved status stays.
        assert!(s.mark_processing(id, false).unwrap());
        assert_eq!(
            s.status_before_processing(id).unwrap(),
            Some(MeetingStatus::Processed)
        );
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!((got.status, got.error), (MeetingStatus::Processed, None));
        // Not processing: nothing to restore.
        assert!(!s.restore_status_before_processing(id).unwrap());

        // A failure keeps its error.
        s.set_meeting_status(id, MeetingStatus::Failed, Some("boom"))
            .unwrap();
        assert!(s.mark_processing(id, false).unwrap());
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Failed);
        assert_eq!(got.error.as_deref(), Some("boom"));

        // An outcome drops the saved status.
        assert!(s.mark_processing(id, false).unwrap());
        s.finish_processing(id, MeetingStatus::Processed, None)
            .unwrap();
        s.set_meeting_status(id, MeetingStatus::Processing, None)
            .unwrap();
        assert_eq!(s.status_before_processing(id).unwrap(), None);
        // Nothing saved (a crash leftover): back to `Recorded`.
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!((got.status, got.error), (MeetingStatus::Recorded, None));
    }

    /// A cancelled reprocess gets its `done` stage back (so retention still
    /// applies); a cancelled first run keeps the stage it reached.
    #[test]
    fn a_cancel_restores_the_stage_of_a_processed_meeting_only() {
        let s = store();
        let mut done = meeting("p", 0);
        done.status = MeetingStatus::Processed;
        done.processing_stage = Some(ProcessingStage::Done.as_str().into());
        done.ended_at = Some(1);
        done.mic_audio_path = Some("1-mic.gopus".into());
        let id = s.insert_meeting(&done).unwrap();
        assert!(s.mark_processing(id, true).unwrap());
        // The new run starts over and gets part-way.
        s.set_processing_stage(id, None).unwrap();
        s.set_processing_stage(id, Some(ProcessingStage::Vad))
            .unwrap();
        // Resumed after a crash: the saved stage stays.
        assert!(s.mark_processing(id, true).unwrap());
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Processed);
        assert_eq!(got.processing_stage.as_deref(), Some("done"));
        assert!(s.clear_audio_paths_if_processed(id).unwrap());

        let mut failed = meeting("f", 0);
        failed.status = MeetingStatus::Failed;
        failed.processing_stage = Some(ProcessingStage::Decode.as_str().into());
        let id = s.insert_meeting(&failed).unwrap();
        assert!(s.mark_processing(id, false).unwrap());
        s.set_processing_stage(id, Some(ProcessingStage::Transcribe))
            .unwrap();
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Failed);
        assert_eq!(got.processing_stage.as_deref(), Some("transcribe"));

        // An outcome drops the saved stage: a later crash leftover can't
        // bring back a stale one.
        assert!(s.mark_processing(id, false).unwrap());
        s.finish_processing(id, MeetingStatus::Processed, None)
            .unwrap();
        s.set_meeting_status(id, MeetingStatus::Processing, None)
            .unwrap();
        s.set_processing_stage(id, Some(ProcessingStage::Vad))
            .unwrap();
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Recorded);
        assert_eq!(got.processing_stage.as_deref(), Some("vad"));
    }

    #[test]
    fn the_stage_is_kept_only_while_processing() {
        let s = store();
        let id = s.insert_meeting(&meeting("m", 0)).unwrap();
        assert!(!s
            .set_processing_stage_if_processing(id, Some(ProcessingStage::Vad))
            .unwrap());
        assert_eq!(s.get_meeting(id).unwrap().unwrap().processing_stage, None);
        assert!(s.mark_processing(id, false).unwrap());
        assert!(s
            .set_processing_stage_if_processing(id, Some(ProcessingStage::Vad))
            .unwrap());
        assert_eq!(
            s.get_meeting(id)
                .unwrap()
                .unwrap()
                .processing_stage
                .as_deref(),
            Some("vad")
        );
    }

    /// An item a version 4 database left "creating" (client id, no sent
    /// draft) keeps its id, and what it sent stays unknown: a later
    /// attempt's draft is not recorded as it.
    #[test]
    fn a_version_4_creating_item_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MEETINGS_DB_FILE);
        {
            let mut conn = Connection::open(&path).unwrap();
            apply_key(&conn, &KEY).unwrap();
            Migrations::new(MIGRATIONS[..4].to_vec())
                .to_latest(&mut conn)
                .unwrap();
            conn.execute(
                "INSERT INTO meetings (title, started_at, ended_at, status)
                 VALUES ('old', 0, 1, 'processed')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO action_items (meeting_id, text, ticket, linear_client_id)
                 VALUES (1, 'x', ?1, 'old-client-id')",
                [to_json(&TicketDraft::default()).unwrap()],
            )
            .unwrap();
        }
        let s = MeetingsStore::open(&path, &KEY).unwrap();
        let id = s.action_items(1).unwrap()[0].id;
        let draft = TicketDraft {
            title: "Now".into(),
            ..Default::default()
        };
        for _ in 0..2 {
            assert_eq!(
                s.ensure_linear_client_id(id, &draft).unwrap(),
                ("old-client-id".to_string(), SentDraft::Unknown)
            );
        }
    }

    /// A database created when migration 3 was the last one gets the stage
    /// column from migration 4.
    #[test]
    fn a_version_3_database_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MEETINGS_DB_FILE);
        {
            let mut conn = Connection::open(&path).unwrap();
            apply_key(&conn, &KEY).unwrap();
            Migrations::new(MIGRATIONS[..3].to_vec())
                .to_latest(&mut conn)
                .unwrap();
            let v: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, 3);
            conn.execute(
                "INSERT INTO meetings (title, started_at, ended_at, status, processing_stage)
                 VALUES ('old', 0, 1, 'processed', 'done')",
                [],
            )
            .unwrap();
        }
        let s = MeetingsStore::open(&path, &KEY).unwrap();
        let id = s.list_meetings().unwrap()[0].id;
        assert!(s.mark_processing(id, false).unwrap());
        s.set_processing_stage(id, Some(ProcessingStage::Vad))
            .unwrap();
        assert!(s.restore_status_before_processing(id).unwrap());
        let got = s.get_meeting(id).unwrap().unwrap();
        assert_eq!(got.status, MeetingStatus::Processed);
        assert_eq!(got.processing_stage.as_deref(), Some("done"));
    }

    #[test]
    fn only_a_wrong_key_is_reported_as_undecryptable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MEETINGS_DB_FILE);
        drop(MeetingsStore::open(&path, &KEY).unwrap());
        let e = MeetingsStore::open(&path, &[8u8; 32]).err().unwrap();
        assert!(
            e.downcast_ref::<MeetingsDbUndecryptable>().is_some(),
            "{e:#}"
        );

        // Locked by another connection: an error, but not a key problem.
        let other = Connection::open(&path).unwrap();
        apply_key(&other, &KEY).unwrap();
        other.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let e = MeetingsStore::open(&path, &KEY).err().unwrap();
        assert!(
            e.downcast_ref::<MeetingsDbUndecryptable>().is_none(),
            "{e:#}"
        );
        other.execute_batch("ROLLBACK").unwrap();
        assert!(MeetingsStore::open(&path, &KEY).is_ok());
    }

    #[test]
    fn deleting_a_person_clears_their_speaker_centroids() {
        let s = store();
        let m = s.insert_meeting(&meeting("t", 0)).unwrap();
        let ann = s.insert_person("Ann", None).unwrap();
        let (sp, segs) = s
            .replace_transcript(
                m,
                &[
                    speaker(0, "Speaker 1", Some(ann.id)),
                    speaker(1, "Speaker 2", None),
                ],
                &[
                    segment(0, Some(0), Track::System),
                    segment(2000, Some(1), Track::System),
                    segment(4000, Some(0), Track::System),
                ],
            )
            .unwrap();
        s.set_speaker_embedding(sp[0].id, Some(&[1.0])).unwrap();
        s.set_speaker_embedding(sp[1].id, Some(&[2.0])).unwrap();
        let mine = s.person_segments(ann.id).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[&m], vec![segs[0].id, segs[2].id]);

        s.delete_person(ann.id).unwrap();
        assert_eq!(s.speaker_embedding(sp[0].id).unwrap(), None);
        assert_eq!(s.speaker_embedding(sp[1].id).unwrap(), Some(vec![2.0]));
        assert!(s.person_segments(ann.id).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn database_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MEETINGS_DB_FILE);
        // Even a pre-existing world-readable file is tightened.
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let s = MeetingsStore::open(&path, &KEY).unwrap();
        s.insert_meeting(&meeting("x", 0)).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // ----- Risk 5.4: SQLCipher build with plain + keyed databases ----------

    #[test]
    fn keyed_file_cannot_be_read_without_the_right_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MEETINGS_DB_FILE);
        {
            let s = MeetingsStore::open(&path, &KEY).unwrap();
            s.insert_meeting(&meeting("secret-title", 0)).unwrap();
        }
        // Reopen with the right key.
        let s = MeetingsStore::open(&path, &KEY).unwrap();
        assert_eq!(s.list_meetings().unwrap()[0].title, "secret-title");
        drop(s);

        // The file is not plain SQLite and doesn't contain the plaintext.
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3\0"));
        assert!(!bytes.windows(12).any(|w| w == b"secret-title"));

        // Wrong key.
        assert!(MeetingsStore::open(&path, &[8u8; 32]).is_err());
        // No key at all (plain SQLite connection).
        let plain = Connection::open(&path).unwrap();
        assert!(plain
            .query_row("SELECT COUNT(*) FROM meetings", [], |r| r.get::<_, i64>(0))
            .is_err());
    }

    #[test]
    fn unkeyed_databases_stay_plain_sqlite() {
        // history.db is opened without a key; with the SQLCipher build it must
        // remain a normal SQLite file readable by any SQLite.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE t (x TEXT); INSERT INTO t VALUES ('hello');")
                .unwrap();
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"SQLite format 3\0"));
        let conn = Connection::open(&path).unwrap();
        let v: String = conn.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hello");
        // And the linked library really is SQLCipher.
        let version: String = conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .unwrap();
        assert!(!version.is_empty());
    }
}
