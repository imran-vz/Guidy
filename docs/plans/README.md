# Reliability and input overhaul: shared rules

Plans in this directory, each owned by one implementer on one branch:

| Plan | Branch | Merge order |
|---|---|---|
| `audio-feedback.md` | `feat/mechanical-feedback` | 1 |
| `single-key-hotkey.md` | `feat/single-key-hotkey` | 2 |
| `storage-page.md` | `feat/storage-page` | 3 |
| `diarization.md` | `feat/diarization-reliability` | 4 |
| `meetings.md` | `feat/meeting-reliability` | 5 |

`reliability-scorecard.md` defines what "as reliable as Granola" means and is the
acceptance test for the meeting work.

## Build rule (user requirement, no exceptions)

Disk is limited. In worktrees and subagents, never run `cargo build`, `check`,
`test`, `clippy`, `xtask`, or anything that compiles or creates `target/`.
`rustfmt --check --edition <edition> <files>` and stdlib-only `python3 -I` scripts
are allowed. The main session builds and tests only on `main`, after a merge.

## Testing rule (user requirement)

Every change gets thoroughly tested by a Codex subagent.

- In the worktree, after each plan step: commit the step, then have a Codex tester
  run the no-compile checks in that plan's test handoff against the worktree.
  Fix what it finds and re-test before starting the next step. It reports logic
  checks as "traced, not executed".
- On `main`, after each merge: a Codex tester runs the post-merge checks. Failures
  are fixed before the next branch merges.

## Working rules

- Code will not be compiled before merge, so read every signature you call, and grep
  every caller of a signature you change, including tests.
- Do not rename or remove tests listed in `docs/baseline/tests.txt`. `xtask parity`
  checks them. PARITY rows must name tests that exist.
- Shared files take additive edits only, placed near related code to limit conflicts:
  `settings.rs`, `types.rs`, `ui_tests.rs`, `ui_snapshots.rs`, `testing.rs`, `backend.rs`,
  `docs/PARITY.md`, `assets/locales/en/translation.json`.
- The pinned key count at `crates/guidy-i18n/src/lib.rs:444` must rise by exactly
  the number of keys added. If it conflicts at merge, recompute it.
- Other locales fall back to English. Add English keys only, except where a plan says otherwise.
- Hex is at `/Users/imran/projects/hex`, MIT, © 2026 Christopher Langton. It is reference only:
  never run it. Copied code needs a `NOTICE.md` row with the full MIT text.
- No telemetry and nothing new sent off-device. Keep cloud summarization providers intact.
- Commits: no AI attribution or `Co-authored-by` trailers. Keep the configured author.
- Do not push and do not merge. The main session merges.
- Line numbers in plans were read from `main` at `b75ee44` and may drift. Verify them.
