# Guidy UI review, round 7: reviewer `opus`

## Evidence

I opened 50 round-7 PNGs with the image tool, plus two crops comparing the sidebar bottom with round 6. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General, `focus-1`, `focus-3`, `focus-9`, `hover-sidebar`, `hover-button` (About), `advanced-limit-confirm`, Debug, `debug-logs`, Post Process (min, discard), Models (min, wide), Usage, Storage and `storage-error`.
- **History:** default.
- **Meetings:** empty, detail, detail-min, transcript, processing, recording-wide and `hover-list-row`.
- **People:** default, min and `mpeople-person`.
- **Meeting settings:** shell (default and full), Setup (default and min), You, Recording, Speakers, AI (cloud default, min and wide, both themes; `msettings-ai-local`, `msettings-ai-local-min`), Linear and Integrations.
- **Onboarding:** welcome, picker, permissions and setup-meetings.
- **Startup and HUDs:** `startup-app-min`, the overlay and the expanded widget.

I also diffed every changed PNG against round 6, pixel by pixel. 186 of 242 files changed:

- In all but the AI panes, only the sidebar region changed (x < 400 px at 900 pt). The content is pixel-identical, so the scores for those screens carry over from round 6.
- The content changed only in `msettings-ai-local-*` (the helper line) and `msettings-ai-min-*` / `msettings-ai-local-min-*` (the status line).

Measurements were taken in 2x pixels with a pure-Python PNG decoder in the scratch directory. Contrast uses WCAG 2.x relative luminance.

Scope rules applied:

- The status bar and the stock gpui-kit focus edges are not counted.
- The `kit::confirm` Return default is not counted.
- New commands are not counted.
- The fixture's Debug and Post Process rows are discounted where the brief says so; I computed the typical sidebar separately.

## The round-7 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| Sidebar rows 26 pt | Holds | Top-level glyph pitch is 56 px (26 pt row + 2 pt gap), down from 60 px. |
| Category rows back to 24 pt | Holds | Category pitch is 52 px (24 + 2), up from 48 px. Codex's target-size concern is met. |
| A fade at the bottom shows that the list scrolls | Holds, with a side effect | `meetings-settings-*` and every `msettings-*`: a 28 pt gradient into the sidebar tone. It covers the whole last row: Storage text drops from 16.0:1 (round 6) to 4.7:1 and reads as disabled (Regression 1). |
| "A typical sidebar with the categories open fits at 900×600" | **Does not hold** | Computed from the measured pitches (56 px per row, 52 px per category, 76 px across a cluster break): without Post Process and Debug, Storage centres at y 1070 px and About at y 1146 px. The About row spans 1121–1173 px, but the list ends at 1152 px (the status bar). About is half clipped under the fade; about 20 pt is missing (10 pt of row plus the 10 pt bottom inset). |
| Local AI pane says "offline" once | Holds | `msettings-ai-local-*`: the status line reads "… This Mac · offline", and the helper is now "Choose a downloaded model." |
| AI status stays on one line: condition first, model name truncating | Holds for the cloud pane; the local pane wraps instead | `msettings-ai-min-*`: "Anthropic key needed · Claude Sonn…" on one line. `msettings-ai-local-min-*`: "Summaries are made by" / "Qwen3 4B Instruct · This Mac · offline" on two lines. Both are clean (no line starts with "·"), but the two panes now handle the same situation differently (Problem 7). |

### My round-6 problems

| Round-6 problem | Status |
|---|---|
| 1. Debug and About below the fold, no cue | **Partly fixed.** There is now a scroll cue. The fixture shows the same rows as before (Storage, now faded), and a typical user still loses About (see the log table). |
| 2. One model, two sizes, names, descriptions | **Not fixed.** 2.6 GB (`msettings-ai-local-*`) vs 2.5 GB (`main-models-wide-*`, `onboarding-setup-meetings-*`); 5.2 vs 5.0 GB. "Qwen3 8B Instruct" vs "Qwen3 8B". |
| 3. Light button hover reads as pressed | **Not fixed.** `hover-button-light` is still `#c5c5c1`. |
| 4. AI status orphans its separator at 720 pt | **Fixed.** |
| 5. "This Mac, offline" twice | **Fixed.** |
| 6. Post Process wraps two rows at minimum size | **Not fixed** (`main-postprocessing-min-dark`). |
| 7. Microcopy remainders | **Not fixed.** Picker periods remain; "Streaming ·" still sits at a line end in `main-models-min-light`. |
| 8. Light accent tone drift; slider and storage segments under 3:1 | **Not fixed** (byte-identical content). |
| 9. Two delete treatments | **Not fixed.** |
| 10. People emails truncate at 900 pt | **Not fixed.** |
| 11. Ambiguous category titles ("You", "Recording") | **Not fixed** (minor). |
| 12. Harness hover on the "Meeting settings" row | **Not fixed.** `meetings-settings-*`: the parent row is `#e1e1e0` / `#2c2c2c`. |
| 13. Startup skeleton offset | **Partly fixed.** The row pitch now matches (56 px). Glyphs are still at x 50 against 54 and text at x 84 against 88 (`startup-app-min-light`). |

### New this round (present before, measured now)

The toolbar's "Set Up…" chip fails text contrast in light mode.

- **Evidence:** `msettings-setup-light`, `msettings-setup-min-light`. The text is `#c86b00` on its own tint `#f4e9db`, which is **3.15:1** at 12 pt.
- **Cause:** `render_setup_chip` uses the raw theme `warning` colour (`#c86b00`), not `design::status_color(Attention)` (`#a35400`). The "Not granted" text on the same page uses `#a35400` and reaches 5.5:1.
- **Dark mode:** passes (`#ff9e45` on `#34281d`, 7.0:1).
- **Round 6:** the pixels are identical, so this is not a regression. I missed it in earlier rounds and score it now.

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 6.

| # | Parameter | Guidy R7 | Guidy R6 | Codex | Claude | Reason for the round-7 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono for IDs and paths (`main-usage-*`, `storage-error-*`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | **8** | 9 | 8 | 8 | The light "Set Up…" chip is 3.15:1 at 12 pt (`msettings-setup-light`), an AA text failure on a call to action. Everything else holds (`#a35400` status text 5.5:1, progress fill 9.7:1). |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Unchanged. Toolbar 17, group 15, row 13 pt. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | Sidebar pitches are now 28 and 26 pt (rows of 26 and 24 plus a 2 pt gap), both on the grid. |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | Unchanged. There are still four light accent tones (Problem 8). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | Remainders: model size, name and description differ by page; two delete forms; the two AI status lines reflow differently. |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | The skeleton is still 2 pt off the loaded sidebar. |
| 12 | Vertical rhythm | 9 | 9 | 8 | 9 | The sidebar's two pitches read as two levels. Post Process at minimum size still mixes inline and wrapped rows. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Unchanged. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Unchanged. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | The Set Up chip is 12 pt text in a 13 pt toolbar (minor). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | The fade signals that the list scrolls, but it also makes the last row look disabled. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still the pressed-looking `#c5c5c1`, and there are no `pressed-*` snapshots. Sidebar hover (half the selection fill, 1.16:1) is fine. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Unchanged. |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-1`, `focus-3`, `focus-9`). |
| 20 | Target size | 9 | 9 | 8 | 8 | Category rows are back to 24 pt; top-level rows are 26 pt. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Now there is a scroll cue. A typical user's About is still half clipped at 900×600 with the categories open. |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Unchanged; the rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Unchanged. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-*`, `debug-logs-*`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-processing-*`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Unchanged. The chip contrast is counted under #3. |
| 30 | Data density | 9 | 9 | 8 | 8 | "Offline" is said once now. |
| 31 | Scannability | 9 | 9 | 8 | 8 | Unchanged. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | The orphaned "·" is gone; the picker periods and "Streaming ·" at a line end remain. |
| 33 | Numeric and tabular formatting | 8 | 8 | 8 | 7 | 2.5 vs 2.6 GB and 5.0 vs 5.2 GB for the same models. |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 7 (k) | 8 (k) | The AI status no longer orphans; Post Process still wraps. |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged; the whole expanded sidebar fits at 1440×900 (`msettings-ai-wide-*`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Unchanged. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | The heavy hover, the dark-brown fill and the weak chip are small costs. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | A scroll-edge fade is platform-like (macOS 26 uses soft scroll edges). |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | Unchanged (storage segments 1.6:1 / 1.8:1; the numbers carry the meaning). |
| 42 | Truncation and text accommodation | 9 | 9 | 8 | 6 | At 720 pt the cloud status truncates the model name ("Claude Sonn…") with 18 pt to spare. Emails still truncate at 900 pt. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged (`advanced-limit-confirm-*`, `postprocessing-discard-*`). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged. |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Unchanged (from source). |

| Column | Mean of 45 |
|---|---|
| Guidy round 7 | **8.9** |
| Guidy round 6 | **9.0** |
| Codex | **7.8** |
| Claude | **8.0** |

The mean dips from 8.96 to 8.93. That comes only from re-scoring an existing contrast defect (#3); nothing visible got worse enough to move a score.

**Holistic "would the lead designer ship this" (Guidy round 7): 9.**

- The sidebar work is the right direction: proper target sizes and an honest scroll cue.
- The AI-pane copy fixes landed.
- The chip contrast is a one-token fix.
- I still sign off. The remaining list is polish, led by the 20 pt the expanded sidebar still needs.

---

## 2. Regressions since round 6

1. **The fade turns the last fully visible row into a "disabled" row.**
   - **Evidence:**
     - `meetings-settings-light`: Storage was `#161616` on `#f1f1ef` (16.0:1) in round 6; it is now `#6b6b6a` (4.7:1) under a 28 pt gradient.
     - The same happens in every `msettings-*` at 900×600, and to Models at 720×480 (`msettings-setup-min-*`, `msettings-ai-min-*`).
     - The row is still clickable, but its signifier now says it is not.
   - **Holds down:** #16, #21 (not enough to move either score).
   - **Fix:** see Problem 1.
2. **At 900×600 the fixture shows no more global rows than in round 6.** Storage sits at the same y (1122 vs 1127 px), because the taller category rows cancel the shorter top-level rows. Not a regression, but the change did not buy the room it aimed for.
3. Nothing else got worse. Outside the sidebar and the two AI panes, every changed PNG is pixel-identical to round 6.

---

## 3. Remaining problems, ranked

None of these blocks a 9.

**1. The expanded sidebar is still about 20 pt too tall for a typical user at 900×600, and the fade dims a whole row.**
- **Evidence:**
  - Computed from the measured pitches in `meetings-settings-*` and `msettings-*`: without Post Process and Debug, About spans y 1121–1173 px against a list bottom of 1152 px.
  - With the fixture's two extra rows, Storage is faded and Debug and About are hidden.
- **Holds down:** #21, #16.
- **Fix (presentation only):**
  - While the categories are listed, drop the 2 pt gap between category rows, so the 24 pt rows abut. Their hit areas are still 24 pt, which meets 2.5.8. That saves 18 pt.
  - Shrink the cluster gaps from 8 to 6 pt (saves 6 pt). Together that is 24 pt, enough for About plus the inset.
  - Shorten the fade from 28 to 12–16 pt, so it softens the edge of a clipped row instead of greying a whole one. Pair it with a 1 px `border` hairline at the bottom edge while `more_below`.
- **File:** `ui/main_window.rs` (`render_sidebar`: the `gap(px(2.))` on the list, the cluster spacer, the fade `h(dp(28.))`).

**2. The light "Set Up…" chip text is 3.15:1.**
- **Evidence:** `msettings-setup-light`, `msettings-setup-min-light`: `#c86b00` on `#f4e9db`.
- **Holds down:** #3, #37, #15.
- **Fix:**
  - Use `design::status_color(Status::Attention, cx)` for the text and glyph (`#a35400` light, `#ff9e45` dark).
  - Lower the tint to about 0.08, so the text clears 4.5:1 on its own fill (`#a35400` reaches 4.6:1 on the current 0.12 tint).
  - Use 13 pt text, like the other toolbar buttons.
- **File:** `pages/meetings/recorder.rs` (`render_setup_chip`).

**3. One model shows two sizes, two names and two descriptions.**
- **Evidence:**
  - Qwen3 4B: 2.6 GB (`msettings-ai-local-*`) vs 2.5 GB (`main-models-wide-*`, `onboarding-setup-meetings-*`).
  - Qwen3 8B: 5.2 vs 5.0 GB.
  - "Qwen3 8B Instruct" vs "Qwen3 8B".
  - "Fast and capable. The default for summaries." vs "Fast, good summaries · Works on any Apple silicon Mac".
- **Holds down:** #33, #10, #32.
- **Fix:**
  - Use one byte formatter.
  - Take the AI-pane name and description from the Models catalogue strings.
- **File:** `kit.rs`, `pages/meetings/ai_settings.rs`.

**4. The light button hover reads as pressed, and pressed is unproven.**
- **Evidence:** `hover-button-light`: `#c5c5c1`.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover = `secondary` (about `#ededeb`); keep `#c5c5c1` for pressed.
  - Add `pressed-*` snapshots.
- **File:** `ui/design.rs`, `kit.rs`, snapshot harness.

**5. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark`.
- **Holds down:** #12, #34.
- **Fix:** let the key and model fields shrink to the 200 pt column (`min_w(200)`, `flex_1`) before wrapping.
- **File:** `pages/post_processing.rs`, `kit.rs` (`SettingRow`).

**6. Microcopy remainders.**
- **Evidence:**
  - Sentence periods next to " · "-joined siblings (`onboarding-picker-*`, `onboarding-setup-meetings-*`).
  - "Streaming ·" at a line end (`main-models-min-light`).
  - Full sentences in the AI-pane descriptions.
- **Holds down:** #32.
- **Fix:**
  - Use the Models strings everywhere.
  - Bind " · " to the following fragment with a non-breaking space.
- **File:** `translation.json`, `kit::model_description`.

**7. The two AI status lines reflow differently at 720 pt, and the cloud one truncates the object.**
- **Evidence:**
  - `msettings-ai-min-*`: "Anthropic key needed · Claude Sonn…", with about 18 pt free before the switch.
  - `msettings-ai-local-min-*`: the status wraps to two clean lines.
- **Holds down:** #42, #10.
- **Fix:** use the local pane's behaviour for both. Wrap at the separator, so the model name moves to line two whole. The model name is what the user is checking.
- **File:** `pages/meetings/ai_settings.rs`.

**8. Light accent tone drift, and fill against remainder below 3:1.**
- **Evidence:**
  - Light accent tones: `#ffc72c`, `#a87500`, `#8a6000` and `#5c3f00`.
  - Slider fill against remainder: 2.4:1.
  - Storage segments: 1.6:1 (light) and 1.8:1 (dark).
- **Holds down:** #9, #41.
- **Fix:**
  - Use `#8a6000` for every light fill.
  - Give sliders and progress a light remainder of about `#d6d6d2`.
  - Make the storage "Other" segment `#c9c9c5` (light) / `#5a5a5a` (dark).
- **File:** `ui/design.rs`, `guidy.json`, `pages/storage.rs`.

**9. Two delete treatments in one pane.**
- **Evidence:** a labeled "Delete" (`msettings-ai-local-*`) next to a bare 14 pt trash icon (`msettings-ai-*`).
- **Holds down:** #10, #14.
- **Fix:** use labeled "Delete" / "Remove" for both, or 24 pt icon buttons with tooltips for both.
- **File:** `pages/meetings/ai_settings.rs`.

**10. People emails truncate at 900 pt.**
- **Evidence:** three of four rows in `meetings-people-*`.
- **Holds down:** #42.
- **Fix:** widen the list to 260 pt between 900 and 1280 pt.
- **File:** `pages/meetings/people.rs`.

**11. Category titles can be ambiguous on their own.**
- **Evidence:** "You" (`msettings-you-*`) and "Recording" (`msettings-recording-*`).
- **Fix:** set the window title to "Meeting settings — You", or add a muted "Meeting settings ›" prefix at wide sizes.
- **File:** `ui/main_window.rs`.

**12. Harness: the "Meeting settings" parent row shows a hover fill.**
- **Evidence:** `meetings-settings-*`: `#e1e1e0` / `#2c2c2c` next to the ringed selection.
- **Fix:** park the pointer outside the sidebar before capture.
- **File:** snapshot harness.

**13. The startup skeleton sidebar sits 2 pt left of the loaded one.**
- **Evidence:** `startup-app-min-light`: glyphs at x 50 against 54, text at x 84 against 88. The row pitch now matches.
- **Holds down:** #11.
- **Fix:** reuse the `render_sidebar` paddings (`px(8)`, `pl(8)`).
- **File:** `ui/onboarding/startup.rs`.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 3 | Color contrast | Problem 2 (one token). |
| 17 | States | A calmer light hover and `pressed-*` snapshots (Problem 4). |
| 33 | Numeric formatting | One byte formatter (Problem 3). |

To get from 9 to 10:

- #21 and #16: Problem 1.
- #10: Problems 3, 7 and 9.
- #34 and #12: Problem 5.
- #32: Problem 6.
- #42: Problems 7 and 10.
- #9 and #41: Problem 8.
- #11: Problem 13.
- #37: Problems 2, 4 and 8.

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-*`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while a prompt is listed. Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key. Backlog #5.
7. Onboarding Permissions makes "Continue Without Accessibility" the default (`onboarding-permissions-*`). Backlog #6.
8. The startup skeleton sidebar omits Post Process and Debug (`startup-app-min-*`).
9. The 1000 / 1024 byte split behind Problem 3 (backlog I4).
10. Meeting settings › Setup still shows a "Set Up…" toolbar chip while Setup is open (`msettings-setup-*`). Hiding it removes an entry point, which is out of scope.
11. Out-of-scope IA, noted once:
    - voice enrollment has two entry points;
    - API keys have two homes (Post Process and AI summaries).
