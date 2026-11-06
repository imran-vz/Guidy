# Round 5 UI review: codex

Reviewer: `codex` · 6 November 2026 · Independent council review

Guidy scores **8.9/10 overall and 9/10 to ship**. I would still sign off on the UI-only refresh. The new sidebar gives meeting forms useful width, keyboard focus is clearer, and compact cloud onboarding now exposes its key help. The small reduction from round 4 comes from navigation: expanding nine categories inside the main sidebar introduces a third level and pushes other app destinations below the fold. The remaining findings are polish, with no holistic 9 blocker on this evidence.

## Evidence and scoring method

I read the complete council brief, including all scope clarifications, the agreed rubric, the round task, the round log and my earlier Codex reports for calibration. I did not read any other reviewer's report.

I inspected a broad sample of the 242 current PNGs using in-memory image sheets and larger crops. Coverage includes every main section in both themes at default and minimum size; all supplied wide variants; meeting notes, transcript, search, playback, processing, recording and empty states; every meeting-settings category; both new local-AI sizes; People and compact person detail; onboarding and startup variants; errors, both alerts and both HUDs. I inspected all twelve focus states and all three hover states in both themes. Lower-content crops covered General, Advanced, Debug, Models, Storage, Post Process and meeting reading views. Larger crops checked the compact AI catalogue and local-model metadata. Contrast measurements use original PNG pixels.

Direct round-4 comparisons covered Meeting settings, minimum cloud AI and minimum General. The earlier report supplies the calibration for its other scores and polish items. Baseline comparisons covered minimum Models, meeting detail and minimum cloud setup. I inspected the Codex, Claude and T3 Code reference PNGs. Codex and Claude retain the round-4 rubric scores; `(k)` marks estimates that require product knowledge beyond their single image. T3 Code, Xcode and System Settings inform the judgment without extra score columns.

Read-only source checks covered shared settings rows, pop-ups, progress, sidebar layout and scrolling, meeting-settings composition, AI status/local-model rows, HUD pressed states and Reduce Motion handling. No source was edited, no Cargo command or live app was run, and no build or preview channel was touched. Only this report was written in the repository. The humanizer skill was applied to the report copy.

Bare snapshot stems refer to `target/ui-review/round-5/<stem>-{light,dark}.png`. A theme or earlier round is named when needed. Captures are 2×. Missing traffic lights, blank chrome in isolated fixtures, opaque HUD backgrounds and unused tall-capture space are harness conventions, not findings.

Interaction scores are provisional judgments from stills and the identified source. They do not certify hit testing, keyboard reachability, VoiceOver, focus restoration, scrolling or frame cost. New hover and focus evidence improves confidence without proving every interaction phase. Stock controls' accepted opaque 1 pt focus edge is not penalized. The custom sidebar now has a separate 2 pt ring. The idle status bar and Check for updates entry point remain outside removal/hiding scope. The brief establishes that Return confirms Delete/Discard and Esc cancels; the filled destructive button accurately represents that default, so it is not a styling defect.

## 1. Scores

| # | Parameter | Guidy round 5 | Guidy round 4 | Codex | Claude | Reason for Guidy round 5 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Typography | 9 | 9 | 8 | 8 | System UI, readable helpers, reading text and code-font identifiers keep distinct roles (`main-general`, `meetings-detail`, `main-usage`, `startup-error-min`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Semibold marks headings and speakers while descriptions and paragraphs stay regular (`main-general`, `meetings-transcript`, `msettings-ai-local`). |
| 3 | Color contrast | 9 | 9 | 7 | 7 | Necessary tracks remain visible in both themes; light progress fill/remainder separation is still a refinement (`meetings-processing`, `main-models-min`, `focus-1`). |
| 4 | Information hierarchy | 9 | 9 | 8 | 9 | Selected content and current model conditions lead their regions; the wider meeting form is easier to parse (`history-selected-min`, `meetings-settings`, `msettings-ai`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Page/object headings, group headings, rows and helpers have clear roles (`main-general`, `msettings-recording`, `msettings-ai-local`, `meetings-detail`). |
| 6 | Design language | 9 | 9 | 8 | 8 | Forms, alerts, lists and the deliberate HUD exception use a coherent component language (`main-general`, `msettings-linear`, `postprocessing-discard`, `recording-widget-expanded`). |
| 7 | Padding and spacing | 9 | 9 | 8 | 9 | Shared panel insets and page/group gaps remain orderly; new forms use the same leading inset (`main-general`, `meetings-settings`, `msettings-recording`). |
| 8 | Width and layout measure | 8 | 8 | 8 | 9 | Left-anchored capped forms work; short pop-up values still occupy 200 pt triggers (`main-models-wide`, `msettings-ai-wide`, `main-general`, `main-advanced-full`). |
| 9 | Color and theme | 9 | 9 | 8 (k) | 8 (k) | Neutral selection and Default chips leave accent and semantic colours with clear jobs (`msettings-ai`, `msettings-ai-local`, `meetings-recording`). |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 8 (k) | Common reset, status, selection and transport treatments survive the layout changes (`main-debug`, `msettings-ai-local`, `meetings-playing`, `history-selected-min`). |
| 11 | Alignment and grid | 9 | 9 | 8 | 9 | Onboarding row text shares a leading slot; settings controls and preview accessories have stable columns (`onboarding-setup-cloud-min`, `main-debug`, `main-postprocessing`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 8 | Groups and descriptive rows retain a useful cadence; compact General recovers an inline layout (`main-general-min`, `msettings-recording`, `msettings-ai-min`). |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Provider credentials, local model choices and recording preferences form understandable regions (`msettings-ai`, `msettings-ai-local`, `msettings-recording`). |
| 14 | Iconography | 9 | 9 | 8 | 8 | Transport symbols agree, People avatars share a slot, and caution marks distinguish alerts (`meetings-playing`, `meetings-people`, `advanced-limit-confirm`). |
| 15 | Control styling and sizing | 8 | 8 | 8 (k) | 8 (k) | Regular controls and accessory bounds are consistent; fixed-width short pop-ups still miss the content-sized target (`main-general`, `main-advanced`, `main-debug-min`). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 8 | Named commands, visible model choices and direct recovery remain clear (`history-selected-min`, `msettings-ai-min`, `storage-error`, `meetings-people-person-min`). |
| 17 | Interactive states | 9 | 9 | 8 (k) | 8 (k) | New hover captures differ from selection; focused switches, busy work and disabled actions are distinct (`hover-sidebar`, `hover-list-row`, `hover-button`, `focus-11`, `meetings-processing`). |
| 18 | Selection clarity | 9 | 9 | 8 (k) | 8 (k) | Neutral selection stays separate from keyboard outlines and catalogue membership checks (`focus-1`, `meetings-tab-focus`, `msettings-ai`, `meetings-people`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Sidebar focus has its own 2 pt ring; the full sequence shows clear pop-up, shortcut, reset and switch edges (`focus-1` through `focus-12`, `meetings-tab-focus`). |
| 20 | Target size | 9 | 9 | 7 (k) | 7 (k) | Existing bounded auxiliary controls remain; category rows are 24 pt high and fill the sidebar width (`main-models-full`, `meetings-playing`, `meetings-settings`; source). |
| 21 | Navigation and wayfinding | 8 | 9 | 8 | 8 | Categories are explicit, but three sidebar levels displace global destinations even at default height (`meetings-settings`, `msettings-ai-min`, `msettings-setup-min`). |
| 22 | Information architecture | 9 | 9 | 8 | 8 | Existing destinations and settings retain clear intent and scope; navigation crowding is counted under 21 (`main-general`, `meetings-settings`, `msettings-transcription`). |
| 23 | Toolbar and window chrome | 9 | 9 | 7 | 8 | Leading Back stays consistent; compact History detail drops irrelevant list filters (`history-selected-min`, `meetings-detail-min`, `meetings-people-person-min`, `debug-logs`). |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Canvas, sidebar, panels and modal surfaces remain distinct in both themes (`main-general`, `meetings-settings`, `advanced-limit-confirm`, `recording-widget-expanded`). |
| 25 | Borders and dividers | 9 | 9 | 8 | 8 | Hairlines divide groups and reading panes without boxing every paragraph (`main-general`, `meetings-detail-wide`, `main-postprocessing`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Chips, controls, panels, alerts and HUD capsules retain their small shape family (`msettings-ai-local`, `advanced-limit-confirm`, `recording-widget`). |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | The empty library has one message pointing to Record; missing summaries have a local generation action (`meetings-empty`, `meetings-recording-wide`). |
| 28 | Loading and progress | 9 | 9 | 8 (k) | 8 (k) | Startup names its work; meeting processing shows its detailed percentage once (`startup-app`, `startup-new`, `meetings-processing`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Failed downloads now show their reason inline, Storage names its path, and AI names the missing key (`main-models-full`, `storage-error`, `msettings-ai-min`). |
| 30 | Data density | 9 | 9 | 8 | 8 | Compact Models retains active work and History prioritizes its text; local AI has minor repeated metadata (`main-models-min`, `history-selected-min`, `msettings-ai-local-min`). |
| 31 | Scannability | 9 | 9 | 8 | 9 | Names, model states, quantities and actions have stable positions; People text now shares an avatar slot (`main-models`, `main-usage`, `meetings-people`). |
| 32 | Microcopy and capitalization | 9 | 9 | 8 (k) | 8 (k) | Usage says Avg / call and Max / call; Known people and neutral Default labels are clear (`main-usage`, `meetings-people`, `msettings-ai-local`). |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 8 | Tabular amounts, explicit cost basis and single-line slider units remain readable (`main-usage-min`, `msettings-ai`, `main-debug-full`, `meetings-played`). |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 8 (k) | 8 (k) | Essential forms fit or scroll, and cloud onboarding exposes key help; compact AI's second catalogue row needs a small height adjustment (`main-general-min`, `onboarding-setup-cloud-min`, `msettings-ai-min`). |
| 35 | Wide-size use of space (1440×900) | 9 | 9 | 8 (k) | 8 (k) | Left-anchored forms keep their cap; wide meetings grow useful reading panes (`main-models-wide`, `msettings-ai-wide`, `meetings-people-wide`, `meetings-detail-wide`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Lifted neutral surfaces support clear tracks, text and restrained bright states (`main-general-dark`, `meetings-processing-dark`, `msettings-ai-local-dark`). |
| 37 | Light-mode quality | 9 | 9 | 8 (k) | 8 (k) | Light controls, sidebar focus and activity marks stay visible on their real surfaces (`focus-1-light`, `focus-11-light`, `main-models-min-light`, `recording-widget-expanded-light`). |
| 38 | Native platform fit | 9 | 9 | 7 (k) | 7 (k) | Source lists, grouped forms, leading Back and truthful destructive defaults read as Mac conventions (`main-general`, `history-selected-min`, `advanced-limit-confirm`, `postprocessing-discard`). |
| 39 | Content-first focus | 9 | 9 | 8 | 9 | Removing the middle settings column gives forms space; History detail starts directly with its content (`meetings-settings`, `history-selected-min`, `msettings-ai-wide`). |
| 40 | Visual noise and restraint | 9 | 9 | 7 | 8 | Neutral chips and simple reading panes remain restrained; local AI can shed repeated offline/default wording (`msettings-ai-local`, `meetings-detail`, `history-selected`). |
| 41 | Non-text indicators and use of colour | 9 | 9 | 8 (k) | 8 (k) | State words, icons, membership checks and opaque tracks provide redundant meaning (`msettings-ai-local`, `msettings-setup`, `meetings-processing`, `main-models-min`). |
| 42 | Truncation and text accommodation | 9 | 9 | 7 (k) | 7 (k) | Compact primary labels fit, model descriptions wrap, and pushed details retain complete text (`startup-app-min`, `main-models-min`, `history-selected-min`, `meetings-people-person-min`). |
| 43 | Dialogs and modality | 9 | 9 | 8 (k) | 8 (k) | Both alerts show clear consequences, cancellation and the actual destructive default (`advanced-limit-confirm`, `postprocessing-discard`; brief). |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Upright text, elapsed time and distinct pause/stop marks remain glanceable (`overlay-live`, `recording-widget`, `recording-widget-expanded`). |
| 45 | Motion and transitions | 9 | 9 | 7 (k) | 7 (k) | Static determinate progress and source-backed Reduce Motion handling remain intact (`meetings-processing`, `startup-app`, `recording-widget-expanded`; source). |
| | **Mean of 45** | **8.9** | **9.0** | **7.8** | **8.0** | Unweighted arithmetic means, rounded to one decimal. |

Sums in column order: **402/45**, **403/45**, **349/45**, **358/45**. Round 5 has 42 scores of 9 and three scores of 8; its unrounded mean is 8.9333. The holistic ship score is **9/10**. The decimal decline reflects one navigation deduction, not a broad loss of quality.

### Check of my round-4 polish items

1. **Short pop-up widths: still open.** `main-general`, `main-advanced-full` and `main-debug` retain the 200 pt trigger. Preview accessories now have explicit space after the control column, which improves their relationship to the field, but does not address the trigger's excess width. The `trailing` comment still says short pop-ups hug their value, while `Dropdown::render` explicitly fixes the width.
2. **Light progress fill/remainder separation: unchanged.** Original `meetings-processing` pixels remain `#8a6000` fill and `#8f8f8a` remainder in light appearance, **1.72:1** against each other. Dark remains `#ffc72c` against `#707070`, **3.17:1**. The remainders still clear their canvas at **3.11:1 light / 3.59:1 dark**. This remains optional refinement within scores of 9, not a reopened track-visibility failure.
3. **Failed download reason: fixed.** Both `main-models-full` images show Connection reset by peer as an inline secondary line under NVIDIA TitaNet Large, alongside Failed and Retry. The person no longer needs a tooltip to find the existing reason. Plain-language interpretation could be improved later without inventing a cause in this pass.
4. **Compact cloud key help: fixed.** `onboarding-setup-cloud-min` now shows provider, key, Test, the provider/Keychain help and Get API Key above the footer in both themes. Primary text remains readable.
5. **Usage shorthand: materially fixed.** `main-usage` and `main-usage-min` show Avg / call; the wider table shows Max / call. Local headings are Audio / tokens, Speed and Model load. These communicate the values more directly than the old Work and Load headings.

The new evidence also closes the sidebar focus doubt: `focus-1` has a separate opaque outline rather than relying on a fill change. `hover-sidebar`, `hover-list-row` and `hover-button` show distinct static hover feedback. All twelve focus captures are present in both themes.

## 2. Regressions since round 4

### Sidebar expansion displaces global destinations

Round-4 `meetings-settings` showed the whole app sidebar alongside a separate category list. Round 5 removes that middle column and gives the form substantially more room, but inserts all nine categories into the main sidebar. In current `meetings-settings`, Usage is near the bottom and Storage, Debug and About are below the viewport. In `msettings-ai-min` and `msettings-setup-min`, even Models only starts at the bottom edge. Both themes show this.

Source confirms a third navigation level and one scrolling container for the entire sidebar. It also defines selection reveal and arrow navigation, so I am not claiming these destinations are unreachable. The regression is in simultaneous visibility and navigation depth. This reduces parameter 21 from 9 to 8. It does not reduce parameter 22 again, and does not undo the form-width improvement.

### Minimum cloud AI no longer shows two complete catalogue rows in the supplied capture

Round-4 `msettings-ai-min` shows both Opus and Sonnet with their secondary metadata and prices. The current full-shell fixture shows all of Opus and Sonnet's title/context, but clips Sonnet's lower metadata at the body boundary above the status bar. Both themes agree.

The fixture composition changed: round 4 used an isolated pane with blank shell chrome; round 5 shows the real sidebar, title and status bar. This establishes the current compact presentation gap, but is not enough to prove that the AI list itself became taller in the running app. Treat it as a small viewport integration issue. One complete choice and most of a second remain visible, so this does not reopen the much larger round-3 blocker or lower parameter 34 by itself. The concrete adjustment is item 3 below.

No other material visual regression was established. Left anchoring on wide forms is a consistent presentation choice with preserved measure. Filled Delete/Discard is correct for the confirmed Return behavior. The idle status bar remains outside hiding/removal scope.

## 3. Remaining problems, ranked

All five are **polish; none blocks the holistic ship score of 9**. Items 1 and 2 block a 9 for their named individual parameters. Paths below are relative to `crates/guidy-app/src/ui/` unless stated otherwise. Recommendations preserve existing destinations, commands, settings visibility, stores, defaults and validation.

### 1. Meeting settings makes the global sidebar too deep and tall

- **What:** Meetings → Meeting settings → category is three visible levels in one narrow source list. Expanding nine rows pushes unrelated destinations out of view and leaves the parent above the selected child without a conventional disclosure indicator.
- **Evidence:** `meetings-settings`, `msettings-general`, `msettings-ai-min`, `msettings-setup-min`, both themes; compare round-4 `meetings-settings`. `main_window.rs::NavItem::level` and `render_sidebar` explicitly implement a third level and one overall scroll region.
- **Parameters:** **21 is 8, so this blocks its individual 9.** Related 22 and 34 remain 9: scope is understandable and offscreen rows are scrollable, not lost.
- **Concrete fix:** keep the wider single form pane, but present at most two navigation levels at once. A contextual sidebar can reuse Meeting settings as its heading/return control, list the nine existing categories with one 16 pt icon slot and 28–30 pt rows, and retain a named return to the full destination list. Reuse the existing parent navigation action rather than adding or removing a command. Make the hierarchy transition explicit and keep the selected category visible. This avoids shrinking text or squeezing twenty rows into 480 pt.
- **Likely source:** `main_window.rs::nav_items`, `render_sidebar`, `render_titlebar`; meeting category labels in `pages/meetings/settings.rs`.

### 2. Short choices still occupy a text-field-sized pop-up

- **What:** Auto, Default, Off and Right Option occupy the entire 200 pt trigger. The shared trailing edge is orderly, but the empty field area gives short enumerations unnecessary weight.
- **Evidence:** `main-general`, `main-general-min`, `main-advanced-full`, `main-debug-min`, both themes; `kit.rs::Dropdown::render` uses `.w(trigger_width)`.
- **Parameters:** **8 and 15 remain 8, blocking their individual 9s.** Alignment remains 9 because the column rule itself is consistent.
- **Concrete fix:** retain the 200 pt layout column and 26 pt height, but right-align a trigger sized to its label, caret and padding within it. Keep the existing 24 pt reset adjacent with a 4 pt gap and a stable shared reset position. Keep long-option menus wide enough independently of the trigger. Preserve the newly separated preview/refresh accessory slot. Update `trailing`'s comment to match the chosen rule.
- **Likely source:** `kit.rs::Dropdown::render`, `kit.rs::trailing`, `shortcut_input.rs::reset_slot`; `design.rs::CONTROL_COLUMN_W` can remain 200 pt.

### 3. Compact cloud AI clips the second choice's metadata

- **What:** The bottom of the selected Sonnet catalogue row is below the initial body viewport. The condition and key form are clear, but a small amount of scaffolding still displaces useful comparison data.
- **Evidence:** `msettings-ai-min`, both themes; larger bottom crops compared with round-4 `msettings-ai-min`. This is viewport clipping in a scrollable pane, not text ellipsis or proof of status-bar overlap.
- **Parameters:** 12, 30, 34 and 39 remain 9. This is a small polish gap, not the former near-empty catalogue.
- **Concrete fix:** recover roughly 12–16 pt above the catalogue at compact height. Reduce related header/saved-model/provider gaps using the existing 8/12 pt tokens while preserving 12 pt helpers and the 26 pt key controls. Verify the full-shell 720×480 fixture, including its unchanged status-bar allocation, shows two complete rows. Do not hide the status bar or change its entry points.
- **Likely source:** `pages/meetings/ai_settings.rs::header`, the cloud `Render` composition and `model_list`; compact allocation in `pages/meetings/settings.rs`.

### 4. Local AI repeats offline, default and download-size information

- **What:** The status says This Mac · offline, followed by Runs offline on this Mac. The first row has Default plus The default for summaries. The second displays 5.2 GB below the description and again inside Download 5.2 GB. Each fragment is understandable, but together they make a short choice list busier than necessary.
- **Evidence:** `msettings-ai-local` and `msettings-ai-local-min`, both themes. The new captures establish this local state; they do not establish a new regression from round 4.
- **Parameters:** 30, 32 and 40 remain 9. Model names and actions are readable, so this is refinement within the existing score.
- **Concrete fix:** retain the readiness icon, active model and one offline statement. Shorten the helper to Choose a downloaded model. Keep the neutral Default chip and remove the repeated default clause from the description. Show download size once per row, preferably with its existing Download action; keep disk size for an installed model. Do not change which model is default or which controls are available.
- **Likely source:** `pages/meetings/ai_settings.rs::status_line`, local `Render` branch and `LocalModelsView::local_rows`; wording in `assets/locales/en/translation.json`.

### 5. Light progress fill and remainder are close in luminance

- **What:** The complete track is visible, but its amber and gray segments are only 1.72:1 apart in light appearance. Dark appearance has a more distinct completion boundary.
- **Evidence:** `meetings-processing-light` and `main-models-min-light`, compared with dark counterparts. Original pixel values and ratios are recorded in the round-4 check above.
- **Parameters:** 3 and 41 retain 9. The adjacent percentage redundantly states progress and the remainder clears the surface target; this is not an asserted failure of a sole necessary indicator.
- **Concrete fix:** preserve the opaque remainder and 4 pt track. Add a small neutral end marker or use a darker light-only progress fill. Measure a candidate such as `#503800` against the remainder and surface. Keep this change specific to progress instead of changing global accent ink or darkening decorative dividers.
- **Likely source:** `design.rs::track`, `assets/themes/guidy.json` progress token, and the status-bar progress consumer.

## 4. What would raise each parameter still below 9

- **8. Width and layout measure:** retain the new left anchoring and measure caps; size short pop-up triggers to their contents within the fixed control column.
- **15. Control styling and sizing:** keep 26 pt controls and explicit accessory bounds; combine content-sized triggers with a consistent adjacent reset slot.
- **21. Navigation and wayfinding:** preserve the wider meeting form while showing no more than two navigation levels at once, with an explicit return to the full app destination list.

The other 42 parameters score 9 on the available evidence. Ranked items 3–5 improve polish within those scores; they are not new release gates.

## 5. Functional findings, separate from visual scores

These are verification questions, not runtime failures established by screenshots. None reduces a visual score.

1. **Shortcut semantics and audience:** confirm precedence between Dictation key and Transcribe shortcut, both shown as Right Option. Debug-only timing controls retain their existing audience; moving them into General is outside this pass (`main-general`, `main-debug`).
2. **Credential ownership and incomplete setup:** verify which keys are shared between Post Process, meeting summaries and speaker services. Verify Finish with an empty cloud key and recovery when a saved default lacks credentials. Store consolidation, validation and default changes remain functional work (`main-postprocessing`, `msettings-ai`, `msettings-speakers`, `onboarding-setup-cloud-min`).
3. **Navigation and keyboard behavior:** verify arrow navigation and selection reveal across expansion/collapse of the new category branch, deep-link selection, category scroll reset and focus retention. Source defines these paths, but stills do not exercise them. Also retain the HUD/scrubber keyboard and accessible-name checks, compact AI nested scrolling, and compact History/People return-selection checks (`meetings-settings`, `msettings-ai-min`, `history-selected-min`, `meetings-people-person-min`, `recording-widget-expanded`).
4. **Audio availability and retention:** verify redo/retranscription when History says Recording not found and after retained meeting audio expires. Command availability and retention defaults are outside styling scope (`history-selected`, `msettings-general`).
5. **Live summary generation:** verify which portion Generate Summary uses during recording and how later generation preserves or replaces it (`meetings-recording-wide`).
6. **Destruction and merging:** the brief settles Return-confirm and Esc-cancel behavior for `kit::confirm`; do not reopen that as an unknown or demand a different default. Runtime checks still need to establish focus restoration and person-merge consequences/undo (`advanced-limit-confirm`, `postprocessing-discard`, `mpeople-person`).
7. **Model-size data consistency:** the fixtures label Qwen3 4B as 2.5 GB in Models and 2.6 GB in local AI, with 5.0/5.2 GB for the larger model. Verify that both pages use the same byte basis and catalogue data before changing labels. This may be fixture/source-data drift rather than a production defect (`main-models-full`, `msettings-ai-local`).
