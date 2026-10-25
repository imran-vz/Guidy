use std::cell::{Cell, RefCell};

use rusqlite_migration::Migrations;

use super::*;
use crate::meetings::audio_store::{TrackReader, TrackWriter};
use crate::meetings::processing::sidecar::ProcessingCache;
use crate::meetings::types::Track;

const KEY: [u8; 32] = [7u8; 32];

/// A data dir as the encrypted version left it: `meetings.db` (schema 10,
/// one meeting with both tracks, a person with a voiceprint), a checkpoint
/// and per-segment voiceprints as sidecar files, and two version 1 tracks.
/// Returns the decoded audio of the mic track.
fn encrypted_data_dir(dir: &Path) -> Vec<f32> {
    let db = dir.join(MEETINGS_DB_FILE);
    let mut conn = Connection::open(&db).unwrap();
    store::apply_key(&conn, &KEY).unwrap();
    Migrations::new(store::MIGRATIONS[..10].to_vec())
        .to_latest(&mut conn)
        .unwrap();
    conn.execute_batch(
        "INSERT INTO meetings (id, title, started_at, status, mic_audio_path, system_audio_path)
             VALUES (1, 'Weekly sync', 0, 'processed', '1-mic.gopus', '1-system.gopus');
         INSERT INTO people (id, display_name, created_at) VALUES (1, 'Ada', 0);
         INSERT INTO voiceprints (person_id, embedding, source_meeting_id, created_at)
             VALUES (1, x'0000803f', 1, 0);",
    )
    .unwrap();
    drop(conn);

    let meetings = dir.join("meetings");
    let embeddings = vec![SegmentEmbedding {
        segment_id: 4,
        duration_ms: 2500,
        embedding: vec![0.5, -0.25],
    }];
    let mut cache = ProcessingCache::new();
    cache.mic_ms = Some(4321);
    for (kind, json) in [
        (
            SidecarKind::Embeddings,
            serde_json::to_vec(&embeddings).unwrap(),
        ),
        (SidecarKind::Cache, serde_json::to_vec(&cache).unwrap()),
    ] {
        let path = sidecar::legacy_path(&meetings, kind, 1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, sidecar::seal_legacy(&KEY, kind, 1, &json)).unwrap();
    }

    let audio = audio_store::audio_dir(dir);
    let mut mic = Vec::new();
    for (track, secs) in [(Track::Mic, 2.0), (Track::System, 1.5)] {
        let plain = dir.join(format!("plain-{}.gopus", track.as_str()));
        let mut w = TrackWriter::create(&plain).unwrap();
        let n = (secs * 16_000.0) as usize;
        w.push(
            &(0..n)
                .map(|i| (i as f32 * 0.05).sin() * 0.3)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        w.finish().unwrap();
        if track == Track::Mic {
            mic = TrackReader::open(&plain).unwrap().read_all().unwrap();
        }
        let name = audio_store::track_file_name(1, track);
        std::fs::create_dir_all(&audio).unwrap();
        audio_store::write_legacy(&plain, &audio.join(name), &KEY);
        std::fs::remove_file(&plain).unwrap();
    }
    mic
}

/// `convert_if_needed` with the test key, recording the retired stamps.
fn convert(dir: &Path, retired: &RefCell<Vec<String>>) -> Result<Report> {
    convert_if_needed(
        dir,
        || Ok(KEY),
        |stamp| {
            retired.borrow_mut().push(stamp.to_string());
            Ok(true)
        },
    )
}

#[test]
fn encrypted_meeting_data_becomes_plain_and_the_originals_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let mic = encrypted_data_dir(dir);
    assert!(has_encrypted_data(dir).unwrap());

    let retired = RefCell::new(Vec::new());
    let report = convert(dir, &retired).unwrap();
    assert!(report.database);
    assert_eq!(
        (report.checkpoints, report.embeddings, report.tracks),
        (1, 1, 2)
    );
    assert!(report.key_retired);
    assert!(!has_encrypted_data(dir).unwrap());

    // The database: plain, migrated, with everything in it.
    let s = MeetingsStore::open(&dir.join(MEETINGS_DB_FILE)).unwrap();
    let m = s.get_meeting(1).unwrap().unwrap();
    assert_eq!(m.title, "Weekly sync");
    assert_eq!(s.list_people().unwrap()[0].voiceprint_count, 1);
    assert_eq!(s.voiceprints().unwrap()[0].embedding, vec![1.0]);
    // The sidecars, now rows.
    assert_eq!(
        sidecar::read_embeddings(&s, 1).unwrap().unwrap(),
        [SegmentEmbedding {
            segment_id: 4,
            duration_ms: 2500,
            embedding: vec![0.5, -0.25],
        }]
    );
    assert_eq!(
        sidecar::read_cache(&s, 1).unwrap().unwrap().mic_ms,
        Some(4321)
    );
    // The audio: plain, the same samples.
    let audio = audio_store::audio_dir(dir);
    let mut r = TrackReader::open(&audio.join("1-mic.gopus")).unwrap();
    assert_eq!(r.read_all().unwrap(), mic);
    assert!(TrackReader::open(&audio.join("1-system.gopus")).is_ok());
    assert!(!dir.join("meetings/processing").exists());
    assert!(!dir.join("meetings/embeddings").exists());

    // The originals, in the backup folder; the key retired under its stamp.
    let backup = report.backup.unwrap();
    for rel in [
        "meetings.db",
        "audio/1-mic.gopus",
        "audio/1-system.gopus",
        "processing/1.bin",
        "embeddings/1.bin",
        KEY_RETIRED_MARKER,
    ] {
        assert!(backup.join(rel).is_file(), "{rel} backed up");
    }
    assert!(audio_store::is_legacy_track(&backup.join("audio/1-mic.gopus")).unwrap());
    let stamp = backup
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix(BACKUP_PREFIX)
        .unwrap()
        .to_string();
    assert_eq!(*retired.borrow(), [format!("backup-{stamp}")]);

    // Done once: the next start asks for nothing.
    let report =
        convert_if_needed(dir, || panic!("no key needed"), |_| panic!("no retire")).unwrap();
    assert_eq!(report, Report::default());
}

#[test]
fn without_encrypted_data_the_keychain_is_never_asked() {
    let dir = tempfile::tempdir().unwrap();
    // A fresh install, then one with plain data.
    let report =
        convert_if_needed(dir.path(), || panic!("no key"), |_| panic!("no retire")).unwrap();
    assert_eq!(report, Report::default());
    drop(MeetingsStore::open(&dir.path().join(MEETINGS_DB_FILE)).unwrap());
    let mut w =
        TrackWriter::create(&audio_store::audio_dir(dir.path()).join("1-mic.gopus")).unwrap();
    w.push(&[0.1; 1600]).unwrap();
    w.finish().unwrap();
    let report =
        convert_if_needed(dir.path(), || panic!("no key"), |_| panic!("no retire")).unwrap();
    assert_eq!(report, Report::default());
}

#[test]
fn a_wrong_or_missing_key_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    encrypted_data_dir(dir);
    let retired = Cell::new(false);

    let e = convert_if_needed(
        dir,
        || Ok([8u8; 32]),
        |_| {
            retired.set(true);
            Ok(true)
        },
    )
    .unwrap_err();
    assert!(
        e.downcast_ref::<MeetingsDbUndecryptable>().is_some(),
        "{e:#}"
    );

    let e = convert_if_needed(
        dir,
        || Err(crate::secrets::MeetingsDataKeyMissing.into()),
        |_| {
            retired.set(true);
            Ok(true)
        },
    )
    .unwrap_err();
    assert!(e
        .downcast_ref::<crate::secrets::MeetingsDataKeyMissing>()
        .is_some());

    assert!(!retired.get());
    assert!(db_is_encrypted(&dir.join(MEETINGS_DB_FILE)).unwrap());
    assert!(!dir.join("meetings.db.plain").exists());
    assert_eq!(legacy_tracks(dir).unwrap().len(), 2);
    assert_eq!(legacy_sidecars(dir).unwrap().len(), 2);
}

/// A run stopped after backing up `meetings.db` (its copy not yet in
/// place) and mid-way through a track: the next start finishes both, into
/// the same backup folder, and retires the key once.
#[test]
fn an_interrupted_conversion_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    encrypted_data_dir(dir);
    let backup = backup_dir(dir).unwrap();
    // The database step, up to its last rename.
    let db = dir.join(MEETINGS_DB_FILE);
    let copy = with_suffix(&db, PLAIN_SUFFIX);
    {
        let conn = Connection::open(&db).unwrap();
        store::apply_key(&conn, &KEY).unwrap();
        conn.execute(
            "ATTACH DATABASE ?1 AS plaintext KEY ''",
            [copy.to_str().unwrap()],
        )
        .unwrap();
        conn.query_row("SELECT sqlcipher_export('plaintext')", [], |_| Ok(()))
            .unwrap();
        conn.execute_batch("PRAGMA plaintext.user_version = 10; DETACH DATABASE plaintext;")
            .unwrap();
    }
    back_up(&db, &backup, Path::new(MEETINGS_DB_FILE)).unwrap();
    // A track's copy, half written, its original still in place.
    let track = audio_store::audio_dir(dir).join("1-mic.gopus");
    std::fs::write(with_suffix(&track, PLAIN_SUFFIX), b"GDYOPUS\0torn").unwrap();

    let retired = RefCell::new(Vec::new());
    let report = convert(dir, &retired).unwrap();
    assert!(!report.database, "already converted");
    assert_eq!(report.tracks, 2);
    assert_eq!(report.backup.as_deref(), Some(backup.as_path()));
    assert_eq!(retired.borrow().len(), 1);
    assert!(!has_encrypted_data(dir).unwrap());
    let s = MeetingsStore::open(&db).unwrap();
    assert_eq!(s.get_meeting(1).unwrap().unwrap().title, "Weekly sync");
    assert!(sidecar::has_embeddings(&s, 1).unwrap());
    assert!(TrackReader::open(&track).unwrap().is_complete());
    assert!(!with_suffix(&track, PLAIN_SUFFIX).exists());
    assert!(backup.join("meetings.db").is_file());
    assert!(backup.join("audio/1-mic.gopus").is_file());
}

#[test]
fn a_keychain_that_wont_retire_the_key_doesnt_stop_meeting_notes() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    encrypted_data_dir(dir);
    let report =
        convert_if_needed(dir, || Ok(KEY), |_| anyhow::bail!("the Keychain is locked")).unwrap();
    assert!(report.database && !report.key_retired);
    assert!(!has_encrypted_data(dir).unwrap());
    let backup = report.backup.unwrap();
    assert!(!backup.join(KEY_RETIRED_MARKER).exists(), "retried later");
    // The next start retires it.
    let retired = RefCell::new(Vec::new());
    let report = convert_if_needed(
        dir,
        || panic!("no key needed"),
        |stamp| {
            retired.borrow_mut().push(stamp.to_string());
            Ok(true)
        },
    )
    .unwrap();
    assert!(report.key_retired);
    assert_eq!(retired.borrow().len(), 1);
    assert!(backup.join(KEY_RETIRED_MARKER).is_file());
}
