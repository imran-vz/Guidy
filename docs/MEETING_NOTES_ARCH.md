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
- **Search:** FTS5 indexes in `meetings.db` (store migration 12) over
  titles and descriptions, summaries and transcript lines: external-content
  tables kept in step by triggers. `meetings::search` adds prefix matching,
  typo correction from the index vocabulary, every-term-per-meeting
  matching and BM25 ranking weighted by field.
- **Titles:** `meetings::titles` writes a title and a one-line description
  from the summary (not the transcript) with the meetings model. The
  calendar event's name is kept by default
  (`meetings.prefer_calendar_titles`), and a title the user typed is kept
  when titling in bulk (`Meeting::title_source`).
- **Earlier encrypted data:** on start, `meetings::legacy` converts an
  encrypted `meetings.db`, the encrypted sidecar files and version 1
  (encrypted) tracks, checking each copy, and moves the originals into
  `meetings-encrypted-backup-<stamp>/` in the app data dir.

## Reliability overhaul (`docs/plans/meetings.md`)

- Detection is pure: `calls.rs` follows the microphone by canonical app and `call_sites.rs` judges pages. A known service counts after 2 s, WhatsApp and Slack after 4 s (they record voice notes too), a browser on an unknown page after 19 s (offered within 20 s of the mic at a 1 s poll). It works with no new permission: a browser's call is offered unnamed. "Name browser calls" reads the front tab through Automation, kept in memory and never logged. `fixtures/calls/*.json` are timed observation sequences.
- The offer is the floater's own bubble: Take notes, Not now, and "..." for Always or Never per provider (not for an unknown page). Unanswered, it closes after 20 s and the call is snoozed until its app has been off the microphone for 60 s. A call that ends asks Keep or Stop and stops after 60 s. A different service taking the microphone after that makes a separate note.
- Calendar prompts land 50-65 s before the event, for events with a link and 2+ attendees, re-read just before posting. A start links to a call event beginning within 5 minutes or ended within 15.
- Capture: each stream starts on its own, and only both failing fails the recording. A failed or dead stream is rebuilt with 0.5-5 s backoff until Stop, its track kept on the clock with silence. A wake rebuilds both. The bridge reports `state:stalled`, `state:recovered`, `state:gap:<ms>` and `state:restarted`. System audio with no callbacks while nothing plays is rebuilt quietly at most every 30 s: whether an idle tap delivers callbacks is still unverified on hardware.
- `recorder/health.rs` turns those, the audio flowing, permissions, free space and a clock into `Healthy`, `Reconnecting`, `Silent` or `Lost` per source, in `RecorderState::Recording.health`. It warns and never stops, except that 15 minutes without audio on both sources stops after 60 s. The scheduler thread runs it every second.
- Side files beside the tracks, deleted with them, with no database change: `<id>.capture.jsonl` (what happened to capture) and `<id>.speakers.jsonl` (names meeting apps showed). Why a recording ended early stays in `Meeting::error`, through processing.
- Speaker names from meeting apps are off by default (`speaker_names_from_apps`): what each app exposes through Accessibility hasn't been checked on real calls. `processing/speaker_hints.rs` only ever adds a suggestion to the picker, and absent names change nothing.

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
| B: native macOS | Swift bridge: EventKit, mic, process tap, notifications; AEC3 echo cancellation (`native/aec.rs`); Info.plist | `swift/meeting_bridge*.swift`, `swift/meeting_bridge.h`, the new fn in `build.rs`, `src/meetings/native.rs` (or `native/`), `assets/Info.plist`, `assets/Entitlements.plist` |
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
