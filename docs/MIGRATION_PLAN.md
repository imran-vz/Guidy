# Guidy: migrating Handy from Tauri to GPUI

Source of truth: Handy at `9e57a0d7929fcc6417ea0d6e54c1b57f62635edd` (v0.9.8), `/Users/imran/projects/Handy`.
Target: `/Users/imran/projects/guidy`, a pure-Rust desktop app that uses GPUI through [gpui-kit](https://gpui-kit.com/docs/getting-started) and follows the visual language of the Zed editor.

The release gate is that **100% of tests pass** and **every row in [`PARITY.md`](./PARITY.md) is covered by a passing test or a signed-off manual check.**

---

## 1. Requirements

### 1.1 What Handy does (the scope we must match)

Handy is a menu-bar/tray speech-to-text app that runs offline. Its main loop is: a global shortcut starts and stops recording, VAD trims the audio, a local GGUF model transcribes it through `transcribe-cpp`, optional LLM post-processing runs, and the text is pasted into the focused app.

| Area | Size in Handy | Notes |
|---|---|---|
| Rust backend (`src-tauri/src`) | 31.3k LOC, **309 unit tests** | About 40% has no Tauri coupling. Most of the rest touches Tauri only through `AppHandle`. |
| React frontend (`src/`) | 185 files | Settings window with 7 pages, onboarding, model migration, What's New, updater, and a recording overlay window |
| IPC surface | ~115 commands, 25 event types | Inventoried in PARITY.md §C and §E |
| i18n | 26 locales, 446 keys in `en` | Tray strings are shared with the backend through `build.rs` |
| Platforms | macOS (Metal), Windows x64/ARM, Linux X11/Wayland | Per-platform paste, overlay, permissions, and hotkey code |

### 1.2 Requirements for Guidy

1. **Functional parity.** Everything in PARITY.md works the same way, including per-platform conditional behavior, settings semantics and defaults, events, the CLI, and portable mode.
2. **No Handy branding.** No Handy name, copy, logo, icon, sounds-as-brand, donate/source links, or pink palette. Handy's i18n files are MIT-licensed, so we reuse their *keys and translations*, but rewrite every string that mentions the brand and parameterize the product name.
3. **Zed design language.** One Dark / One Light (MIT, from Atom) mapped onto gpui-kit's theme schema; IBM Plex Sans for UI text and Lilex for monospace (both OFL). Hairline borders, dense rows, `Kbd` chips for shortcuts, a Zed-style title bar and status bar.
4. **License hygiene.** Zed's `ui`, `theme` and `settings_ui` crates are **GPL-3.0**. Do not depend on them or copy code from them. Use gpui-kit (Apache-2.0) and gpui-pre (Apache-2.0) only.
5. **Gate.** `cargo nextest run --workspace` passes on macOS, Windows and Linux CI with 0 failures and no unexplained `#[ignore]`. A parity-coverage check (§6.3) passes.

### 1.3 Explicitly dropped (no user-visible behavior)

These go away with the webview:
- the specta/`bindings.ts` generation
- Playwright's "dev server responds" test
- the `log://log` webview plugin target (replaced by an in-process log channel)
- `WEBKIT_DISABLE_DMABUF_RENDERER`
- the `tao` patch
- portable `Data/webview`
- dead components `TextDisplay`, `MicrophoneIcon`, `TranscriptionIcon`, `CancelIcon` and `DebugPaths`

---

## 2. Target architecture

> The plan originally split the code into five crates (core, platform, i18n, ui, binary) behind a new `CoreCtx`. During P1 it proved cheaper and safer to keep the backend's shape and swap only what Tauri supplied. The layout as built:

```
guidy/                          (cargo workspace)
├─ crates/
│  ├─ guidy-core/   Handy's backend, ported. No UI and no gpui dependency.
│  │    host.rs: Tauri-shaped AppHandle (state, events, paths, store, clipboard,
│  │      opener, main-thread hop) + the `Shell` trait the UI implements.
│  │    tray.rs (tray-icon + muda), shortcut/global_hotkey_impl.rs, single_instance.rs,
│  │    permissions.rs, logging.rs, overlay.rs (placement math), lib.rs (startup, CLI, headless)
│  ├─ guidy-i18n/   runtime over the i18next JSON files (`{{x}}`, `_one`/`_other`), RTL
│  └─ guidy-app/    binary `guidy`: GPUI shell, `Backend` trait over the commands,
│       logic/* (pure ports of the React stores and hooks), ui/* (gpui-kit views),
│       windows.rs, shell.rs, playback.rs, updater.rs, ui_tests.rs
├─ xtask/           `cargo xtask test [--full] | parity | gate`
├─ assets/          themes/zed-one.json, fonts/, icons/, resources/ (sounds, tray, VAD), locales/
└─ docs/            MIGRATION_PLAN.md, PARITY.md, Handy baseline lists
```

### 2.1 Key seams

- **`host::AppHandle`.** It has the same method names the backend used on `tauri::AppHandle` (`manage`/`state`, `emit`/`listen`, `path().resolve`, `store`, `run_on_main_thread`). Most of the port was therefore an import change, and Handy's 300 tests run unchanged. Window operations go to `host::Shell`: `NullShell` in headless/CLI mode and `GpuiShell` in the app, which sends `ShellMsg`s to the GPUI main loop.
- **`Backend` trait (`guidy-app/src/backend.rs`).** Each Tauri command is a method with the same name and semantics. `CoreBackend` calls the core; UI tests run against `FakeBackend`.
- **Events.** Backend events reach `AppHandle::listen_any`. They then pass through a channel into `dispatch_event` on the GPUI loop, which updates the `AppState` and `OverlayState` entities.
- **Wire compatibility.** Serde enum values stay exactly as Handy writes them (e.g. `min2`, `days3`, not the `bindings.ts` forms).

### 2.2 Platform mapping

| Tauri piece | Guidy replacement |
|---|---|
| main webview window 680×570 | `open_window` with a gpui-kit `Root`; close hides it (`set_quit_mode`) |
| overlay webview + nspanel / gtk-layer-shell / Win32 topmost | `WindowKind::PopUp`, transparent, `focus:false`, cursor-monitor positioning math ported from `overlay.rs`; `WindowKind::LayerShell` on Wayland |
| tray plugin | `tray-icon` + `muda` on the GPUI main thread; on Linux on a dedicated GTK thread (libappindicator needs a GTK loop, which GPUI doesn't run) |
| global-shortcut plugin (the `tauri` keyboard implementation) | `global-hotkey` crate. The setting value stays `"tauri"` for compatibility and is labelled "System" in the UI. |
| handy-keys | unchanged |
| single-instance plugin | lock file + `interprocess` local socket that forwards argv (`--toggle-transcription`, `--toggle-post-process`, `--cancel`, otherwise show the window) |
| store plugin | `SettingsStore` (atomic write: temp file + rename) |
| clipboard-manager | `arboard` in core (the paste path runs off the UI thread); GPUI clipboard in the UI |
| autostart plugin | `SMAppService` on macOS 13+ (existing code) and `auto-launch` elsewhere |
| macos-permissions plugin | objc2 calls: `AXIsProcessTrustedWithOptions`, `AVCaptureDevice authorizationStatus/requestAccess` |
| dialog (`ask`) | gpui-kit `Dialog` |
| fs + asset protocol (history playback) | `rodio` player in core with play/pause/seek/position; the UI polls position |
| opener | `cx.open_url` / `reveal_path` |
| updater (minisign `latest.json`) | `cargo-packager-updater` (same manifest shape, so release CI barely changes). Velopack is the alternative; see the ADR in §7. |
| log plugin | `log` → file (500 KB, keep one) + stdout + in-memory channel for the Debug page's live viewer |
| bundling | `cargo-packager`: .app/.dmg (Info.plist mic string, entitlements, notarize), NSIS, deb/AppImage |

### 2.3 Design system (Zed look)

- **Themes.** `assets/themes/zed-one.json` contains One Dark and One Light, translated from Zed's `one.json` (MIT) into gpui-kit's schema, with a mapping table checked into the repo. The theme setting (system/light/dark) follows the OS appearance.
- **Window chrome.** A Zed-style title bar with inline traffic lights on macOS. A 200 px left nav styled like Zed's project panel (icon + label, selected row uses `element.selected`). A content column with grouped setting rows, like Zed's settings editor.
- **Footer.** Becomes a **status bar** with the model status indicator on the left and update status and version on the right.
- **Controls.** gpui-kit `Switch`, `Select`, `Slider`, `Input`, `Button` (ghost/subtle variants), `Kbd` for shortcuts, `Notification` for toasts, `Dialog`, `Tooltip`.
- **Overlay.** A floating Zed-style panel: elevated surface, 1 px border, accent dot, Lilex for live text. Sizes and animation timings follow Handy's overlay so the behavior matches (PARITY §O).
- **New brand assets.** App icon, tray glyphs (idle, recording, transcribing, warning, each with a dark variant), and feedback sounds. The sound themes stay named `marimba` and `pop` in settings for compatibility, but ship new audio.

---

## 3. Data directory (decided)

Guidy uses its own identifier **`dev.guidy.app`** and its own data dir. There is **no import or migration from Handy**. On-disk formats (settings JSON, history DB schema and migrations) are kept identical to Handy's, so the ported migration code and tests still apply.

Decisions: updater = `cargo-packager-updater`; app name = **Guidy**. Signed installers are out of scope for now; unsigned packaging is done in P6.

---

## 4. Phases and exit gates

Every phase ends green on CI (macOS, Windows, Linux) running `fmt`, `clippy -D warnings` and `nextest`. Nothing merges with a red or skipped test.

### P0: Scaffold (≈2 days)
- Set up the workspace. Pin `gpui-kit = "=0.7.3"` and commit `Cargo.lock`.
- Add the CI matrix and the `xtask parity` command (§6.3).
- Commit the theme JSON and fonts, and a hello window with a sidebar using gpui-kit `application()/init/open_window`.
- **Gate:** CI is green on all three OSes, and a `#[gpui::test]` smoke test opens the main view.

### P1: Core extraction (≈1.5–2 weeks). The largest de-risking step.
1. Copy the P (pure) modules verbatim: `audio_toolkit`, `engine_supervisor`, `catalog`, `gguf_meta`, `model_capabilities`, `transcription/{policy,chunked}`, `llm_client`, `chinese_script`, `memory`, `apple_intelligence` + swift + `build.rs` pieces, and `cli`.
   - Bring their tests along unchanged.
2. Introduce `CoreCtx`, `EventSink` and `MainThread`, then port the L (lightly coupled) modules one at a time:
   - `portable` → `settings` → `history` → `model` (+download) → `audio` → `transcription` (+live) → `coordinator` → `actions` → `clipboard`/`paste_tx`/`input` → `secure_input` → `autostart` → `shortcut` (handler, handy_keys, global-hotkey impl).
   - Each module's tests must pass before the next one starts.
3. Build the `Backend` facade with one method per Tauri command. Add a facade test for each `change_*_setting` that checks persistence and side effects (events, rebinding, tray refresh hooks).
4. Port the headless CLI path (`--transcribe-file`, `--list-devices`, `--list-models`, `--repeat`, `--json`) and the `--transcribe-worker` dispatch.
- **Gate:**
  - All 309 ported tests pass.
  - The new facade, event and migration tests pass.
  - **Golden check:** `guidy --transcribe-file fixtures/*.wav --json --model <id>` matches Handy's JSON output for the same fixtures and model (run in CI with a tiny model; skipped only when the model cache is absent).
  - Handy's real `settings_store.json` and `history.db` fixtures load and migrate identically.

### P2: Platform services and headless dictation (≈1.5 weeks)
- Tray with all menu items and dynamic model submenu; i18n labels; icon state changes with theme and recording state.
- Single-instance and remote CLI flags, signals (SIGUSR1/SIGUSR2), autostart, permissions, log sink, paths.
- Activation policy: Accessory when hidden with a tray, Reopen handling.
- An overlay window shell that responds to `ShowOverlay`/`HideOverlay` and does the positioning math.
- **Gate:**
  - A real shortcut → record → transcribe → paste cycle works on all three OSes with no settings UI. This is a manual smoke run, recorded in PARITY §HW.
  - Unit tests cover the tray menu builder (pure function: state → menu model), overlay geometry, the single-instance argv protocol, and the autostart/permission adapters (behind traits with fakes).

### P3: Overlay UI (≈1 week)
- Compact and live forms, the waveform smoothing (0.7/0.3, 9 bars), the timer, phase and loading notices, the scroll-pin behavior, cancel, and top/bottom placement.
- **Gate:** `VisualTestContext` tests for every overlay state transition in PARITY §O, plus pure tests for the bar-height formula and the effective-label logic.

### P4: Main window (≈3 weeks)
Order: shell (sidebar, banners, status bar, toasts, Cmd/Ctrl+Shift+D) → General → Advanced → Models (+ status-bar ModelSelector) → History (+ player) → Post-processing → Debug (+ log viewer, keyboard diagnostic) → About.
- Every control binds through a `SettingsModel::update(key, value)` that copies Handy's optimistic update, `isUpdating` flag and rollback behavior.
- **Gate:** each PARITY row in §W/§G/§A/§M/§H/§P/§D/§B has a `#[gpui::test]` against `FakeBackend`, asserting visibility rules, the command called with its arguments, rollback on error, and event-driven refresh.

### P5: Flows (≈1.5 weeks)
- Startup step machine (accessibility → migration → model → done), onboarding model picker, retired-model migration, What's New gate (release notes rendered with gpui-kit's markdown text), updater with the portable-installer branch.
- **Gate:** step-machine tests for every branch (new user, returning user, missing permission, retired model, error path), plus ported `portableInstaller` tests and the What's New version-selection tests.

### P6: i18n, accessibility, packaging (≈1.5 weeks)
- All 26 locales converted and debranded. `guidy-i18n` tests check that keys match `en` and that no locale mentions `Handy`.
- RTL layout for ar and he. AccessKit labels on controls.
- `cargo packager` produces unsigned packages: macOS .app/.dmg with an ad-hoc signature, NSIS, and deb/AppImage. CI builds them (`.github/workflows/ci.yml`). The updater reads a `latest.json` endpoint and public key set at build time.
- **Out of scope for now:** Developer ID signing, notarization and Authenticode.
- **Gate:** the i18n tests pass, the packaging job produces artifacts, and a fresh install and an update both pass on each OS.

### P7: Parity sign-off (≈1 week)
- `xtask parity` reports 100%.
- Manual hardware matrix (§6.4) signed off on macOS (Apple Silicon), Windows x64, and Linux X11 + Wayland.
- Performance check: idle RAM/CPU and shortcut-to-recording latency are no worse than Handy's.

Rough total: **12–14 weeks** for one engineer. P3 and P4 can overlap once P1 lands.

## 4.1 Status (2026-10)

| Phase | State |
|---|---|
| P0–P5 | Done. `cargo xtask gate` is green: all tests pass, and Handy's 300 tests, 120 commands, 25 events and 159 PARITY rows are covered. |
| P6 | Done except signing. Packaging is unsigned (see above). |
| P7 | Pending: the manual hardware checks HW-01…HW-17 in `PARITY.md` and the performance comparison. |

---

## 5. Porting rules
1. **Behavior first, refactor later.** During P1, change only what removing Tauri requires. Every diff against Handy should be explainable by "replaced `AppHandle` usage".
2. Keep Handy's module, function and test names where possible, so the PARITY matrix can trace back to the original.
3. Keep serde names, defaults and migrations byte-compatible.
4. Every Tauri command keeps its name on the `Backend` trait. The `xtask parity` check diffs the `#[tauri::command]` list in Handy against the `Backend` methods.
5. Port all logic out of the frontend into **pure Rust functions with tests**, not into view code. This covers keyboard name formatting, effective-language resolution, custom-word normalization, What's New version selection, download-speed EMA, model card status ordering, the auto-submit two-key mapping, the paste-method option matrix, and portable installer URL resolution.

---

## 6. Test strategy and the "100% passing + parity" gate

### 6.1 Layers
| Layer | Tooling | Source |
|---|---|---|
| Core unit tests | `cargo nextest` | 309 tests ported from Handy, plus new facade, event and migration tests |
| Frontend logic | Rust unit tests | `keyboard.test.ts`, `portableInstaller.test.ts`, `clipboard.test.ts` ported, plus every pure function from rule 5 |
| UI behavior | `#[gpui::test]`, `VisualTestContext` (`simulate_click`, `simulate_keystrokes`), gpui-kit `test-support`, `FakeBackend` + `RecordingSink` | One or more tests per PARITY row |
| Golden / integration | CLI JSON golden test against Handy, settings/history fixture migration, single-instance socket round trip | |
| Visual (non-gating) | `Window::render_to_image` snapshots on macOS CI | Catches regressions in the Zed look |
| Manual hardware | Checklist in PARITY §HW | Mic, permissions, paste into real apps, tray, Wayland |

### 6.2 Rules
- 0 failures, 0 flaky retries in gating jobs, and no `#[ignore]` without an `IGNORE-REASON:` comment pointing to a manual PARITY row.
- Tests that need hardware or a model are guarded by an env feature and run in a nightly job on self-hosted macOS/Windows/Linux runners.
- Coverage is reported with `cargo llvm-cov` as information only. The gate is passing tests plus parity coverage, not a line-coverage percentage.

### 6.3 `xtask parity`
- Parses `docs/PARITY.md`. Every row has an ID (e.g. `G-07`) and is either `test: <path::name>` or `manual: <checklist id>`.
- Runs `cargo nextest list` and fails if any referenced test is missing, or if any row has no test or manual reference.
- Diffs Handy's command list and event list against `Backend` and `CoreEvent`, and fails on anything missing.
- CI fails unless 100% of rows are covered. Manual rows require a sign-off entry in `docs/signoff/<release>.md`.

### 6.4 Manual hardware matrix (P7)
Run on macOS 14+ (Apple Silicon), Windows 11 x64, Ubuntu X11, and Fedora/GNOME Wayland:
- First-run permissions
- Every paste method into a text editor, a browser and a terminal
- Hold, toggle and push-to-talk activation
- Cancel shortcut
- Tray menu, including model switching
- Overlay positions top and bottom, on multiple monitors
- Autostart
- Update from version N to N+1
- Portable mode on Windows
- Secure-input warning on macOS (open a password field in Terminal)

---

## 7. Risks and mitigations
| Risk | Mitigation |
|---|---|
| gpui-pre is a weekly snapshot with breaking changes | Pin `=0.7.3`, commit the lockfile, upgrade deliberately in a dedicated PR that must pass the full suite |
| Wayland: no always-on-top or global hotkeys | LayerShell overlay; handy-keys or portal GlobalShortcuts via `ashpd`; documented as manual parity rows |
| No click-through for the overlay | Handy's overlay is also interactive (cancel button), so it isn't needed |
| Tray on Linux needs a GTK loop | The tray lives on its own thread running `gtk::main`; every native tray call is sent there |
| macOS TCC permissions need a stable signed bundle | Do dev runs from a `cargo-packager` debug .app; document this in the README |
| History audio player had HTML5 seeking | `rodio` `Sink::try_seek` on WAV with a position ticker; covered by a test using a generated WAV |
| Markdown in What's New | gpui-kit text/markdown, limited to the same allowed elements; URL scheme allowlist tested |
| Big-bang UI risk | P2 delivers usable headless dictation before any UI exists; UI pages land one at a time against `FakeBackend` |

### ADRs to write in P0
1. gpui-kit over Zed's `ui` crate (licensing).
2. Data dir identity and the importer (§3).
3. The updater choice: `cargo-packager-updater` vs Velopack.
4. How the `CoreCtx`/`Backend`/`CoreEvent` seams are designed.
