# UI refresh plan

Derived from both Phase 1 reports. Where the reviewers differed, the
decision and the reason are stated. Everything here is presentation only:
no setting, flow, data or command is added, removed or changes behaviour.
Commands may move into an overflow menu, but every command stays reachable.

## Decisions

| Topic | Opus | Codex | Decision |
|---|---|---|---|
| UI typeface | SF Pro (system) | keep Geist | **System font (SF Pro).** Every reference (Codex, Claude, T3 Code, Xcode, System Settings) uses it, menus and alerts already do, and it is a one-token change the owner can revert. Geist stays for the onboarding display title; Geist Mono only for IDs, paths and code. |
| Numbers | UI font, tabular | tabular | UI font with tabular figures (`tnum`); mono only for clocks in the player/transcript. |
| Page title | 20 in content or 15 in toolbar | 17 | 15 semibold in the toolbar (System Settings, Xcode). Object titles (meeting, history entry) 17 semibold. |
| Group header | 13 semibold, primary | 12 semibold | 13 semibold, primary text (T3 Code, System Settings). Column and stat labels stay 12 medium muted. |
| Helper text | 12/16 | 12/16 | 12/16. 11/14 only for captions, badges, counts. |
| Reading text | 14/22 | 14/21 | 14/21 for summaries, transcripts, History text. |
| Radii | 5 / 4 / 8 / 10 | 3 / 4 / 6 | Chip 4, control 5, panel 8, dialog 10, capsule for HUD/switch/avatar. |
| Spacing | page 24/20, rows 10×14, group 24, header 8 | page 20, panel 12, group 24, header 8 | Page 24 × 20, row 10 × 14 (min 36), group 24, header 8. |
| Form width | 680 centered | ≤ 640 | 680 max, centered in the content area so wide windows are balanced. Lists and catalogues 960 max. |
| Dark surfaces | content 1a, sidebar 22, panel 24 | canvas 17, frame 21, panel 20 | Content `#181818`, sidebar `#202020`, panel `#222222`, raised `#2c2c2c`, hairline `#323232`. |
| Accent | 4 jobs | primary + selection marks; ink `#8a6000` in light | Primary button, focus ring, on-state switch/slider/progress, links. Never values, totals, speaker names, badges or status. Light non-text accent `#a87500` (4.0:1); dark `#ffc72c`. |
| Selection | neutral, accent icon | neutral + mark | Neutral fill in every list, the same in both themes; accent icon in the sidebar. |
| Status | semantic icon + text | same | One status treatment: success green, attention orange, live/error red, working = spinner + secondary text. |
| Control boundaries | ≥ 3:1 | ≥ 3:1 for necessary edges | Inputs, pop-ups, off switches ≥ 3:1. Decorative hairlines stay quiet. |
| Motion | keep, honour Reduce Motion | remove decorative loops | Remove the decorative sheen from tracks and skeletons; keep spinners and the live meter. |
| Sidebar | grouped sections | grouped, quiet | Group headings (Library / Settings / App); same destinations, same shortcuts. |
| Meeting header | 2 labeled + overflow | labeled Copy + primary + More | Labeled Copy and More (…) menu holding the remaining commands; player compact. |
| Onboarding | full-window, no card | keep, fix compact layout | Keep the assistant pane; quieter frame; fix the compact cloud layout. |

## Round 1 work

1. Foundation (tokens, theme, kit, shell): typeface, type scale, spacing,
   radii, both palettes, control contrast, focus ring, chips, mono, stat
   grid, setting rows, groups, centered form column, alert placement,
   sidebar groups, toolbar title, status bar, static progress tracks.
2. Copy: sentence case for labels, headers and window headings in English;
   Title Case kept for buttons and menu items.
3. Meetings: detail header (labeled Copy + More), compact idle player,
   tabs with count badges, single empty state, transcript metadata,
   speaker-edit affordance, AI settings truncation and catalogue height,
   category list collapse by available width.
4. Models: names outrank badges, catalogue max width, size/state columns.
5. Onboarding and startup: compact cloud layout, quieter assistant frame,
   "Starting Guidy…" label, startup error hierarchy.
6. History, Usage, Storage, Post Process, Advanced, Debug, About: accent
   discipline, stat values, storage legend, inline detail height, Debug
   split clipping.
7. HUDs: upright overlay text, red live dot, compact widget timer.

Later rounds are driven by the council's scores.

## Decisions made during the loop

- **Settings pop-ups are a fixed 200 pt column** (round 4). Hugging pop-ups
  (round 2) broke the shared reset column, and a reset that floats away
  from a short pop-up reads as detached (Opus, rounds 2–3). Codex keeps
  noting that short values ("3 days", "Auto") get wide triggers; it rates
  this polish, not a blocker. Aligned resets and one control column won.
- **Status bar stays** (round 4 scope note): it carries the model status
  and the "Check for updates" entry point.
- **Stock focus edge is 1 pt** (round 4): opaque ring colour, no halo,
  ≥ 5:1. Custom elements (sidebar rows, tabs, segments) draw 2 pt.
- **Alert default:** Return triggers the confirm button, which is drawn
  filled; changing the binding is behaviour.
- **Meeting settings navigation** (rounds 5 and 10): no middle column;
  Meeting settings is a top-level row closing the settings cluster, with
  its categories listed under it while it is shown (two levels at most).
