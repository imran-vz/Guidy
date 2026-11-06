# Round 2 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy now scores **8.4/10 overall and 8/10 to ship**. The major round-1 composition problems are fixed. Cloud setup exposes its key form, the short AI pane shows choices, People has a usable compact detail view, and Meetings no longer carries global navigation in its title bar. I would ship this UI improvement. I would still withhold the requested 9 until the remaining indicator, compact-density and interaction-styling issues are addressed.

## Evidence and scoring method

I inspected in-memory overviews of all **99 current states in both themes, 198 PNGs**. This includes every main section at default and minimum size, every supplied wide variant, all meeting/settings/People states, onboarding, startup, errors, both alerts and the HUDs. I reopened originals or larger crops for cloud setup at both sizes, compact AI, minimum Models and People, playing meetings, speaker sliders, History headings, wide catalogues/reading panes, alerts and the expanded recorder. Lower-content crops covered Models, Advanced, Debug, Post Process and Usage. Overview thumbnails establish composition; contrast measurements use original pixels.

I directly compared round-1 light and dark versions of minimum Models, General, AI and People, default cloud setup and playing meetings. A round-0 comparison covered General, meeting detail, minimum Models, cloud setup, minimum AI and minimum People. Earlier Codex reports provide the remaining calibration. All three reference images were inspected; Codex and Claude retain the same rubric scores as round 1. T3 Code, Xcode and System Settings inform the design judgment without adding score columns.

Only this report was written. No source was edited, no Cargo command or live app was run, and no other reviewer's report was read. Read-only source checks covered UI tokens, shared controls, navigation, History, models/downloads, Meetings, onboarding and HUDs. The round log was treated as a set of claims to verify.

Snapshot stems below refer to `target/ui-review/round-2/<stem>-{light,dark}.png` unless a theme or previous round is stated. A bare stem cites both themes. Images are 2×. Missing traffic lights, stand-in frames in isolated settings/People captures, opaque HUD capture backgrounds and unused lower space in tall captures are harness conventions, not defects. The alerts are tall fixtures; I do not infer their ordinary-window position from the total tall-image height.

Scores 17, 19, 20 and 45 are provisional judgments from stills plus explicit source styling, sizes and animations. This was not a keyboard, VoiceOver, scroll, performance or full state-matrix test. Missing runtime evidence is not itself a reported failure. The 2 pt focus-ring and unconditional 24 pt hit-area requirements are the council's targets; a smaller or differently shaped indicator is not automatically a WCAG AA failure. Reference scores marked `(k)` use the same product-knowledge estimates as the previous report.

The new **UI-only scope** is applied throughout. Navigation and IA receive credit for the new presentation without waiting for credential-store consolidation, setting exposure changes or new commands. Defaults, validation and Enter behavior are excluded from visual scores. Round 1 is reproduced as published, rather than retrospectively changing its scores under the new scope; some improvement in parameter 22 therefore reflects the clarified scoring boundary.

## 1. Scores

| # | Parameter | Guidy round 2 | Guidy round 1 | Codex | Claude | Reason for Guidy round 2 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 8 | 8 | 8 | 8 | System body, helper and reading sizes work; onboarding still uses Geist display headings (`main-general`, `onboarding-welcome`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Regular, medium and semibold express role without heavy paragraphs (`main-general`, `meetings-detail`, `main-usage`). |
| 3 | Color contrast | 8 | 7 | 7 | 7 | Speaker tracks and lit HUD bars pass the measured target; the seek remainder is only 1.13:1 light / 1.31:1 dark (`msettings-speakers`, `meetings-playing`). |
| 4 | Information hierarchy | 8 | 7 | 8 | 9 | Cloud requirements and AI choices are visible; compact recommendations now overshadow downloads (`onboarding-setup-cloud`, `msettings-ai-min`, `main-models-min`). |
| 5 | Typographic hierarchy | 8 | 8 | 8 | 8 | Page/object/display titles are distinct; 12 pt group/helper and 13 pt row roles still lean on weight (`msettings-general`, `main-general`, `onboarding-welcome`). |
| 6 | Design language | 8 | 8 | 8 | 8 | Forms, alerts and reading panes agree; custom focus and small-action exceptions remain (`main-general`, `advanced-limit-confirm`, `meetings-find`). |
| 7 | Padding and spacing | 8 | 8 | 8 | 9 | Shared page gaps are orderly, but compact model spacing is wasteful and 14 pt row insets remain outside the agreed grid (`main-models-min`, `main-general`). |
| 8 | Width and layout measure | 8 | 7 | 8 | 9 | Forms wrap before labels squeeze and wide columns stay bounded; compact recommendation rows overallocate height (`main-postprocessing-min`, `main-models-wide`, `main-models-min`). |
| 9 | Color and theme | 9 | 8 | 8 (k) | 8 (k) | Neutral surfaces and selection, restrained gold and semantic recording/error colours work in both themes (`main-general`, `meetings-recording`, `recording-widget-expanded`). |
| 10 | Overall consistency | 8 | 8 | 8 (k) | 8 (k) | Resets and commands improved; model actions, History seeking and HUD commands retain different sizing/focus contracts (`main-models-full`, `history-selected`, `recording-widget-expanded`). |
| 11 | Alignment and grid | 9 | 8 | 8 | 9 | Leading edges and trailing controls remain aligned when rows stack (`main-general-min`, `main-postprocessing-min`, `main-usage-min`). |
| 12 | Vertical rhythm | 8 | 8 | 8 | 8 | Normal rows are tighter; the compact Models fallback makes simple recommendations unnecessarily tall (`main-general`, `main-models-min`). |
| 13 | Perceptual grouping | 9 | 8 | 8 | 8 | Setting groups, provider requirements and notes have clear common regions without nested summary cards (`meetings-settings`, `msettings-ai`, `meetings-detail`). |
| 14 | Iconography | 9 | 8 | 8 | 8 | Filled playback glyphs and quieter transcript-row actions resolve the most visible exceptions (`meetings-playing`, `meetings-transcript`, `recording-widget-expanded`). |
| 15 | Control styling and sizing | 8 | 7 | 8 (k) | 8 (k) | Bordered text commands and shorter pop-ups help; small buttons and several fixed-width selectors still fall short of the target (`history-selected`, `main-general`, `main-models-full`). |
| 16 | Affordance and signifiers | 9 | 7 | 7 | 8 | Commands are named and bounded; compact AI search now leads directly to choices (`history-selected`, `msettings-ai-min`, `meetings-detail-min`). |
| 17 | Interactive states | 8 | 7 | 8 (k) | 8 (k) | Selected, disabled, busy and playing states are clear; custom pressed/focus presentation remains incomplete (`meetings-playing`, `meetings-processing`, `msettings-ai`). |
| 18 | Selection clarity | 9 | 7 | 8 (k) | 8 (k) | Neutral selection is visible in both themes; source now distinguishes list focus for sidebar, History and People (`main-general`, `history-selected`, `mpeople-person`). |
| 19 | Focus visibility | 8 | 6 | 7 (k) | 7 (k) | Segments/disclosures now reserve 2 pt borders; History seeking retains 1 pt and content tabs use only an underline (`meetings-find`, `history-selected`; UI source). |
| 20 | Target size | 8 | 6 | 7 (k) | 7 (k) | Resets are 24 pt and meeting header icons 28 pt; scrubbers remain 16 pt high and model actions still use small buttons (`meetings-playing`, `main-models-full`; UI source). |
| 21 | Navigation and wayfinding | 9 | 8 | 8 | 8 | People/settings are titled sidebar destinations, categories remain discoverable at 900 pt, and compact detail has Back (`meetings-settings`, `meetings-people`, `mpeople-person-min`). |
| 22 | Information architecture | 9 | 7 | 8 | 8 | Within the UI-only scope, library/preferences/app clusters and meeting-specific categories communicate intent (`main-general`, `meetings-settings`, `msettings-ai`). |
| 23 | Toolbar and window chrome | 9 | 8 | 7 | 8 | Title-bar navigation is gone, Back leads at minimum size, and detail commands stay a small group (`meetings-detail-min`, `meetings-settings`, `meetings-detail-wide`). |
| 24 | Surface elevation and layering | 9 | 8 | 8 | 8 | Canvas, source list and panels separate calmly; alerts and HUDs sit above content in both themes (`main-general`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 8 | 8 | 8 | Hairlines separate grouped rows and pane boundaries without double-framing notes (`main-general`, `meetings-detail`, `meetings-settings`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Chips, fields, groups, alerts and capsules retain a coherent radius family (`main-models`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 8 | 8 (k) | 8 (k) | The meeting empty state has one headline, explanation and direct action (`meetings-empty`). |
| 28 | Loading and progress | 8 | 8 | 8 (k) | 8 (k) | Named static labels and percentages are clear; redundant spinner/progress combinations persist (`meetings-processing`, `startup-app`, `main-models-full`). |
| 29 | Error and status messaging | 8 | 7 | 7 (k) | 7 (k) | Startup now explains permission denial plainly; AI attention status and duplicated error sentences need refinement (`startup-error`, `msettings-ai-min`, `storage-error`). |
| 30 | Data density | 8 | 7 | 8 | 8 | AI choices and People context fit better; compact Models now exposes less active work (`msettings-ai-min`, `mpeople-person-min`, `main-models-min`). |
| 31 | Scannability | 8 | 7 | 8 | 9 | Wrapped capabilities and stable numeric columns improve comparison; repeated IDs and tall compact rows slow scanning (`main-models-full`, `msettings-ai`, `main-models-min`). |
| 32 | Microcopy and capitalization | 8 | 7 | 8 (k) | 8 (k) | Picker repetition, tray wording and Continue Without are fixed; shortcut wording, repeated model metadata and error phrasing need a final edit (`onboarding-picker`, `onboarding-permissions`, `main-general`, `storage-error`). |
| 33 | Numeric and tabular formatting | 9 | 8 | 8 | 8 | Tabular figures, complete 2×2 summary labels and explicit catalogue price basis are readable (`main-usage-min`, `msettings-ai`, `meetings-playing`). |
| 34 | Minimum-size reflow (720×480) | 8 | 6 | 8 (k) | 8 (k) | Cloud setup, AI choices, People, Usage and shared form wrapping work; compact model density remains the exception (`onboarding-setup-cloud-min`, `msettings-ai-min`, `mpeople-person-min`, `main-models-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 8 | 8 (k) | 8 (k) | Capped catalogue/form columns and useful dual reading panes use wide space well (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`). |
| 36 | Dark-mode quality | 8 | 8 | 8 | 8 | Lifted neutral layers and static recording states are strong; the seek remainder remains too close to the canvas (`main-general-dark`, `meetings-recording-dark`, `meetings-playing-dark`). |
| 37 | Light-mode quality | 8 | 8 | 8 (k) | 8 (k) | Light controls and lit HUD bars improved; the seek extent still washes out (`msettings-speakers-light`, `recording-widget-expanded-light`, `meetings-playing-light`). |
| 38 | Native platform fit | 8 | 7 | 7 (k) | 7 (k) | Sidebar, panes and destructive alerts feel more Mac-like; focus and auxiliary sizing retain custom exceptions (`meetings-settings`, `advanced-limit-confirm`, `history-selected`). |
| 39 | Content-first focus | 8 | 8 | 8 | 9 | Reading begins sooner and onboarding exposes its decision; compact Models still puts completed setup above current downloads (`meetings-detail`, `onboarding-setup-cloud`, `main-models-min`). |
| 40 | Visual noise and restraint | 8 | 8 | 7 | 8 | Transcript icons recede and busy text is static; saved-model IDs/default labels and duplicate History text remain (`meetings-transcript`, `msettings-ai`, `history-selected`). |
| 41 | Non-text indicators and use of colour | 8 | 7 | 8 (k) | 8 (k) | Icon-plus-word states and corrected speaker/HUD marks work; seek extent remains a necessary low-contrast indicator (`msettings-setup`, `msettings-speakers`, `meetings-playing`). |
| 42 | Truncation and text accommodation | 8 | 6 | 7 (k) | 7 (k) | Essential configuration and People text now wrap; the new History title still truncates above its repeated full sentence (`main-advanced-min`, `mpeople-person-min`, `history-selected`). |
| 43 | Dialogs and modality | 9 | 7 | 8 (k) | 8 (k) | Both destructive fixtures now show clear alerts with separated Cancel/destructive actions and conventional placement (`advanced-limit-confirm`, `postprocessing-discard`; shared alert source). |
| 44 | HUD and overlay design | 8 | 8 | 7 (k) | 7 (k) | Upright text, elapsed time and contrasting lit meters are strong; custom command focus and inactive-source status still need polish (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 8 | 5 | 7 (k) | 7 (k) | Recording pulse and text shimmer are removed and panel fades honor Reduce Motion; the shared spinner still repeats unconditionally (`meetings-recording`, `meetings-processing`; `kit.rs`). |
| | **Mean of 45** | **8.4** | **7.5** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums, in column order: **377/45**, **336/45**, **349/45**, **358/45**. Guidy round 2's holistic ship score is **8/10**.

The verified improvements are substantial: sidebar destinations replace the global segmented control; meeting content tabs are quieter; playback no longer collides with its end time; category navigation returns at ordinary width; cloud credentials and compact catalogue choices are visible; essential form/model/voiceprint descriptions wrap; Usage uses a complete 2×2 compact summary; Debug Settings and Logs have separate views; the undo message floats; and the discard fixture finally shows its alert. The speaker-slider remainders and lit light-mode HUD bars now pass the measured contrast target. Source confirms static recording dots, static busy labels and Reduce Motion handling on the recorder-panel fade. The log's motion cleanup is incomplete because the shared spinner still animates without that check.

## 2. Regressions since round 1

1. **Minimum Models hides the downloads that were previously visible.** In `main-models-min`, round 1 showed Parakeet, the downloading Whisper Turbo and the paused Whisper Large in the first viewport. Round 2 shows the three recommendations and only part of Parakeet; Whisper Turbo/Large and their controls require scrolling. This is visible in both themes. Full names and explanations are an improvement, but even the short Dictation recommendation now spends an extra full band on size and Ready, with a large empty stretch between them. The loss of active-work visibility is avoidable. See ranked problem 2.

No other clear regression was established in the inspected evidence. General and Post Process require more scrolling after their controls stack, and People no longer shows every voiceprint at once; those are reasonable exchanges for readable text and a useful detail width. The new History heading repeats content and truncates, but it also replaces an uninformative heading, so I treat that as unfinished refinement rather than a net regression. The faint seek remainder is a remaining defect, not a newly introduced one.

## 3. Remaining problems, ranked

Paths here are relative to `crates/guidy-app/src/ui/` unless stated otherwise. **Blocks 9** identifies the five issues that still prevent my holistic sign-off. Other items are polish. Fixes below preserve existing destinations, commands, validation and defaults.

### 1. The seek range remains too faint

**Blocks 9.** The playing header has a legible playhead, filled portion and times, but the unplayed range almost disappears. The original flat pixels are `#ececea` against `#fafaf9` in light appearance, **1.13:1**, and `#2e2e2e` against `#181818` in dark appearance, **1.31:1**. This line is the signifier and extent of a seek control, rather than a decorative separator.

- Evidence: `meetings-playing`, `meetings-detail`, `meetings-detail-wide`; `player.rs::render_scrubber` paints the remainder with `tone.raised`. The History scrubber uses the same low-contrast base when audio is available, although that available-audio state is not captured here.
- Parameters: 3, 36, 37, 41.
- Fix: give seek controls a dedicated opaque track token, for example `#85857e` light / `#767676` dark. Keep a dark/light knob with a visible edge and distinguish the played portion without depending only on hue. Check the painted range and playhead at 0%, mid-playback and the end. Ordinary grouping hairlines need not become darker.
- Likely source: `pages/meetings/player.rs`, `pages/history.rs`, `design.rs`.

Do retain the successful indicator fixes: sampled speaker remainders are `#988e73` on white (**3.25:1**) and `#80755a` on `#222222` (**3.49:1**). The lit light HUD bars use `#8a6000` on white (**5.59:1**). Unlit bars are not the lit-state contrast failure reported in round 1; they are not being held to the same role.

### 2. Compact Models applies an expensive layout to simple recommendations

**Blocks 9.** The generic narrow-row branch gives every recommendation a separate size/action band. It preserves text, but pushes the active speech downloads out of the 720×480 first view. The short Dictation row does not need that much vertical separation. The three ready bundles occupy more attention than the work currently downloading.

- Evidence: `main-models-min` in both themes, compared directly with round 1; `main-models` and `main-models-wide` show a better balance.
- Parameters: 4, 7, 8, 12, 30, 31, 34, 39.
- Fix: give recommendation rows their own compact composition. Keep title and complete bundle text, then one short metadata line containing size and Ready. Use 4 pt related gaps and 8 pt vertical padding; aim for approximately 60–68 pt for ordinary compact recommendations, allowing growth for wrapped text. Avoid a 24 pt action band when the trailing content is only a status word. Keep real download controls on their own band when needed. Fit at least the downloading row and its action in the first compact viewport without reintroducing ellipses.
- Likely source: the recommendation-row builder in `pages/models.rs`, `model_card.rs::ModelRow` and its narrow branch.

### 3. Focus treatment still has known custom exceptions

**Blocks 9.** Shared segments and disclosures now have the requested reserved 2 pt border. That fix should stay. History's keyboard-focusable seek control still reserves only a 1 pt border, and meeting content tabs only recolour their 2 pt selected underline. HUD commands have hover styling without a comparable local focus treatment. These are source-backed styling gaps; I am not claiming the stills prove a lost-focus runtime bug.

- Evidence: `meetings-find`, `meetings-transcript`, `history-selected`, `recording-widget-expanded`; `pages/history.rs::HistoryPlayer::render`, `pages/meetings/detail.rs` tab strip, `recording_widget.rs::pill` and `pill_button`.
- Parameters: 6, 10, 17, 19, 38, 44.
- Fix: reuse the opaque theme ring and a reserved 2 pt outline with 2 pt clearance for existing focusable controls. Give tabs a focus indicator around the tab, distinct from its selected underline. Apply the same visual recipe to HUD commands wherever they already receive focus. Capture focused History seeking, tabs and HUD commands in both themes to check clipping. Any missing keyboard reachability belongs in the functional list below.
- Likely source: `design.rs`, `pages/history.rs`, `pages/meetings/detail.rs`, `recording_widget.rs`.

### 4. Some auxiliary controls still miss the shared hit-area target

**Blocks 9 against the council target.** Resets now explicitly use 24×24 pt, and meeting player/header icons use 28 pt. Remaining seek wrappers are only 16 pt high; model delete/cancel actions still use the small button variant without an explicit minimum. The design source itself documents small buttons at approximately 22 pt.

- Evidence: `meetings-playing`, `main-models-full`, `main-models-wide`; `player.rs::render_scrubber`, `history.rs` seek wrapper, `model_card.rs::delete_button` and `progress_line_with`.
- Parameters: 10, 15, 20, 38.
- Fix: make seek hit regions at least 24 pt high while keeping a centred 4 pt visible track. Give standalone trash/cancel/menu actions an explicit 24×24 pt minimum, 14–16 pt glyph and at least 4 pt spacing. Keep the new 28 pt player actions. Do not infer click bounds from the drawing alone; verify the resulting bounds once.
- Likely source: `pages/meetings/player.rs`, `pages/history.rs`, `model_card.rs`, relevant `download_control.rs` actions.

WCAG includes spacing exceptions for small controls. This finding is a specific miss against the council's simpler target, not a claim that every small model button fails WCAG.

### 5. The shared loader still runs an unconditional repeat animation

**Blocks 9.** The old recording pulse and shimmering label implementation are gone. However, `kit.rs::spinner` still creates an 800 ms repeating rotation and takes no Reduce Motion input. The processing header and list call it beside a named stage and a percentage/progress bar. Removing shimmer did not remove this repeated repaint path. I did not measure GPU cost.

- Evidence: `meetings-processing`; `pages/meetings/detail.rs` processing header and `pages/meetings/list.rs` call the shared helper. Onboarding and permission/settings work also have callers. The overlay's separate spinner already checks Reduce Motion, showing an inconsistent policy.
- Parameters: 28, 45.
- Fix: use a static stage glyph where percentage or live progress already communicates work. For genuinely indeterminate work, use a stable named state or make the existing loader respect Reduce Motion. Prefer removing the repeat where it adds no information. Keep real audio-level updates and brief purposeful transitions.
- Likely source: `kit.rs::spinner` and its UI callers; `overlay.rs::waveform` and its arming ripple are worth including in the same policy audit.

### 6. Model configuration still repeats identities and underexplains attention

**Polish.** AI settings names the default model in the header, the saved list and the catalogue; the saved list places the human name, provider and raw model ID in one band. A warning glyph next to the model name does not itself say what needs attention. The provider form below does explain the missing key, so the state is recoverable. The speech catalogue also repeats capability fragments and exposes terse `Q8_0` metadata as ordinary helper text in this debug-enabled fixture.

- Evidence: `msettings-ai`, `msettings-ai-min`, `msettings-ai-wide`, `main-models-full`.
- Parameters: 6, 29, 30, 31, 32, 40.
- Fix: retain one readable model name per row; keep exact IDs secondary and shorten redundant provider/default text. Use a short explicit status such as Key Needed beside the current-provider warning, reflecting the existing state. Preserve fields and commands. Where debug metadata is already exposed, label its meaning or give it a quieter details treatment within that same audience. This is a wording/layout fix, not a request to move credentials or change setting visibility.
- Likely source: `pages/meetings/ai_settings.rs::header` / `your_models`, `model_card.rs`, English locale strings.

### 7. History's new heading repeats and truncates the text immediately below it

**Polish.** The sentence-based title is more useful than Transcription, but the default split view cuts it at “dietary lis…” while the complete identical sentence appears just below. The compact view shows the full title because its header receives more width. It is a small presentation inconsistency, not lost transcript content.

- Evidence: `history-selected`, `history-selected-min`, `main-history`, both themes.
- Parameters: 4, 12, 32, 39, 40, 42.
- Fix: give the object heading a short natural excerpt ending at a word/sentence boundary, or allow the existing first sentence to wrap before moving Copy/More below it. Keep the complete transcript in the reading body and avoid making the same full paragraph a second heading. Do not introduce automatic summarization or a new naming command for this fix.
- Likely source: `pages/history.rs` detail header and title-excerpt builder.

### 8. Finish the small control, type and spacing conventions

**Polish.** The UI now has a clear system, but its small exceptions are still visible: the onboarding heading uses Geist while the app uses the system family; some helper/group roles differ only by 1 pt or weight; short selectors retain more width than their values need; shared 14 pt insets and 5/14 pt custom gaps sit outside the agreed spacing set.

- Evidence: `onboarding-welcome`, `main-general`, `main-advanced`, `msettings-recording`, `mpeople-person`; corroborated by `theme.rs`, `design.rs` and the assistant/row builders.
- Parameters: 1, 5, 6, 7, 8, 10, 15.
- Fix: use the system family for the assistant heading to meet the literal one-family target. Keep 12/16 helpers, 13 pt controls and 14/21 reading; reserve the existing 15/17/22 hierarchy for headings rather than adding more styles. Align custom gaps/insets to the nearest agreed 4/8-pt token where composition benefits. Use content-width selectors for short enumerations while retaining useful width for free text, paths and shortcut capture.
- Likely source: `onboarding/assistant.rs`, `theme.rs`, `design.rs`, `kit.rs`, shared selector builders.

### 9. Error blocks still repeat the failure in the next sentence

**Polish.** Startup now gives a plain cause, consequence and secondary selectable details. Storage and Usage have calmer centred recovery states, but “Storage can't be measured” is followed by “Couldn't measure storage…”, and Usage does the same. The second line repeats the headline before reaching the cause.

- Evidence: `storage-error`, `usage-error`, compared with improved `startup-error`.
- Parameters: 29, 32.
- Fix: let the heading name the failed task and the sentence start with the existing cause, for example The data folder couldn't be found or The usage database is locked. Retain Try Again and existing diagnostic content. No new recovery command or error classification is required to improve these shown states.
- Likely source: `pages/storage.rs`, `pages/usage.rs`, English locale strings.

## 4. What would raise each parameter still below 9

The remaining 17 parameters already score 9 on the available UI evidence. These 28 lines cover every lower score.

1. **Typography:** use the system family for assistant headings while retaining readable helper/body/reading sizes.
3. **Color contrast:** give seek extents a dedicated opaque ≥3:1 track and verify the final painted playhead/track states.
4. **Information hierarchy:** reduce compact recommendation weight so active downloads remain visible; refine the repeated History heading.
5. **Typographic hierarchy:** use the existing heading roles consistently and distinguish small group labels through a deliberate size/weight role.
6. **Design language:** apply the shared focus/target contract and quiet repeated technical model metadata.
7. **Padding and spacing:** compact recommendation metadata and bring custom 5/14 pt gaps/insets onto the agreed token grid where useful.
8. **Width and layout measure:** keep complete descriptions, shorten short-value selectors and recover compact model height without shrinking names.
10. **Overall consistency:** finish common seek, icon-action and custom-focus styling across History, Meetings, models and HUDs.
12. **Vertical rhythm:** eliminate redundant metadata bands in simple recommendations and avoid duplicating long History headings.
15. **Control styling and sizing:** retain bordered commands and finish 24–26 pt controls, explicit small-action bounds and content-width pop-ups.
17. **Interactive states:** give custom tabs/segments/HUD commands deliberate pressed and focus styling without conflating selection with focus.
19. **Focus visibility:** replace History's 1 pt seek focus border and tab-only underline with the same reserved 2 pt focus recipe.
20. **Target size:** raise 16 pt seek hit regions and remaining small icon actions to the agreed 24×24 pt contract.
28. **Loading and progress:** retain named work/percentages and remove redundant spinning glyphs beside progress.
29. **Error and status messaging:** name the AI attention condition and let error detail sentences begin with the cause.
30. **Data density:** restore active-download visibility at minimum size and reduce repeated model identity text.
31. **Scannability:** keep compact recommendation metadata together and make exact model IDs secondary to readable names.
32. **Microcopy and capitalization:** finish repeated errors/metadata, clarify the shortcut distinction only when its existing semantics are known, and use natural title excerpts.
34. **Minimum-size reflow:** preserve the repaired cloud/AI/People/Usage layouts while fitting an active model download above the fold.
36. **Dark-mode quality:** strengthen the necessary seek extent without increasing ordinary panel/hairline contrast.
37. **Light-mode quality:** strengthen seek tracks while keeping the successful speaker-slider and lit-HUD colours.
38. **Native platform fit:** finish shared focus and hit areas; retain the new source-list navigation and alert composition.
39. **Content-first focus:** give active downloads room and avoid a long duplicated History heading ahead of the reading body.
40. **Visual noise and restraint:** reduce repeated model/provider/default fragments and duplicated transcript-heading content.
41. **Non-text indicators and use of colour:** retain icon-plus-word statuses and corrected meters; finish visible seek extents/playheads.
42. **Truncation and text accommodation:** use a deliberate History title excerpt or wrap its full first sentence before actions take the width.
44. **HUD and overlay design:** retain the corrected meters and timer; finish the existing custom commands' visual focus/state contract.
45. **Motion and transitions:** remove redundant repeated loaders and apply one Reduce Motion policy to remaining indeterminate activity.

## 5. Functional findings, separate from visual scores

These are carry-forward verification questions unless stated otherwise. They are not asserted runtime defects and do not reduce the UI-only score.

1. **Shortcut semantics/precedence.** General still displays Right Option for both Dictation key and Transcribe shortcut. Establish their actual relationship before changing wording that could promise different behavior. Evidence: `main-general`. Moving debug-only timing controls into General would change their audience and is outside this refresh.
2. **Credential ownership.** Confirm which provider credentials are shared between Post Process, meeting summaries and speaker services. Consolidating stores or creating one credential home is out of scope. The UI can still improve wording within the current homes. Evidence: `main-postprocessing`, `msettings-ai`, `msettings-speakers`.
3. **Cloud/default readiness.** Finish remains available with an empty cloud key; a saved default AI model can lack a usable key. Verify the supported incomplete-setup outcome and downstream recovery. Do not change defaults or validation as part of this visual pass. Evidence: `onboarding-setup-cloud`, `msettings-ai`.
4. **Keyboard reachability and scrolling.** Verify custom HUD commands and the meeting scrubber are keyboard reachable, and check compact AI's outer/inner scrolling, category switching, focus retention and accessible names. Adding missing keyboard operation is a functional task; focus-indicator styling for existing focusable controls is covered above. Evidence: `recording-widget-expanded`, `meetings-playing`, `msettings-ai-min`; associated UI source.
5. **Retained-audio availability.** Verify retranscription/redo availability when History says Recording not found and after meeting audio expires. Evidence: `history-selected`, `msettings-general`. Command availability and retention defaults are outside this review's visual score.
6. **Summary generation during recording.** Verify the scope and later replacement behavior of Generate Summary during a live recording. Evidence: `meetings-recording-wide`.
7. **Alert and merge behavior.** Verify Esc cancellation, focus return, destructive confirmation/undo around person merge, and the supported default action for alerts. Enter behavior is explicitly outside refresh scope. Evidence: `advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`. The previous discard-fixture concern is resolved: the current fixture visibly reaches the alert.
