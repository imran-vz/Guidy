# Guidy UI review, round 3: reviewer `opus`

I viewed 62 round-3 snapshots with the image tool, plus 8 zoomed crops. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General, Advanced, Post Process, Usage, Storage, Debug and About.
- **History:** default, `history-selected-min` and `history-undo`.
- **Models:** default, min and wide.
- **Meetings:** list, detail, played, playing, tab-focus, find, find-wide, transcript, recording, recording-wide, processing, empty, detail-min and detail-wide.
- **People:** `meetings-people`, `mpeople-person` and `mpeople-person-min`.
- **Meeting settings:** shell, setup, AI (default and min), speakers, linear, you and recording.
- **Onboarding:** welcome, permissions, picker, setup-cloud (default and min), setup-meetings and migration.
- **Startup, errors, dialogs and HUDs:** `startup-app`, `startup-error`, `storage-error`, both dialogs, the overlay and both widget states.

I compared these with round 2 and round 0. Pixels were measured with a pure-Python PNG decoder (Pillow is missing), and contrast uses WCAG 2.x relative luminance. Coordinates are 2x pixels.

As in round 2, the `msettings-*` and `mpeople-person*` snapshots render the pane without the shell, so the empty grey left column is a harness framing and not a defect.

## The round-3 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| One opaque focus edge, stock halo off | Holds, but there are two drawings | `meetings-tab-focus-*`: a 2 pt ring (`#8a6000` L / `#ffc72c` D) with about 4 pt clearance. `meetings-find-*`: the focused field is a 1 pt accent border with no clearance and no halo. |
| `tone.track` for seek/progress/meter remainders ≥ 3:1 | Seek only | Seek track: `#8f8f8a` on `#fafaf9` = 3.11 (L), `#707070` on `#181818` = 3.59 (D). Transcription progress remainder: `#cacac7` 1.57 (L) / `#404040` 1.71 (D) (`meetings-processing-*`). List-row progress: 1.42 / 1.54. Models download bar `#cdcdca` on white 1.59 (L), 1.66 (D). Slider remainders are still olive (`#988e73` / `#80755a`). |
| 26 pt standard control height | Holds | Pop-ups, text fields, search, push buttons and toolbar buttons all measure 50–51 px (25–25.5 pt), e.g. `main-general-*`, `msettings-ai-*`, `msettings-setup-*`, `main-meetings-*`. |
| `kit::trailing` 200 pt column with a fixed reset slot | Holds, with side effects | Every reset is at x 1289 on General, Post Process and Debug. Side effects are in Regression 1 and Problem 4. |
| Reduce-Motion-aware spinner | Holds (source) | `kit.rs:96` returns the static icon. |
| Group headers 15 semibold | Holds | Cap height 22 px, up from 19 in round 2. It now equals the toolbar title (22 px). |
| Filled stop glyph | Holds | `recording-widget-expanded-*`, `meetings-recording-*`, `debug-logs-*` (filled Pause). |
| One selection token for sidebar, child rows and lists | Holds | Contrast against each background is 1.34 (L) and 1.36–1.39 (D) for sidebar, child rows, meeting list, People list, AI catalog and category list. |
| Record/Stop/Cancel as bordered push buttons; recording toolbar without the title | Holds | `meetings-recording-*`, `meetings-processing-*` |
| Text-only empty state; Generate Summary inside its block | Holds | `meetings-empty-*`, `meetings-recording-wide-*` |
| Sidebar 172 pt below 760 pt | Holds | `*-min-*`: the sidebar edge is at x 344. |
| Speaker names semibold | Holds | `meetings-transcript-*` |
| Accent-ink checkboxes and radios everywhere | Holds | `#8a6000` (L) in onboarding, AI settings and setup-meetings. |
| Filled play glyphs; one back style | Play holds; back does not | Voiceprint and Sound-theme ▶ are filled. People still uses an in-content "‹ Back" bar (`mpeople-person-min-*`, `people.rs:1415`), while Meetings uses a toolbar chevron (`meetings-detail-min-*`). |
| Outline buttons and stock disabled state | Holds | Disabled "Test", "Test Connection", "Copy All", "Clear" and "Add" are all bordered and greyed. |
| AI status in words, "default" once, ~51 pt catalog rows | Holds | `msettings-ai-*`. The status now wraps to two lines (Problem 7). |
| "Meeting notes" category under a "Meeting settings" header | Holds | `meetings-settings-*` |
| Startup skeleton draws the real sidebar | Holds | `startup-app-*` |
| Models: fixed slots, Delete in More | Holds | `main-models-*`: sizes right-aligned at x 1128, status right edge at x 1465, then the action slot and "…". |
| History: date + Copy + More; labeled "Saved" and "Show in Finder" | Holds | `main-history-*`, `history-undo-*` |
| Storage free space outlined | Holds | `main-storage-*`: outlined free segment and hollow swatch; folder icons and the "Shared" badge are gone. |
| One description-join helper | Holds | The ". ·" join is gone in Models and the picker. |

## Measured facts

| What | Round 2 | Round 3 |
|---|---|---|
| Seek track vs pane | 1.13 (L) / 1.31 (D) | 3.11 / 3.59 |
| Progress remainder (processing, Models download) | `#ececea` 1.13 / `#2e2e2e` 1.31 | 1.57–1.59 / 1.66–1.71 |
| Slider fill vs its own remainder | — | `#a87500` vs `#988e73` 1.24 (L); `#c08a00` vs `#80755a` 1.49 (D) |
| Focus | 1 pt + 3 pt halo (2.1:1) | Tabs: 2 pt solid, 5.4:1 (L), 11.4:1 (D), with clearance. Fields: 1 pt accent border, no clearance. |
| Selection vs background | 1.34–1.48, four fills | 1.34 (L) / 1.36–1.39 (D), one token |
| Control heights | 19–24 pt mixed | 25–25.5 pt everywhere |
| Type (cap height, 2x px) | toolbar 22, group 19, row 19 | toolbar 22, group 22, pane heading 24, row 19, helper 17 |
| Transcript tool buttons | ~14 pt glyph, no padding | 28 pt (56 px) |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 2.

| # | Parameter | Guidy R3 | Guidy R2 | Codex | Claude | Reason for the round-3 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 8 | 8 | 8 | 9 | One family, 14 pt reading, semibold speakers (`meetings-transcript-*`). Model IDs are mono in AI summaries but not in Usage; paths are mono in `startup-error` but not in `main-about`. |
| 2 | Font weight usage | 9 | 8 | 8 | 8 | Weight marks role only: semibold names, groups and titles; 700 only on onboarding titles. |
| 3 | Color contrast | 8 | 8 | 8 | 8 | Seek 3.1/3.6, storage free outlined, solid ring. Progress remainders 1.57/1.71 and slider fill-vs-remainder 1.24 (`meetings-processing-*`, `msettings-speakers-*`). |
| 4 | Information hierarchy | 8 | 8 | 8 | 8 | No repeated History title, one Record, Generate Summary in its block. The status bar repeats the model on every page; processing progress is shown twice (`meetings-processing-*`). |
| 5 | Typographic hierarchy | 8 | 7 | 8 | 8 | Groups 15 sb, rows 13, helpers 12, pane headings 17. The toolbar title and group headers are both 15 semibold (`main-general-*`). |
| 6 | Design language | 8 | 8 | 8 | 9 | One checkbox ink, three button styles. Two back styles; olive slider tracks vs neutral seek track. |
| 7 | Padding and spacing | 8 | 8 | 8 | 8 | Catalog rows now 51 pt. Wide empty gaps open between descriptions and the fixed column (`msettings-recording-*`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Provider 160 pt, "3 days" 120 pt. "Auto" still fills 200 pt; "Floating, at the screen edge" and "Don't modify clipboard" overflow the column. |
| 9 | Color and theme | 8 | 8 | 8 | 8 | Neutral base, accent checkboxes. Olive slider remainders; accent "Recommended" chip (`onboarding-picker-*`). |
| 10 | Overall consistency | 8 | 7 | 8 (k) | 9 (k) | Selection, glyph fills, disabled and heights are unified. Back button, track tokens and segmented order (`onboarding-setup-*` vs `msettings-ai-*`) differ. |
| 11 | Alignment and grid | 8 | 7 | 8 | 9 | Reset column at x 1289 everywhere; Models slots fixed. The player's time label jumps from x 972 to x 1100 once playing (`meetings-detail-*` vs `meetings-playing-*`). |
| 12 | Vertical rhythm | 8 | 8 | 8 | 9 | 41/53 pt rows, 51 pt catalog. Three-line descriptions where the column squeezes the label (`msettings-recording-*` "Reminder interval"). |
| 13 | Perceptual grouping | 8 | 8 | 8 | 8 | Clear groups. Short pop-ups sit 96 pt away from their reset (`main-general-*` Microphone). |
| 14 | Iconography | 8 | 8 | 8 | 8 | Filled play, pause and stop everywhere. "Meetings on this Mac" still uses the Models CPU glyph; the Sound-theme ▶ sits in the reset slot (`main-debug-*`). |
| 15 | Control styling and sizing | 8 | 7 | 8 | 8 | 26 pt controls, three button styles. Three pop-up width rules: hug, 120 pt minimum, 200 pt fill. |
| 16 | Affordance and signifiers | 8 | 8 | 7 | 7 | History toolbar labeled. "Streaming" and "Translate" in the Models header read as labels, not filters (`main-models-*`); the prompt list "+ −" are bare glyphs. |
| 17 | Interactive states | 8 | 7 | 8 | 8 | One disabled look, played/playing/busy states and a focused tab. Hover and pressed are still not captured. |
| 18 | Selection clarity | 8 | 7 | 8 | 8 | One token, equal contrast in every list and both themes. Focused vs unfocused list selection is not shown. |
| 19 | Focus visibility | 7 | 6 | 7 (k) | 7 (k) | A good 2 pt ring on tabs, but fields use a 1 pt border, and buttons, pop-ups, switches and rows are uncaptured. |
| 20 | Target size | 8 | 7 | 8 | 8 | 26 pt controls, 28 pt transcript tools and Edit. The Post Process prompt "+ −" are about 12 pt (`postprocessing-discard-*`). |
| 21 | Navigation and wayfinding | 8 | 7 | 8 | 8 | No name clash; a labeled sub-list. Meeting settings still shows three levels at once, and there are two back styles. |
| 22 | Information architecture | 8 | 7 | 7 (k) | 8 (k) | The in-scope presentation is clear. Clusters are separated by space but unlabeled. |
| 23 | Toolbar and window chrome | 7 | 7 | 8 | 8 | The recording toolbar is fixed. The status bar still shows "Parakeet V3" and "Check for updates" on every page. |
| 24 | Surface elevation and layering | 8 | 8 | 8 | 8 | Three tones in both themes. History at min still expands into an inline card (`history-selected-min-*`). |
| 25 | Borders and dividers | 8 | 8 | 8 | 9 | Single hairlines. The light find bar adds a white strip as a fourth tone (`meetings-find-light`). |
| 26 | Corner radius consistency | 8 | 8 | 8 | 8 | Family unchanged and applied by rule. |
| 27 | Empty states | 9 | 8 | 8 (k) | 8 (k) | One headline, one sentence, one action (`meetings-empty-*`, `storage-error-*`, `debug-logs-*`). |
| 28 | Loading and progress | 8 | 8 | 8 (k) | 8 (k) | Named progress and a real skeleton. Faint progress remainders; the player layout shifts when playback starts. |
| 29 | Error and status messaging | 8 | 8 | 7 (k) | 7 (k) | Causes first, words with icons ("Anthropic key needed"). `storage-error` gives a cause but no next step beyond Try Again. |
| 30 | Data density | 8 | 7 | 8 | 8 | No duplicate History text, shorter descriptions. Paste method still explains options in prose; "VAD tail" jargon (`main-advanced-full-*`). |
| 31 | Scannability | 8 | 8 | 8 | 8 | Stable Models slots, semibold speakers. Two- to three-line descriptions remain in Advanced and Recording. |
| 32 | Microcopy and capitalization | 8 | 7 | 8 | 9 | Most round-2 items are fixed. Remaining: "English (English)", "Download Models (501 MB)" for one model, "Live logs ● Live", “Confirm?”. |
| 33 | Numeric and tabular formatting | 9 | 8 | 8 | 7 | "60 ms", tabular columns, right-aligned sizes and costs, stated price basis. |
| 34 | Minimum-size reflow | 9 | 8 | 7 (k) | 8 (k) | 172 pt sidebar, stacked panes and back at min, 2×2 stats, downloads in view. |
| 35 | Wide-size use of space | 8 | 8 | 8 | 8 | Transcript pane at 1440. Models centres with a dead left gutter; the meeting list stays at 240 pt (`main-models-wide-*`, `meetings-detail-wide-*`). |
| 36 | Dark-mode quality | 8 | 8 | 8 | 8 | Lifted surfaces and a neutral selection. Olive slider tracks; progress remainder 1.71. |
| 37 | Light-mode quality | 8 | 8 | 7 (k) | 8 (k) | Crisp controls; seek and free segment fixed. Progress remainders 1.57–1.59. |
| 38 | Native platform fit | 8 | 7 | 7 | 7 | 26 pt controls, NSAlert layout, source list. Status bar, field focus as a border, red glyph instead of an app icon in alerts. |
| 39 | Content-first focus | 9 | 8 | 9 | 9 | Reading starts at the top; narrower sidebar at min. |
| 40 | Visual noise and restraint | 8 | 8 | 8 | 9 | Storage icons gone, one "default". Accent "Recommended" chip; status bar. |
| 41 | Non-text indicators | 8 | 7 | 7 | 7 | Seek, free segment and AI status fixed. Slider fill vs remainder 1.24/1.49; progress remainders below 3:1. |
| 42 | Truncation and text accommodation | 8 | 8 | 8 | 6 | History and recording titles fixed. New: "300 ms" wraps onto two lines (`main-debug-full-*`); the AI status wraps beside the segmented control. |
| 43 | Dialogs and modality | 8 | 8 | 7 (k) | 8 (k) | Unchanged: upper third, scrim, solid red. Still a red glyph, and the default button is not drawn as default. |
| 44 | HUD and overlay design | 9 | 8 | 7 (k) | 7 (k) | Filled stop in both widget states, steady dot, tabular timer, labeled expanded controls. |
| 45 | Motion and transitions | 9 | 8 | 8 (k) | 8 (k) | Spinner, overlay and widget all gate on Reduce Motion (`kit.rs:96`, `overlay.rs:240`, `recording_widget.rs:153`). No perpetual loops. |

| Column | Mean of 45 |
|---|---|
| Guidy round 3 | **8.1** |
| Guidy round 2 | **7.6** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 3): 8.**

The one-component-per-concept work from round 2 landed: one selection token, one control height, three button styles, filled glyphs and one checkbox ink. The app now reads as one product. What stops a 9 is:

- the always-on status bar;
- focus that is drawn two ways and proven on only two controls;
- the second back style;
- the side effects of the fixed trailing column (a wrapped value, mixed pop-up widths, orphaned resets);
- slider and progress tracks still off-token.

---

## 2. Regressions since round 2

1. **The fixed 200 pt trailing column wraps or detaches content.**
   - **Evidence:**
     - `main-debug-full-light/dark`: the Hold threshold value now renders as "300" / "ms" on two lines (rows y 1120–1180). In round 2 it was one line ("300ms").
     - `main-general-*`: the Microphone reset sits at x 1289 while its hugging pop-up starts at x 1482, so the reset floats 96 pt from the control it resets. In round 2 it sat next to it.
     - `msettings-recording-*`: "Reminder interval" now wraps to three lines with a one-word last line ("end.").
   - **Fix:**
     - Value labels get a no-wrap minimum width of 56 pt, taken from the slider's width, not the label's.
     - Either make every pop-up and shortcut field in a setting row fill the 200 pt column, so the reset always touches its control, or keep the hug and move the reset into a fixed 24 pt slot at a constant 8 pt left of the control.
   - **Files:** `kit.rs` (`trailing`, `reset_button`, slider value label), `pages/debug.rs`.
2. **The AI status line wraps.**
   - **Evidence:** `msettings-ai-*` and `msettings-ai-min-*`: "Claude Sonnet 4.5 · Anthropic key needed" breaks after "key" because it shares a row with the 230 pt segmented control.
   - **Fix:** put the status on its own line under the "AI summaries" heading, with the segmented control on the heading row (trailing) or on its own row.
   - **File:** `meetings/ai_settings.rs`.

Nothing else got worse.

---

## 3. Remaining problems, ranked

**1. The status bar is persistent chrome (blocks a 9 on #23 and #38).**
- **Evidence:** every `main-*`, `meetings-*` and `*-error` shell snapshot. A green dot with "Parakeet V3" sits bottom-left and "Check for updates" bottom-right, in both themes and at every size. About also has "Check for Updates", and Models already names the active model. Only `main-models-*` ("Downloading 2 models · 9%") uses the bar for something the page does not show.
- **Holds down:** #23, #38, #40, #4, #30.
- **Fix:** show the 24 pt bar only while background work runs (downloads, migration, processing), and hide it when idle. Move "Check for updates" to About only; it already exists there.
- **File:** `ui/status_bar.rs`.

**2. Focus is drawn two ways and proven on two controls (blocks a 9 on #19).**
- **Evidence:**
  - `meetings-tab-focus-*`: a 2 pt solid ring with about 4 pt clearance. Good.
  - `meetings-find-*`: the focused field is a 1 pt accent border (`#8a6000` / `#ffc72c`) with no clearance.
  - No focused button, pop-up, switch, slider, sidebar row or list row is captured.
- **Holds down:** #19, #38, #17.
- **Fix:** use the same 2 pt ring at 2 pt outset on every focusable control, including text fields. A field may keep its 1 pt border and add the ring outside it. Add snapshots of a focused push button, pop-up, switch, slider and sidebar row in both themes.
- **Files:** `assets/themes/guidy.json` (`ring`), `kit.rs:173` (`focus_visible` only recolours the border), the snapshot harness.

**3. Two back styles (blocks a 9 on #10 and #21).**
- **Evidence:**
  - `meetings-detail-min-*`: a ghost chevron in the toolbar, beside the "Meetings" title.
  - `mpeople-person-min-*`: a separate 40 pt header bar inside the content with a labeled "‹ Back" (`people.rs:1415–1430`). In the shell this stacks under the toolbar, so there are two bars.
- **Holds down:** #10, #21, #6.
- **Fix:** People uses the same toolbar-leading chevron and title as Meetings, and drops the in-content bar.
- **Files:** `pages/meetings/people.rs`, `pages/meetings/mod.rs:1110`.

**4. Trailing-column rules for pop-ups and values (blocks a 9 on #8 and #15).**
- **Evidence:**
  - Pop-up widths follow three rules. "Right Option", "Default" and "Mechanical" hug. "Live", "Off", "Bottom", "3 days" and "10 min" sit at a 120 pt minimum. "Auto" fills 200 pt (`main-general-*`, `main-general-min-*`).
  - "Don't modify clipboard" (`main-advanced-full-*`) and "Floating, at the screen edge" (`msettings-recording-*`) grow left past the column edge.
  - Plus Regression 1.
- **Holds down:** #8, #15, #11, #13, #42.
- **Fix:** one rule. Every pop-up in a setting row is 200 pt, right-aligned, and never wider; long option labels truncate inside the closed pop-up, and the full label appears in the menu. The reset slot stays at x 1289. Sliders and value labels share the same 200 pt, with the value at 56 pt no-wrap.
- **Files:** `kit.rs` (`Dropdown`, `trailing`).

**5. Track tokens are not one token (blocks a 9 on #41).**
- **Evidence:**
  - The seek track is neutral `tone.track` (3.11 / 3.59).
  - Progress remainders: `#cacac7` / `#404040` at 1.57 / 1.71 (`meetings-processing-*`); `#b8b8b5` / `#4d4d4d` at 1.42 / 1.54 on a selected list row; Models download `#cdcdca` / `#454545` at 1.59 / 1.66.
  - Slider remainders are olive `#988e73` / `#80755a`, only 1.24 / 1.49 against the amber fill (`msettings-speakers-*`, `main-debug-*`, `main-general-full-*` Volume).
- **Holds down:** #41, #3, #9, #36, #37, #10.
- **Fix:** every unfilled remainder (seek, progress, slider, meter) uses `tone.track` (`#8f8f8a` L / `#707070` D). The filled part uses accent ink (`#8a6000` L / `#ffc72c` D) for sliders and progress. That gives fill vs remainder 1.72 (L) / 3.17 (D), up from 1.24 / 1.49, plus a clear hue split. The fill is 5.6 (L) / 10.2 (D) against the panel, and the remainder is ≥ 3:1.
- **Files:** `kit.rs` (progress, slider), `ui/download_control.rs`, `meetings/detail.rs`, `guidy.json`.

**6. Meeting settings still shows three navigation levels.**
- **Evidence:** `meetings-settings-*`: sidebar › "Meeting settings" › a 180 pt category list (Setup, Meeting notes, You, Recording…) › pane. The list header helps, but three columns of navigation are visible at 900 pt.
- **Holds down:** #21.
- **Fix (presentation only):** when Meeting settings is selected, show its nine categories as an indented, collapsible list under "Meeting settings" in the sidebar, as System Settings does with nested items, and drop the middle column. The destinations stay the same. If that is judged too large a change, at least give the category list the sidebar's own fill (`#f1f1ef` / `#202020`) so it reads as a sub-sidebar rather than content.
- **Files:** `ui/main_window.rs`, `pages/meetings/settings.rs`.

**7. AI summaries header crowding.**
- **Evidence:** Regression 2. The "Cloud provider | On this Mac" order is also the reverse of onboarding's "Apple Intelligence | On this Mac | Cloud provider" (`onboarding-setup-cloud-*`, `onboarding-setup-meetings-*`).
- **Holds down:** #42, #10, #7.
- **Fix:** status on its own line. Use one segment order everywhere (local first: On this Mac, then Cloud provider).
- **Files:** `meetings/ai_settings.rs`, `onboarding/setup.rs`.

**8. Alert details.**
- **Evidence:** `advanced-limit-confirm-*`, `postprocessing-discard-*`. The alerts use a red warning glyph, not the app icon with a caution badge. Cancel is not drawn as the default button (no accent ring or fill), so Return's target is invisible.
- **Holds down:** #43, #38.
- **Fix:** a 48 pt app icon with a 20 pt caution badge. Draw Cancel with the default style for destructive alerts. Add an Esc snapshot.
- **File:** `kit.rs` (confirm).

**9. Player layout shifts when playback starts.**
- **Evidence:**
  - `meetings-detail-*`: [▶] 0:00 at x 972.
  - `meetings-playing-*` and `meetings-played-*`: [↺15][⏸][↻15] 1:13 at x 1100. The track loses about 64 pt, and the time label moves.
- **Holds down:** #28, #11.
- **Fix:** always render all three transport buttons; the skip buttons can be disabled at 0:00.
- **File:** `meetings/player.rs`.

**10. Microcopy remainders.**
- **Evidence:**
  - "English (English)" (`main-general-full-*`, Application language).
  - "Download Models (501 MB)" for a single model (`onboarding-migration-*`).
  - "Live logs ● Live · 0 lines" (`debug-logs-*`).
  - “Confirm?”. with doubled punctuation (`msettings-speakers-*`).
  - The Paste method prose explains "Direct" and "None" in the description (`main-advanced-*`).
  - "Streaming-capable models use a longer VAD tail" (jargon).
  - "Paste a key and press Enter" (Deepgram), where every other key field has a Test button.
- **Holds down:** #32, #30.
- **Fix:**
  - Show the native name once ("English").
  - Pluralize by count.
  - "Live logs · 0 lines" with the dot only.
  - Quote option names without the trailing period.
  - Move the option explanations into `DropdownOption::description`.
  - "Filter silence from recordings."
  - "Paste your Deepgram key."
- **Files:** `assets/locales/en/translation.json`, `pages/advanced.rs`, `pages/debug.rs`.

**11. Accent noise in the picker and onboarding.**
- **Evidence:** `onboarding-picker-*`: "Recommended" is an accent chip (yellow on olive in dark) next to an accent radio and an accent primary button. That is three accent marks in one row region.
- **Holds down:** #40, #9.
- **Fix:** a neutral chip (`tone.badge`), matching "default" in AI summaries.
- **File:** `onboarding/model_picker.rs`.

**12. Icon reuse and the reset slot used for preview.**
- **Evidence:**
  - "Meetings on this Mac" uses the CPU glyph that is also the Models sidebar icon (`main-models-*`).
  - The Sound theme ▶ preview sits in the reset slot at x 1290 (`main-debug-*`), so the slot means two things.
  - "Streaming" and "Translate" in the Speech recognition header are icon+text with no control chrome. They look like a legend but act as filters.
- **Holds down:** #14, #16.
- **Fix:**
  - Use a laptop glyph for "Meetings on this Mac".
  - Put the preview button after the pop-up (trailing) or inside the menu.
  - Draw the two filters as toggle buttons (bordered when off, tinted when on) or as a "Show: Streaming · Translate" filter pop-up.
- **Files:** `pages/models.rs`, `pages/debug.rs`.

**13. Wide-size layout.**
- **Evidence:**
  - `main-models-wide-*`: the 960 pt list is centred, leaving about 140 pt of dead gutter left of it, while settings forms are left-anchored.
  - `meetings-detail-wide-*`: the meeting list stays at 240 pt at 1440.
- **Holds down:** #35.
- **Fix:** left-anchor Models like the other pages, and let the meeting list grow to 280 pt above 1280 pt.
- **Files:** `pages/models.rs`, `pages/meetings/mod.rs`.

**14. Typographic hierarchy: toolbar title equals the group header.**
- **Evidence:** `main-general-*`: both have a cap height of 22 px (15 semibold). The first group header sits 50 pt under a title of the same size and weight.
- **Holds down:** #5.
- **Fix:** keep the toolbar title at 15 semibold (chrome). Make group headers 13 semibold in `tone.text` with 24 pt above, as macOS System Settings does, or 17 semibold to match Meeting-settings pane headings. Either choice gives a 2 pt step from its neighbours.
- **File:** `design.rs`.

**15. Small targets in Post Process.**
- **Evidence:** `postprocessing-discard-*`: the prompt list's "+" and "−" are about 12 pt glyphs with no button chrome.
- **Holds down:** #20, #16.
- **Fix:** a 24 × 24 pt segmented add/remove control at the list foot, as in macOS list editors.
- **File:** `pages/post_processing.rs`.

**16. History at minimum size expands inline.**
- **Evidence:** `history-selected-min-*`: the selected entry becomes a bordered card inside the list. Its saved star disappears.
- **Holds down:** #24, #10.
- **Fix:** push a detail view with the same toolbar back chevron used in Meetings, and keep the star in the detail header.
- **File:** `pages/history.rs`.

**17. Mono usage.**
- **Evidence:**
  - Model IDs are mono in AI summaries (`claude-sonnet-4-5`) but UI font in Usage and in the "Generated with anthropic/claude-sonnet-4-5" footer (`meetings-detail-wide-*`).
  - Paths are mono in `startup-error-*` but UI font in `main-about-*`.
- **Holds down:** #1, #10.
- **Fix:** mono for every model ID and path, or for none.
- **Files:** `pages/usage.rs`, `pages/about.rs`, `meetings/summary.rs`.

**18. Duplicate progress while processing.**
- **Evidence:** `meetings-processing-*`: "Transcribing · 62%" with a bar appears in the detail header and again in the selected list row, side by side.
- **Holds down:** #30, #4.
- **Fix:** when the processing meeting is selected, show only the header bar. Keep the list-row bar for unselected rows.
- **Files:** `meetings/list.rs`.

Problems 1–5 block a 9 overall. Problem 1 also blocks #23, 2 blocks #19, 3 blocks #10 and #21, 4 blocks #8 and #15, and 5 blocks #41.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 1 | Typography | One mono rule for model IDs and paths (Problem 17). |
| 3 | Contrast | All track remainders on `tone.track` (Problem 5). |
| 4 | Information hierarchy | Status bar only while busy; one progress bar per meeting. |
| 5 | Typographic hierarchy | Group headers a step away from the toolbar title (Problem 14). |
| 6 | Design language | One back style; one track token. |
| 7 | Padding | One trailing-column rule so descriptions don't leave holes. |
| 8 | Width | Pop-ups exactly the column width, never overflowing (Problem 4). |
| 9 | Color | Neutral slider remainder; neutral "Recommended" chip. |
| 10 | Consistency | Problems 3, 5, 7, 17. |
| 11 | Alignment | Stable player transport; reset adjacent to its control. |
| 12 | Rhythm | No three-line descriptions; values never wrap. |
| 13 | Grouping | Reset within 8 pt of its control. |
| 14 | Iconography | Laptop glyph; preview out of the reset slot. |
| 15 | Control styling | One pop-up width rule. |
| 16 | Affordance | Bordered filter toggles in Models; real "+ −" buttons. |
| 17 | States | Hover and pressed snapshots for a push button and a list row. |
| 18 | Selection | Snapshot of an unfocused list selection, drawn lighter than the focused one. |
| 19 | Focus | Problem 2. |
| 20 | Target size | "+ −" at 24 pt. |
| 21 | Navigation | Problems 3 and 6. |
| 22 | IA | Small muted cluster labels in the sidebar (Library, Settings, App), or keep the spacing and accept 8. |
| 23 | Chrome | Problem 1. |
| 24 | Elevation | History at min as a pushed detail. |
| 25 | Borders | Find bar on the pane tone, not white. |
| 26 | Radius | Fine; audit chip vs control radius once more. |
| 28 | Loading | Visible progress remainders; no player shift. |
| 29 | Errors | Storage error: add the folder path and "Choose Folder…" or "Show Logs". |
| 30 | Density | Option semantics into pop-up items. |
| 31 | Scannability | One-line descriptions in Advanced and Recording. |
| 32 | Microcopy | Problem 10. |
| 35 | Wide | Left-anchored Models; a meeting list that grows. |
| 36 | Dark | Neutral slider remainder. |
| 37 | Light | Progress remainders ≥ 3:1. |
| 38 | Native fit | Status bar only while busy; field focus ring; app-icon alerts. |
| 40 | Noise | Neutral chip; no idle status bar. |
| 41 | Non-text | Problem 5. |
| 42 | Truncation | No wrapped values; single-line AI status. |
| 43 | Dialogs | App icon + caution badge; visible default button; Esc snapshot. |

---

## 5. Functional findings (not visual)

1. "Generate Summary" is still the yellow primary while a meeting is recording (`meetings-recording-wide-*`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Every older fixture meeting starts at 14:30 (`main-meetings-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while "Improve Transcriptions" is listed; check whether the first prompt should be preselected (`main-postprocessing-*`).
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-*`). Backlog #5.
7. Voice enrollment still has two entry points (People "You" row and Meeting settings › You › Open in People), and API keys have two homes (Post Process vs AI summaries). Both are out of scope, noted once and not scored.
8. The startup skeleton sidebar omits Post Process and Debug (`startup-app-*`). If those rows appear once settings load, the sidebar will still jump. Check that the skeleton reads the same visibility flags as the shell.
9. The Deepgram key field relies on Enter to save ("Paste a key and press Enter"), while the other key fields have Test buttons. Saving is a behaviour choice; noted, not scored.
