# Guidy UI review, round 1: reviewer `opus`

Evidence: 60 round-1 snapshots viewed in both themes: every main section at default size, plus `-min`, `-wide` and `-full` for General, Models, Advanced, Meetings, Usage and Meeting settings AI. Also all meetings states (detail, transcript, playing, recording, recording-wide, processing, empty, find), People and person, Meeting settings panes (setup, recording, transcription, speakers, AI, linear), onboarding (welcome, permissions, picker, setup, setup-cloud-min, migration), startup and startup error, both dialogs, the overlay and both widget states. I compared these with round 0 and cropped them with Pillow (scratch venv, `python -I`). Contrast was measured on screenshot pixels using WCAG 2.x relative luminance.

I treated the round-1 log in `README.md` as a claim to check. Most of it holds. Three claims do not hold:

- **"Centered alerts".** The alert is still top-anchored, has no icon, and keeps a pale destructive button.
- **"Debug split no longer clips".** At default size the settings panel is still cut mid-row; a fade now hides the cut.
- **"Duplicated description fragments" fixed.** Only Models is fixed; the onboarding picker still repeats fragments.

## Measured facts

| What | Round 0 | Round 1 |
|---|---|---|
| UI font | Geist, Geist Mono for numbers and badges | SF Pro with tabular figures; mono only for model IDs and raw errors |
| Page title / group header / row / helper (cap-height at 2x) | 13 / 11 muted / 13 / 11 | 15 semibold / 13 semibold / 13 / 12 |
| Dark surfaces: sidebar / content / panel | `#1b1b1b` / `#0d0d0d` / `#141414` (1.05:1) | `#202020` / `#181818` / `#222222`: sidebar vs content 1.09:1, panel vs content 1.12:1 |
| Light surfaces: sidebar / content / panel | n/a | `#f1f1ef` / `#fafaf9` / `#ffffff`: 1.08:1 and 1.04:1 |
| Off switch track vs panel | 1.42 (L) / 1.32 (D) | 3.04 / 3.21 |
| On switch vs panel | 1.56 | 4.03 (L, `#a87500`) / 5.21 (D, `#c08a00`); knob on track 4.03 / 3.05 |
| Input / pop-up border vs panel | 1.46 / 2.08 | 3.25 / 3.50 |
| Sidebar selection vs sidebar | 1.23 (L) | 1.26 (L) / 1.39 (D), plus accent icon |
| Slider unfilled track vs panel | n/a | 2.06 (L, beige `#beb49b`) / 2.30 (D, olive `#64593e`) |
| Overlay live dot | 1.31 (L) | 5.40 (L) / 5.49 (D) |
| Meetings toolbar live dot (`meetings-recording-*`) | n/a | 2.24 (L) / 2.44 (D) at the captured pulse frame (dips to 45% opacity) |
| Destructive "Delete" label in alert | 3.31 (L) | 3.7 (L) / 4.7 (D) |
| Link text | 5.6 | 5.59 (L) / 10.2 (D) |
| Hairline vs panel | 1.22 (D) | 1.29 (L) / 1.26 (D) |
| Setting rows | 32–57 pt | one-line 45 pt, with description 57 pt, two-line description 73 pt; AI catalog two-line rows about 36 pt |

---

## 1. Scores

Guidy round 0 here is my round-0 scores mapped onto the agreed rubric. #13 (perceptual grouping) is new, and I scored round 0 for it at 5 from the round-0 evidence. `(k)` means a reference-app score from product knowledge.

| # | Parameter | Guidy R1 | Guidy R0 | Codex | Claude | Reason for the round-1 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 8 | 5 | 8 | 9 | SF Pro throughout. Reading 14 pt in `meetings-detail-*`, helper 12, mono only for IDs (`msettings-ai-*`) and errors (`startup-error-*`). |
| 2 | Font weight usage | 8 | 5 | 8 | 8 | 400/500/600 by role. 700 only on the onboarding display title (`onboarding-welcome-*`). Transcript speaker names are regular weight and could be medium. |
| 3 | Color contrast | 7 | 5 | 8 | 8 | Controls now 3.0–5.2:1. Still failing: the light destructive label (3.7:1, `advanced-limit-confirm-light`), the slider unfilled track (2.1/2.3:1, `msettings-speakers-*`) and the pulsing toolbar dot (2.2:1). |
| 4 | Information hierarchy | 7 | 4 | 8 | 8 | Meeting header is one labeled action plus two icons (`meetings-detail-*`). `meetings-empty-*` has two Record buttons. History detail title is a timestamp (`main-history-*`). |
| 5 | Typographic hierarchy | 7 | 4 | 8 | 8 | 17/15/13sb/13/12/11. The toolbar page title is only 2 pt above rows, and group headers are row-size, separated only by weight (`main-general-*`). |
| 6 | Design language | 7 | 4 | 8 | 9 | One token set across pages and HUDs. Onboarding is still a bordered card in the window (`onboarding-*`). Meeting settings navigates with a pop-up (`meetings-settings-*`). |
| 7 | Padding and spacing | 7 | 5 | 8 | 8 | Page 24 × 20 and groups 24 are applied everywhere. AI catalog rows have about 4 pt top padding, so titles touch dividers (`msettings-ai-light`). |
| 8 | Width and layout measure | 7 | 4 | 8 | 9 | 680 pt form and 960 pt catalog are centered (`main-models-wide-*`). Pop-ups are fixed at 200 pt, so "Auto" sits in a 200 pt field (`main-general-*`, `kit.rs:360`). |
| 9 | Color and theme | 7 | 4 | 8 | 8 | Accent is mostly disciplined. Leaks: the "You" avatar icon (`meetings-people-dark`), a yellow Record next to a neutral Record (`meetings-empty-*`), and olive/beige slider tracks. |
| 10 | Overall consistency | 6 | 3 | 8 (k) | 9 (k) | Two Record styles, outlined vs solid pause glyphs, `00:00` vs `0:00`, and toolbar actions bordered on About but plain on Post Process and Usage. Search fields with and without a glyph. |
| 11 | Alignment and grid | 7 | 5 | 8 | 9 | The reset slot now has a fixed x (`main-general-*`). In `meetings-detail-min-*` the tabs (x 420), content (432) and title (488) start at three different edges. |
| 12 | Vertical rhythm | 7 | 5 | 8 | 9 | Rows are consistent within settings, but 45/57/73 pt is taller than the 36/52 target. The AI catalog runs 36 pt for two lines. |
| 13 | Perceptual grouping | 7 | 5 | 8 | 8 | Groups 24 apart with an 8 pt header gap. Debug still mixes log level, What's New, updates and sound theme in one panel (`main-debug-*`). |
| 14 | Iconography | 7 | 5 | 8 | 8 | Clear Lucide sidebar set. "Meetings on this Mac" reuses the Models CPU glyph (`main-models-*`). Per-line play and speaker-edit icons repeat down the transcript. |
| 15 | Control styling and sizing | 7 | 5 | 8 | 8 | 24 pt controls and three button styles. Toolbar commands render as plain text ("Test Connection", "Clear Usage…"). Pop-ups don't hug their content. |
| 16 | Affordance and signifiers | 7 | 4 | 7 | 7 | Meeting commands are named. "Manage" (`main-storage-*`), "Edit…" (`main-models-wide-*`) and the Post Process "Test Connection" read as labels. |
| 17 | Interactive states | 7 | 5 | 8 | 8 | Selected, disabled (Copy All, Export, Test Connection), busy (Transcribing 62%) and pressed (find toggle) are visible. Hover is not visible in stills. |
| 18 | Selection clarity | 8 | 4 | 8 | 8 | Neutral fill in every list in both themes (`history-selected-*`, `msettings-ai-*`, `onboarding-picker-*`). Light sidebar is only 1.26:1, and focused vs unfocused is not shown. |
| 19 | Focus visibility | 6 | 5 | 7 (k) | 7 (k) | Only one focus state is visible: a heavy two-tone gold ring of about 4 pt (`meetings-find-*`). There is no evidence it is the same on other controls. |
| 20 | Target size | 7 | 6 | 8 | 8 | Icon buttons are 24 pt. "Edit" in the summary and per-line transcript icons are about 16–18 pt targets. |
| 21 | Navigation and wayfinding | 5 | 4 | 8 | 8 | Still sidebar › segmented Meetings/People/Settings › category pop-up › tabs (`meetings-settings-*`, `meetings-detail-*`). Meeting settings has no title. |
| 22 | Information architecture | 5 | 3 | 7 (k) | 8 (k) | Sound theme and Check for updates are still in Debug. API keys still have two homes ("shared with Post Process"). Update checks appear in three places. |
| 23 | Toolbar and window chrome | 7 | 5 | 8 | 8 | Title left at 15 pt, short toolbars. The status bar remains, though the download progress now earns some of its space. |
| 24 | Surface elevation and layering | 7 | 4 | 8 | 8 | Three tones are distinguishable in dark. The summary is no longer card-in-card. Onboarding still puts panels in a card in the window. |
| 25 | Borders and dividers | 7 | 5 | 8 | 9 | Hairlines about 1.27:1, used sensibly. Onboarding has a card border plus inner panel borders plus a footer rule. |
| 26 | Corner radius consistency | 8 | 7 | 8 | 8 | 4/5/8/10 plus HUD 14 applied by rule. |
| 27 | Empty states | 7 | 4 | 8 (k) | 8 (k) | `meetings-empty-*` has one icon, headline, sentence and action, but repeats the toolbar Record. `usage-error-*` and `storage-error-*` still leave a blank page. |
| 28 | Loading and progress | 8 | 6 | 8 (k) | 8 (k) | Named work with % and time (`main-models-*`, `meetings-processing-*`), static tracks. The startup skeleton has 6 sidebar rows vs the real 10 in 4 groups (`startup-app-*`). |
| 29 | Error and status messaging | 7 | 4 | 7 (k) | 7 (k) | Icon + word + color statuses (`msettings-setup-*`). Plain startup error. No "Copy Details" or "Try Again" on startup. Duplicate Refresh and Try Again on Usage. |
| 30 | Data density | 7 | 5 | 8 | 8 | Comfortable. Some descriptions run 2–3 lines (`msettings-recording-*`, `msettings-setup-*` speaker recognition). |
| 31 | Scannability | 7 | 4 | 8 | 8 | Stable columns in Models and Usage. Long prose descriptions in Advanced and Meeting settings, cut with "…" at `-min`. |
| 32 | Microcopy and capitalization | 7 | 3 | 8 | 9 | Mostly HIG casing now. Leftovers: "Show What's New", "Preview What's New", "Test Connection" as a row label, "On This Mac" next to "Cloud provider", picker "English only. · English only", "speaks English". |
| 33 | Numeric and tabular formatting | 8 | 5 | 8 | 7 | Tabular, right-aligned and with units (`main-usage-*`, `main-models-*`). Price basis stated. Minutes use `00:00` while the transcript uses `0:00`. |
| 34 | Minimum-size reflow | 7 | 4 | 7 (k) | 8 (k) | Detail stacks with a back arrow, Usage columns drop, names keep. Descriptions truncate with "…" (`main-advanced-min-*`, `main-models-min-*`). The sidebar never collapses. |
| 35 | Wide-size use of space | 8 | 5 | 8 | 8 | Models capped and centered. Meetings adds a transcript pane. Meeting settings AI at 1440 leaves about 150 pt of dead gutter between the category list and the form. |
| 36 | Dark-mode quality | 8 | 4 | 8 | 8 | Lifted surfaces and neutral selection; no glare except the yellow primaries. Olive slider track (`msettings-speakers-dark`). |
| 37 | Light-mode quality | 7 | 5 | 7 (k) | 8 (k) | Controls are visible on white. Weak sidebar selection (1.26:1). Beige slider track (`main-debug-light`). |
| 38 | Native platform fit | 6 | 4 | 7 | 7 | System font, source list and pop-ups. Custom top-anchored alerts without an icon, segmented navigation, card onboarding and a web-style status bar. |
| 39 | Content-first focus | 7 | 5 | 9 | 9 | Reading starts near the top (`meetings-detail-*`). At 900 pt the sidebar plus list still take 440 pt. |
| 40 | Visual noise and restraint | 7 | 4 | 8 | 9 | Much quieter. Repeated per-line transcript icons, ten folder icons in Storage, and "default" shown twice in AI. |
| 41 | Non-text indicators | 7 | 4 | 7 | 7 | Dots come with words, and switches are ≥ 3:1. Slider tracks are 2.1–2.3:1, and the toolbar live dot pulses down to 2.2:1. |
| 42 | Truncation and text accommodation | 7 | 3 | 8 | 6 | Names never lose to chips now. Descriptions still truncate at default size ("Fast and accurate. Sup…", `main-models-light`). "Audio transcri…" at min. |
| 43 | Dialogs and modality | 5 | 4 | 7 (k) | 8 (k) | No icon, top-anchored, pale red destructive button (`advanced-limit-confirm-*`). The discard dialog is missing from `postprocessing-discard-*`. |
| 44 | HUD and overlay design | 8 | 6 | 7 (k) | 7 (k) | Upright text, red dot 5.4:1, right-hand tabular timer (`overlay-live-*`). The pill shows time, pause and stop (`recording-widget-*`). Pause glyph is outlined in expanded mode but solid in the pill. |
| 45 | Motion and transitions | 6 | 5 | 8 (k) | 8 (k) | Sheen loops are gone. The meetings toolbar live dot still pulses forever and ignores Reduce Motion (`recorder.rs:262`). |

| Column | Mean of 45 |
|---|---|
| Guidy round 1 | **7.0** |
| Guidy round 0 (on the agreed rubric) | **4.5** |
| Codex | **7.8** |
| Claude | **8.0** |

**Holistic "would the lead designer ship this" (Guidy round 1): 6.** The page-level craft is now professional. What stops sign-off is structure: Meetings navigation and settings IA, alerts, and onboarding.

---

## 2. Regressions since round 0

1. **Compact meeting player squeezes and collides.**
   - **Evidence:** `meetings-playing-dark/light` (playing state, 900 pt). The scrub track is about 50 pt wide and runs under the "32:00" end label. The thumb overlaps the track start, and the played portion has no visible fill.
   - **Round 0:** a full-width scrubber with a yellow fill.
   - **Fix:** give the player its own row under the metadata when the header is narrower than about 560 pt, with the track at least 160 pt. Keep the end label outside the track with an 8 pt gap. Show the fill in `tone.text_muted` at 3:1.
   - **File:** `meetings/player.rs`, `meetings/detail.rs`.
2. **Toolbar commands lost their button shape.**
   - **Evidence:** `main-postprocessing-*`. "Test Connection" went from `.outline()` to `.ghost()` (`post_processing.rs:1455`) and now reads as a label. The same goes for "Manage" (`pp-builtin-manage`) and "Clear Usage…" in `main-usage-*`. Meanwhile "Check for Updates" on About is still bordered.
   - **Fix:** toolbar text commands use the secondary (bordered) style. Ghost is only for icon buttons and in-row links.
3. **Discard-changes alert no longer appears in its snapshot.**
   - **Evidence:** `postprocessing-discard-light/dark`. No dialog, and "Meeting notes" is selected showing the default prompt. In round 0 the dialog showed.
   - **Next step:** either the harness edit missed `pp-prompt-text` after the layout change, or the unsaved-changes guard no longer fires. Verify before the next round, because a 9 on #43 cannot be checked without this screen. Also listed under functional findings.
4. **Meeting settings navigation became a pop-up at 900 pt.**
   - **Evidence:** `meetings-settings-*`, `msettings-*` at default size. The category list was replaced by a 220 pt "General ⌄" pop-up that acts as page navigation. A pop-up is a value picker on macOS, not navigation, and it hides the eight categories and the Setup badge ("Setup · 3").
   - **Assessment:** this is less discoverable than round 0's list, even though it frees width.

---

## 3. Remaining problems, ranked

**1. Meetings still has 3–4 navigation levels (blocks a 9).**
- **Evidence:**
  - `meetings-detail-*`: sidebar › segmented Meetings | People | Settings in the title bar › list › Notes | Action Items | Tickets | Transcript.
  - `meetings-settings-*`: sidebar › segmented › category pop-up › page, with no page title.
- **Holds down:** #21, #22, #6, #38, #10.
- **Fix:**
  - Move People into the sidebar Library cluster (History, Meetings, People).
  - Fold Meeting settings into the settings cluster as a "Meeting notes" page with sections, or one sidebar item per category (Recording, Transcription, Speakers, AI summaries, Integrations).
  - Drop the title-bar segmented control and put the 15 pt page title there instead.
  - Make the detail tabs a plain 28 pt tab strip with a 2 pt underline in `tone.text`, not a segmented control.
- **Files:** `ui/main_window.rs`, `pages/meetings/mod.rs`, `pages/meetings/settings.rs`.

**2. Settings IA unchanged (blocks a 9).**
- **Evidence:**
  - `main-debug-*`: Sound theme, Check for updates and Preview What's New sit in Debug.
  - `msettings-ai-*` says "shared with Post Process", so API keys are entered in two places.
  - "Check for updates" appears in the status bar, About (`main-about-*`) and the Debug toggle.
  - Sidebar clusters are unlabeled.
- **Holds down:** #22, #13, #21.
- **Fix:**
  - Move Sound theme next to Audio feedback in General › Microphone & sound.
  - Put the update toggle and button in About.
  - Create one "AI providers" section (in Models or a new page) that owns all keys; Post Process and AI summaries point to it.
  - Add 11 pt muted cluster labels (Library, Settings, Diagnostics) or keep the gaps but order them by intent.
- **Files:** `pages/debug.rs`, `pages/general.rs`, `pages/about.rs`, `pages/post_processing.rs`, `meetings/ai_settings.rs`, `status_bar.rs`.

**3. Alerts are not macOS alerts (blocks a 9).**
- **Evidence:** `advanced-limit-confirm-*`. The alert is top-anchored (about 115 pt from the top), has no app or caution icon, and the destructive button is pale pink `#f0d3d1` with red text at 3.7:1 in light. The discard alert is missing (Regression 3).
- **Holds down:** #43, #38, #3.
- **Fix:**
  - Center the alert in the window, or attach it as a sheet under the title bar.
  - 400 pt wide with a 48 pt app icon (caution badge for destructive alerts), 13 pt semibold title and 12 pt body.
  - Destructive button: solid `#d6403a` fill with a white label (4.5:1), 24 pt tall. Cancel is the Esc and default button.
  - Scrim `#00000033`.
- **File:** `kit.rs` (confirm/alert).

**4. Onboarding is still a window inside a window (blocks a 9).**
- **Evidence:** `onboarding-welcome-*`, `-picker-*`, `-setup-*`, `-migration-*`, `startup-error-*`. A bordered 600 pt card sits on the window background, 40–55% of it empty, with its own footer rule.
- **Holds down:** #6, #24, #25, #38, #39.
- **Fix:** remove the card border and fill. Use a full-window assistant: 560 pt centered column, 48 pt top margin, footer pinned to the window bottom with a hairline. On short steps, center the content block vertically so it isn't top-heavy.
- **File:** `onboarding/assistant.rs`.

**5. Consistency leftovers: one concept drawn two ways.**
- **Evidence:**
  - Record: neutral with a red dot in the toolbar, yellow with a mic in `meetings-empty-*`, both on the same screen.
  - Pause: outlined "▯▯" in `meetings-playing-*`, `meetings-recording-*` and the expanded widget, solid in the collapsed pill.
  - Timecodes: `00:00` in minutes (`meetings-detail-wide-*`) vs `0:00` in the transcript.
  - Search fields: with a glyph everywhere except `onboarding-picker-*`.
  - Grouping: History has no day groups while Meetings does.
- **Holds down:** #10, #9, #14.
- **Fix:**
  - Hide the empty-state Record when the toolbar Record is visible, or make the empty-state button the same neutral style.
  - Use the filled `pause` glyph everywhere.
  - One timecode formatter, `m:ss` (`h:mm:ss` above an hour).
  - One `kit::search_field`.
- **Files:** `meetings/list.rs`, `meetings/player.rs`, `meetings/summary.rs`, `onboarding/model_picker.rs`, `pages/history.rs`.

**6. Non-text contrast still short in a few places.**
- **Evidence:**
  - Slider unfilled track: 2.06:1 light beige `#beb49b`, 2.30:1 dark olive `#64593e` (`msettings-speakers-*`, `main-debug-*`).
  - Toolbar live dot: pulses to 45% opacity, 2.2–2.4:1 (`meetings-recording-*`).
  - Light sidebar selection: 1.26:1.
  - Dark on-switch knob vs track: 3.05:1, borderline.
- **Holds down:** #3, #41, #37, #18.
- **Fix:**
  - Unfilled track uses the neutral control border token (`#8f8f8a` light, `#767676` dark, ≥ 3:1), not an accent tint.
  - Live dot stays solid: pulse a 2 pt halo instead, and only when Reduce Motion is off.
  - Light sidebar selection `#d9d9d5` → about `#d2d2cd` (≥ 1.35:1) and keep medium weight.
  - Dark on-switch: `#121212` knob on `#c08a00` (6.5:1).
- **Files:** `assets/themes/guidy.json` (`slider.*`), `meetings/recorder.rs:262`, `design.rs` `tone()`.

**7. Plain-text commands read as labels.**
- **Evidence:**
  - `main-postprocessing-*`: "Test Connection" in the toolbar.
  - `main-usage-*`: "Clear Usage…".
  - `main-storage-*`: "Manage".
  - `main-models-wide-*`: "Edit…".
  - `msettings-linear-*`: "Test Connection" is both the row title and the button.
- **Holds down:** #15, #16, #10.
- **Fix:**
  - Toolbar commands use the secondary bordered button (24 pt, 12 pt horizontal padding).
  - In-row actions use either secondary buttons or accent links (`#8a6000` / `#ffc72c`) with underline on hover.
  - Rename the Linear row to "Connection" with the description "Check the key with Linear."
- **Files:** `pages/post_processing.rs:1455`, `pages/usage.rs`, `pages/storage.rs`, `pages/models.rs`, `meetings/settings.rs`.

**8. History detail is still weak.**
- **Evidence:** `main-history-*`, `history-selected-*`, `history-undo-light`.
  - The detail title is a 17 pt timestamp.
  - Four unlabeled icons (copy, star, reprocess, delete).
  - The "Delete entry" tooltip floats 28 pt above its button over the search field.
  - The undo toast is a strip in the list footer.
- **Holds down:** #4, #16, #21, #27.
- **Fix:**
  - Title: the first sentence (17 semibold, 2 lines max), with the date as 12 pt metadata below it.
  - Actions: a labeled "Copy" plus a "…" menu (Favorite, Reprocess, Delete).
  - Anchor tooltips to their trigger.
  - Undo: a floating toast bottom-center in the detail pane with `raised` fill, 8 pt radius and a bordered "Undo".
- **File:** `pages/history.rs`.

**9. Debug panel still clips at default size.**
- **Evidence:** `main-debug-light/dark`. "Word correction threshold" is cut mid-description behind a fade.
- **Holds down:** #13, #42, #22.
- **Fix:** after Problem 2 moves the user settings out, Debug is just log level plus logs. Otherwise use a resizable split with a visible 1 pt splitter and a minimum of 3 rows, or tabs (Logs | Tuning).
- **File:** `pages/debug.rs`.

**10. Pop-ups and shortcut fields don't hug their content.**
- **Evidence:** `main-general-*` ("Auto" and "Default" in 200 pt fields), `main-postprocessing-*` (provider pop-up about 320 pt), `meetings-settings-*` ("3 days" 200 pt). `kit.rs:360` hard-codes `min_width: dp(200.)`.
- **Holds down:** #8, #15, #11.
- **Fix:** trigger width = content + 32 pt, clamped to 120–240 pt, and right-aligned in the trailing column. The shortcut field hugs its keycaps with a 160 pt minimum.
- **File:** `kit.rs` `Dropdown`.

**11. Row heights taller than the rhythm target.**
- **Evidence:**
  - `main-general-*`: Microphone single-line row 45 pt, Mute row 57 pt, Dictation key 73 pt.
  - `msettings-ai-*`: catalog two-line rows are about 36 pt with about 4 pt top padding, cramped.
- **Holds down:** #12, #7, #30.
- **Fix:**
  - Single-line rows: 36 pt min with `ROW_Y` 10 → 9. The 45 pt comes from a 24 pt control plus 10+10 padding plus the line box; use 6 pt vertical padding when the control is the tallest element.
  - Rows with a description: 52 pt (title 18 + 2 + helper 16 + 16 padding).
  - Catalog rows: 8 × 12 padding, 44 pt.
- **Files:** `design.rs` (`ROW_Y`, `SECTION_Y`), `kit.rs` `SettingRow`, `meetings/ai_settings.rs`.

**12. Description truncation and long prose.**
- **Evidence:**
  - `main-models-light`: "Fast and accurate. Sup…" at default size.
  - `main-advanced-min-*`: "when the model hasn't been used for…".
  - `msettings-setup-*`: the speaker-recognition paragraph runs two lines with model jargon.
  - `main-advanced-full-*`: 2–3 line option explanations.
- **Holds down:** #42, #31, #30, #32.
- **Fix:** descriptions at most 72 characters, wrapped, never ellipsized. Move option semantics into `DropdownOption::description`. On Models, wrap descriptions onto a second line when chips are present, and put "Required by" on its own caption line, as `-min` already does.
- **Files:** i18n, `pages/models.rs`, `pages/advanced.rs`.

**13. Microcopy leftovers.**
- **Evidence:**
  - Title Case labels: "Show What's New" (`main-about-*`), "Preview What's New" (`main-debug-*`), "On This Mac" next to "Cloud provider" (`msettings-ai-*`).
  - Duplicates: "Very fast, English only. · English only" and "Supports 25 European languages. · 25 languages" (`onboarding-picker-*`).
  - Unclear: "speaks English" (`onboarding-setup-*`). "Old format" chip on the new model (`onboarding-migration-*`). "Clipboard (Cmd+V)" (`main-advanced-full-*`).
  - Single quotes: `"Confirm?"` in plain quotes.
- **Holds down:** #32, #10.
- **Fix:**
  - Sentence-case the labels ("Show what's new", "Preview what's new", "On this Mac").
  - Run the picker through `kit::model_description` like Models does.
  - Replace "speaks English" with "English UI language" or drop it.
  - Attach "Old format" to "Replaces Parakeet V2 (old format)".
  - Use "Clipboard (⌘V)" and curly quotes.
- **Files:** `assets/locales/en/translation.json`, `onboarding/model_picker.rs`, `onboarding/migration.rs`.

**14. Focus ring is heavy and unproven elsewhere.**
- **Evidence:** `meetings-find-*` shows a dark-gold 1 pt border plus a 4 pt light-gold halo, about 6 pt of ring on a 24 pt field. No other focused control is captured.
- **Holds down:** #19, #38.
- **Fix:** one ring on every focusable control: 2 pt `#c08a00` (light) / `#ffc72c99` (dark) outside a 1 pt gap, no second halo. Add a focus snapshot for a button, a pop-up and a list row.
- **Files:** `assets/themes/guidy.json` (`ring`), `kit.rs`, snapshot harness.

**15. Error pages leave the content area empty.**
- **Evidence:** `usage-error-*`, `storage-error-*`. A single banner sits on a blank page, with both a toolbar Refresh and an in-banner Try Again. `startup-error-*` has only Quit and Show Logs, with no way to copy details or retry.
- **Holds down:** #27, #29.
- **Fix:**
  - A centered error block: 32 pt icon, 17 pt title ("Usage isn't available"), a one-sentence cause and one "Try Again" button. Hide the toolbar Refresh while in error.
  - Startup: add "Copy Details" (plain) and make "Try Again" the primary if retry is possible; otherwise keep Show Logs.
- **Files:** `pages/usage.rs`, `pages/storage.rs`, `onboarding/startup.rs`.

**16. Transcript per-line chrome.**
- **Evidence:** `meetings-transcript-*`, `meetings-find-*`. Every turn has a left ▷ and a right person-pen icon at full opacity. Speaker names are 13 pt regular, so they barely separate from the 14 pt text.
- **Holds down:** #40, #5, #14.
- **Fix:**
  - Show ▷ and edit only on hover or for the selected line (keep them keyboard reachable).
  - Speaker names 13 pt medium in `tone.text`, with the time as 12 pt muted.
  - 12 pt between turns instead of about 20.
- **File:** `meetings/detail.rs` (transcript).

**17. Status bar duplicates chrome.**
- **Evidence:** every `main-*` shows "Parakeet V3" on the left and "Check for updates" on the right. Only the download progress (`main-models-*`) is unique.
- **Holds down:** #23, #22, #39.
- **Fix:** show the bar only while downloads run, or move download progress into a toolbar progress item that opens Models. Remove "Check for updates" from it.
- **File:** `ui/status_bar.rs`.

**18. The Reduce Motion gap.**
- **Evidence:** `recorder.rs:262-265` runs `Animation::new(1600 ms).repeat()` opacity 1 → 0.45 without checking `cx.reduce_motion()`. Overlay and widget do check it.
- **Holds down:** #45, #41.
- **Fix:** gate the animation on `!cx.reduce_motion()` and animate a halo, not the dot's opacity.
- **File:** `meetings/recorder.rs`.

**19. Alignment at minimum size.**
- **Evidence:** `meetings-detail-min-*`. The back chevron sits at x 440, the title at 488, the tabs at 420 and the body at 432.
- **Holds down:** #11, #34.
- **Fix:** place the back button in the toolbar (leading), so title, metadata, tabs and body share the 24 pt page edge.
- **File:** `meetings/detail.rs`, `meetings/mod.rs`.

**20. Wide Meeting settings leaves a dead gutter.**
- **Evidence:** `msettings-ai-wide-*`. About 150 pt of empty space sits between the category list and the centered 680 pt form.
- **Holds down:** #35, #11.
- **Fix:** when a secondary list is present, left-anchor the form 32 pt from the list (max 680) instead of centering it in the remaining space.
- **File:** `meetings/settings.rs`.

Problems 1–4 block a 9. Problems 5 and 6 block a 9 on their parameters (#10, #3, #41).

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 1 | Typography | Speaker names medium. Captions ("Generate Titles (4)", tab counts) at 12 pt where they carry commands. |
| 2 | Font weight | Medium for list titles and speaker names. Semibold only for titles and group headers. |
| 3 | Contrast | Solid destructive alert button, neutral ≥ 3:1 slider tracks, no opacity pulse on the live dot. |
| 4 | Information hierarchy | One Record per screen. History title from content. Setup rows with one action each (put "Open System Settings" in the row's "…"). |
| 5 | Typographic hierarchy | Page title 17–20 pt at the top of the content (or keep 15 in the toolbar and make group headers 15). |
| 6 | Design language | Full-window onboarding, no pop-up navigation, no segmented navigation. |
| 7 | Padding and spacing | AI catalog rows 8 × 12 padding. Trim single-line rows to 36 pt. |
| 8 | Width and measure | Pop-ups hug content (120–240 pt). |
| 9 | Color and theme | Neutral "You" icon, neutral slider tracks, one Record style. |
| 10 | Consistency | Shared player and pause glyph, one timecode format, one search field, toolbar buttons all bordered, day groups in History. |
| 11 | Alignment | One leading edge in the min-size detail. Left-anchor forms beside secondary lists. |
| 12 | Vertical rhythm | 36/52 rows and 44 pt list rows everywhere. |
| 13 | Grouping | Split Debug and move user settings out. |
| 14 | Iconography | Distinct glyph for "Meetings on this Mac" (laptop). Hide per-line transcript icons until hover. Filled pause. |
| 15 | Control styling | Bordered toolbar commands, hugging pop-ups. |
| 16 | Affordance | Label History actions. Make Manage/Edit… buttons or links. |
| 17 | Interactive states | Add a hover/pressed snapshot set to prove the states. |
| 18 | Selection | Light sidebar selection ≥ 1.35:1. Show the focused selection in one snapshot. |
| 19 | Focus | One 2 pt ring on all controls, plus snapshots. |
| 20 | Target size | 24 pt minimum for "Edit", per-line transcript buttons and the speakers disclosure. |
| 21 | Navigation | Remove the segmented navigation and settings pop-up (Problem 1). |
| 22 | IA | One home per setting: Sound theme to General, updates to About, keys in one place. |
| 23 | Toolbar and chrome | Status bar only while downloading. |
| 24 | Elevation | Remove the onboarding card. |
| 25 | Borders | Drop the onboarding card border and footer double rule. |
| 26 | Radius | Use the 10 pt dialog radius on the alert and 8 on the onboarding inner panels. Nothing else. |
| 27 | Empty states | Centered error blocks. No duplicate Record. |
| 28 | Loading | Skeleton sidebar matching the 4 grouped clusters. |
| 29 | Errors | Startup "Copy Details" / "Try Again". One retry control on Usage and Storage. |
| 30 | Density | Descriptions ≤ 72 characters. Option semantics in pop-up items. |
| 31 | Scannability | No ellipsized descriptions. "Required by" on its own caption line. |
| 32 | Microcopy | Fix the leftovers in Problem 13. |
| 33 | Numeric | One timecode format. |
| 34 | Min reflow | Wrap descriptions instead of ellipsizing. Collapsible sidebar under 760 pt. |
| 35 | Wide | Left-anchored form next to the category list. |
| 36 | Dark mode | Neutral slider track, dark knob on the on-switch. |
| 37 | Light mode | Stronger sidebar selection, neutral slider track. |
| 38 | Native fit | Native-style alerts or sheets, sidebar navigation instead of segmented controls, status bar removed. |
| 39 | Content-first | Collapse the meetings list to 240 pt or make it resizable at 900 pt. |
| 40 | Visual noise | Hover-only per-line icons, one folder action per Storage row on hover, one "default" marker. |
| 41 | Non-text indicators | Slider tracks ≥ 3:1, solid live dot. |
| 42 | Truncation | Wrap model descriptions. Abbreviate the "Audio transcribed" stat label at min ("Audio"). |
| 43 | Dialogs | Problem 3, and restore the discard snapshot. |
| 44 | HUD | Filled pause glyph. Overlay timer starts with audio (verify). |
| 45 | Motion | Gate the recorder pulse on Reduce Motion and animate a halo instead. |

---

## 5. Functional findings (not visual)

1. `postprocessing-discard-*` no longer shows the discard alert. Either the harness's click on `pp-prompt-text` misses after the layout change, or the unsaved-changes guard no longer fires when switching prompts. Verify; this could silently drop prompt edits.
2. The History "Delete entry" tooltip still renders about 28 pt above its trigger, over the toolbar search field (`history-undo-light`).
3. The onboarding picker still concatenates duplicate description fragments ("English only. · English only"). Models is fixed, so the picker is not using the same helper.
4. "Generate Summary" is still offered as the yellow primary while a meeting is recording (`meetings-recording-wide-*`).
5. The meetings toolbar live-dot pulse ignores Reduce Motion (`recorder.rs:262`).
6. Usage error: toolbar Refresh and in-banner Try Again do the same thing (Clear Usage… is now correctly disabled).
7. The compact player's track overlaps the end-time label while playing (`meetings-playing-*`). Check the hit area of the end of the scrubber.
8. History still shows "Recording not found" for every fixture entry. Consider hiding the strip when no audio was ever recorded.
9. The startup skeleton shows 6 sidebar rows while the real sidebar has 10 in 4 groups, so the layout jumps on load (`startup-app-*`).
10. Every older meeting in the fixture starts at 14:30 (unchanged from round 0); check the list's time formatting against real start times.
11. AI summaries still lists the default model twice (radio "Your models" and the checked catalog row, both marked "default").
