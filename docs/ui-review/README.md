# Guidy UI refresh: review log

A UI-only refresh driven by a two-reviewer design council. No functionality
changes; anything functional the council finds goes to
[`FUNCTIONAL_BACKLOG.md`](FUNCTIONAL_BACKLOG.md).

## Outcome

**Target reached in round 11:** both reviewers score Guidy 9.0 (403 of
45 × 10) and would ship it (9/10), with no blockers. Guidy now scores
above both reference apps on both reviewers' rubrics (Codex desktop 7.8,
Claude desktop 8.0). Baseline on the same rubric: 4.5 (Opus) and 5.9
(Codex). 11 review rounds after the baseline; 42 agent runs (24 council
reviews: 2 in Phase 1 and 2 per round, plus 18 implementation agents in
rounds 1–5; rounds 6–11 were implemented by the orchestrator). Every round's checks: full workspace tests, clippy with
`-D warnings`, rustfmt; at the end `cargo xtask gate` (298 baseline tests,
119 commands, 25 events, 167/167 PARITY rows).

The ten parameters the product owner named, baseline vs final, with the
two reference apps (scores: Opus / Codex):

| Parameter | Guidy round 0 | Guidy round 11 | Codex app | Claude app |
|---|---|---|---|---|
| Typography | 5 / 7 | 9 / 9 | 8 / 8 | 9 / 8 |
| Font weight usage | 5 / 7 | 9 / 9 | 8 / 8 | 8 / 8 |
| Color contrast | 5 / 5 | 9 / 9 | 8 / 7 | 8 / 7 |
| Information hierarchy | 4 / 5 | 9 / 9 | 8 / 8 | 8 / 9 |
| Typographic hierarchy | 4 / 5 | 9 / 9 | 8 / 8 | 8 / 8 |
| Design language | 4 / 7 | 9 / 9 | 8 / 8 | 9 / 8 |
| Padding and spacing | 5 / 7 | 9 / 9 | 8 / 8 | 8 / 9 |
| Width and layout measure | 4 / 4 | 9 / 8 | 8 / 8 | 9 / 9 |
| Color and theme | 4 / 7 | 9 / 9 | 8 / 8 (k) | 8 / 8 (k) |
| Overall consistency | 3 / 7 | 9 / 9 | 8 / 8 (k) | 9 / 8 (k) |

`(k)`: reference scored from product knowledge (one screenshot each).
The remaining sub-9 items are recorded trade-offs (`PLAN.md`, "Decisions
made during the loop": fixed 200 pt pop-ups, the status bar, the stock 1 pt
focus edge) and polish listed in `round-11/*.md`.

## Council

| Reviewer id | Model | Effort |
|---|---|---|
| `opus` | Claude Opus 5.5 | high |
| `codex` | Codex GPT-6.1 Sol | high |

Each reviewer works independently and does not read the other's report.

## Files

| File | What |
|---|---|
| `COUNCIL_BRIEF.md` | Shared brief: role, evidence, scoring scale |
| `PHASE1_TASK.md` | Phase 1: research the rubric, score the baseline |
| `RUBRIC.md` | The agreed rubric every round is scored against |
| `round-N/<reviewer>.md` | Each reviewer's report for round N |
| `FUNCTIONAL_BACKLOG.md` | Bugs and functional ideas, not worked on here |

Screenshots: `target/ui-review/round-N/` (regenerated with the headless
snapshot harness, `docs/UI_SNAPSHOTS.md`; not committed) and
`target/ui-review/references/` (Codex, Claude, T3 Code).

## Scores

Mean of all rubric parameters (0–10) and the holistic "would the lead
designer ship this" score. Round 0 was scored on each reviewer's own
Phase 1 rubric (opus: 45 parameters, codex: 40); from round 1 both use
the agreed 45-parameter `RUBRIC.md` and re-score the references on it.

| Round | Guidy (opus) | Guidy (codex) | Ship (opus / codex) | Codex app (opus / codex) | Claude app (opus / codex) |
|---|---|---|---|---|---|
| 0 (baseline) | 4.6 | 6.0 | 3 / 5 | 7.8 / 7.8 | 8.0 / 8.0 |
| 0 re-scored on agreed rubric | 4.5 | 5.9 | — | — | — |
| 1 | 7.0 | 7.5 | 6 / 7 | 7.8 / 7.8 | 8.0 / 8.0 |
| 2 | 7.6 | 8.4 | 7 / 8 | 7.8 / 7.8 | 8.0 / 8.0 |
| 3 | 8.1 | 8.7 | 8 / 8 | 7.8 / 7.8 | 8.0 / 8.0 |
| 4 | 8.7 | 9.0 | 8 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 5 | 8.9 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 6 | 9.0 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 7 | 8.9 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 8 | 8.9 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 9 | 9.0 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| 10 | 8.9 | 8.9 | 9 / 9 | 7.8 / 7.8 | 8.0 / 8.0 |
| **11 (final)** | **9.0** (403/45) | **9.0** (403/45) | **9 / 9** | 7.8 / 7.8 | 8.0 / 8.0 |

## Rounds

### Round 0: research and baseline

- Rendered 98 screens × 2 themes (196 PNGs) with the headless harness.
- Both reviewers researched a rubric (Apple HIG, WCAG 2.2, Nielsen,
  Gestalt, the reference apps), scored Guidy, Codex and Claude, and listed
  25 problems each. Merged into `RUBRIC.md` (45 parameters).
- Shared top problems: flat type hierarchy (everything 13/11 pt), yellow
  accent used for ~15 roles, monospace everywhere, dark surfaces nearly
  identical (1.05:1), non-text contrast of switches/inputs/sliders
  1.3–2.1:1, icon-only meeting toolbar, names truncating behind badges,
  nested navigation in Meetings, inconsistent casing and status colours.
- Functional findings moved to `FUNCTIONAL_BACKLOG.md`.

### Round 1: foundation and page pass

Implementation: foundation by the orchestrator; English copy, Meetings
reading views, Meeting settings + People + onboarding, Models + settings
pages, and the HUDs by five parallel Opus 5.5 (high) agents, each owning
its own files. 40 files, about +3,300 / −1,700 lines. 478 tests pass,
clippy clean.

- **Foundation:** system font (SF Pro) with tabular figures for numbers;
  monospace only for IDs, paths and code. New type scale: toolbar title 15
  semibold, object title 17, group header 13 semibold primary, body 13/18,
  reading 14/21, helper 12/16, caption 11. Spacing: page 24 × 20, rows
  10 × 14 (min 36), groups 24 apart, header gap 8. Radii 4/5/8/10. Dark
  surfaces lifted (content `#181818`, sidebar `#202020`, panel `#222222`).
  Control edges and switch/slider tracks ≥ 3:1; opaque focus ring. Neutral
  badges, a shared status palette, stat values at 17 pt, static progress
  tracks, centered 680 pt form column, centered alerts, sidebar grouped
  into four clusters with clearer icons.
- **Copy:** 129 English strings moved to HIG casing (sentence case labels,
  Title Case buttons and menus).
- **Meetings:** labeled Copy Notes + More menu instead of 9 icons, compact
  inline player, count badges on tabs, summary without card-in-card in
  reading type, compact transcript metadata, readable speaker-edit
  affordance, one empty state, neutral Record with red dot, no accent on
  speakers or timestamps.
- **Meeting settings, People, onboarding:** category list only when the
  form keeps ≥ 520 pt, rows stack under 220 pt, AI catalogue names never
  truncate behind badges, wrapping instructions, price basis stated once,
  status words with icons, quieter assistant frame, compact cloud layout
  at 720×480, "Starting Guidy…" label, plainer startup error.
- **Models and settings pages:** stable model columns, 960 pt catalogue
  cap, one shortcut field style with a fixed reset column, three button
  styles, History detail sized to content, storage legend, Debug split no
  longer clips, status bar status semantics.
- **HUDs:** upright overlay text, red live dot, tabular timer, the
  collapsed recording pill shows elapsed time, no decorative sheen loops.

Round 1 review: Opus 7.0 (ship 6), Codex 7.5 (ship 7). Verified
regressions: the compact player collided with its end time while playing;
toolbar text commands lost their border; the discard-alert snapshot no
longer reached its dialog (harness click below the fold; the guard's unit
test still passes); Meeting settings switched to pop-up navigation at
900 pt; cloud onboarding pushed provider/key below the fold at 900×600;
the minimum AI pane showed no catalogue rows. Claims that did not hold:
alerts were still top-anchored, Debug still clipped.

### Round 2: blockers from round 1

- **Orchestrator (shared):** NSAlert-like confirm (400 pt, upper third,
  caution icon, solid red destructive button), light sidebar selection
  1.26 → ~1.4:1, opaque slider track, pop-ups hug their value, single-line
  rows 36–40 pt, setting rows wrap the control under the label when the
  label would get < 220 pt, static busy labels (no shimmer), stat labels
  wrap, 2 pt focus on custom segments and disclosures, discard scenario
  rendered tall so it reaches its dialog. Scope note added to the brief
  (no setting moves that change who can see it).
- **Meetings (agent):** People and Meeting settings moved from a title-bar
  segmented control into indented sidebar rows under Meetings (same
  destinations, no new sections; ↑/↓ follows visual order); content tabs
  are a plain 28 pt tab strip with underline and count badges; the player
  gets its own row under 560 pt (track ≥ 160 pt, muted fill); one m:ss
  timecode; steady live dot; per-line transcript icons on hover/selection;
  one neutral Record style; filled pause/play glyphs; back button leading
  at minimum size; 28 pt header icon targets.
- **Settings panes, People, onboarding (agent):** 180 pt category list
  back at 900 pt (pop-up only below a 480 pt form), form left-anchored next
  to it with a 17 pt pane heading; AI pane shows search + two models at
  720×480, provider form folds to one line once a key is saved; onboarding
  is full-window (no card), centred column, pinned footer; cloud
  provider/key in the first viewport at both sizes; one capability line
  per model in the picker; People stacks list → detail when narrow;
  startup error names the cause with details in a selectable box.
- **Models, History, pages (agent):** bordered text commands, 24 pt icon
  targets; History detail titled by its first sentence, Copy + More menu,
  floating undo toast; model descriptions wrap with "Required by" on its
  own line; Advanced explanations wrap fully; Debug split into
  Settings | Logs; Usage 2×2 stats when narrow; centred error blocks for
  Storage/Usage; copy fixes ("Show menu bar icon", "Clipboard (⌘V)",
  "Shortcut").
- **HUD (agent):** light-mode meter bars in accent ink (5.6:1), one filled
  pause glyph, overlay Cancel 24 pt, Reduce Motion on the panel fade.
- Checks: 479 tests pass, clippy clean, fmt clean. 198 PNGs (new
  `debug-logs`).

Round 2 review: Opus 7.6 (ship 7), Codex 8.4 (ship 8). Both confirmed the
round-1 structural blockers fixed (Meetings navigation, alerts,
onboarding card). Regressions: hugging pop-ups broke the shared reset
column; the History title repeated its body; two selection fills in one
sidebar; compact Models pushed active downloads below the fold. Remaining
blockers: one concept drawn several ways (selection fills, checkbox
colours, outlined vs filled glyphs, two back styles), mixed control
heights (19.5 vs 24 pt), faint seek track (1.13:1), focus halo, small
targets, the spinner ignoring Reduce Motion.

### Round 3: consistency and controls

- **Orchestrator (shared):** one opaque focus edge (stock halo off),
  `tone.track` for seek/progress/meter remainders (≥ 3:1), 26 pt standard
  control height, `kit::trailing` (fixed 200 pt control column with the
  reset in a fixed slot, so resets align while short pop-ups hug),
  Reduce-Motion-aware `kit::spinner`, group headers 15 semibold, filled
  stop glyph.
- **Meetings, shell, HUD (agent):** seek bar on `tone.track` (3.1:1 light,
  3.6:1 dark) with a 24 pt hit region; one selection token for sidebar,
  child rows and lists; 26 pt controls; Record/Stop/Cancel as bordered push
  buttons; focus ring on content tabs; 28 pt Edit and transcript tools;
  static stage glyph where a percentage shows; recording toolbar without
  the duplicated title; text-only empty state (one Record); Generate
  Summary inside its empty block; sidebar narrows to 172 pt below 760 pt;
  speaker names semibold (the system font draws 500 as 400).
- **Settings panes, People, onboarding (agent):** accent-ink checkboxes and
  radios everywhere (`AccentMark`), filled play glyphs, one back style,
  outline buttons and stock disabled state, 26 pt controls, pop-ups in the
  shared column; AI status in words ("Anthropic key needed"), "default"
  once, ~51 pt catalogue rows; "Meeting notes" category (no clash with
  General) under a "Meeting settings" list header; onboarding heading in
  the system font; startup skeleton draws the real sidebar; copy pass.
- **Models, History, pages (agent):** every pop-up/shortcut row in
  `kit::trailing` (one reset column); 26 pt controls; Models rows with
  fixed size/status/action slots and Delete in a trailing More menu,
  downloading row in the first viewport at 720×480; History header is the
  date + Copy + More (no repeated title), labeled "Saved" filter and "Show
  in Finder"; Storage free space outlined; error sentences lead with the
  cause; copy and long-description pass.
- **Orchestrator:** real sidebar width on every page below 760 pt, slider
  resets in the shared column, one description-join helper for Models and
  the picker, startup sidebar uses the real section icons and clusters.
- Checks: 481 tests, clippy and fmt clean. 202 PNGs (new
  `meetings-played`, `meetings-tab-focus`).

Round 3 review: Opus 8.1 (ship 8), Codex 8.7 (ship 8). Guidy now scores
above both reference apps on both reviewers' means. Regressions: the
compact AI pane lost its first complete catalogue rows (Codex's sole
blocker); the slider value wrapped ("300 / ms"); the AI status line
wrapped beside its segmented control; a reset could sit 96 pt from a
hugging pop-up. Remaining Opus blockers: persistent status bar (ruled out
of scope: it carries an entry point and model status), focus drawn two
ways, two back styles, three pop-up width rules, progress/slider
remainders not on the track token.

### Round 4: last blockers

- **Orchestrator (shared):** one pop-up rule (exactly the 200 pt column,
  long values truncate), slider value in a 56 pt no-wrap column, opaque
  progress remainders and accent-ink slider/progress fills, toolbar title
  17 / group headers 15 / rows 13 (2 pt steps), 16 pt panel inset,
  NSAlert-style icon (app mark on a 48 pt tile with a caution badge for
  destructive alerts), eight `focus-N` snapshots of keyboard focus on
  General, scope note on the status bar and stock focus.
- **Meetings, People, HUD (agent):** People's compact detail uses the
  toolbar-leading back like Meetings (in-content back bar removed; new
  `meetings-people-*` shell snapshots); all three transport buttons always
  present (as in the original); progress shown once when the processing
  meeting is selected; meeting list 280 pt above 1280 pt; static pressed
  states on HUD commands; model ID in the code font; 4/8/12/16 gaps.
- **AI pane, settings panes, onboarding (agent):** compact AI pane gains
  ~66 pt: status on its own line, the On this Mac / Cloud provider switch
  on the heading row (local first everywhere), 28 pt saved rows, "Get API
  Key" link, two complete catalogue rows at 720×480; exact model IDs only
  when they differ from the name; category list on the sidebar fill;
  neutral Recommended chip; compact startup sidebar fits; plural-aware
  migration button.
- **Models, History, pages (agent):** Sound theme preview moved out of the
  reset slot; laptop glyph for "Meetings on this Mac"; Streaming/Translate
  as toggle buttons; "Q8_0 quantization"; Post Process add/remove as a
  24 pt segmented pair; History at minimum size opens a detail view with
  the toolbar back (Return opens, Esc returns); model IDs and paths in the
  code font; copy fixes; Paste method / Clipboard option explanations in
  the pop-up items.
- **Orchestrator:** slider backing track on `tone.track`; restored the
  Deepgram "press Enter" hint; English key count test updated for the 32
  strings the refresh added.
- Checks: full workspace tests pass (guidy-app 481, guidy-core 1,165,
  guidy-i18n 8, …), clippy and fmt clean. 224 PNGs (new `focus-1…10`,
  `meetings-people-*`).

Round 4 review: Opus 8.7 (ship 8), Codex 9.0 (ship 9 — Codex signs off;
remaining items are polish). Opus blockers: sidebar keyboard focus shown
only by a 1.1:1 fill shift; rows wrapping one by one (mixed layouts in a
group); Meeting settings' category list reads as a second sidebar. Opus
also read the filled destructive alert button as a wrong default; Return
does trigger it, so the drawing is correct (clarified in the brief).

### Round 5: Opus blockers and Codex polish

- **Orchestrator (shared):** 2 pt focus ring on the selected sidebar row
  while the sidebar has focus; setting rows keep the control inline down
  to a 180 pt label and, when they do wrap, keep it right-aligned in the
  trailing column; form columns left-anchored at the page inset like every
  other page; `focus-7/9/11/12` and `hover-*` snapshots.
- **Meeting settings, People (agent):** while Meeting settings is shown,
  its nine categories (Setup with its count, …, Integrations) are a third
  sidebar level under it, so the middle category column and the compact
  pop-up are gone at every width; ↑/↓ walks them; deep links select the
  right category. People rows share a 16 pt avatar slot; list header
  "Known people"; People detail left-anchored at wide sizes.
- **Pages (agent):** Models and Debug left-anchored; an accessory slot
  after the 200 pt column (Sound theme preview, model refresh) so pop-ups
  keep 200 pt and resets stay leading; Storage error shows the expected
  data path in code font; description fragments joined with " · " and no
  inner periods; History's pushed detail hides the list filters;
  download failure reasons as an inline caption; clearer Usage headings.
- **Onboarding, AI pane (agent):** one 20 pt leading slot for icons and
  checks (all row text at one x); compact cloud setup shows provider, key,
  help line and link in the first viewport; new `msettings-ai-local`
  snapshots; neutral "Default" chip, consistent sizes.
- Checks: full workspace tests pass (key-count test updated to 1,360),
  clippy and fmt clean. 242 PNGs.

Round 5 review: Opus 8.9 (ship 9 — Opus signs off; all three of its
round-4 blockers fixed), Codex 8.9 (ship 9; dipped from 9.0 because the
three-level sidebar pushed Storage/Debug/About below the fold at
900×600). Remaining items were polish: People subtitles truncating the
voiceprint count, sidebar density while categories are expanded, compact
category indent, two 17 pt titles stacked in Meeting settings, light
progress fill vs remainder separation, repeated information in the local
AI pane.

### Round 6: final polish (orchestrator)

- People subtitles lead with the voiceprint count (a long email
  truncates after it).
- While Meeting settings' categories are listed: 8 pt cluster gaps and
  22 pt category rows; compact categories indent one step past their
  parent's name.
- Meeting settings: the toolbar title names the shown category (its
  sidebar row is selected under "Meeting settings") and the pane drops
  its own 17 pt heading; the AI pane's status line and On this Mac /
  Cloud provider switch share one row.
- Light-mode slider and progress fill darkened to `#5c3f00` (9.3:1 on the
  page, 3.0:1 against the `#8f8f8a` remainder).
- Local AI pane: "offline" said once; a model's size line only once it is
  downloaded (the Download button carries it before).
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 6 review: Opus 9.0 (ship 9), Codex 8.9 (ship 9). Opus noted the
"offline said once" claim had not held (reworded, still twice) and a stray
"· model" line start when the AI status wrapped at 720 pt. Codex: the
expanded sidebar still crowded the global destinations and category rows
had shrunk to 22 pt (below the 24 pt target).

### Round 7: sidebar density and last copy (orchestrator)

- Sidebar rows 26 pt; category rows back to 24 pt (target size); when
  the list still overflows (debug builds with Post Process enabled), a
  fade at the bottom shows that it scrolls. A typical sidebar with the
  categories open fits at 900×600.
- Local AI pane says "offline" once (status line only).
- AI status line stays on one line: the condition first, the model name
  truncating after it (no wrapped line starting with "·").
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 7 review: Opus 8.9 (ship 9), Codex 8.9 (ship 9), no blockers.
Opus re-scored contrast down for a defect it had missed earlier: the light
"Set Up…" chip text used the raw theme warning (3.15:1). It also measured
that the expanded sidebar still clipped About for a typical user (the
round-7 log's "fits" claim did not hold). Both reviewers found the new
sidebar fade made the last row look disabled.

### Round 8: contrast tokens and sidebar fit (orchestrator)

- Light warning token `#c86b00` → `#a35400` (5.5:1 on white) and success
  `#2f9a58` → `#23804a` (4.9:1): every place that draws status text with
  the theme colours now passes.
- Expanded sidebar: no row gaps, 6 pt cluster gaps; a typical sidebar
  with the categories open needs ~508 of 536 pt at 900×600. The fade is
  gone (debug builds with Post Process on scroll like any source list).
- Local AI status on one line (the "· This Mac · offline" tail truncates
  first); local model sizes use the Models page's formatter.
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 8 review: Opus 8.9 (400/45, ship 9), Codex 8.9 (ship 9). Two
regressions from round 8 itself: the compact local AI status overflowed
into its switch (heading and model could not shrink), and the sidebar used
different metrics collapsed vs expanded, so rows shifted. Not fixed as
claimed: the AI pane still showed 2.4 / 5.2 GB vs Models' 2.5 / 5.0 GB.
The light "Set Up…" chip reached 4.46:1, just under 4.5.

### Round 9: regressions from round 8 (orchestrator)

- AI status leads with the model ("Qwen3 4B Instruct · This Mac ·
  offline"; the toolbar already says "AI summaries"); the model never
  truncates, the trailing part truncates with an ellipsis. Verified at
  720×480 in both themes before logging.
- One set of sidebar metrics in both states: 26 pt rows, 24 pt category
  rows, no row gaps, 8 pt cluster gaps.
- AI pane sizes and its Download button in decimal megabytes like Models
  and onboarding (2.5 / 5.0 GB). Verified in `msettings-ai-local*`.
- Light warning `#944c00` (5.5:1 on the chip tint); light slider and
  progress fill `#523800` (3.4:1 against the track, 10.9:1 on white).
- Design decisions that the two reviewers weigh differently are recorded
  in `PLAN.md` ("Decisions made during the loop").
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 9 review: Opus 9.0 (403/45, ship 9), Codex 8.9 (401/45 = 8.91,
ship 9). Opus found one regression: the startup sidebar placeholder kept
round-8 spacing, so rows jumped on load. Codex traced the production data:
the backend's `size_mb` is MiB, so round 9's decimal conversion was right
only for the fixture (production would show 2.4 / 4.8 GB vs Models'
2.5 / 5.0). Codex's four sub-9 parameters were #33 (sizes), #21 (three
navigation levels) and #8/#15 (the recorded fixed-pop-up trade-off).

### Round 10: two-level navigation and production sizes (orchestrator)

- Meeting settings moved from under Meetings to the end of the settings
  cluster (Models, Advanced, Post Process, Meeting settings); its
  categories are one level under it. The sidebar now has two levels at
  most (People stays under Meetings). Same destinations, same commands;
  ↑/↓ follows the new visual order (test updated to the new order).
- AI pane sizes converted from MiB back to bytes and shown in decimal
  units, matching Models and onboarding for production data; the fixture
  now uses the catalogue's real MiB values, names and descriptions.
- Unavailable Apple Intelligence: the helper starts with the instruction
  (the chip already says "Not available").
- Startup sidebar placeholder uses the real sidebar's metrics (no jump).
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 10 review: Opus 8.9 (402/45, ship 9), Codex 8.9 (ship 9). Both
found the startup sidebar placeholder still in the old order (the round-10
log's "no jump" claim did not hold: spacing matched, order did not), and
Opus found People and the category rows at the same depth drawn with two
geometries. Codex: Storage's Guidy and Other segments ~1.8:1, no scroll
cue when the sidebar overflows, the 8B model's RAM advice twice. Process
note: part of `target/ui-review/round-9/` was re-rendered after the
round-9 review began (Opus noticed); from round 11 a round's folder is
never written after its review starts.

### Round 11: verified fixes (orchestrator)

Each item below was checked in the round-11 PNGs before being logged.

- Startup placeholder uses the real order (People under Meetings,
  Meeting settings closing the settings cluster) and metrics.
- People and the category rows share one child geometry (18 pt slot,
  16 pt glyph, 8 pt gap; categories 24 pt tall).
- Storage: other used space in the track grey (≥ 3:1 from Guidy's fill
  and from free space).
- Slider backing chosen for the stock 20 % fill tint, so the visible
  remainder is ≥ 3:1 from the fill and the panel in both themes.
- Local model descriptions joined with " · " like Models; the RAM badge
  hides when the description already gives the advice.
- A 1 pt hairline at the sidebar's bottom edge when rows continue below
  (no dimming).
- Checks: full workspace tests pass, clippy and fmt clean. 242 PNGs.

Round 11 review: Opus 9.0 (403/45, ship 9; all six round-11 claims
verified; one regression: the scroll-cue hairline doubles the status
bar's divider under the sidebar), Codex 9.0 (403/45, ship 9; no blockers
or material regressions). **Both at the target: the loop stops here.**

Remaining polish (not required for the target): the sidebar scroll cue
(Opus: too strong, doubles a line; Codex: too subtle), light button hover
reads close to pressed (stock outline-button formula), four light accent
tones, the cloud vs local AI status ordering, Post Process wrapping two
rows at 720 pt, compact cloud onboarding's last optional row, dim
Apple Intelligence recovery text.
