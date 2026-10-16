//! Logging (replacement for tauri-plugin-log).
//!
//! Three sinks, matching Handy's targets:
//! - console (stdout, or stderr in headless mode so stdout stays parseable),
//!   filtered by `RUST_LOG` (default info);
//! - a rotating file `guidy.log` in the log dir (500 KB, keep one), filtered by
//!   the user's log level ([`crate::FILE_LOG_LEVEL`]);
//! - the `log://log` event for the debug page's live viewer, only while
//!   [`crate::LOG_STREAMING`] is on.

use crate::host::AppHandle;
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

pub const LOG_FILE_NAME: &str = "guidy.log";
pub const MAX_LOG_FILE_SIZE: u64 = 500_000;
pub const LOG_EVENT: &str = "log://log";

/// Payload of the `log://log` event. `level` follows tauri-plugin-log's
/// numbering: 1 trace, 2 debug, 3 info, 4 warn, 5 error.
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct LogRecordPayload {
    pub message: String,
    pub level: u8,
}

pub fn level_number(level: log::Level) -> u8 {
    match level {
        log::Level::Trace => 1,
        log::Level::Debug => 2,
        log::Level::Info => 3,
        log::Level::Warn => 4,
        log::Level::Error => 5,
    }
}

pub fn build_console_filter() -> env_filter::Filter {
    let mut builder = env_filter::Builder::new();
    match std::env::var("RUST_LOG") {
        Ok(spec) if !spec.trim().is_empty() => {
            if let Err(err) = builder.try_parse(&spec) {
                eprintln!(
                    "Ignoring invalid RUST_LOG value '{}': {}. Falling back to info-level console logging",
                    spec, err
                );
                builder.filter_level(log::LevelFilter::Info);
            }
        }
        _ => {
            builder.filter_level(log::LevelFilter::Info);
        }
    }
    builder.build()
}

/// Size-capped log file: when a write would exceed the cap, the file is
/// truncated and logging starts over (tauri-plugin-log `KeepOne`).
pub struct RotatingFile {
    path: PathBuf,
    max_size: u64,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    pub fn open(path: PathBuf, max_size: u64) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let size = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len())
            .unwrap_or(0);
        Self {
            path,
            max_size,
            file,
            size,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_line(&mut self, line: &str) {
        let len = line.len() as u64 + 1;
        if self.size + len > self.max_size {
            self.file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.path)
                .ok();
            self.size = 0;
        }
        if let Some(f) = self.file.as_mut() {
            if writeln!(f, "{line}").is_ok() {
                self.size += len;
            }
        }
    }
}

struct Logger {
    console: env_filter::Filter,
    stderr: bool,
    file: Mutex<Option<RotatingFile>>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static LOGGER: OnceLock<Logger> = OnceLock::new();

impl log::Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let file_level = crate::level_filter_from_u8(crate::FILE_LOG_LEVEL.load(Ordering::Relaxed));
        let now = chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]");
        let line = format!(
            "{}[{}][{}] {}",
            now,
            record.target(),
            record.level(),
            record.args()
        );

        if self.console.matches(record) {
            if self.stderr {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }

        if record.level() <= file_level {
            if let Some(file) = self.file.lock().as_mut() {
                file.write_line(&line);
            }

            if crate::LOG_STREAMING.load(Ordering::Relaxed) {
                if let Some(app) = APP.get() {
                    // Never recurse: emitting must not log.
                    let _ = app.emit(
                        LOG_EVENT,
                        LogRecordPayload {
                            message: format!("[{}] {}", record.target(), record.args()),
                            level: level_number(record.level()),
                        },
                    );
                }
            }
        }
    }

    fn flush(&self) {
        if let Some(Some(file)) = self.file.lock().as_mut().map(|f| f.file.as_mut()) {
            let _ = file.flush();
        }
    }
}

/// Install the global logger. `log_dir` receives `guidy.log`.
pub fn init(log_dir: &Path, headless: bool) {
    let logger = LOGGER.get_or_init(|| Logger {
        console: build_console_filter(),
        stderr: headless,
        file: Mutex::new(Some(RotatingFile::open(
            log_dir.join(LOG_FILE_NAME),
            MAX_LOG_FILE_SIZE,
        ))),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

/// Connect the logger to the app's event bus for live streaming.
pub fn attach_app(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_file_truncates_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.log");
        let mut f = RotatingFile::open(path.clone(), 20);
        f.write_line("0123456789"); // 11 bytes
        f.write_line("abcdefghij"); // would exceed 20 → truncate first
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "abcdefghij\n");
    }

    #[test]
    fn rotating_file_appends_to_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.log");
        RotatingFile::open(path.clone(), 1000).write_line("a");
        RotatingFile::open(path.clone(), 1000).write_line("b");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\n");
    }

    #[test]
    fn level_numbers_match_the_log_viewer() {
        assert_eq!(level_number(log::Level::Trace), 1);
        assert_eq!(level_number(log::Level::Error), 5);
    }
}
