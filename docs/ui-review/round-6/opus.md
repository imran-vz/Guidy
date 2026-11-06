# Guidy UI review, round 6: reviewer `opus`

## Evidence

I opened 52 round-6 PNGs with the image tool, plus one round-5 PNG and two crops for comparison. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (default, full), `focus-1`, `hover-sidebar`, `hover-button`, Advanced, `advanced-limit-confirm`, Post Process (min, discard), Models (default, min, wide), Debug, Usage, Storage, `storage-error` and About (via `hover-button`).
- **History:** default.
- **Meetings:** empty, detail, detail-min, transcript, processing and recording-wide.
- **People:** default, min, wide, `meetings-people-person-min`, `mpeople-person` and `mpeople-you`.
- **Meeting settings:** shell (both themes), setup, setup-min, Meeting notes (`msettings-general`), You, Recording, Transcription, Speakers, AI (cloud default, min and wide; `msettings-ai-local`, `msettings-ai-local-min`), Linear and Integrations.
- **Onboarding:** picker, permissions, setup-meetings and setup-cloud-min.
- **Startup and HUDs:** `startup-app-min`, the overlay and the expanded widget.

I also byte-compared all 242 PNGs against round 5. Sixty changed: People, every `msettings-*`, `meetings-settings-*`, and the light Models, Debug, Storage and processing snapshots. The two alerts moved by a few pixels of animation timing. Everything else is byte-identical, so the scores for unchanged screens carry over from round 5.

Measurements were taken in 2x pixels with a pure-Python PNG decoder in the scratch directory, because Pillow is not installed. Contrast uses WCAG 2.x relative luminance.

Scope rules applied:

- The status bar and stock gpui-kit focus edges are not counted.
- The `kit::confirm` Return default is not counted.
- New commands are not counted.

## The round-6 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| People subtitles lead with the voiceprint count | Holds | `meetings-people-*`: "3 voiceprints · ada@example.c…". Only the email truncates. |
| 8 pt cluster gaps and 22 pt category rows while the categories are listed | Holds | The selected category fill is 44 px (22 pt), and the category pitch went from 26 to 24 pt. The gap from Integrations to Models went from 42 to 37 pt. At 900×600, Storage is now fully visible; Debug and About are still below the fold (Problem 1). |
| Compact categories indent one step past the parent | Holds | `msettings-setup-min-*`, `msettings-ai-min-light`: the category text starts at x 132, the parent text at x 112 (a 10 pt step). |
| The toolbar title names the category; the pane drops its 17 pt heading | Holds | `msettings-transcription-light`: toolbar "Transcription", then the "After the meeting" group header. Content moves up by about 60 px. |
| The AI status line and the On this Mac / Cloud provider switch share one row | Holds at 900 and 1440 pt; costs at 720 pt | `msettings-ai-*`, `msettings-ai-wide-*`. At minimum width the status wraps and leaves "· Claude Sonnet 4.5" on its own line (Regression 1). |
| Light slider and progress fill `#5c3f00`, 3.0:1 against the remainder | Holds for progress, not for sliders | Progress (`main-models-light`, `meetings-processing-light`): the fill is 9.7:1 on white and 2.98:1 against `#8f8f8a`. Sliders (`main-debug-light`, `msettings-speakers-light`) use a `#857f6e` remainder, which gives 2.43:1. Dark sliders are 2.47:1 (`#ffc72c` against `#8d8162`). |
| Local AI pane: "offline" said once | **Does not hold** | `msettings-ai-local-*`: "… Qwen3 4B Instruct · This Mac · offline", then directly below "Choose a downloaded model. It runs on this Mac, offline." The helper was reordered, not reduced. |
| A model's size line only once it is downloaded | Holds | The Qwen3 8B row no longer repeats 5.2 GB under the "Download 5.2 GB" button. |

### My round-5 problems

| Round-5 problem | Status |
|---|---|
| 1. People subtitles truncate the count | **Fixed.** The count always shows; only the email truncates. |
| 2. Sidebar overflows while Meeting settings is open | **Partly fixed.** Storage is back. Debug and About are still off-screen at 600 pt, with no scroll cue. |
| 3. Compact categories read as siblings | **Fixed** (a 10 pt indent). |
| 4. Two stacked 17 pt titles | **Fixed.** |
| 5. One model, two sizes | **Not fixed.** Qwen3 4B is 2.6 GB in the AI pane and 2.5 GB in Models and onboarding; Qwen3 8B is 5.2 vs 5.0 GB. |
| 6. Light button hover reads as pressed; pressed unproven | **Not fixed** (`hover-button-light` is still `#c5c5c1`). |
| 7. Post Process wraps at minimum size | **Not fixed** (`main-postprocessing-min-dark`). |
| 8. Microcopy remainders | **Not fixed** (picker periods; "Streaming ·" at a line end in `main-models-min-light`). |
| 9. "Offline / this Mac" twice | **Not fixed** (see the log table). |
| 10. Two delete treatments | **Not fixed.** |
| 11. Harness hover on the "Meeting settings" row | **Not fixed** (`meetings-settings-*`). |
| 12. Startup skeleton offset | **Not fixed** (`startup-app-min-light`: names at x 84, against x 88 when loaded). |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 5.

| # | Parameter | Guidy R6 | Guidy R5 | Codex | Claude | Reason for the round-6 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono for IDs and paths (`main-usage-*`, `storage-error-*`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | 9 | 9 | 8 | 8 | The light progress fill is 9.7:1. Slider fill against remainder is 2.4:1, but the thumb marks the value. |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | One title per Meeting-settings pane (`msettings-*`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | The duplicate 17 pt title is gone. Toolbar 17, group 15, row 13 pt throughout. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | The 24 pt category pitch and the 2 pt gap match the top-level rhythm. |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. The AI pane keeps its measure at wide size (`msettings-ai-wide-*`). |
| 9 | Color and theme | 9 | 9 | 8 | 8 | The light accent now has four tones: `#ffc72c`, `#a87500`, `#8a6000` and `#5c3f00` (Problem 8). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | Remainders: model size, name and description differ by page; two delete forms. |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | The compact categories now sit one step in. |
| 12 | Vertical rhythm | 9 | 9 | 8 | 9 | Post Process at minimum size still mixes inline and wrapped rows. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | The AI status and its switch read as one header row. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Unchanged. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | Unchanged. |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | Unchanged. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still the pressed-looking `#c5c5c1`, and there are no `pressed-*` snapshots. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Unchanged. |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-1`: 2 pt ring). |
| 20 | Target size | 9 | 9 | 8 | 8 | Category rows are now 22 pt tall on a 24 pt pitch. That is still at the WCAG 2.5.8 floor, since the spacing exemption covers the 2 pt gap. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | The compact level reads correctly and Storage is back. Debug and About are still hidden at 600 pt with no cue. |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Unchanged; the rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | The toolbar title is the location. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-*`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | The progress fill is clearer in light (`meetings-processing-light`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Unchanged. |
| 30 | Data density | 9 | 9 | 8 | 8 | The size line is no longer repeated; "offline / this Mac" is still said twice. |
| 31 | Scannability | 9 | 9 | 8 | 8 | The people count now sits at a stable x on every row. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | Nits only (Problem 7). |
| 33 | Numeric and tabular formatting | 8 | 8 | 8 | 7 | 2.5 vs 2.6 GB and 5.0 vs 5.2 GB for the same models. |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 7 (k) | 8 (k) | Two remainders: Post Process wraps, and the AI status orphans its separator. |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged. |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Unchanged. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | Progress is crisper. The heavy hover and the dark-brown fill are small costs. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | Unchanged. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Meeting-settings content starts about 60 px higher. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | The storage bar segments Guidy and Other are 1.61:1 (light) and 1.80:1 (dark), up from 1.08 in light. The numbers below the bar carry the information. |
| 42 | Truncation and text accommodation | 9 | 8 | 8 | 6 | Names and counts never truncate. The three emails at 900 pt and one meeting title are secondary text. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged. |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged. |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Unchanged (from source). |

| Column | Mean of 45 |
|---|---|
| Guidy round 6 | **9.0** |
| Guidy round 5 | **8.9** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 6): 9.**

I signed off in round 5, and round 6 fixed the three items I wanted fixed first:

- the People truncation;
- the compact indent;
- the stacked titles.

No new problem rises above a nit. Getting the remaining parameters from 9 to 10 is a punch list, not a redesign.

---

## 2. Regressions since round 5

1. **At 720 pt, the AI status line now wraps beside the switch.**
   - **Evidence:**
     - `msettings-ai-min-*`: "Anthropic key needed" / "· Claude Sonnet 4.5". The second line starts with the separator.
     - `msettings-ai-local-min-*`: "Summaries are made by" / "Qwen3 4B Instruct · This Mac · offline".
     - In round 5 the status had its own full-width row and fit on one line.
   - **Holds down:** #34, #32 (not enough to move either score).
2. **Side effect, not a contrast regression: the light progress and slider fill left the accent's hue family.** At `#5c3f00` it reads as dark brown next to the `#a87500` switches and the `#ffc72c` primary buttons (`main-debug-light`, `msettings-speakers-light`, `main-storage-light`). Contrast improved, so I do not count it against #3.
3. Nothing else got worse. The two alerts differ only by animation timing.

---

## 3. Remaining problems, ranked

None of these blocks a 9.

**1. With Meeting settings open, Debug and About are still below the fold at 900×600, and nothing says so.**
- **Evidence:** `meetings-settings-*` and every `msettings-*` at the default size: the list ends at Storage (y 1128 px), with the status bar at y 1152. At 720×480 (`msettings-setup-min-*`) it ends at Models.
- **Holds down:** #21.
- **Fix (presentation only):**
  - Add a scroll-edge cue while the sidebar can scroll: a 1 px `border` hairline at the bottom edge, or a 12 pt fade into `sidebar` bg.
  - To fit at 600 pt, about 70 pt more is needed:
    - top-level rows 28 → 26 pt;
    - category rows 22 → 20 pt;
    - the top inset reduced by 4 pt.
- **File:** `ui/main_window.rs` (`render_sidebar`), `ui/design.rs`.

**2. One model shows two sizes, two names and two descriptions.**
- **Evidence:**
  - Qwen3 4B: 2.6 GB in `msettings-ai-local-*`, but 2.5 GB in `main-models-wide-*` and `onboarding-setup-meetings-*`.
  - Qwen3 8B: 5.2 GB vs 5.0 GB.
  - The 8B is "Qwen3 8B" in Models but "Qwen3 8B Instruct" in the AI pane.
  - The 4B is "Fast, good summaries · Works on any Apple silicon Mac" in Models but "Fast and capable. The default for summaries." in the AI pane.
- **Holds down:** #33, #10, #32.
- **Fix:**
  - Route `format_model_size` through the one decimal byte formatter.
  - Take the AI-pane name and description from the same catalogue strings Models uses.
- **File:** `kit.rs` (`model_description`, the size formatter), `pages/meetings/ai_settings.rs`.

**3. The light-theme button hover reads as pressed, and pressed is unproven.**
- **Evidence:** `hover-button-light`: Check for Updates goes from `#fafaf9` to a solid `#c5c5c1`, the same tone as the border.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover = `secondary` (about `#ededeb`); keep `#c5c5c1` for pressed.
  - Add `pressed-button` and `pressed-list-row` snapshots.
- **File:** `ui/design.rs`, `kit.rs`, snapshot harness.

**4. At 720 pt the AI status line orphans its separator (Regression 1).**
- **Holds down:** #34, #32.
- **Fix:**
  - Below about 820 pt pane width, drop the status to its own row under the switch (the round-5 layout).
  - Or keep the row and render the status as two spans with no leading separator, letting the model name wrap as a unit ("Anthropic key needed" / "Claude Sonnet 4.5").
- **File:** `pages/meetings/ai_settings.rs`.

**5. The local AI pane still says "this Mac, offline" twice.**
- **Evidence:** `msettings-ai-local-*`.
- **Holds down:** #30.
- **Fix:** reduce the helper to "Choose a downloaded model." The status line already says "This Mac · offline".
- **File:** `pages/meetings/ai_settings.rs`, `assets/locales/en/translation.json`.

**6. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark`. API key and Model drop their fields to a second line, while Provider stays inline.
- **Holds down:** #12, #34.
- **Fix:** let the key field and the model-name field shrink to the 200 pt column (`min_w(200)` with `flex_1`) before wrapping. "API key" needs about 60 pt.
- **File:** `pages/post_processing.rs`, `kit.rs` (`SettingRow`).

**7. Microcopy remainders.**
- **Evidence:**
  - Picker and setup: "Fast and accurate. Supports 25 European languages." and "Very fast, English only." next to " · "-joined siblings (`onboarding-picker-*`, `onboarding-setup-meetings-*`).
  - Models: a capital after the dot ("Fast and accurate · Supports 25 …").
  - "Streaming ·" stranded at a line end at 720 pt (`main-models-min-light`).
  - AI pane descriptions use full sentences with periods.
- **Holds down:** #32.
- **Fix:**
  - Use the Models strings everywhere: "Fast and accurate · 25 European languages", "Very fast · English only".
  - Bind " · " to the following fragment with a non-breaking space.
- **Files:** `translation.json`, `kit::model_description`.

**8. Light accent tone drift, and fill against remainder below 3:1 on sliders and the storage bar.**
- **Evidence:**
  - Light accent tones: `#ffc72c` (primary fill), `#a87500` (switch on), `#8a6000` (focus ring, radio, links) and `#5c3f00` (slider and progress fill).
  - Slider fill against remainder: 2.43:1 (L) and 2.47:1 (D).
  - Storage segments Guidy against Other: 1.61:1 (L, `#5c3f00` / `#63635e`) and 1.80:1 (D, `#ffc72c` / `#9a9a9a`).
- **Holds down:** #9, #41 (nits only; the thumb and the numbers carry the meaning).
- **Fix:**
  - Use `#8a6000` for every light fill, so there is one "strong accent" tone.
  - Give sliders and progress the same light remainder, about `#d6d6d2`. That is 3.6:1 against `#8a6000`, so the fill reads at a glance.
  - Give the storage "Other" segment a light neutral (`#c9c9c5` L / `#5a5a5a` D), so Guidy against Other is ≥ 3:1. Keep the outlined Free segment.
- **File:** `ui/design.rs`, `assets/themes/guidy.json`, `pages/storage.rs`.

**9. Two delete treatments in one pane.**
- **Evidence:** a labeled "Delete" for local models (`msettings-ai-local-*`), a bare 14 pt trash for saved cloud models (`msettings-ai-*`).
- **Holds down:** #10, #14.
- **Fix:** use labeled buttons for both, "Delete" (files) and "Remove" (from the list). Or use 24 pt plain icon buttons with tooltips for both.
- **File:** `pages/meetings/ai_settings.rs`.

**10. People emails still truncate at 900 pt.**
- **Evidence:** three of four rows in `meetings-people-*`. They fit at 720 pt, because the list takes the whole pane there, and at 1440 pt.
- **Holds down:** #42 (9 → 10 only).
- **Fix:** widen the People list to 260 pt between 900 and 1280 pt, matching the Meetings list's growth above 1280 pt.
- **File:** `pages/meetings/people.rs`.

**11. Category titles can be ambiguous on their own.**
- **Evidence:** the toolbar now reads "You" (`msettings-you-*`), the same word as the "You" row in People, or "Recording" (`msettings-recording-*`), the same word as the recording state.
- **Comment:** the sidebar disambiguates, so this is minor.
- **Fix:** set the window title (not the toolbar title) to "Meeting settings — You", or add a 13 pt muted "Meeting settings ›" prefix in the toolbar at wide sizes only.
- **File:** `ui/main_window.rs`.

**12. Harness: `meetings-settings-*` still shows a hover fill on the parent row.**
- **Evidence:** "Meeting settings" is drawn at `#e1e1e0` / `#2c2c2c` next to the ringed selection.
- **Fix:** park the pointer outside the sidebar before capture.
- **File:** snapshot harness.

**13. The startup skeleton sidebar is offset from the loaded one.**
- **Evidence:** `startup-app-min-light`: text at x 84 and glyphs at x 50, where the loaded sidebar has x 88 and x 54.
- **Holds down:** #11.
- **Fix:** build the skeleton from the `render_sidebar` paddings.
- **File:** `ui/onboarding/startup.rs`.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 17 | States | A calmer light hover and `pressed-*` snapshots (Problem 3). |
| 33 | Numeric formatting | One byte formatter, so each model has one size (Problem 2). |

To get from 9 to 10:

- #21: Problem 1.
- #10: Problems 2 and 9.
- #34: Problems 4 and 6.
- #12: Problem 6.
- #30: Problem 5.
- #32: Problems 4 and 7.
- #9 and #41: Problem 8.
- #42: Problem 10.
- #11: Problem 13.

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-*`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while a prompt is listed. Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-min-*`). Backlog #5.
7. Onboarding Permissions makes "Continue Without Accessibility" the default (`onboarding-permissions-*`). Backlog #6.
8. The startup skeleton sidebar omits Post Process and Debug (`startup-app-min-*`).
9. The 1000 / 1024 byte split behind Problem 2 (backlog I4).
10. Meeting settings › Setup shows a "Set Up…" toolbar button while Setup is the open category (`msettings-setup-*`). Hiding it removes an entry point, which is out of scope.
11. Out-of-scope IA, noted once:
    - voice enrollment has two entry points (People › You and Meeting settings › You › Open in People);
    - API keys have two homes (Post Process and AI summaries).
