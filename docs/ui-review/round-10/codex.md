# Round 10 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would sign off on this UI-only refresh. The two-level navigation and production model-size correction resolve my two actionable sub-9 findings from round 9. One new inconsistency remains: startup still shows the previous sidebar order. I also caught a pre-existing Storage graph contrast miss that my earlier scores overlooked.

## Evidence and scoring method

I read the complete COUNCIL_BRIEF.md, all scope clarifications, RUBRIC.md, REVIEW_TASK.md, the round-10 log and the decisions at the end of PLAN.md. I checked my round-9 findings and earlier calibration in my own round-0 through round-8 reports. I did not read any `opus.md` file or another reviewer’s report.

I inspected overview sheets covering all **121 snapshot stems in both themes, 242 PNGs**. These include every main section at default/minimum size, all supplied wide variants, every meeting-settings category, People and person detail, meeting notes/transcript/search/playback/processing/recording/empty states, onboarding, startup, errors, alerts, HUDs, twelve focus states and three hover states. Larger views checked local and cloud AI at minimum size, Meeting settings, startup, compact Usage, Storage, wide meeting reading and the references. Crops checked lower content in tall captures, complete alerts, HUDs and Storage’s segment boundary at original pixels.

Direct round-9 comparisons covered local AI, Meeting settings and startup in both themes. Decoded pixels show **60 captures identical and 182 changed**. Most changes are sidebar placement, not page redesigns. Outside the sidebar, light-theme differences are confined to the local AI panes, Storage’s graph colour and the two alerts. All `startup-app*` captures are identical to round 9. Baseline comparisons covered compact Models, meeting detail and compact cloud onboarding in both themes. I inspected the supplied Codex, Claude and T3 Code reference PNGs. Codex/Claude scores retain my previous calibration; `(k)` means product knowledge beyond their single supplied screenshot. Xcode and System Settings inform platform judgment. This is not a fresh audit of live reference products.

Read-only source checks covered the real and startup sidebars, model sizes from catalogue through backend to both AI labels, fixture values/copy, shared pop-ups, Storage’s segments and motion paths. Only this report was written in the repository; image sheets/crops were made under `/tmp`. I did not edit source, run Cargo or the app, or touch a build/preview channel or live database. Logged test results were not rerun. I used the humanizer skill for the report wording.

Snapshot stems below mean `target/ui-review/round-10/<stem>-{light,dark}.png` unless qualified. Captures are 2×. Missing traffic lights, opaque HUD backgrounds, blank chrome in isolated fixtures and empty tails in tall captures are harness conventions. Debug mode and Post Process add two global sidebar rows; their presence is not evidence that a typical user’s default-height sidebar fails to fit. At 720×480 even a typical expanded sidebar needs scrolling.

Interaction scores use stills and limited source inspection, not runtime verification of hit testing, VoiceOver, focus restoration, scrolling, animation smoothness or GPU cost. I accept the fixed 200 pt pop-up column on its recorded merits, retain the status bar/update entry point and accept the stock 1 pt focus edge versus custom 2 pt rings. Return confirms Delete/Discard and Esc cancels, so the filled destructive default is correct. Audience, command, store, default, validation and key-binding changes remain outside this refresh.

## 1. Scores

| # | Parameter | Guidy round 10 | Guidy round 9 | Codex | Claude | Reason for Guidy round 10 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | Control, helper and reading roles remain comfortable (`main-general-min`, `meetings-detail-min`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Medium and semibold distinguish headings, names and speakers without heavy paragraphs (`msettings-ai-local-min`, `meetings-transcript`). |
| 3 | Color contrast | 8 | 9 | 7 | 7 | Storage’s adjacent used-space segments measure only 1.807:1 light and 1.803:1 dark; other previously corrected pairs remain sound (`main-storage`, `msettings-setup`, `meetings-processing`). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | Category title, content and regional action remain clear (`msettings-recording`, `meetings-detail`, `main-models`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Toolbar, group, row and helper roles retain a clear hierarchy (`main-general`, `main-usage`, `msettings-general`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, lists, alerts and the deliberate HUD exception use a coherent language (`main-general`, `msettings-linear`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Consistent content insets and the 26/24 pt sidebar rows preserve order (`main-general-min`, `meetings-settings`, `msettings-ai-wide`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Forms and reading widths work; fixed 200 pt short pop-ups retain the recorded content-sizing cost (`main-general`, `main-advanced-full`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral surfaces and selection leave colour for action and status (`main-models`, `msettings-setup`, `meetings-recording`). |
| 10 | Overall consistency | 8 | 9 | 8 (k) | 8 (k) | The startup sidebar still uses the old Meeting settings position and indent, unlike the loaded shell (`startup-app`, `startup-app-min`, `main-general`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Shared control/reset columns and leading icon slots remain orderly (`main-general`, `main-debug`, `meetings-people`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Rows and group gaps stay regular; local AI helpers wrap without collisions (`msettings-ai-local-min`, `main-general`, `msettings-recording`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, model choices and recording preferences form clear groups (`msettings-setup-min`, `msettings-ai-local`, `msettings-recording`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Categories now retain recognizable line glyphs at compact width; transport marks remain clear (`msettings-ai-min`, `meetings-playing`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Regular controls remain well sized; fixed short-value triggers are the accepted cost of an aligned column (`main-general-min`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Commands and recovery actions remain named and recognizable (`history-selected-min`, `storage-error`, `msettings-setup-min`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Hover, focus, selection, disabled and busy stills remain distinct (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and independent focus outlines distinguish location from keyboard focus (`focus-1`, `meetings-tab-focus`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Custom outlines and the accepted opaque stock focus edges stay visible (`focus-1` through `focus-12`, `meetings-tab-focus`; brief). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Global rows remain 26 pt and category rows 24 pt; inspected controls retain usable targets (`meetings-settings`, `msettings-ai-min`; source). |
| 21 | Navigation and wayfinding | 9 | 8 | 8 | 8 | Meeting settings is now top-level with one category level; People stays under Meetings and compact detail has Back (`meetings-settings`, `msettings-ai-min`, `meetings-detail-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Settings closes its intent-based cluster and category titles identify the shown pane (`main-general`, `meetings-settings`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Restrained toolbar commands, category titles and leading Back remain consistent (`msettings-recording`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar, panels and modal scrim maintain depth in both themes (`main-general`, `postprocessing-discard`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines separate panes and controls without enclosing reading content (`main-postprocessing`, `meetings-detail-wide`, `main-general`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, chips, panels, alerts and HUD capsules retain a small radius family (`msettings-ai-local`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Empty libraries and missing summaries explain their next step without duplicate page messages (`meetings-empty`, `meetings-recording-wide`, `storage-error`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named work, percentages and static skeletons communicate loading; startup row-order mismatch is counted under 10 (`startup-app`, `startup-new`, `meetings-processing`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Failure and recovery remain plain, and Apple’s helper now leads with the instruction (`usage-error`, `storage-error`, `msettings-ai-local-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Compact cloud still shows two complete choices and local options remain visible; RAM is repeated in one row (`msettings-ai-min`, `msettings-ai-local-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Names, states, quantities and prices retain stable scan positions (`main-models-full`, `main-usage-min`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | The redundant Default and unavailable phrases are removed; the larger local model repeats its RAM advice (`msettings-ai-local-min`, `main-usage`). |
| 33 | Numeric and tabular formatting | 9 | 8 | 8 | 8 | Production and fixture now reconstruct bytes from MiB, yielding the same 2.5/5.0 GB labels as Models (`msettings-ai-local`, `main-models-full`; source). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Compact detail, local status and cloud catalogue remain usable; sidebar overflow needs a clearer cue (`meetings-detail-min`, `history-selected-min`, `msettings-ai-min`, `msettings-ai-local-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms retain their measure while meetings use width for notes and transcript (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted neutral surfaces and restrained accents remain well judged; graph contrast is counted under 3/41 (`main-general-dark`, `meetings-settings-dark`, `meetings-processing-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Surfaces and control edges remain crisp; the Storage graph exception is counted under 3/41 (`focus-1-light`, `main-general-light`, `msettings-setup-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source list, grouped forms, Back navigation and accurate destructive defaults read as a Mac app (`main-general`, `history-selected-min`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Notes dominate their pane and settings start near the top without duplicate large titles (`meetings-detail-wide`, `msettings-recording`, `history-selected-min`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral state treatments and open reading panes remain restrained, with minor local-model repetition (`meetings-detail`, `history-selected`, `msettings-ai-local`). |
| 41 | Non-text indicators and use of colour | 8 | 9 | 8 (k) | 8 (k) | Words and quantities reinforce states, but Storage’s Guidy/Other boundary misses the 3:1 indicator target in both themes (`main-storage`, `main-storage-min`). |
| 42 | Truncation and text accommodation | 9 | 9 | 7 (k) | 7 (k) | Primary model names survive compact width; only the secondary cloud status tail truncates above its complete saved-model row (`msettings-ai-local-min`, `msettings-ai-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Alerts state consequences and provide Cancel; filled Delete/Discard correctly matches Return (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | HUD state, transcript, clock and transport remain readable at a glance (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Startup is static, decorative sheen has no callers, and HUD transitions/caret have Reduce Motion branches; runtime smoothness is untested (`startup-app`, `overlay-live`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.9** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums, in column order: **400/45**, **401/45**, **349/45**, **358/45**. Guidy’s unrounded mean is **8.8889**, versus **8.9111** in round 9. Forty parameters score 9; five score 8. Parameters 21 and 33 rise to 9, 10 falls to 8, and 3/41 fall to 8 for a newly recognized pre-existing contrast defect. The small arithmetic decline does not mean the navigation or size fixes made the UI worse. **Holistic ship score: 9/10.** No remaining finding blocks that holistic score.

### Check of round-9 findings and round-10 claims

1. **Production size formatting: fixed.** `meetings_backend.rs::g2_core::to_view` still supplies `size_bytes / (1024 * 1024)`. Both `LocalModelsView::local_rows` paths in `ai_settings.rs` now multiply by `1024 * 1024`: the DownloadControl fallback at line 2469 and installed metadata at line 2565. Catalogue values reconstruct as **2,497,281,120 → 2,381 MiB → 2,496,659,456 bytes → 2.5 GB**, and **5,027,783,488 → 4,794 MiB → 5,026,873,344 bytes → 5.0 GB**. The sub-MiB precision loss does not affect these labels. The fixture now supplies 2,381/4,794, matching the backend conversion, rather than the previous decimal-looking values. `msettings-ai-local*` and `main-models-full` agree in both themes. This closes the production-path defect, not merely the screenshot mismatch.
2. **Navigation depth: fixed in the loaded shell.** `nav_items` places Meeting settings after the last configuration section; `NavItem::level` gives it level 0 and categories level 1. Default and compact screenshots retain the category glyphs and full labels. People stays one level below Meetings. `step_section` uses that visual order and requests selected-row reveal. The two-level target is met, so parameter 21 rises to 9. Compact overflow remains a minor presentation refinement, not a reason to reopen this structure or blame the two extra fixture rows.
3. **Startup geometry: partly verified; no-jump claim does not hold.** Source now uses 26 pt rows, reserved 2 pt borders, 8 pt cluster gaps and no row gaps, matching the real sidebar’s metrics. However, it still inserts Meeting settings as an indented child immediately after People. The loaded shell places it after Advanced/Post Process as a top-level row. The unchanged `startup-app*` screenshots show that old structure. Static captures cannot demonstrate the transition, but source establishes that the two layouts differ even with the same visibility configuration.
4. **Apple unavailable repetition: fixed.** The chip carries Not available and the helper leads with the System Settings instruction, fitting on one line in `msettings-ai-local-min`. Whether the instruction applies to every unavailable cause remains a functional question.
5. **Fixture-only Default repetition: fixed.** Local fixtures now use the catalogue names and descriptions. The new 8B helper reveals a separate, small duplication: the RAM recommendation appears in both chip and prose. That wording already exists in production; this is better evidence of it, not a new production-copy regression.
6. **Fixed pop-ups: accepted and unchanged.** I retain the small deductions under 8/15 for the literal content-sizing target, while crediting alignment and consistency elsewhere. No return to floating resets is requested. The stale `kit::trailing` comment has also been corrected.

## 2. Regressions since round 9

### Startup and loaded navigation now disagree on row order

`startup-app` and `startup-app-min`, both themes, retain Meetings → People → Meeting settings → Models → Advanced. The loaded `main-general` and `main-general-min` show Meetings → People → Models → Advanced → Post Process → Meeting settings. Post Process/Debug visibility differs between fixtures, so those extra rows alone are not the finding. **The parentage and relative order of Meeting settings versus Models/Advanced differ regardless of those flags.** `SidebarSkeleton::render` confirms the old insertion and indentation. The new loaded-shell ordering introduces this mismatch; the startup screenshot itself did not change. Parameter 10 falls from 9 to 8. See finding 2 for the fix.

No other material visual regression was established in the inspected captures. The Storage segment contrast miss predates this round: light improves from **1.605:1 to 1.807:1**, and dark stays **1.803:1**. My previous scores missed it. The changed 8B prose reflects a more faithful fixture, not newly changed catalogue text.

## 3. Remaining problems, ranked

None blocks a holistic ship score of 9. “Blocks a parameter 9” below refers to that rubric target. The fixed-pop-up item records an accepted design cost, not unfinished work.

### 1. Storage’s two used-space segments lack contrast

- **What:** the small Guidy segment touches Other with no separator. The enclosing track outline and separate legend do not distinguish their shared boundary.
- **Evidence:** `main-storage`, `main-storage-min`, `main-storage-full`, both themes. Original-pixel interiors are **#523800 against #63635e = 1.807:1** in light and **#ffc72c against #9a9a9a = 1.803:1** in dark. Source draws adjacent fills without an internal edge. This is separate from the corrected progress-bar fill versus `#8f8f8a` remainder. The readable numeric totals provide another way to obtain the quantities, limiting the practical severity.
- **Parameters:** **3 and 41 = 8; blocks their individual 9s.** Theme quality remains 9; I do not count this again under 36/37.
- **Concrete fix:** add a **1 pt divider in `tone.surface` inside the Guidy segment’s trailing edge**, mirrored for RTL, without changing segment fractions. White in light and `#222222` in dark contrast above 3:1 with both neighboring fills. Apply it only when both used segments are present. Keep the legend, totals and existing palette.
- **Likely source:** `crates/guidy-app/src/ui/pages/storage.rs::{bar_colors,disk_bar}`. No global progress-token change is needed.

### 2. Startup still presents the previous navigation structure

- **What:** Meeting settings changes location and indent when startup gives way to the loaded shell. Matching row metrics alone cannot prevent the move.
- **Evidence:** `startup-app`, `startup-app-min` versus `main-general`, `main-general-min`, both themes; `SidebarSkeleton::render` versus `MainWindow::nav_items`. The source establishes the layout mismatch; the duration and perceptual strength of the live transition were not tested.
- **Parameters:** **10 = 8; blocks its individual 9.** Loaded navigation itself meets 21; named/static loading still meets 28. No duplicate deduction under motion.
- **Concrete fix:** apply the loaded sidebar’s row-order rule to startup: People alone under Meetings, then Models/Advanced/optional Post Process, then unindented Meeting settings in that cluster. Preserve the matching 26 pt row, 8 pt cluster-gap and inset rules. Prefer one small shared row-description builder so both states follow the same visibility/order policy. Update stale third-level comments beside `NavItem` and `render_sidebar` as part of that alignment.
- **Likely source:** `crates/guidy-app/src/ui/onboarding/startup.rs::{SidebarSkeleton::render,sidebar_skeleton}` and `crates/guidy-app/src/ui/main_window.rs::{nav_items,NavItem::level}`.

### 3. Short choices retain oversized pop-up triggers, by decision

- **What:** Auto, Off and 3 days fill the 200 pt column, giving short values more field area than they need. The aligned reset/control relationship is a real benefit.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `meetings-settings`, both themes. Advanced’s mixed-length values demonstrate why the shared column works.
- **Parameters:** **8 and 15 = 8; below their content-sizing targets.** Alignment, spacing and overall component discipline are credited. This does not block the holistic 9.
- **Concrete action:** **retain the decided 200 pt width and 26 pt height in this loop**. A future alternative would need equally clear reset pairing and alignment before replacing it. There is no new implementation fix requested here.
- **Likely source if the owner revisits it:** `crates/guidy-app/src/ui/kit.rs::{trailing,Dropdown::render}` and `design::CONTROL_COLUMN_W`.

### 4. Compact sidebar overflow has little visible indication

- **What:** at 720×480, the expanded category run continues below Linear without a visible scrollbar in the supplied still. Integrations and data/app destinations require scrolling. The two-level structure improves grouping, but does not reduce the number of rows.
- **Evidence:** `msettings-ai-min`, `msettings-ai-local-min`, `msettings-setup-min`, both themes. `render_sidebar` enables vertical scrolling and source reveals keyboard-selected rows. This is a discoverability refinement, not proof that the rows are unreachable. A typical expanded configuration still overflows minimum height; default-height fixture overflow includes the two extra rows.
- **Parameters:** **21, 34 and 42 remain 9. Not a 9 blocker.** Essential navigation labels fit, scrolling is an appropriate compact response, and the recorded two-level structure should remain.
- **Concrete refinement:** expose a standard slim vertical scrollbar when content exceeds the viewport, with its track occupying a reserved gutter or overlay space that does not shorten category labels. Keep all 24 pt category targets and 26 pt global rows. Do not fade enabled destination text.
- **Likely source:** `crates/guidy-app/src/ui/main_window.rs::render_sidebar` and its existing `sidebar_scroll` handle.

### 5. The 8B local model repeats its memory recommendation

- **What:** Recommended for 16 GB+ RAM sits above prose ending Recommended with 16 GB of memory or more. At compact width the helper takes two lines to repeat an already stated requirement.
- **Evidence:** `msettings-ai-local`, `msettings-ai-local-min`, both themes. `main-models-full` shows that the helper is real catalogue wording. Round 10’s fixture now represents it accurately.
- **Parameters:** **30, 32 and 40 remain 9. Not a 9 blocker.** The copy is understandable and fits; this is minor repetition.
- **Concrete fix:** in the local AI rows, retain the complete catalogue helper and omit the redundant RAM recommendation chip for these catalogue models. Preserve Default, Downloaded, availability warnings and all RAM eligibility logic. Models can continue showing the same complete helper. No model metadata or behavior needs to change.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::LocalModelsView::local_rows`; catalogue reference `crates/guidy-core/src/llm/local_models.rs::CATALOG`.

## 4. What would raise each parameter still below 9

- **3. Color contrast:** separate Storage’s Guidy/Other fills with a 1 pt edge that contrasts at least 3:1 against both fills in both themes.
- **8. Width and layout measure:** a future alternative would need less short-value trigger slack with equally clear reset pairing; retain the recorded 200 pt decision now.
- **10. Overall consistency:** give startup the loaded shell’s Meeting settings position, hierarchy and visibility/order rules.
- **15. Control styling and sizing:** a future alternative would need content-sized visible triggers while preserving 26 pt height and the shared reset column; no width change is requested in this loop.
- **41. Non-text indicators and use of colour:** make the internal Storage boundary readable independently of similar segment luminance; preserve numeric totals and the legend.

The other 40 parameters score 9 on the supplied evidence. Status-bar retention, stock focus width, destructive default and the updated two-level navigation are settled choices assessed on their merits.

## 5. Functional findings, separate from visual scores

These are verification questions carried forward, not runtime failures established by stills. They do not reduce the visual scores.

1. **Navigation and accessibility:** exercise arrow navigation through the new visual order, category deep links, selected-row reveal and sidebar scrolling. Verify compact History/People Back and focus restoration, accessible names and keyboard access to HUD controls and the scrubber. Source order/reveal code does not establish the full runtime flow.
2. **Credentials and validation:** verify sharing across Post Process, summaries and speaker services, Finish with an empty cloud key and recovery from a saved default without credentials (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`). Stores, defaults and validation remain outside scope.
3. **Apple availability explanation:** establish whether unavailable means disabled, unsupported hardware/OS, or several causes. The instruction to turn it on is appropriate only where that action can resolve it (`msettings-ai-local`). The fixture alone does not establish the cause.
4. **Shortcut semantics:** verify precedence and displayed modifiers for Dictation key and Transcribe shortcut (`main-general`). Debug-only timing settings retain their audience.
5. **Audio, retention and summaries:** verify retranscription when History says Recording not found, behavior after meeting audio retention and which recording interval Generate Summary covers (`history-selected`, `msettings-general`, `meetings-recording-wide`). Commands and retention defaults remain unchanged.
6. **Destruction and merging:** Return-confirm/Esc-cancel is settled. Runtime focus restoration, person-merge consequences and undo remain unverified (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
