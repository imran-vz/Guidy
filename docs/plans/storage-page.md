# Plan: Storage page

Requirement: a sidebar page listing models, configuration, cache, audio and everything else, with a small bar showing disk capacity, occupied space, and the app's share.

## Current state
Registering a section:
- `crates/guidy-app/src/ui/main_window.rs`: enum :35, `id` :48, `from_id` :62, label :78, icon :92, `Pages` :137, `render_page` :451, reload on visit :313
- `ui/mod.rs`: actions :47, handlers :121, menu :193
- `logic/flows.rs`: :601, tests :995, :1007
- `ui_snapshots.rs`: :1749 (`[..; 9]`); `ui_tests.rs`: :271
- Docs: `docs/PARITY.md` W-01 (:119), `docs/UI_SNAPSHOTS.md`, `REDESIGN.md` :101
- Cmd+1 to 9 are all taken.

Stored data, under `app_data_dir` unless noted (`host.rs:294`; portable and dev overrides `lib.rs:117-125`):
- Models: `models/` (`model.rs:642`), `llm` (`local_models.rs:201`), `speaker` (`speaker_model.rs:338`), portable `huggingface/`
- Outside the data dir: shared HF cache (`model.rs:1889`), logs (`host.rs:299-307`, `guidy.log` plus rotated `.1`)
- Audio: `recordings/` (`history.rs:81`), `meetings/audio` (`audio_store.rs:105`)
- Working files: `meetings/{processing,embeddings}` (`manager.rs:1472`)
- Databases and sidecars: `history.db`, `meetings.db`, `llm_usage.db`
- Config: `settings_store.json`, `window-state.json`
- Cache: `llm-registry.json`
- Backups: `meetings-encrypted-backup-*`, `meetings-unreadable-*`
- Bundled: VAD and `transcribe-libs` (`guidy-app/Cargo.toml:73-76`)

Reusable: `format_bytes` (`downloads.rs:337`), `libc`, `open` (`host.rs:840`). Unix-only `statvfs_free` (:764) reads free only. `sysinfo` isn't a direct dependency.

## Decisions
- Disk space: new `statvfs` code for total and free. Windows: `GetDiskFreeSpaceExW` through the `Win32_Storage_FileSystem` feature, no new crate. Label free as "excludes purgeable space", since it can read lower than Finder.
- Size: allocated (`blocks()*512`) on Unix, logical on Windows, with one consistent caption.
- Scan: `symlink_metadata`, never follow symlinks; dedupe by `(dev, ino)` across the whole scan; HF models from new `ModelManager::downloaded_paths()`, canonicalized to blobs, labelled "shared"; off-UI thread with `background_spawn`.
- Buckets: Models, Shared models, Meeting audio, Dictation recordings, Databases, Meeting working files, Configuration, Cache, Logs, Backups, App files, Other. Other catches anything unclassified under the data dir.
- App files: `resource_dir` plus executable, release builds only, skipping `data_dir`.
- Excluded, with a caption: Keychain secrets, user exports, autostart plist, transient updater downloads.
- Bar: `h-2` track, `RADIUS_TRACK`. Guidy `tone.accent` (min 2px), other used `tone.text_faint`, free `tone.grid`. Same-device bytes only, clamped to used. No new colours. Text states capacity, occupied, free, and Guidy's total.
- Refresh: on `new`, each visit, toolbar Refresh, keeping old data on screen (pattern `usage.rs:46`). No timers.
- Errors: partial results; total reads "at least"; inline count shows the first unreadable path.
- Actions: per-row Open folder, resolved from the category, never an arbitrary path. Single-file buckets open the parent. Models rows link to the Models page. No clear-cache in v1: `storage.rs` deletes nothing.
- Navigation: Storage after Usage, menu item, no shortcut.

## Steps
1. `guidy-core/src/storage.rs` and `pub mod storage;`: `classify(rel) -> Category`, `scan(&StorageRoots) -> StorageReport`, `disk_space(path)`, `report(app)`, `open_category(app, c)`. Add `downloaded_paths()` to `model.rs`, the Windows feature to `guidy-core/Cargo.toml`.
2. `logic/storage.rs`: pure bar fractions and row ordering; register in `logic/mod.rs`.
3. Backend: trait and impl in `backend.rs`, fake in `testing.rs` with a `fail_settings` hook.
4. `ui/pages/storage.rs`, then register everywhere above. Icon `hard-drive` (`models.rs:611`).
5. `storage.*` and `sidebar.storage` in `en`, update the i18n count.
6. Snapshot `storage-error`; update docs and snapshot count; a PARITY W row naming real tests.

## Tests
`guidy-core`, temp dirs: `classify` for every bucket including backups and sidecars; unknown files land in Other; sparse file reports allocated below logical (Unix); symlink not followed; hardlink counted once; overlapping roots counted once; missing root gives 0 with no error; `chmod 000` dir gives partial results plus an error. Fractions clamp, with a minimum sliver.

UI: nav row appears; report renders; failure shows inline; Open folder recorded.

## Test handoff
(a) Worktree, per step:
- 1: no `metadata()` in the walk; `rg 'remove_|fs::write' storage.rs` is empty.
- 3: every `Section::` match has `Storage`; `flows.rs` tests updated.
- 4: `rg 'Section::Usage'` is paired with a Storage entry.
- 5: `python3 -I -m json.tool`; flattened key delta equals the pinned count delta.
- 6: PARITY names exist via `rg`.

Also write a stdlib-only temp-dir script that reproduces the expected scan numbers, to compare by hand against traced logic.

(b) On main after merge: `cargo fmt --all -- --check`, clippy, `cargo xtask gate`, `GUIDY_SNAPSHOT_FILTER=storage cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture`, then inspect normal, `-min` (720x480), both themes. Manual: compare with `du -sk` and Finder, unreadable dir, portable mode, Open folder per row.

## Risks
- Windows and `cfg` code stays uncompiled until merge, and Windows isn't built on this Mac at all.
- A large walk is slow, so keep the old report showing and guard reentry.
- `statvfs` on macOS uses 32-bit block counts. Check on volumes over 16 TB (unverified).
- `icons/hard-drive.svg` isn't in `assets/icons` though already used. Confirm in the snapshot.
