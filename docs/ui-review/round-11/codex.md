# Round 11 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **9.0/10 overall and 9/10 to ship**. I would sign off on this UI-only refresh. The startup-order mismatch and Storage contrast defect from my round-10 report are fixed. The RAM repetition is gone, and the shared child-row geometry is more consistent. No remaining finding blocks a holistic 9.

## Evidence and scoring method

I read the complete COUNCIL_BRIEF.md, including all scope sections, RUBRIC.md, REVIEW_TASK.md, the round log in README.md and the decisions at the end of PLAN.md. I checked all five round-10 findings and consulted my round-0 through round-9 opening assessments for calibration. I did not read any `opus.md` file or another reviewer's report.

I inspected contact sheets covering all **121 current snapshot stems in both themes, 242 PNGs**. Coverage includes every main section at default/minimum size, every supplied wide variant, all Meeting settings categories, People/detail, meeting notes/transcript/find/playback/processing/recording/empty states, onboarding, startup, errors, alerts, HUDs, twelve focus states and three hover states. Larger paired views checked compact local/cloud AI, Debug, onboarding, alerts and HUDs. Tall-capture crops checked General, Debug, Models, Storage, meeting transcript and Meeting settings. Original-pixel crops checked Storage, sliders and the new sidebar edge.

Direct round-10 comparisons covered Storage, startup, local AI and Speakers in both themes. Decoded-pixel comparison found **188 captures identical and 54 changed**, across 27 stems. Baseline comparisons covered compact Models, meeting detail and compact cloud onboarding in both themes. I viewed all three supplied reference PNGs. Codex and Claude scores retain my round-10 calibration; `(k)` marks judgments drawn from product knowledge where their single screenshots are insufficient. T3 Code, Xcode and macOS System Settings inform platform judgment. These scores describe the supplied reference evidence and earlier calibration, not a new audit of live reference products.

Read-only source checks covered startup/loaded navigation order and metrics, overflow presentation, Storage colours, slider backing, local-model detail/RAM labels and MiB conversion, unavailable Apple styling, onboarding scrolling and motion branches. I wrote only this report in the repository. Temporary image sheets/crops and a report-generation script are under `/tmp/guidy-review11`. I did not edit source, run Cargo, launch the app, touch a build/preview channel or a live database, or rerun the log's claimed checks.

Snapshot stems mean `target/ui-review/round-11/<stem>-{light,dark}.png` unless a round/theme is stated. Captures are 2×. Missing traffic lights, opaque HUD backgrounds, empty chrome in isolated fixtures and empty tails in tall captures are harness conventions. Debug and Post Process add two sidebar rows here. Their overflow at default height is not evidence that a typical sidebar fails to fit. Minimum-height expanded navigation legitimately scrolls.

Interaction scores use stills and limited source inspection. They do not establish runtime hit testing, VoiceOver, focus restoration, scroll responsiveness, animation smoothness or GPU cost. I used the humanizer skill for report wording.

### Recorded decisions

The fixed 200 pt pop-up column earns credit for alignment and reset pairing. It retains a modest content-sizing cost under 8/15, as in round 10; I request no reversal. The status bar and update entry point remain in scope as retained UI. Stock 1 pt focus edges are accepted under the brief, with custom 2 pt rings. Filled Delete/Discard correctly expresses the Return default. Top-level Meeting settings with one category level is a sound presentation choice. None of these decisions is treated as an overlooked requirement.

## 1. Scores

| # | Parameter | Guidy round 11 | Guidy round 10 | Codex | Claude | Reason for Guidy round 11 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | Comfortable control, helper and reading sizes survive compact panes (`main-general-min`, `meetings-detail-min`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Medium and semibold distinguish titles, model names and speakers without heavy paragraphs (`msettings-ai-local-min`, `meetings-transcript`). |
| 3 | Color contrast | 9 | 8 | 7 | 7 | Storage segment edges and actual slider remainders now exceed 3:1 in both themes (`main-storage`, `main-debug-min`, `msettings-speakers`; pixel checks below). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | Category title, content and regional commands remain clear (`msettings-recording`, `meetings-detail`, `main-models`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Toolbar, group, row and helper roles retain a legible hierarchy (`main-general`, `main-usage-min`, `msettings-general`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, lists, alerts and the deliberate HUD exception share a coherent language (`main-general`, `msettings-linear`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Regular content insets, cluster gaps and shared child geometry keep spacing orderly (`main-general-min`, `meetings-settings`, `msettings-ai-min`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Forms and reading widths work; short values retain the recorded 200 pt trigger slack (`main-general`, `main-advanced-full`, `meetings-settings`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral surfaces and selection reserve colour for meaningful action and status (`main-models`, `msettings-setup`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 8 | 8 (k) | 8 (k) | Startup now matches the loaded navigation order; People and categories share child geometry (`startup-app`, `main-general`, `meetings-settings`; source). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Name, glyph and trailing control columns align, including the two kinds of child row (`main-debug-full`, `msettings-ai-local-min`, `meetings-settings-full`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Regular row heights and group gaps remain steady as local-model helpers wrap (`main-general-full`, `msettings-ai-local-min`, `msettings-recording`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Permissions, model choices and recording preferences remain clear groups (`msettings-setup-min`, `msettings-ai-local`, `msettings-recording`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | People and category glyphs now use the same 16 pt treatment and slot (`meetings-settings`, `msettings-ai-min`, `meetings-people`; source). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Regular controls are usable; fixed short-value pop-ups retain the accepted content-sizing cost (`main-general-min`, `meetings-settings`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Commands, recovery actions and transport controls remain recognizable (`history-selected-min`, `storage-error`, `recording-widget-expanded`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | Hover, focus, selection, disabled and busy stills remain distinct (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection remains separate from keyboard focus in sidebar, list and tabs (`focus-1`, `meetings-tab-focus`, `meetings-people-min`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Custom 2 pt outlines and the accepted opaque stock focus edges remain visible (`focus-1` through `focus-12`, `meetings-tab-focus`; scope exception). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Global rows remain 26 pt and categories 24 pt; supplied controls retain usable targets (`meetings-settings`, `msettings-ai-min`; source, not live hit testing). |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Top-level Meeting settings and one category level retain explicit location; compact details have Back (`msettings-ai-min`, `meetings-detail-min`, `meetings-people-person-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Intent-based categories and the separate settings cluster remain understandable (`main-general`, `meetings-settings`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Toolbars retain a title and restrained commands, with compact Back paths (`msettings-recording`, `history-selected-min`, `meetings-people-person-min`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar, panels and modal scrim preserve depth in both themes (`main-general`, `postprocessing-discard`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines divide panes and controls; the new overflow edge does not dim navigation (`main-postprocessing`, `meetings-detail-wide`, `msettings-ai-min`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, chips, panels, alerts and HUD capsules retain a consistent radius family (`msettings-ai-local`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Empty libraries and missing summaries state a clear next step (`meetings-empty`, `meetings-recording-wide`, `storage-error`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named work, percentages and static skeletons explain waiting; startup order now matches (`startup-app`, `startup-new`, `meetings-processing`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Cause and recovery remain plain; unavailable Apple recovery text is somewhat subdued (`usage-error`, `storage-error`, `msettings-ai-local-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | The RAM recommendation now appears once; compact cloud still shows two complete choices (`msettings-ai-local-min`, `msettings-ai-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Stable positions preserve scanning across names, states, quantities and prices (`main-models-full`, `main-usage-min`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Local-model detail now uses the same separators as Models and drops redundant RAM wording (`msettings-ai-local-min`, `main-models-full`). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Installed and download labels remain 2.5/5.0 GB and units retain their basis (`msettings-ai-local-min`, `main-models-full`, `main-usage-min`; source). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Compact details and AI catalogues stay usable; optional cloud-onboarding content still requires scrolling (`meetings-detail-min`, `msettings-ai-min`, `onboarding-setup-cloud-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms retain their measure while meeting notes and transcript use extra width (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted neutral surfaces, restrained accents and corrected indicator tracks remain comfortable (`main-general-dark`, `main-storage-dark`, `msettings-speakers-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | White panels and control edges stay crisp, with corrected Storage and slider separation (`main-general-light`, `main-storage-light`, `msettings-speakers-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source lists, grouped forms, Back paths and truthful alert defaults read as a Mac app (`main-general`, `history-selected-min`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Reading content dominates its pane and settings begin near the top (`meetings-detail-wide`, `msettings-recording`, `history-selected-min`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral states, open reading panes and removal of the RAM badge keep visual noise low (`meetings-detail`, `history-selected`, `msettings-ai-local`). |
| 41 | Non-text indicators and use of colour | 9 | 8 | 8 (k) | 8 (k) | Storage segments and slider remainders now meet 3:1; text and icons reinforce state (`main-storage`, `msettings-speakers`, `msettings-setup`; pixel checks). |
| 42 | Truncation and text accommodation | 9 | 9 | 7 (k) | 7 (k) | Primary model names fit; compact cloud only truncates its repeated status tail (`msettings-ai-local-min`, `msettings-ai-min`, `main-models-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Alerts explain consequences and provide Cancel; filled Delete/Discard matches Return (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | HUD state, clock, transcript and transport remain readable at a glance (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Startup stays static; inspected HUD motion honors Reduce Motion; live smoothness remains untested (`startup-app`, `overlay-live`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **9.0** | **8.9** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Exact sums, in column order: **403/45**, **400/45**, **349/45**, **358/45**. Guidy's unrounded mean is **8.9556**, versus **8.8889** in round 10. Forty-three parameters score 9 and two score 8. Parameters **3, 10 and 41 rise from 8 to 9**; the other scores are unchanged. **Holistic ship score: 9/10.**

### Check of round-10 findings and round-11 claims

1. **Storage contrast: fixed.** In `main-storage`, `main-storage-min` and `main-storage-full`, Other now uses the track grey. Sampled flat pixels are `#523800` Guidy / `#8f8f8a` Other / `#ffffff` free in light, and `#ffc72c` / `#707070` / `#222222` in dark. Guidy/Other is **3.359:1 light, 3.174:1 dark**; Other/free is **3.249:1 light, 3.213:1 dark**. This resolves my previous ~1.8:1 internal-boundary finding without changing the fractions or removing the readable totals. Parameters 3/41 return to 9.
2. **Startup ordering: fixed.** `startup-app` and `startup-app-min` now put People below Meetings, then Models/Advanced, then unindented Meeting settings closing the settings cluster. `SidebarSkeleton::render` uses current visibility rules and inserts Meeting settings after the last settings section, matching `MainWindow::nav_items`. The two states share 26 pt global rows, reserved 2 pt borders, 18 pt glyph slots, 16 pt glyphs and 8 pt gaps. Fixture flags differ, so raw y-position comparisons to a Debug/Post Process-enabled shell would be misleading. Parameter 10 returns to 9. A live transition was not exercised.
3. **Fixed pop-ups: retained by decision.** The aligned reset/control column remains useful in `main-general`, `main-advanced-full` and `meetings-settings`, both themes. Parameters 8/15 stay 8 against the literal content-sized-control targets. This is an accepted cost, not a pending fix.
4. **Sidebar overflow cue: improved, still subtle.** `msettings-ai-min`, `msettings-ai-local-min`, `msettings-setup-min` and default-height expanded settings now have a 1 pt track-coloured bottom edge when more rows remain. Native crops show the edge; `render_sidebar` conditions it on remaining scroll extent. The edge leaves enabled text at full contrast. A horizontal rule can still read as a footer separator rather than an invitation to scroll, so I retain a minor discoverability note. This does not reduce 21/34/42 below 9.
5. **Repeated RAM advice: fixed for the supplied catalogue.** `msettings-ai-local*` now presents the 8B memory recommendation once, in the helper. The redundant chip is absent and descriptions use the same middle-dot detail convention as `main-models-full`. Both size-label paths still convert MiB back to bytes with `1024 * 1024`; the 2.5/5.0 GB correction survives. The source's badge suppression detects the numeric memory phrase in the description. Arbitrary localized or future catalogue wording was not exercised.
6. **Shared child geometry: verified.** People and settings categories now share the same slot, glyph and gap; categories retain their 24 pt row height versus 26 pt for People. Default and compact screenshots show the same relationship in both themes. The old third-level/narrow-no-glyph comments remain stale in `main_window.rs`; this is documentation debt, not a visual regression or a reason to reopen the settled hierarchy.
7. **Slider backing: verified on rendered pixels.** In `msettings-speakers` and Debug crops, the visible remainder is `#908b7c` light and `#7b7051` dark after the stock 20% fill tint. Fill/remainder is **3.206:1 light, 3.145:1 dark**. Remainder/panel is **3.404:1 light, 3.242:1 dark**. These are the actual composite colours, not just backing-token calculations. The changes also reach the disabled Volume slider in `main-general-full`. Stock controls were not patched.

## 2. Regressions since round 10

**No material visual regression found in the inspected evidence.** The changed captures support the logged Storage, startup, child-row, slider, detail-copy and overflow-edge work. The two alert captures retain the same readable modal composition; changed underlying controls do not establish an alert regression. The 188 identical captures provide useful continuity, but do not prove behavior unchanged.

## 3. Remaining problems, ranked

These are residual polish observations. **None blocks a holistic 9.** Only item 5 holds individual parameters below 9, and it is the recorded trade-off. Items 1–4 do not justify reopening a finished UI pass on their own.

### 1. The new overflow edge is easy to read as a footer rule

- **What/evidence:** in `msettings-ai-min`, `msettings-ai-local-min` and `msettings-setup-min`, both themes, the list ends neatly after Linear while Integrations and the lower destinations continue below. A darker horizontal edge is visible, but no scroll thumb or directional signifier identifies the remaining rows. Default-height fixture overflow also shows the edge; the two extra fixture rows are acknowledged.
- **Parameters:** 16/21/34/42 remain 9. This limits discoverability polish, not reachability or the settled two-level hierarchy. **Not a 9 blocker.**
- **Concrete refinement:** use a conventional 3–4 pt vertical scroll thumb when the sidebar overflows, in a reserved or overlay gutter that preserves labels and 24/26 pt targets. Keep text at full contrast. The current hairline may remain as separation.
- **Likely source:** `crates/guidy-app/src/ui/main_window.rs::render_sidebar`, using `sidebar_scroll`.

### 2. Compact cloud onboarding gives little sign of its remaining optional row

- **What/evidence:** `onboarding-setup-cloud-min`, both themes, fully exposes the provider/key form and help, which is the right priority. The Better speaker turns row visible in default `onboarding-setup-cloud` lies below the first compact viewport, immediately beyond the fixed footer. The remaining content has little visible scrolling cue. This is a smaller version of a familiar short-window issue, not a return of the hidden-key problem.
- **Parameters:** 16/34 remain 9. The affected choice is optional, and source confirms a scrollable body. **Not a 9 blocker.**
- **Concrete refinement:** reserve a 12–16 pt glimpse of the next row, or expose the body's scroll thumb while it overflows. Keep the cloud key/help in the first viewport and keep all commands, defaults and choices intact. Do not shrink the reading text to make it fit.
- **Likely source:** `crates/guidy-app/src/ui/onboarding/assistant.rs::Assistant` and `ui/onboarding/setup.rs` layout around `setup-turns`.

### 3. The unavailable Apple row dims its recovery instruction with its control

- **What/evidence:** in `msettings-ai-local` and `msettings-ai-local-min`, both themes, the Apple option, Not available chip and System Settings instruction all receive the disabled row opacity. The helper is visibly fainter than the other model descriptions, although it tells the reader how to proceed. Source applies `opacity(0.55)` to the whole Apple row.
- **Parameters:** 16/29 remain 9. The inactive option is correctly disabled; I do not treat its exempt disabled styling as a new global contrast failure. This is a refinement to the explanation's prominence. **Not a 9 blocker.**
- **Concrete refinement:** apply disabled opacity to the radio/name/chip only; render the existing recovery helper at `tone.text_muted` outside that opacity. Preserve unavailable selection behavior. Whether that instruction is the correct recovery for every unavailable cause is a separate functional question.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::AiSettingsView::local_rows`, Apple row.

### 4. Compact cloud status spends width repeating a model name

- **What/evidence:** `msettings-ai-min`, both themes, shows “Anthropic key needed · Claude Sonn…” immediately above the complete saved-model row “Claude Sonnet 4.5”. The important cause survives and the full name is available below, so this is harmless truncation, but the clipped duplicate gives the otherwise tidy top band an unfinished edge. The local status in `msettings-ai-local-min` fits cleanly.
- **Parameters:** 30/32/42 remain 9. There is only one secondary truncation in this compact pane. **Not a 9 blocker.**
- **Concrete refinement:** at the compact breakpoint, show the existing condition alone in this status line; let the selected saved-model row carry the full model name. Keep the same provider/key condition, selection and commands.
- **Likely source:** `crates/guidy-app/src/ui/pages/meetings/ai_settings.rs::{status_line,render}`.

### 5. Short values retain oversized pop-up triggers, as agreed

- **What/evidence:** Auto and 3 days fill the same 200 pt column as longer values in `main-general`, `main-general-min`, `main-advanced-full` and `meetings-settings`, both themes. The resulting slack is visible; so is the benefit of stable reset pairing and a consistent trailing column.
- **Parameters:** **8/15 remain 8**, against the literal content-sizing targets. **Not a holistic 9 blocker.**
- **Concrete action:** retain the recorded 200 pt width and 26 pt control height in this loop. A future alternative should be considered only if it preserves reset pairing and alignment while reducing visible slack. There is no new implementation request here.
- **Likely source if reconsidered later:** `crates/guidy-app/src/ui/kit.rs::{trailing,Dropdown::render}` and `design::CONTROL_COLUMN_W`.

## 4. What would raise each parameter still below 9

- **8. Width and layout measure:** a future design would need less short-value trigger slack while preserving the aligned reset/control relationship; retain the agreed 200 pt column now.
- **15. Control styling and sizing:** a future design would need content-sized visible triggers with the same 26 pt height, target area and clear reset pairing; no width change is requested in this pass.

The other 43 parameters score 9 on the supplied evidence. The recorded status bar, focus edges, alert default and two-level navigation are accepted on their merits.

## 5. Functional findings, separate from visual scores

These are carried-forward verification questions, not behavior failures established by this review. They do not reduce visual scores.

1. **Navigation and accessibility:** verify arrow navigation in visual order, selected-row reveal, scrolling to Integrations/About, compact Back/focus restoration, accessible names and keyboard access to HUD controls and the scrubber. The new edge is conditionally drawn from scroll extent; live scroll and keyboard behavior were not tested.
2. **Credentials and validation:** verify sharing across Post Process, summaries and speaker services, Finish with an empty cloud key, and recovery from a saved default without credentials (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`). Store/default/validation changes remain out of scope.
3. **Apple availability:** establish whether unavailable means disabled, unsupported hardware/OS, or several causes. “Turn it on” is appropriate only where that action resolves the cause (`msettings-ai-local`). This review confirms the displayed wording and styling, not the backend diagnosis.
4. **Shortcut semantics:** verify precedence and displayed modifiers for Dictation key versus Transcribe shortcut (`main-general`). Debug-only controls retain their existing audience.
5. **Audio, retention and summaries:** verify retranscription when History says Recording not found, behavior after audio retention, and which recording interval Generate Summary covers (`history-selected`, `msettings-general`, `meetings-recording-wide`). Commands and defaults remain unchanged.
6. **Destruction and merging:** Return-confirm/Esc-cancel is a settled behavior. Runtime focus restoration, person-merge consequences and undo remain unverified (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
