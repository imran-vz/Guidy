# Plan: diarization reliability

Paths are under `crates/guidy-core/src/meetings/processing/` unless noted.

Current path: Silero VAD spans, optional turn split (system only), transcribe, one embedding per span, average-linkage clustering, known-person matching, text echo drop at merge. Editing works through pins (`pipeline.rs:1618-1760`, `naming.rs:395-428`).

## Root causes, ranked
Reading proves the mechanisms exist, not how much each contributes. Only measurement confirms the ranking.
1. One label per multi-voice line. Turn splitting needs TitaNet on disk (`jobs.rs:801,835`), and consent defaults off (`settings.rs:179`, `logic/onboarding.rs:78`). A line of up to 30 s, merged across 600 ms gaps (`vad.rs:70-71`), gets one blended voiceprint, and those long lines become stored prints first (`naming.rs:323-330`), poisoning later matches.
2. The default model is the one the code distrusts. `pipeline.rs:95-97` says ResNet34 scores two voices on call audio about as alike as one, yet an empty setting uses it (`jobs.rs:754-763`). Consent installs TitaNet but doesn't select it (`list.rs:513`).
3. Short segments clustered as equals. Units from 1.5 s are embedded (`pipeline.rs:141`). `turns.rs:64-66` records 1.5 s similarity dipping to ~0.15 inside one voice: distance .85, above `max_threshold` .65. Thresholds were never calibrated per model (`docs/MEETING_NOTES_ARCH.md:9-13`).
4. `choose_cut` over-splits (`cluster.rs:200-214`). Heights `.20,.25,.38,.49` clamp the cut to .35, making one voice three clusters.
5. Mic and system get mixed once a voice is enrolled. A user line under .5 joins system clustering (`pipeline.rs:1326-1328`). In a mixed cluster the is-this-the-user check is skipped (`:1423`). Echo units are clustered too, as echo flags only run at merge.
6. Echo detection is text only (`timeline.rs:41-61,100-111`). A one-word reply matching nearby system words is deleted; misrecognized bleed is credited to the user (`pipeline.rs:808-809`).

Needs measuring: the effect of 24 kbps Opus (`audio_store.rs:53`) on embeddings.

## Other bugs and cleanup
- Deepgram loses room speakers: `embedded` empty (`pipeline.rs:1341-1342`), only system labels applied (`:1385`). Comment at `:1375` is wrong for this path.
- A segmenter error discards the voiceprint scan (`turns.rs:232`) and stops splitting for the meeting (`pipeline.rs:614`).
- An embed error kills the job (`pipeline.rs:938`); "installed" only checks file size.
- Possible panic: a line moved to a new cluster with no separation vector can be overwritten by re-attachment (`:1469`), so `renumber[&c]` panics (`:1507`).
- Redo speakers never re-splits lines (`jobs.rs:409-421`).
- `cluster.rs:526` test name says "both sides agree", but a one-sided attach is allowed (`:295`).
- Duplicates: `ensure_speaker_model` repeats `ensure_file`; four size checks (`speaker_model.rs:193,349,402,693`); `download_verified*` only in tests and e2e; `cluster_units` looks test-only (confirm by grep).
- Stale docs: explorer says VPIO (`docs/meeting-pipeline-explorer.html:481`), but `swift/meeting_bridge.swift:298` dropped it for AEC3. `speaker_model.rs:137,652` misdescribes downloads.

Hex has no clustering: system is Them, mic is You (`src/meeting.rs:316-317`). Its signal-level bleed classifier `src/meeting/echo.rs` is the useful part.

## Decisions
1. Mic is the user by default. Uncertain stays the user. No schema or `CACHE_VERSION` change. "Others on mic" needs a mic-only cluster with at least 8 s of speech and a centroid under the voice threshold.
2. Separate clustering per track. Mic labels offset by system cluster count. Tracks join only through known-person matches. Also fixes the Deepgram bug.
3. Empty `speaker_separation_model` means automatic: `effective_separation_model()` picks TitaNet if installed, else ResNet34. Use at `jobs.rs:754` and `manager.rs:790`. Known-person matching stays on ResNet34.
4. `better_turns: true` in onboarding. Checkbox stays visible, existing users' choices untouched. Adds ~107 MB.
5. Two-tier clustering in `cluster.rs`: cluster only units >= 3 s; assign shorter units with a voiceprint (from `min_embed_ms`, now 1 s) to the nearest duration-weighted centroid if similarity >= .35 and lead >= .1; else `attach_by_time`. Dissolve clusters under 6 s total back into assignment. Fewer than 2 anchors uses the current path. Starting values, tune with the harness.
6. `choose_cut`: accept a gap only when `hi > default_threshold`, else the default. Add `ClusterParams::for_model(id)` with identical values until measured.
7. Echo: compute flags before clustering and exclude echo units. Text matching requires >= 3 mic tokens. Phase 2, only if the harness shows surviving bleed: `echo_signal.rs` (Bleed always drops, Local never drops, unclear falls back to text).
8. Redo re-splits: `resplit_pending` (`serde(default)`) in the cache; `voiceprints()` cuts units at detected changes; each word goes to the piece holding its midpoint; skip units with no words.
9. Fallbacks: a segmenter error continues with scan only; a separation-model error falls back to ResNet34 with a warning; skip re-attachment for lines with a `voices[i]` match. Cancellation still propagates.
10. `speaker_prints` drops prints with similarity under .5 to the speaker centroid.
11. Not now: overlap modelling, word-level labels, attendee-count cap, Sortformer, removing Deepgram.

## Boundary with recording
Use only `AudioSource` (`processing/mod.rs:81-85`), `TrackReader`, `Meeting.{mic,system}_audio_path`. Do not touch `native/`, `recorder/`, `audio_store.rs`, `calls.rs` or Swift. Contract: separate 16 kHz mono f32 tracks, sample `n` captured at `n/16000` s from one shared start, gaps padded, skew under 100 ms (proposed bound). Missing track is `None`. `<id>.capture.jsonl` is optional and read-only. Speaker names from meeting apps arrive through a separate optional file owned by the meetings plan; if absent, nothing changes.

## Steps and tests
Tests use synthetic vectors, reusing `Lcg`/`jitter`. Don't rename baseline tests.
1. Decision 9. `pipeline/tests.rs`: moved line without vector doesn't panic; failed separation model falls back. `turns.rs`: failing segmenter still returns scan changes.
2. Decision 6. `cluster.rs`: `.20,.25,.38,.49` gives one cluster; a gap straddling the default still splits.
3. Decision 5. `cluster.rs`: noisy short vectors make no clusters; tiny cluster dissolves; long units dominate the centroid; fewer than 2 anchors falls back.
4. Decisions 1, 2. `pipeline/tests.rs`: user-like mic line never joins a system cluster; Deepgram keeps room speakers.
5. Decision 7 text part. `timeline.rs`: one-word reply kept, three-word echo dropped; echo units excluded from clustering.
6. Decisions 3, 4. Update affected onboarding tests.
7. Decision 8. Redo re-splits by word midpoint and keeps pins.
8. Decision 10. `naming.rs`: an outlier print isn't stored.
9. Harness: `cargo xtask diar-eval <manifest.json>` shells to an ignored test in `processing/eval.rs`. Manifest: `system.wav`, optional `mic.wav`, `ref.rttm`. Pure unit-tested scorer: DER with 250 ms collar and Hungarian mapping, speaker-count error, purity, coverage, unknown-speech share, mic "you" precision and recall. Grid: {ResNet34, TitaNet} x {turns on/off} x {old/new clustering} x cut values, embeddings cached by (model, track, span). No audio or RTTM committed.
10. Cleanup and docs: dedupe, stale comments, explorer, ARCH doc.

Phase 2 echo classifier isn't in this branch; it waits for harness results.

## Test handoff
(a) Worktree, every step: `git diff --check`; rustfmt; no recording-owned file changed; grep callers of changed signatures including `pipeline/tests.rs` and `crates/guidy-app`; no baseline test renamed.

| Step | Trace by reading or script |
|---|---|
| 1 | Segmenter error through `find_changes`: scan candidates survive; cancel propagates at `pipeline.rs:608`. Lines with `voices[i]` never overwritten, so `renumber[&c]` can't miss. |
| 2 | `.20,.25,.38,.49` gives cut .5, one cluster. `.3,.45,.8` gives .625, two clusters. Empty, one-height, all-equal give default. Re-derive existing `cluster.rs` and `two_close_voices_cache` expectations. List changes. |
| 3 | Every unit has exactly one outcome. Labels stay dense after dissolving. Fewer-than-2-anchors matches old behaviour. Durations pair with the right vectors. No division by zero. |
| 4 | Trace no enrollment, enrolled user, in-room guest, Deepgram. Only known-person match can mix tracks. Mic offsets don't collide with Deepgram labels. |
| 5 | Echo flags computed once and agree at merge. Hand-run `timeline.rs:222-306` against the 3-token rule. List changes. |
| 6 | Table: setting `""`, `resnet34`, `titanet_large` x TitaNet installed or not. `jobs.rs` and `manager.rs` share the function. Check `ui_tests.rs:5013`, `requirements.rs:554`, `onboarding.rs:511-533`. A stored `false` isn't overridden. |
| 7 | Old checkpoints parse. `transcripts`/`embeddings` lengths match `units`. Flag clears before save; resume can't split twice. A word on a cut goes to one piece only. `cache_covers` passes. |
| 8 | Missing centroid doesn't drop all prints. 20-print cap and longest-first hold. |
| 9 | Hand-check scorer: perfect gives DER 0; swapped labels give 0; known all-wrong value; collar applied. `#[ignore]` with `IGNORE-REASON`. |
| 10 | Removed helpers have no callers, including `e2e.rs:42`. |

(b) On main after merge: `cargo test -p guidy-core --lib meetings::processing` (per module), `cargo test -p guidy-app`, clippy, `cargo xtask gate`. Real-model ignored tests: e2e per `e2e.rs:6-10`, `speaker_model::tests::embeds` per model, `turns::tests::calibrate`. `cargo xtask diar-eval`, baseline on a pre-change commit first. Adopt each default only if real-call DER improves and "you" precision doesn't fall; otherwise revert that decision.

Manual: headphones with 3+ remote speakers; speakers on; rapid handoffs; enrolled voice with and without a guest; redo on an old meeting; name then process next; corrupt TitaNet; corrupt segmenter; Deepgram with enrolled voice and guest; mic-only and system-only; cancel and resume; fresh onboarding versus upgrade.

## Risks
TitaNet clustering thresholds are unproven and add processing time. Decisions 5 and 6 may raise unknown-speaker share or merge similar voices. Poisoned stored prints stay until re-named. No real labelled data exists in `fixtures/`.
