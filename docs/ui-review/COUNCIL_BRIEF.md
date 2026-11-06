# Guidy UI review council: brief

You are one of two independent reviewers on a design review council. Act as a
lead product designer with 15 years of experience shipping macOS and
cross-platform desktop apps, with deep knowledge of Apple's Human Interface
Guidelines, WCAG 2.2, and the design languages of Codex (desktop), Claude
(desktop), T3 Code, Xcode and macOS System Settings. The product owner says
the app "feels unusable" and your persona currently rates it 0/10. The goal
is a UI the persona would rate at least 9/10.

Work independently. Do not read the other reviewer's files in
`docs/ui-review/` (any file named after a reviewer other than you).

## The product

Guidy is a macOS menu-bar app for local dictation and meeting notes, written
in Rust with GPUI (`gpui-kit`). The main window is a settings-style app: a
source-list sidebar (General, History, Meetings, Models, Advanced, Post
Process, Usage, Storage, Debug, About), a unified title bar, pages of
grouped settings, plus a Meetings split view (list | detail with notes and
transcript), People, Meeting Settings, an onboarding assistant, a dictation
overlay HUD and a recording widget HUD.

Key sources if you want to see how something is built (read-only):

- Design tokens: `crates/guidy-app/src/ui/design.rs`, `assets/themes/guidy.json`
- Shared components: `crates/guidy-app/src/ui/kit.rs`
- Shell: `crates/guidy-app/src/ui/main_window.rs`, `ui/status_bar.rs`
- Pages: `crates/guidy-app/src/ui/pages/**`, onboarding in `ui/onboarding/**`
- Current design spec: `docs/design/REDESIGN.md`
- Snapshot names: `docs/UI_SNAPSHOTS.md`

## Evidence

- Guidy screenshots for the round you are reviewing are under
  `target/ui-review/round-<N>/` as `<name>-light.png` and `<name>-dark.png`
  (2x scale; a 900x600 pt window is 1800x1200 px). The macOS traffic lights
  are not drawn offscreen; the empty top-left of the sidebar is where they
  sit. Overlay/widget HUDs render on the theme background instead of being
  transparent.
- Reference screenshots: `target/ui-review/references/codex.png` (Codex
  desktop), `claude.png` (Claude desktop), `t3-code.png` (T3 Code settings).
  For Xcode and macOS System Settings use your own product knowledge.
- Open the PNGs with your image-viewing tool. Look at enough of them to
  judge fairly: every main section in both themes at the default size, the
  `-min` and `-wide` variants, the meetings screens, onboarding, and the HUDs.
  Zoom into crops when judging small text, hairlines or contrast.
- You may compute contrast ratios from pixel values if your tools allow it
  (for example with Python + Pillow run with `python3 -I`), or from the hex
  values in `design.rs` and `guidy.json`.

Do not edit any source file and do not run `cargo` (another process owns the
build). Only write your own report file.

## Scoring scale (use for every parameter)

| Score | Meaning |
|---|---|
| 0–2 | Broken: actively harms use (illegible, misleading, chaotic). |
| 3–4 | Amateur: works, but a designer would reject it on sight. |
| 5–6 | Acceptable: no blockers, visibly inconsistent or unrefined. |
| 7 | Solid professional work with a few rough edges. |
| 8 | Polished; only nitpicks remain. |
| 9 | Best in class; the lead designer would sign off. |
| 10 | Flawless; nothing to improve. |

Score each app only on what the evidence shows. Where a reference app has
too little evidence for a parameter (one screenshot), score it from your
product knowledge and mark it `(k)`.

## Scope of this refresh (from round 2)

The owner asked for a UI-only pass: no functionality changes. Score what
is in scope; list the rest under functional findings instead of letting it
hold down a parameter indefinitely.

- In scope: layout, type, colour, spacing, components, copy (wording and
  casing), how navigation is presented, where a command appears (a toolbar
  button vs a menu item), states, motion.
- Out of scope: moving a setting so that a different set of users can see
  it (for example Debug-only settings into General: Debug is only shown in
  debug mode, so that would expose hidden settings), merging or splitting
  stores (one API-key home), adding or removing commands or entry points,
  changing defaults, validation or which button Enter triggers.
- Parameters 21 (navigation) and 22 (IA) are scored on what the in-scope
  presentation achieves. Note an out-of-scope IA problem once in the
  functional list; do not count it against the score again.
- Also out of scope (round 4 clarification): hiding or removing the
  status bar's "Check for updates" link or the bar itself while idle (an
  entry point and the model-readiness status), and patching the stock
  gpui-kit controls. Stock controls draw focus as one opaque 1 pt
  ring-coloured edge (≥ 5:1 light, ≥ 10:1 dark, no halo); custom
  elements draw a 2 pt ring. `focus-N` snapshots show it on the sidebar,
  pop-ups, shortcut fields, reset buttons and switches.
- Alerts (round 5 clarification): in `kit::confirm` Return triggers the
  confirm button (Delete/Discard) and Esc cancels. The filled button is
  therefore the true default; changing which button Return triggers is
  out of scope (behaviour).
- New evidence from round 5: `focus-1…12` (sidebar focus now draws a 2 pt
  ring), `hover-sidebar`, `hover-list-row`, `hover-button`.
