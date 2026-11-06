# Guidy UI review, round 8: reviewer `opus`

## Evidence

I opened 50 round-8 PNGs with the image tool, plus round-7 counterparts and 2x crops where the change was small. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (both), `focus-1`, `hover-sidebar`, `hover-button` (About), `advanced-limit-confirm` (round 7 and 8), `debug-logs`, Post Process (min, discard), Models (min, wide), Usage, Storage and `storage-error`.
- **History:** default.
- **Meetings:** empty, detail, detail-min, transcript and recording-wide.
- **People:** default.
- **Meeting settings:** shell (default both themes, full), Setup (default both themes, min), Meeting notes, You, Recording, Transcription, Speakers, AI (cloud default, min and wide; `msettings-ai-local`, `msettings-ai-local-min` in both themes), Linear and Integrations.
- **Onboarding:** welcome, picker, permissions and setup-meetings.
- **Startup and HUDs:** `startup-app-min`, the overlay and the expanded widget.

I also diffed every PNG against round 7, pixel by pixel, with a pure-Python PNG decoder in the scratch directory. 205 of 242 files are byte-identical. Of the 37 that changed:

- 30 changed only in the sidebar: `meetings-settings-*` and every `msettings-*` (the expanded sidebar).
- `msettings-setup-light` and `msettings-setup-min-light` changed in the toolbar chip.
- `msettings-ai-local-*` changed in the size line; `msettings-ai-local-min-*` changed in the status line.
- `debug-logs-light` changed only in the green "Live logs" dot (`#2f9a58` → `#23804a`).
- `advanced-limit-confirm-*` and `postprocessing-discard-*`: the alert is the same size and content but sits 10–20 px higher or lower. That looks like capture timing during the alert's entry, not a design change.

Everything outside those regions carries its round-7 score.

Contrast uses WCAG 2.x relative luminance on sampled pixels. Scope rules applied as before:

- The status bar and the stock gpui-kit 1 pt focus edges are not counted.
- The `kit::confirm` Return default is not counted.
- New commands are not counted.
- The fixture's Debug and Post Process rows are discounted; I computed the typical sidebar separately.

## The round-8 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| Light warning `#c86b00` → `#a35400`, success `#2f9a58` → `#23804a` | Holds | The chip text is now `#a35400` (`msettings-setup-light`). The Live logs dot is `#23804a`. Granted / Downloaded text is 4.9:1 on white. |
| "Every place that draws status text with the theme colours now passes" | **Just misses at one place** | The Set Up… chip is `#a35400` on its own tint `#f0e6db`: **4.46:1** at 12 pt. That is a near miss of 4.5:1, not a pass. Dark: `#ff9e45` on `#34281d`, 7.0:1. |
| Expanded sidebar: no row gaps, 6 pt cluster gaps | Holds | Top-level pitch is 52 px (26 pt), category pitch 48 px (24 pt). Cluster breaks add 10–14 px (≈ 6 pt). |
| A typical sidebar with the categories open fits at 900×600 | **Holds** | Measured centres in `meetings-settings-light`: Usage 988, Storage 1040, Debug 1104 px. Without Post Process and Debug, About would centre at about y 1051 px and span 1025–1077 px against a list bottom of 1152 px, leaving about 37 pt. |
| The fade is gone; debug builds scroll like any source list | Holds | No gradient. In the fixture About is cut in half by the status bar, a normal source-list scroll cue. Storage is back to full contrast. |
| Local AI status on one line; the "· This Mac · offline" tail truncates first | **Holds at 900 pt, breaks at 720 pt** | `msettings-ai-local-*`: one line, fits. `msettings-ai-local-min-*`: the tail is gone, but the model name overflows its box. It reads "Summaries are made by Qwen3 4B Instru", clipped mid-glyph with no ellipsis, and runs flush into the On this Mac / Cloud provider control with no gap (Regression 1). |
| Local model sizes use the Models page's formatter | **Does not hold** | `msettings-ai-local-*` now says 2.4 GB for Qwen3 4B (it said 2.6 GB). The Models page (`main-models-wide-*`) and onboarding (`onboarding-setup-meetings-*`) say 2.5 GB. "Download 5.2 GB" for Qwen3 8B still disagrees with 5.0 GB on Models. |

### My round-7 problems

| Round-7 problem | Status |
|---|---|
| 1. Expanded sidebar about 20 pt too tall; the fade dims a row | **Fixed** for a typical user, and the fade is gone. Side effect: the sidebar now has two rhythms (Regression 2). |
| 2. Light Set Up… chip at 3.15:1 | **Mostly fixed**: 4.46:1. The tint needs to drop slightly (Problem 4). |
| 3. One model, two sizes, names, descriptions | **Not fixed.** The AI pane changed from 2.6 to 2.4 GB, still not 2.5 GB. 5.2 vs 5.0 GB, "Qwen3 8B Instruct" vs "Qwen3 8B", and two descriptions remain. |
| 4. Light button hover reads as pressed | **Not fixed** (`hover-button-light`, `#c5c5c1`). |
| 5. Post Process wraps two rows at minimum size | **Not fixed** (`main-postprocessing-min-dark`). |
| 6. Microcopy remainders | **Not fixed.** Picker periods next to " · " siblings (`onboarding-picker-*`). "Streaming ·" at a line end (`main-models-min-light`). |
| 7. The two AI status lines reflow differently at 720 pt | **Worse.** Cloud still ellipsizes the model; local now clips it without an ellipsis (Regression 1). |
| 8. Light accent tone drift; slider and storage segments under 3:1 | **Not fixed** (byte-identical). |
| 9. Two delete treatments | **Not fixed.** |
| 10. People emails truncate at 900 pt | **Not fixed** (`meetings-people-*`). |
| 11. Ambiguous category titles | **Not fixed** (minor). |
| 12. Harness hover on the "Meeting settings" row | **Not fixed.** `meetings-settings-*` still shows the parent-row fill next to the ringed selection. |
| 13. Startup skeleton offset | **Not fixed** (`startup-app-min-light`: glyph x 50 vs 54, text x 84 vs 88). |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 7.

| # | Parameter | Guidy R8 | Guidy R7 | Codex | Claude | Reason for the round-8 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono for IDs and paths (`main-usage-*`, `storage-error-*`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | **9** | 8 | 8 | 8 | The light chip went from 3.15 to 4.46:1 (`msettings-setup-light`), a hair under AA. Status text is now `#a35400` 5.3:1 and `#23804a` 4.7–4.9:1. |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | The expanded sidebar's 6 pt cluster gap is off the {4, 8, 12} set (minor). |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | Status hues now sit one step darker in light mode and stay distinct. Light accent tones still drift (Problem 8). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | Remainders: model size and name differ by page, two delete forms, AI status lines behave differently, and the sidebar rhythm depends on state. |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | The leading edges are unchanged. The skeleton is still 2 pt off. |
| 12 | Vertical rhythm | **8** | 9 | 8 | 9 | The sidebar now uses 28 pt pitch and about 14 pt cluster gaps when collapsed (`main-general-*`), but 26/24 pt pitch and 6 pt gaps when expanded (`meetings-settings-*`). Rows above the expansion shift 2–8 pt as you enter and leave Meeting settings. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | The 6 pt cluster gap between Integrations and Models is weak, but indentation still separates the two levels. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Unchanged. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | The chip is still 12 pt text in a 13 pt toolbar (minor). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | The fade no longer makes a live row look disabled. A half-cut last row is an honest scroll cue. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still `#c5c5c1`, and there are no `pressed-*` snapshots. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Unchanged. |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-1`). |
| 20 | Target size | 9 | 9 | 8 | 8 | Expanded rows abut at 26 and 24 pt; the hit areas still meet 2.5.8. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | A typical user now sees every destination at 900×600 with the categories open. |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Unchanged; the rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Unchanged. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-*`, `debug-logs-*`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Unchanged. |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`storage-error-*`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Unchanged. |
| 31 | Scannability | 9 | 9 | 8 | 8 | Unchanged. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | The picker periods and "Streaming ·" at a line end remain. |
| 33 | Numeric and tabular formatting | 8 | 8 | 8 | 7 | 2.4 vs 2.5 GB and 5.2 vs 5.0 GB for the same models. |
| 34 | Minimum-size reflow (720×480) | **8** | 9 | 7 (k) | 8 (k) | `msettings-ai-local-min-*`: the status overflows into the place switch. Post Process still wraps. |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged (`msettings-ai-wide-*`, `main-models-wide-*`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Unchanged. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | Status text is now crisp. The heavy hover and the dark-brown fills remain. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | A source list that simply scrolls is the native behaviour. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | Unchanged. |
| 42 | Truncation and text accommodation | **8** | 9 | 8 | 6 | The model name, the object of the status line, is clipped mid-glyph with no ellipsis at 720 pt. The cloud name ellipsizes ("Claude Sonn…"). Emails still truncate at 900 pt. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged in content. |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged. |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Unchanged (from source). The sidebar re-flow on entering Meeting settings is counted under #12. |

| Column | Mean of 45 |
|---|---|
| Guidy round 8 | **8.9** (400 / 45 = 8.89) |
| Guidy round 7 | **8.9** (402 / 45 = 8.93) |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 8): 9.**

- The sidebar now fits for real users, and the misleading fade is gone.
- The status tokens are fixed at the source.
- I sign off on condition that the local AI status overflow at 720 pt is fixed before release. It is a one-line layout fix, but it is the only place in the app where text runs into a control.

---

## 2. Regressions since round 7

1. **The local AI status overflows into the place switch at the minimum size.**
   - **Evidence:** `msettings-ai-local-min-light`, `msettings-ai-local-min-dark`. The line reads "Summaries are made by Qwen3 4B Instru", cut through the "c". The text runs into the segmented control's edge with 0 pt between them, where the header's `gap_3` should give 12 pt.
   - **Round 7:** the same status wrapped to two clean lines.
   - **Cause** (`pages/meetings/ai_settings.rs`, `status_line`):
     - The heading ("Summaries are made by") and the model are both `flex_none`, so only the `· {via}` tail can shrink.
     - At 720 pt the heading and model alone need about 24 pt more than the column has.
     - They overflow the `min_w_0` parent, which does not clip, and the switch paints over them.
   - **Holds down:** #42, #34, #10.
2. **The sidebar's rhythm now depends on state.**
   - **Evidence:**
     - Collapsed (`main-general-*`, `main-history-*`): 56 px pitch (26 pt row + 2 pt gap), cluster breaks of 84–86 px.
     - Expanded (`meetings-settings-*`, every `msettings-*`): 52 px pitch, cluster breaks of 62–66 px.
   - **Effect:** entering Meeting settings moves History up 2 pt, Meetings 4 pt, People 6 pt and Meeting settings itself 8 pt (y 337 → 321 px). Leaving moves them back. The cluster gaps shrink from about 14 pt to about 6 pt, so the groups also change shape.
   - **Holds down:** #12, #10, #13.
3. Nothing else got worse. The two alert snapshots differ only in vertical offset, which looks like capture timing.

---

## 3. Remaining problems, ranked

Problem 1 blocks a 9 on #34 and #42. Problem 2 blocks a 9 on #12. The rest are polish.

**1. The local AI status overflows at 720 pt (blocks a 9).**
- **Evidence:** `msettings-ai-local-min-*` (Regression 1).
- **Holds down:** #42, #34, #10.
- **Fix:**
  - Make the line behave like the cloud one: let the model element `min_w_0().truncate()`, so it ellipsizes ("Qwen3 4B Instr…"), and give the heading `flex_shrink` too.
  - Better, at narrow widths drop "Summaries are made by" and show glyph + model + tail, or move the status below the switch. The model name is what the user checks.
  - Add `overflow_hidden()` on the status column as a backstop, so nothing can paint under the switch.
- **File:** `pages/meetings/ai_settings.rs` (`status_line`, `header`).

**2. One sidebar rhythm for both states (blocks a 9 on #12).**
- **Evidence:** Regression 2.
- **Holds down:** #12, #10, #13.
- **Fix:**
  - Use the expanded metrics everywhere: 26 pt top-level rows and 24 pt category rows abutting (no gap), and one cluster gap of 8 pt in both states.
  - Budget for a typical user, expanded: 10 × 26 + 9 × 24 + 3 × 8 = 500 pt against about 524 pt available at 900×600, so it still fits.
  - Hover and selection fills keep their 24/26 pt height, and nothing above the expansion moves when it opens.
- **File:** `ui/main_window.rs` (`render_sidebar`: the per-state `gap` and cluster spacer).

**3. One model shows two sizes, two names and two descriptions.**
- **Evidence:**
  - Qwen3 4B: 2.4 GB (`msettings-ai-local-*`) vs 2.5 GB (`main-models-wide-*`, `onboarding-setup-meetings-*`).
  - Qwen3 8B: 5.2 GB ("Download 5.2 GB") vs 5.0 GB.
  - "Qwen3 8B Instruct" vs "Qwen3 8B".
  - "Fast and capable. The default for summaries." vs "Fast, good summaries · Works on any Apple silicon Mac".
- **Holds down:** #33, #10, #32.
- **Fix:**
  - Feed the AI pane the same value the Models row shows (catalogue size through the same formatter) for both the size line and the Download button.
  - If 2.4 GB is the real on-disk size, show it on Models too.
  - Take the name and description from the Models catalogue strings.
- **File:** `pages/meetings/ai_settings.rs`, `kit.rs` / `pages/models.rs` formatter.

**4. The light Set Up… chip is 4.46:1, just under AA.**
- **Evidence:** `msettings-setup-light`, `msettings-setup-min-light`: `#a35400` on `#f0e6db`.
- **Holds down:** #3 (nit), #15.
- **Fix:**
  - Drop the tint from about 0.12 to 0.08 (about `#f4ede5`, giving 4.7:1), or darken the chip text to `#944c00` (5.2:1 on the current tint).
  - Use 13 pt text, like the other toolbar buttons.
- **File:** `pages/meetings/recorder.rs` (`render_setup_chip`).

**5. The light button hover reads as pressed, and pressed is unproven.**
- **Evidence:** `hover-button-light`: `#c5c5c1`.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover = about `#ededeb`; keep `#c5c5c1` for pressed.
  - Add `pressed-*` snapshots.
- **File:** `ui/design.rs`, `kit.rs`, snapshot harness.

**6. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark` (API key and Model fields drop below their labels).
- **Holds down:** #12, #34.
- **Fix:** let the trailing fields shrink to a 200 pt column (`min_w(200)`, `flex_1`) before wrapping.
- **File:** `pages/post_processing.rs`, `kit.rs` (`SettingRow`).

**7. Microcopy remainders.**
- **Evidence:**
  - Sentence periods next to " · "-joined siblings (`onboarding-picker-*`, `onboarding-setup-meetings-*`).
  - "Streaming ·" at a line end (`main-models-min-light`).
  - Full sentences in the AI-pane descriptions.
- **Holds down:** #32.
- **Fix:**
  - Use the Models strings everywhere.
  - Bind " · " to the following fragment with a non-breaking space.
- **File:** `translation.json`, `kit::model_description`.

**8. Light accent tone drift, and fill against remainder below 3:1.**
- **Evidence:**
  - Four light accent tones: `#ffc72c`, `#a87500`, `#8a6000` and `#5c3f00` (the slider and progress fills in `msettings-speakers-light`).
  - Slider fill against remainder: 2.4:1.
  - Storage segments: 1.6:1 (light) and 1.8:1 (dark).
- **Holds down:** #9, #41.
- **Fix:**
  - Use `#8a6000` for every light fill.
  - Give sliders and progress a remainder of about `#d6d6d2`.
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
- **Fix:** widen the list to 260 pt between 900 and 1280 pt, or put the email on its own line.
- **File:** `pages/meetings/people.rs`.

**11. Category titles can be ambiguous on their own.**
- **Evidence:** "You" (`msettings-you-*`) and "Recording" (`msettings-recording-*`).
- **Fix:** set the window title to "Meeting settings — You", or add a muted "Meeting settings ›" prefix at wide sizes.
- **File:** `ui/main_window.rs`.

**12. Harness: the "Meeting settings" parent row shows a hover fill.**
- **Evidence:** `meetings-settings-*`.
- **Fix:** park the pointer outside the sidebar before capture.
- **File:** snapshot harness.

**13. The startup skeleton sidebar sits 2 pt left of the loaded one.**
- **Evidence:** `startup-app-min-light`: glyphs at x 50 against 54, text at x 84 against 88.
- **Holds down:** #11.
- **Fix:** reuse the `render_sidebar` paddings.
- **File:** `ui/onboarding/startup.rs`.

**14. Harness: alert snapshots are not position-stable.**
- **Evidence:** `advanced-limit-confirm-*`, `postprocessing-discard-*`: the same alert moves 10–20 px between rounds.
- **Fix:** capture after the entry transition settles, or disable motion in the snapshot run, so diffs show only design changes.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 12 | Vertical rhythm | One sidebar rhythm for both states (Problem 2), and no Post Process wrap at minimum size (Problem 6). |
| 17 | States | A calmer light hover and `pressed-*` snapshots (Problem 5). |
| 33 | Numeric formatting | One size value per model everywhere (Problem 3). |
| 34 | Minimum-size reflow | Fix the local status overflow (Problem 1), then Problem 6. |
| 42 | Truncation | Problem 1, then the People emails (Problem 10). |

To get from 9 to 10:

- #3: Problem 4.
- #10: Problems 2, 3 and 9.
- #32: Problem 7.
- #9 and #41: Problem 8.
- #11: Problem 13.
- #37: Problems 4, 5 and 8.
- #7 and #13: an 8 pt cluster gap (Problem 2).

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-*`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while a prompt is listed. Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key. Backlog #5.
7. The startup skeleton sidebar omits Post Process and Debug (`startup-app-min-*`).
8. The 1000 / 1024 byte split behind Problem 3 (backlog I4). The AI pane's switch from 2.6 to 2.4 GB suggests it now shows a binary-GiB or on-disk figure, while Models shows the catalogue figure.
9. Meeting settings › Setup still shows a "Set Up…" toolbar chip while Setup is open (`msettings-setup-*`). Hiding it removes an entry point, which is out of scope.
10. Fixture: the Setup badge reads "1" on every other Meeting settings page but "3" on Setup itself, matching "3 things still need your attention". The snapshots use different readiness states; one state for all `msettings-*` captures would avoid a badge that seems to change.
11. Out-of-scope IA, noted once:
    - voice enrollment has two entry points;
    - API keys have two homes (Post Process and AI summaries).
