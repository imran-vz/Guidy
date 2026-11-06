# Guidy UI review, round 5: reviewer `opus`

## Evidence

I opened 55 PNGs with the image tool: 53 from round 5 and 2 from round 4 for comparison. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (default, min, full), `focus-1`, `focus-11`, `focus-12`, `hover-sidebar`, `hover-button`, Advanced (min), Post Process (default, min, discard), Usage, Storage error, Debug (min, full) and About (via `hover-button`).
- **History:** default and `history-selected-min`.
- **Models:** min and wide.
- **Meetings:** detail, detail-min, transcript, processing, recording-wide and `hover-list-row`.
- **People:** default, min, wide and `meetings-people-person-min`.
- **Meeting settings:** shell (both themes), setup, setup-min, Meeting notes, You, Recording, Transcription, Speakers, AI (cloud default, min and wide; `msettings-ai-local`, `msettings-ai-local-min`), Linear and Integrations.
- **Onboarding:** picker, permissions, setup-meetings and setup-cloud-min.
- **Startup and HUDs:** `startup-app-min`, the overlay and the expanded widget.

I diffed all 24 `focus-N` PNGs against `main-general-*` pixel by pixel. I also measured:

- sidebar, list and button fills for hover, selection and focus;
- cap heights of the toolbar, category, group and row type.

Pixels were read with a pure-Python PNG decoder in the scratch directory, because Pillow is not installed. Contrast uses WCAG 2.x relative luminance. Coordinates are 2x pixels.

Scope rules applied (from the brief):

- The status bar and stock gpui-kit focus edges are not counted.
- The filled destructive button in `kit::confirm` is the true Return default, so the alert is drawn correctly. I no longer count my round-4 Problem 5.
- Adding commands such as "Show Logs" on the storage error is out of scope.

## The round-5 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| 2 pt ring on the selected sidebar row while the sidebar has focus | Holds | `focus-1`: an opaque `#8a6000` / `#ffc72c` 2 pt border on the selected row. Against the sidebar it is 4.95:1 (L) / 10.4:1 (D); against the selected fill it is 3.34 / 6.05. |
| Rows keep the control inline down to a 180 pt label; wrapped controls stay in the trailing column | Holds | `main-general-min-*` and `main-debug-min-*`: every pop-up, field and slider is inline, and the labels wrap instead. `main-postprocessing-min-dark`: API key and Model wrap, but right-aligned to x 1357 (see Problem 7). |
| Form columns left-anchored at the page inset | Holds | Models wide starts at x 311 = the page inset (`main-models-wide-*`). The People detail is left-anchored (`meetings-people-wide-*`). |
| `focus-7/9/11/12` and `hover-*` snapshots | Holds | `focus-7` and `focus-9` are the Language and Microphone resets. `focus-11` and `focus-12` are switch edges. Hover fills: sidebar `#e1e1e0` against selected `#d2d2d0` (1.16:1 apart). List rows show a hover fill and a "…" overflow. |
| Meeting settings categories as a third sidebar level; middle column and compact pop-up gone | Holds | `meetings-settings-*` and every `msettings-*` snapshot now show the shell with nine indented children under Meeting settings and one pane. |
| People rows share a 16 pt avatar slot; header "Known people"; detail left-anchored | Holds, with a cost | `meetings-people-*`: all names start at x 481. The slot pushed three of four subtitles into truncation at 900 pt (Regression 1). |
| Accessory slot after the 200 pt column | Holds | `main-debug-min-*`: the Sound theme pop-up is 200 pt, with ▶ after it. `main-postprocessing-*`: refresh after Choose. Provider is now 200 pt (x 1318–1717). |
| Storage error shows the expected path in the code font | Holds | `storage-error-*`: `/tmp/guidy` in mono under the sentence. |
| Description fragments joined with " · " and no inner periods | Mostly | Models and Breeze ASR are fixed. In the picker, Parakeet still reads "Fast and accurate. Supports 25 European languages." (`onboarding-picker-*`, `onboarding-setup-meetings-*`). |
| History pushed detail hides the list filters | Holds | `history-selected-min-*`: only Show in Finder remains. |
| Onboarding: one 20 pt leading slot | Holds | `onboarding-setup-meetings-*`: the checkbox and the icons are centred at x 374. |
| Compact cloud setup shows provider, key, help and link in the first viewport | Holds | `onboarding-setup-cloud-min-dark`. |
| `msettings-ai-local` snapshots; neutral "Default" chip | Holds | `msettings-ai-local-*`. |

### My round-4 problems

| Round-4 problem | Status |
|---|---|
| 1. Sidebar focus nearly invisible | **Fixed.** A 2 pt ring, ≥ 4.9:1. |
| 2. One row layout at every width | **Fixed** on General, Debug, Advanced and Meeting settings. The one remainder is Post Process at minimum size (Problem 7). |
| 3. Meeting settings shows three navigation columns | **Fixed.** The categories are sidebar children. This brings two new costs (Problems 2 and 3). |
| 4. Wide pages centre | **Fixed.** |
| 5. Alert default button | **Withdrawn** by the round-5 clarification. |
| 6. Two icon-slot rules | **Fixed:** leading reset, trailing accessory, 200 pt pop-ups. |
| 7. Storage error has no next step | **Fixed in scope.** The path is shown; Show Logs would be a new command. |
| 8. People page details | **Fixed**, but the slot causes Regression 1. |
| 9. Microcopy nits | **Mostly fixed.** Picker residue remains (Problem 8). |
| 10. Hover and pressed unproven | **Half fixed.** Hover is shown; pressed is not. |
| 11. Onboarding leading edge | **Fixed.** |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 4.

| # | Parameter | Guidy R5 | Guidy R4 | Codex | Claude | Reason for the round-5 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | One family; mono for IDs and paths (`main-usage-*`, `storage-error-*`, `msettings-ai-*`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. The selected sidebar row is medium only. |
| 3 | Color contrast | 9 | 9 | 8 | 8 | The sidebar ring is 4.95 / 10.4 against the sidebar. Body text and tracks are unchanged. |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | One pane per category, one primary per region (`msettings-*`, `meetings-processing-*`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Cap heights 24 / 21 / 19 px (17 / 15 / 13 pt). The Meeting-settings category title repeats the toolbar title's 17 pt (Problem 4). |
| 6 | Design language | 9 | 9 | 8 | 9 | The categories use the sidebar tokens; no foreign column. |
| 7 | Padding and spacing | 9 | 8 | 8 | 8 | No mixed wrap gaps; the reset sits 5 pt from its control everywhere (`main-general-min-*`, `main-debug-min-*`). |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Every pop-up is 200 pt, including Provider and Sound theme. Forms are left-anchored. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | The accent is still limited to on-states, primary fills, focus and links. |
| 10 | Overall consistency | 9 | 8 | 8 (k) | 9 (k) | One slot rule and one row layout. Small remainders: delete is a labeled button in the local AI list and a bare trash in the cloud list (`msettings-ai-local-*` vs `msettings-ai-*`). |
| 11 | Alignment and grid | 9 | 8 | 8 | 9 | The trailing column holds at 720 pt. The People avatar slot and onboarding slot are aligned. The compact sidebar puts category names at the parent's x (Problem 3). |
| 12 | Vertical rhythm | 9 | 8 | 8 | 9 | Rows in a group share a layout; heights differ only by description lines. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Unchanged. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Category glyphs at 14 pt; one meaning per glyph. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | Unchanged; 200 pt pop-ups everywhere. |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | Hover reveals the row "…" (`hover-list-row-*`). Commands are named. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Hover now shown for sidebar, list rows and buttons; disabled and busy are shown. Pressed is still unproven, and the light-theme button hover (`#c5c5c1`) reads as pressed (Problem 6). |
| 18 | Selection clarity | 9 | 8 | 8 | 8 | Focused selection = fill + ring; unfocused = fill only. One treatment in every list. |
| 19 | Focus visibility | 9 | 8 | 7 (k) | 7 (k) | Custom elements (sidebar, tabs) draw a 2 pt ring; stock edges are accepted per scope; switches are captured (`focus-11/12`). |
| 20 | Target size | 9 | 9 | 8 | 8 | 24 pt category rows; unchanged elsewhere. |
| 21 | Navigation and wayfinding | 9 | 8 | 8 | 8 | One navigation column, location titled, toolbar back. Costs: at 900×600 an open Meeting settings pushes Storage, Debug and About below the fold, and compact children read as siblings (Problems 2 and 3). |
| 22 | Information architecture | 9 | 8 | 7 (k) | 8 (k) | Meeting settings now reads as part of Meetings. The rest is out of scope (functional list). |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Unchanged. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | One pane where there were two sidebar-toned columns. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | No exceptions found. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged. |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-processing-*`, `main-models-*`). |
| 29 | Error and status messaging | 9 | 8 | 7 (k) | 7 (k) | The storage error now names the path. Setup uses icon + text + colour (`msettings-setup-*`). |
| 30 | Data density | 9 | 9 | 8 | 8 | The local AI pane says "offline / this Mac" twice (Problem 9); everything else is lean. |
| 31 | Scannability | 9 | 9 | 8 | 8 | Stable slots on Models and Setup. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | Nits only (Problem 8). |
| 33 | Numeric and tabular formatting | 8 | 9 | 8 | 7 | New evidence: the same model shows two sizes (Qwen3 4B: 2.6 GB in the AI pane, 2.5 GB in Models and onboarding; Qwen3 8B: 5.2 vs 5.0 GB). |
| 34 | Minimum-size reflow (720×480) | 9 | 8 | 7 (k) | 8 (k) | Controls stay inline. Only Post Process wraps, and it stays right-aligned. |
| 35 | Wide-size use of space | 9 | 8 | 8 | 8 | Models, People detail and the AI pane are left-anchored with a kept measure (`*-wide-*`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | The ring at 10.4:1 is crisp, not glaring. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | Crisp; one heavy hover (Problem 6). |
| 38 | Native platform fit | 9 | 8 | 7 | 7 | Source-list focus ring, nested source list like Mail or Finder, Settings-style rows that don't stack, and NSAlert placement. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | The Setup badge is a dot plus a number; the ring is ≥ 3:1 on both neighbours. |
| 42 | Truncation and text accommodation | 8 | 9 | 8 | 6 | Regression: three of four People subtitles truncate at 900 pt (`meetings-people-*`), plus one meeting title (`meetings-detail-*`). |
| 43 | Dialogs and modality | 9 | 8 | 7 (k) | 8 (k) | Re-scored under the clarification: the filled Discard is the Return default, Esc cancels, the app-icon tile carries a caution badge, and the alert sits at the top (`postprocessing-discard-*`). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`recording-widget-expanded-*`, `overlay-live-*`). |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Unchanged (from source, as in round 4). |

| Column | Mean of 45 |
|---|---|
| Guidy round 5 | **8.9** |
| Guidy round 4 | **8.7** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 5): 9.**

All three round-4 blockers are fixed:

- the sidebar focus ring;
- one row layout at every width;
- one navigation column for Meeting settings.

What remains is a punch list of polish, not a blocker:

- the People list subtitles truncating at the default size;
- the nested sidebar overflowing and flattening at compact width;
- two stacked 17 pt titles;
- a size figure that differs between pages.

I would sign off with that list attached.

---

## 2. Regressions since round 4

1. **The People list now truncates its subtitles at the default size.**
   - **Evidence:**
     - `meetings-people-light` / `-dark`: "ada@example.com · 3 voicepri…", "bob@example.com · 3 voicepri…" and "chen@example.com · 3 voicep…".
     - In `round-4/meetings-people-light` all three fit.
   - **Cause:** the new 16 pt avatar slot and its gap take about 24 pt from a 240 pt list.
   - **Effect:** the information that is cut is the voiceprint count, the item that differs between people.
   - **Holds down:** #42, #31.
2. **The sidebar is taller than the window while Meeting settings is open.**
   - **Evidence:** `meetings-settings-*` and `msettings-*` at 900×600. Usage is cut at the status bar, and Storage, Debug and About are off-screen. At 720×480 (`msettings-setup-min-*`) Models is cut as well.
   - **Comment:** this is the cost of the fix I asked for in round 4. It is a scrolling source list, so nothing is lost, but a third of the destinations disappear when one pane opens.
   - **Holds down:** #21.
3. Nothing else got worse.

---

## 3. Remaining problems, ranked

None of these blocks a 9 overall. Problems 1–3 are the ones I would fix before calling it done.

**1. People list subtitles truncate (Regression 1).**
- **Holds down:** #42, #31.
- **Fix:**
  - Put the count first and let only the email truncate: "3 voiceprints · ada@example.com", with `truncate` on the email span only.
  - Or widen the People list to 260 pt between 900 and 1280 pt (the Meetings list already grows to 280 pt above 1280 pt).
  - Or drop the generic person glyph on known people and keep the 16 pt slot only for "You". This undoes my round-4 suggestion, but alignment can be kept by indenting the "You" text to x 433 with the glyph hanging in the 16 pt list inset.
- **File:** `pages/meetings/people.rs`.

**2. Opening Meeting settings pushes three destinations off the sidebar.**
- **Evidence:** Regression 2.
- **Holds down:** #21, #34.
- **Fix (presentation only):** recover about 90 pt so that About fits at 600 pt:
  - top-level rows 30 → 28 pt (`SIDEBAR_ITEM_H`);
  - category rows 24 → 22 pt;
  - group gaps 12 → 8 pt while the categories are expanded.
  - If it still overflows, scroll the selected category into view (already done) and add a 1 px top/bottom hairline on the sidebar only while it can scroll, so the overflow is signalled.
- **File:** `ui/main_window.rs` (`render_sidebar`), `ui/design.rs`.

**3. In the compact sidebar the categories read as siblings of People and Meeting settings.**
- **Evidence:** `msettings-setup-min-*`, `msettings-ai-local-min-dark`, `msettings-ai-min-light`. The category names start at x 112, the same x as "People" and "Meeting settings", and their glyphs are dropped, so the only cue for the third level is the missing icon.
- **Holds down:** #21, #11, #10.
- **Fix:** indent the categories 12 pt past the parent's name (`pl` 46 → 58 pt in the compact branch). Even "Transcription" and "AI summaries" (about 85 pt at 13 pt) fit in the 172 pt compact sidebar. Optionally keep the 14 pt glyph, as at the default width.
- **File:** `ui/main_window.rs:663` (the `(_, true)` arm).

**4. Two 17 pt titles are stacked in Meeting settings.**
- **Evidence:** `msettings-transcription-light`: toolbar "Meeting settings" cap 24 px, then the pane's "Transcription" cap 24 px, then the group "After the meeting" 21 px. The other pages have only the toolbar title.
- **Holds down:** #5, #39.
- **Fix:** show the category in the toolbar title ("Transcription"; the sidebar already shows that it sits under Meeting settings) and drop the in-pane H1. That also gives about 60 pt back to content. Alternatively, keep the H1 and set the toolbar to the parent at 13 pt muted as a breadcrumb.
- **File:** `pages/meetings/settings.rs`, `ui/main_window.rs` (toolbar title).

**5. The same model shows two sizes.**
- **Evidence:**
  - Qwen3 4B Instruct: 2.6 GB in `msettings-ai-local-*`, 2.5 GB in `main-models-wide-*` and `onboarding-setup-meetings-*`.
  - Qwen3 8B: 5.2 GB in the AI pane, 5.0 GB in Models.
  - This is the 1000 vs 1024 split (backlog I4).
- **Holds down:** #33, #10.
- **Fix:** one byte formatter everywhere, decimal GB to match Finder (`format_bytes`). Route `format_model_size` through it.
- **File:** `kit.rs` / wherever `format_model_size` lives.

**6. The light-theme button hover reads as pressed, and pressed is still unproven.**
- **Evidence:** `hover-button-light`. The outline button goes from the pane tone (`#fafaf9`) to a solid `#c5c5c1` fill, 1.6:1, which is the same tone as its border, so the button looks depressed or disabled. The dark hover (`#343434` → `#474747`) is fine. No `pressed-*` snapshot exists.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover = `tone.secondary` (about `#ededeb`, about 1.07:1 against the pane) in light.
  - Reserve `#c5c5c1` for pressed.
  - Add `pressed-button` and `pressed-list-row` harness snapshots.
- **File:** `ui/design.rs`, `kit.rs` (button styles), snapshot harness.

**7. Post Process at minimum size still mixes inline and wrapped rows.**
- **Evidence:** `main-postprocessing-min-dark`. Provider is inline. API key ("API key", a short label) and Model wrap their wide fields to a second line, although they stay right-aligned.
- **Holds down:** #12, #34.
- **Fix:** let the API-key field and the Model-name field shrink to the 200 pt column before wrapping (`min_w(200)` with `flex_1`). The label "API key" needs about 60 pt, so all three rows fit inline at 720 pt.
- **File:** `pages/post_processing.rs`, `kit.rs` (`SettingRow`).

**8. Microcopy remainders.**
- **Evidence:**
  - Picker and setup: "Fast and accurate. Supports 25 European languages." and "Very fast, English only." beside " · "-joined siblings (`onboarding-picker-*`, `onboarding-setup-meetings-*`).
  - In Models a capital follows the dot ("Fast and accurate · Supports 25 …").
  - At 720 pt a separator is left at the end of a line ("… Streaming ·" / "Q8_0 quantization", `main-models-min-light`).
- **Holds down:** #32.
- **Fix:**
  - Use the Models strings in the picker: "Fast and accurate · 25 European languages" and "Very fast · English only".
  - Bind " · " to the following fragment with a non-breaking space, so it wraps with that fragment.
- **Files:** `assets/locales/en/translation.json`, `kit::model_description`.

**9. The local AI pane says "on this Mac, offline" twice.**
- **Evidence:** `msettings-ai-local-*`: "Summaries are made by Qwen3 4B Instruct · This Mac · offline", then "Runs offline on this Mac. Choose a downloaded model."
- **Holds down:** #30.
- **Fix:** keep the status line and reduce the helper to "Choose a downloaded model."
- **File:** `pages/meetings/ai_settings.rs`, translation.

**10. Two delete treatments in one pane.**
- **Evidence:** `msettings-ai-local-*` has a labeled "Delete" button for a downloaded model, while `msettings-ai-*` has a bare 14 pt trash on the saved gpt-4.1 row.
- **Comment:** they do different things (delete files vs remove from the list), which argues for different words, not different forms.
- **Holds down:** #10, #14.
- **Fix:** a 24 pt plain icon button with a tooltip for both, or labeled buttons for both: "Delete" (files) and "Remove" (from the list).
- **File:** `pages/meetings/ai_settings.rs`.

**11. Harness: `meetings-settings-*` shows a hover fill on the parent row.**
- **Evidence:** "Meeting settings" carries `#e1e1e0` / `#2c2c2c`, the hover tone, while "Meeting notes" is selected and ringed. Read cold, that looks like two selections.
- **Holds down:** nothing scored.
- **Fix:** move the pointer off the sidebar before capture.
- **File:** snapshot harness.

**12. The startup skeleton sidebar is offset.**
- **Evidence:** `startup-app-min-light`: names at x 84 and the People glyph at x 82, where the loaded sidebar has x 88 and x 78. The sidebar shifts on load (the missing Post Process and Debug are a functional finding).
- **Holds down:** #11.
- **Fix:** build the skeleton from the same `render_sidebar` paddings.
- **File:** `ui/onboarding/startup.rs`.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 17 | States | A calmer light hover; `pressed-*` snapshots (Problem 6). |
| 33 | Numeric formatting | One byte formatter, so a model has one size everywhere (Problem 5). |
| 42 | Truncation | People subtitles fit or truncate only the email (Problem 1). |

To get from 9 to 10:

- #21: Problems 2 and 3.
- #5: Problem 4.
- #12 and #34: Problem 7.
- #32: Problem 8.
- #30: Problem 9.
- #10: Problems 5 and 10.

---

## 5. Functional findings (not visual)

1. "Generate Summary" is still the yellow primary while a meeting records (`meetings-recording-wide-light`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Every older fixture meeting starts at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-*`, `history-selected-min-*`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." with "Improve Transcriptions" listed (`main-postprocessing-light`). Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-min-dark`). Backlog #5.
7. Onboarding Permissions makes "Continue Without Accessibility" the default (`onboarding-permissions-dark`). Backlog #6.
8. The startup skeleton sidebar omits Post Process and Debug (`startup-app-min-*`).
9. The 1000 / 1024 byte split behind Problem 5 (backlog I4). The visual fix is the formatter; the data is unchanged.
10. Meeting settings › Setup shows a "Set Up…" toolbar button while Setup is the open category (`msettings-setup-*`). Decide whether it starts a guided flow or should be hidden there. Hiding it removes an entry point, so it is out of scope for this pass.
11. Out-of-scope IA, noted once: voice enrollment has two entry points (People › You and Meeting settings › You › Open in People), and API keys have two homes (Post Process and AI summaries, "shared with Post Process").
