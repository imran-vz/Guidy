//! What Guidy keeps on disk, for the Storage page: every file under the data
//! directory sorted into a bucket, plus the log directory, downloaded models
//! in the shared Hugging Face cache and (release builds) the app itself.
//!
//! Read-only: this module deletes and writes nothing.
//!
//! Sizes are allocated bytes on Unix (`blocks * 512`, so a sparse file counts
//! for what it really occupies) and logical bytes on Windows. Symlinks are
//! never followed and a file reached twice (hard link, overlapping roots)
//! counts once, in the bucket that saw it first.
//!
//! Not counted: Keychain secrets, user exports, the autostart plist and
//! transient updater downloads.

use crate::host::AppHandle;
use std::collections::HashSet;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// A row of the Storage page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Models,
    SharedModels,
    MeetingAudio,
    DictationRecordings,
    Databases,
    MeetingWorkingFiles,
    Configuration,
    Cache,
    Logs,
    Backups,
    AppFiles,
    Other,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Models,
        Category::SharedModels,
        Category::MeetingAudio,
        Category::DictationRecordings,
        Category::Databases,
        Category::MeetingWorkingFiles,
        Category::Configuration,
        Category::Cache,
        Category::Logs,
        Category::Backups,
        Category::AppFiles,
        Category::Other,
    ];
}

/// The bucket of a file, from its path relative to the data directory.
/// Anything unrecognized is [`Category::Other`], so nothing goes uncounted.
pub fn classify(rel: &Path) -> Category {
    // A name that isn't UTF-8 matches nothing, but keeps its place in the path.
    let mut parts = rel.components().filter_map(|c| match c {
        Component::Normal(name) => Some(name.to_str().unwrap_or("")),
        _ => None,
    });
    let Some(first) = parts.next() else {
        return Category::Other;
    };
    let second = parts.next();
    let deeper = parts.next().is_some();
    match (first, second) {
        ("models" | "huggingface", Some(_)) => Category::Models,
        ("recordings", Some(_)) => Category::DictationRecordings,
        // Portable mode keeps its logs in the data directory.
        ("logs", Some(_)) => Category::Logs,
        ("meetings", Some("audio")) if deeper => Category::MeetingAudio,
        ("meetings", Some("processing" | "embeddings")) if deeper => Category::MeetingWorkingFiles,
        (name, None) if is_database(name) => Category::Databases,
        ("settings_store.json" | "window-state.json", None) => Category::Configuration,
        ("llm-registry.json", None) => Category::Cache,
        (name, _) if is_backup(name) => Category::Backups,
        _ => Category::Other,
    }
}

/// A database or one of its SQLite side files.
fn is_database(name: &str) -> bool {
    const DATABASES: [&str; 3] = [
        "history.db",
        crate::meetings::store::MEETINGS_DB_FILE,
        crate::llm::usage::USAGE_DB_FILE,
    ];
    DATABASES.iter().any(|db| {
        name.strip_prefix(db)
            .is_some_and(|side| matches!(side, "" | "-wal" | "-shm" | "-journal"))
    })
}

/// A folder the meeting store set aside (`meetings/legacy.rs`,
/// `meetings/manager.rs`).
fn is_backup(name: &str) -> bool {
    name.starts_with(crate::meetings::legacy::BACKUP_PREFIX)
        || name.starts_with("meetings-unreadable-")
}

/// Where to look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageRoots {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    /// The shared Hugging Face cache (for Open folder; it isn't walked, only
    /// the models Guidy uses are ours to count).
    pub shared_cache_dir: PathBuf,
    /// Downloaded speech-to-text models. Those in the data directory are
    /// already counted when it is walked; the rest are shared models.
    pub model_paths: Vec<PathBuf>,
    /// The bundled resources. `None` in debug builds, where this is the repo.
    pub resource_dir: Option<PathBuf>,
    /// The running executable. `None` in debug builds.
    pub executable: Option<PathBuf>,
}

/// One bucket's total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketUsage {
    pub category: Category,
    pub bytes: u64,
}

/// The volume holding the data directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskSpace {
    pub total_bytes: u64,
    /// Available to this user. Excludes purgeable space, so it can read lower
    /// than Finder.
    pub free_bytes: u64,
}

impl DiskSpace {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }
}

/// The result of a scan. Partial when `error_count > 0`: what could be read
/// is still here and the totals are lower bounds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageReport {
    /// Every category, in [`Category::ALL`] order.
    pub buckets: Vec<BucketUsage>,
    pub total_bytes: u64,
    /// The part of `total_bytes` on the volume described by `disk`.
    pub same_device_bytes: u64,
    pub disk: Option<DiskSpace>,
    pub error_count: u64,
    /// The first path that couldn't be read.
    pub first_error_path: Option<String>,
}

impl StorageReport {
    pub fn bytes(&self, category: Category) -> u64 {
        self.buckets
            .iter()
            .find(|b| b.category == category)
            .map_or(0, |b| b.bytes)
    }
}

/// Identifies a file or directory across the whole scan.
#[cfg(unix)]
type FileKey = (u64, u64);
/// Identifies the volume a file is on.
#[cfg(unix)]
type VolumeKey = u64;

#[cfg(unix)]
fn identify(_path: &Path, meta: &Metadata) -> (FileKey, VolumeKey) {
    use std::os::unix::fs::MetadataExt;
    ((meta.dev(), meta.ino()), meta.dev())
}

/// Volume serial and file index when the file can be opened, else its path:
/// roots are canonical and symlinks never followed, so a path is unique
/// short of a hard link.
#[cfg(not(unix))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FileKey {
    Id(u64, u64),
    Path(PathBuf),
}
/// Volume serial, else the drive prefix of the canonical path.
#[cfg(not(unix))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum VolumeKey {
    Serial(u64),
    Prefix(std::ffi::OsString),
}

#[cfg(not(unix))]
fn identify(path: &Path, _meta: &Metadata) -> (FileKey, VolumeKey) {
    match file_id(path) {
        Some((volume, file)) => (FileKey::Id(volume, file), VolumeKey::Serial(volume)),
        None => (
            FileKey::Path(path.to_path_buf()),
            VolumeKey::Prefix(
                path.components()
                    .next()
                    .map(|c| c.as_os_str().to_os_string())
                    .unwrap_or_default(),
            ),
        ),
    }
}

/// `(volume serial, file index)`. Std has no stable accessor for these.
#[cfg(windows)]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };
    // No access rights (metadata only), and directories need the flag.
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for this call and `info` a valid out-pointer.
    let result = unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) };
    result.ok()?;
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Some((u64::from(info.dwVolumeSerialNumber), index))
}

#[cfg(not(any(unix, windows)))]
fn file_id(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// What a file occupies on disk.
#[cfg(unix)]
fn file_size(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn file_size(meta: &Metadata) -> u64 {
    meta.len()
}

/// Which bucket a root's files go in.
#[derive(Clone, Copy)]
enum Filing {
    /// By path relative to the root ([`classify`]).
    Classified,
    Fixed(Category),
}

struct Walk {
    seen: HashSet<FileKey>,
    totals: Vec<u64>,
    total_bytes: u64,
    same_device_bytes: u64,
    disk_volume: Option<VolumeKey>,
    error_count: u64,
    first_error_path: Option<String>,
}

impl Walk {
    fn error(&mut self, path: &Path, error: &std::io::Error) {
        // Gone since it was listed (a recording finishing, a log rotating).
        if error.kind() == ErrorKind::NotFound {
            return;
        }
        self.error_count += 1;
        if self.first_error_path.is_none() {
            self.first_error_path = Some(path.display().to_string());
        }
    }

    fn add(&mut self, category: Category, bytes: u64, volume: VolumeKey) {
        if let Some(i) = Category::ALL.iter().position(|c| *c == category) {
            self.totals[i] = self.totals[i].saturating_add(bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        if self.disk_volume.as_ref() == Some(&volume) {
            self.same_device_bytes = self.same_device_bytes.saturating_add(bytes);
        }
    }

    /// Count everything at `root`. Its own path is configuration, not
    /// content, so a symlink there is resolved; below it none are followed.
    /// A root that doesn't exist is just empty. An explicit stack, so a deep
    /// tree can't overflow this thread's.
    fn walk(&mut self, root: &Path, filing: Filing) {
        let root = match std::fs::canonicalize(root) {
            Ok(root) => root,
            Err(e) => {
                self.error(root, &e);
                return;
            }
        };
        let mut stack = vec![root.clone()];
        while let Some(path) = stack.pop() {
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) => {
                    self.error(&path, &e);
                    continue;
                }
            };
            let kind = meta.file_type();
            if !kind.is_dir() && !kind.is_file() {
                // Symlinks, sockets, fifos.
                continue;
            }
            let (key, volume) = identify(&path, &meta);
            if !self.seen.insert(key) {
                continue;
            }
            if kind.is_file() {
                let category = match filing {
                    Filing::Fixed(category) => category,
                    Filing::Classified => {
                        path.strip_prefix(&root).map_or(Category::Other, classify)
                    }
                };
                self.add(category, file_size(&meta), volume);
                continue;
            }
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(e) => {
                    self.error(&path, &e);
                    continue;
                }
            };
            for entry in entries {
                match entry {
                    Ok(entry) => stack.push(entry.path()),
                    Err(e) => self.error(&path, &e),
                }
            }
        }
    }
}

/// The volume of `path`, or of its nearest ancestor that exists.
fn disk_volume(path: &Path) -> Option<VolumeKey> {
    let existing = nearest_existing(path)?;
    let canonical = std::fs::canonicalize(existing).ok()?;
    let meta = std::fs::symlink_metadata(&canonical).ok()?;
    Some(identify(&canonical, &meta).1)
}

fn nearest_existing(path: &Path) -> Option<&Path> {
    let mut dir = path;
    while !dir.exists() {
        dir = dir.parent()?;
    }
    Some(dir)
}

/// Add up `roots`. Blocks on the file system: call off the UI thread.
///
/// The data directory goes first, so anything also reachable another way
/// (portable logs and models, an app folder holding `Data/`) keeps the bucket
/// its place in the data directory gives it.
pub fn scan(roots: &StorageRoots) -> StorageReport {
    let mut walk = Walk {
        seen: HashSet::new(),
        totals: vec![0; Category::ALL.len()],
        total_bytes: 0,
        same_device_bytes: 0,
        disk_volume: disk_volume(&roots.data_dir),
        error_count: 0,
        first_error_path: None,
    };
    walk.walk(&roots.data_dir, Filing::Classified);
    walk.walk(&roots.log_dir, Filing::Fixed(Category::Logs));
    for model in &roots.model_paths {
        walk.walk(model, Filing::Fixed(Category::SharedModels));
    }
    for app_path in [&roots.resource_dir, &roots.executable]
        .into_iter()
        .flatten()
    {
        walk.walk(app_path, Filing::Fixed(Category::AppFiles));
    }
    StorageReport {
        buckets: Category::ALL
            .iter()
            .zip(&walk.totals)
            .map(|(category, bytes)| BucketUsage {
                category: *category,
                bytes: *bytes,
            })
            .collect(),
        total_bytes: walk.total_bytes,
        same_device_bytes: walk.same_device_bytes,
        disk: disk_space(&roots.data_dir),
        error_count: walk.error_count,
        first_error_path: walk.first_error_path,
    }
}

/// Capacity and free space of the volume holding `path` (or its nearest
/// ancestor that exists).
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    let space = volume_space(nearest_existing(path)?)?;
    (space.total_bytes > 0).then_some(space)
}

#[cfg(unix)]
fn volume_space(dir: &Path) -> Option<DiskSpace> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a valid out-pointer.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return None;
    }
    let block = st.f_frsize as u64;
    Some(DiskSpace {
        total_bytes: (st.f_blocks as u64).saturating_mul(block),
        free_bytes: (st.f_bavail as u64).saturating_mul(block),
    })
}

#[cfg(windows)]
fn volume_space(dir: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    let mut total = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out-pointers
    // are valid for its duration.
    let result = unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free as *mut u64),
            Some(&mut total as *mut u64),
            None,
        )
    };
    result.ok()?;
    Some(DiskSpace {
        total_bytes: total,
        free_bytes: free,
    })
}

#[cfg(not(any(unix, windows)))]
fn volume_space(_dir: &Path) -> Option<DiskSpace> {
    None
}

/// The folder a row's Open folder shows. Buckets of single files beside
/// others open their parent. `None` when there's nothing to show.
pub fn category_dir(category: Category, roots: &StorageRoots) -> Option<PathBuf> {
    let data = &roots.data_dir;
    match category {
        Category::Models => Some(data.join("models")),
        Category::SharedModels => Some(roots.shared_cache_dir.clone()),
        Category::MeetingAudio => Some(crate::meetings::audio_store::audio_dir(data)),
        Category::DictationRecordings => Some(data.join("recordings")),
        Category::MeetingWorkingFiles => Some(data.join("meetings")),
        Category::Logs => Some(roots.log_dir.clone()),
        Category::Databases
        | Category::Configuration
        | Category::Cache
        | Category::Backups
        | Category::Other => Some(data.clone()),
        Category::AppFiles => roots.resource_dir.clone(),
    }
}

/// This app's real locations.
fn roots(app: &AppHandle) -> Result<StorageRoots, String> {
    let data_dir = crate::portable::app_data_dir(app).map_err(|e| e.to_string())?;
    let log_dir = crate::portable::app_log_dir(app).map_err(|e| e.to_string())?;
    let model_paths = app
        .try_state::<Arc<crate::managers::model::ModelManager>>()
        .map(|mm| mm.downloaded_paths())
        .unwrap_or_default();
    // A debug build's "app" is the repo checkout.
    let release = !cfg!(debug_assertions);
    Ok(StorageRoots {
        data_dir,
        log_dir,
        shared_cache_dir: crate::managers::model::hf_cache_dir(&model_paths),
        model_paths,
        resource_dir: release.then(|| app.path().resource_dir().ok()).flatten(),
        executable: release.then(|| std::env::current_exe().ok()).flatten(),
    })
}

/// [`scan`] this app's locations. Blocks: call off the UI thread.
pub fn report(app: &AppHandle) -> Result<StorageReport, String> {
    Ok(scan(&roots(app)?))
}

/// Show a row's folder. The path always comes from the category, never from
/// the caller.
pub fn open_category(app: &AppHandle, category: Category) -> Result<(), String> {
    let dir = category_dir(category, &roots(app)?)
        .ok_or_else(|| format!("{category:?} has no folder in this build"))?;
    app.opener().open_path(dir, None::<String>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn roots_at(data: &Path, logs: &Path) -> StorageRoots {
        StorageRoots {
            data_dir: data.to_path_buf(),
            log_dir: logs.to_path_buf(),
            ..Default::default()
        }
    }

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::File::create(path).unwrap();
        std::io::Write::write_all(&mut file, &vec![7u8; bytes]).unwrap();
    }

    /// What a file really occupies, whatever the file system's block size.
    fn on_disk(path: &Path) -> u64 {
        file_size(&fs::symlink_metadata(path).unwrap())
    }

    #[test]
    fn classifies_every_bucket() {
        for (rel, want) in [
            ("models/ggml-base.bin", Category::Models),
            ("models/parakeet/model.onnx", Category::Models),
            ("models/llm/qwen.gguf", Category::Models),
            ("models/speaker/resnet.onnx", Category::Models),
            ("huggingface/hub/blobs/abc", Category::Models),
            ("meetings/audio/1-mic.opus", Category::MeetingAudio),
            ("recordings/guidy-1.wav", Category::DictationRecordings),
            ("history.db", Category::Databases),
            ("meetings.db", Category::Databases),
            ("llm_usage.db", Category::Databases),
            ("meetings/processing/1.bin", Category::MeetingWorkingFiles),
            ("meetings/embeddings/1.bin", Category::MeetingWorkingFiles),
            ("settings_store.json", Category::Configuration),
            ("window-state.json", Category::Configuration),
            ("llm-registry.json", Category::Cache),
            ("logs/guidy.log", Category::Logs),
            ("meetings-encrypted-backup-1/meetings.db", Category::Backups),
            ("meetings-unreadable-1/meetings.db", Category::Backups),
        ] {
            assert_eq!(classify(Path::new(rel)), want, "{rel}");
        }
    }

    #[test]
    fn classifies_database_sidecars() {
        for db in ["history.db", "meetings.db", "llm_usage.db"] {
            for side in ["-wal", "-shm", "-journal"] {
                let name = format!("{db}{side}");
                assert_eq!(classify(Path::new(&name)), Category::Databases, "{name}");
            }
        }
    }

    #[test]
    fn unknown_files_land_in_other() {
        for rel in [
            "custom_start.wav",
            "history.db-backup",
            "history.db.old",
            "nested/history.db",
            "meetings/notes.txt",
            "meetings/audio",
            "models",
            "recordings",
            "settings_store.json.tmp",
            "nested/settings_store.json",
            "",
        ] {
            assert_eq!(classify(Path::new(rel)), Category::Other, "{rel:?}");
        }
    }

    #[test]
    fn totals_add_up_by_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let logs = dir.path().join("logs");
        let model = data.join("models/m.bin");
        let audio = data.join("meetings/audio/1.opus");
        let stray = data.join("stray.txt");
        let log = logs.join("guidy.log");
        write(&model, 9000);
        write(&audio, 5000);
        write(&stray, 100);
        write(&log, 2000);
        let report = scan(&roots_at(&data, &logs));
        assert_eq!(report.bytes(Category::Models), on_disk(&model));
        assert_eq!(report.bytes(Category::MeetingAudio), on_disk(&audio));
        assert_eq!(report.bytes(Category::Other), on_disk(&stray));
        assert_eq!(report.bytes(Category::Logs), on_disk(&log));
        assert_eq!(
            report.total_bytes,
            on_disk(&model) + on_disk(&audio) + on_disk(&stray) + on_disk(&log)
        );
        assert_eq!(report.same_device_bytes, report.total_bytes);
        assert_eq!(report.buckets.len(), Category::ALL.len());
        assert_eq!(report.error_count, 0);
    }

    #[cfg(unix)]
    #[test]
    fn sparse_file_reports_allocated_below_logical() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        fs::create_dir_all(&data).unwrap();
        let logical = 64 * 1024 * 1024;
        let file = fs::File::create(data.join("sparse.bin")).unwrap();
        file.set_len(logical).unwrap();
        drop(file);
        let report = scan(&roots_at(&data, &dir.path().join("no-logs")));
        assert!(report.total_bytes < logical, "{}", report.total_bytes);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let outside = dir.path().join("outside");
        write(&outside.join("big.bin"), 50_000);
        fs::create_dir_all(&data).unwrap();
        std::os::unix::fs::symlink(outside.join("big.bin"), data.join("file-link")).unwrap();
        std::os::unix::fs::symlink(&outside, data.join("dir-link")).unwrap();
        let report = scan(&roots_at(&data, &dir.path().join("no-logs")));
        assert_eq!(report.total_bytes, 0);
    }

    #[test]
    fn hardlink_counts_once() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let file = data.join("models/a.bin");
        write(&file, 9000);
        fs::hard_link(&file, data.join("models/b.bin")).unwrap();
        let report = scan(&roots_at(&data, &dir.path().join("no-logs")));
        assert_eq!(report.total_bytes, on_disk(&file));
    }

    #[test]
    fn overlapping_roots_count_once_and_data_dir_wins() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let log = data.join("logs/guidy.log");
        let model = data.join("models/m.bin");
        write(&log, 2000);
        write(&model, 9000);
        let roots = StorageRoots {
            // Portable mode: the logs are inside the data directory.
            log_dir: data.join("logs"),
            model_paths: vec![model.clone()],
            // A portable install: the app folder holds `data/`.
            resource_dir: Some(dir.path().to_path_buf()),
            ..roots_at(&data, &data)
        };
        let report = scan(&roots);
        assert_eq!(report.total_bytes, on_disk(&log) + on_disk(&model));
        assert_eq!(report.bytes(Category::Logs), on_disk(&log));
        assert_eq!(report.bytes(Category::Models), on_disk(&model));
        assert_eq!(report.bytes(Category::SharedModels), 0);
        assert_eq!(report.bytes(Category::AppFiles), 0);
    }

    #[test]
    fn models_outside_the_data_dir_are_shared() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let blob = dir.path().join("hub/blobs/abc");
        write(&blob, 9000);
        fs::create_dir_all(&data).unwrap();
        let roots = StorageRoots {
            model_paths: vec![blob.clone()],
            ..roots_at(&data, &dir.path().join("no-logs"))
        };
        let report = scan(&roots);
        assert_eq!(report.bytes(Category::SharedModels), on_disk(&blob));
    }

    #[test]
    fn missing_root_gives_zero_with_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let report = scan(&StorageRoots {
            model_paths: vec![dir.path().join("gone.bin")],
            resource_dir: Some(dir.path().join("gone")),
            ..roots_at(&dir.path().join("no-data"), &dir.path().join("no-logs"))
        });
        assert_eq!(report.total_bytes, 0);
        assert_eq!(report.error_count, 0);
        assert_eq!(report.first_error_path, None);
        // The volume is still known, through the nearest existing ancestor.
        assert!(report.disk.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_dir_gives_partial_results_and_an_error() {
        use std::os::unix::fs::PermissionsExt;
        // Permission bits don't bind root.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let locked = data.join("recordings");
        let readable = data.join("models/m.bin");
        write(&locked.join("hidden.wav"), 9000);
        write(&readable, 9000);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let report = scan(&roots_at(&data, &dir.path().join("no-logs")));
        // Unlock first, or the temp dir can't be removed if an assert fails.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(report.bytes(Category::Models), on_disk(&readable));
        assert_eq!(report.bytes(Category::DictationRecordings), 0);
        assert_eq!(report.error_count, 1);
        let canonical = fs::canonicalize(&locked).unwrap();
        assert_eq!(
            report.first_error_path.as_deref(),
            Some(canonical.display().to_string().as_str())
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let file = real.join("models/m.bin");
        write(&file, 9000);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let report = scan(&roots_at(&link, &dir.path().join("no-logs")));
        assert_eq!(report.bytes(Category::Models), on_disk(&file));
    }

    #[test]
    fn disk_space_uses_the_nearest_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let space = disk_space(&dir.path().join("a/b/c")).unwrap();
        assert!(space.total_bytes > 0);
        assert!(space.free_bytes <= space.total_bytes);
        assert_eq!(space.used_bytes(), space.total_bytes - space.free_bytes);
    }

    #[test]
    fn open_folder_comes_from_the_category() {
        let roots = StorageRoots {
            data_dir: PathBuf::from("/d"),
            log_dir: PathBuf::from("/l"),
            shared_cache_dir: PathBuf::from("/hf"),
            ..Default::default()
        };
        let dir = |c| category_dir(c, &roots);
        assert_eq!(dir(Category::Models), Some(PathBuf::from("/d/models")));
        assert_eq!(dir(Category::SharedModels), Some(PathBuf::from("/hf")));
        assert_eq!(
            dir(Category::MeetingAudio),
            Some(PathBuf::from("/d/meetings/audio"))
        );
        assert_eq!(
            dir(Category::DictationRecordings),
            Some(PathBuf::from("/d/recordings"))
        );
        assert_eq!(
            dir(Category::MeetingWorkingFiles),
            Some(PathBuf::from("/d/meetings"))
        );
        assert_eq!(dir(Category::Logs), Some(PathBuf::from("/l")));
        // Single files and loose folders open what contains them.
        for c in [
            Category::Databases,
            Category::Configuration,
            Category::Cache,
            Category::Backups,
            Category::Other,
        ] {
            assert_eq!(dir(c), Some(PathBuf::from("/d")));
        }
        assert_eq!(dir(Category::AppFiles), None);
    }
}
