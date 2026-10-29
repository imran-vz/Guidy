# UI snapshots

Offscreen PNGs of the real Guidy windows, rendered by GPUI's headless Metal
renderer with real CoreText shaping and the app's own assets (Geist fonts,
icons, illustrations, themes). Use them to check a UI change visually without
launching the app. The installed Guidy is never touched: every backend is a
fake (`FakeBackend`, `FakeMeetingsBackend`) filled with made-up data.

macOS only. The code is in `crates/guidy-app/src/ui_snapshots.rs`.

## Run

```sh
# Everything (45 snapshots x light/dark = 90 PNGs, ~2 min after the build)
cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture

# Only what you're working on: comma-separated substrings of the name
GUIDY_SNAPSHOT_FILTER=meetings cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture
GUIDY_SNAPSHOT_FILTER=main-models,onboarding cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture

# One theme only
GUIDY_SNAPSHOT_THEME=dark GUIDY_SNAPSHOT_FILTER=main-general cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture
```

The test is `#[ignore]`d, so a normal `cargo test -p guidy-app` never runs it.

Output: `target/ui-snapshots/<name>-<light|dark>.png` (override the directory
with `GUIDY_SNAPSHOT_DIR`). Each line of output gives the path and pixel size.
Images are at 2x scale (a 900x600 window gives a 1800x1200 PNG).

## Names

| Name | What |
| --- | --- |
| `main-<section>` | Main window at its default size (`windows::MAIN_DEFAULT_*` = 900x600 pt), sidebar section selected: `general`, `history`, `meetings`, `models`, `advanced`, `postprocessing`, `usage`, `debug`, `about` |
| `main-<section>-full` | Same, but 2400 px tall, so content below the fold shows (pages scroll inside the window, so a taller window shows more) |
| `main-<section>-min` | At the minimum window size (`windows::MAIN_WIDTH/HEIGHT` = 720x480 pt): every page must still work there |
| `main-meetings-wide`, `main-models-wide` | 1440x900 |
| `meetings-people`, `meetings-settings` (+ `-full`) | Meetings page tabs |
| `msettings-<category>` | One meeting settings pane (`setup`, `general`, `you`, `recording`, `transcription`, `speakers`, `ai`, `linear`, `integrations`) in a stand-in window frame; `msettings-ai-min`, `msettings-ai-wide`, `msettings-setup-min` at other sizes |
| `mpeople-you`, `mpeople-person` (+ `-min`) | People list / detail: You selected, a person selected |
| `meetings-detail` (+ `-full`, `-wide`) | An open meeting, Notes tab (summary, minutes, action items) |
| `meetings-transcript` (+ `-full`) | The same meeting, Transcript tab (three named speakers) |
| `meetings-detail-min` | The open meeting at the minimum size (stack layout: the detail over the list, with a back button) |
| `meetings-recording` (+ `-wide`) | Recording: toolbar timer, Pause/Stop, the live transcript |
| `meetings-processing` | A meeting being transcribed (progress in the row and the header) |
| `meetings-find` (+ `-wide`) | Find in the transcript (⌘F), matches highlighted |
| `meetings-playing` | The player mid-meeting, its line marked |
| `meetings-empty` | No meetings yet |
| `onboarding-welcome` | Setup assistant, Welcome: Dictation / Meeting Notes and what each needs |
| `onboarding-permissions` | Permissions (microphone granted, Accessibility not: "Continue Without") |
| `onboarding-setup` | Choose your setup, dictation only (speech model, size, total vs free space) |
| `onboarding-setup-meetings`, `-apple`, `-cloud` | Dictation + meeting notes with summaries on this Mac / Apple Intelligence / a cloud provider (key entry) |
| `onboarding-setup-meetings-only` | Meeting notes without dictation |
| `onboarding-picker` | "Choose Another…": every speech model, language filter, search |
| `onboarding-migration` | Retired-model migration (Parakeet V2 to V3), with Later |
| `startup-app`, `startup-new` | The skeleton while the core starts: the app's shell (returning user), the assistant frame (new user) |
| `startup-error` | The core couldn't start: Show Logs / Quit |

Every onboarding and startup snapshot also has a `-min` variant (720x480).
| `overlay-live` | Dictation overlay, live style, mid-sentence |
| `recording-widget`, `recording-widget-expanded` | Meeting recording floater: collapsed pill, and its panel with live transcript |

The overlay and widget windows are transparent in the app; here they render
on the theme background. The macOS traffic lights are not drawn offscreen:
the empty space at the top left of the sidebar is where they sit.

## Determinism

Each snapshot gets a fresh app, its own fakes and fixed timestamps
(March 2026), so the same code gives the same images. GPUI runs timers and
background tasks on its test clock (`settle()` advances it and runs queued
work). Two things are not fixed:

- `Animation`s (shimmer, pulsing dots, the overlay's level bars) run on the
  wall clock, so they render at whatever phase they are in. The overlay and
  widget wait 800 ms of real time so their entrance animations finish.
- The recording widget's timer reads the wall clock (about 12:34).

## Adding a snapshot

1. If the screen needs data the fakes don't have, set it in `populate()`
   (all populated screens share it) or in the scenario's builder. Change
   `testing.rs` / `meetings_backend.rs::fake` only when a fake has no field
   for it, and don't change production code.
2. Write a builder `fn(&mut Harness, Size<Pixels>) -> AnyWindowHandle`:
   fill the fakes, call `h.install()`, open a window (`h.open_main(size)` or
   `h.open(size, titlebar, View::new)`), then drive it with entity methods
   (`main.update(...)`) or real clicks
   (`h.with_window(w, |window, cx| window.click("element-id", cx))`, ids come
   from `.id(...)` + `.test_support()` in the views).
3. Register it in `shots()` with a name and a size (`default_size()`,
   `tall_size()`, `wide_size()` or your own).
4. Run it with `GUIDY_SNAPSHOT_FILTER=<name>` and open the PNG.
