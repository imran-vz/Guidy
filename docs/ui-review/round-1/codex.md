# Round 1 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy is substantially easier to read and understand than round 0. I would give this version **7.5/10 overall and 7/10 to ship**. The meeting reading view now feels like a document, and the shared settings language is much more coherent. I would still withhold a 9: important configuration is hidden in ordinary window sizes, compact forms sacrifice useful text, and several control indicators and animation paths remain unfinished.

## Evidence and scoring method

I inspected an overview of every current snapshot state in both themes: 98 states, 196 PNGs. This covered every main section at default and minimum size, all supplied wide variants, meetings, People, meeting settings, onboarding, startup, errors, the deletion alert, and both HUDs. Contact sheets were generated in memory. Tall images were initially inspected through their upper viewport; additional lower-content crops covered General, Advanced, Debug, Models, Post Process, Storage, Usage and meeting notes. I reopened original images or larger crops for cloud setup, the minimum AI pane, slider tracks, welcome checkboxes, wide layouts, alerts and HUDs.

I compared round-0 images of General, meeting detail, minimum Models, default/minimum AI settings, default/minimum cloud setup and minimum People. The remaining baseline judgments are calibrated against my own round-0 report. I inspected all three supplied reference images. T3 Code is useful context for grouped navigation and provider configuration, but is not an extra scored column.

Only this report was written. No source files were changed, no Cargo command was run, and no other reviewer's report was read. Source reads were limited to the UI, theme, and snapshot documentation.

Snapshot stems in this report refer to `target/ui-review/round-1/<stem>-{light,dark}.png` unless a theme or round is explicitly stated. Thus `main-general` cites both theme files. Images are 2× scale. Blank traffic-light space, stand-in frames in isolated settings/People snapshots, and opaque capture backgrounds around the HUDs are harness conventions, not defects. The 1800×4800 tall captures are not evidence of wasted space in the ordinary window.

Scores use the agreed **45-parameter** rubric. Round 0 originally used my 40-parameter proposal; its column below has been remapped and re-scored onto the agreed rubric. Selection, information architecture, separate wide layout, dialogs and HUD design receive explicit baseline judgments. Therefore the comparable baseline mean is 5.9 here, rather than the old published 6.0 over 40 items.

`(k)` marks reference scores requiring product knowledge beyond their single supplied screenshot. Reference scores otherwise retain the earlier judgments. Interaction scores are provisional: stills and source can establish visible styling and explicit animation code, but cannot prove pointer hit bounds, keyboard reachability, focus retention, VoiceOver behavior, scrolling quality or frame cost. Missing interaction evidence is a verification gap, not a claimed runtime failure.

## 1. Scores

| # | Parameter | Guidy round 1 | Guidy round 0 | Codex | Claude | Reason for Guidy round 1 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 8 | 7 | 8 | 8 | System UI text and 12 pt helpers are comfortable; the assistant still uses a separate Geist heading family (`main-general`, `onboarding-welcome`). |
| 2 | Font weight usage | 9 | 7 | 8 | 8 | Restrained regular/medium/semibold roles separate labels, metadata and headings (`main-general`, `main-usage`, `meetings-detail`). |
| 3 | Color contrast | 7 | 5 | 7 | 7 | Body text, fields and switches improved; slider remainders and the light HUD meter still miss the non-text target (`msettings-speakers`, `recording-widget-expanded`). |
| 4 | Information hierarchy | 7 | 5 | 8 | 9 | Notes now dominate their pane, but setup and AI configuration still displace the decision itself (`meetings-detail`, `onboarding-setup-cloud`, `msettings-ai-min`). |
| 5 | Typographic hierarchy | 8 | 5 | 8 | 8 | 15 pt page titles, 17 pt object titles and 14 pt reading text establish hierarchy; several adjacent smaller roles differ by only 1 pt (`main-general`, `meetings-detail`). |
| 6 | Design language | 8 | 7 | 8 | 8 | Shared forms, neutral chips and restrained reading views belong together; custom HUD and focus treatments remain less unified (`main-models`, `msettings-recording`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 8 | 7 | 8 | 9 | Page/group spacing is regular; 24 pt gaps consume too much height before the compact catalogue (`main-general`, `msettings-ai-min`). |
| 8 | Width and layout measure | 7 | 4 | 8 | 9 | Wide forms and model lists are capped, but narrow forms and setup descriptions still lose measure (`main-models-wide`, `main-postprocessing-min`, `onboarding-setup-cloud`). |
| 9 | Color and theme | 8 | 7 | 8 (k) | 8 (k) | Neutral selection, semantic recording red and quieter stats are substantial improvements; amber activity bars still fail in light appearance (`main-usage`, `meetings-recording`, `recording-widget-expanded-light`). |
| 10 | Overall consistency | 8 | 7 | 8 (k) | 8 (k) | Shortcuts, status words and badges are more consistent; source chips, selections and custom focus still use exceptions (`main-general`, `msettings-setup`, `recording-widget-expanded`). |
| 11 | Alignment and grid | 8 | 8 | 8 | 9 | Control/reset columns and catalogue quantities align predictably (`main-general`, `main-models`, `msettings-ai-wide`). |
| 12 | Vertical rhythm | 8 | 7 | 8 | 8 | History now sizes its inline detail to content; short setup panes still spend height on fixed scaffolding (`history-selected-min`, `msettings-ai-min`). |
| 13 | Perceptual grouping | 8 | 7 | 8 | 8 | Settings categories collapse appropriately and notes lose the nested summary card (`meetings-settings`, `meetings-detail`). |
| 14 | Iconography | 8 | 7 | 8 | 8 | Sidebar glyphs are clearer and meeting commands have labels; tiny playback and row actions still need stronger treatment (`main-general`, `meetings-transcript`). |
| 15 | Control styling and sizing | 7 | 6 | 8 (k) | 8 (k) | Fields are clearer and shortcuts share a style; 20–22 pt auxiliary controls and overwide pop-ups remain (`main-general`, `main-postprocessing-min`, `main-models-min`). |
| 16 | Affordance and signifiers | 7 | 5 | 7 | 8 | Copy Notes and More improve recognition; catalogue discovery and icon-only compact download actions remain weak (`meetings-detail-min`, `msettings-ai-min`, `main-models-min`). |
| 17 | Interactive states | 7 | 6 | 8 (k) | 8 (k) | Selected, playing, disabled and busy examples exist; focused selection and custom pressed states are not established (`meetings-playing`, `main-general-full`, `meetings-processing`). |
| 18 | Selection clarity | 7 | 6 | 8 (k) | 8 (k) | Neutral selected rows are legible, but sidebar, people, history and catalogue do not yet demonstrate a consistent focused/unfocused set (`history-selected`, `mpeople-person`, `msettings-ai`). |
| 19 | Focus visibility | 6 | 5 | 7 (k) | 7 (k) | Find has a stronger visible ring; custom segmented controls and fold headings still use a 1 pt border (`meetings-find`; `design.rs::segment`, `kit.rs::foldable_heading`). |
| 20 | Target size | 6 | 5 | 7 (k) | 7 (k) | Speaker editing has a 24 pt slot, but many toolbar/reset/download controls remain compact and actual hit bounds are unverified (`meetings-transcript`, `main-general`, `main-models-min`). |
| 21 | Navigation and wayfinding | 8 | 5 | 8 | 8 | Sidebar clusters and collapsing meeting categories improve orientation; minimum People retains its two-pane squeeze (`main-general`, `meetings-settings`, `mpeople-person-min`). |
| 22 | Information architecture | 7 | 5 | 8 | 8 | Work and maintenance are grouped; shortcut intent and the scope of shared AI configuration still need clearer names (`main-general`, `main-postprocessing`, `msettings-ai`). |
| 23 | Toolbar and window chrome | 8 | 6 | 7 | 8 | Meeting actions now fit a small labeled group and playback is compact; global tabs plus detail bands still occupy noticeable height (`meetings-detail-min`, `meetings-find`). |
| 24 | Surface elevation and layering | 8 | 7 | 8 | 8 | Lifted dark panels separate from the canvas and reading has fewer nested surfaces (`main-general-dark`, `meetings-detail-dark`, `recording-widget-expanded-dark`). |
| 25 | Borders and dividers | 8 | 6 | 8 | 8 | Quieter flat groups and removal of the summary's extra border improve separation; some fields still depend heavily on outlines (`main-general`, `meetings-detail`, `main-postprocessing`). |
| 26 | Corner radius consistency | 9 | 8 | 8 | 8 | Chip/control/panel/dialog/capsule shapes now follow a coherent family (`main-models`, `onboarding-welcome`, `advanced-limit-confirm`, `recording-widget`). |
| 27 | Empty states | 8 | 6 | 8 (k) | 8 (k) | One meeting empty message and a direct Record action replace the duplicate pane messages (`meetings-empty`); other empty states are not sampled. |
| 28 | Loading and progress | 8 | 6 | 8 (k) | 8 (k) | Startup names the operation and processing retains a percentage; shimmer and repeated progress announcements remain (`startup-app`, `startup-new`, `meetings-processing`). |
| 29 | Error and status messaging | 7 | 5 | 7 (k) | 7 (k) | Inline failures give a cause and retry; startup still says only that something went wrong above raw diagnostics (`storage-error`, `usage-error`, `startup-error`). |
| 30 | Data density | 7 | 6 | 8 | 8 | History is denser where it should be, but the minimum AI catalogue shows no rows and People previews lose space (`history-selected-min`, `msettings-ai-min`, `mpeople-person-min`). |
| 31 | Scannability | 7 | 5 | 8 | 9 | Primary model names survive; truncated capabilities and hidden catalogue choices still hinder comparison (`main-models-min`, `msettings-ai-min`). |
| 32 | Microcopy and capitalization | 7 | 5 | 8 (k) | 8 (k) | Sentence case improves most labels, but tray/hotkey terminology, duplicated capabilities and 'Continue Without' remain (`main-advanced`, `main-postprocessing`, `onboarding-picker`, `onboarding-permissions`). |
| 33 | Numeric and tabular formatting | 8 | 6 | 8 | 8 | Tabular UI figures and the explicit per-million-token price basis improve comparison; the minimum usage label still truncates (`main-usage-min`, `msettings-ai`). |
| 34 | Minimum-size reflow (720×480) | 6 | 4 | 8 (k) | 8 (k) | Meeting detail and cloud compact setup adapt better; AI rows disappear and global form labels still squeeze (`meetings-detail-min`, `onboarding-setup-cloud-min`, `msettings-ai-min`, `main-postprocessing-min`). |
| 35 | Wide-size use of space (1440×900) | 8 | 5 | 8 (k) | 8 (k) | Catalogue cap and side-by-side meeting content use wide windows well; saved model IDs still consume comparison space (`main-models-wide`, `meetings-detail-wide`, `msettings-ai-wide`). |
| 36 | Dark-mode quality | 8 | 7 | 8 | 8 | Lifted neutral surfaces and white reading text are easier to use; faint tracks and some custom states remain (`main-general-dark`, `msettings-speakers-dark`, `meetings-playing-dark`). |
| 37 | Light-mode quality | 8 | 6 | 8 (k) | 8 (k) | Crisper fields and darker switch tracks improve parity; HUD activity bars and slider remainders still wash out (`main-general-light`, `msettings-speakers-light`, `recording-widget-expanded-light`). |
| 38 | Native platform fit | 7 | 6 | 7 (k) | 7 (k) | The source list, grouped preferences and quieter toolbar feel more Mac-like; custom buttons and modal evidence remain uneven (`main-general`, `meetings-detail`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 8 | 5 | 8 | 9 | Notes start sooner, summary framing recedes and compact playback helps; configuration still pushes key inputs and choices down (`meetings-detail`, `onboarding-setup-cloud`, `msettings-ai-min`). |
| 40 | Visual noise and restraint | 8 | 6 | 7 | 8 | Neutral model tags, stats and transcript metadata remove much of the previous noise; provider/ID repetitions persist (`main-models`, `main-usage`, `meetings-transcript`, `msettings-ai`). |
| 41 | Non-text indicators and use of colour | 7 | 6 | 8 (k) | 8 (k) | Statuses now combine icons and words; faint slider extents and light audio levels remain exceptions (`msettings-setup`, `msettings-speakers`, `recording-widget-expanded-light`). |
| 42 | Truncation and text accommodation | 6 | 5 | 7 (k) | 7 (k) | Parakeet's full identity is restored, but multiple explanatory lines, usage labels and People previews still truncate (`main-models-min`, `main-advanced-min`, `main-usage-min`, `mpeople-person-min`). |
| 43 | Dialogs and modality | 7 | 6 | 8 (k) | 8 (k) | The deletion alert has a clear consequence and separated actions; minimum-size/focus states are absent and the discard fixture shows no dialog (`advanced-limit-confirm`, `postprocessing-discard`). |
| 44 | HUD and overlay design | 8 | 6 | 7 (k) | 7 (k) | Upright text, red recording state and elapsed time improve glanceability; light activity levels and keyboard treatment need work (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 5 | 5 | 7 (k) | 7 (k) | Static tracks and removed HUD sheen help, but an unguarded recording pulse and active shimmering labels remain (`meetings-recording`, `meetings-processing`; `pages/meetings/recorder.rs`, `design.rs`). |
| | **Mean of 45, one decimal** | **7.5** | **5.9** | **7.8** | **8.0** | Unweighted arithmetic means. |

Sums are **336/45**, **266/45**, **349/45** and **358/45**, respectively. Guidy round 1's holistic ship score is **7/10**. I would ship the foundation improvements, but would not sign off on the complete UI at the requested 9/10 bar.

The round log is mostly borne out: readable helpers, stronger hierarchy, neutral badges, clearer status words, grouped navigation, useful form/catalogue caps, a compact meeting player, fewer meeting actions, and a timer in the collapsed recorder are all visible. Its contrast and motion claims are incomplete. The compact cloud fix also stops at a height breakpoint and leaves the default-size cloud screen worse.

## 2. Regressions since round 0

1. **Default cloud setup hides the provider/key form.** In round 0, `onboarding-setup-cloud` shows the provider, API-key field and key-help line above the footer. In round 1 those controls begin below the initial viewport; only their top edges are visible. Both themes show this. The minimum-size version improves and shows the fields, so the larger default window now has the worse initial composition. This is below-fold content in a scrolling body, not proof that the controls are unreachable.

2. **The minimum AI catalogue loses its last visible row.** Round-0 `msettings-ai-min` shows one catalogue row and its footer. Round 1 shows only the search/filter strip at the bottom, with no model choices. The new 260 pt catalogue minimum preserves a usable list after scrolling to it, but increased form height and spacing push that list out of the first view. Model identity improves once visible; initial discovery regresses.

3. **Minimum People reveals less preview content.** Round-0 `mpeople-person-min` shows all three voiceprint preview rows. In round 1 the third runs below the viewport and the first two lose more text to ellipses. Larger helpers are welcome, but the retained list/detail split needs to adapt with them.

Increased scrolling alone is not automatically a regression. I do not count the lower Appearance group in General or fewer speech rows above the fold as separate failures when the added space improves legibility. The issues above affect the current choice or comparison.

## 3. Remaining problems, ranked

Paths below are relative to `crates/guidy-app/src/ui/`, except the theme and locale files. “Blocks 9” means the issue must be resolved before my sign-off at that score.

### 1. Cloud onboarding adapts to window height instead of its content needs
**Blocks 9.** At 900×600, the 600×500 pt assistant spends its available body height on the display header, selected speech-model description and prerequisite rows. The selected Cloud choice is visible, but the provider/key form is below the initial viewport. At 720×480 the compact branch stacks the summary choice and exposes the form. The default screen should not hide a requirement that the smaller one reveals.

- Evidence: `onboarding-setup-cloud`, `onboarding-setup-cloud-min`; compare round 0's same stems.
- Parameters: 4, 8, 16, 30, 34, 39, 42.
- Fix: use the stacked summary-choice/cloud form at both sizes, or switch based on available body height and selected content. Keep the chosen dictation model to a 36–40 pt summary; collapse optional speaker-turn detail. Preserve the existing footer and body scrolling. Fit provider, key and a short readiness instruction above Finish at both supplied heights.
- Likely source: `onboarding/setup.rs::summary_rows`, its compact decision, and `onboarding/assistant.rs`.

### 2. Minimum AI settings hides every catalogue choice
**Blocks 9.** Status, saved models and provider setup occupy nearly the whole pane. A search box without visible results is a poor introduction to adding a model. The outer scroll region is present in source, so this is a visibility/composition finding, not a claim that catalogue rows cannot be reached.

- Evidence: `msettings-ai-min`; default `msettings-ai` shows only a few rows; `msettings-ai-wide` demonstrates that the rows themselves are now readable.
- Parameters: 4, 8, 16, 30, 31, 34, 39.
- Fix: show a compact current-provider/key summary after configuration, with Edit to reveal the form. Merge the missing-key message into that form instead of repeating its task above it. At 480 pt height, reserve roughly 150–180 pt of the initial viewport for search plus at least two model choices; full catalogue height can remain 260 pt when scrolled into view. Reduce related compact gaps from 24 to 12 pt.
- Likely source: `pages/meetings/ai_settings.rs::render`, `header`, `provider_form`, `your_models`, and `CATALOGUE_MIN_H`.

### 3. Shared forms still retain a horizontal layout after labels lose their measure
**Blocks 9.** At minimum width, the Post Process model controls leave a narrow strip for the explanation. In Advanced, the unload-model explanation is cut after two lines. The meeting-specific stacking work has not solved the shared form case.

- Evidence: `main-postprocessing-min`, `main-general-min`, `main-advanced-min`; default `main-advanced` also truncates the overlay explanation.
- Parameters: 8, 15, 30, 31, 34, 42.
- Fix: stack a row when its remaining label area would be under 220 pt. Let the model field and chooser wrap onto a second line at compact widths. Remove the page-wide two-line clamp in Advanced; wrap essential explanations or provide an explicit disclosure. Pop-ups such as Off and After 5 minutes can hug their content rather than each claiming a full 200 pt.
- Likely source: `kit.rs::SettingRow`, `pages/post_processing.rs`, `pages/advanced.rs::render_body`, shared dropdown/shortcut widths.

### 4. Slider remainders and the light HUD meter still miss the contrast target
**Blocks 9.** The stronger palette is not consistently preserved after opacity and component compositing. Pixel samples from the flat center of the unfilled speaker slider are `#beb49b` on white in light appearance and `#64593e` on `#222222` in dark appearance. They give **2.06:1** and **2.30:1** respectively. The expanded recorder paints lit microphone bars with yellow `#ffc72c` on white, **1.56:1**.

- Evidence: `msettings-speakers`, `main-debug-full`, `recording-widget-expanded-light`; compare the dark HUD.
- Parameters: 3, 9, 37, 41, 44.
- Fix: use an opaque unfilled track such as `#85857e` light and `#767676` dark, then verify the final painted remainder after the stock slider's overlay. Use `tone.accent_text` for lit light-mode source bars; retain yellow on dark. Apply the same rule to necessary seek/progress extents where those communicate information.
- Likely source: `kit.rs::tracked_slider`, `recording_widget.rs::source_chip`, `design.rs::track`, `assets/themes/guidy.json`.

The input-border and switch work should be retained: light `#8f8f8a` is 3.25:1 on white, and the white thumb against the dark-mode on-track `#c08a00` is 3.05:1. The welcome checkbox also has a dark check, so its yellow fill alone is not evidence of an unreadable checked state. The remaining failures are specific painted indicators.

### 5. Decorative recording motion survives the cleanup
**Blocks 9.** `pages/meetings/recorder.rs::render_recording` still attaches a repeating 1600 ms opacity pulse to the live dot without a Reduce Motion check. Processing and queued states still call `design::shimmer_text`. Static progress tracks and removal of HUD sheen are real improvements, but the normal recording surface still has a perpetual decorative loop.

- Evidence: state represented by `meetings-recording`, `meetings-processing` and `main-models`; repeat behavior is established by source, not by a still image.
- Parameters: 28, 45.
- Fix: keep recording dots and busy labels static. Use the existing words, timer and actual progress to communicate work. Retain real input-level updates and brief entrance transitions, with Reduce Motion respected.
- Likely source: `pages/meetings/recorder.rs`, `design.rs::shimmer_text` and its callers in meeting/list/history/model UI. The generic `sheen` helper still exists, but I found no remaining UI call to it and do not count dead helper code as visible animation.

This review does not measure GPU use or assert a frame-cost regression.

### 6. Focus styling is still split between stock and custom controls
**Blocks 9 pending verification.** The light focus token is corrected, and Find is clearly focused. Custom segments and fold headings still reveal focus by recoloring a 1 pt border. HUD pill actions use custom clickable divs without a local focus-visible treatment.

- Evidence: `meetings-find`, `main-history-min`, `recording-widget-expanded`; `design.rs::segment`, `kit.rs::foldable_heading`, `recording_widget.rs::pill`.
- Parameters: 17, 18, 19, 38.
- Fix: one shared 2 pt focus treatment with clearance, using the current opaque theme ring. Apply it to custom segments, disclosures, list items and HUD commands. Verify focus stays visible after scrolling, changing category and resizing. This is a styling/source gap; keyboard reachability itself needs runtime inspection.

### 7. Small auxiliary actions still need a consistent target contract
**Blocks 9 pending verification.** Meeting Info/More and resets use small/xsmall button variants; several compact model actions become tiny symbols. A 24 pt reset column controls alignment but does not itself prove the child's hit rectangle is 24 pt high.

- Evidence: `main-general`, `main-models-min`, `meetings-detail-min`, `mpeople-person-min`.
- Parameters: 14, 15, 16, 20.
- Fix: give standalone icon actions explicit minimum 24×24 pt hit bounds, preferably 28 pt in meeting headers, with at least 4 pt spacing. Keep 14–16 pt glyphs. Make compact Resume/Retry discoverable through a label or a clearly named menu, retaining tooltips.
- Likely source: `kit.rs::reset_button`, `model_card.rs`, `download_control.rs`, `pages/meetings/detail.rs`, `pages/meetings/people.rs`. Actual hit bounds remain unmeasured.

### 8. Model names are fixed, but model comparison text still loses to fixed columns
**Blocks 9.** Parakeet V3 is now readable at minimum width. Its capability description, the two recommended meeting bundles and several other descriptions still truncate. At default width, the Required by chip leads the description and uses the space needed to explain the model. The cloud catalogue is much better and should keep its current primary-name allocation.

- Evidence: `main-models`, `main-models-min`, `main-models-full`, `main-models-wide`; compare `msettings-ai`.
- Parameters: 4, 8, 30, 31, 40, 42.
- Fix: keep primary names on their own line; allow up to two lines for decision-relevant capabilities. Put Required by below the description, or in details, at ordinary as well as minimum widths. Move size/actions to a second band when their fixed 56+150/196 pt columns would otherwise take essential text. Bundle summaries should wrap rather than hide which models are included.
- Likely source: `model_card.rs::ModelRow`, `pages/models.rs`.

### 9. People does not adapt its second pane at minimum width
**Blocks 9 for text accommodation.** The person name and fields survive, but the list metadata and voiceprint quote/source lose context. The larger type has made the existing split less useful.

- Evidence: `mpeople-person-min`, compared with `mpeople-person` and round 0's minimum pair.
- Parameters: 8, 21, 30, 31, 34, 42.
- Fix: below a useful detail width of about 400 pt, use a person-detail view with Back, matching compact meetings. Let voiceprint quotes wrap to two lines, keep a short meeting label and time below, and move full email/voiceprint counts out of the compressed list summary.
- Likely source: `pages/meetings/people.rs`.

### 10. Short copy still carries repetition and ambiguous terms
**Blocks 9 for copy quality.** The picker repeats “English only” and language counts after descriptions that already say them. Advanced still says “Show tray icon” above a helper that says “menu bar”. General does not explain why two Right Option bindings exist. “Continue Without” names no object.

- Evidence: `onboarding-picker`, `main-advanced`, `main-general`, `onboarding-permissions`, `main-postprocessing`.
- Parameters: 10, 16, 22, 30, 32.
- Fix: one capability statement per model; “Show menu bar icon”; “Continue Without Accessibility” or a short action with a clear adjacent consequence; a behavior-specific label/helper for each shortcut. Use “shortcut” consistently instead of mixing hotkey and shortcut. Keep the agreed distinction between sentence-case labels and Title Case buttons.
- Likely source: `onboarding/model_picker.rs`, `kit.rs::model_description`, `pages/general.rs`, `pages/advanced.rs::platform_key`, `assets/locales/en/translation.json`. Resolve shortcut semantics before changing its meaning.

### 11. Startup failure copy is plainer but still does not explain the cause
The heading is better; “Something went wrong while starting” still requires the user to read “Failed to initialize history manager: Permission denied” to learn what happened. The large monospaced block remains the main substantive explanation.

- Evidence: `startup-error`, `startup-error-min`; ordinary `storage-error` is a better cause/action example.
- Parameters: 4, 29, 32, 38.
- Fix: for this known error, say Guidy cannot open its history file and lacks permission. Put the exact path and OS error in a Details disclosure or secondary copyable area. Keep the existing Show Logs and Quit commands; a recovery action requires a real supported operation.
- Likely source: `onboarding/startup.rs`.

### 12. Selection and interaction states need one complete shared specification
Neutral selection is a sound direction. Sidebar selection has an accent icon and weight, history and People mainly have a tint, the catalogue uses a checked box, and playing transcript text receives another fill. These concepts can differ, but focused versus unfocused selection should be predictable within each list.

- Evidence: `history-selected`, `mpeople-person`, `msettings-ai`, `meetings-playing`, `main-general`.
- Parameters: 6, 10, 17, 18, 24, 36.
- Fix: document resting, hover, pressed, selected-unfocused and selected-focused colors for the shared list row. Keep a persistent check for catalogue membership and a play marker for playback, while reusing the selection tokens for actual selection. Add focused/unfocused examples before claiming parity.
- Likely source: `design.rs::Tone`, `main_window.rs`, `pages/history.rs`, `pages/meetings/people.rs`, `pages/meetings/detail.rs`, `pages/meetings/ai_settings.rs`.

### 13. Usage's minimum summary label loses its unit of meaning
“Audio transcri…” sits over “5 h”. It can be inferred, but this is unnecessary truncation in a four-cell summary whose label is essential.

- Evidence: `main-usage-min`; compare `main-usage`.
- Parameters: 8, 33, 34, 42.
- Fix: use a 2×2 stat grid at this width or shorten the label to “Audio time”. Let labels wrap before truncating. Preserve the improved 17 pt tabular values.
- Likely source: `design.rs::stat_grid`, `section_header`, `pages/usage.rs`.

### 14. Finish the shared type and navigation language
These are refinements after the preceding fixes. The assistant uses Geist display headings while ordinary UI uses the system family. Group headings at 13 pt and helpers at 12 pt are distinguishable mostly by weight. Sidebar clusters help, but remain unnamed, and the broader AI/meeting preferences still need clear scope.

- Evidence: `onboarding-welcome`, `main-general`, `main-postprocessing`, `meetings-settings`, `msettings-ai`.
- Parameters: 1, 5, 6, 9, 11, 13, 21, 22, 23, 38.
- Fix: either use the system family throughout or document a narrow brand-heading exception and agree it against rubric 1. Keep the 15/17/22 pt hierarchy; use role/weight consistently for small headings rather than adding more sizes. Clarify preference scope through destination names and short helper copy; avoid adding another permanent navigation row. Quiet sidebar group labels are worth comparing only if they improve recognition without crowding the 480 pt height.
- Likely source: `onboarding/assistant.rs`, `design.rs`, `main_window.rs`, meeting settings labels.

## 4. What would raise every parameter still below 9

Items 2 and 26 already score 9. The following lines cover every other parameter. Where stills cannot settle the target, the next step is verification, not a speculative redesign.

1. **Typography:** use one UI family or agree an explicit display-only exception; retain 12 pt helpers and 14 pt reading text.
3. **Color contrast:** fix the final composited slider remainders and light HUD meters; check all necessary indicators on their actual surfaces.
4. **Information hierarchy:** put cloud requirements and catalogue choices in the first viewport of their task.
5. **Typographic hierarchy:** apply a documented role/weight hierarchy consistently, including the small group/helper roles.
6. **Design language:** bring custom focus, HUD controls and list-state exceptions into the shared system.
7. **Padding and spacing:** keep normal form spacing, but use 12 pt related gaps in short configuration panes.
8. **Width and layout measure:** stack controls before labels fall below 220 pt; preserve readable People and model comparison text.
9. **Color and theme:** use darker light-mode ink for necessary activity marks and keep accent roles explicit.
10. **Overall consistency:** unify status, selection, shortcut terminology, custom action sizing and focus treatment.
11. **Alignment and grid:** preserve the current aligned columns when controls wrap; verify both compact form layouts.
12. **Vertical rhythm:** make short-pane scaffolding content-aware while retaining the improved History detail height.
13. **Perceptual grouping:** keep each requirement with its field/action and avoid a separate repeated provider warning.
14. **Iconography:** standardize standalone action glyphs at 14–16 pt and label uncommon compact operations.
15. **Control styling and sizing:** use regular 24–26 pt controls and content-sized pop-ups; give auxiliary actions explicit hit bounds.
16. **Affordance and signifiers:** show choices under catalogue search, expose required setup, and name ambiguous actions.
17. **Interactive states:** verify rest/hover/pressed/selected/disabled/busy for stock and custom controls in both themes.
18. **Selection clarity:** demonstrate focused/unfocused selection for each list using shared tokens and appropriate persistent marks.
19. **Focus visibility:** apply the same 2 pt visible ring with clearance to every custom focusable control and verify clipping.
20. **Target size:** measure actual 24×24 pt minimum hit bounds and spacing for toolbar, reset, download and voiceprint actions.
21. **Navigation and wayfinding:** give minimum People the same explicit list/detail return path as minimum Meetings.
22. **Information architecture:** distinguish shortcut behavior and clearly name where meeting configuration and shared AI credentials belong.
23. **Toolbar and window chrome:** retain Copy Notes/More and compact playback; confirm narrow titles and Find keep useful reading height.
24. **Surface elevation and layering:** retain lifted dark surfaces and verify focused/playing/selected states on each layer.
25. **Borders and dividers:** preserve flat grouping while making necessary control/state edges reliable without extra nested boxes.
27. **Empty states:** retain the improved meeting empty state and verify History, catalogue/no-results and missing-note states follow it.
28. **Loading and progress:** retain named work and percentages; replace shimmering labels with stable text and verify layout stability.
29. **Error and status messaging:** give startup failures a plain cause and consequence, with technical details secondary.
30. **Data density:** expose catalogue rows at minimum height and fit useful People/model comparison content.
31. **Scannability:** keep full model capability/bundle text and stable quantity/action positions when rows wrap.
32. **Microcopy and capitalization:** remove duplicated capabilities, ambiguous “Continue Without”, and Mac tray/hotkey wording.
33. **Numeric and tabular formatting:** retain tabular figures and explicit price basis; keep every summary label and unit readable.
34. **Minimum-size reflow:** resolve cloud/catalogue visibility, narrow shared forms, People adaptation and Usage labels at 720×480.
35. **Wide-size use of space:** retain current caps and useful dual reading panes; test long names/content against their column budgets.
36. **Dark-mode quality:** finish necessary track and custom-state contrast on lifted surfaces without adding glare.
37. **Light-mode quality:** give activity meters and slider extents sufficient contrast while retaining the improved fields and switches.
38. **Native platform fit:** finish shared focus/targets, scoped navigation wording and conventional modal keyboard behavior.
39. **Content-first focus:** retain the improved meeting composition and put configuration's current decision above secondary scaffolding.
40. **Visual noise and restraint:** remove repeated provider/ID/capability fragments; keep neutral stats and badges.
41. **Non-text indicators and use of colour:** preserve icon-plus-word statuses and repair slider/audio activity marks.
42. **Truncation and text accommodation:** wrap essential explanations, bundle contents, voiceprint context and summary labels; stress longer strings.
43. **Dialogs and modality:** capture the actual discard alert plus minimum-size and focused deletion alerts; verify Esc, default action and focus restoration.
44. **HUD and overlay design:** fix light activity contrast and verify focus/target behavior across collapsed and expanded recording states.
45. **Motion and transitions:** remove perpetual recording pulses and label shimmer; verify remaining transitions honor Reduce Motion.

## 5. Functional findings, separate from visual scores

These are verification questions or fixture concerns. No runtime bug is established by the screenshot data alone, and they do not enter the visual score.

1. **Shortcut precedence remains unclear.** General still presents Dictation key and Transcribe shortcut as Right Option. Establish their activation/precedence rules before renaming them. Evidence: `main-general`.
2. **Cloud setup readiness still needs a defined outcome.** Finish is enabled with an empty key. Verify whether this intentionally enters an incomplete-but-usable app, and whether missing summary configuration is clearly surfaced afterward. Evidence: `onboarding-setup-cloud` and its minimum variant.
3. **Saved/default versus usable AI state needs checking.** Claude is labeled default while the page asks for its provider key. Verify that downstream actions distinguish a saved choice from a ready provider. Evidence: `msettings-ai`.
4. **Check nested catalogue scrolling.** Source gives the short AI page an outer scroll and the catalogue its own trapped inner scroll. Verify wheel/trackpad, Page Down, keyboard focus and returning to the form at scroll limits. The screenshot proves initial composition only. Evidence: `msettings-ai-min`, `ai_settings.rs::render`, `kit.rs::ScrollTrap`.
5. **Recording retention and retranscription need an availability check.** History offers a redo symbol while recording is not found; meeting audio retention deletes source audio after three days. Verify disabled/recovery behavior and the stated consequence. Evidence: `history-selected`, `msettings-general`.
6. **Active-recording summary behavior remains unspecified in the evidence.** Verify whether Generate Summary uses the current portion and how it relates to later generation. Evidence: `meetings-recording-wide`.
7. **The discard fixture does not show a discard alert.** `postprocessing-discard` shows the editable prompt with no modal or scrim. Check that the scenario actually reaches the intended confirmation before using it to certify dialog design. This may be a harness issue; it is not proof that runtime confirmation is absent.
8. **Person merge and keyboard flows still require runtime review.** Verify confirmation/scope/undo for merge, and tab order, accessible names and focus restoration across People, model choices, HUD commands and alerts. Evidence: `mpeople-person`, `mpeople-person-min`, `recording-widget-expanded`, `advanced-limit-confirm`.

