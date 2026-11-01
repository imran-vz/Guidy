# Plan: meeting detection and recording

Bar: `reliability-scorecard.md`, modelled on Granola, then Hex. Keep Guidy's cloud summarization untouched. Paths are under `crates/` unless they start with `assets/`. Hex paths are relative to `/Users/imran/projects/hex`.

## Findings: how Hex "hooks" Chrome
No real hook: no extension, native messaging, remote debugging, or `execute javascript`.
1. Reads CoreAudio `kAudioProcessPropertyIsRunningInput`, then bundle ID and PID, about every 500 ms (`src/microphone_activity.rs:405-430`, `src/meeting/watcher.rs:193`). No samples read.
2. Walks parent PIDs, with special cases for WebKit.GPU and `avconferenced` (`src/microphone_activity.rs:272-350`).
3. Only if a browser holds the mic and isn't snoozed, runs `/usr/bin/osascript` for the front window's active tab URL and title (`src/dictation/context.rs:350-356`), about once a second on its own thread, 2 s limit.
4. Falls back to Accessibility `AXURL` for Firefox (`src/accessibility.rs:75`).

Background tabs are deliberately not read. Named services: Meet, Banshee, Zoom, Teams, Slack, FaceTime. Other sites get a generic offer after 20 s. No telemetry package in `Cargo.lock`; outbound calls identified are update checks, dev notes, and model downloads (not a full network audit). `window_log.rs` and `sdk.js` serve Annotate, not meetings; don't copy them. **Adopt the mic and tab read; keep URLs in memory, never log them; send nothing off-device.**

Granola adds: EventKit calendar prompts at T-60 s for 2+ attendees; `ax_meeting_detector`, per-app mic attribution; custom pop-ups instead of native notifications; speaker names from Zoom, Teams and Chrome Meet through Accessibility; per-source activity bubbles; system capture auto-restart; end on mic release or 15 min without audio; a call within 15 min after an event links to it.

## Permissions
| Permission | Enables | Guidy |
|---|---|---|
| Microphone | Your track | Has; keep |
| System Audio Recording (`NSAudioCaptureUsageDescription`) | Process tap, other participants | Has; keep |
| Calendar | Titles, attendees, prompts | Has; keep on opt-in |
| Automation (`NSAppleEventsUsageDescription`, `automation.apple-events`), one prompt per browser | Tab URL in Chrome, Safari, Edge, Brave, Arc, Opera, Vivaldi | **Add**, optional, asked in context |
| Accessibility | Window titles (`swift/meeting_bridge.swift:1649`), Firefox `AXURL`, speaker names | Keep existing grant; never required for meetings |
| Screen Recording | Hex's ScreenCaptureKit fallback | **Do not request** |
| Notifications | Reminders | Has; keep |
| Hex Microphone driver | Virtual mic, unrelated to meetings | **Do not adopt** |

Native apps need mic, system audio and CoreAudio metadata only. Browser calls add optional Automation to name the site; without it they get a generic offer after 20 s.

## Weaknesses (`guidy-core/src/meetings/` unless noted)
Detection: only Zoom, Meet, WhatsApp (`calls.rs:21-25`). Meet needs Accessibility titles (`:96-103`). Other browser calls never offered (`:114-119`). No-bundle processes dropped, no PID lookup (`swift/meeting_bridge.swift:1640`). Any `com.apple.WebKit` counts as Safari (`calls.rs:62`). Run clock with no gap check (`:147-160`). No left-the-call prompt or per-provider choice.

Capture (`guidy-core/swift/meeting_bridge.swift`): either stream failing fails everything (`:1221-1243`). Tap rebuild tries once (`:1077-1087`). No dead-stream detection; silence timers pad forever (`:490-503`, `:951-966`). Restart depends on `AVAudioEngineConfigurationChange` (`:460-476`), no default-input listener though the device is pinned (`:513`). No sleep/wake handling or sleep prevention. `MicPlan` ignores a selected mic (`:380-388`).

User-facing: capture errors held until stop (`recorder/mod.rs:462-469`). A failed disk write closes the track while the UI still shows recording (`:983-990`). `RecorderState` has no health (`types.rs:595-609`). Crash recovery records no reason (`mod.rs:880`). `start` has no preflight or free-space check (`:302`). No stop on Quit (`guidy-app/src/lib.rs:156`).

Ahead of Hex, keep: CRC-checked Opus chunks synced every 5 s and tolerant of a torn tail; host-time `TimelineAligner`; AEC3; resumable processing; cloud providers.

## Decisions
- Default policy Ask. No browser extension.
- Thresholds: known service 2 s; unknown browser page 20 s; a gap over 10 s restarts the count; snooze until absent 60 s. Poll every 1 s (`CALL_POLL`, now 2 s). 8 s end grace before the left-the-call prompt.
- **Supersedes D16 (stops were manual only), to match Granola:** when the call ends, show Keep or Stop. With no answer after 60 s, stop automatically. Also, 15 min with no audio on both sources warns, then stops after 60 s with the reason shown. Always-record sessions follow the same rule. A real call starting straight after another ends (mic released then re-acquired by a different source) starts a separate meeting, satisfying R11.
- Twist: detection works with zero new permissions; Automation only improves naming. If one source is lost, keep recording the other. Live per-source health. Audio kept for re-transcription.
- Update `docs/MEETING_NOTES_ARCH.md` and `MEETING_NOTES_PLAN.md` with these changes.

## Phase 1: pure detection
Files: `meetings/calls.rs`, new `meetings/call_sites.rs`.
- Replace `CallApp` with `CallSource`: Zoom, Teams, Slack, FaceTime, Webex, Discord, WhatsApp, Meet, `BrowserSite(host)`, `BrowserUnknown`. Keep serde names compatible.
- Extend `InputApp` with `pid`, `canonical`, `is_browser`, `page: Option<PageInfo{host, path}>`.
- `Provider` key, `Detection::{Offer, Record}`, `CallPresence` for left-the-call.
- `call_sites.rs` classifies Meet room paths, `teams.microsoft.com`, `teams.live.com`, `app.zoom.us/wc`, `*.webex.com`, `meet.jit.si`, `whereby.com`. Add Arc to browsers (Hex misses it).

Tests: 3-6 s voice-note bursts give no offer; 20 s unknown page offers; finding the page mid-run keeps the count; sleep gap restarts it; dismissal snoozed until 60 s absent; lookalike hosts rejected; tab switching gives no second offer; back-to-back sources split. Fixture `fixtures/calls/*.json`, timed observation sequences.

## Phase 2: signals and flow
- Swift: `guidy_mb_audio_input_processes`, JSON with pid, bundle, parent chain. Add C symbols to `meeting_bridge.h`.
- New `meetings/native/browser_tab.rs`: `/usr/bin/osascript` off-thread, one query at a time, 2 s kill. Dialect table from Hex (NOTICE row). 60 s page memory. Test parser and script strings.
- `assets/Info.plist`, `assets/Entitlements.plist`: Apple Events usage string and entitlement.
- `meetings/settings.rs`: `call_providers`, `name_browser_calls` (default false).
- `ui/recording_widget.rs`, `logic/recording_widget.rs`: custom in-app prompt with Record, Not now, and "..." (Always/Never); no Always for unknown pages; 20 s timeout closes only the offer; Keep or Stop on left-the-call.
- Calendar: verify the existing calendar path against R8 (prompt T-60 s, 2+ attendees, correct title, 15 min link to event). Fix gaps, reusing the existing code.
- `ui/pages/meetings/settings.rs`: Detected calls section, "Name browser calls" button. Locale keys.

## Phase 3: capture resilience (`swift/meeting_bridge.swift`)
- Start each stream independently; fail only when both fail; a failed stream keeps retrying.
- Watchdog on existing timers: 3 s without callbacks or `!engine.isRunning` triggers rebuild, retry with 0.5-5 s backoff until Stop.
- Default-input listener. `NSWorkspace` wake rebuilds both streams. `IOPMAssertion` while recording.
- Events: `state:stalled`, `state:recovered`, `state:gap:<ms>`.
- `MicPlan` honours a selected mic; otherwise keep Bluetooth-to-built-in.

## Phase 4: health and recovery
- New `recorder/health.rs`: pure reducer from events, levels, clock to `Healthy | Reconnecting | Silent | Lost` per track. Warns, never stops. All-zero system audio 120 s during a detected call; 30 s zero mic; writer failure; R14's dead stream within 10 s.
- `recorder/mod.rs`: preflight permissions, refuse only if both denied; warn below 500 MB; keep the channel unbounded (dropping misaligns tracks), warn past 60 s backlog.
- `types.rs`: `#[serde(default)]` `health` on `RecorderState::Recording`.
- Capture log: append-only `<id>.capture.jsonl`, no DB migration; `audio_store.rs` deletes it with the tracks.
- `recover_interrupted` sets "Recording was interrupted; saved up to HH:MM". Quit calls a bounded stop in `guidy-app/src/lib.rs`.
- UI: per-source indicators (Me, Them) with live level, health in the widget, Recovered banner and Process action in `pages/meetings/detail.rs`.

Tests: reducer transitions via `FakeCapture::fail_stream`; recovery reason text; torn `.gopus` fixture; preflight matrix; 15-min silence and left-the-call timers with a fake clock.

## Phase 5: speaker names from meeting apps
Granola's feature. Optional, needs Accessibility, off without it. New `meetings/native/speaker_hints.rs` plus a Swift AX reader: sample the active-speaker name from Zoom and Teams desktop and Meet in Chrome once a second during a recording. Write `<id>.speakers.jsonl` (time, name), local only. In `processing/`, one small reader maps cluster to the name with the most overlap, as a suggestion the user can edit, only if absent names change nothing. This is the last phase; AX trees change often and each app needs checking on hardware. If the trees can't be verified without a real call, ship it disabled behind a setting and say so.

## Boundary with diarization
Recording owns detection, capture, Swift, `recorder/`, the `.gopus` writer, health, and setting status to `Recorded`. It guarantees two 16 kHz mono tracks on one timeline, gaps filled with silence, mic as the user. Format unchanged. Diarization owns `processing/`; the Phase 5 reader is the only addition there, in a new file plus one call site.

## Test handoff
(a) Worktree, each phase: rustfmt; `swiftc -parse crates/guidy-core/swift/meeting_bridge.swift` (parses only, no artifacts); `plutil -lint` on plists; `json.tool` on locale and fixtures; `osascript` snippets run for syntax with `osacompile -o /dev/null` if it needs no target apps; serde compatibility traced against old rows; every `CallApp` use migrated (grep); hand-trace each fixture through the detector; Swift and Rust FFI signatures compared line by line; no URL in any log call (grep `log::`, `tracing::`); no new outbound calls (grep `reqwest`).

(b) On main after merge: `cargo test -p guidy-core meetings`, `cargo test -p guidy-app`, clippy, `cargo xtask gate`, meeting snapshots. Then real-call runs of the scorecard, recorded in its results log.

## Manual only, on real calls
Each app's bundle IDs, especially Teams, Webex (`Cisco-Systems.Spark`?), Discord (`com.hnc.Discord`?), which are unverified. Automation prompts per browser; Arc, Safari, Firefox pages. Whether a muted call app keeps the mic. AirPods mid-call, 24 kHz HFP. Lid sleep. Revoking permission mid-recording. `kill -9`. Full disk. Whether an idle tap keeps delivering callbacks.

## Risks
- Tap stall, highest. Hex says Core Audio delivers silence continuously (`src/meeting/capture.rs:24`); Guidy's comment says a tap may deliver nothing (`meeting_bridge.swift:736`). Test first. If callbacks stop when idle, the watchdog must rebuild quietly, not report a stall.
- Host time may not advance during sleep, so tracks may omit that stretch. Check on hardware.
- Repeat Automation dialogs annoy users: query only after opt-in.
- Mic can hear feedback clicks during recording; meetings play none.
- Always-record legal exposure stays with the user, so it must always show visibly.
