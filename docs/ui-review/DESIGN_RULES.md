# Design rules for the UI refresh (implementers)

The tokens are in `crates/guidy-app/src/ui/design.rs`, `ui/kit.rs` and
`assets/themes/guidy.json` (already updated). Use them; never hard-code a
size, color or radius a token covers. The plan and its reasons:
`docs/ui-review/PLAN.md`. The council's findings: `docs/ui-review/round-*/`.

## Hard constraints

- UI only. Do not change behaviour, settings, data, commands, flows or
  state machines. A command may move into an overflow ("More…") menu or get
  a label; it must stay reachable and do exactly what it did.
- Keep every element `.id(...)` and `.test_support()` that tests use. If a
  control moves into a menu, keep a test-visible path or update the test
  only where it asserts layout, never behaviour.
- New user-visible strings: add them to `assets/locales/en/translation.json`
  only when unavoidable (other locales fall back to English). Prefer
  existing keys.
- Keep `cargo check -p guidy-app` green after each edit. Other agents edit
  other files in the same checkout at the same time.

## Type

| Role | Token | Notes |
|---|---|---|
| Toolbar page title | `TEXT_TITLE` 15 semibold | set by the shell |
| Object title (meeting, entry, empty state, alert) | `TEXT_HEADLINE` 17 semibold | |
| Group title above a panel | `design::group_header` (13 semibold, primary) | `kit::group(Some(..))` does it |
| Body, row titles, controls | `TEXT_BODY` 13 / `LINE_BODY` 18 | list titles may be MEDIUM |
| Reading (summaries, transcripts, history text) | `TEXT_READING` 14 / `LINE_READING` 21 | |
| Helper, descriptions, metadata | `TEXT_SECONDARY` 12 / `LINE_SECONDARY` 16, muted | |
| Captions, badges, counts, column heads | `TEXT_CAPTION` 11 / `LINE_CAPTION` 14 | `design::section_header` = 12 medium muted label |
| Numbers, sizes, durations, prices | `design::mono()` (UI font + tabular figures) | not monospace any more |
| IDs, paths, code, raw errors | `design::code()` / `theme::MONO_FONT` | the only monospace |

Weights: 400 / 500 / 600 only (700 only for the onboarding display title).
The UI font is the system font; `theme::BRAND_FONT` (Geist) only for the
onboarding display title.

## Spacing, size, shape

- 4-pt grid: 2, 4, 8, 12, 16, 20, 24, 32. Page padding `PAGE_X` 24 /
  `PAGE_TOP` 20; groups `GROUP_GAP` 24 apart; header → panel `HEADER_GAP` 8;
  rows `SECTION_X` 14 × `ROW_Y` 10, min height 36.
- Forms: `kit::page()` (680 max, centered). Lists and catalogues: cap at
  `LIST_MAX_W` 960 and center (`.mx_auto()`), don't stretch across 1440 pt.
- Icon-only buttons: 24 pt slot (`ICON_BUTTON`), 16 pt glyph, ≥ 4 pt apart.
  At most 4 icon-only buttons in a toolbar; the rest get labels or go in a
  "More" (`ellipsis`) menu.
- Radii: chip 4, control 5, panel 8, dialog 10 (`RADIUS_*`); capsule only
  for HUD, switch, dots, avatars.
- No card inside a card. Content that is the page's subject (a summary, a
  transcript) sits directly on its pane, not in another bordered box.

## Color

- Accent (yellow) has four jobs only: the one primary button per region,
  the focus ring, on-state switches/sliders/progress (theme handles it),
  and links. Never for values, totals, timestamps, speaker names (including
  "You"), badges, counts or status words. Use `tone.text` / `text_muted`.
- Status: `design::Status` + `design::status_color` — Ok (green), Attention
  (orange), Live/Error (red), Neutral. Always icon or dot + word; never
  color alone. "Granted"/"Ready" = Ok; "Processing" = neutral spinner +
  secondary text; "Recording" = Live red dot + word.
- Badges: `design::chip(text, false, cx)` is neutral; `accent = true` only
  for a single "Recommended"-type highlight per screen.
- Selection: neutral fill (`tone.selection` / `selection_focused`) in every
  list, same in both themes, title MEDIUM when selected.
- `text_faint` only on `surface`/`ink`/`frame`, never on `raised` or a
  selected row (use `text_muted` there).

## Buttons

- Primary: filled accent, one per region.
- Secondary: `.outline()` bordered, for every text command in a toolbar or
  row ("Test Connection", "Manage", "Clear Usage…", "Edit…", "Show in
  Finder").
- Ghost: icon-only buttons, and in-text links only.
- Icon-only actions: 24×24 pt minimum hit area (`ICON_BUTTON`), 14–16 pt
  glyph, ≥ 4 pt apart; 28 pt in meeting headers.

## Busy and motion

- No perpetual decorative loops: `design::shimmer_text` is now static
  muted text; a spinner or a real progress value carries the activity.
- Live dots are steady (no opacity pulse). Any remaining animation checks
  `cx.reduce_motion()`.

## Forms

- `kit::SettingRow` now wraps its control under the label when the label
  would get < 220 pt. Don't clamp or ellipsize descriptions; let them wrap.
- `kit::Dropdown` hugs its value (min 120, max 260 unless `min_width`).

## Copy

Sentence case for labels, headers, headings, empty states; Title Case for
buttons and menu items; "…" on commands that open a dialog.

## Verify

Render your screens with the snapshot harness (`docs/UI_SNAPSHOTS.md`):

```sh
GUIDY_SNAPSHOT_DIR=target/ui-review/wip-<you> GUIDY_SNAPSHOT_FILTER=<names> \
  cargo test -p guidy-app --lib ui_snapshots -- --ignored --nocapture
```

and look at the PNGs (light and dark, default, `-min`, `-wide`). Run the
UI tests for your area (`cargo test -p guidy-app --lib <module_or_test_name>`)
and finally `cargo test -p guidy-app --lib`.

## Round 3 additions (binding)

- **Control height: 26 pt** for every row and toolbar control: push
  buttons, text inputs, pop-ups, search fields, shortcut fields. That is the
  stock default size: drop `.small()` on these. Keep `.small()`/`.xsmall()`
  only for icon-only buttons inside a 24–28 pt slot and for badges.
- **Trailing column:** a settings row's pop-up or shortcut field goes in
  `kit::trailing(reset, control)`: the control right-aligned in the 200 pt
  `design::CONTROL_COLUMN_W`, the reset (if any) in a fixed 24 pt slot just
  before it, so all resets on a page share one x. Toggles stay flush right.
- **Tracks:** the unfilled part of seek bars, progress and meters uses
  `tone.track` (≥ 3:1). Seek hit regions are ≥ 24 pt tall around a 4 pt
  visible track.
- **Glyphs:** play/pause/stop are always the filled `brand::PLAY_ICON`,
  `brand::PAUSE_ICON`, `brand::STOP_ICON` (`kit::icon(name)`).
- **Selection:** every list (sidebar rows and child rows, meeting list,
  People, categories, catalogue, History) uses `tone.selection` /
  `tone.selection_focused` — no other fill for "selected".
- **Checks and radios:** accent ink (`tone.accent_text`: `#8a6000` light,
  `#ffc72c` dark) for the mark/fill everywhere.
- **Back:** one style: a ghost button with `chevron-left` + label, at the
  leading edge of the pane header.
- **Group headers** are now 15 semibold (`design::group_header`).
- **Focus:** stock controls draw one opaque ring-coloured edge (no halo).
  Custom focusable elements: a 2 pt `cx.theme().ring` border that is
  transparent at rest (reserve it so layout doesn't move).
- **Motion:** `kit::spinner` is static under Reduce Motion. Where a stage
  name and a percentage or progress bar already show the work, use a static
  glyph instead of a spinner.
- **Disabled buttons:** the button's own style at reduced opacity (stock
  `.disabled(true)`); no separate grey fill.

## Round 4 additions (binding)

- **Pop-ups:** `kit::Dropdown` is exactly 200 pt (`CONTROL_COLUMN_W`) by
  default; long values truncate in the closed pop-up. `.min_width(w)` now
  sets a fixed width (toolbars, compact rows). Don't let a pop-up grow past
  its column.
- **Type:** toolbar title 17 semibold (`TEXT_TITLE`); group headers and
  note section titles 15 semibold (`TEXT_GROUP`); rows 13.
- **Tracks:** `design::track` remainder is opaque `tone.track`; slider and
  progress fills are accent ink (`#8a6000` light / `#ffc72c` dark).
- **Panels:** `SECTION_X` is 16. Custom gaps use 4/8/12/16 only.
- **Mono:** every model ID and every file path uses `design::code()`;
  nothing else does.
- **Back:** every list → detail stack uses the toolbar-leading back
  (chevron + title) like Meetings; no in-content back bars.
