# Round 4 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **9.0/10 overall and 9/10 to ship**. I would sign off on this UI-only refresh. The minimum-size AI catalogue now shows two complete choices, the progress remainders are visible, and compact library navigation is consistent. The remaining findings are polish. None blocks a holistic 9 on the supplied evidence.

## Evidence and scoring method

I read the council brief, including both scope sections, the agreed rubric, the round task, the round log and my reports from rounds 0–3. I inspected both themes across the 112 supplied snapshot stems, 224 PNGs, through in-memory overviews and content crops. Coverage includes every main section at default and minimum size, all supplied wide variants, meeting playback/recording/processing, People and its new shell fixtures, every meeting-settings category, onboarding, startup, errors, both alerts and both HUDs. Tall captures were inspected through their content rather than scaled as entire 4800-pixel images. Larger views checked compact AI, progress, sliders, General pop-ups, focused controls and HUDs.

Direct round-3 comparisons covered compact/default AI, General, compact Models and History, compact startup, processing and the Debug slider readout. Baseline comparisons covered compact Models, meeting detail and compact cloud setup. I inspected all three reference PNGs. Codex and Claude keep my previous reference scores; T3 Code, Xcode and System Settings inform the judgment without adding score columns. `(k)` marks a reference estimate from product knowledge beyond its single supplied image. These are the same estimates as round 3, not a fresh audit of those products.

Source checks were read-only and limited to UI tokens, shared controls, AI settings, startup, the sidebar, model/download presentation, Usage, onboarding and HUD states. No source was edited, no Cargo command or live app was run, and no other reviewer's report was read. Only this report was written in the repository. The humanizer skill was applied to the report's wording.

Bare snapshot stems cite `target/ui-review/round-4/<stem>-{light,dark}.png`; a theme or earlier round is stated where needed. Captures are 2×. Missing traffic lights, blank shell regions in isolated fixtures, opaque HUD capture backgrounds and unused tall-capture space are harness conventions. They are excluded from the findings.

Interaction scores remain provisional judgments from stills and the identified source. They do not establish actual hit testing, VoiceOver behavior, every hover/pressed phase, scroll behavior, focus retention or frame cost. The supplied focus sequence contains eight stems: `focus-1`, `focus-2`, `focus-3`, `focus-4`, `focus-5`, `focus-6`, `focus-8` and `focus-10`. I do not infer coverage of missing numbers or claim that every control is shown focused.

The scope clarification accepts the stock control's opaque 1 pt focus edge. I do not deduct for its difference from the custom 2 pt outline, or ask for a gpui-kit patch. The idle status bar and Check for updates entry point remain outside the permitted removal/hiding work. Functional questions about settings exposure, credential stores, defaults, validation, command availability and missing keyboard operations are listed separately. Round-3 scores are reproduced unchanged.

## 1. Scores

| # | Parameter | Guidy round 4 | Guidy round 3 | Codex | Claude | Reason for Guidy round 4 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | System UI, 12 pt helpers, 14 pt reading text and code-font IDs have distinct uses (`main-general`, `meetings-detail`, `main-usage`, `onboarding-welcome`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Headings and speakers use restrained semibold; explanations remain regular (`main-general`, `meetings-transcript`, `main-advanced-full`). |
| 3 | Color contrast | 9 | 8 | 7 | 7 | Original progress remainders now reach 3.25:1 on light panels and 3.21:1 on dark panels; seeking remains clear (`main-models-min`, `meetings-processing`, `meetings-playing`). |
| 4 | Information hierarchy | 9 | 8 | 8 | 9 | Compact AI exposes complete choices; Models retains active work and History starts with its selected text (`msettings-ai-min`, `main-models-min`, `history-selected-min`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | 17 pt page titles, 15 pt group headings and 13 pt rows now establish the agreed two-point steps (`main-general`, `main-debug`, `meetings-settings`; tokens). |
| 6 | Design language | 9 | 9 | 8 | 8 | Settings, scoped meeting panes, alerts and the deliberate HUD exception share a coherent vocabulary (`main-general`, `msettings-recording`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 8 | 8 | 9 | The reported 14 pt panel inset and custom AI/tab gaps are corrected; regular groups keep their cadence (`main-general`, `msettings-ai`, `meetings-tab-focus`; source). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Wide forms and catalogues keep useful measures, but short settings pop-ups now occupy the full 200 pt column (`main-models-wide`, `msettings-ai-wide`, `main-general`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and surfaces support a restrained accent and separate recording/error colours (`msettings-ai`, `meetings-recording`, `main-general`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Back paths, reset slots, transport silhouettes and ordinary controls follow common rules (`meetings-people-person-min`, `history-selected-min`, `meetings-playing`, `main-debug`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Control and reset columns align; the sound preview has its own position outside the reset slot (`main-general`, `main-debug`, `main-postprocessing`). |
| 12 | Vertical rhythm | 9 | 8 | 8 | 8 | Compact saved rows and a separate status line recover the AI catalogue without compressing its names (`msettings-ai-min`, `msettings-ai`, `main-models-min`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Provider requirements, settings categories and reading sections have clear common regions and proximity (`msettings-ai`, `meetings-settings`, `meetings-detail`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Filled transport glyphs agree; the local-meetings laptop and caution-badged app icon communicate their roles (`main-models-min`, `meetings-playing`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 9 | 8 (k) | 8 (k) | 26 pt controls and bounded auxiliary actions are consistent, but fixed 200 pt short-value pop-ups miss the content-sized target (`main-general`, `main-advanced`, `main-debug`). |
| 16 | Affordance and signifiers | 9 | 8 | 7 | 8 | Search leads to visible model choices; compact details have named commands and an explicit return path (`msettings-ai-min`, `history-selected-min`, `meetings-people-person-min`). |
| 17 | Interactive states | 9 | 8 | 8 (k) | 8 (k) | Selected, disabled, busy and focused examples are distinct; HUD builders now define static active fills (`focus-2`, `focus-4`, `msettings-linear`, `recording-widget-expanded`; source). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | The sidebar and libraries use consistent neutral selection, with stronger list focus and separate catalogue membership checks (`focus-1`, `main-history`, `meetings-people`, `msettings-ai`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | The supplied keyboard sequence shows opaque pop-up, reset and shortcut edges; custom tab focus remains separate from selection (`focus-2`, `focus-3`, `focus-4`, `focus-10`, `meetings-tab-focus`). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Existing 24 pt reset/model/seek bounds and 28 pt meeting tools are retained; HUD commands have explicit footprints (`main-models-full`, `meetings-playing`, `recording-widget-expanded`; source). |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Compact People, History and Meetings now place Back in the leading toolbar; settings categories remain explicitly scoped (`meetings-people-person-min`, `history-selected-min`, `meetings-detail-min`, `meetings-settings`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | The existing destination clusters and meeting categories communicate their intent within the UI-only boundary (`main-general`, `meetings-settings`, `msettings-ai`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Titles and small action groups recede around the content; compact Back is consistent across libraries (`meetings-detail-min`, `meetings-people-person-min`, `history-selected-min`, `debug-logs`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar and panels separate in both themes; category navigation now shares the sidebar tone (`main-general`, `meetings-settings`, `advanced-limit-confirm`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines separate rows and reading panes without enclosing every paragraph; necessary controls have stronger edges (`main-general`, `meetings-detail-wide`, `main-postprocessing`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Chips, controls, grouped panels, alerts and HUD capsules retain their small shape family (`main-models`, `advanced-limit-confirm`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | The empty library has one message referring to Record; missing summaries place generation beside the content (`meetings-empty`, `meetings-recording-wide`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Named startup and processing states remain stable; the selected processing meeting no longer repeats a percentage in the list (`startup-app`, `startup-new`, `meetings-processing`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | AI names its missing key, startup states the permission cause, and Storage/Usage offer direct recovery (`msettings-ai-min`, `startup-error-min`, `storage-error`, `usage-error`). |
| 30 | Data density | 9 | 8 | 8 | 8 | Compact AI shows two complete choices, saved models lose redundant IDs, and compact speech downloads remain visible (`msettings-ai-min`, `main-models-min`, `history-selected-min`). |
| 31 | Scannability | 9 | 8 | 8 | 9 | Names, quantities, states and actions retain stable positions; saved AI identities are quieter (`main-models`, `msettings-ai`, `main-usage`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Quantization is labeled, the migration action is singular-aware, and the shorter key link avoids needless wrapping (`main-models-min`, `onboarding-migration`, `msettings-ai-min`). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Slider units stay on one line, costs and counts align, and AI prices state their basis (`main-debug-full`, `main-usage-min`, `msettings-ai`, `meetings-played`). |
| 34 | Minimum-size reflow (720×480) | 9 | 8 | 8 (k) | 8 (k) | The former compact AI blocker is fixed; compact libraries and setup preserve useful primary content (`msettings-ai-min`, `main-models-min`, `history-selected-min`, `onboarding-setup-cloud-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Forms keep their measure, wide reading gains a second pane, and the meeting list gains useful width (`main-models-wide`, `msettings-ai-wide`, `meetings-detail-wide`, `meetings-people-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted neutral surfaces, legible progress extents and restrained bright marks work together (`main-general-dark`, `main-models-min-dark`, `meetings-playing-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Light controls, focus edges and progress remainders now remain visible against their real surfaces (`focus-2-light`, `main-models-min-light`, `meetings-processing-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source lists, grouped preferences, leading Back and conventional alert composition read as a Mac app (`main-general`, `meetings-settings`, `meetings-people-person-min`, `advanced-limit-confirm`). |
| 39 | Content-first focus | 9 | 8 | 8 | 9 | Model choice gets a usable compact viewport, and reading starts directly beneath bounded chrome (`msettings-ai-min`, `history-selected-min`, `meetings-detail`). |
| 40 | Visual noise and restraint | 9 | 8 | 7 | 8 | Saved AI rows lose near-duplicate exact IDs and processing shows its detailed progress once (`msettings-ai`, `meetings-processing`, `history-selected`). |
| 41 | Non-text indicators and use of colour | 9 | 8 | 8 (k) | 8 (k) | Checks, state words, seek extents and opaque progress remainders provide redundant meaning in both themes (`msettings-ai`, `meetings-playing`, `main-models-min`, `meetings-processing`). |
| 42 | Truncation and text accommodation | 9 | 8 | 7 (k) | 7 (k) | The compact startup label now fits; primary form/model text wraps and selected History text is complete (`startup-app-min`, `main-advanced-min`, `main-models-min`, `history-selected-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Both destructive fixtures show readable consequences, separated cancellation and an explicit destructive action (`advanced-limit-confirm`, `postprocessing-discard`). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | The compact timer, filled transport symbols and upright expanded transcript remain glanceable (`recording-widget`, `recording-widget-expanded`, `overlay-live`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Determinate work uses static progress and stage marks; the loader and panel transitions retain Reduce Motion handling (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **9.0** | **8.7** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums in column order are **403/45**, **391/45**, **349/45** and **358/45**. Round 4's unrounded mean is 8.9556. There are 43 scores of 9 and two scores of 8. The holistic ship score is **9/10**. A 9 permits the small refinements below; it does not mean every control has been certified through runtime testing.

### Check of the round-3 blocker and polish items

1. **Compact AI catalogue: fixed.** Both `msettings-ai-min` images show complete Claude Opus 4.1 and Claude Sonnet 4.5 rows, including IDs/capability metadata and price/context values. The condition gets its own line, the provider switch shares the heading band, saved rows are compact, and Get API Key leaves the storage sentence on one line. This directly repairs my round-3 blocker. The catalogue is still scrollable; two visible choices are sufficient for the initial view requested in my previous report.
2. **Shared progress remainders: fixed.** `design.rs::track` now paints `tone.track` without the previous 0.45 opacity. Original pixels in `main-models-min` are `#8f8f8a` on white, **3.25:1** light, and `#707070` on `#222222`, **3.21:1** dark. `meetings-processing` gives **3.11:1** and **3.59:1** on its canvas. Seek tracks retain their previous correction. The optional fill/remainder refinement below is a separate distinction, not a claim that this fix failed.
3. **HUD pressed styling: fixed in source.** `recording_widget.rs::pill_button`, `pill`, `icon_button` and the other inspected HUD command builders now have static `.active` fills. Black chrome steps from 0.14 white hover to 0.34 white pressed; panel pills blend toward the text tone. Resting `recording-widget` and `recording-widget-expanded` remain readable. The captures do not demonstrate the pressed phase, so this is source-backed closure, with keyboard operation still a functional question.
4. **Repeated AI identity and terse quantization: materially fixed.** `msettings-ai` removes the saved Claude row's near-duplicate exact ID, retains one default marker and leads the status with the missing-key condition. Catalogue IDs remain secondary and in the code font. `main-models-min` and `main-models-full` say Q8_0 quantization. Seeing a model in both a saved list and an add-model catalogue serves different tasks and is no longer a blocker or score penalty.
5. **Compact startup truncation: fixed.** `startup-app-min` now shows Meeting settings in full in both themes, matching the real compact sidebar's text budget.
6. **The cited spacing exceptions: fixed.** The shared panel inset is now 16 pt; the cited AI and meeting-tab custom spacing is normalized. `main-general`, `msettings-ai` and `meetings-tab-focus` show the intended result. This closes the specific round-3 finding, rather than claiming every number in every transient skeleton is on the same grid.

The new shell fixtures also verify the leading toolbar Back for People (`meetings-people-person-min`), and `main-debug-full` keeps 300 ms on one line. `meetings-processing` retains a simple Processing list status while showing the percentage and bar once in detail.

## 2. Regressions since round 3

**Short-value pop-ups have become unnecessarily wide.** In `main-general`, Right Option and Default previously hugged their values inside the shared column. Round 4 expands them to the full 200 pt trigger width in both themes. `main-advanced` similarly gives short values such as Off a large field. Source confirms that `Dropdown::render` now uses a fixed trigger width.

The change makes the reset's relationship to the field clearer and gives controls a consistent edge. Its cost is a small loss of restraint and a miss against the rubric's content-sized pop-up target. This lowers parameter 15 from 9 to 8; parameter 8 remains 8 for this new, smaller reason after the AI wrapping problem was fixed. It does not block use or a holistic 9. Ranked item 1 proposes a way to retain a fixed reset column and short triggers together.

No other material visual regression was established. Compact History now presents a separate detail view with Back instead of an inline expansion. That is a deliberate presentation tradeoff with a readable result in `history-selected-min`; preservation of selection and keyboard behavior needs runtime verification. The persistent status bar is outside the removal/hiding scope and is not listed as a regression.

## 3. Remaining problems, ranked

All five items are **polish; none blocks a holistic 9**. Paths are relative to `crates/guidy-app/src/ui/` unless stated otherwise. Only item 1 keeps parameters below 9. The other items describe refinements within a score of 9, rather than newly imposed release gates.

### 1. Short enumerated values occupy text-field-sized pop-ups

- **What:** Right Option, Default, Off and similar short choices occupy the full 200 pt field. The resulting settings column is orderly but heavier than its content warrants.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced` and `main-advanced-full`, both themes; direct round-3 General comparison. `kit.rs::Dropdown::render` fixes the trigger to `min_width`.
- **Parameters:** 8 and 15 remain at 8. Alignment and consistency retain 9 because their common rule is working.
- **Concrete fix:** retain the 200 pt layout column, but size the pop-up trigger to its selected label plus caret/padding, bounded by that column. Place the existing 24 pt reset in an adjoining trailing slot with a 4 pt gap, so a right-aligned short trigger and the reset stay adjacent and all resets share an x position. Apply the same wrapper to fields, shortcuts and sliders. Keep a full-width menu when options need it. No commands need to move to another page or disappear.
- **Likely source:** `kit.rs::Dropdown::render`, `kit.rs::trailing`, `shortcut_input.rs::reset_slot`; `design.rs::CONTROL_COLUMN_W` can remain 200 pt. Update the `trailing` comment, which still promises that pop-ups hug their values although the current trigger is fixed.

### 2. Light progress fill and remainder have weak tonal separation

- **What:** The corrected remainder is easy to see against the surface. The amber filled segment and gray remainder are closer to each other in light appearance than in dark appearance, so the completion boundary relies more on hue.
- **Evidence:** `meetings-processing-light`, `main-models-min-light`, compared with their dark counterparts. Original fill `#8a6000` against remainder `#8f8f8a` is **1.72:1**; dark `#ffc72c` against `#707070` is **3.17:1**.
- **Parameters:** 3 and 41 are relevant, but retain 9: the complete track clears the surface target and an adjacent percentage states the value. This is a further perceptual improvement, not an asserted WCAG failure for a necessary sole indicator.
- **Concrete fix:** keep the corrected opaque remainder. Add a small neutral end marker to the filled segment, or use a darker light-theme progress fill with stronger separation from the remainder. For example `#503800` is a candidate to measure against both the remainder and surface. Preserve the 4 pt geometry and percentage. Do not revert to fading the remainder or darken decorative dividers.
- **Likely source:** `design.rs::track`, the light progress token in `assets/themes/guidy.json`, and the status-bar progress consumer. Keep any change specific to progress rather than altering the global accent ink.

### 3. A failed model download puts its reason behind a tooltip

- **What:** The NVIDIA TitaNet Large row says Failed and offers Retry. Its reason is available only through the status tooltip, so the row does not explain whether retry is likely to help until the person investigates it.
- **Evidence:** `main-models-full`, both themes. `model_card.rs` attaches the existing error string to the Failed status tooltip; `download_control.rs` uses the same pattern.
- **Parameters:** 29 and 31; both remain 9 for the broader, well-designed status and recovery system. This is a carry-forward edge case, not a round-4 regression.
- **Concrete fix:** when an existing error string is available, give it a wrapping 12/16 pt secondary line under that failed row, capped visually to a short explanation with full diagnostics in the existing tooltip. Keep the semantic icon, Failed label and Retry action. Do not invent a cause or add a new error-classification system for this pass.
- **Likely source:** `model_card.rs::render` failed branch and `download_control.rs::DownloadControl` failed branch.

### 4. Compact cloud setup leaves its key-help line below the initial fold

- **What:** The provider, key field and Test button are visible at 720×480, but the explanatory storage/provider-help line and Get API Key link are below the initial body viewport. The fade at the pinned footer signals continuation, but finding a key still requires a scroll.
- **Evidence:** `onboarding-setup-cloud-min`, both themes, compared with `onboarding-setup-cloud`. This is below-fold content, not a claim that the footer paints over unreachable controls. The assistant source has a scrolling body and lower fade.
- **Parameters:** 34 and 39 retain 9 because the primary configuration now fits; this is smaller than the missing-form blocker fixed in earlier rounds.
- **Concrete fix:** recover roughly one helper line, about 16–24 pt, above the form at compact height. Reduce the gap between the compact selected-model summary and Meeting notes, and tighten the related summary/form gap. Preserve the current type sizes, complete key field, footer commands and scroll behavior. Do not change Finish validation or defaults.
- **Likely source:** `onboarding/setup.rs::cloud_form` and compact summary composition; `onboarding/assistant.rs` compact body allocation.

### 5. Usage still asks readers to decode table shorthand

- **What:** Avg, Max, Work and Load are terse headings. Currency symbols and the local row's hours make their meaning inferable, but the table could state the basis directly.
- **Evidence:** `main-usage`, `main-usage-full` and `main-usage-min`, both themes. The headline totals and token input/output labels are already clear.
- **Parameters:** 32 and 33 retain 9. Numbers, units and cost totals are readable; this is wording polish within a functioning table.
- **Concrete fix:** use Avg / call and Max / call for cloud cost; use Audio and Load time where those match the existing local values. Allow a two-line 12 pt table heading at minimum width rather than squeezing the model name. Keep the current right alignment, tabular figures and compact secondary-column behavior.
- **Likely source:** `pages/usage.rs::cloud_columns`, `local_columns`, and `assets/locales/en/translation.json` under `usage.col`. Confirm each label against the existing value before renaming it.

## 4. What would raise each parameter still below 9

- **8. Width and layout measure:** keep the existing form/catalogue caps and compact AI allocation; let short enumerated triggers fit their contents within the fixed layout column.
- **15. Control styling and sizing:** preserve the 26 pt control height and explicit auxiliary bounds; combine content-sized pop-ups with an adjacent fixed reset slot.

The other 43 parameters score 9 on the available evidence. Items 2–5 above are possible refinements toward 10, not additional requirements for a 9.

## 5. Functional findings, separate from visual scores

These are verification questions carried forward or prompted by the new presentation. No runtime defect is established by a fixture alone, and none reduces the UI-only scores.

1. **Shortcut semantics and setting exposure:** verify precedence between Dictation key and Transcribe shortcut, both shown as Right Option. Resolve that before copy promises a behavior distinction. Moving debug-only timing settings to General changes their audience and remains out of scope (`main-general`, `main-debug`).
2. **Credential ownership and readiness:** verify which keys are shared between Post Process, meeting summaries and speaker services. AI now states the missing-key condition clearly, but store consolidation remains functional work. Verify the outcome of Finish with an empty cloud key and the recovery for a saved default that lacks credentials (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`).
3. **Keyboard, focus retention and scrolling:** verify HUD/scrubber keyboard operation and accessible names; compact AI's outer/inner scrolling; category changes; and focus return after alerts. The new General focus stills demonstrate visible existing focus paths, not complete reachability. For compact History and People, verify Return/Esc, the leading Back action and restoration of the previously selected list row after returning or resizing (`focus-1` through the supplied `focus-10` subset, `history-selected-min`, `meetings-people-person-min`, `msettings-ai-min`, `recording-widget-expanded`).
4. **Audio availability and retention:** verify redo/retranscription availability when History reports Recording not found and after retained meeting audio expires. Command availability and retention defaults remain outside the styling pass (`history-selected`, `msettings-general`).
5. **Summary generation during recording:** verify which portion Generate Summary uses and how later generation preserves or replaces an earlier result (`meetings-recording-wide`).
6. **Destructive and merge behavior:** verify Esc cancellation, focus return and supported default alert actions; verify person-merge scope, confirmation and undo. Both destructive fixtures visibly reach their alerts, so the old discard-fixture concern remains closed (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
