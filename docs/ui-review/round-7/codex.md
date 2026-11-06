# Round 7 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would sign off on this UI-only refresh. Category targets are back to 24 pt, compact cloud status no longer starts a wrapped line with a separator, and the local AI helper no longer repeats offline. The new sidebar fade washes out enabled navigation labels when the list overflows. That small contrast regression offsets the target-size improvement in the arithmetic mean. None of the remaining findings blocks the holistic ship score of 9.

## Evidence and scoring method

I read the complete council brief, including every scope clarification, the agreed rubric, review task and round log. I checked my round-6 findings in detail and consulted my own reports from rounds 0–5 for calibration. I did not read any `opus.md` file or another reviewer's report.

I inspected overview sheets covering all 121 round-7 stems in both themes, 242 PNGs. This includes every main section at default and minimum size, all supplied wide variants, every meeting-settings category, People and compact person detail, meeting notes/transcript/search/playback/processing/recording/empty states, onboarding and startup variants, errors, both alerts and both HUDs. All twelve focus states and three hover states were inspected in both themes. Larger content crops covered General, Advanced, Debug, Models, Post Process, Usage, Storage, meeting notes/transcript and both alerts. Original-pixel crops checked the bottom fade and its text.

Direct round-6 comparisons covered `meetings-settings`, `msettings-ai-min` and `msettings-ai-local-min`, both themes. Baseline comparisons covered `main-models-min`, `meetings-detail` and `onboarding-setup-cloud-min`, both themes. I inspected the three supplied reference PNGs. Codex and Claude retain my round-6 reference scores; `(k)` marks product-knowledge estimates beyond their single screenshot. T3 Code, Xcode and System Settings inform calibration without extra score columns. These are judgments of the supplied references, not audits of the latest live apps.

The fixture has Debug and Post Process enabled, so it has two more global rows than a typical sidebar. I do not treat their absence below the default fold as proof that a typical user's default window overflows. The remaining navigation deduction concerns the three visible levels and compact-height presentation. There is no normal-configuration capture here to independently verify the log's claim that every destination fits at 900×600.

Limited read-only source checks covered sidebar bounds and fade, AI status layout and English copy, pop-up width/reset layout, model fixture/catalogue descriptions, and Reduce Motion paths. Only this report was written in the repository. Temporary image sheets were made outside it. No source was edited, no Cargo command or app was run, and no build, preview channel or live database was touched. The round log's test claims were not independently rerun. I applied the humanizer skill to the report wording.

Snapshot stems below mean `target/ui-review/round-7/<stem>-{light,dark}.png` unless a theme or earlier round is named. Captures are 2×. Missing traffic lights, blank chrome in isolated fixtures, opaque HUD backgrounds and unused tall-capture tails are harness conventions.

Interaction scores use stills and the identified source checks. They do not certify runtime hit testing, VoiceOver, focus restoration, scroll behaviour or animation cost. The brief accepts stock controls' opaque 1 pt focus edge. Hiding the status bar/update entry point, exposing Debug-only settings, consolidating stores, changing commands/defaults/validation, or changing Return's alert action remains outside scope. Return confirms Delete/Discard and Esc cancels; the filled destructive default is therefore accurate.

## 1. Scores

| # | Parameter | Guidy round 7 | Guidy round 6 | Codex | Claude | Reason for Guidy round 7 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | System controls, helper copy and reading text keep distinct, readable roles (`main-general`, `meetings-detail`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Semibold marks headings and speakers without turning paragraphs bold (`meetings-transcript`, `main-general`, `msettings-ai-local`). |
| 3 | Color contrast | 8 | 9 | 7 | 7 | The new bottom fade reduces contrast on enabled navigation labels in overflow states (`meetings-settings`, `msettings-ai-min`, both themes); see regression 1. |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | The selected settings category leads the toolbar; each content region keeps a clear primary object (`msettings-recording`, `meetings-detail`, `main-models`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Toolbar titles, group headings, rows and helpers retain the established size steps (`main-general`, `main-usage`, `msettings-general`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, lists and alerts share one language, with an intentional HUD exception (`msettings-linear`, `main-general`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | The denser sidebar retains readable spacing and page insets remain consistent (`main-general-min`, `meetings-settings`, `msettings-ai-wide`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Wide forms keep their measure, but short choices still occupy fixed 200 pt triggers (`main-general`, `main-advanced-full`, `main-debug-min`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral surfaces and selection leave accent and semantic colours with limited roles (`main-models`, `msettings-ai`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Resets, selections, badges and transports retain shared treatments (`main-debug`, `meetings-playing`, `history-selected-min`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Labels and trailing controls align; People names and subtitles retain their avatar slot (`main-general`, `meetings-people`, `main-postprocessing`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Group rhythm remains orderly; the ready local status still has a minor compact wrap (`main-general`, `msettings-ai-local-min`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, recording preferences and model choices remain clearly grouped (`msettings-setup`, `msettings-recording`, `msettings-ai-local`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Line icons and filled transport glyphs remain consistent and readable (`meetings-playing`, `meetings-people`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | The 26 pt control height is consistent; short pop-ups still look like wide text fields (`main-general-min`, `main-advanced-full`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Named content and recovery commands remain clear; the edge fade is a local signifier flaw (`history-selected-min`, `storage-error`, `meetings-settings`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Hover, focus, selected, busy and disabled states remain distinct away from the overflow fade (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection stays distinct from focus outlines and catalogue membership checks (`focus-1`, `meetings-tab-focus`, `msettings-ai`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | The custom sidebar/tab rings remain visible; stock controls retain the accepted opaque edge (`focus-1` through `focus-12`, `meetings-tab-focus`). |
| 20 | Target size | 9 | 8 | 7 (k) | 7 (k) | Category click elements are restored to 24 pt; normal sidebar rows are 26 pt (`meetings-settings`, `msettings-ai-min`; source). |
| 21 | Navigation and wayfinding | 8 | 8 | 8 | 8 | The three-level settings tree persists; compact windows still bury global destinations (`meetings-settings`, `msettings-setup-min`, `msettings-ai-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Destination names and settings groups retain clear intent; presentation crowding is counted under 21 (`main-general`, `msettings-transcription`, `msettings-general`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Current-category titles and leading Back remain consistent across settings and compact detail (`msettings-recording`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, source list, panels and alerts keep distinct tonal layers (`main-general`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines separate groups and panes without boxing reading paragraphs (`main-general`, `main-postprocessing`, `meetings-detail-wide`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, panels, alerts and HUD capsules keep the established radius family (`msettings-ai-local`, `advanced-limit-confirm`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Empty libraries and missing summaries each offer a clear next action (`meetings-empty`, `meetings-recording-wide`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named startup work and determinate progress remain clear and stable (`startup-app`, `startup-new`, `meetings-processing`, `main-models-min`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Failure causes, affected paths and recovery actions remain visible (`main-models-full`, `storage-error`, `usage-error`, `msettings-ai-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Two complete cloud choices fit at minimum size; local helper copy is shorter (`msettings-ai-min`, `msettings-ai-local-min`, `main-models-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Names, status, prices and quantities retain stable scan positions; voiceprint counts survive email truncation (`main-models`, `main-usage`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Offline is now stated once in local AI; the residual Default repetition comes from fixture copy (`msettings-ai-local`, `main-usage`; source). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Tabular amounts, units and price basis remain clear (`main-usage-min`, `msettings-ai-min`, `main-debug-full`, `meetings-played`). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Compact cloud status keeps its condition intact and two catalogue rows fit; compact detail uses clear Back navigation (`msettings-ai-min`, `history-selected-min`, `meetings-detail-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms retain their cap while meeting panes grow into useful reading regions (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Dark surfaces retain depth and restrained bright marks; the boundary fade needs local tuning (`main-general-dark`, `meetings-processing-dark`, `meetings-settings-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Light controls and focus stay crisp; overflow text is the isolated exception scored under contrast (`focus-1-light`, `focus-11-light`, `meetings-settings-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Grouped settings, source lists, leading Back and truthful alert defaults retain a Mac character (`main-general`, `history-selected-min`, `postprocessing-discard`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Reading content dominates its region and settings avoid duplicate page titles (`meetings-detail-wide`, `msettings-recording`, `history-selected-min`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral chips and simple reading panes remain restrained; stale fixture copy adds minor repetition (`msettings-ai-local`, `meetings-detail`, `history-selected`). |
| 41 | Non-text indicators and use of colour | 9 | 9 | 8 (k) | 8 (k) | Icons, state words and membership checks reinforce colour; progress and track marks remain visible (`msettings-setup`, `msettings-ai`, `meetings-processing`). |
| 42 | Truncation and text accommodation | 9 | 9 | 7 (k) | 7 (k) | Compact cloud status truncates secondary model text while Your models preserves its full name; primary catalogue names fit (`msettings-ai-min`, `main-models-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Alerts state consequences and distinguish Cancel from the actual destructive Return default (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Text, elapsed time and pause/stop marks remain glanceable (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Static progress and source-backed Reduce Motion handling remain; stills cannot establish transition quality (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |

| | **Mean of 45** | **8.9** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums in column order: **401/45**, **401/45**, **349/45**, **358/45**. Guidy's unrounded mean is 8.9111 in both rounds. Parameter 20 rises from 8 to 9; parameter 3 falls from 9 to 8. Round 7 has 41 scores of 9 and four scores of 8. The holistic ship score is **9/10**.

### Check of my round-6 items and the round log

1. **Expanded settings navigation: partly improved.** Normal rows are 26 pt and category rows are 24 pt. The overflow fade is visible. The fixture still places Storage at the default viewport edge and Models at the minimum viewport edge (`meetings-settings`, `msettings-ai-min`, `msettings-setup-min`, both themes). Debug/Post Process inflate this fixture; the three-level presentation and compact crowding remain independently visible. Selection reveal exists in source, so offscreen does not mean unreachable.
2. **Category target size: fixed.** `render_sidebar` uses `dp(24.)`, `.h(height)` and the click listener on the same category element. Ordinary rows use the 26 pt token. This closes the round-6 target-size regression without shrinking the 13 pt labels.
3. **Short pop-up widths: unchanged.** Auto, Default and Off still occupy 200 pt triggers (`main-general`, `main-advanced-full`, `main-debug-min`). `Dropdown::render` still fixes trigger width. The nearby `trailing` comment still incorrectly says short pop-ups hug their value.
4. **Local AI repetition: offline fixed; remaining Default repetition is fixture copy.** The helper now says Choose a downloaded model. Offline appears only in the status. Download size is still stated once before installation. The repeated The default for summaries sentence remains visible, but `ui_snapshots.rs::local_models` injects it. The production catalogue description in `guidy-core/src/llm/local_models.rs` does not repeat Default. Correcting that stale evidence should not require another production copy change.
5. **Compact AI status: cloud fixed, ready local unchanged.** `msettings-ai-min` keeps Anthropic key needed intact and truncates the secondary model name; the full name remains directly below in Your models. Two complete catalogue rows still fit. `msettings-ai-local-min` still wraps Summaries are made by above its model. Source confirms only the missing-key branch gained a single-line/truncation rule; the ready branch still uses `flex_wrap`. The log's one-line claim is therefore true for the missing-key state, not every AI status.

People voiceprint counts, current-category toolbar titles and compact cloud onboarding remain intact. Light progress pixels still contain `#5c3f00` fill and `#8f8f8a` remainder, a **2.984:1** pair. That is unchanged from round 6, not a new regression or a strict 3:1 pass. I retain the prior practical judgment because percentage text also communicates completion. The new contrast deduction concerns enabled sidebar text under the fade.

## 2. Regressions since round 6

### 1. The overflow fade makes enabled destinations look disabled

In round 6, Storage at the bottom of `meetings-settings` has normal text contrast. In round 7, the new 28 pt overlay fades through the label and icon before the viewport ends. In both themes Storage looks unavailable, although it remains a navigation destination. At minimum size the same treatment affects Models (`msettings-ai-min`, `msettings-ai-local-min`, `msettings-setup-min`).

Original PNG crops show the gradient running through the glyph strokes, with increasingly weak lower strokes. For example, at y=1130 in the default `meetings-settings` capture, the strongest text-region samples are about `#9a9a99` on the light `#f1f1ef` sidebar and `#6f6f6f` on dark `#202020`. These are rendered-pixel observations of a gradient, not a formal whole-label accessibility measurement. They establish that enabled label text is being washed toward the background. Source places the absolute gradient over the scrolling list.

The fade helps signal continuation, but should not borrow the visual language of disabled commands. Parameter 3 falls from 9 to 8. This is conditional on sidebar overflow and does not block the holistic ship score. Keep the cue in a reserved footer strip or beside the list, with full-contrast text on every completely visible row.

### 2. Compact cloud status loses the complete secondary model name

Round 6 shows the full model on a second line; round 7 ellipsizes it beside Anthropic key needed (`msettings-ai-min`, both themes). This is an acceptable tradeoff: the actionable condition survives, the full model is immediately below, and the catalogue gains space. Parameter 42 stays 9. I do not request restoring the separator-led wrap.

No other material regression was established in the inspected evidence. Ready-local status wrapping is carried forward, not new. The restored target size and shorter local helper are improvements.

## 3. Remaining problems, ranked

These are polish. **None blocks the holistic ship score of 9.** Findings 1–3 block individual parameters from reaching 9. Paths are relative to `crates/guidy-app/src/ui/` unless stated otherwise. Suggested fixes preserve existing commands, destinations, audiences, stores and behaviour.

### 1. The scroll cue reduces enabled-label contrast

- **What:** A 28 pt fade crosses fully visible destination text and resembles a disabled state.
- **Evidence:** `meetings-settings` and `msettings-ai` at default size; `msettings-ai-min` and `msettings-setup-min` at minimum size, both themes. Direct round-6 comparison and original-pixel crops confirm the change.
- **Parameters:** **3 is 8, blocking its individual 9.** It also affects affordance/state polish under 16 and 17, which remain 9; I do not deduct the same issue across several columns.
- **Fix:** place an 8–12 pt continuation shadow/cue in a reserved bottom strip, outside row text, or use a narrow visible scroll thumb. Keep complete row labels/icons at normal opacity. Retain 24 pt category targets and selection reveal.
- **Likely source:** `main_window.rs::render_sidebar`, especially the absolute 28 pt gradient overlay.

### 2. Meeting settings still presents three navigation levels

- **What:** Meetings → Meeting settings → nine categories creates a long indented tree. At 720×480 the categories dominate the source list and global preferences are mostly below the fold. The parent still lacks a clear disclosure signifier.
- **Evidence:** `meetings-settings`, `msettings-ai-min`, `msettings-setup-min`, both themes. The two fixture-only extra rows explain some default-height overflow, but not the extra hierarchy level or compact category run.
- **Parameters:** **21 is 8, blocking its individual 9.** Parameter 22 stays 9 because destination meanings and audiences are clear.
- **Fix:** present at most two levels at once. At compact heights, a labeled category pop-up can expose the same nine existing choices while leaving global destinations stable. At larger heights, flatten the category presentation under clear Meeting settings context. Reuse current actions and keep the selected category named in the toolbar; do not remove destinations or shrink targets/type.
- **Likely source:** `main_window.rs::nav_items`, `render_sidebar`, title-bar presentation; `pages/meetings/settings.rs`.

### 3. Short choices still occupy full-width pop-up triggers

- **What:** Auto, Default and Off carry a 200 pt field outline despite their short labels. The column aligns well, but the empty trigger area gives simple choices too much weight.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `main-debug-min`, both themes; fixed width in `kit.rs::Dropdown::render`.
- **Parameters:** **8 and 15 are 8, blocking their individual 9s.** Alignment remains 9.
- **Fix:** preserve the 200 pt layout slot and 26 pt height, but size the visible trigger to label + caret + padding, roughly 72–120 pt for short choices and up to 200 pt for long ones. Keep its leading edge adjacent to the fixed reset slot, with spare column space after the trigger; do not recreate round 3's large reset-to-control gap. Menu width can be independent. Update `trailing`'s comment to describe the actual rule.
- **Likely source:** `kit.rs::Dropdown::render`, `kit.rs::trailing`, `shortcut_input.rs::reset_slot`; the column token can stay 200 pt.

### 4. Ready local AI status still wraps awkwardly at minimum width

- **What:** Summaries are made by occupies the first line; the model and This Mac · offline occupy the second, beside the mode switch. The cloud-specific fix did not cover this branch.
- **Evidence:** `msettings-ai-local-min`, both themes, unchanged in direct round-6 comparison; the ready branch still uses `flex_wrap`. Default-size local status fits.
- **Parameters:** 12, 31, 32 and 34 remain 9. **Not a 9 blocker.**
- **Fix:** use concise ready-state copy such as Qwen3 4B Instruct · offline at compact width, retaining the success glyph and full model name in the local list. Alternatively give ready status a deliberate full-width line. Keep 13 pt text, the existing switch, and the restored two-choice cloud viewport.
- **Likely source:** `pages/meetings/ai_settings.rs::status_line`, `header`; `assets/locales/en/translation.json`.

### 5. The local-model fixture still repeats Default

- **What:** The Default chip is followed by The default for summaries in the model description. This is stale screenshot copy, not a demonstrated production catalogue defect.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, both themes. `ui_snapshots.rs::local_models` supplies the sentence. Production catalogue text is Fast, good summaries. Works on any Apple silicon Mac.
- **Parameters:** 30, 32 and 40 remain 9. **Not a 9 blocker.**
- **Fix:** have the fixture reuse the real catalogue description, or shorten its test description to Fast and capable. Keep the useful Default and Downloaded indicators and installed size. Do not alter production descriptions to fix fixture-only repetition.
- **Likely source:** `crates/guidy-app/src/ui_snapshots.rs::local_models`; production reference is `crates/guidy-core/src/llm/local_models.rs::CATALOG`.

## 4. What would raise each parameter still below 9

- **3. Color contrast:** keep enabled sidebar labels/icons at normal contrast; move the overflow cue out of text. Separately, a tiny darkening of the light progress fill would clear the exact 3:1 pair threshold noted above.
- **8. Width and layout measure:** retain useful form/list caps and size short pop-up triggers to their content within the shared column.
- **15. Control styling and sizing:** combine content-sized pop-ups with 26 pt height and adjacent, predictable reset/accessory slots.
- **21. Navigation and wayfinding:** show at most two levels at once and keep compact global navigation discoverable while settings categories are available.

The other 41 parameters score 9 on the available evidence. Findings 4–5 improve polish within those scores.

## 5. Functional findings, separate from visual scores

These are carried-forward verification questions, not runtime failures established by these PNGs. None reduces the visual scores.

1. **Shortcut semantics and audience:** verify precedence when Dictation key and Transcribe shortcut are both Right Option (`main-general`). Debug-only timing settings retain their current audience; exposing them in General is outside this pass.
2. **Credentials and validation:** verify sharing between Post Process, summaries and speaker services, Finish with an empty cloud key, and recovery from a credential-less saved default (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`). Store consolidation, validation and default changes remain functional work.
3. **Navigation and accessibility:** exercise arrow navigation, category expansion, deep links, selection reveal and scroll reset, including reaching destinations below the fade. Verify the overlay does not intercept pointer/wheel input, and check focus/selection restoration for compact History/People, accessible names and HUD/scrubber keyboard access. The source shows selection-reveal intent; stills do not prove the flow.
4. **Audio and retention:** verify redo/retranscription when History says Recording not found and after retained meeting audio expires (`history-selected`, `msettings-general`). Command availability and retention defaults are outside styling scope.
5. **Live summaries:** establish which recording interval Generate Summary uses and what later generation replaces or preserves (`meetings-recording-wide`).
6. **Destruction and merging:** Return-confirm and Esc-cancel are settled by the brief. Runtime checks still need to establish focus restoration and person-merge consequences/undo (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
7. **Model-size evidence:** Models shows 2.5/5.0 GB while the local AI fixture shows 2.6/5.2 GB (`main-models-full`, `msettings-ai-local`). The fixture injects 2,500/5,000 MiB and the local view formats bytes; production catalogue entries carry actual byte counts. Align fixture inputs and verify displayed unit conventions before treating this as a production data bug.
