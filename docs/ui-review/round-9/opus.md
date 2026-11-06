# Guidy UI review, round 9: reviewer `opus`

## Evidence

I opened 55 round-9 PNGs with the image tool, plus a 2x crop of the Speakers sliders. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (light, dark, min), `focus-1`, `hover-sidebar`, `hover-button` (About), `advanced-limit-confirm`, `postprocessing-discard`, Advanced, Post Process (min), Models (default, min, wide), Usage, Storage, `storage-error`, Debug, `debug-logs`, About.
- **History:** default and `history-selected-min`.
- **Meetings:** list/detail, detail-min, transcript, processing, empty, `meetings-find-wide`, `meetings-recording-wide`.
- **People:** default and min.
- **Meeting settings:** shell (both themes), Setup (default, min), You, Recording, Transcription, Speakers, Integrations, AI (cloud default, min, wide; `msettings-ai-local`, `msettings-ai-local-min`).
- **Onboarding and startup:** welcome, picker, permissions, setup-meetings, setup-cloud-min, `startup-app-min`, `startup-error`.
- **HUDs:** overlay, widget and expanded widget.

I also diffed every PNG against round 8, pixel by pixel, with a pure-Python PNG decoder in the scratch directory, splitting each diff into sidebar and content regions. 60 of 242 files are byte-identical. Of the 182 that changed:

- **Sidebar only (most files):** every collapsed-sidebar screen changed from y 152 px down (the new row and cluster metrics). In the expanded state (`meetings-settings-*`, every `msettings-*`), only the region below Integrations changed, from y 803 px down (cluster gap 6 → 8 pt).
- **Content, light theme only:** the slider and progress fill, `#5c3f00` → `#523800`. Affected: `main-debug-*`, `main-general-full`, `main-models-*`, `meetings-processing`, `msettings-speakers`.
- **Content, Set Up… chip:** `msettings-setup-light`, `msettings-setup-min-light`.
- **Content, local AI pane:** `msettings-ai-local-*` and `-min-*` (status line, sizes, Download button).
- **Alerts:** `advanced-limit-confirm-*`, `postprocessing-discard-*` moved again by 10–60 px. That is capture timing, as in round 8.
- **Not changed:** onboarding, startup, HUDs and History content are byte-identical.

Contrast uses WCAG 2.x relative luminance on sampled pixels. Scope rules applied as before:

- The status bar and the stock gpui-kit 1 pt focus edges are not counted.
- The `kit::confirm` Return default is not counted.
- The fixed 200 pt pop-up column is a recorded trade-off (PLAN.md). I do not count short values in wide triggers against #15.
- The three-level Meeting settings sidebar is a recorded decision. I score its execution, not its existence.
- The fixture's Debug and Post Process rows are discounted; I computed the typical sidebar separately.

## The round-9 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| AI status leads with the model; the model never truncates; the tail truncates with an ellipsis; verified at 720×480 | **Holds** | `msettings-ai-local-min-*` reads "Qwen3 4B Instruct · This Mac · offline" in full. 20 pt of clear background separates it from the On this Mac / Cloud provider switch (x 897–937 px). Nothing paints under the control. |
| One set of sidebar metrics in both states: 26 pt rows, 24 pt category rows, no row gaps, 8 pt cluster gaps | **Holds in the main window** | Collapsed and expanded both centre General / History / Meetings / People / Meeting settings at y 114 / 166 / 218 / 270 / 322 px. Rows are 52 px (26 pt), category rows 48 px (24 pt), and cluster breaks add 16 px (8 pt). Nothing above the expansion moves when it opens. **The startup skeleton was not updated** (Regression 1). |
| AI pane sizes and its Download button in decimal MB like Models and onboarding (2.5 / 5.0 GB) | **Holds** | `msettings-ai-local-*`: "2.5 GB" and "Download 5.0 GB". This matches `main-models-wide-*` (2.5 GB, 5.0 GB) and `onboarding-setup-meetings-*` (2.5 GB). |
| Light warning `#944c00`, 5.5:1 on the chip tint | **Passes, but the figure is high** | Chip text `#944c00` on its tint `#eee5db` measures **5.12:1** (it was 4.46). It passes AA; 5.5 is the figure against white. |
| Light slider and progress fill `#523800`: 3.4:1 against the track, 10.9:1 on white | **Holds for progress, not for sliders** | Progress: `#523800` on `#8f8f8a` is 3.36:1. On white it is 10.91:1. Sliders (`msettings-speakers-light`, `main-debug-light`) draw their remainder in `#837e6e`, not the track token: **2.69:1** against the fill (it was 2.43). The Storage "Guidy" segment still uses the old `#5c3f00`. |

### My round-8 problems

| Round-8 problem | Status |
|---|---|
| 1. Local AI status overflows into the switch at 720 pt | **Fixed** (`msettings-ai-local-min-*`). |
| 2. Two sidebar rhythms | **Fixed** in the window; the skeleton now differs instead (Regression 1). |
| 3. One model, two sizes, names, descriptions | **Sizes fixed.** Still not fixed: "Qwen3 8B Instruct" (AI pane) vs "Qwen3 8B" (Models). The AI pane says "Fast and capable. The default for summaries."; Models says "Fast, good summaries · Works on any Apple silicon Mac". |
| 4. Light chip 4.46:1 | **Fixed** (5.12:1). The text is still 12 pt in a 13 pt toolbar. |
| 5. Light button hover reads as pressed | **Not fixed** (`hover-button-light`, `#c5c5c1`). There are still no `pressed-*` snapshots. |
| 6. Post Process wraps two rows at minimum size | **Not fixed** (`main-postprocessing-min-dark`). |
| 7. Microcopy remainders | **Not fixed.** The picker still mixes periods with " · " (`onboarding-picker-light`). "Streaming ·" still ends a line (`main-models-min-dark`). |
| 8. Light accent tone drift; fill vs remainder under 3:1 | **Partly.** Progress now passes 3:1. Sliders reach 2.69:1, and the storage segments are unchanged at 1.6:1. The light theme now draws five accent tones: `#ffc72c`, `#a87500`, `#8a6000`, `#5c3f00` (storage) and `#523800` (sliders, progress). |
| 9. Two delete treatments | **Not fixed** (labeled Delete in `msettings-ai-local-*`, bare trash icon in `msettings-ai-*`). |
| 10. People emails truncate at 900 pt | **Not fixed** (`meetings-people-light`). |
| 11. Ambiguous category titles ("You", "Recording") | **Not fixed** (minor). |
| 12. Harness hover on the "Meeting settings" parent row | **Not fixed** (`meetings-settings-*`). The other `msettings-*` captures show no fill there, which confirms it is the pointer. |
| 13. Startup skeleton 2 pt left of the loaded sidebar | **Not fixed, and now larger** (Regression 1). |
| 14. Alert snapshots not position-stable | **Not fixed.** |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 8.

| # | Parameter | Guidy R9 | Guidy R8 | Codex | Claude | Reason for the round-9 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono for IDs, paths and the Usage model column (`main-usage-dark`, `storage-error-dark`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | 9 | 9 | 8 | 8 | The chip text is now 5.12:1 (`msettings-setup-light`). All measured status text passes. The slider remainder pair (2.69:1) is covered under #41. |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | Unchanged. The local status now leads with the model, the thing the user checks (`msettings-ai-local-*`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | The sidebar cluster gap is now 8 pt in both states, on the grid. |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | Status hues are fine. The light accent family is still not one tone: there are now five (Problem 3). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | Sizes and sidebar rhythm are fixed. Remaining: the skeleton's metrics, two delete forms, two model names, and cloud vs local status lines with opposite order. |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | The window is aligned. The skeleton is still offset 4 px in x (counted under #28 because it is a load jump). |
| 12 | Vertical rhythm | **9** | 8 | 8 | 9 | One sidebar rhythm, measured identical in `main-general-*` and `meetings-settings-*`. Post Process still wraps at minimum size. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | 8 pt cluster gaps in both states. They are tighter than round 8's collapsed 14 pt, but still read as groups. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Unchanged. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | Unchanged. The chip uses 12 pt text (nit). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | Unchanged. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still `#c5c5c1` (`hover-button-light`), and pressed is unproven. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Unchanged. |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-1-light`, `meetings-settings-dark`). |
| 20 | Target size | 9 | 9 | 8 | 8 | Rows at 26 and 24 pt meet 2.5.8. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Expanded and collapsed now share positions, so the sidebar does not jump on entry. A typical user's About sits about 32 pt above the list bottom at 900×600. |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Unchanged; the rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Unchanged. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-light`, `debug-logs-light`). |
| 28 | Loading and progress | **8** | 9 | 8 (k) | 8 (k) | `startup-app-*` still draws the round-8 sidebar: 28 pt pitch and 14 pt cluster gaps against the real 26 / 8. On load the list jumps by up to 16 pt in y and 2 pt in x (Regression 1). Progress fills are otherwise good. |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`storage-error-dark`, `startup-error-dark`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Unchanged. |
| 31 | Scannability | 9 | 9 | 8 | 8 | The local status now scans model-first; the cloud status scans condition-first (Problem 5). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | The picker periods, "Streaming ·" at a line end, and AI-pane sentence descriptions remain. |
| 33 | Numeric and tabular formatting | **9** | 8 | 8 | 7 | One size per model everywhere: 2.5 GB and 5.0 GB in the AI pane, Models and onboarding. |
| 34 | Minimum-size reflow (720×480) | **9** | 8 | 7 (k) | 8 (k) | `msettings-ai-local-min-*` fits with 20 pt of clearance. Post Process still wraps (`main-postprocessing-min-dark`). |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged (`msettings-ai-wide-dark`, `main-models-wide-light`, `meetings-find-wide-dark`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Unchanged. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | The chip is crisp now. The heavy hover and the near-black brown fills remain. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | Unchanged. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | Progress fill against remainder is 3.36:1. The slider remainder is 2.69:1, but the thumb marks the value. Storage segments are 1.6:1, with a legend. |
| 42 | Truncation and text accommodation | **9** | 8 | 8 | 6 | No clipped text anywhere. The cloud model ellipsizes at 720 pt ("Claude Sonn…") and emails truncate at 900 pt. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged in content (`advanced-limit-confirm-dark`, `postprocessing-discard-light`). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`overlay-live-dark`, `recording-widget-*`). |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Entering Meeting settings no longer re-flows the rows above. |

| Column | Mean of 45 |
|---|---|
| Guidy round 9 | **9.0** (403 / 45 = 8.96) |
| Guidy round 8 | **8.9** (400 / 45 = 8.89) |
| Codex | **7.8** (351 / 45) |
| Claude | **8.0** (361 / 45) |

**Holistic "would the lead designer ship this" (Guidy round 9): 9.**

- Round 8's two regressions are fixed and verified at both sizes and in both themes.
- The model sizes agree across the app.
- My sign-off condition from round 8, that no text runs into a control, is met.
- The new skeleton mismatch is a launch-moment jump. It is a one-file fix, not a ship blocker.

---

## 2. Regressions since round 8

1. **The startup skeleton no longer matches the sidebar it stands in for.**
   - **Evidence:**
     - `startup-app-light`: row centres at y 172 / 228 / 282 / 337 / 422 / 477 / 564 / 620 / 702 px, a 56 px pitch with 84–87 px cluster breaks.
     - `main-general-light`: the real sidebar sits at 166 / 218 / 270 / 322 / 390 / … / 734, a 52 px pitch with 68 px breaks.
     - At `-min`: People is at 282 vs 270, Meeting settings 338 vs 322, Models 422 vs 390, About 702 vs 734. Glyphs and text also sit 4 px (2 pt) left (`startup-app-min-light` vs `main-general-min-light`).
   - **Effect:** when Guidy finishes starting, every row below General moves, some by 16 pt.
   - **Cause:** round 9 changed the sidebar metrics in `main_window.rs`, but the skeleton in `onboarding/startup.rs` keeps its own copy of the old values.
   - **Holds down:** #28, #10, #11.
2. **The slider fill moved further from the accent family.** `#523800` is close to black-brown on white (crop of `msettings-speakers-light`). The storage segment kept `#5c3f00`, so one role now has two tokens. This is token debt, not a visible break. It is scored under #9, not as a new defect.
3. Nothing else got worse. The alert captures moved again (timing).

---

## 3. Remaining problems, ranked

None blocks a 9. Problem 1 is the only one introduced this round.

**1. The startup skeleton sidebar uses stale metrics.**
- **Evidence:** Regression 1 (`startup-app-*`, `startup-app-min-*`).
- **Holds down:** #28, #10, #11.
- **Fix:**
  - Build the skeleton sidebar from the same constants as `render_sidebar`: 26 pt rows, 8 pt cluster gaps, the same leading inset (glyph x 54 px, text x 88 px at 2x).
  - Better, render the real sidebar rows with `disabled` styling, so they can never drift again.
  - Add Post Process and Debug when they are enabled (functional finding 7).
- **File:** `ui/onboarding/startup.rs`, `ui/main_window.rs`.

**2. One model has two names and two descriptions.**
- **Evidence:**
  - "Qwen3 8B Instruct" (`msettings-ai-local-*`) vs "Qwen3 8B" (`main-models-wide-light`).
  - "Fast and capable. The default for summaries." vs "Fast, good summaries · Works on any Apple silicon Mac".
- **Holds down:** #10, #32.
- **Fix:** have the AI pane use the Models catalogue's display name and description strings, so there is one string per model.
- **File:** `pages/meetings/ai_settings.rs`, `translation.json`.

**3. Light accent tones: five where the plan names two.**
- **Evidence:**
  - Primary `#ffc72c`.
  - Switches `#a87500`.
  - Marks, links and icons `#8a6000`.
  - Storage segment `#5c3f00` (`main-storage-light`).
  - Sliders and progress `#523800`.
- **Contrast:** the slider remainder `#837e6e` is 2.69:1 against its fill. Storage Guidy vs Other (`#63635e`) is 1.61:1.
- **Holds down:** #9, #41, #37.
- **Fix:**
  - Use one light non-text accent, `#8a6000` (5.6:1 on white), for switches, sliders, progress and storage, as the plan intended.
  - Put every remainder on a lighter track, about `#c9c9c5`. That gives 4.7:1 against `#8a6000`, and the track border stays visible at 3:1 if it keeps a `#8f8f8a` hairline.
  - Make the storage "Other" segment `#c9c9c5` (light) / `#5a5a5a` (dark).
- **File:** `assets/themes/guidy.json` (`slider.background`, `progress.bar.background`), `ui/design.rs`, `pages/storage.rs`.

**4. The light button hover reads as pressed.**
- **Evidence:** `hover-button-light`: the "Check for Updates" fill is `#c5c5c1`, nearly as dark as a selected sidebar row.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover about `#ededeb`; keep `#c5c5c1` for pressed.
  - Add `pressed-*` snapshots.
- **File:** `ui/design.rs`, `kit.rs`, snapshot harness.

**5. The two AI status lines follow opposite orders.**
- **Evidence:**
  - Local: "Qwen3 4B Instruct · This Mac · offline"; the model is primary and never truncates.
  - Cloud: "Anthropic key needed · Claude Sonn…" (`msettings-ai-min-dark`); the condition is primary and the model truncates.
- **Defence:** each order is defensible, since a problem should lead. But the rule is not visible, and the cloud model still ellipsizes at 720 pt.
- **Holds down:** #10, #31, #42.
- **Fix:** state one rule: "condition first when something needs action, model first when ready". Apply it to both providers, so a ready cloud reads "Claude Sonnet 4.5 · Anthropic". When the cloud line needs to shrink, truncate the provider tail before the model, as the local line does.
- **File:** `pages/meetings/ai_settings.rs` (`status_line`).

**6. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark`: the API key and Model fields drop below their labels.
- **Holds down:** #12, #34.
- **Fix:** keep the trailing control inline with `min_w(200)`/`flex_1` until the label column reaches its 180 pt floor, as the other `SettingRow`s already do.
- **File:** `pages/post_processing.rs`.

**7. Microcopy remainders.**
- **Evidence:**
  - Periods next to " · " siblings (`onboarding-picker-light`).
  - "Streaming ·" ends a line (`main-models-min-dark`).
  - Full-sentence descriptions in the AI pane.
- **Holds down:** #32.
- **Fix:**
  - Bind " · " to the following fragment with a non-breaking space.
  - Use the " · "-joined fragment form for every model description.
- **File:** `translation.json`, `kit::model_description`.

**8. Two delete treatments in one pane.**
- **Evidence:** labeled "Delete" (`msettings-ai-local-*`) vs a bare 14 pt trash glyph (`msettings-ai-*`).
- **Holds down:** #10, #14.
- **Fix:** use a 24 pt icon button with a tooltip in both places, or a labeled "Remove" in both.
- **File:** `pages/meetings/ai_settings.rs`.

**9. People emails truncate at 900 pt.**
- **Evidence:** three of four rows in `meetings-people-light` ("ada@example.c…").
- **Holds down:** #42.
- **Fix:** widen the list to 260 pt between 900 and 1280 pt, or wrap the email onto its own line.
- **File:** `pages/meetings/people.rs`.

**10. The Set Up… chip is 12 pt in a 13 pt toolbar.**
- **Evidence:** `msettings-setup-*`.
- **Holds down:** #15 (nit).
- **Fix:** use 13 pt, the same as the other toolbar buttons.
- **File:** `pages/meetings/recorder.rs`.

**11. Category titles can be ambiguous on their own.**
- **Evidence:** "You" (`msettings-you-dark`) and "Recording" (`msettings-recording-light`).
- **Fix:** window title "Meeting settings — You", or a muted "Meeting settings ›" prefix at wide sizes.
- **File:** `ui/main_window.rs`.

**12. Harness: the pointer leaves a hover fill on "Meeting settings".**
- **Evidence:** `meetings-settings-*`.
- **Fix:** park the pointer outside the window before capture.

**13. Harness: alert captures are not position-stable.**
- **Evidence:** `advanced-limit-confirm-*`, `postprocessing-discard-*`.
- **Fix:** capture after the entry transition settles, or disable motion for the snapshot run.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 17 | States | A calmer light hover and `pressed-*` snapshots (Problem 4). |
| 28 | Loading and progress | Skeleton sidebar built from the real sidebar metrics (Problem 1). |

To get from 9 to 10:

- #9, #37 and #41: Problem 3.
- #10: Problems 1, 2, 5 and 8.
- #12 and #34: Problem 6.
- #32: Problems 2 and 7.
- #42: Problems 5 and 9.
- #15: Problem 10.
- #11: Problem 1 (x offset).

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-light`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-dark`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-light`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while a prompt is listed. Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-min-dark`). Backlog #5.
7. The startup skeleton sidebar omits Post Process and Debug (`startup-app-*`).
8. Meeting settings › Setup still shows the "Set Up…" toolbar chip while Setup is open. Hiding it removes an entry point, which is out of scope.
9. Fixture: the Setup badge reads "1" on other Meeting settings pages but "3" on Setup itself. One readiness state for all `msettings-*` captures would avoid a badge that seems to change.
10. Out-of-scope IA, noted once:
    - voice enrollment has two entry points;
    - API keys have two homes (Post Process and AI summaries).
