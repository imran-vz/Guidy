# Meeting notes: implementation architecture and ownership

How `docs/MEETING_NOTES_PLAN.md` is built. Several workstreams build it in
parallel in **one checkout**, so this file fixes who owns which files and the
contracts between the workstreams.

## Decisions made at implementation time

- **Voiceprints:** the official `sherpa-onnx` crate (static, prebuilt, includes
  ONNX Runtime), linked into the main binary. The 1.0 spike (real recorded Meet
  calls) cannot run here, so the match thresholds are settings
  (`meetings.speaker_match_threshold`, default 0.65, and
  `speaker_suggest_threshold`, default 0.45). Sortformer is not used.
- **llama.cpp:** a separate helper binary, `crates/guidy-llm` (`llama-cpp-2`
  with Metal). Two static copies of ggml can't link into one binary, so a
  worker *mode* of `guidy` is impossible. The app starts `guidy-llm` next to
  its own executable.
- **Meeting data:** `meetings.db` is plain SQLite, like `history.db`. It also
  holds each meeting's processing checkpoint and per-segment voiceprints.
  `rusqlite` is still built with `bundled-sqlcipher` (CommonCrypto on macOS,
  vendored OpenSSL elsewhere), only so that `meetings::legacy` can convert a
  database an earlier version encrypted.
- **Secrets:** `keyring` 3 (Keychain on macOS), wrapped by `crate::secrets`:
  API keys. The old meetings data key is read only for that conversion, then
  kept under `meetings_data_key.backup-<stamp>` and deleted.
- **Audio at rest:** Opus (`opus` crate) at 16 kHz mono, in a chunked file
  (`.gopus` version 2) with a CRC-32 per chunk. Chunks are flushed as
  recording happens, so a crash loses at most the last chunk.
- **Earlier encrypted data:** on start, `meetings::legacy` converts an
  encrypted `meetings.db`, the encrypted sidecar files and version 1
  (encrypted) tracks, checking each copy, and moves the originals into
  `meetings-encrypted-backup-<stamp>/` in the app data dir.

## Shared contracts (already in the tree; change only with care)

- `crates/guidy-core/src/meetings/types.rs`: every shared data type and the
  event names.
- `crates/guidy-core/src/meetings/settings.rs`: `MeetingSettings`, stored as
  `AppSettings.meetings`.
- `crates/guidy-core/src/meetings/manager.rs`: `MeetingsManager`, the only
  facade the app calls. Each section names its owner. Owners implement their
  own section's methods in place, keep the signatures, and add private state
  fields to the struct as needed. If a signature really has to change, change
  it and tell the integrator in your final report.
- `crates/guidy-core/src/linear/mod.rs`: the `Team`, `Project`, `User` and
  `Label` types.

## Workstreams and file ownership

| WS | Owner of | Files |
|---|---|---|
| A: secrets + store | Keychain module, `meetings.db` | `src/secrets.rs`, `src/meetings/store.rs` (or `store/`) |
| B: native macOS | Swift bridge: EventKit, VPIO mic, process tap, notifications; Info.plist | `swift/meeting_bridge*.swift`, `swift/meeting_bridge.h`, the new fn in `build.rs`, `src/meetings/native.rs` (or `native/`), `assets/Info.plist`, `assets/Entitlements.plist` |
| C: capture | Recording session, audio files, retention, reminders, calendar prompts, tray | `src/meetings/audio_store.rs`, `src/meetings/recorder.rs` (or `recorder/`), the meeting items in `src/tray.rs` and `tray_i18n`, registering `MeetingsManager` in `lib.rs` startup |
| D: processing | VAD, batch transcription with words, voiceprints, clustering, matching, timeline merge, resumable jobs | `src/meetings/processing.rs` (or `processing/`), the speaker-embedding model download, word-timestamp support in `managers/transcription*` / `engine_supervisor` |
| E: LLM | `LlmProvider` layer, the `guidy-llm` worker + GGUF catalog, summarizer, BYOK providers | `src/llm/`, `crates/guidy-llm/`, `src/meetings/summarize.rs`, provider defaults in `settings.rs` (`default_post_process_providers`) |
| F: Linear | GraphQL client, ticket drafting, idempotent creation | `src/linear/` |
| G1: UI meetings | Meetings page, list, detail transcript, speaker picker, People tab, playback, meeting settings | `crates/guidy-app/src/ui/pages/meetings/` (except G2's files), `crates/guidy-app/src/logic/meetings.rs`, `Section`/`page!` in `main_window.rs`, `crates/guidy-app/src/meetings_backend.rs` (the meetings section) |
| G2: UI AI + Linear | Summary/action-item editor, ticket review list, Linear key + LLM provider settings, BYOK "Test connection" | `ui/pages/meetings/summary.rs`, `ui/pages/meetings/tickets.rs`, `ui/pages/meetings/ai_settings.rs`, the new BYOK UI in `ui/pages/post_processing.rs`, the AI/Linear section of `meetings_backend.rs` |

Locale strings: add English keys to `assets/locales/en/translation.json`
under your own top-level namespace (`meetings` for G1, `meetingsAi` for G2,
`linear` for F if needed, `tray` keys for C). Don't reformat the file.

## Rules for parallel work in one checkout

1. **No git commits, stashes, checkouts or resets.** The integrator commits.
   Never revert a file you don't own.
2. **Builds.** Only in `/Users/imran/projects/guidy`, never in a worktree. The
   disk is nearly full. Cargo's lock serializes concurrent builds, so waiting
   on "Blocking waiting for file lock" is normal. Prefer
   `cargo check -p guidy-core` / `cargo check -p guidy-app` and targeted
   `cargo test -p guidy-core <module>`. Don't run `cargo clean`.
3. **Keep the tree compiling.** Make changes in small, compiling steps. If a
   compile error is in another workstream's file, wait a minute and retry. Do
   not fix it unless it is a trivial one-line fix to the shared contracts.
4. **Tests.** Every module gets unit tests that run without hardware,
   network, or the real Keychain. `secrets` has an in-memory backend for
   tests, and HTTP clients are tested against a local `tokio` TCP mock.
5. **macOS-only code** is `#[cfg(target_os = "macos")]` with stubs elsewhere,
   so Windows and Linux still build.
6. Match the surrounding style. Run `cargo fmt` on your own files only
   (`rustfmt <file>`).
