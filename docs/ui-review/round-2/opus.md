# Guidy UI review, round 2: reviewer `opus`

Evidence: 54 round-2 snapshots viewed with the image tool. They cover both themes, every main section at default size, and the `-min`, `-wide` and `-full` variants for General, Advanced, Usage, Models, Meetings, AI summaries and History. They also cover:

- **Meetings:** detail, transcript, playing, recording, recording-wide, processing, empty, find, detail-min, detail-wide.
- **People:** `meetings-people`, `mpeople-person`, `mpeople-person-min`.
- **Meeting settings panes:** setup, AI (default, min, wide), speakers, linear, you.
- **Onboarding:** welcome, permissions, picker, setup-cloud, setup-min, migration.
- **Startup and errors:** `startup-app`, `startup-error`, `usage-error`.
- **Dialogs and HUDs:** both dialogs, the overlay, and both widget states.

I compared these with round 1 (`round-1/`) and round 0. Pixel measurements used Pillow in a scratch venv (`python -I`), with WCAG 2.x relative luminance. Coordinates are 2x pixels.

Note: the `msettings-*` and `mpeople-person*` snapshots render the pane without the shell sidebar or toolbar (empty grey left column). This is the same in round 1, so it is a harness framing and not a defect. `meetings-settings-*` shows the real shell.

## The round-2 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| NSAlert-like confirm, caution icon, solid red destructive button | Holds | `advanced-limit-confirm-*`, `postprocessing-discard-*`: upper third, red caution glyph, Delete/Discard white on `#c8372f` 5.2:1 (L), `#d13a33` 4.8:1 (D). |
| Discard scenario reaches its dialog | Holds | `postprocessing-discard-*` |
| Light sidebar selection ~1.4:1 | Partly | Top-level rows `#d2d2d0` on `#f1f1ef` = 1.34:1. Indented child rows (People, Meeting settings) use a different, darker `#c8c8c7` = 1.48:1. |
| Opaque slider track | Holds | `#988e73` on white 3.25:1 (L), `#80755a` on `#222222` 3.49:1 (D) in `main-debug-*` and `msettings-speakers-*`. |
| Pop-ups hug their value | Partly | "Right Option", "Default" and the Advanced pop-ups hug. "Auto" (`main-general-*`) and "3 days" (`meetings-settings-*`) are still 200 pt. The Post Process provider is about 320 pt. |
| Single-line rows 36–40 pt | Close | Microphone row is 41 pt; rows with a description are 53 pt. |
| 2 pt focus on custom segments and disclosures | Not visible | No still shows it. The only captured focus (`meetings-find-*`) is unchanged: 1 pt `#8a6000` plus a 3 pt `#c5af80` halo (2.1:1 on white). |
| People and Meeting settings as sidebar rows; plain tab strip | Holds | `meetings-*`, `meetings-settings-*` |
| Player on its own row, m:ss, steady live dot, one Record style, filled pause | Holds | `meetings-detail-*`, `meetings-playing-*`, `meetings-recording-*`, `meetings-empty-*`. Two exceptions: Debug "Pause" (`debug-logs-*`) still uses the outlined glyph, and the widget "Stop" is an outlined square. |
| Back button leading at minimum size | Meetings only | `meetings-detail-min-*` uses a toolbar chevron. `mpeople-person-min-*` uses an in-content "‹ Back" text button. |
| 180 pt category list at 900 pt, left-anchored form, 17 pt pane heading | Holds | `meetings-settings-*`, `msettings-ai-wide-*` |
| Full-window onboarding, pinned footer | Holds | `onboarding-*` |
| One capability line per model in the picker | Holds | It now disagrees with Models (see Problem 9). |
| History detail titled by its first sentence, Copy + More, floating undo | Holds | Introduces duplication (Regression 2). |
| Model descriptions wrap; Advanced wraps; Debug split; Usage 2×2; centred error blocks | Holds | `main-models-*`, `main-advanced-*`, `main-debug-*`, `debug-logs-*`, `main-usage-min-*`, `usage-error-*` |

## Measured facts

| What | Round 1 | Round 2 |
|---|---|---|
| Alert destructive label | 3.7 (L) / 4.7 (D) on pale pink | 5.2 (L) / 4.8 (D), solid red |
| Slider unfilled track vs panel | 2.06 (L) / 2.30 (D) | 3.25 / 3.49 |
| Toolbar live dot | pulsing to 2.2:1 | steady `#c4312b` 5.3:1 with a static `#f0d6d4` halo |
| Sidebar selection vs sidebar (light / dark) | 1.26 / 1.39 | top-level 1.34 / 1.39; child rows 1.48 (`#c8c8c7`) / `#464646` |
| Other selection fills (light / dark) | — | meeting list `#dadad9` / `#313131`, AI catalog `#dedede`, category list `#313131` (dark) |
| Player scrub track vs pane | n/a (collided) | `#ececea` on `#fafaf9` 1.13 (L); `#2e2e2e` on `#181818` 1.31 (D) |
| Storage "Free" segment and swatch vs panel | — | `#ececea` on white 1.18 (L); 1.17 (D) |
| Checkbox fill, light | — | `#ffc72c` in onboarding vs `#8a6000` in AI settings |
| Control heights | — | Pop-ups 23.5 pt; text fields, in-row buttons and toolbar buttons 19–19.5 pt; onboarding footer buttons 25.5 pt |
| Focus ring (search) | 1 pt + about 3 pt halo | unchanged (1 pt `#8a6000` + 3 pt `#c5af80`) |
| Setting rows | 45 / 57 / 73 pt | 41 / 53 / 69 pt; AI catalog two-line rows still about 36 pt |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 1; my judgement of those apps still holds.

| # | Parameter | Guidy R2 | Guidy R1 | Codex | Claude | Reason for the round-2 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 8 | 8 | 8 | 9 | SF Pro, 14 pt reading (`meetings-detail-*`), 12 pt helpers, mono only for IDs and errors. Transcript speaker names are still 13 regular (`meetings-transcript-*`). |
| 2 | Font weight usage | 8 | 8 | 8 | 8 | Weight follows role. 700 only on onboarding titles (`onboarding-welcome-*`). |
| 3 | Color contrast | 8 | 7 | 8 | 8 | Alert, slider and live dot fixed. Still short: scrub track 1.13/1.31 (`meetings-detail-*`), Storage free 1.18 (`main-storage-*`), focus halo 2.1. |
| 4 | Information hierarchy | 8 | 7 | 8 | 8 | One labeled action per header (`meetings-detail-*`). History title repeats the body (`main-history-*`). Two Records in `meetings-empty-*`. |
| 5 | Typographic hierarchy | 7 | 7 | 8 | 8 | 17 pt pane headings in Meeting settings help. Settings pages still go 15 (toolbar) › 13 sb (group) › 13 (row) (`main-general-*`). |
| 6 | Design language | 8 | 7 | 8 | 9 | Onboarding is full-window and the segmented/pop-up navigation is gone. Four button styles and two checkbox colours remain. |
| 7 | Padding and spacing | 8 | 7 | 8 | 8 | Rows tightened to 41/53. AI catalog rows still about 36 pt with titles touching the divider (`msettings-ai-*`). |
| 8 | Width and layout measure | 8 | 7 | 8 | 9 | Forms left-anchored beside the category list (`msettings-ai-wide-*`). "Auto" and "3 days" in 200 pt, provider in 320 pt. |
| 9 | Color and theme | 8 | 7 | 8 | 8 | One neutral Record, neutral "You" when unselected. Light checkboxes are bright `#ffc72c` in onboarding but `#8a6000` in settings; slider tracks are olive/beige. |
| 10 | Overall consistency | 7 | 6 | 8 (k) | 9 (k) | Four selection fills, two checkbox fills, outlined Stop/Play/Pause beside filled ones, two back-button styles, four button styles. |
| 11 | Alignment and grid | 7 | 7 | 8 | 9 | Fixed at min (`meetings-detail-min-*`). The reset slot moves with hugging pop-ups (x 1289 vs 1449, `main-general-*`). The Models trailing slot shifts (`main-models-*`). |
| 12 | Vertical rhythm | 8 | 7 | 8 | 9 | 41/53/69 pt rows, constant group gaps. Catalog rows are tighter than everything else. |
| 13 | Perceptual grouping | 8 | 7 | 8 | 8 | Debug split into Settings | Logs (`main-debug-*`, `debug-logs-*`). Meeting settings groups are clear. |
| 14 | Iconography | 8 | 7 | 8 | 8 | Hover-only transcript icons, filled pause. "Meetings on this Mac" still reuses the Models CPU glyph. ▷ outlined in voiceprints vs ▶ in the player. |
| 15 | Control styling and sizing | 7 | 7 | 8 | 8 | Bordered toolbar commands. A 24 pt pop-up sits next to 19 pt fields in the same row (`main-postprocessing-*`, `msettings-ai-*`, `onboarding-setup-cloud-*`). Four button styles. |
| 16 | Affordance and signifiers | 8 | 7 | 7 | 7 | Manage, Test Connection and Clear Usage are bordered. History toolbar star and folder are unlabeled (`main-history-*`). |
| 17 | Interactive states | 7 | 7 | 8 | 8 | Disabled comes in two looks: filled grey (`msettings-linear-*`) and bordered grey (`onboarding-setup-cloud-*`). Hover is still unproven. |
| 18 | Selection clarity | 7 | 8 | 8 | 8 | Visible everywhere, but four different fills, and child sidebar rows differ from top-level rows (1.48 vs 1.34). |
| 19 | Focus visibility | 6 | 6 | 7 (k) | 7 (k) | Only the search ring is visible, and it is unchanged and heavy (`meetings-find-*`). |
| 20 | Target size | 7 | 7 | 8 | 8 | 28 pt header icons. In-row buttons are 19 pt tall (Request Again, Use, Resume, Test). "Edit" and the transcript search/copy icons are about 16 pt. |
| 21 | Navigation and wayfinding | 7 | 5 | 8 | 8 | Sidebar tree instead of segmented navigation. Meeting settings still adds a second list, and its first category "General" (gear icon) collides with top-level General. |
| 22 | Information architecture | 7 | 5 | 7 (k) | 8 (k) | In-scope presentation is clearer. "You" appears in both People and Meeting settings › You; sidebar clusters are unlabeled. |
| 23 | Toolbar and window chrome | 7 | 7 | 8 | 8 | Short, labeled toolbars. The status bar still repeats the model and "Check for updates". The recording toolbar repeats a truncated title (`meetings-recording-*`). |
| 24 | Surface elevation and layering | 8 | 7 | 8 | 8 | Onboarding card gone. History at min expands into an inline card (`history-selected-min-*`). |
| 25 | Borders and dividers | 8 | 7 | 8 | 9 | No onboarding double rules. Hairlines about 1.27:1. |
| 26 | Corner radius consistency | 8 | 8 | 8 | 8 | Family unchanged and applied by rule. |
| 27 | Empty states | 8 | 7 | 8 (k) | 8 (k) | Centred error blocks with one Try Again (`usage-error-*`). Empty Meetings still shows a second Record. |
| 28 | Loading and progress | 8 | 8 | 8 (k) | 8 (k) | Named progress (`meetings-processing-*`). The skeleton has 6 sidebar rows vs the real 12 in 5 clusters (`startup-app-*`). |
| 29 | Error and status messaging | 8 | 7 | 7 (k) | 7 (k) | `startup-error-*` names the cause with a details box. The AI pane shows its key warning twice, the first time as an icon with no word (`msettings-ai-*`). |
| 30 | Data density | 7 | 7 | 8 | 8 | History title and body are the same text. 3-line descriptions in Advanced (Style), Debug (Reliable paste, Paste delay) and Meeting notes. |
| 31 | Scannability | 8 | 7 | 8 | 8 | Descriptions wrap and "Required by" has its own line (`main-models-*`). |
| 32 | Microcopy and capitalization | 7 | 7 | 8 | 9 | ". ·" joins in Models, "(Old format)", "Confirm?" in straight quotes, mixed apostrophes and periods, "Linear" over "Linear", options named differently in prose and pop-up. |
| 33 | Numeric and tabular formatting | 8 | 8 | 8 | 7 | One m:ss format. "60ms" without a space (`main-debug-*`). |
| 34 | Minimum-size reflow | 8 | 7 | 7 (k) | 8 (k) | No ellipsized descriptions, stacked rows, 2×2 stats, stacked People. The sidebar still takes 200 of 720 pt. |
| 35 | Wide-size use of space | 8 | 8 | 8 | 8 | Left-anchored forms, transcript pane at 1440 (`meetings-detail-wide-*`). The meeting list stays 230 pt. |
| 36 | Dark-mode quality | 8 | 8 | 8 | 8 | Lifted surfaces, neutral selection. Olive slider track. |
| 37 | Light-mode quality | 8 | 7 | 7 (k) | 8 (k) | Controls visible. Scrub track and Storage free segment near-invisible. |
| 38 | Native platform fit | 7 | 6 | 7 | 7 | Alerts and sidebar now read as macOS. The status bar, the custom focus halo and mixed control heights do not. |
| 39 | Content-first focus | 8 | 7 | 9 | 9 | Reading starts near the top in Meetings and History. |
| 40 | Visual noise and restraint | 8 | 7 | 8 | 9 | Transcript icons hover-only. Ten folder icons in Storage, two "default" markers in AI, accent "Recommended" chip. |
| 41 | Non-text indicators | 7 | 7 | 7 | 7 | Sliders and live dot fixed. Scrub track 1.13/1.31 and Storage free 1.18; the AI status is shown by icon only. |
| 42 | Truncation and text accommodation | 8 | 7 | 8 | 6 | Descriptions no longer ellipsize. The History title truncates ("bef…") above the full text; the recording toolbar truncates its title. |
| 43 | Dialogs and modality | 8 | 5 | 7 (k) | 8 (k) | Upper-third alert, caution glyph, solid red button, scrim. It uses a red glyph rather than the app icon with a caution badge; Esc is unproven in stills. |
| 44 | HUD and overlay design | 8 | 8 | 7 (k) | 7 (k) | Filled pause. The expanded widget Stop is an outlined square while the pill uses a solid one (`recording-widget-*`). |
| 45 | Motion and transitions | 8 | 6 | 8 (k) | 8 (k) | No perpetual decorative loops. The live dot is static. The sheen is gated on Reduce Motion (`design.rs:425`). The spinner (`kit.rs:83`) always rotates, which is acceptable for a progress glyph. |

| Column | Mean of 45 |
|---|---|
| Guidy round 2 | **7.6** |
| Guidy round 1 | **7.0** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 2): 7.** The structural blockers from round 1 (navigation, alerts, onboarding) are fixed, and the app now reads as a Mac app. A designer would still send it back once, for one-component-per-concept work: selection fills, control heights, button styles, glyph fills and back buttons. Copy needs a final pass too.

---

## 2. Regressions since round 1

1. **The reset column no longer lines up.**
   - **Evidence:** `main-general-light/dark`. Now that pop-ups hug, the Microphone reset sits at x 1449 while Transcribe, Cancel and Language resets sit at x 1289. The pop-ups' left edges are ragged too (1322 vs 1482). In round 1 every reset shared one x. Debug resets sit at x 1265, a third position.
   - **Fix:** right-align controls, and give the reset a fixed slot of 24 pt + 8 pt gap that sits immediately left of a fixed trailing column (a 200 pt column, with the control right-aligned inside it). Alternatively, keep the hug width and put the reset inside the row's trailing area at a constant offset from the panel edge. Files: `kit.rs` (`reset_button`, `Dropdown`), `ui/shortcut_input.rs` (`reset_slot`).
2. **History detail shows the same text twice.**
   - **Evidence:** `main-history-*`, `history-undo-*`, `history-selected-min-light`. The 17 pt semibold title is the first sentence, cut at two lines ("…roadmap review bef…"). Directly below, the 14 pt body repeats it in full. For one-sentence entries (most dictations) the title and body are identical. This was my own round-1 suggestion, and in practice it does not work.
   - **Fix:** drop the title. Lead with the 12 pt date and time as metadata, with Copy and More on the same line, then the 14 pt body as the content. If a title is wanted, show it only when the text is longer than about 140 characters, and then never repeat it in the body.
   - **File:** `pages/history.rs`.
3. **Two selection fills in one sidebar.**
   - **Evidence:** `meetings-people-*`, `meetings-settings-*`. A selected child row is `#c8c8c7` (L) / `#464646` (D). A selected top-level row is `#d2d2d0` / `#383838`.
   - **Fix:** one `tone.selection` for every source-list row.
   - **File:** `ui/main_window.rs`.

There are no other regressions. Everything else is equal or better.

---

## 3. Remaining problems, ranked

**1. One concept, several drawings (blocks a 9).**
- **Evidence:**
  - Selection fills: sidebar `#d2d2d0`, child rows `#c8c8c7`, meeting list `#dadad9`, AI catalog `#dedede` (light); `#383838` / `#464646` / `#313131` (dark).
  - Checkbox fill (light): `#ffc72c` in `onboarding-welcome-light` and `onboarding-setup-cloud-light`, `#8a6000` in `msettings-ai-light`.
  - Glyphs: filled pause and play in the player, but an outlined Pause in `debug-logs-*`, an outlined ▷ in `mpeople-person-*` and in the Sound theme preview (`main-debug-*`), and an outlined Stop in `recording-widget-expanded-*` next to a filled Pause.
  - Back: a toolbar chevron in `meetings-detail-min-*`, an in-content "‹ Back" in `mpeople-person-min-*`.
  - Disabled button: filled grey ("Test Connection", `msettings-linear-*`) vs bordered grey ("Test", `onboarding-setup-cloud-*`).
  - Model IDs: mono in AI summaries, UI font in Usage.
- **Holds down:** #10, #18, #9, #14, #17, #6.
- **Fix:**
  - One `tone.selection` for every list: `#d2d2d0` / `#383838` in the sidebar, and the same value in content lists. If focus should differ, unfocused lists use `#e3e3e1` / `#2e2e2e`.
  - Checkboxes, radios and switches use the same accent ink (`#8a6000` light, `#ffc72c` dark) everywhere.
  - Filled `play`, `pause` and `square` everywhere.
  - Back always in the toolbar's leading slot.
  - One disabled style: the button's own shape at 40% opacity.
- **Files:** `ui/main_window.rs`, `pages/meetings/list.rs`, `meetings/ai_settings.rs`, `onboarding/welcome.rs`, `onboarding/setup.rs`, `pages/debug.rs`, `meetings/people.rs:1420`, `recording_widget.rs:2179`, `kit.rs`.

**2. Mixed control heights and four button styles (blocks a 9).**
- **Evidence:**
  - `main-postprocessing-*`: the "Model name" field is 19 pt tall next to a 23.5 pt "Choose" pop-up.
  - `msettings-ai-*` and `onboarding-setup-cloud-*`: Anthropic pop-up 23.5 pt, key field and Test 19 pt.
  - `main-general-*`: shortcut fields and pop-ups 23.5 pt, while toolbar buttons (Record, Test Connection, Clear Usage…) and in-row buttons (Request Again, Use, Resume, Manage) are 19 pt.
  - Button styles: yellow primary; bordered; tinted grey fill (Record, Cancel in `meetings-processing-*`, Stop); plain (Copy Notes, Edit, Set Up Later, Done).
- **Holds down:** #15, #20, #38, #10.
- **Fix:**
  - One regular control height of 24 pt for pop-ups, text fields, push buttons and search fields, and 28 pt in the toolbar.
  - Three button styles only. Record and Stop become bordered push buttons, with the red dot or square as the icon. Plain is reserved for in-row links and icon buttons.
- **Files:** `assets/themes/guidy.json` (`input`, `button`), `kit.rs`, `ui/design.rs`.

**3. Meeting settings: three navigation levels and a name collision (blocks a 9 on #21).**
- **Evidence:** `meetings-settings-*` shows sidebar › Meeting settings › category list (Setup, General, You, Recording…) › pane. The category "General" uses the same gear icon and name as the top-level General. "You" is a category here and also the pinned first row in People (`meetings-people-*`). Commands stay as they are; this is a presentation fix only.
- **Holds down:** #21, #22, #6.
- **Fix:**
  - Rename the category "General" to "Meeting notes" with the `notebook` glyph, or fold its two rows into the top of "Recording".
  - In People, label the pinned row "You · Voice" and have "Open in People" point at it, or show the You row only in Meeting settings. Both change only presentation.
  - Give the category list a 12 pt muted header ("Meeting settings") so the second level reads as a sub-list, like System Settings › General's inner list.
- **Files:** `pages/meetings/settings.rs`, `pages/meetings/people.rs`, `assets/locales/en/translation.json`.

**4. Focus ring unchanged and unproven.**
- **Evidence:** `meetings-find-light`: a 1 pt `#8a6000` border plus a 3 pt `#c5af80` halo (2.1:1 on white). It is about 4 pt in all on a 19 pt field. No other focused control is captured.
- **Holds down:** #19, #38, #3.
- **Fix:** one 2 pt ring, `#8a6000` (L) / `#ffc72c` (D), drawn outside a 1 pt gap, with no second halo, on every focusable control. Add snapshots of a focused button, pop-up, switch, sidebar row and list row.
- **Files:** `assets/themes/guidy.json` (`ring`), `kit.rs`, the snapshot harness.

**5. Non-text contrast leftovers.**
- **Evidence:**
  - Player scrub track: `#ececea` on `#fafaf9` 1.13 (L), `#2e2e2e` on `#181818` 1.31 (D) (`meetings-detail-*`, `meetings-processing-*`). The unplayed track is the only cue for the control's length.
  - Storage free segment and its legend swatch: 1.18 (L) / 1.17 (D) (`main-storage-*`).
  - The AI pane's state is an orange "!" icon with no word (`msettings-ai-*`).
- **Holds down:** #3, #41, #37.
- **Fix:**
  - Scrub and progress tracks: use the slider's unfilled track token (`#988e73` / `#80755a` at 3.2–3.5:1), or better a neutral `#8f8f8a` / `#767676`.
  - Storage free: draw it as an outline (1 pt `#8f8f8a`) or a 25% hatch, plus a bordered swatch.
  - AI status line: add the word ("Needs a key").
- **Files:** `meetings/player.rs`, `pages/storage.rs`, `meetings/ai_settings.rs`, `guidy.json`.

**6. Trailing column alignment.**
- **Evidence:** Regression 1, plus `main-models-*`:
  - Parakeet's trash sits at x 1702, but Whisper Small's trash sits at 1655 with "…" at 1702.
  - Whisper Large's size shifts left (ends at 1306 vs 1313) because "Paused at 30%" pushes it.
- **Holds down:** #11, #31.
- **Fix:** fixed slots on Models rows: size (right-aligned, 64 pt), status/progress (160 pt), primary action (hug), and an overflow "…" that is always last. Move Delete into "…" on every row so the trash never changes x.
- **Files:** `pages/models.rs`, `ui/model_card.rs`, `kit.rs`.

**7. Microcopy pass.**
- **Evidence:**
  - Models joins with ". ·": "Fast and accurate. Supports 25 European languages. · Streaming · Q8_0" (`main-models-*`). The picker (`onboarding-picker-*`) describes the same model as "Fast and fairly accurate · 20 languages" while Models says "Fast and fairly accurate. · 20 languages · Translate · Q8_0".
  - "Replaces Parakeet V2 (Old format)" (`onboarding-migration-*`).
  - "Confirm?" in straight quotes (`msettings-speakers-*`).
  - Mixed apostrophes: "can't" (`startup-error-*`) vs "what’s" (`main-about-*`).
  - Descriptions end with a period on General and Advanced but not on Debug, About or "Mute while recording".
  - "Clipboard handling" prose names "Don't Modify Clipboard / Copy to Clipboard" while the pop-up says "Don't modify clipboard" (`main-advanced-full-*`).
  - Duplicated or empty labels:
    - The pane heading "Linear" sits over the group header "Linear" (`msettings-linear-*`).
    - The row title "Test Connection" is also its button's label.
    - "Your API key." is a sentence fragment (`onboarding-setup-cloud-*`).
  - "Cloud" (onboarding) vs "Cloud provider" (AI summaries) for the same choice.
  - "60ms" (`main-debug-*`).
- **Holds down:** #32, #33, #10.
- **Fix:**
  - Join description parts with " · " after stripping the trailing period, and use one helper in both the picker and Models.
  - "(old format)".
  - Curly quotes and apostrophes everywhere.
  - Periods on every full-sentence description.
  - Quote option names exactly as the pop-up shows them.
  - Drop the inner "Linear" group header. Rename the row to "Connection".
  - "Meeting transcripts are sent to the provider to summarize."
  - "Cloud provider" in both places.
  - "60 ms".
- **Files:** `assets/locales/en/translation.json`, `kit::model_description`, `onboarding/model_picker.rs`.

**8. Long prose descriptions still do the explaining.**
- **Evidence:**
  - `main-advanced-*` "Style": 3 lines that explain each option.
  - `main-debug-*`: "Paste delay (after)" and "Reliable paste" at 3 lines.
  - `meetings-settings-*` "Meeting notes": 3 lines.
  - `msettings-speakers-*`: threshold descriptions at 3 lines in a 220 pt label column.
- **Holds down:** #30, #31, #12.
- **Fix:** one line of at most 72 characters per row. Option semantics go into the pop-up items (`DropdownOption::description`), and caveats ("macOS and Windows only") go into a 12 pt footnote under the group.
- **Files:** i18n, `pages/advanced.rs`, `pages/debug.rs`, `meetings/settings.rs`.

**9. AI summaries pane density.**
- **Evidence:** `msettings-ai-*`:
  - The status line "! Claude Sonnet 4.5 · Anthropic" plus "! Add your Anthropic API key" in the provider form says the same thing twice.
  - "default" appears as a badge on Your models and again as a chip in the catalog.
  - Catalog two-line rows are about 36 pt with the title flush against the divider.
- **Holds down:** #7, #29, #40, #12.
- **Fix:**
  - Status line: "Claude Sonnet 4.5 · Needs an Anthropic key", and remove the second warning when the form is visible.
  - Keep "default" only in Your models.
  - Catalog rows: 8 pt vertical × 12 pt horizontal padding, 48 pt rows.
- **File:** `meetings/ai_settings.rs`.

**10. Status bar and recording toolbar repeat what is already on screen.**
- **Evidence:**
  - Every `main-*` view shows "Parakeet V3" plus "Check for updates" in the status bar. About already has "Check for Updates".
  - `meetings-recording-*`: the toolbar shows "12:34 Design review: recording…", truncated, right above the full 17 pt title.
- **Holds down:** #23, #38, #42, #30.
- **Fix:**
  - Show the status bar only while downloads or other background work run. When idle, keep the model name in Models.
  - Recording toolbar: show the red dot, the time, Pause and Stop, and drop the title, which is visible in the detail. Keep it when the user is on another page, as a "Back to recording" item.
- **Files:** `ui/status_bar.rs`, `meetings/recorder.rs`.

**11. Pop-ups that still don't hug.**
- **Evidence:** "Auto" in a 200 pt field (`main-general-*`), "3 days" in 200 pt (`meetings-settings-*`), the OpenAI provider in about 320 pt (`main-postprocessing-*`).
- **Holds down:** #8, #15.
- **Fix:** width = widest option + 32 pt, clamped to 120–240 pt. Never stretch to fill the row. The Post Process provider becomes a 160 pt pop-up, with the API key field filling the rest of the row.
- **Files:** `kit.rs` `Dropdown`, `pages/post_processing.rs`, `meetings/settings.rs:1890`.

**12. Typographic hierarchy on settings pages.**
- **Evidence:** `main-general-*`, `main-advanced-*`. The toolbar title is 15 pt, group headers 13 semibold and rows 13 regular, so groups separate from rows by weight only. Meeting settings panes have a 17 pt heading; top-level settings pages do not.
- **Holds down:** #5.
- **Fix:** either group headers at 15 semibold with 24 pt above and 8 below, or a 20 pt semibold page heading at the top of the content (as Meeting settings has at 17). Pick one rule for both page families.
- **Files:** `design.rs` (`group_header`), `kit.rs` `group`.

**13. Empty states with a duplicate action.**
- **Evidence:**
  - `meetings-empty-*`: the toolbar Record and an empty-state Record, the same button twice.
  - `meetings-recording-wide-*`: a yellow "Generate Summary" floats alone at the top right of an empty Notes pane.
  - `main-postprocessing-*`: "Select a prompt to edit it." in a large empty panel while a prompt is listed beside it.
- **Holds down:** #27, #4.
- **Fix:**
  - Hide the toolbar Record while the empty state is shown, or keep the toolbar one and make the empty state text-only ("Click Record or press ⌥⇧M").
  - Put "Generate Summary" inside the Summary empty block, under its sentence.
  - Preselect the first prompt.
- **Files:** `meetings/mod.rs`, `meetings/summary.rs`, `pages/post_processing.rs`.

**14. Storage list chrome.**
- **Evidence:** `main-storage-*`. There is a folder icon on each of ten rows, and a "Shared" badge on the row already called "Shared models".
- **Holds down:** #40, #30.
- **Fix:** show the folder action on row hover and selection, keeping it keyboard reachable. Drop the "Shared" badge.
- **File:** `pages/storage.rs`.

**15. Small targets.**
- **Evidence:**
  - "Edit" in the summary (`meetings-detail-*`) is about 16 pt tall.
  - The transcript search and copy icons sit at the tab-strip edge, about 14 pt glyphs with no padding (`meetings-transcript-*`).
  - In-row buttons are 19 pt tall.
- **Holds down:** #20.
- **Fix:** 24 × 24 pt minimum hit areas, and put the transcript tools in the same 28 pt icon-button style as the header.
- **Files:** `meetings/summary.rs`, `meetings/detail.rs`, `kit.rs`.

**16. Startup skeleton doesn't match the shell.**
- **Evidence:** `startup-app-*`. The skeleton has 6 sidebar rows in one block and no toolbar title, while the real sidebar has 12 rows in 5 clusters with two indented. The layout jumps on load.
- **Holds down:** #28.
- **Fix:** draw the real sidebar labels, which are static, and skeleton only the content.
- **File:** `onboarding/startup.rs`.

**17. History toolbar icons unlabeled.**
- **Evidence:** `main-history-*`. A star (favorites filter) and a folder (open recordings) sit next to search with no labels. A star next to a search field is ambiguous: filter, or favorite this?
- **Holds down:** #16, #23.
- **Fix:** turn the star into a "Favorites" toggle button with a label, or make it a scope bar under search ("All | Favorites"). Move the folder into the More menu.
- **File:** `pages/history.rs`.

**18. Minimum size still keeps a full sidebar.**
- **Evidence:** `*-min-*`. The 200 pt sidebar stays at 720 pt, so General's rows alternate between inline and stacked layouts (`main-general-min-*`).
- **Holds down:** #34, #39.
- **Fix:** below 760 pt, narrow the sidebar to 160 pt (14 pt icons, 12 pt gap), or allow collapsing it with the standard sidebar toggle. Use one rule per group: if any row in a group must stack its control, stack them all.
- **Files:** `ui/main_window.rs`, `kit.rs` `SettingRow`.

**19. Dialog details.**
- **Evidence:** `advanced-limit-confirm-*`, `postprocessing-discard-*`. The alert uses a red warning glyph instead of the app icon with a caution badge. Which button is the default (Cancel) cannot be seen.
- **Holds down:** #43, #38.
- **Fix:** a 48 pt app icon with a 20 pt caution badge, and the default button drawn with the default style (accent ring or fill). Cancel is the default for destructive alerts. Add a snapshot that shows Esc dismissing.
- **File:** `kit.rs` (confirm).

**20. Transcript speaker names.**
- **Evidence:** `meetings-transcript-*`, `meetings-find-*`. Speaker names are 13 pt regular next to 14 pt body text, so turns scan poorly.
- **Holds down:** #2, #5, #31.
- **Fix:** names in 13 pt medium `tone.text`, with the time in 12 pt muted.
- **File:** `meetings/detail.rs`.

Problems 1 and 2 block a 9 overall. Problem 3 blocks a 9 on navigation, Problem 4 on focus, and Problem 5 on #3 and #41.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 1 | Typography | Speaker names medium. "Edit" and "Generate Titles" at 13 pt. |
| 2 | Font weight | Medium speaker names. |
| 3 | Contrast | Scrub track and Storage free ≥ 3:1. Single-layer focus ring. |
| 4 | Information hierarchy | History without a duplicate title. One Record on the empty screen. |
| 5 | Typographic hierarchy | 15 pt group headers or a 20 pt page heading, one rule. |
| 6 | Design language | Three button styles, one checkbox colour. |
| 7 | Padding | AI catalog rows 8 × 12. |
| 8 | Width | Pop-ups clamp to 120–240 pt; Post Process provider 160 pt. |
| 9 | Color | Checkbox accent ink everywhere. Neutral slider track (`#8f8f8a` / `#767676`). |
| 10 | Consistency | Problem 1. |
| 11 | Alignment | Fixed reset slot and Models trailing slots. |
| 12 | Rhythm | Catalog rows 48 pt like the rest. |
| 13 | Grouping | Footnotes for caveats instead of 3-line descriptions. |
| 14 | Iconography | Laptop glyph for "Meetings on this Mac". Filled play, pause and stop everywhere. |
| 15 | Control styling | 24 pt for all regular controls (Problem 2). |
| 16 | Affordance | Label the History favorites toggle. |
| 17 | States | One disabled style; hover and pressed snapshots. |
| 18 | Selection | One selection token in every list. |
| 19 | Focus | Problem 4. |
| 20 | Target size | 24 pt hit areas (Problem 15). |
| 21 | Navigation | Rename the inner "General", toolbar back everywhere, label the category list. |
| 22 | IA | One visible home for "You"; labeled sidebar clusters. |
| 23 | Chrome | Status bar only while busy; no repeated title in the recording toolbar. |
| 24 | Elevation | History at min as a pushed detail, not an inline card. |
| 25 | Borders | Fine; keep. |
| 26 | Radius | Fine; keep. |
| 27 | Empty states | Problem 13. |
| 28 | Loading | A skeleton that matches the real sidebar. |
| 29 | Errors | One AI key warning with words. |
| 30 | Density | No duplicate History text; one-line descriptions. |
| 31 | Scannability | Fixed Models slots; medium speaker names. |
| 32 | Microcopy | Problem 7. |
| 33 | Numeric | "60 ms"; consistent decimals in prices ("$0.80 / $4"). |
| 34 | Min reflow | Narrow or collapsible sidebar under 760 pt; one stacking rule per group. |
| 35 | Wide | Let the meeting list grow to 280 pt at 1440. |
| 36 | Dark | Neutral slider track. |
| 37 | Light | Scrub track and free segment ≥ 3:1. |
| 38 | Native fit | 24 pt controls, a native-style focus ring, status bar only while busy. |
| 39 | Content-first | Narrower sidebar at min. |
| 40 | Noise | Hover-only Storage folder icons, one "default", neutral "Recommended" chip. |
| 41 | Non-text | Problem 5. |
| 42 | Truncation | No truncated History title; no truncated recording toolbar title. |
| 43 | Dialogs | App icon with a caution badge; a visible default button. |
| 44 | HUD | Filled stop in the expanded widget. |
| 45 | Motion | Verify Reduce Motion on the overlay and widget in a snapshot. |

---

## 5. Functional findings (not visual)

1. "Generate Summary" is still offered as the yellow primary while a meeting is recording (`meetings-recording-wide-*`). Backlog #4.
2. The overlay timer still reads 0:00 while text is mid-sentence (`overlay-live-*`). Backlog #7.
3. Every older meeting in the fixture still starts at 14:30 (`main-meetings-*`). Backlog #8.
4. History shows "Recording not found" for every fixture entry; if no audio was ever recorded, the strip could be hidden (`main-history-*`). Backlog #10.
5. "Open in People" (`msettings-you-*`) and the People "You" row (`meetings-people-*`) are two entry points to the same voice enrollment. Choosing one home is an IA change, out of scope. Noted once.
6. Sound theme, Check for updates and Preview what's new still live in Debug, and API keys still have two homes (Post Process vs AI summaries). These are out of scope per the brief; noted once and not scored.
7. `kit::spinner` (`kit.rs:83`) rotates regardless of Reduce Motion. That is acceptable for a progress glyph, but confirm the intent.
8. The Post Process prompt detail shows "Select a prompt to edit it." while "Improve Transcriptions" is listed. Check whether a prompt should be preselected on open (`main-postprocessing-*`).
9. The onboarding cloud setup lets "Finish" stay enabled with an empty key (`onboarding-setup-cloud-*`). Backlog #5, unchanged.
