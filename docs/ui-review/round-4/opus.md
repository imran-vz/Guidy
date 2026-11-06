# Guidy UI review, round 4: reviewer `opus`

## Evidence

I opened 56 round-4 snapshots with the image tool, plus 2 round-3 snapshots for comparison and 5 zoomed crops. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (default, min, full, `focus-1`, `focus-2`), Advanced (min, full, limit confirm), Post Process (default, min, discard), Usage, `usage-error`, Storage, `storage-error`, Debug (default, min, full), `debug-logs` and About.
- **History:** default and `history-selected-min`.
- **Models:** default, min and wide.
- **Meetings:** list, detail, playing, processing, recording, find, empty, detail-min and detail-wide.
- **People:** `meetings-people`, `meetings-people-person-min`, `meetings-people-wide` and `mpeople-person`.
- **Meeting settings:** shell, setup, AI (default and min), recording, speakers and you.
- **Onboarding:** welcome, permissions, picker, setup-cloud, setup-meetings and migration.
- **Startup and HUDs:** `startup-app`, `startup-error`, the overlay and both widget states.

I also diffed all 16 `focus-N` PNGs against `main-general-*` pixel by pixel to find exactly what each focus state draws.

Pixels were measured with a pure-Python PNG decoder in the scratch directory, because Pillow is not installed. Contrast uses WCAG 2.x relative luminance. Coordinates are 2x pixels.

As in earlier rounds:

- The `msettings-*` and `mpeople-person*` snapshots render the pane without the shell, so their empty left column is harness framing.
- `-full` snapshots are tall captures, so dialogs sit near the top of a 4800 px canvas.

**Scope rules applied this round (from the brief):**

- The status bar, including "Check for updates", is out of scope.
- Patching stock gpui-kit controls is out of scope. Stock controls draw focus as a 1 pt edge.
- I no longer count either against any parameter. I still score custom elements against the 2 pt ring rule.

## The round-4 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| One pop-up rule: exactly the 200 pt column | Mostly holds | General, Advanced, Debug and Meeting-settings pop-ups measure 399–400 px. Exceptions: Post Process › Provider is 160 pt (x 1398–1717, `main-postprocessing-*`), and Sound theme is 171 pt to fit its ▶ (`main-debug-*`). |
| Slider value in a 56 pt no-wrap column | Holds | "300 ms", "60 ms" and "0 ms" are on one line (`main-debug-full-*`). |
| Opaque progress remainders; accent-ink fills | Holds | Processing bar and Models download: fill `#8a6000` / `#ffc72c` on remainder `#8f8f8a` / `#707070`. Fill vs remainder is 1.72 (L) / 3.17 (D); the remainder is 3.11 / 3.59 against the pane. |
| Slider backing track on `tone.track` | Partly | The slider remainder renders `#8e866e` (L) / `#8d8162` (D), so the stock slider still tints it. It is now 3.63 / 4.13 against the row and 1.54 / 2.47 against the fill, up from 1.24 / 1.49. This is a stock control, so it is out of scope and accepted. |
| Toolbar title 17, group headers 15, rows 13 | Holds | Cap heights: title 25 px, group 22, row 18, helper 17 (`main-general-light`). |
| 16 pt panel inset | Holds | Row text starts at x 842 in Meeting settings and x 482 on General. |
| NSAlert-style icon with a caution badge | Holds | `postprocessing-discard-*`, `advanced-limit-confirm-*`: the app mark on a 48 pt tile with a red caution badge. |
| `focus-1…10` snapshots | 8 of 10 exist | `focus-7` and `focus-9` are missing. No snapshot reaches a switch, although the brief says switches are shown. |
| Stock focus is a 1 pt opaque edge (≥ 5:1 L, ≥ 10:1 D) | Holds | Pop-ups, shortcut fields and resets draw a 2 px edge in `#8a6000` (5.59:1 on white) / `#ffc72c` (10.2:1 on `#222`). |
| Sidebar focus | Barely visible | `focus-1`: the only change is the selected row's fill, from `#d2d2d0` to `#c8c8c7` (1.11:1) in light and from `#383838` to `#464646` (1.24:1) in dark. There is no ring. The sidebar is a custom element (`main_window.rs:546`), not a stock control. See Problem 1. |
| People compact detail uses the toolbar back | Holds | `meetings-people-person-min-*`: a chevron next to "People" in the toolbar, with no in-content bar. |
| All three transport buttons always present | Holds | `meetings-detail-*` and `meetings-playing-*`: the time label sits at x 1100 in both, and skip-back is disabled at 0:00. |
| Progress shown once | Holds | `meetings-processing-*`: the list row says "Processing" with no bar. |
| Meeting list 280 pt above 1280 pt | Holds | `meetings-detail-wide-*`: the list is 560 px wide at 2x. |
| Compact AI pane: status on its own line, local first, two full catalogue rows | Holds | `msettings-ai-min-*`. |
| Category list on the sidebar fill | Holds | `meetings-settings-*` |
| Neutral Recommended chip | Holds | `onboarding-picker-*` |
| Laptop glyph; preview out of the reset slot; filter toggle buttons | Holds | `main-models-*`, `main-debug-*` |
| Post Process add/remove as a 24 pt segmented pair | Holds | `postprocessing-discard-*` (crop) |
| History at minimum size opens a detail view | Holds | `history-selected-min-*`: toolbar back, and the star in the detail header. |
| Model IDs and paths in the code font | Holds | Usage table, About paths, the "Generated with" footer and the AI catalogue. |
| Copy fixes | Hold | "English" appears once. "Download Model (501 MB)". "Live logs · 0 lines". "below, it asks you to confirm." The Paste method description is shorter, and the VAD row now reads "Filters silence from recordings." |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 3.

| # | Parameter | Guidy R4 | Guidy R3 | Codex | Claude | Reason for the round-4 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 8 | 8 | 9 | One family. Mono now for every model ID and path (`main-usage-*`, `main-about-*`, `meetings-detail-wide-*`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged: weight marks role only. |
| 3 | Color contrast | 9 | 8 | 8 | 8 | Track remainders are 3.11 / 3.59, stock focus edges 5.6 / 10.2, body text unchanged (`meetings-processing-*`, `focus-2-*`). |
| 4 | Information hierarchy | 9 | 8 | 8 | 8 | Progress shown once, one primary per region, and the History min detail is pushed (`meetings-processing-*`, `history-selected-min-*`). |
| 5 | Typographic hierarchy | 9 | 8 | 8 | 8 | Four roles with 2 pt steps: 17 / 15 / 13 / 12 (`main-general-*`). |
| 6 | Design language | 9 | 8 | 8 | 9 | One back style, one track token, neutral chips. Rows now reflow two ways (see #10). |
| 7 | Padding and spacing | 8 | 8 | 8 | 8 | 16 pt inset everywhere. Narrow rows that wrap leave a 6 pt label-to-control gap beside 20 pt gaps on trailing rows (`main-general-min-*`). |
| 8 | Width and layout measure | 9 | 8 | 8 | 9 | Pop-ups are 200 pt in every setting row. Small exceptions remain: Provider at 160 pt and Sound theme at 171 pt. |
| 9 | Color and theme | 9 | 8 | 8 | 8 | Neutral chip and neutral tracks. The accent is limited to on-states, primary fills, focus and links. |
| 10 | Overall consistency | 8 | 8 | 8 (k) | 9 (k) | The round-3 problems are fixed. New: trailing and wrapped rows mix within one group (`main-debug-min-*`). The Post Process refresh sits after its control while resets sit before theirs. |
| 11 | Alignment and grid | 8 | 8 | 8 | 9 | The player is stable and resets touch their controls. At 720 pt the wrapped controls leave the trailing column (`main-general-min-*`). The People "You" row is indented 20 pt more than the rows below it. |
| 12 | Vertical rhythm | 8 | 8 | 8 | 9 | Values never wrap. Row heights within one group differ by layout at min (`main-debug-min-*`: 53, 85 and 85 pt). |
| 13 | Perceptual grouping | 9 | 8 | 8 | 8 | Every reset sits about 5 pt from its control (`main-general-*`). |
| 14 | Iconography | 9 | 8 | 8 | 8 | Laptop glyph; the reset slot means only reset; filled transport glyphs. |
| 15 | Control styling and sizing | 9 | 8 | 8 | 8 | One pop-up width, 26 pt controls, a segmented "+ −", and toggle filters. |
| 16 | Affordance and signifiers | 9 | 8 | 7 | 7 | Bordered Streaming/Translate toggles and a real "+ −" control (`main-models-*`, `postprocessing-discard-*`). |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Focus is now captured. Hover and pressed are still not shown for window controls. |
| 18 | Selection clarity | 8 | 8 | 8 | 8 | One token everywhere. Focused vs unfocused selection is only 1.11 / 1.24 apart (`focus-1-*`). |
| 19 | Focus visibility | 8 | 7 | 7 (k) | 7 (k) | Stock edges are accepted per scope, and tabs have a 2 pt ring. The sidebar, a custom element, has no ring and a near-invisible fill change. Switches are not captured. |
| 20 | Target size | 9 | 8 | 8 | 8 | 24 pt "+ −", 24 pt resets, 28 pt transcript tools. |
| 21 | Navigation and wayfinding | 8 | 8 | 8 | 8 | One back style everywhere. Meeting settings still shows three navigation columns, two of them sidebar-toned (`meetings-settings-*`). |
| 22 | Information architecture | 8 | 8 | 7 (k) | 8 (k) | The in-scope presentation is clear. Clusters are still unlabeled. |
| 23 | Toolbar and window chrome | 9 | 7 | 8 | 8 | The status bar is now out of scope. A 17 pt title, ≤ 4 labeled items, and the back in the toolbar. |
| 24 | Surface elevation and layering | 9 | 8 | 8 | 8 | The History min card is gone; three tones in both themes. |
| 25 | Borders and dividers | 9 | 8 | 8 | 9 | The find bar is on the pane tone (`meetings-find-light`); single hairlines. |
| 26 | Corner radius consistency | 9 | 8 | 8 | 8 | No exceptions found in 56 screens. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-*`, `debug-logs-*`). |
| 28 | Loading and progress | 9 | 8 | 8 (k) | 8 (k) | No player shift, one bar per meeting, visible remainders. |
| 29 | Error and status messaging | 8 | 8 | 7 (k) | 7 (k) | `storage-error`: "The data folder can't be located." with no path and no next step beyond Try Again. |
| 30 | Data density | 9 | 8 | 8 | 8 | Option semantics are out of the prose and the jargon is gone (`main-advanced-full-*`). |
| 31 | Scannability | 9 | 8 | 8 | 8 | One-line descriptions in Advanced and Recording; stable Models slots. |
| 32 | Microcopy and capitalization | 9 | 8 | 8 | 9 | All of the round-3 list is fixed. Only nits remain (Problem 9). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 7 | Unchanged. |
| 34 | Minimum-size reflow (720×480) | 8 | 9 | 7 (k) | 8 (k) | Nothing truncates, but rows reflow inconsistently within a group (Regression 1). |
| 35 | Wide-size use of space | 8 | 8 | 8 | 8 | The meeting list grows to 280 pt. Models and the People detail still centre, with a 100–140 pt dead left gutter (`main-models-wide-*`, `meetings-people-wide-*`). |
| 36 | Dark-mode quality | 9 | 8 | 8 | 8 | Neutral tracks at 3.59 and a lifted selection. |
| 37 | Light-mode quality | 9 | 8 | 7 (k) | 8 (k) | Remainders at 3.11 and crisp 1 pt control edges. |
| 38 | Native platform fit | 8 | 8 | 7 | 7 | NSAlert icon and toolbar back. The sidebar focus is not like a source list's, and stacked controls in Settings rows are not System Settings behaviour. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 8 | 8 | 9 | Neutral chip; one accent element per region. |
| 41 | Non-text indicators | 9 | 8 | 7 | 7 | Every remainder is ≥ 3:1. Progress fill vs remainder is 1.72 / 3.17 and the hue split is clear. |
| 42 | Truncation and text accommodation | 9 | 8 | 8 | 6 | Values and the AI status no longer wrap. One title truncates in the meeting list. |
| 43 | Dialogs and modality | 8 | 8 | 7 (k) | 8 (k) | App icon with a badge. Cancel is drawn as an ordinary outline button while Discard is a filled red button, so Return's target is not shown. There is still no Esc snapshot. |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`recording-widget-*`, `overlay-live-*`). |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Unchanged. |

| Column | Mean of 45 |
|---|---|
| Guidy round 4 | **8.7** |
| Guidy round 3 | **8.1** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 4): 8.**

Almost every round-3 blocker is fixed:

- one back style;
- one pop-up width;
- visible tracks;
- a real type ladder;
- pushed details at minimum size;
- app-icon alerts.

What still stops a sign-off at 9:

- The sidebar, the app's main custom focus target, shows keyboard focus as a 1.1:1 fill change.
- A new narrow-width reflow mixes two row layouts inside one group.
- Meeting settings still shows three navigation columns.

---

## 2. Regressions since round 3

1. **Narrow rows now wrap their control under the label, so one group mixes two layouts.**
   - **Evidence:**
     - `main-general-min-*`: Dictation key keeps its pop-up trailing. Transcribe shortcut and Cancel shortcut drop the field under the description, left-aligned, with the reset to its left. In round 3 (`round-3/main-general-min-*`) all three rows were trailing.
     - `main-debug-min-*`: Log level and Sound theme are trailing. Word correction and Paste delay sliders wrap to a second line at the leading edge, with the value floating at x 882.
     - `main-postprocessing-min-*`: the API key and Model rows wrap.
     - At the default 900 pt, every pop-up in Meeting settings now sits under its description (`msettings-recording-*`, `meetings-settings-*`), while switches stay trailing. In round 3 they were trailing.
   - **Cause:** `kit.rs:316–326`. The non-stacked row is an `h_flex().flex_wrap()` with `label.min_w(220)` and `justify_between`. Once the label would get less than 220 pt, the control wraps to its own line and starts at the leading edge.
   - **Holds down:** #10, #11, #12, #34, #38, #7.
   - **Fix:** see Problem 2.
2. Nothing else got worse.

---

## 3. Remaining problems, ranked

**1. Sidebar keyboard focus is nearly invisible (blocks a 9 on #19 and #18).**
- **Evidence:**
  - `focus-1-light`: the focused, selected General row changes from `#d2d2d0` to `#c8c8c7`, a 1.11:1 difference.
  - `focus-1-dark`: it changes from `#383838` to `#464646`, a 1.24:1 difference.
  - There is no ring. Against the sidebar the focused fill is 1.48 / 1.73.
  - The sidebar is custom (`main_window.rs:546–556`, `tone.selection_focused`), so the brief's rule for custom elements, a 2 pt ring, applies. Every other custom focus target (content tabs) has the ring.
- **Holds down:** #19, #18, #38, #17.
- **Fix:** either of these, both in-scope presentation:
  - Draw the 2 pt `ring` (`#8a6000` / `#ffc72c`) inset 1 pt on the selected row while `sidebar_focus` is focused, as `meetings-tab-focus` already does.
  - Or follow macOS source lists: make `selection_focused` an accent-tinted fill, for example `#ffc72c` at about 35 % on light and the existing `0xffc72c33` variant on dark, so focused vs unfocused differs by ≥ 3:1 in hue and tone.
- **Also:** add `focus-7` and `focus-9` (the Language and Microphone resets) and a switch focus to the harness.
- **Files:** `ui/design.rs:197/220` (`selection_focused`), `ui/main_window.rs:584`.

**2. One row layout at every width (blocks a 9 on #11 and #34).**
- **Evidence:** Regression 1.
- **Holds down:** #10, #11, #12, #34, #38, #7.
- **Fix:**
  - When a row wraps, keep the control in the trailing column: `justify_end` on the wrapped line, so the reset and control stay at x 1285 and x 1318 relative to the right edge.
  - Better, decide at group level: if any row in a group must wrap, wrap all of them, or none.
  - In Meeting settings at 900 pt the pane has room for the trailing column if the label minimum drops from 220 to 180 pt (the rubric's name floor). Use that before wrapping.
- **File:** `kit.rs` (`SettingRow::render`, the `flex_wrap` branch).

**3. Meeting settings shows three navigation columns.**
- **Evidence:** `meetings-settings-*`, `msettings-*`. The category list now has the sidebar fill, so the window reads as two adjacent sidebars plus a pane.
- **Holds down:** #21, #38.
- **Fix (presentation only):** show the nine categories as indented children of "Meeting settings" in the main sidebar while it is selected, as System Settings does, and drop the middle column. The destinations do not change. Below 760 pt keep the compact pop-up from `msettings-ai-min`.
- **Files:** `ui/main_window.rs`, `pages/meetings/settings.rs`.

**4. Wide pages centre instead of anchoring.**
- **Evidence:**
  - `main-models-wide-*`: the list starts about 100 pt right of the pane edge.
  - `meetings-people-wide-*`: the detail column starts about 140 pt right of the pane edge.
  - Meetings detail, History and all settings pages are left-anchored.
- **Holds down:** #35, #11.
- **Fix:** left-anchor the capped column at the 24 pt page inset, as the settings forms are.
- **Files:** `pages/models.rs`, `pages/meetings/people.rs`.

**5. The alert's default button is not drawn.**
- **Evidence:** `postprocessing-discard-*`, `advanced-limit-confirm-*`. Cancel has an ordinary `#8f8f8a` / `#767676` outline, while Discard or Delete is a filled red button, so the filled one reads as the default.
- **Holds down:** #43, #38.
- **Fix:** whichever button Return triggers gets the default treatment, and that binding itself stays as it is. For a destructive alert on macOS that is usually Cancel: give it the accent fill. The destructive button becomes an outline button with red text (`danger` text on `tone.panel`). Add an Esc snapshot.
- **File:** `kit.rs` (confirm).

**6. Two icon-slot rules beside controls.**
- **Evidence:**
  - The reset sits before its control (x 1285, `main-general-*`).
  - The Post Process Model refresh sits after its pop-up (x 1693, `main-postprocessing-*`).
  - The Sound theme ▶ sits after its pop-up and shrinks it to 171 pt (`main-debug-*`).
- **Holds down:** #10, #11, #8.
- **Fix:**
  - Leading slot: reset only.
  - Trailing slot: a 24 pt "accessory" (refresh, preview) placed outside the 200 pt column, with the column moved left by 32 pt in rows that have one, so every pop-up stays 200 pt.
  - Make Provider 200 pt as well.
- **Files:** `kit.rs` (`trailing`), `pages/post_processing.rs`, `pages/debug.rs`.

**7. Storage error has no next step.**
- **Evidence:** `storage-error-*`: "The data folder can't be located." with only Try Again, while `startup-error` shows the path in mono and offers Show Logs.
- **Holds down:** #29.
- **Fix:** show the expected path in the code font under the sentence, and add "Show Logs" as a secondary button next to Try Again (the command already exists in About).
- **File:** `pages/storage.rs`.

**8. People page details.**
- **Evidence:**
  - `meetings-people-*`: the "You" row text starts at x 471 because of its avatar glyph, while the other rows start at x 432.
  - The list header "People" repeats the toolbar title "People".
- **Holds down:** #11, #30.
- **Fix:**
  - Either give every person row a 16 pt avatar slot, or drop the glyph on "You" and mark it with the "Not recorded" subtitle alone.
  - Rename the header to "Others" or "Known people".
- **File:** `pages/meetings/people.rs`.

**9. Microcopy nits.**
- **Evidence:**
  - Breeze ASR: "Optimized for Taiwanese Mandarin. Code-switching support · 2 languages" mixes a full stop and the dot join (`onboarding-picker-*`, `main-models-wide-*`).
  - The History min detail toolbar keeps the list's Search and Saved filters while the list is hidden (`history-selected-min-*`).
- **Holds down:** #32, #23.
- **Fix:**
  - Join description fragments with " · " only, without sentence periods.
  - In the pushed detail, hide or disable the list filters. That is presentation, not a removed command, because they come back with the list.
- **Files:** `assets/locales/en/translation.json`, `pages/history.rs`.

**10. Hover and pressed states are still unproven.**
- **Evidence:** no snapshot shows hover or pressed for a push button, a list row or a sidebar row. HUD pressed states exist in source only.
- **Holds down:** #17.
- **Fix:** add `hover-*` and `pressed-*` harness snapshots for a push button, a list row and a sidebar row in both themes.
- **File:** snapshot harness (`docs/UI_SNAPSHOTS.md`).

**11. Onboarding leading edge.**
- **Evidence:** `onboarding-setup-meetings-*`: the "Better speaker turns" text starts at x 414 beside a checkbox, while the icon rows start at x 406.
- **Holds down:** #11.
- **Fix:** put the checkbox in the same 20 pt slot as the icons.
- **File:** `onboarding/setup.rs`.

Problems 1–3 block a 9 overall. Problem 1 blocks #19, Problem 2 blocks #11 and #34, and Problem 3 blocks #21.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 7 | Padding | One wrap rule, so gaps don't differ between rows of one group (Problem 2). |
| 10 | Consistency | Problems 2 and 6. |
| 11 | Alignment | Wrapped controls stay in the trailing column; aligned People and onboarding leading edges. |
| 12 | Rhythm | Rows in one group share a layout, and so a height class. |
| 17 | States | Hover and pressed snapshots (Problem 10). |
| 18 | Selection | Focused selection distinct from unfocused (Problem 1). |
| 19 | Focus | Ring on the focused sidebar row; switch focus captured (Problem 1). |
| 21 | Navigation | Meeting-settings categories nested in the sidebar (Problem 3). |
| 22 | IA | Small muted cluster labels in the sidebar, or accept 8. |
| 29 | Errors | Path and Show Logs on the storage error (Problem 7). |
| 34 | Min reflow | Problem 2. |
| 35 | Wide | Left-anchored Models and People detail (Problem 4). |
| 38 | Native fit | Problems 1, 2 and 5. |
| 43 | Dialogs | Default button drawn; Esc snapshot (Problem 5). |

---

## 5. Functional findings (not visual)

1. "Generate Summary" is still the yellow primary while a meeting records (round 3 finding; not re-checked in `meetings-recording-wide`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Every older fixture meeting starts at 14:30 (`main-meetings-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while "Improve Transcriptions" is listed (`main-postprocessing-*`).
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-*`). Backlog #5.
7. The startup skeleton sidebar omits Post Process and Debug (`startup-app-*`), so the sidebar may jump when settings load.
8. The Deepgram key still saves on Enter ("Paste your Deepgram key and press Enter", `msettings-speakers-*`), while the other key fields have Test. This is a behaviour choice; noted, not scored.
9. Out-of-scope IA, noted once: voice enrollment has two entry points (People › You and Meeting settings › You › Open in People), and API keys have two homes (Post Process and AI summaries). The status bar's persistent "Check for updates" and the model readiness line are out of scope by the round-4 clarification.
