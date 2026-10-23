# Guidy architecture

Guidy is a menu-bar/tray speech-to-text app that runs offline. It is pure Rust and uses GPUI through [gpui-kit](https://gpui-kit.com/docs/getting-started), following the visual language of the Zed editor. Its main loop is: a global shortcut starts and stops recording, VAD trims the audio, a local GGUF model transcribes it through `transcribe-cpp`, optional LLM post-processing runs, and the text is pasted into the focused app.

It runs on macOS (Metal), Windows x64/ARM, and Linux X11/Wayland.

The release gate is that **100% of tests pass** and **every row in [`PARITY.md`](./PARITY.md) is covered by a passing test or a signed-off manual check.**

---

## 1. Layout

```
guidy/                          (cargo workspace)
├─ crates/
│  ├─ guidy-core/   The backend. No UI and no gpui dependency.
│  │    host.rs: AppHandle (state, events, paths, store, clipboard, opener,
│  │      main-thread hop) + the `Shell` trait the UI implements.
│  │    tray.rs (tray-icon), shortcut/ (handler, native_keys, global_hotkey_impl),
│  │    single_instance.rs, permissions.rs, logging.rs, overlay.rs (placement math),
│  │    lib.rs (startup, CLI, headless)
│  ├─ guidy-i18n/   runtime over the i18next JSON files (`{{x}}`, `_one`/`_other`), RTL
│  └─ guidy-app/    binary `guidy`: GPUI shell, `Backend` trait over the commands,
│       logic/* (pure, tested UI logic), ui/* (gpui-kit views),
│       windows.rs, shell.rs, playback.rs, updater.rs, ui_tests.rs
├─ xtask/           `cargo xtask test [--full] | parity | gate [--full]`
├─ assets/          themes/guidy.json, fonts/, icons/, illustrations/, resources/ (sounds, tray, VAD), locales/
└─ docs/            ARCHITECTURE.md, PARITY.md, baseline/ (tests, commands, events)
```

### 1.1 Key seams

- **`host::AppHandle`.** Owns managed state (`manage`/`state`), events (`emit`/`listen`), paths (`path().resolve`), the settings store, and `run_on_main_thread`. Window operations go to `host::Shell`: `NullShell` in headless/CLI mode and `GpuiShell` in the app, which sends `ShellMsg`s to the GPUI main loop.
- **`Backend` trait (`guidy-app/src/backend.rs`).** One method per backend command. `CoreBackend` calls the core; UI tests run against `FakeBackend`.
- **Events.** Backend events reach `AppHandle::listen_any`. They then pass through a channel into `dispatch_event` on the GPUI loop, which updates the `AppState` and `OverlayState` entities.
- **Wire compatibility.** Serde enum values, settings keys and defaults are stable on disk (e.g. `min2`, `days3`). Renamed values keep a `#[serde(alias)]` for the old name.

### 1.2 Platform services

| Concern | Implementation |
|---|---|
| Main window 816×684 minimum, opens at 960×744 (design sizes × `design::SCALE`) | `open_window` with a gpui-kit `Root`; the close button hides the app into the tray |
| Overlay | `WindowKind::PopUp`, transparent, `focus:false`, positioned on the cursor's monitor (`overlay.rs`); `WindowKind::LayerShell` on Wayland |
| Meeting controls | While a meeting records, `meetings.recording_controls` picks one of two non-activating panels. **Floating** (default): a pill at the right edge of the primary display (`ui/recording_widget.rs`, model and placement in `logic/recording_widget.rs`) with a live dot, waveform and timer; a click expands it into a panel with the meeting title, a level meter per source (mic, system audio), the live transcript, pause/resume, stop and "Open notes". GPUI doesn't follow a native resize of these borderless panels, so expanding reopens the window at the panel's size. **Notch**: a borderless black panel wraps the notch (`ui/notch.rs`, placement in `logic/notch.rs` from `NSScreen` `safeAreaInsets`/`auxiliaryTop*Area`; a small island mid-menu-bar on displays without one): waveform with pause/resume on hover, elapsed time, stop |
| Live meeting transcript | On by default (`meetings.live_transcription`). The recorder copies every capture buffer to a `LiveSink` (`guidy_core::meetings::live`): per track, a speech detector cuts utterances (`live::segmenter`, partial every 1.2 s while speaking, final after a 0.6 s pause, cut at 12 s), decoded on a third transcribe worker with the meeting model, loaded only while recording. Updates go out as `meetings://live-transcript`; the stored transcript still comes from processing after the stop |
| MCP server | Off by default (`meetings.mcp_enabled`). `guidy_core::mcp` serves read-only meeting tools (`list_meetings`, `get_meeting`, `get_transcript`) over Streamable HTTP on `127.0.0.1:<meetings.mcp_port>/mcp`, refusing foreign `Origin`/`Host` headers; `guidy --mcp-stdio` relays stdio clients to it |
| Tray | `tray-icon` on the GPUI main thread; on Linux on a dedicated GTK thread (libappindicator needs a GTK loop, which GPUI doesn't run) |
| Global shortcuts | Two implementations: `global-hotkey` (stored as `"tauri"` for compatibility, labelled "System" in the UI) and `NativeKeys` (`shortcut/native_keys.rs`, a native key-listener library) |
| Single instance | lock file + `interprocess` local socket that forwards argv (`--toggle-transcription`, `--toggle-post-process`, `--cancel`, otherwise show the window); SIGUSR1/SIGUSR2 on Unix |
| Settings store | `SettingsStore` (atomic write: temp file + rename) |
| Clipboard | `arboard` in core (the paste path runs off the UI thread); GPUI clipboard in the UI |
| Autostart | `SMAppService` on macOS 13+ and `auto-launch` elsewhere |
| macOS permissions | objc2 calls: `AXIsProcessTrustedWithOptions`, `AVCaptureDevice authorizationStatus/requestAccess` |
| Confirmations | gpui-kit `Dialog` |
| History playback | `rodio` player with play/pause/seek/position; the UI polls position |
| Opening URLs/paths | `cx.open_url` / `reveal_path` |
| Updater | `cargo-packager-updater` reading a minisign-signed `latest.json` |
| Logging | `log` → file (500 KB, keep one) + stdout + in-memory channel for the Debug page's live viewer |
| Bundling | `cargo-packager`: .app/.dmg (Info.plist mic string, entitlements), NSIS, deb/AppImage. Packages are unsigned for now. |

### 1.3 Design system

The design language is technical drawing: graphite surfaces, hairline grids, uppercase mono labels and a single signal-yellow accent. Tokens and building blocks live in `crates/guidy-app/src/ui/design.rs`; colours the stock components need live in `assets/themes/guidy.json`. A clickable mock of the main screens is in `docs/design/guidy-mock.html`.

- **Licensing.** Zed's `ui`, `theme` and `settings_ui` crates are **GPL-3.0**. Do not depend on them or copy code from them. Use gpui-kit (Apache-2.0) and gpui-pre (Apache-2.0) only.
- **Themes.** `assets/themes/guidy.json` contains Guidy Dark and Guidy Light. The theme setting (system/light/dark) follows the OS appearance. `design::tone` adds the surface, line and accent tones the theme schema has no slot for. Fonts are Geist for UI text and Geist Mono for labels and numbers (both OFL).
- **Scale.** Sizes in code are design sizes: `design::dp(n)` is `n × design::SCALE` (1.2), and the theme's rem sizes in `guidy.json` carry the same factor for the stock components. Hairlines stay `px(1.)`; the dictation overlay and the notch panel are sized to the screen and not scaled.
- **Type.** Body 13 px, titles 15 px, page headlines 17 px (design sizes, before the scale). Section names, column heads and metadata are `design::eyebrow` (10.5 px uppercase mono). Numbers are `design::mono`.
- **Window chrome.** The title bar reads "Guidy / <page>". The sidebar and status bar share the frame colour, and pages sit in an ink "well" inset into the frame (`WELL_INSET`, `RADIUS_WELL`).
- **Surfaces.** `design::panel` (hairline border, rim light on the top edge) holds sections divided by hairlines; settings groups are an eyebrow over a panel. Facts are a `stat_grid` (eyebrow over mono value).
- **Glass and motion.** Every panel has a rim light. In-progress work gets `design::sheen` (a slow sweep) on its surface or track, and `design::shimmer_text` on its label. All of it is static when macOS asks for reduced motion.
- **Drawings.** `design::blueprint` is a grid stage with crosshair marks; `design::illustration` stacks three single-colour SVG layers (lines, soft fill, accent) from `assets/illustrations/`, and `design::callout` hangs a label off a dashed leader. Drawings are used where they explain something: onboarding (what each permission is for), About and empty History. They are generated from 3D geometry by `scripts/illustrations.py` so every object shares one camera.
- **Controls.** gpui-kit controls. Actions are pill buttons (`design::Pill`): primary in the accent, secondary filled. Shortcuts render as key caps. Tabs are `design::segmented`.
- **Overlay.** A floating glass panel: near-opaque surface, hairline edge, rim light, signal dot, Geist for live text, Geist Mono for the timer (PARITY §O).
- **Brand assets.** App icon, tray glyphs (idle, recording, transcribing, warning, each with a dark variant), and feedback sounds. The sound themes are named `marimba` and `pop` in settings.

---

## 2. Data directory

Guidy uses the identifier **`dev.guidy.app`** (`host::APP_IDENTIFIER`), which names the per-user data and log directories. On-disk formats (settings JSON, history DB schema and migrations) are versioned and covered by fixture tests.

---

## 3. Test strategy and the "100% passing + parity" gate

### 3.1 Layers
| Layer | Tooling | Covers |
|---|---|---|
| Core unit tests | `cargo test` | Backend modules, the `Backend` facade, events, settings/history migrations |
| UI logic | Rust unit tests in `guidy-app/src/logic` | Keyboard name formatting, effective-language resolution, What's New version selection, model ordering, flows, and other pure functions |
| UI behavior | `#[gpui::test]`, `VisualTestContext` (`simulate_click`, `simulate_keystrokes`), gpui-kit `test-support`, `FakeBackend` | One or more tests per PARITY row |
| Integration | Real-model transcription through the engine worker (`cargo xtask test --full`, needs a local model), legacy settings/history migration, single-instance argv forwarding | |
| Manual hardware | Checklist in PARITY §HW | Mic, permissions, paste into real apps, tray, Wayland |

### 3.2 Rules
- 0 failures in gating jobs, and no `#[ignore]` without an `IGNORE-REASON:` comment.
- Tests that need a model or real hardware are `#[ignore]`d and run with `cargo xtask test --full`, which builds the `guidy` binary and passes the model via `GUIDY_TEST_MODEL`.
- CI (`.github/workflows/ci.yml`) runs `fmt`, `clippy -D warnings`, and `cargo xtask gate` in one job, then builds an unsigned macOS package. Jobs run on a self-hosted mac-mini runner (an ephemeral Tart VM booted per job by [sand](https://github.com/khoi/sand), label `guidy`); pull requests from forks and Dependabot use GitHub's `macos-latest` instead. Windows and Linux are left out until there are machines to test them on.

### 3.3 `cargo xtask parity`
- Every test listed in `docs/baseline/tests.txt` must still exist. Tests that only build on some platforms are listed in `docs/baseline/platform-tests.txt` and are required only where they build.
- Every command in `docs/baseline/commands.txt` must have an implementation, and every event in `docs/baseline/events.txt` must still be emitted.
- Every row in `docs/PARITY.md` has an ID (e.g. `G-07`) and a cover of `test: <path::name>` or `manual: HW-xx`. The check fails on unknown tests, unknown manual checks, or uncovered rows.
- `cargo xtask gate` runs `test` then `parity`; this is what CI runs.

### 3.4 Manual hardware matrix
Run on macOS 14+ (Apple Silicon), Windows 11 x64, Ubuntu X11, and Fedora/GNOME Wayland, and sign off the PARITY §HW rows per release:
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

## 4. Risks and mitigations
| Risk | Mitigation |
|---|---|
| gpui-pre is a weekly snapshot with breaking changes | Pin `gpui-kit = "=0.7.3"`, commit the lockfile, upgrade deliberately in a dedicated PR that must pass the full suite |
| Wayland: no always-on-top or global hotkeys | LayerShell overlay; the `NativeKeys` shortcut implementation; covered by manual parity rows |
| No click-through for the overlay | The overlay is interactive (cancel button), so it isn't needed |
| Tray on Linux needs a GTK loop | The tray lives on its own GTK thread; every native tray call is sent there |
| macOS TCC permissions need a stable signed bundle | Do dev runs from a `cargo-packager` debug .app |
| History audio seeking | `rodio` `try_seek` with a position ticker the UI polls (`guidy-app/src/playback.rs`) |
| Markdown in What's New | gpui-kit text/markdown, limited to an allowed set of elements; URL scheme allowlist tested |
