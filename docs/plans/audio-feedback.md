# Plan: mechanical key feedback sounds

Paths are under `crates/guidy-core/src` unless noted.

## Current state
- Two call sites: start at `actions.rs:701`, blocking, after first mic samples and before `apply_mute`; stop at `actions.rs:776`, async, after `remove_mute`.
- No sound on error, cancel (`utils.rs:95`), or meetings.
- Playback: `audio_feedback.rs:126` opens a device stream per play and reads the WAV from disk.
- Settings: theme enum `settings.rs:281`, Marimba default `:631`, `audio_feedback` off by default `:1008`.
- UI: volume slider `general.rs:48`. Theme picker and preview on Debug page (`debug.rs:724-771`).
- `scripts/gen_brand_assets.py` needs numpy and PIL and rewrites icons, so don't use it.
- Hex's `thock` set is cut from an ElevenLabs MP3. Copy nothing.

## Decisions
1. New theme `mechanical`, using committed WAVs from a new stdlib-only script. No runtime synthesis or per-play variation.
2. Make it the default. Migrate stored `marimba` to it through the next free settings schema number (3 unless another branch took it). Explicit Pop and Custom stay.
3. Keep Marimba, Pop, Custom selectable.
4. `audio_feedback` stays off by default.
5. Add `SoundType::Error`, always `mechanical_error.wav` from Resource for every theme. Fire it where `recording-error`, `paste-error`, `transcription-error` are emitted (`actions.rs:724`, `~929`, `955`).
6. Cancel stays silent, so hotkey chord aborts make no sound. No capture trimming, no sink caching.

## Synthesis: `python3 -I scripts/gen_key_sounds.py`
48kHz mono 16-bit, starting on sample 0. Seeded 32-bit LCG noise, one-pole HP/LP filters. 3ms audible fade-out, then 18ms digital zeros, 80ms or less in total. Components are damped sines `a*sin(2*pi*f*t)*e^(-t/tau)`, written Hz/tau ms/a, or noise as HP-LP/tau/a.

| Sound (seed, audible, peak) | Click at 0ms | Later |
|---|---|---|
| start (0x4B01, 62ms, 0.50) | noise 2.5-9k/1.0/.55; 3100/1.2/.3; spring ping 5200/9/.035, 7300/6/.02 | Thock at 7ms: 210/14/1.0, 430/8/.45, housing 1150/6/.3, noise 400-2.5k/3/.45 |
| stop (0x4B02, 46ms, 0.40) | noise 3.5-11k/.7/.35; spring 6100/8/.03 | Clack at 4ms: 2400/3.5/.7, 1450/5/.5, 340/9/.4, noise 1.5-8k/1.5/.4 |
| error (0x4B03, 62ms, 0.50) | None: 150/12/1, 320/7/.4, 780/5/.5, noise 0-1.4k/2/.5 | At 30ms: same at x0.8, with 125Hz replacing 150 |

`--check` regenerates in memory and asserts +/-1 LSB against committed files, plus duration, peak, trailing zeros. Run the script and commit the WAVs to `assets/resources/mechanical_{start,stop,error}.wav`.

## Code changes
- `settings.rs`: `Mechanical` variant and `as_str`, default change, schema bump, `stored < N` block near `:1315`.
- `shortcut/mod.rs:603`: parse `mechanical`, make it the fallback.
- `audio_feedback.rs`: Error variant, path and base directory per sound.
- `commands/audio.rs:254`: accept `"error"`.
- `crates/guidy-app/src/ui/pages/debug.rs`: option first in the list, preview plays start, stop, error.
- `assets/locales/en/translation.json:678`: `"mechanical": "Mechanical"`, and update the i18n count.
- Docs: `NOTICE.md:13`, `docs/ARCHITECTURE.md:76`, `docs/PARITY.md:91,207`, `website/index.html:369`.

## Tests to write
- `settings.rs`: schema 2 marimba becomes Mechanical; Pop and Custom unchanged.
- `audio_feedback.rs`: Mechanical paths exist under `assets/`; Error resolves to Resource under Custom.
- `ui_tests.rs:842`: also assert `play_test_sound error`.

## Test handoff
(a) Worktree, per step: `gen_key_sounds.py --check`; parse the WAV headers with stdlib `wave`; determinism across two runs; trace the schema migration for 2, 3, and fresh settings; every match on the sound enum handles the new variants; `json.tool` on locale; rustfmt.

(b) On main after merge: `cargo test -p guidy-core audio_feedback`, settings tests, `cargo test -p guidy-app`, clippy, `cargo xtask gate`. Manual: `afplay assets/resources/mechanical_start.wav`; General > Audio Feedback on, Debug > Sound Theme > Play Sample; AirPods for clipped onset; speakers on with a short recording to check for phantom text.

## Risks
- Sink drops right after `sleep_until_end` (`audio_feedback.rs:134`); the 18ms of zeros covers a cut tail.
- Per-play stream opening may swallow the click on Bluetooth. If so, a cached sink is a separate change.
- Laptop speakers roll off below about 200Hz, so 1150Hz and 780Hz carry the sound there.
- Clicks can reach Whisper through VAD prefill (450ms), as marimba can today.
