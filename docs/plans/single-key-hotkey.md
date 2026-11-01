# Plan: single-key dictation

Requirement: one key (Fn or Option). Hold to speak by default, release to stop. Double-press latches on until another double-press.

## Current state
- Mac and Windows default to `NativeKeys` (`settings.rs:236-243`), whose validator already accepts a lone modifier (`native_keys.rs:412-421`), and the recorder commits one (`logic/keyboard.rs:367-395`).
- Missing:
  - Chord abort: `HotkeyManager` emits only Pressed/Released, so Option+C stays pressed (`handy-keys manager.rs:33-78`).
  - The blocking manager swallows the modifier's flagsChanged down event (`native_keys.rs:113`, `handy-keys macos/listener.rs:310-312`).
  - Double-press: `HoldOrToggle` locks on a single tap (`transcription_coordinator.rs:437-459`).
- Linux defaults to `global-hotkey`, which rejects modifier-only bindings and Fn (`global_hotkey_impl.rs:50-75`).
- Routing: `handler.rs:38-50` sends edges to the coordinator; pure `CoordinatorState` at `:219`; `is_locked()` makes a press stop the session (`:361`).
- Permission: Accessibility only (`permissions.rs:1-5`). No change.
- UI: recorder `shortcut_input.rs`; activation row `general.rs:152-167`; default bindings `settings.rs:948-1000`.

## Hex reference (`/Users/imran/projects/hex`)
- Passive annotated-session tap for flagsChanged; a modifying tap breaks Finder's Option menus (`src/suppression.rs:627-650`).
- Accessibility only (`desktop/onboarding.rs:1019-1021`).
- 300 ms minimum hold and double-tap window (`dictation.rs:39`, `suppression.rs:18`).
- A key, click or extra modifier inside 300 ms discards; dirty state waits for neutral (`suppression.rs:2206-2214`, `:2258`). Locked sessions ignore chords.
- Fn is level-based from flags; Globe's system action is left alone; key code 179 isn't a chord; arrow and F-keys carry a false Fn flag (`:903`, `:2810`, `:3943`).

## Decisions
1. A binding with exactly one modifier (`fn`, `option`, `option_left`, `option_right`, Ctrl/Shift/Cmd equivalents) on a transcribe id always uses the gesture and ignores `shortcut_activation`. No new enum variant. Chords and bare keys are unchanged.
2. These bindings bypass `HotkeyManager`. The manager thread owns a lazily created non-blocking `KeyboardListener::new()` polled in its 10 ms loop (`native_keys.rs:125-139`).
3. A pure reducer owns gestures, the coordinator owns sessions. States: `Idle`, `FirstDown`, `AwaitSecond`, `SecondDown`, `Holding`, `Dirty`. Latched is the coordinator's locked session.

| State | Event | Result |
|---|---|---|
| Idle | bare modifier edge, `matches()` true | FirstDown |
| Idle | bare modifier edge, binding held, `matches()` false | Dirty |
| FirstDown | down >= ARM (200 ms) | emit HoldStart, Holding |
| FirstDown | release before ARM | AwaitSecond |
| AwaitSecond | press within DOUBLE_WINDOW (300 ms) | SecondDown |
| AwaitSecond | window expires | Idle, no output |
| SecondDown | release before ARM | emit DoublePress, Idle |
| SecondDown | down >= ARM | emit HoldStart, Holding |
| Holding | release >= SHORT_HOLD (350 ms from press) | emit HoldEnd, Idle |
| Holding | release < SHORT_HOLD | emit Abort, Idle |
| Holding | non-modifier key down | emit Abort, Dirty |
| FirstDown/AwaitSecond/SecondDown | key down, mouse down, extra modifier | Dirty, no output |
| Dirty | binding up | Idle |

Holding ignores extra modifiers and clicks. Time is passed in; no `Instant::now()` in the reducer.

4. Fn: press edges only from `key == None` events; release is level-based; `Key::Dictation` (`keycode.rs:133-136`, `:250`) isn't a chord for an Fn binding.
5. Coordinator: add `Command::Gesture` and pure `on_gesture`, reusing `on_input`.
   - HoldStart/HoldEnd: ignored if `is_locked()`, else PushToTalk press/release.
   - DoublePress: Toggle press, latches on or stops a latched session.
   - Abort: ignored if locked, else `Effect::Cancel` calling `utils::cancel_current_operation`, or drop an unlocked `pending_press`.
   - Escape still cancels a latched session.
6. macOS stuck-hold watchdog: handy-keys reconciles modifiers silently (`listener.rs:179-193`), so a release can be lost. In Holding only, poll `CGEventSourceFlagsState(0)` every 500 ms and feed a synthetic release. Never poll `CGEventTapIsEnabled`, which handy-keys documents as an IPC leak panic.
7. Recording starts after ARM, not at key-down, so Option+C never flashes the overlay or plays a sound. Costs about 200 ms if the user doesn't pause; tune on hardware. `hold_threshold_ms` stays chord-only.
8. Migration: no schema bump. Saved chords stay. Existing modifier-only bindings move to the gesture.
9. **Fresh-install default (user requirement):** macOS `option_right`; Windows `ctrl_right` (use the exact binding string `Modifiers::parse_single` accepts); Linux unchanged, because its default implementation can't do modifier-only. Any reset path that would apply a single-modifier default under the `Tauri` implementation must keep the chord (`shortcut/mod.rs:498-504`).
10. UI: "Dictation key" dropdown with Custom and presets. macOS: Fn/Globe, Right Option, Left Option. Windows/Linux: Right Ctrl, Right Alt. Use `change_binding`. Hide the activation row for single-modifier bindings. Choosing Fn shows text: set "Press 🌐 key to" Do Nothing and avoid the double-Fn system Dictation shortcut. Show the hold and double-press behaviour in the row's description.
11. Non-mac: reject lone `fn` in `native_keys::validate_shortcut` via `cfg` (`state.rs:67-69`). With `Tauri`, disable presets with a hint. Suspending gesture entries emits Abort if mid-hold.

## Implementation steps
1. New `crates/guidy-core/src/shortcut/gesture.rs`: `ModifierGesture`, `GestureIntent`, `is_single_modifier_binding`, tests; a `mod` line in `shortcut/mod.rs`.
2. `transcription_coordinator.rs`: `Command::Gesture`, `on_gesture`, `Effect::Cancel`, `send_gesture`, tests.
3. `shortcut/native_keys.rs`: `do_register`/`do_unregister` routing, listener polling, watchdog, duplicate/overlap rejection.
4. Defaults in `settings.rs`; `crates/guidy-app/src/logic/keyboard.rs` presets; `ui/pages/general.rs` dropdown; `ui_tests.rs`. Follow `ShortcutInput::commit` (`shortcut_input.rs:249`).
5. English locale keys and the count at `guidy-i18n/src/lib.rs:444`.
6. `docs/PARITY.md` rows and `HW-18`; `docs/ARCHITECTURE.md:49`.

## Tests to write
`gesture.rs`: `tap_alone_emits_nothing`, `hold_emits_start_after_arm_then_end`, `hold_under_short_hold_aborts`, `double_press_within_window_emits_once`, `press_after_window_is_a_new_gesture`, `second_press_held_is_a_hold`, `option_c_in_first_down_goes_dirty_until_release`, `key_while_holding_aborts`, `extra_modifier_while_holding_is_ignored`, `key_during_await_second_blocks_double`, `arrow_fn_flag_never_presses_fn_binding`, `dictation_key_is_not_a_chord_for_fn`, `right_binding_ignores_left_side`, `hold_release_is_not_a_first_tap`, `missed_release_via_watchdog_ends_hold`.

Coordinator: `double_press_latches_and_second_stops`, `hold_and_abort_ignored_while_latched`, `abort_cancels_unlocked_recording`, `double_press_during_processing_starts_locked_after_drain`.

Settings: `saved_chord_survives_load`, fresh-default test. App: preset row test; `fn` rejected off-mac (add to `docs/baseline/platform-tests.txt` only if `cfg`-gated).

## Test handoff
(a) Worktree, every step: rustfmt; `git diff --name-only main...HEAD` lists only that step's files; no new dependencies.

| Step | Trace by reading |
|---|---|
| 1 | Every table row has a transition and a test. No `AppHandle`, thread, or `Instant::now()`. Hand-trace tap, hold, double-press, Option+C at 50 and 400 ms, false Fn flag, `Key::Dictation` with Fn. `is_single_modifier_binding` rejects `option+shift`, `option+space`, `f13`. |
| 2 | `on_gesture` checks `is_locked()` first for HoldStart, HoldEnd, Abort. Trace DoublePress from Idle, locked Recording, Processing. `Effect::Cancel` runs only on the coordinator thread, with no blocking or re-entry through `notify_cancel`. Existing tests unmodified. |
| 3 | Single-modifier bindings never reach `manager.register`; `cancel` and chords still do. Listener dropped when no gestures remain. Mid-hold unregister emits Abort. Suspend/resume (`mod.rs:272-302`) stays correct. Watchdog only in Holding; no `CGEventTapIsEnabled`. FFI is `cfg(target_os = "macos")`. Linux and Windows paths read as valid. |
| 4 | Presets parse under `Modifiers::parse_single`. Activation row stays for chords, hidden for single modifiers. Presets disabled under `Tauri`. Fresh default correct on each OS; saved chords not overwritten. |
| 5 | `json.tool`; flattened key delta equals the pinned count delta; every new `t("...")` exists in `en`. |
| 6 | Every PARITY `test:` name has a matching `fn`; HW-18 is in the HW table; row format matches `xtask`. |

(b) On main after merge: `cargo test -p guidy-core gesture`, `transcription_coordinator`, `shortcut`, `saved_chord_survives_load`; `cargo test -p guidy-i18n`; `cargo test -p guidy-app shortcut_rows`; clippy; `cargo xtask gate`; `GUIDY_SNAPSHOT_FILTER=main-general cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture`.

Manual HW-18 on macOS with a packaged build: bind Fn, Right and Left Option; hold, speak, release pastes. Short tap does nothing. Double-press, speak, tap and long hold, still recording; double-press again stops and pastes. Option+C quick and after 400 ms held. Option+C while latched continues. Escape while latched cancels. Right bound, Left does nothing. Fn with Globe set to emoji and Do Nothing; double-Fn with system Dictation. Arrow keys with Fn bound. Option held through lock screen or Cmd+Tab ends within 500 ms. Spaceman Option-1. Secure Keyboard Entry. Saved chord after upgrade. Report whether 200/300/350 ms feel right and whether speech start clips.

## Risks
- Carbon shortcuts and Secure Input hide key-downs, so those can start dictation if held past ARM. Hex's HID tap is a follow-up.
- A keyed binding like `option+space` may never reach the gesture tap. Check tap order on hardware.
- A second Default-mode tap may disturb Finder's Option menus.
- Fn hold versus emoji picker is unverified on hardware.
