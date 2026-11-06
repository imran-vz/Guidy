# Round 6 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would sign off on the UI-only refresh. Compact cloud AI now shows two complete model choices, the settings toolbar names the current category, and light progress is easier to read. The expanded sidebar remains too deep, and reducing its category rows to 22 pt introduces a small target-size regression. None of the remaining findings blocks the holistic ship score of 9.

## Evidence and scoring method

I read the full council brief, including every scope clarification, the rubric, review task and round log. I checked my round-5 report in detail and used my own earlier reports for calibration. I did not read any `opus.md` file or any other reviewer's report.

I inspected overviews of all 121 round-6 snapshot stems in both themes, 242 PNGs. Coverage includes every main section at default and minimum sizes, every supplied wide variant, all meeting-settings categories, meeting notes/transcript/search/playback/recording/processing, People and compact person detail, onboarding/startup variants, errors, both alerts and both HUDs. I inspected the twelve focus states and three hover states in both themes. Larger crops checked compact cloud/local AI, People subtitles, alert content, and lower content in General, Advanced, Models, Post Process, Usage and meeting reading views. Original-pixel colour sampling was separate from scaled overview inspection.

Direct round-5 comparisons covered default Meeting settings, minimum cloud AI and minimum local AI in both themes. Baseline comparisons covered minimum Models, meeting detail and minimum cloud onboarding in both themes. I inspected all three supplied reference PNGs. Codex and Claude keep my previous reference scores; `(k)` marks estimates based on product knowledge beyond their single screenshot. T3 Code, Xcode and System Settings inform the judgment without additional score columns. These reference scores are calibration against the supplied evidence, not a fresh audit of the live products.

Read-only source checks covered sidebar hierarchy, row height, spacing and selection reveal; shared pop-up width and reset layout; AI header/local rows and English copy; progress tokens and Reduce Motion handling. Only this report was written in the repository. No source was changed, no Cargo command or live app was run, and no production, daily-driver build or preview channel was touched. Temporary image sheets were made outside the repository. I applied the humanizer skill to the report wording.

Snapshot stems refer to `target/ui-review/round-6/<stem>-{light,dark}.png` unless a theme or earlier round is named. Captures are 2×. Missing traffic lights, blank chrome in isolated fixtures, opaque HUD backgrounds and unused tall-capture space are known harness conventions.

Interaction scores are judgments from stills and limited source checks. They do not establish runtime hit testing, VoiceOver, keyboard reachability, scroll/focus restoration or animation cost. The brief accepts stock controls' opaque 1 pt focus edge; I do not penalize it. The idle status bar and Check for updates entry point remain outside hiding/removal scope. Return confirms Delete/Discard and Esc cancels, so the filled destructive alert button accurately represents its default.

## 1. Scores

| # | Parameter | Guidy round 6 | Guidy round 5 | Codex | Claude | Reason for Guidy round 6 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | System UI, reading text and monospace identifiers retain clear roles (`main-general`, `meetings-detail`, `main-usage`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Semibold identifies titles and speakers; helpers and paragraphs remain regular (`main-general`, `meetings-transcript`, `msettings-ai-local`). |
| 3 | Color contrast | 9 | 9 | 7 | 7 | Light progress is substantially clearer; necessary tracks and control edges remain visible in both themes (`meetings-processing`, `main-models-min`, `main-debug`). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | The toolbar now names the selected settings category, removing the competing pane title (`meetings-settings`, `msettings-ai`, `msettings-recording`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Page, group, row and helper roles remain distinct without a second 17 pt settings title (`main-general`, `meetings-settings`, `msettings-ai-local`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Shared forms, lists and alerts retain one language, with a deliberate HUD exception (`main-general`, `msettings-linear`, `postprocessing-discard`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Page insets and group gaps remain consistent; compact category indentation is clearer (`main-general`, `msettings-ai-min`, `msettings-recording`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Capped wide forms work, but short values still occupy the same 200 pt pop-up trigger (`main-general`, `main-advanced-full`, `main-models-wide`, `msettings-ai-wide`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and chips leave accent and semantic colours with limited roles (`msettings-ai`, `msettings-ai-local`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Selection, resets, status marks and transport controls remain consistent across sections (`main-debug`, `meetings-playing`, `history-selected-min`, `msettings-ai-local`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Form controls and reset columns stay aligned; People names and subtitles share a stable leading edge (`main-general`, `main-postprocessing`, `meetings-people`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Removing the pane title improves the opening rhythm; the compact status line has a minor wrap tradeoff (`meetings-settings`, `msettings-ai-min`, `msettings-ai-local-min`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, credentials, model choices and recording preferences retain understandable groups (`msettings-setup`, `msettings-ai`, `msettings-recording`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | One line-icon family and consistent transport symbols support named commands (`meetings-playing`, `meetings-people`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Regular controls remain consistent, but short choices still have text-field-sized triggers (`main-general-min`, `main-advanced-full`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Named recovery and content commands remain clear; compact model choices are now complete (`storage-error`, `history-selected-min`, `msettings-ai-min`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Hover, selection, disabled controls, keyboard outlines and busy work remain distinct in the supplied states (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection stays separate from focus outlines and catalogue membership checks (`focus-1`, `meetings-tab-focus`, `msettings-ai`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Sidebar and custom tab outlines remain clear; stock controls retain the accepted opaque edge (`focus-1` through `focus-12`, `meetings-tab-focus`). |
| 20 | Target size | 8 | 9 | 7 (k) | 7 (k) | Expanded settings category hit rows shrink from 24 to 22 pt; other inspected auxiliary controls retain their bounded targets (`meetings-settings`, `msettings-ai-min`; source). |
| 21 | Navigation and wayfinding | 8 | 8 | 8 | 8 | Tighter spacing recovers Storage at default height, but three levels still hide Debug/About and most global sections at minimum height (`meetings-settings`, `msettings-ai-min`, `msettings-setup-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Destinations and settings still communicate their existing intent and audience; sidebar crowding is counted under navigation (`main-general`, `msettings-general`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | The category title now leads the toolbar, and compact detail views keep a consistent leading Back (`meetings-settings`, `msettings-ai`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar, panels and alerts stay tonally distinct in both themes (`main-general`, `meetings-settings`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines divide groups and panes without boxing reading paragraphs (`main-general`, `meetings-detail-wide`, `main-postprocessing`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, chips, panels, alerts and HUD capsules retain a small consistent radius family (`msettings-ai-local`, `advanced-limit-confirm`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | The empty meeting library points to Record; missing summaries have a local generation action (`meetings-empty`, `meetings-recording-wide`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named work, percentages and static tracks remain clear; the darker light fill improves progress reading (`startup-app`, `startup-new`, `meetings-processing`, `main-models-min`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Inline download reasons, the Storage path and missing-key status remain visible with recovery actions (`main-models-full`, `storage-error`, `msettings-ai-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Compact cloud AI now shows two complete choices, and local downloads state their size once (`msettings-ai-min`, `msettings-ai-local-min`, `main-models-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | People subtitles now lead with voiceprint counts; model states, quantities and actions keep stable positions (`meetings-people`, `main-models`, `main-usage`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Category names and Usage headings are clear; local AI still repeats offline/default wording as minor copy polish (`msettings-general`, `main-usage`, `msettings-ai-local`). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Tabular amounts, explicit price basis and unwrapped slider units remain readable (`main-usage-min`, `msettings-ai-min`, `main-debug-full`, `meetings-played`). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Two full cloud choices now fit at 720×480, alongside readable forms and compact detail navigation (`msettings-ai-min`, `onboarding-setup-cloud-min`, `main-general-min`, `meetings-detail-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Wide forms retain their measure, while meeting panes grow into useful reading regions (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted neutral surfaces, restrained selection and readable status marks remain designed for dark mode (`main-general-dark`, `meetings-processing-dark`, `msettings-ai-local-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Light controls and focus remain crisp; progress fill is substantially more distinct from its remainder (`focus-1-light`, `focus-11-light`, `meetings-processing-light`, `main-models-min-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Grouped settings, source lists, leading Back and truthful alert defaults retain a Mac character (`main-general`, `history-selected-min`, `advanced-limit-confirm`, `postprocessing-discard`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Removing the repeated settings heading starts content higher; meeting notes remain the primary reading object (`meetings-settings`, `msettings-ai-min`, `meetings-detail-wide`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral chips and simple reading panes remain restrained; repeated local-model wording is the main small excess (`msettings-ai-local`, `meetings-detail`, `history-selected`). |
| 41 | Non-text indicators and use of colour | 9 | 9 | 8 (k) | 8 (k) | Icons and words reinforce state; opaque tracks and the stronger light progress fill preserve readable indicators (`msettings-setup`, `msettings-ai-local`, `meetings-processing`, `main-models-min`). |
| 42 | Truncation and text accommodation | 9 | 9 | 7 (k) | 7 (k) | People counts survive email truncation, model text wraps, and compact pushed details preserve primary content (`meetings-people`, `main-models-min`, `history-selected-min`, `meetings-people-person-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Alerts state consequences and distinguish Cancel from the actual destructive Return default (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Upright text, elapsed time and distinct pause/stop marks remain glanceable (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Static determinate progress and source-backed Reduce Motion handling remain intact; stills cannot certify transitions (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.9** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums in column order: **401/45**, **402/45**, **349/45**, **358/45**. The unrounded Guidy mean moves from 8.9333 to 8.9111; both round to 8.9. Round 6 has 41 scores of 9 and four scores of 8. The holistic ship score is **9/10**. This tiny mean decrease records the new target-size tradeoff; the compact catalogue and title hierarchy have improved within their existing scores of 9.

### Check of my round-5 items and the round log

1. **Meeting-settings navigation: partly improved, still open.** Tighter cluster gaps and category rows recover Storage at the bottom of default `meetings-settings` and `msettings-ai`. Debug and About remain below the fold. At minimum height, `msettings-ai-min` and `msettings-setup-min` show Models near the bottom, with the other global sections offscreen. The hierarchy still has three visible levels. Compact indentation is clearer. Both themes agree. The log/source comment that Storage, Debug and About all stay in view at default height is not supported by these captures.
2. **Short pop-up widths: unchanged.** Auto, Default and Off still occupy 200 pt triggers (`main-general`, `main-advanced-full`, `main-debug-min`). `kit.rs::Dropdown::render` still fixes the trigger width. The `trailing` comment still says short pop-ups hug their value, which does not describe the rendered rule.
3. **Compact cloud catalogue: fixed.** `msettings-ai-min` now exposes both Opus and Sonnet, including IDs, context and prices, above the body boundary in both themes. Removing the in-pane title saves enough height even though the compact status now wraps. This closes the round-5 viewport finding.
4. **Local AI repetition: partly fixed.** The undownloaded model's 5.2 GB appears only in its Download button. The downloaded model retains its useful disk-size line. However, the status still ends in This Mac · offline, and the helper says Choose a downloaded model. It runs on this Mac, offline. Default still appears in both a chip and The default for summaries (`msettings-ai-local`, `msettings-ai-local-min`, both themes). The log's claim that offline is said once is not borne out by the screenshots or `meetingsAi.llm.localDescription`.
5. **Light progress separation: materially improved.** Original `meetings-processing-light` pixels confirm `#5c3f00` fill and `#8f8f8a` remainder. Their contrast is **2.984:1**, up from **1.72:1** in round 5. Against the `#fafaf8` page the new fill is **9.28:1**. Dark remains `#ffc72c` against `#707070`, **3.17:1**. The light pair rounds to 3.0, but is fractionally below an exact 3:1 threshold. I close the practical polish finding: the percentage states completion separately and the necessary remainder boundary remains visible. I am not claiming the fill/remainder pair strictly passes 3:1. A tiny further darkening would clear that numeric target if desired, without changing global accent text.

The People change is also confirmed: `meetings-people` now puts 3 voiceprints before the ellipsized email in both themes. Names remain intact. The settings toolbar now names the selected category and the repeated 17 pt pane title is gone (`meetings-settings`, `msettings-recording`, `msettings-ai`).

## 2. Regressions since round 5

### Settings-category targets shrink below the rubric's 24 pt bar

Round-5 category rows were 24 pt. Round-6 `main_window.rs::render_sidebar` assigns 22 pt to every level-2 category, with the existing 2 pt gap between rows. `meetings-settings`, `msettings-recording` and `msettings-ai-min` show the denser result in both themes. The click listener belongs to that 22 pt element; the source does not show a larger target wrapper.

This makes the category rows less forgiving to point at and falls short of this council's explicit ≥24×24 pt target. Parameter 20 drops from 9 to 8. The spacing may qualify for an accessibility spacing exception; this is not a claim of a proven WCAG failure. Restoring 24 pt rows would meet the rubric directly. Use a navigation presentation change to recover height instead of further compressing targets.

### Compact AI status loses its full-width line

In round 5, `msettings-ai-min` showed Anthropic key needed · Claude Sonnet 4.5 on one line. Round 6 places it beside the provider switch, so the model moves to a second line beginning with a separator. The ready local status similarly splits Summaries are made by from the model (`msettings-ai-local-min`). Both themes show this.

This is a small typography tradeoff, not a compact-layout blocker: all words remain readable, controls do not collide, and the cloud catalogue gains a complete second row. Parameters 12 and 34 remain 9. Item 5 below gives a compact copy/layout adjustment that preserves the height gain.

No other material regression was established in the inspected evidence. The sidebar's persistent depth/crowding is an unresolved round-5 finding, rather than a new regression. People metadata priority, settings title hierarchy and light progress have improved.

## 3. Remaining problems, ranked

These five are polish. **None blocks the holistic ship score of 9.** Items 1–3 block individual rubric parameters from reaching 9. Paths are relative to `crates/guidy-app/src/ui/` unless stated otherwise. Fixes preserve current destinations, commands, audiences, stores, defaults and validation.

### 1. Expanded Meeting settings still crowds the global sidebar

- **What:** Meetings → Meeting settings → category remains three visible levels in one narrow scrolling source list. Debug/About are still below the default viewport; most global preferences are below the minimum viewport. The parent also has no conventional disclosure signifier.
- **Evidence:** `meetings-settings`, `msettings-ai`, `msettings-ai-min`, `msettings-setup-min`, both themes; compare round-5 counterparts. Source retains level-2 categories and one overall sidebar scroll region. Selection reveal exists, so offscreen does not mean unreachable.
- **Parameters:** **21 is 8, blocking its individual 9.** Parameter 22 stays 9 because the destination meanings remain clear. Do not count this again as an out-of-scope IA/store problem.
- **Fix:** keep the wider form, but show no more than two navigation levels at once. A contextual settings sidebar can reuse the existing parent navigation action as a named return to the full destination list, show Meeting settings as context, and list its nine categories at 24–28 pt. Keep the selected category visible. Reuse existing navigation actions and retain all entry points. Do not shrink the type, hide the status bar or compress the rows further.
- **Likely source:** `main_window.rs::nav_items`, `render_sidebar`, `render_titlebar`; `pages/meetings/settings.rs`.

### 2. Category hit rows are now only 22 pt high

- **What:** The density adjustment solves some crowding by making category targets smaller than the agreed size. A 2 pt visual gap keeps rows distinct, but does not make each click element 24 pt high.
- **Evidence:** `meetings-settings`, `msettings-recording`, `msettings-ai-min`, both themes; `main_window.rs::render_sidebar` uses `dp(22.)`, `.h(height)` and `.on_click` on the same row.
- **Parameters:** **20 is 8, blocking its individual 9.** This is the only new score deduction. It is a rubric finding, not a screenshot-based assertion that all accessibility exceptions fail.
- **Fix:** restore a minimum 24 pt clickable category row, preferably 26–28 pt if item 1 removes the need for compression. Retain the 2 pt focus outline inside the row and the current readable 13 pt labels. Update the nearby comment that still describes a 24 pt row.
- **Likely source:** `main_window.rs::render_sidebar`.

### 3. Short choices still have 200 pt pop-up triggers

- **What:** Auto, Default and Off carry the same field-sized visual weight as long choices. The control column aligns well, but the empty trigger area dominates short enumerations.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `main-debug-min`, both themes. `kit.rs::Dropdown::render` fixes the width rather than measuring the current label.
- **Parameters:** **8 and 15 are 8, blocking their individual 9s.** Alignment remains 9.
- **Fix:** keep the 200 pt layout column and 26 pt height; right-align a label/caret/padding-sized trigger within it. Preserve the stable leading reset slot and explicit preview/refresh accessory bounds. Menu width can accommodate long options independently. Keep a visible relationship between reset and field, and update `trailing`'s comment to match the final rule.
- **Likely source:** `kit.rs::Dropdown::render`, `kit.rs::trailing`, `shortcut_input.rs::reset_slot`; `design.rs::CONTROL_COLUMN_W` can remain 200 pt.

### 4. Local AI still repeats offline and default information

- **What:** The status and next helper both say the model runs offline on this Mac. The first model's Default chip is immediately repeated in its description. The duplicate download-size line is fixed.
- **Evidence:** `msettings-ai-local` and `msettings-ai-local-min`, both themes; original-size crops confirm the wording. The helper in `assets/locales/en/translation.json` still contains the second offline statement.
- **Parameters:** 30, 32 and 40 remain 9. This is refinement within those scores, **not a 9 blocker**.
- **Fix:** keep one offline statement with the active-model status. Shorten the helper to Choose a downloaded model. Keep the Default chip and shorten its model description to Fast and capable. Preserve installed disk size and the undownloaded model's size-bearing Download action.
- **Likely source:** `pages/meetings/ai_settings.rs::status_line`, local `Render` branch and `LocalModelsView::local_rows`; `assets/locales/en/translation.json`; model description source feeding the local rows.

### 5. Compact AI status wraps into an awkward sentence fragment

- **What:** Cloud status starts its second line with · Claude Sonnet 4.5; local status isolates Summaries are made by above the model. The switch remains tidy, but takes the width formerly available to the complete status.
- **Evidence:** `msettings-ai-min` and `msettings-ai-local-min`, both themes, directly compared with round 5. Default/wide variants do not show the same wrap.
- **Parameters:** 12, 31, 32 and 34 remain 9. This is small copy/layout polish, **not a 9 blocker**, and must not undo the restored two-row cloud catalogue.
- **Fix:** use concise compact status text, such as Anthropic key needed and Qwen3 4B Instruct · offline, with existing model identity still shown in Your models/local choices. Avoid starting a wrapped line with a separator. Alternatively give the status a full-width row at narrow widths only if the two complete cloud choices still fit. Keep 13 pt status text and the existing mode control.
- **Likely source:** `pages/meetings/ai_settings.rs::header`, `status_line`; compact wording in `assets/locales/en/translation.json`.

## 4. What would raise each parameter still below 9

- **8. Width and layout measure:** retain form caps and shared control columns; size short pop-up triggers to their contents.
- **15. Control styling and sizing:** combine content-sized triggers with consistent reset/accessory slots and the existing 26 pt control height.
- **20. Target size:** restore category click elements to at least 24 pt high; recover space through navigation presentation.
- **21. Navigation and wayfinding:** present at most two levels at once, with clear settings context and a named route back to the full destination list.

The other 41 parameters score 9 on the available evidence. Items 4–5 improve polish within those scores.

## 5. Functional findings, separate from visual scores

These are carried-forward verification questions, not new runtime failures proved by screenshots. They do not reduce visual scores.

1. **Shortcut semantics and audience:** verify precedence between Dictation key and Transcribe shortcut, both set to Right Option. Debug-only timing settings retain their existing audience; exposing them in General is outside this pass (`main-general`, `main-debug`).
2. **Credential ownership and setup validation:** verify sharing between Post Process, meeting summaries and speaker services; Finish with an empty cloud key; and recovery when a saved default lacks credentials. Store consolidation, validation and changing defaults remain functional work (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`).
3. **Navigation, focus and accessible controls:** exercise sidebar arrow navigation, selection reveal across category expansion/collapse, deep links and category scroll reset. Check compact AI scrolling, History/People return-selection, HUD/scrubber keyboard access and accessible names. Source and stills do not prove these flows (`meetings-settings`, `msettings-ai-min`, `history-selected-min`, `meetings-people-person-min`, `recording-widget-expanded`).
4. **Audio availability and retention:** verify redo/retranscription when History says Recording not found and after retained meeting audio expires. Command availability and retention defaults are outside styling scope (`history-selected`, `msettings-general`).
5. **Live summaries:** establish which recording interval Generate Summary uses and how later generation preserves or replaces it (`meetings-recording-wide`).
6. **Destruction and merging:** Return-confirm and Esc-cancel are settled by the brief. Runtime checks still need to establish focus restoration and person-merge consequences/undo (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
7. **Model-size data:** Models still shows 2.5/5.0 GB while local AI shows 2.6/5.2 GB for the Qwen models. Verify byte basis and shared catalogue/fixture data before altering labels. This may be fixture drift (`main-models-full`, `msettings-ai-local`).
