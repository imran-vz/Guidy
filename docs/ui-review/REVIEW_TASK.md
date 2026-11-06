# Review round task (rounds 1+)

Read `docs/ui-review/COUNCIL_BRIEF.md` (role, evidence rules, scale) and
`docs/ui-review/RUBRIC.md` (the agreed 45 parameters and their 9-targets).
Your task names the round `N` and your reviewer id.

Evidence:

- Current Guidy: `target/ui-review/round-N/` (both themes, every screen).
- Previous Guidy: `target/ui-review/round-<N-1>/` and the baseline
  `target/ui-review/round-0/`.
- References: `target/ui-review/references/` (codex.png, claude.png,
  t3-code.png) plus product knowledge for Xcode and System Settings.
- What changed this round: `docs/ui-review/README.md` (round log). Treat it
  as a claim to verify, not as evidence.

Do not read the other reviewer's files for this round. You may read your
own report from earlier rounds to stay calibrated. Do not edit source and
do not run cargo.

## Output: `docs/ui-review/round-N/<your-id>.md`

1. **Scores.** One table: every rubric parameter with columns
   `Guidy round N`, `Guidy round N-1`, `Codex`, `Claude`, and a one-line
   reason for the round-N score that cites snapshot names. Score the
   references on the agreed rubric too (re-use your earlier reference
   judgement where it still holds). Then: mean of the 45 (one decimal) for
   each column, and a holistic ship score for Guidy round N.
2. **Regressions.** Anything that got worse since round N-1, with evidence.
3. **Remaining problems, ranked.** Up to 20, most severe first. For each:
   what, evidence (snapshot names, both themes where relevant), the rubric
   parameters it holds down, and a concrete fix (tokens, sizes, colours,
   layout) with the likely source file. Mark the ones that block a 9.
4. **What would raise each parameter still below 9** — one short line per
   parameter, so the next round can target them.
5. **Functional findings** (not visual), listed separately.

Your final message: the four means, the ship score, and the five most
severe remaining problems, in at most 10 lines.
