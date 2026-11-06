# Guidy UI review, round 10: reviewer `opus`

## Evidence

I opened 52 PNGs with the image tool, plus 2x crops of the Storage bar and the Models summaries section. 47 are round-10 captures and 5 are round-9 captures for comparison. They cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (light, dark-min), `focus-1-light`, `hover-sidebar-light`, `hover-button-light` (About), Advanced (`advanced-limit-confirm-dark`), Post Process (min dark, `postprocessing-discard-light`), Models (wide dark, full light), Usage, Storage (dark; light crop for both rounds), Debug.
- **History:** default.
- **Meetings:** list/detail (both themes), detail-min, `meetings-find-wide-dark`, `meetings-recording-wide-light`.
- **People:** default, min, `mpeople-person-light`.
- **Meeting settings:** shell (round 10 and round 9), full-height, Setup (dark, min light), You, Recording, Transcription, Speakers, Linear, Integrations, AI (cloud min and wide, local default in both rounds, local min).
- **Onboarding and startup:** picker, setup with Apple Intelligence, cloud min, `startup-app-light`, `startup-app-min-dark`, `startup-error-dark`.
- **HUDs:** overlay, widget and expanded widget.

### Diff against round 9

I diffed every PNG against round 9 with a pure-Python PNG decoder, split at the sidebar edge (x 400 px). 60 of 242 files are byte-identical, including every onboarding, startup and HUD capture. Of the 182 that changed:

- **Sidebar only (almost all):** in the collapsed state, rows y 310–508 px changed, which is the Meeting settings row moving clusters. In the expanded state, y 296–951 px changed.
- **Content changes:**
  - `msettings-ai-local*`: model names, descriptions and the Apple Intelligence helper.
  - `main-storage-light`, `main-storage-min-light`: the Guidy segment, `#5c3f00` → `#523800`. This change is not in the log.
  - The two alert captures moved again (timing).

### Calibration note

The `round-9/` directory was regenerated at 05:05, one minute after my round-9 report was written (05:04). Its `startup-app-*` files now show the round-9 metrics fix: a 52 px pitch and 68 px cluster breaks. So the skeleton metrics were fixed between rounds. Round 10's `startup-app-*` is byte-identical to that re-capture, and that is the problem (Regression 1).

### Scope rules applied as before

- The status bar, the stock 1 pt focus edges, the `kit::confirm` Return default and the fixed 200 pt pop-up column are not counted.
- The fixture's Debug and Post Process rows are discounted.
- The new position of Meeting settings is the recorded round-10 decision. I score its execution, not the choice.

## The round-10 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| Meeting settings closes the settings cluster, categories one level under it, two levels at most | **Holds** | `main-general-*`: Models / Advanced / Post Process / Meeting settings at y 338 / 390 / 442 / 494 px. Expanded (`msettings-*`), the categories sit at one indent, y 546–928 px. People stays under Meetings. The ↑/↓ order cannot be checked from stills. **New defect:** the two kinds of child row now share a depth but not a geometry (Regression 2). |
| AI pane sizes from bytes, decimal units; fixture uses the catalogue's MiB values, names and descriptions | **Holds for sizes and names, partly for descriptions** | 2.5 GB and "Download 5.0 GB" (`msettings-ai-local-*`) match Models. "Qwen3 8B" now matches Models. The descriptions now use the catalogue words but with ". " where Models uses " · ". The 8B description repeats its own badge (Problem 4). |
| Unavailable Apple Intelligence: helper starts with the instruction | **Holds** | "Turn it on in System Settings > Apple Intelligence & Siri." (`msettings-ai-local-light`, `-min-dark`) |
| Startup sidebar placeholder uses the real sidebar's metrics (no jump) | **Fails** | The metrics match (52 px pitch, 68 px breaks). But `startup-app-*` still draws Meeting settings indented under Meetings, at y 322 px with its label at x 146 px. The loaded sidebar has it top-level at the end of the settings cluster, with its label at x 94 px. So rows still jump on load (Regression 1). |

### My round-9 problems

| Round-9 problem | Status |
|---|---|
| 1. Skeleton uses stale metrics | **Metrics fixed** (in the round-9 re-capture). **The order is now stale instead** (Regression 1). |
| 2. One model, two names and descriptions | **Names fixed.** Descriptions now share their words but not their punctuation ("Fast, good summaries. Works on…" vs "Fast, good summaries · Works on…"). |
| 3. Light accent tones | **Partly.** Storage now uses `#523800`, the slider and progress tone, so the light theme has four accent tones instead of five. Fill against remainder is unchanged: slider 2.69:1 (`#523800` / `#837e6e`), storage 1.81:1 (it was 1.61). |
| 4. Light button hover reads as pressed | **Not fixed** (`hover-button-light`: still `#c5c5c1`). |
| 5. Cloud and local AI status in opposite orders | **Not fixed** (`msettings-ai-min-light`: "Anthropic key needed · Claude Sonn…"). |
| 6. Post Process wraps two rows at minimum size | **Not fixed** (`main-postprocessing-min-dark`). |
| 7. Microcopy remainders | **Not fixed** (`onboarding-picker-light`: periods next to " · "; `main-models-min-dark` content unchanged). |
| 8. Two delete treatments | **Not fixed** (labeled Delete in `msettings-ai-local-*`, bare trash icon in `msettings-ai-*`). |
| 9. People emails truncate at 900 pt | **Not fixed** (`meetings-people-light`). |
| 10. Set Up… chip at 12 pt | **Not fixed** (`msettings-setup-*`). |
| 11. Category titles ambiguous on their own | **Not fixed** (minor). |
| 12. Harness pointer hover on "Meeting settings" | **Not fixed.** In `meetings-settings-light` the parent row's fill is `#e1e1e0`, identical to the hover fill in `hover-sidebar-light`. |
| 13. Alert captures not position-stable | **Not fixed.** |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 9.

| # | Parameter | Guidy R10 | Guidy R9 | Codex | Claude | Reason for the round-10 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono only for IDs, paths and the Usage model column (`main-usage-dark`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | 9 | 9 | 8 | 8 | Unchanged; all measured text passes. Fill pairs are under #41. |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | Unchanged. The local status leads with the model (`msettings-ai-local-*`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | 26 / 24 pt rows and 8 pt cluster gaps hold in both states. |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | Storage joined the slider/progress tone, so four light accent tones instead of five. |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | Model names now agree. Remaining: two child-row geometries, two delete forms, description punctuation, opposite status orders. |
| 11 | Alignment and grid | **8** | 9 | 8 | 9 | Same-depth children no longer share a leading edge. People's label is at x 146 px, the categories' labels at x 133 px (`msettings-you-light`, `meetings-settings-dark`); at `-min` it is 112 vs 100 px (`msettings-setup-min-light`). Glyphs are 16 vs about 12.5 pt. |
| 12 | Vertical rhythm | 9 | 9 | 8 | 9 | One sidebar rhythm in both states. Post Process still wraps at minimum size. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Better: Meetings + People reads as the library, and Models / Advanced / Post Process / Meeting settings as preferences (`main-meetings-dark`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Category glyphs are smaller than the People glyph at the same depth (counted under #11). |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | Unchanged (chip at 12 pt is a nit). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | Unchanged. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still `#c5c5c1` (`hover-button-light`); there are no `pressed-*` captures. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Unchanged. |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-1-light`, `meetings-settings-dark`, `meetings-people-light`). |
| 20 | Target size | 9 | 9 | 8 | 8 | Unchanged. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Two levels at most now, and the title always names the category. Held at 9 by the load-time reorder (Regression 1). |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Grouping by intent is clearer. The rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Meeting settings pages keep "Team standup · 15:00" and Record while sibling settings pages have neither. That is now more visible, but changing it would remove an entry point (functional 8). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | 9 | 9 | 8 | 9 | Unchanged. |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged. |
| 28 | Loading and progress | 8 | 8 | 8 (k) | 8 (k) | The skeleton matches the metrics but not the order. On load, Meeting settings drops 60 pt and outdents 26 pt, and Models and Advanced rise 26 pt (Regression 1). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | The Apple Intelligence helper now gives the action plainly (`msettings-ai-local-*`); `startup-error-dark` is unchanged. |
| 30 | Data density | 9 | 9 | 8 | 8 | New: the Qwen3 8B badge "Recommended for 16 GB+ RAM" sits beside "Recommended with 16 GB of memory or more." in the description. |
| 31 | Scannability | 9 | 9 | 8 | 8 | Unchanged. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | The helper is better. The ". " vs " · " split between surfaces remains. |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 7 | One size per model in the fixture; the log says production now matches. |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 7 (k) | 8 (k) | The 8B description wraps cleanly beside its button (`msettings-ai-local-min-dark`). Post Process still wraps. |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged (`msettings-ai-wide-light`, `meetings-find-wide-dark`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Unchanged. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | Unchanged: the heavy hover and the near-black brown fills remain. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | A System Settings-style source list with one disclosure level. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | Slider 2.69:1 (the thumb marks the value); storage 1.81:1 (with a legend). |
| 42 | Truncation and text accommodation | 9 | 9 | 8 | 6 | Unchanged: "Claude Sonn…" at 720 pt, emails at 900 pt. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged in content. |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Byte-identical to round 9. |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Expanding Meeting settings moves only the rows below it. |

| Column | Mean of 45 |
|---|---|
| Guidy round 10 | **8.9** (402 / 45 = 8.93) |
| Guidy round 9 | **9.0** (403 / 45 = 8.96) |
| Codex | **7.8** (351 / 45) |
| Claude | **8.0** (361 / 45) |

**Holistic "would the lead designer ship this" (Guidy round 10): 9.**

- The navigation move is the right call, and it is executed cleanly in the loaded window.
- The two new defects are both in the sidebar:
  - the startup placeholder still lists the old order;
  - same-depth child rows have two different insets.
- Each is a small, local fix. Neither blocks use.

---

## 2. Regressions since round 9

1. **The startup placeholder lists Meeting settings in its old place.**
   - **Evidence:** `startup-app-light` and `startup-app-min-dark` draw General / History / Meetings / People / *Meeting settings* (indented; y 322 px, label x 146 px), then Models (390) and Advanced (442), then Usage, Storage and About. The loaded sidebar for the same set of sections (`main-general-light`, without the fixture's Post Process and Debug) is Models 338, Advanced 390, Meeting settings 442 (label x 94 px), with Usage, Storage and About in the same place.
   - **Effect:** when the app becomes ready, Meeting settings moves 60 pt down and 26 pt left, and Models and Advanced move 26 pt up.
   - **Cause:** `sidebar_skeleton()` in `ui/onboarding/startup.rs` still pushes `MeetingsTab::Settings` as an indented child right after `Section::Meetings`. Its doc comment still says "Meetings' two views indented under it".
   - **Holds down:** #28, #21, #10.
2. **Same-depth child rows have two geometries.**
   - **Evidence:**

     | Child row | Row height | Glyph | Glyph x | Label x |
     |---|---|---|---|---|
     | People (under Meetings) | 26 pt | 16 pt | 95 px | 146 px |
     | Meeting settings categories | 24 pt | about 12.5 pt | 93 px | 133 px |

     The two label edges sit 6.5 pt apart in the same column (`msettings-you-light`, `msettings-speakers-dark`, `meetings-settings-full-light`). At `-min` they are 112 vs 100 px (`msettings-setup-min-light`).
   - **Why it is new:** in round 9 the difference followed depth (level 2 vs level 3). Now both are level 2, so the ragged edge reads as a mistake.
   - **Holds down:** #11, #10, #14.

Nothing else got worse. The light storage segment moved to `#523800`, a small unlogged improvement.

---

## 3. Remaining problems, ranked

None blocks a 9 on its own. Problems 1 and 2 are why #11 and #28 sit at 8.

**1. The startup placeholder uses the old sidebar order.**
- **Evidence:** Regression 1.
- **Holds down:** #28, #21, #10.
- **Fix:**
  - Build the placeholder from the same row list `render_sidebar` uses: Meetings → People only, then Meeting settings as a top-level row closing the settings cluster.
  - Better, have `render_sidebar` and `sidebar_skeleton` share one `fn sidebar_rows(cx) -> Vec<(Section, depth)>`, so the order can't drift again.
  - Update the doc comment.
- **File:** `ui/onboarding/startup.rs`, `ui/main_window.rs`.

**2. One child-row style.**
- **Evidence:** Regression 2.
- **Holds down:** #11, #10, #14.
- **Fix:** give every depth-1 row the same glyph slot and label x. Either:
  - (a) draw People with the category metrics: 24 pt row, 14 pt glyph in an 18 pt slot, label at x 66.5 pt (133 px); or
  - (b) move the categories' label to People's x 73 pt (146 px) and their glyph to 16 pt.

  Option (a) is denser and keeps a 9-row category list compact. In both cases, keep the compact-width inset equal for both kinds of child row (label at 50 pt).
- **File:** `ui/main_window.rs` (sidebar child-row builder), `ui/design.rs` (one `SIDEBAR_CHILD_INSET`).

**3. Light accent tones and fill/remainder contrast.**
- **Evidence:** four light accent tones: `#ffc72c` (primary), `#a87500` (switches), `#8a6000` (marks and links), `#523800` (sliders, progress, storage). Slider fill against remainder is 2.69:1 (`msettings-speakers-light`, `main-debug-light`). Storage Guidy against Other is 1.81:1 (`main-storage-light`).
- **Holds down:** #9, #41, #37.
- **Fix:**
  - Use one light non-text accent, `#8a6000`, for switches, sliders, progress and storage.
  - Lighten remainders to about `#c9c9c5` (4.7:1 against `#8a6000`), with a `#8f8f8a` hairline for the track edge.
  - Make storage "Other" `#c9c9c5` in light and `#5a5a5a` in dark.
- **File:** `assets/themes/guidy.json`, `ui/design.rs`, `pages/storage.rs`.

**4. Model descriptions: punctuation, and a fact said twice.**
- **Evidence:**
  - AI pane: "Fast, good summaries. Works on any Apple silicon Mac." Models: "Fast, good summaries · Works on any Apple silicon Mac" (`msettings-ai-local-light` vs `main-models-wide-dark`).
  - The 8B row in the AI pane shows the badge "Recommended for 16 GB+ RAM" and the description "…Recommended with 16 GB of memory or more." Models has no badge, so it doesn't repeat itself (`main-models-full-light`).
- **Holds down:** #30, #32, #10.
- **Fix:**
  - Render the catalogue fragments through the same `kit::model_description` joiner (" · ", no final period) on both surfaces.
  - In the AI pane, drop the description fragment that the badge already states, or drop the badge.
- **File:** `pages/meetings/ai_settings.rs`, `kit.rs`.

**5. The light button hover reads as pressed.**
- **Evidence:** `hover-button-light`, `#c5c5c1`.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover about `#ededeb`; keep `#c5c5c1` for pressed.
  - Add `pressed-*` captures.
- **File:** `ui/design.rs`, `kit.rs`, snapshot harness.

**6. The cloud and local AI status lines use opposite orders.**
- **Evidence:** "Anthropic key needed · Claude Sonn…" (`msettings-ai-min-light`) vs "Qwen3 4B Instruct · This Mac · offline".
- **Holds down:** #10, #31, #42.
- **Fix:** condition first only when action is needed. When the line has to shrink, truncate the provider tail before the model name.
- **File:** `pages/meetings/ai_settings.rs`.

**7. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark` (API key and Model).
- **Holds down:** #12, #34.
- **Fix:** keep the control inline (`min_w(200)`, `flex_1`) until the label column reaches 180 pt.
- **File:** `pages/post_processing.rs`.

**8. Two delete treatments in one pane.**
- **Evidence:** labeled Delete (`msettings-ai-local-*`) vs a bare trash glyph (`msettings-ai-*`).
- **Holds down:** #10, #14.
- **Fix:** use one form: a 24 pt icon button with a tooltip, or a labeled "Remove", in both places.
- **File:** `pages/meetings/ai_settings.rs`.

**9. Onboarding picker punctuation.**
- **Evidence:** `onboarding-picker-light`: "Fast and accurate. Supports 25 European languages." sits next to "Fast and fairly accurate · 20 languages". "Streaming ·" still ends a line at `main-models-min-dark`.
- **Holds down:** #32.
- **Fix:**
  - Use the " · " join everywhere.
  - Bind each " · " to the following fragment with a non-breaking space.
- **File:** `translation.json`, `kit::model_description`.

**10. People emails truncate at 900 pt.**
- **Evidence:** `meetings-people-light`, three of four rows.
- **Holds down:** #42.
- **Fix:** widen the list to 260 pt between 900 and 1280 pt, or put the email on its own line.
- **File:** `pages/meetings/people.rs`.

**11. The Set Up… chip is 12 pt in a 13 pt toolbar.**
- **Evidence:** `msettings-setup-*`.
- **Holds down:** #15 (nit).
- **Fix:** use 13 pt.
- **File:** `pages/meetings/recorder.rs`.

**12. Minor copy items.**
- "Turn it on in System Settings > Apple Intelligence & Siri.": macOS writes paths with "›" (or "→"), not ">". Use "System Settings › Apple Intelligence & Siri". **File:** `translation.json`.
- Category titles such as "You" and "Recording" are ambiguous on their own. Consider a muted "Meeting settings ›" prefix in the toolbar at wide sizes. **File:** `ui/main_window.rs`.

**13. Harness: the pointer hover on "Meeting settings" and unstable alert captures.**
- **Evidence:** `meetings-settings-*` (fill `#e1e1e0` = hover); `advanced-limit-confirm-*` and `postprocessing-discard-*` moved again.
- **Fix:**
  - Park the pointer outside the window before capture.
  - Capture alerts after their transition settles.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 11 | Alignment and grid | One depth-1 row geometry for People and the categories (Problem 2). |
| 17 | Interactive states | A calmer light hover and `pressed-*` captures (Problem 5). |
| 28 | Loading and progress | Placeholder built from the real row list and order (Problem 1). |

To get from 9 to 10:

- #9, #37 and #41: Problem 3.
- #10: Problems 2, 4, 6 and 8.
- #12 and #34: Problem 7.
- #21: Problem 1.
- #30 and #32: Problems 4, 9 and 12.
- #42: Problems 6 and 10.
- #15: Problem 11.
- #14: Problem 2 (glyph size) and Problem 8.

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-light`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-dark`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`meetings-detail-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-light`). Backlog #10.
5. Post Process shows "Select a prompt to edit it." while a prompt is listed. Backlog idea #5.
6. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-min-dark`). Backlog #5.
7. The startup placeholder omits Post Process and Debug when they are enabled (`startup-app-*`).
8. Meeting settings pages carry the recorder toolbar ("Team standup · 15:00", Record). Now that Meeting settings sits among the preference pages, it is the only settings page with a Record command. Removing it removes an entry point, which is out of scope.
9. Meeting settings › Setup still shows the "Set Up…" chip while Setup is open (out of scope). The fixture's Setup badge reads "1" elsewhere and "3" on Setup itself.
10. The ↑/↓ order change claimed in the log can't be verified from stills. A `focus-*` capture after one ↓ from Post Process would show it.
11. Out-of-scope IA, noted once:
    - voice enrollment has two entry points (Meeting settings › You and People);
    - API keys have two homes (Post Process and AI summaries).
