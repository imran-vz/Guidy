# Round 3 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.7/10 overall and 8/10 to ship**. The shared controls now look substantially more consistent. The seek range is visible, compact Models exposes the active download, History starts directly with its content, and focused meeting tabs have a separate outline. I would ship these improvements. I would withhold the requested 9 because minimum-size AI settings has regressed to showing only part of its first catalogue choice.

## Evidence and scoring method

I inspected both themes across the 101 supplied states, using in-memory contact sheets and original PNGs. Coverage included every main section at default and minimum size, all supplied wide variants, meeting playback/finished/focused/processing/recording states, every meeting-settings category, People, onboarding, startup, errors, alerts, Debug Logs and both HUDs. Tall captures were inspected in lower-content crops, including the bottom of Advanced and Models. Originals or larger crops were used for compact Models and AI, playing Meetings and the expanded recorder. Contrast measurements below use original PNG pixels, not compressed contact sheets.

Direct round-2 comparisons covered compact AI and Models, playing Meetings, History, minimum General, the expanded recorder and startup. Round-0 comparisons covered compact Models, meeting detail and cloud setup. I read my round-0, round-1 and round-2 reports for calibration and inspected all three supplied reference images. Codex and Claude retain their previous reference scores; T3 Code, Xcode and System Settings inform the comparison without adding score columns.

The round log was treated as a claim to verify. Read-only source checks covered tokens, shared controls, model/download rows, AI settings, the sidebar/startup sidebar, History, meeting playback/tabs and HUDs. Only this report was written. No source was edited, no Cargo command or live app was run, and no other reviewer's report was read.

Snapshot stems refer to `target/ui-review/round-3/<stem>-{light,dark}.png` unless a theme or previous round is stated. A bare stem cites both themes. Captures are 2×. Missing traffic lights, blank shell areas in isolated pane fixtures, opaque HUD capture backgrounds and unused space in tall fixtures are harness conventions. I do not treat those as defects.

Scores 17, 19, 20 and 45 are provisional judgments from stills and the identified source. They do not certify keyboard reachability, hit testing, assistive technology, scrolling, performance or every interaction state. Missing runtime evidence is not itself a failure. Reference scores marked `(k)` use product knowledge beyond the single supplied screenshot. The published round-2 scores are reproduced unchanged.

The UI-only boundary applies throughout. Credential-store ownership, setting exposure, defaults, validation, command availability and missing keyboard operation belong in functional findings. Parameters 21 and 22 judge the presentation of the existing destinations and settings.

## 1. Scores

| # | Parameter | Guidy round 3 | Guidy round 2 | Codex | Claude | Reason for Guidy round 3 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 8 | 8 | 8 | The assistant now uses the system family; controls, helpers and reading text retain readable roles (`onboarding-welcome`, `main-general`, `meetings-detail`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Semibold identifies headings and speakers without turning prose heavy (`main-general`, `meetings-transcript`). |
| 3 | Color contrast | 8 | 8 | 7 | 7 | Seek tracks now pass; shared progress remainders still fade to about 1.6–1.7:1 (`meetings-playing`, `meetings-processing`, `main-models-min`). |
| 4 | Information hierarchy | 8 | 8 | 8 | 9 | Models gives active work room, but minimum AI puts scaffolding above virtually every catalogue choice (`main-models-min`, `msettings-ai-min`). |
| 5 | Typographic hierarchy | 9 | 8 | 8 | 8 | The 15 pt group headings now separate clearly from 13 pt rows and 12 pt helpers (`main-general`, `main-models`, `msettings-recording`). |
| 6 | Design language | 9 | 8 | 8 | 8 | System headings, filled playback glyphs and common control geometry join settings, reading panes and onboarding (`onboarding-picker`, `mpeople-person`, `meetings-playing`). |
| 7 | Padding and spacing | 8 | 8 | 8 | 9 | Most gaps are regular; 14 pt panel insets and custom 5/6 pt gaps still miss the agreed token set (`main-general`, `msettings-ai`, `meetings-tab-focus`; source). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Form/catalogue caps work; the AI status and help/link composition wrap inefficiently at minimum width (`main-models-wide`, `msettings-ai-wide`, `msettings-ai-min`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Necessary checkbox/radio marks use accent ink and selection stays neutral in both themes (`onboarding-welcome`, `msettings-ai`, `main-general`). |
| 10 | Overall consistency | 9 | 8 | 8 (k) | 8 (k) | Standard controls, fixed reset slots, list selections and filled playback symbols now follow common rules (`main-general`, `main-debug`, `mpeople-person`, `meetings-playing`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Short pop-ups hug their values while reset positions remain fixed (`main-general`, `main-debug`, `main-postprocessing`). |
| 12 | Vertical rhythm | 8 | 8 | 8 | 8 | Compact recommendations are repaired; AI's stacked header, groups and provider help still consume too much height (`main-models-min`, `msettings-ai-min`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Meeting settings, provider controls, notes and the in-place summary action have clear relationships (`meetings-settings`, `msettings-ai`, `meetings-recording-wide`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Filled play/pause/stop marks agree, speaker names lead their metadata, and ordinary actions remain recognizable (`mpeople-person`, `meetings-playing`, `recording-widget-expanded`). |
| 15 | Control styling and sizing | 9 | 8 | 8 (k) | 8 (k) | Ordinary fields/buttons use 26 pt geometry; small model icon actions have explicit 24 pt bounds (`main-general`, `main-models-full`, `msettings-linear`; source). |
| 16 | Affordance and signifiers | 8 | 9 | 7 | 8 | Named History commands improve recognition; compact AI search no longer leads to a complete visible choice (`history-selected`, `msettings-ai-min`). |
| 17 | Interactive states | 8 | 8 | 8 (k) | 8 (k) | Selected, disabled, busy and focused examples are distinct; custom HUD commands still define hover without local pressed feedback (`meetings-tab-focus`, `msettings-linear`, `recording-widget-expanded`; source). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Sidebar, child destinations and lists share selection tokens; source retains focused/unfocused variants (`meetings-people`, `history-selected`, `mpeople-person`). |
| 19 | Focus visibility | 9 | 8 | 7 (k) | 7 (k) | The new tab outline is distinct from selection; History seeking now reserves a 2 pt border (`meetings-tab-focus`, `meetings-find`, `history-selected`; source). |
| 20 | Target size | 9 | 8 | 7 (k) | 7 (k) | Source gives seeking 24 pt regions, model icon actions 24 pt and meeting tools 28 pt (`meetings-playing`, `main-models-full`, `meetings-transcript`). |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Meeting settings is explicitly scoped, Meeting notes avoids the General collision, and compact details retain clear Back paths (`meetings-settings`, `mpeople-person-min`, `meetings-detail-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Existing library/preferences/app clusters and named meeting categories communicate their scope (`main-general`, `meetings-settings`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Recording no longer duplicates its title; toolbar commands stay in a small named group (`meetings-recording`, `meetings-detail-min`, `debug-logs`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar, groups and transient surfaces separate without nested note cards (`main-general`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Group/pane hairlines remain restrained while actionable fields have clear boundaries (`main-general`, `meetings-detail`, `main-postprocessing`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Controls, groups, alerts and HUD capsules retain their established shape family (`main-models`, `postprocessing-discard`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | One meeting headline and sentence point to the sole Record action; summary generation sits beside its empty content (`meetings-empty`, `meetings-recording-wide`). |
| 28 | Loading and progress | 9 | 8 | 8 (k) | 8 (k) | Determinate processing uses a static stage mark and percentage; startup names its work (`meetings-processing`, `startup-app`, `startup-new`). |
| 29 | Error and status messaging | 9 | 8 | 7 (k) | 7 (k) | AI names the missing provider key; Storage/Usage sentences now begin with the cause (`msettings-ai`, `storage-error`, `usage-error`, `startup-error`). |
| 30 | Data density | 8 | 8 | 8 | 8 | Models and History improve, but minimum AI loses its usable initial catalogue view (`main-models-min`, `history-selected-min`, `msettings-ai-min`). |
| 31 | Scannability | 8 | 8 | 8 | 9 | Model state/size/action columns are stable; repeated saved-model IDs still distract and compact AI barely exposes its list (`main-models`, `msettings-ai`, `msettings-ai-min`). |
| 32 | Microcopy and capitalization | 9 | 8 | 8 (k) | 8 (k) | History has named commands, model capability repetition is reduced, and error wording is direct (`history-selected`, `onboarding-picker`, `storage-error`, `usage-error`). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Usage labels/units survive compact layout, catalogue price basis is stated, and times are stable (`main-usage-min`, `msettings-ai`, `meetings-played`). |
| 34 | Minimum-size reflow (720×480) | 8 | 8 | 8 (k) | 8 (k) | Models recovers active-download visibility and forms keep their text; AI catalogue discovery regresses (`main-models-min`, `main-postprocessing-min`, `msettings-ai-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms/catalogues keep useful measures while meetings gain two reading panes (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`). |
| 36 | Dark-mode quality | 9 | 8 | 8 | 8 | The corrected seek range and bright state marks work on lifted neutral surfaces without glare (`meetings-playing-dark`, `msettings-ai-dark`, `main-general-dark`). |
| 37 | Light-mode quality | 9 | 8 | 8 (k) | 8 (k) | Seek range, checked controls and HUD activity now have strong light-mode ink (`meetings-playing-light`, `onboarding-welcome-light`, `recording-widget-expanded-light`). |
| 38 | Native platform fit | 9 | 8 | 7 (k) | 7 (k) | Source lists, common controls, bounded toolbar commands and destructive alerts form a convincing Mac settings app (`main-general`, `meetings-settings`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 8 | 8 | 8 | 9 | History and Models start with useful material; compact AI still makes its current choice subordinate to scaffolding (`history-selected`, `main-models-min`, `msettings-ai-min`). |
| 40 | Visual noise and restraint | 8 | 8 | 7 | 8 | History duplication is gone; AI repeats the selected identity in status, saved models and catalogue (`history-selected`, `msettings-ai`). |
| 41 | Non-text indicators and use of colour | 8 | 8 | 8 (k) | 8 (k) | Seek extents, state words and check marks improve; shared progress remainders remain unnecessarily faint (`meetings-playing`, `msettings-setup`, `meetings-processing`). |
| 42 | Truncation and text accommodation | 8 | 8 | 7 (k) | 7 (k) | Essential settings and History detail wrap; the new compact startup sidebar cuts Meeting settings while the real sidebar fits it (`main-advanced-min`, `history-selected-min`, `startup-app-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Both alerts retain clear consequence text, separated cancellation and a destructive action (`advanced-limit-confirm`, `postprocessing-discard`). |
| 44 | HUD and overlay design | 9 | 8 | 7 (k) | 7 (k) | Upright text, elapsed time, contrasting activity and filled pause/stop commands are glanceable (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 8 | 7 (k) | 7 (k) | Determinate stages are static and the shared loader now checks Reduce Motion; panel transitions remain guarded (`meetings-processing`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.7** | **8.4** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums, in column order: **391/45**, **377/45**, **349/45**, **358/45**. Guidy round 3's holistic ship score is **8/10**. That score still falls short of sign-off at 9 because the compact AI decision surface needs another layout pass. Most remaining findings are polish rather than broad structural faults.

### Check of my five round-2 blockers

1. **Seek contrast: fixed.** Original playing-track pixels are now `#8f8f8a` on `#fafaf9`, **3.11:1** light, and `#707070` on `#181818`, **3.59:1** dark. `meetings-detail`, `meetings-playing` and `meetings-played` show the start, near-start and completed positions. The completed fill and knob remain distinguishable. This is separate from the shared progress issue below.
2. **Compact Models: fixed.** `main-models-min` now contains all three complete recommendation descriptions, Parakeet and the downloading Whisper Turbo row, including its cancel control. Size and Ready share the title band instead of occupying a separate band. The paused row is below the fold, but active work is visible again.
3. **Custom focus styling: fixed for the demonstrated existing focusable controls.** `meetings-tab-focus` shows the outline around Action Items while Notes retains its selection underline. Source gives History seeking a reserved 2 pt border. The recorder's custom clickable divs still need a keyboard reachability check; I list that as functional work, rather than indefinitely penalizing focus styling for an operation the evidence does not show they support.
4. **Auxiliary target sizes: fixed in the inspected source.** Both seek wrappers are 24 pt high; model cancel/More actions explicitly use 24×24 pt and meeting tools 28 pt. Actual runtime hit testing remains unverified.
5. **Unconditional shared loader: fixed.** `kit.rs::Spinner::render` returns a static glyph for Reduce Motion. Determinate processing uses a static stage glyph instead of a redundant spinner. Remaining indeterminate spinners are conditional on real busy states. This meets the remedy offered in my round-2 report; I am not introducing a new requirement to remove every useful indeterminate indicator. The unused `sheen` definition is not evidence of visible motion, and the overlay caret is not the old decorative recording pulse.

Other round-2 requests also landed: assistant headings use the system family; reset positions align while short pop-ups hug; History removes the repeated title; default labeling is quieter; and Storage/Usage stop repeating the failed task in their second sentence. The round log's blanket claim about opaque progress remainders does not hold for `design::track`, which still applies opacity.

## 2. Regressions since round 2

**Minimum AI settings loses its complete initial catalogue choices.** In round 2, `msettings-ai-min` showed all of Claude Opus 4.1 and Claude Sonnet 4.5, including their secondary metadata. Round 3 shows only the top of Opus: its name is visible, but the row's lower metadata and the next choice are below the viewport. Both themes show the same loss.

The catalogue begins at approximately image y=847 in round 3, versus y=763 in round 2, a **42 pt downward shift** at 2×. The more explicit status wraps, headings and controls consume more height, and the key-storage sentence wraps beside a long nonshrinking link. The new status is clearer, but the composition needs to recover the lost space. This is an initial-viewport regression, not a claim that scrolling cannot reach the list.

No other clear net regression was established. The compact startup sidebar's new truncated label is a new polish defect within an otherwise more informative skeleton; its old counterpart showed placeholders rather than a complete label. Faded progress remainders and HUD pressed-state gaps are carry-forward findings, not new regressions.

## 3. Remaining problems, ranked

Paths below are relative to `crates/guidy-app/src/ui/`. **Blocks 9** identifies the remaining holistic sign-off blocker. All proposed fixes preserve settings visibility, commands, stores, defaults and validation.

### 1. Compact AI settings again sacrifices its catalogue

**Blocks 9.** The current 720×480 composition presents a status band, saved-model group, provider form, help/link band, search and price heading before any complete model choice. A visible search field with a cropped first result gives the comparison task too little initial space.

- Evidence: `msettings-ai-min` in both themes, directly compared with round 2; `msettings-ai` shows the same hierarchy with more room and `msettings-ai-wide` shows that the row design itself is readable.
- Parameters: 4, 8, 12, 16, 30, 31, 34, 39.
- Fix: recover roughly 70–90 pt above the catalogue at minimum height. Keep the explicit missing-key wording, but give it a short full-width line rather than repeating the complete selected identity beside the fixed-width place switch. Use 12 pt related group gaps, compact saved-model row padding, and a shorter key-help link such as Get API Key so the storage sentence does not wrap into a second line. Reserve about 150 pt for search, the price basis and at least two complete catalogue choices. Keep the current commands and all explanatory text available within the existing scrolling pane.
- Likely source: `pages/meetings/ai_settings.rs::header`, `provider_form`, `your_models`, `render` and the `BLOCK_GAP`/catalogue allocation.

### 2. Shared progress remainders still fade the corrected track token

**Polish; misses the council's indicator target.** Seeking now uses the opaque token correctly. `design::track` still paints `t.track.opacity(0.45)`, so downloading and processing retain a weaker progress dialect. On the original compact Models panel the remainder is `#cdcdca` on white, **1.59:1** light, and `#454545` on `#222222`, **1.66:1** dark. Processing gives **1.57:1** and **1.71:1** on its canvas.

- Evidence: `main-models-min`, `main-models-full`, `meetings-processing`, both themes; contrast from flat original pixels and corroborating source.
- Parameters: 3, 41. The low-contrast instances are counted here rather than holding both whole-theme scores down again after their specific seek defect was fixed.
- Fix: remove the 0.45 opacity at this call site and use the existing opaque track token, retaining the 4 pt geometry and restrained fill. Recheck the result on white panels, dark panels and the canvas. Do not darken decorative grouping hairlines.
- Likely source: `design.rs::track`; used by `download_control.rs` and meeting processing views.

The percentage text remains readable and redundant with the bar. This is a miss against the council's explicit track target and a consistency issue, not a claim that the progress value is inaccessible or that every redundant progress illustration independently violates WCAG.

### 3. Custom HUD commands stop at hover styling

**Polish.** The expanded recorder is clear at rest. Its Pause, Stop and Open Notes pills use a custom `Div` builder with hover styling, but no local active/pressed treatment. The compact pause/stop builder has the same omission. Shared app buttons now have a fuller visual state contract.

- Evidence: `recording-widget`, `recording-widget-expanded`, both themes; `pill` and `pill_button` source defines the styling. Stills alone do not demonstrate the missing pressed phase.
- Parameter: 17. HUD legibility itself now earns 9 in parameter 44.
- Fix: add a static pressed background or inset edge to these existing pointer commands, keeping the current footprint and labels. Reserve an outline for any command that already receives focus; do not disguise missing keyboard operation with a painted ring. Check the actual pressed state in both themes before declaring the full state set complete.
- Likely source: `recording_widget.rs::pill`, `pill_button` and their theme/black-chrome tones.

Keyboard operation is a separate functional finding below. This item does not ask the visual pass to add a new input path.

### 4. The selected AI identity is still repeated as ordinary content

**Polish.** The same Claude selection appears in the status, Your models and catalogue. The saved row displays Claude Sonnet 4.5, Anthropic and the near-identical `claude-sonnet-4-5` ID together. The default badge is no longer repeated in the catalogue, and the duplicate gpt-4.1 ID is gone, so this is a smaller issue than in round 2.

- Evidence: `msettings-ai`, `msettings-ai-min`, `msettings-ai-wide`, both themes. Debug-enabled speech rows also still show terse `Q8_0` metadata in `main-models-full`.
- Parameters: 30, 31, 40.
- Fix: let the status lead with the condition, the saved list with the readable identity, and the catalogue with comparison data. Keep exact IDs visually subordinate to those roles, with a separate secondary line or existing detail/tooltip presentation when they differ meaningfully. Label debug quantization metadata in plain language within its existing audience. Do not change which users can see debug data or consolidate credentials.
- Likely source: `pages/meetings/ai_settings.rs::header`, `your_models`, `model_list`; `model_card.rs` and the shared model-description helper.

### 5. The compact startup sidebar does not match the real sidebar's text budget

**Polish.** `startup-app-min` shows Meeting sett… while ordinary compact pages show Meeting settings in full. The skeleton now uses the real labels, icons and clusters, but retains the full-width child indent and trailing padding in its narrow branch.

- Evidence: `startup-app-min` versus `main-general-min` and `main-models-min`, both themes; `SidebarSkeleton` uses 34 pt child indentation and 8 pt trailing padding, while the real compact sidebar uses 24 pt and 4 pt.
- Parameter: 42. The eventual destination remains clear once startup finishes, so I do not lower navigation scoring for this transient defect.
- Fix: use the existing compact indentation and trailing padding in the startup rows. Keep the label and sidebar width unchanged; verify the full destination fits both themes at 720 pt.
- Likely source: `onboarding/startup.rs::SidebarSkeleton`, matching `main_window.rs::render_sidebar`.

### 6. A few shared and custom gaps still sit outside the agreed grid

**Polish.** The general rhythm is sound. The remaining mismatch is specific: `SECTION_X` is 14 pt, AI provider rows use 6 pt padding, and meeting tab labels/counts use a 5 pt gap. These are not severe visual defects, but they prevent a literal 9 against parameter 7's token target.

- Evidence: `main-general`, `msettings-ai`, `meetings-tab-focus`, corroborated by the corresponding constants/builders.
- Parameter: 7.
- Fix: use 12 or 16 pt panel insets, 4 or 8 pt custom padding and 4 pt label/count gaps where they preserve alignment. Make this a small token normalization pass after the compact AI allocation is repaired; avoid inflating short panes merely to fit a spacing rule.
- Likely source: `design.rs::SECTION_X`, `pages/meetings/ai_settings.rs`, `pages/meetings/detail.rs` tab builder.

## 4. What would raise each parameter still below 9

These 14 lines cover every lower score. The other 31 parameters score 9 on the available UI evidence.

- **3. Color contrast:** retain the corrected opaque seek range and remove the shared progress track's 0.45 opacity.
- **4. Information hierarchy:** put complete catalogue choices back into minimum AI's initial viewport.
- **7. Padding and spacing:** normalize the remaining 14 pt insets and 5/6 pt gaps to the agreed tokens without growing compact scaffolding.
- **8. Width and layout measure:** give AI status/help enough width to avoid unnecessary wrapping beside fixed controls and links.
- **12. Vertical rhythm:** shorten the compact AI stack with related 12 pt gaps and compact saved-model rows.
- **16. Affordance and signifiers:** show at least two complete choices directly below minimum AI's visible search/filter controls.
- **17. Interactive states:** give custom recorder commands deliberate static pressed feedback; keep keyboard reachability separate.
- **30. Data density:** restore a useful initial AI catalogue view and reduce repeated identity text.
- **31. Scannability:** keep readable names primary and move near-duplicate exact IDs out of the ordinary comparison band.
- **34. Minimum-size reflow:** recover roughly 70–90 pt above the compact AI catalogue while preserving every existing control and explanation.
- **39. Content-first focus:** let the model choice occupy more of minimum AI than the preceding status/form scaffolding.
- **40. Visual noise and restraint:** retain History's cleanup and stop repeating the selected AI identity across adjacent regions.
- **41. Non-text indicators and use of colour:** use the opaque track token for progress remainders as well as seeking.
- **42. Truncation and text accommodation:** apply the real compact sidebar's indentation/padding to the startup sidebar so Meeting settings fits.

## 5. Functional findings, separate from visual scores

These remain verification questions unless explicitly described as source evidence. None reduces the UI-only score.

1. **Shortcut semantics and setting exposure.** General still assigns Right Option to Dictation key and Transcribe shortcut. Confirm their activation/precedence before promising a behavioral distinction in copy. Moving debug-only timing controls into General would change their audience and remains outside this refresh. Evidence: `main-general`, `main-debug`.
2. **Credential ownership.** Establish which credentials are shared between Post Process, meeting summaries and speaker services before considering one credential home. The current wording improvement does not establish a shared underlying store. Evidence: `main-postprocessing`, `msettings-ai`, `msettings-speakers`.
3. **Incomplete cloud/default setup.** Finish remains available with an empty cloud key; AI can show a saved default and an explicit missing-key status. Verify the intended incomplete-setup outcome and downstream recovery. Validation/default changes are outside this review. Evidence: `onboarding-setup-cloud`, `onboarding-setup-cloud-min`, `msettings-ai`.
4. **Keyboard reachability, focus retention and scrolling.** The inspected meeting scrubber and recorder pill builders primarily define pointer interaction; their complete keyboard operation is not established here. Verify those flows, accessible names, compact AI's outer/inner scrolling, category switching and focus retention. Adding missing input behavior is functional work. Existing focusable-control styling receives credit above. Evidence: `meetings-playing`, `recording-widget-expanded`, `msettings-ai-min`; their UI source.
5. **Audio availability and retention.** Verify retranscription availability when a recording is missing and after retained meeting audio expires. Command availability and retention defaults are outside the visual pass. Evidence: `history-selected`, `msettings-general`.
6. **Summary generation during recording.** Verify what Generate Summary covers during a live recording and how later generation replaces or preserves it. Its new placement is visually appropriate but does not settle behavior. Evidence: `meetings-recording-wide`.
7. **Alert and merge behavior.** Verify Esc cancellation, focus return, merge consequences/undo and the supported default alert action. Enter behavior remains out of scope. Both destructive fixtures visibly show their alerts, so the former discard-capture concern remains resolved. Evidence: `advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`.
