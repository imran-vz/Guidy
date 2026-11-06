# Round 9 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would sign off on this UI-only refresh. The compact local status now preserves its model name, the light warning chip passes on its actual tint, and progress has enough separation from its track. The remaining size-formatting issue is subtler than the screenshots suggest: the fixture now agrees with Models, but the real backend still supplies MiB to a view that now treats the value as decimal MB.

## Evidence and scoring method

I read the complete council brief, including all scope sections, the rubric, review task, round log and the decisions at the end of PLAN.md. I checked my round-8 findings in detail, read my round-7 calibration and consulted my own earlier reports. I did not read any `opus.md` file or another reviewer's report.

I inspected overview sheets containing all 121 current snapshot stems in both themes, 242 PNGs. These cover every main section at default and minimum size, all supplied wide variants, every meeting-settings category, People and person detail, meeting notes/transcript/search/playback/processing/recording/empty states, onboarding, startup, errors, alerts, HUDs, twelve focus states and three hover states. Larger crops checked compact General and Debug, cloud and local AI, Setup, person detail, meeting reading/search, and HUDs. Lower-content crops checked the tall General, Advanced, Debug, Models, Storage and meeting-detail captures. Original-pixel crops checked the ready-local line, warning chip, progress, and destructive alerts.

Direct round-8 image comparisons covered `msettings-ai-local-min` and `meetings-settings` in both themes. A decoded-pixel comparison found **60 captures identical and 182 changed**. Many changes are confined to sidebar positions; the number is not a count of redesigned screens. Baseline comparisons covered `main-models-min`, `meetings-detail` and `onboarding-setup-cloud-min`, both themes. I inspected the supplied Codex, Claude and T3 Code reference PNGs. Codex and Claude retain my earlier reference scores; `(k)` marks product-knowledge estimates beyond their one screenshot. T3 Code, Xcode and System Settings inform calibration without extra columns. These are assessments of supplied evidence, not audits of current live products.

Read-only source checks covered status layout, sidebar metrics and selection reveal, control-column layout, model-size production/fixture paths, Apple Intelligence copy, warning-chip compositing and motion helpers. Only this report was written in the repository. Image sheets were made under `/tmp`. No source was edited, Cargo or the app run, or build/preview channel or live database touched. Logged test results were not rerun. I applied the humanizer skill to the report wording.

Snapshot stems below mean `target/ui-review/round-9/<stem>-{light,dark}.png` unless otherwise stated. Captures are 2×. Missing traffic lights, opaque HUD capture backgrounds, blank chrome in isolated fixtures and unused tails in tall captures are harness conventions. The fixture has Debug and Post Process enabled, adding two global rows. Its clipped bottom row does not prove that a typical default-height sidebar fails to fit.

Interaction scores use stills and limited source evidence. They do not certify runtime hit testing, VoiceOver, keyboard restoration, scroll behavior, animation smoothness or GPU cost. The accepted stock focus edge is 1 pt; custom outlines are 2 pt. The status bar/update entry point stays. Return confirms Delete/Discard and Esc cancels, so the filled destructive default is accurate. Changes to audiences, commands, entry points, stores, defaults, validation and Return bindings remain outside scope.

## 1. Scores

| # | Parameter | Guidy round 9 | Guidy round 8 | Codex | Claude | Reason for Guidy round 9 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | Controls, helpers and reading text retain comfortable roles (`main-general`, `meetings-detail-min`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Medium/semibold marks models, headings and speakers without heavy paragraphs (`msettings-ai-local-min`, `meetings-transcript`). |
| 3 | Color contrast | 9 | 8 | 7 | 7 | Actual warning text/tint now measures 5.122:1; light progress/track is 3.359:1 (`msettings-setup-light`, `meetings-processing-light`). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | Current category, primary content and regional actions remain clear (`msettings-recording`, `meetings-detail`, `main-models`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Toolbar, group, row and helper roles remain distinct (`main-general`, `main-usage`, `msettings-general`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, lists, alerts and the deliberate HUD exception remain coherent (`main-general`, `msettings-linear`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Unified sidebar metrics and consistent content insets preserve order (`main-general-min`, `meetings-settings`, `msettings-ai-wide`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Forms and reading caps work; 200 pt short pop-ups trade content sizing for reset alignment, as recorded (`main-general`, `main-advanced-full`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral surfaces and selection keep semantic colour focused on state (`main-models`, `msettings-setup`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Resets, selections, shortcut fields and transports share treatments; unit formatting is counted under 33 (`focus-9`, `history-selected-min`, `meetings-playing`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Shared reset/control columns and fixed avatar slots remain strong (`main-general`, `main-debug`, `meetings-people`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Stable row/group rhythm and the concise local-status line avoid awkward wraps (`msettings-ai-local-min`, `main-general`, `msettings-recording`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, model choices and recording preferences form clear groups (`msettings-setup-min`, `msettings-ai-local`, `msettings-recording`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Consistent line icons and recognizable transport marks retain their roles (`meetings-playing`, `meetings-people`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Regular 26 pt controls are orderly; wide short-value triggers remain the accepted cost of the aligned column (`main-general-min`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Named commands and recovery actions read clearly (`history-selected-min`, `storage-error`, `msettings-setup-min`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Focus, hover, selection, unavailable and busy stills remain distinguishable (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and independent focus outlines remain clear in both themes (`focus-1`, `meetings-tab-focus`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Custom outlines and accepted opaque stock focus edges stay visible (`focus-1` through `focus-12`, `meetings-tab-focus`; brief). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Sidebar categories remain 24 pt and ordinary rows 26 pt (`meetings-settings`, `msettings-ai-min`; source). |
| 21 | Navigation and wayfinding | 8 | 8 | 8 | 8 | The accepted three-level tree preserves context, but compact global navigation requires scrolling (`meetings-settings`, `msettings-setup-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Intent-based groups and category titles remain clear; tree depth is counted only under 21 (`main-general`, `msettings-general`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Current-category titles, restrained commands and leading Back stay consistent (`msettings-recording`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, source list, panels and modal scrim retain clear tonal levels (`main-general`, `postprocessing-discard`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines separate controls and panes without boxing reading content (`main-general`, `main-postprocessing`, `meetings-detail-wide`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, chips, panels, alerts and HUD capsules retain a consistent radius family (`msettings-ai-local`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Empty libraries and absent summaries explain their next step without duplicate page messages (`meetings-empty`, `meetings-recording-wide`, `storage-error`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named work, percentages and stable layouts remain clear (`startup-app`, `startup-new`, `meetings-processing`, `main-models-min`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Failure and recovery stay understandable; local ready status now fits (`storage-error`, `usage-error`, `msettings-ai-min`, `msettings-ai-local-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Compact cloud shows two catalogue choices; local choices remain visible, with minor copy repetition (`msettings-ai-min`, `msettings-ai-local-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Names, states, prices and quantities have stable scan positions (`main-models-full`, `main-usage-min`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Labels remain plain and casing consistent; repeated local availability/default wording is minor (`main-usage`, `msettings-ai-local`). |
| 33 | Numeric and tabular formatting | 8 | 8 | 8 | 8 | Fixture sizes agree, but production MiB are now treated as decimal MB (`msettings-ai-local`, `main-models-full`; backend/view source). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Compact detail/Back and cloud catalogue remain usable; local status clipping is fixed (`meetings-detail-min`, `history-selected-min`, `msettings-ai-min`, `msettings-ai-local-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms keep their measure while meeting content uses extra width (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted surfaces, neutral selection and restrained bright accents retain depth (`main-general-dark`, `meetings-settings-dark`, `meetings-processing-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Crisp surfaces and readable status colours now include the corrected chip/progress pairs (`focus-1-light`, `msettings-setup-light`, `meetings-processing-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source list, grouped forms, leading Back and accurate destructive defaults feel native (`main-general`, `history-selected-min`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Notes dominate their pane and category pages avoid duplicate large headings (`meetings-detail-wide`, `msettings-recording`, `history-selected-min`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Open reading panes and neutral state treatments remain restrained (`meetings-detail`, `history-selected`, `msettings-ai-local`). |
| 41 | Non-text indicators and use of colour | 9 | 9 | 8 (k) | 8 (k) | Glyphs, checks, words and percentages reinforce colour, with corrected light progress separation (`msettings-setup`, `msettings-ai`, `meetings-processing`). |
| 42 | Truncation and text accommodation | 9 | 8 | 7 (k) | 7 (k) | Ready-local model now stays whole; compact cloud truncates secondary text whose full name is directly below (`msettings-ai-local-min`, `msettings-ai-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Consequences and Cancel are clear; filled destructive action matches Return (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | State, transcript, time and transport remain readable at a glance (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Static progress labels and Reduce Motion paths remain; no new motion defect is established (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.9** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums, in column order: **401/45**, **399/45**, **349/45**, **358/45**. Guidy's unrounded mean improves from **8.8667 to 8.9111**. Round 9 has 41 scores of 9 and four scores of 8. Parameters 3 and 42 rise to 9. Parameter 33 remains 8 despite corrected fixture labels. The holistic ship score is **9/10**; none of the findings below blocks that score.

### Check of round-8 items and round-9 claims

1. **Ready-local clipping: fixed in supplied captures.** Both minimum-size themes preserve `Qwen3 4B Instruct · This Mac · offline` before the switch (`msettings-ai-local-min`). The redundant introductory phrase is gone. Source makes the model fixed and the context tail ellipsizable within an overflow-clipped status container. These captures verify this model at these sizes, not every arbitrary long cloud model or translation.
2. **Warning and progress contrast: fixed.** Original PNG interiors give warning **#944c00** on **#eee5db**, or **5.122:1**, and progress **#523800** against **#8f8f8a**, or **3.359:1** (`msettings-setup-light`, `meetings-processing-light`, `main-models-min-light`). These use actual adjacent colours, excluding antialiasing fringes. The log's 5.5:1 chip claim is high, but the result passes. Source's 20% hover tint predicts approximately **#e6d7c7**, or **4.528:1**, over the observed titlebar base #fafaf9. That hover value is a compositing calculation, not a supplied hover-chip screenshot.
3. **Unified sidebar metrics: verified in source and consistent with stills.** Both states use 26 pt ordinary rows, 24 pt categories, no row gaps and 8 pt group gaps. The category tree naturally inserts rows; the fix removes the separate metric change, not that insertion. Typical expanded geometry is now **10×26 + 9×24 + 3×8 + 4 + 10 = 514 pt**, within the 536 pt body at 900×600. The inflated fixture adds 52 pt. No normal-configuration screenshot is supplied, so typical fit is source-derived. Compact height still needs scrolling, as expected for the recorded navigation choice.
4. **Short pop-ups: unchanged, deliberate.** The 200 pt column keeps resets attached and controls aligned. I retain the small parameter 8/15 deductions against the rubric's content-sizing target, while crediting alignment and consistency at 9. This is an accepted trade-off, not an unfinished fix. I do not ask the next round to restore the rejected floating-reset layout.
5. **Model units: screenshots fixed, production path not fixed.** `msettings-ai-local*` now says 2.5 GB and Download 5.0 GB, matching `main-models-full`. But `meetings_backend.rs::to_view` still computes `size_bytes / (1024 * 1024)`. Both installed metadata and the local DownloadControl now multiply that MiB integer by **1,000,000**, not 1,048,576. The fixture changed to 2,500 and 5,000, values the production conversion does not produce. See finding 1 for exact catalogue calculations.
6. **Repeated Default helper: unchanged and fixture-only.** The real catalogue description does not repeat Default. A production-copy rewrite is not needed to fix that stale fixture.

## 2. Regressions since round 8

**No material visual regression was established in the supplied PNGs.** Local status is better, warning/progress contrast passes, and sidebar metric consistency improves. The fixture's About row now sits farther below the expanded default-height fold because cluster gaps increased from 6 to 8 pt; that is the recorded uniform-spacing trade-off, not evidence of a typical-user default-height failure.

**Source-inferred label regression:** the real local-model Download button previously reconstructed bytes from MiB with `× 1024 × 1024`; it now uses `× 1,000,000`. With actual catalogue input, the larger model's download label would change from **5.0 GB to 4.8 GB** while Models still says 5.0 GB. Installed metadata's former mismatch improves numerically but remains wrong. The new fixture hides this because it supplies decimal-looking values directly. This is display formatting, not evidence of changed model files, download payloads or validation. Parameter 33 stays 8; I do not count it again elsewhere.

## 3. Remaining problems, ranked

None blocks the holistic ship score of 9. “Blocks a parameter 9” below means the particular rubric target, not a request to reopen an accepted design decision.

### 1. Production model sizes still disagree across views

- **What:** the local AI view treats a binary-derived count as decimal MB. Correct-looking fixture labels do not establish production parity.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, `main-models-full`, both themes, plus the production data path. The 4B catalogue has **2,497,281,120 bytes → 2,381 MiB → 2,381,000,000 reconstructed bytes → 2.4 GB** in the AI view, versus **2.5 GB** in Models. The 8B catalogue has **5,027,783,488 bytes → 4,794 MiB → 4,794,000,000 bytes → 4.8 GB**, versus **5.0 GB** in Models. These are source calculations, not runtime observations.
- **Parameters:** **33 = 8; blocks its individual 9.** No additional deduction under consistency or IA.
- **Concrete fix:** preserve the existing backend unit and use `format_bytes(m.size_mb * 1024 * 1024)` for both installed metadata and DownloadControl's size. Build fixture values through the same catalogue-byte-to-MiB conversion, rather than passing 2,500/5,000. This small presentation fix avoids a data-contract migration. Source truncation below 1 MiB does not change these one-decimal GB labels.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::LocalModelsView::local_rows` at the two size multiplications; `crates/guidy-app/src/ui_snapshots.rs::local_models`. Contract reference: `crates/guidy-app/src/meetings_backend.rs::to_view`; exact bytes: `crates/guidy-core/src/llm/local_models.rs::CATALOG`.

### 2. The accepted navigation tree costs compact discoverability

- **What:** Meetings → Meeting settings → categories makes context explicit without a middle pane, but at 720×480 the category run pushes most global destinations below the viewport. The parent has no explicit expanded-state marker, and the still does not show a persistent scroll cue.
- **Evidence:** `msettings-ai-min`, `msettings-setup-min`, both themes; `meetings-settings` for default-height context. The two extra fixture rows are accounted for separately. Typical default-height fit is no longer an open defect.
- **Parameters:** **21 = 8; blocks the rubric's literal two-level target.** This is the recorded trade-off, not an overlooked architecture problem; 22 remains 9.
- **Concrete refinement within the decision:** keep the tree, titles, 24 pt categories and 26 pt global rows. Show the standard vertical scrollbar when the sidebar overflows, and consider a noninteractive expanded-state chevron beside Meeting settings. Do not fade enabled text or add another navigation pane. Those refinements improve discoverability; they cannot make a three-level tree meet a two-level numerical target.
- **Likely source:** `crates/guidy-app/src/ui/main_window.rs::{render_sidebar,nav_items}`. No structural navigation rewrite is requested.

### 3. Fixed pop-ups retain visible slack for short choices

- **What:** Auto, Default, Off and 3 days occupy the full 200 pt trigger. The cost is disproportionate field emphasis for small choices; the benefit is a stable control/reset column with no detached reset button.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `meetings-settings`, both themes. The column is especially successful beside mixed-length values in Advanced.
- **Parameters:** **8 and 15 = 8; block their content-sizing targets.** Alignment, consistency and spacing remain 9. This is an accepted design cost, not a current implementation defect.
- **Concrete action:** retain the decided 200 pt width and 26 pt height. Correct `kit.rs::trailing`'s stale comment saying short pop-ups still hug their value. I do not recommend another width experiment within this loop. A future change should only replace the current rule after demonstrating equally clear reset pairing and alignment.
- **Likely source:** `crates/guidy-app/src/ui/kit.rs::{trailing,Dropdown::render}`. The comment correction has no visual score effect.

### 4. Unavailable Apple Intelligence repeats its status

- **What:** the disabled row says Not available in a trailing chip and starts its helper with Not available on this Mac. At minimum width the helper occupies two lines; the useful System Settings instruction comes after the repeated condition.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, both themes. This is existing polish, not a round-9 regression.
- **Parameters:** 29, 30, 32 and 40 remain **9**. **Not a 9 blocker.** Disabled styling itself is not treated as an active-control contrast failure.
- **Concrete fix:** keep the unavailable chip and make the helper start with the existing instruction, Turn it on in System Settings > Apple Intelligence & Siri. Preserve availability behavior and the existing row layout. Whether that instruction applies to every unavailable reason needs the functional check below.
- **Likely source:** `assets/locales/en/translation.json::meetingsAi.llm.appleUnavailableHint`; its use in `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::local_rows`.

### 5. Fixture-only local-model prose repeats Default

- **What:** the Default chip is followed by The default for summaries. This makes the example noisier than production's catalogue copy.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, both themes; `ui_snapshots.rs::local_models` supplies the repetition. Models shows the catalogue wording in `main-models-full`.
- **Parameters:** 30, 32 and 40 remain **9**. **Not a 9 blocker.** This is evidence cleanup, not a production-copy defect.
- **Concrete fix:** reuse the catalogue description, or shorten the fixture helper to Fast and capable. Keep the Default and Downloaded states and the size.
- **Likely source:** `crates/guidy-app/src/ui_snapshots.rs::local_models`; production description in `crates/guidy-core/src/llm/local_models.rs::CATALOG`.

## 4. What would raise each parameter still below 9

- **8. Width and layout measure:** a future design would need less short-value trigger slack with equally clear reset pairing; the recorded 200 pt decision remains accepted, with no width change requested now.
- **15. Control styling and sizing:** the same future solution would need content-sized visible triggers while preserving 26 pt height and the shared column; retain the current rule in this loop.
- **21. Navigation and wayfinding:** scrollbar/expanded-state cues can improve compact discovery within the accepted tree; meeting the literal two-level target would require an owner-approved change to that recorded decision.
- **33. Numeric and tabular formatting:** reconstruct bytes from the backend's actual MiB unit in both local labels, and make fixtures follow that production conversion.

The other 41 parameters score 9 on the supplied evidence. Status-bar retention, stock focus width and the destructive default are settled scope choices, not new requests for changes. My scores reflect the visual merits and costs of the recorded decisions; documentation alone has not raised or lowered them.

## 5. Functional findings, separate from visual scores

These are verification questions, not runtime failures established by screenshots. They do not reduce visual scores. The source-established unit conversion remains a presentation finding above, not an out-of-scope store/data redesign.

1. **Shortcut semantics:** verify precedence when Dictation key and Transcribe shortcut both display Right Option (`main-general`). Debug-only timing settings retain their audience.
2. **Credentials and validation:** verify sharing across Post Process, summaries and speaker services; Finish with an empty cloud key; and recovery from a saved default without credentials (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`). Store consolidation, defaults and validation changes remain outside this refresh.
3. **Navigation and accessibility:** exercise arrow navigation, category selection/deep links, selection reveal and sidebar scrolling, including global destinations below the compact fold. Check compact History/People focus restoration, accessible names and keyboard access to the HUD and scrubber. Unified metrics and source reveal logic do not establish the runtime flow.
4. **Apple availability explanation:** check whether the single unavailable condition covers unsupported hardware/OS as well as a disabled feature. If so, the current instruction to turn it on cannot describe every cause (`msettings-ai-local`). Do not infer which reason applies from the fixture alone.
5. **Audio, retention and summaries:** verify retranscription when History says Recording not found, behavior after meeting audio retention, and which recording interval Generate Summary covers (`history-selected`, `msettings-general`, `meetings-recording-wide`). Commands and retention defaults stay unchanged.
6. **Destruction and merging:** Return-confirm/Esc-cancel is settled. Runtime focus restoration and person-merge consequences/undo still need verification (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
