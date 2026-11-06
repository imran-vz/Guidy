# Round 8 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would sign off on the UI-only refresh. Removing the sidebar fade is a clear improvement, and tighter spacing brings more destinations into view without shrinking targets. Two small regressions remain: compact local AI status cuts off its model name, and installed local-model sizes now use a different unit formatter from Models and downloads. The updated warning colour is better, but its actual tinted button background leaves it just below the text-contrast target.

## Evidence and scoring method

I read the complete council brief, including all scope sections, the agreed rubric, review task and round log. I checked my round-7 report in detail and consulted the opening judgments of my own rounds 0–6 for calibration. I did not read any `opus.md` file or another reviewer's report.

I inspected overview sheets covering all 121 round-8 stems in both themes, 242 PNGs. Coverage includes every main section at default and minimum size, every supplied wide variant, all meeting-settings categories, People and compact person detail, meeting notes/transcript/search/playback/processing/recording/empty states, onboarding and startup variants, errors, both alerts and both HUDs. All twelve focus states and three hover states were inspected in both themes. Larger content crops covered General, Advanced, Debug, Models, Storage, Usage, Post Process, meeting notes/transcript and both alerts. Original-pixel crops checked the sidebar, compact local status, warning chip, permission statuses, progress and HUDs.

Direct round-7 comparisons covered `meetings-settings`, `msettings-ai-min` and `msettings-ai-local-min`, both themes. A pixel comparison found 205 of 242 captures identical to round 7; 37 differ. Baseline comparisons covered `main-models-min`, `meetings-detail` and `onboarding-setup-cloud-min`, both themes. I inspected the three supplied reference PNGs. Codex and Claude retain my earlier reference scores; `(k)` marks product-knowledge estimates beyond the single reference screenshot. T3 Code, Xcode and System Settings inform calibration without extra columns. These are judgments of the supplied evidence, not audits of the latest live apps.

The fixture includes Debug and Post Process, adding two global rows. Its partly clipped About row at 900×600 therefore does not establish that a typical sidebar still fails to fit. Source geometry supports the new normal-configuration fit claim, as explained below. The remaining navigation deduction concerns three visible levels and compact-height presentation.

Read-only source checks covered sidebar geometry and selection reveal, status layout, the two model-size formatters and their data path, fixture/catalogue copy, theme colours, the setup chip and Reduce Motion handling. I wrote only this report in the repository. Temporary image sheets were made outside it. No source was edited, no Cargo command or app was run, and no build, preview channel or live database was touched. The round log's test claims were not rerun. I applied the humanizer skill to the report wording.

Snapshot stems below mean `target/ui-review/round-8/<stem>-{light,dark}.png` unless a theme or earlier round is named. Captures are 2×. Missing traffic lights, blank chrome in isolated fixtures, opaque HUD backgrounds and unused tall-capture tails are harness conventions.

Interaction scores use stills and limited source evidence. They do not certify runtime hit testing, VoiceOver, focus restoration, scrolling or animation cost. I accept the brief's stock 1 pt focus edge and the actual Return-confirm/Esc-cancel alert behavior. Changing audiences, commands, entry points, stores, defaults, validation or the alert's Return action remains outside scope.

## 1. Scores

| # | Parameter | Guidy round 8 | Guidy round 7 | Codex | Claude | Reason for Guidy round 8 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | System controls, helpers and reading text retain readable roles (`main-general`, `meetings-detail`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Semibold identifies headings, models and speakers without making paragraphs heavy (`meetings-transcript`, `main-models`, `msettings-ai-local`). |
| 3 | Color contrast | 8 | 8 | 7 | 7 | The fade is removed and status inks improve, but Set Up text is 4.456:1 on its tint and light progress is 2.984:1 against its remainder (`msettings-setup-light`, `meetings-processing-light`). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | Each region retains a clear object and action, including current-category titles (`msettings-recording`, `meetings-detail`, `main-models`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Toolbar titles, group headings, body and helpers keep distinct roles (`main-general`, `main-usage`, `msettings-general`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, lists, alerts and the intentional HUD exception remain coherent (`main-general`, `msettings-linear`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Tighter expanded navigation keeps readable row heights and consistent content insets (`meetings-settings`, `main-general-min`, `msettings-ai-wide`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Form and reading caps work, but Auto, Default and Off still occupy 200 pt triggers (`main-general`, `main-advanced-full`, `main-debug-min`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral surfaces and selection reserve semantic colours for status (`main-models`, `msettings-setup`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Selections, resets, shortcuts and transports retain shared treatments; unit formatting is scored under 33 (`main-debug`, `meetings-playing`, `history-selected-min`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Labels, trailing controls and avatar/name slots align predictably (`main-general`, `main-postprocessing`, `meetings-people`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Groups and rows retain orderly rhythm; the ready status is now one line (`main-general`, `msettings-ai-local-min`, `msettings-recording`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, recording preferences and model choices have clear grouping (`msettings-setup`, `msettings-recording`, `msettings-ai-local`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Line icons and transport glyphs remain consistent and recognizable (`meetings-playing`, `meetings-people`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Controls keep a 26 pt height, but short pop-ups still read as oversized fields (`main-general-min`, `main-advanced-full`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Named commands and recovery actions remain clear; enabled navigation no longer resembles disabled text (`history-selected-min`, `storage-error`, `meetings-settings`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Hover, focus, selected, disabled and busy stills remain distinct (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and separate focus outlines remain legible in both themes (`focus-1`, `meetings-tab-focus`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Custom outlines remain clear and stock controls retain the accepted opaque focus edge (`focus-1` through `focus-12`, `meetings-tab-focus`; brief). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Expanded categories remain 24 pt and ordinary navigation rows 26 pt (`meetings-settings`, `msettings-ai-min`; source). |
| 21 | Navigation and wayfinding | 8 | 8 | 8 | 8 | The tighter source list improves fit, but Meetings, Meeting settings and its nine categories still show three levels (`meetings-settings`, `msettings-setup-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Destination meanings and intent-based groups remain clear; presentation depth is counted under 21 (`main-general`, `msettings-general`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Current-category titles, restrained commands and leading Back stay consistent (`msettings-recording`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, source list, panels and alerts retain distinct tonal levels (`main-general`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines separate controls and panes while reading content stays open (`main-general`, `main-postprocessing`, `meetings-detail-wide`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | The established control, panel, alert and capsule radius family remains consistent (`msettings-ai-local`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Empty libraries and missing summaries give clear next steps (`meetings-empty`, `meetings-recording-wide`, `storage-error`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Startup names its work, determinate progress has percentages, and layout stays stable (`startup-app`, `startup-new`, `meetings-processing`, `main-models-min`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Failure conditions and recovery actions remain understandable (`storage-error`, `usage-error`, `main-models-full`, `msettings-ai-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Two cloud catalogue rows still fit at minimum size; local status saves a line (`msettings-ai-min`, `msettings-ai-local-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Names, states, prices and amounts keep stable scan positions; counts survive secondary email truncation (`main-models`, `main-usage`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Labels remain plain and consistent; repeated Default prose is still fixture-only (`main-usage`, `msettings-ai-local`; source). |
| 33 | Numeric and tabular formatting | 8 | 9 | 8 | 8 | Installed local AI sizes now use binary-derived GB while Models and downloads use decimal GB (`msettings-ai-local`, `main-models-full`; source). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Compact detail/back navigation and catalogue access remain usable; local status clipping is counted under 42 (`meetings-detail-min`, `history-selected-min`, `msettings-ai-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms keep their cap while meeting panes use the extra width (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Dark surfaces retain depth, restrained bright marks and full-contrast sidebar text (`main-general-dark`, `meetings-settings-dark`, `meetings-processing-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Light surfaces and focus remain crisp; the small contrast misses are counted under 3 (`focus-1-light`, `main-general-light`, `msettings-setup-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source lists, grouped forms, leading Back and accurate destructive defaults retain a Mac character (`main-general`, `history-selected-min`, `postprocessing-discard`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Meeting text dominates its region and settings avoid duplicate page titles (`meetings-detail-wide`, `msettings-recording`, `history-selected-min`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral chips and open reading panes stay restrained (`meetings-detail`, `history-selected`, `msettings-ai-local`). |
| 41 | Non-text indicators and use of colour | 9 | 9 | 8 (k) | 8 (k) | Status words, glyphs, checks and percentages reinforce colour; the borderline progress pair is counted under 3 (`msettings-setup`, `msettings-ai`, `meetings-processing`). |
| 42 | Truncation and text accommodation | 8 | 9 | 7 (k) | 7 (k) | The compact ready-local model name is cut off beneath the mode switch without an ellipsis (`msettings-ai-local-min`, both themes). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Alerts state consequences and distinguish Cancel from the actual destructive Return default (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Text, time and transport marks remain readable at a glance (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Static progress and source-backed Reduce Motion paths remain; stills cannot certify transition quality or GPU cost (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.9** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums in column order: **399/45**, **401/45**, **349/45**, **358/45**. Guidy's unrounded mean changes from 8.9111 to 8.8667. Round 8 has 39 scores of 9 and six scores of 8. Parameters 33 and 42 fall from 9 to 8. Contrast stays 8 for the smaller residual failures below, despite the clear improvement to sidebar and status text. The holistic ship score remains **9/10**. No remaining issue blocks that holistic score.

### Check of my round-7 items and the round log

1. **Sidebar fade: fixed.** Round-7 Storage is washed out; round 8 shows normal label/icon contrast in the same region (`meetings-settings`, both themes). The absolute gradient is gone from `main_window.rs::render_sidebar`.
2. **Expanded navigation: fit improved, depth unchanged.** Expanded mode now has no row gaps and 6 pt cluster gaps. Categories remain 24 pt; ordinary rows remain 26 pt. In this inflated fixture, Storage and Debug now fit at default height and About meets the lower edge. Removing the two fixture-only rows saves 52 pt. For the typical eight global sections plus two Meetings children and nine categories, source geometry gives 10×26 + 9×24 + 3×6 + 4 + 10 = **508 pt**, within the 536 pt body. This supports the round log's normal-fit claim without pretending a normal-configuration capture was supplied. It does not resolve the three-level tree or 720×480 crowding.
3. **Short pop-ups: unchanged.** Auto, Default and Off retain 200 pt triggers (`main-general`, `main-advanced-full`, `main-debug-min`). `Dropdown::render` explicitly sets the fixed width. The `trailing` comment still says short pop-ups hug their value.
4. **Ready local status: wrapping removed, clipping introduced.** At minimum width the mode switch now covers the end of Qwen3 4B Instruct. There is no ellipsis (`msettings-ai-local-min`, both themes). The source makes the heading and model `flex_none`, leaving only the trailing location eligible to truncate. Once that tail has no room, the fixed parts still exceed the status allocation. The log's one-line claim is true; its claim that the model stays whole is not.
5. **Default repetition: unchanged and fixture-only.** `ui_snapshots.rs::local_models` still injects Fast and capable. The default for summaries. The real catalogue says Fast, good summaries. Works on any Apple silicon Mac. Do not change production descriptions to repair stale fixture prose.

The new size claim also needs correction. Installed local rows now call `format_model_size`, which divides MiB by 1024 and labels the result GB. The actual Models LLM row calls decimal `format_bytes(model.size_bytes)`; DownloadControl uses that decimal formatter too. In the captures, installed Qwen3 4B changes **2.6 → 2.4 GB**, while Models remains **2.5 GB** and the undownloaded 8B button remains **5.2 GB** versus Models' **5.0 GB**. Fixture inputs explain some cross-screen differences, but the source now demonstrates a unit-formatting mismatch independent of those inputs. With the production 4B catalogue value, the path predicts **2.3 GB** installed versus **2.5 GB** in Models/downloads. That prediction is calculated from source, not a runtime observation.

Status colours improve substantially, but white is not every actual background. Original PNG pixels in `msettings-setup-light` give:

- Set Up text **#a35400** on tinted **#f0e6db**: **4.456:1**, below 4.5:1. The same text on white is 5.490:1. Its former round-7 text/tint pair is about 3.16:1, so this is an improvement, not a regression.
- Granted text **#23804a** on white: **4.932:1**, a pass. Required Not granted uses #a35400 on white and passes too.
- Light progress fill **#5c3f00** against remainder **#8f8f8a**: **2.984:1**, unchanged (`main-models-min-light`, `meetings-processing-light`). Percentage text independently communicates completion.

These ratios use opaque interior colours, not antialiasing fringes. My previous report treated the progress miss leniently. This round I keep parameter 3 at 8 because the actual warning chip and progress pair still miss the rubric's explicit thresholds. The fade fix is credited; neither ratio is rounded up into a pass.

## 2. Regressions since round 7

### 1. Compact ready-local status cuts off the model beneath the switch

Round 7 wraps the status onto two lines and preserves the full model and offline context. Round 8 keeps one line but visibly cuts the model at roughly Qwen3 4B Instru beneath On this Mac (`msettings-ai-local-min-light`, `msettings-ai-local-min-dark`). The missing tail has no ellipsis. The model remains fully named in the list below, so this is a local polish regression rather than a task blocker. Parameter **42 falls from 9 to 8**; I do not duplicate the deduction under minimum-size reflow.

### 2. Installed local-model sizes switch to an incompatible unit formatter

The same local 4B fixture changes from 2.6 to 2.4 GB without a changed size input (`msettings-ai-local`, `msettings-ai-local-min`, both themes). The installed path now uses binary division while download labels and the Models LLM rows use decimal bytes. The source calculation above establishes the production inconsistency too. Parameter **33 falls from 9 to 8**. This is presentation formatting, not a request to change downloads or model data.

No other material visual regression was established. Tighter sidebar spacing preserves target sizes and readable labels. Removing the fade also removes its scroll cue, but I would not restore that cue over text. Continued minimum-height overflow needs a separate, unobtrusive presentation treatment.

## 3. Remaining problems, ranked

These are polish. **None blocks a holistic ship score of 9.** Findings 1–5 block the identified individual parameters from reaching 9. Suggested fixes preserve existing commands, destinations, audiences, data and behavior.

### 1. Meeting settings still exposes three navigation levels

- **What:** Meetings → Meeting settings → nine categories creates a long indented tree. At 720×480 it displaces most global preferences below the fold. The parent has no clear disclosure signifier. The normal default-height fit improvement closes that part of my earlier finding.
- **Evidence:** `meetings-settings`, `msettings-ai-min`, `msettings-setup-min`, both themes. Debug/Post Process account for two extra fixture rows; they do not account for the third level or the compact category run.
- **Parameters:** **21 = 8; blocks its individual 9.** IA remains 9 because destination meanings and audiences are clear.
- **Fix:** show at most two levels at once. At compact heights, present the same nine category actions in a labeled category pop-up while keeping global destinations stable. At larger heights, flatten categories under clear Meeting settings context. Keep the current-category title, 13 pt labels and ≥24 pt targets. Do not remove destinations.
- **Likely source:** `crates/guidy-app/src/ui/main_window.rs::{nav_items,render_sidebar}` and `ui/pages/meetings/settings.rs`.

### 2. The compact local status overruns its allocated width

- **What:** The fixed introductory phrase and fixed model together are wider than the space before the mode switch. The switch obscures the end of the model.
- **Evidence:** `msettings-ai-local-min`, both themes, compared directly with round 7. Default local status fits.
- **Parameters:** **42 = 8; blocks its individual 9.** Reflow, hierarchy and status messaging retain 9 because the full model is directly below and the rest of the pane remains usable.
- **Fix:** at compact width use concise status such as Qwen3 4B Instruct · offline, keeping the success glyph and 13 pt text. Make the model's container shrinkable and apply an explicit ellipsis if needed. Alternatively give status its own full-width line below the switch. Preserve visible catalogue choices and avoid covering text with a sibling.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::{status_line,header}`; compact copy in `assets/locales/en/translation.json` if needed.

### 3. Short pop-ups still occupy oversized visible triggers

- **What:** Auto, Default and Off have a 200 pt outlined trigger despite their short values. Alignment is orderly, but the empty field gives simple choices undue weight.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `main-debug-min`, both themes; fixed width in `kit.rs::Dropdown::render`.
- **Parameters:** **8 and 15 = 8; blocks both individual 9s.** Alignment stays 9.
- **Fix:** preserve a 200 pt layout slot and 26 pt height, but make short visible triggers roughly 72–120 pt, sized from value + caret + padding, up to 200 pt for long values. Keep the trigger beside the reset slot, with spare column space after it; avoid recreating the earlier reset-to-control gap. Menu width can remain independent. Correct the stale `trailing` comment.
- **Likely source:** `crates/guidy-app/src/ui/kit.rs::{Dropdown::render,trailing}` and `ui/shortcut_input.rs::reset_slot`.

### 4. Installed-model size labels use different units from the other model views

- **What:** A model now appears to shrink after installation because only its installed metadata uses a binary-derived figure labeled GB. The Models LLM row and download action use decimal GB.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, `main-models-full`, both themes; same-input round-7 comparison plus `local_rows`, `ModelsPage::other_row`, `format_model_size` and `format_bytes` source.
- **Parameters:** **33 = 8; blocks its individual 9.** I do not infer changed file contents or penalize IA for this display error.
- **Fix:** use `format_bytes(m.size_mb * 1024 * 1024)` for installed local metadata, matching the existing local download path and Models' decimal convention. For exact cross-screen fixture comparisons, supply equivalent bytes in both fixtures. No store migration or download behavior change is needed.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::LocalModelsView::local_rows`; fixture inputs in `crates/guidy-app/src/ui_snapshots.rs`. Shared formatters are in `guidy-core/src/downloads.rs` and `guidy-app/src/logic/models.rs`.

### 5. Two small contrast misses remain on actual adjacent colours

- **What:** Warning text passes on white but misses on the Set Up chip's tint. Light progress is also just short of 3:1 against its remainder.
- **Evidence:** `msettings-setup-light`, `msettings-setup-min-light`, `main-models-min-light`, `meetings-processing-light`; original-pixel measurements above. Dark counterparts remain readable and do not show these same pairs.
- **Parameters:** **3 = 8; blocks its individual 9.** Parameters 37 and 41 stay 9 to avoid counting the same misses twice. These are small numerical failures, not broad illegibility.
- **Fix:** separate warning text ink from its tinted fill; for example, a darker #8f4700 text ink over the existing tint gives margin while preserving the warmer status colour elsewhere. Verify both rest and hover backgrounds, since `render_setup_chip` deepens its tint on hover. Change only the light progress fill from #5c3f00 to approximately #5b3e00: this gives **3.029:1** against the existing #8f8f8a remainder. Keep percentage labels.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/recorder.rs::render_setup_chip`, `ui/design.rs::status_color`, and light `progress.bar.background` in `assets/themes/guidy.json`.

### 6. Local-model fixture prose still repeats Default

- **What:** The Default chip is followed by The default for summaries. The real catalogue description does not repeat it.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, both themes; `ui_snapshots.rs::local_models` versus the production catalogue.
- **Parameters:** 30, 32 and 40 remain 9. **Not a 9 blocker.** This is stale evidence copy.
- **Fix:** reuse the real catalogue description in the fixture or shorten the injected helper to Fast and capable. Keep Default, Downloaded and the size.
- **Likely source:** `crates/guidy-app/src/ui_snapshots.rs::local_models`; reference catalogue in `crates/guidy-core/src/llm/local_models.rs`.

## 4. What would raise each parameter still below 9

- **3. Color contrast:** check warning text against the chip's actual rest/hover tints and darken the light progress fill enough to exceed 3:1 against its remainder.
- **8. Width and layout measure:** size short visible pop-up triggers to their content within the shared layout column.
- **15. Control styling and sizing:** retain 26 pt height and predictable reset placement while reducing short pop-up outlines to roughly 72–120 pt.
- **21. Navigation and wayfinding:** show at most two navigation levels and keep compact global destinations discoverable while categories remain available.
- **33. Numeric and tabular formatting:** use the same decimal byte formatter for installed local models, downloads and Models LLM rows.
- **42. Truncation and text accommodation:** fit or deliberately ellipsize the ready-local status before the mode switch; never let the switch cover the model name.

The other 39 parameters score 9 on the supplied evidence.

## 5. Functional findings, separate from visual scores

These are carried-forward verification questions, not runtime failures established by these PNGs. None reduces a visual score. The source-established unit formatter issue belongs in the UI findings above, rather than being deferred as a functional data defect.

1. **Shortcut semantics and audience:** verify precedence when Dictation key and Transcribe shortcut are both Right Option (`main-general`). Debug-only timing settings keep their current audience; exposing them in General is outside this pass.
2. **Credentials and validation:** verify sharing between Post Process, summaries and speaker services, Finish with an empty cloud key, and recovery from a saved default without credentials (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`). Consolidating stores, changing validation and changing defaults remain functional work.
3. **Navigation and accessibility:** exercise arrow navigation, category expansion, deep links, selection reveal and scroll reset, including reaching global destinations below the compact fold. Check focus/selection restoration in compact History/People, accessible names, and keyboard access to the HUD and scrubber. Source shows selection-reveal intent; stills do not establish the complete flow.
4. **Audio and retention:** verify redo/retranscription when History says Recording not found and after meeting audio retention expires (`history-selected`, `msettings-general`). Command availability and retention defaults are outside styling scope.
5. **Live summaries:** establish which recording interval Generate Summary uses and what later generation replaces or preserves (`meetings-recording-wide`).
6. **Destruction and merging:** Return-confirm and Esc-cancel are settled by the brief. Runtime checks still need to establish focus restoration and person-merge consequences/undo (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
