# Guidy UI review, round 11: reviewer `opus`

## Evidence

I opened 57 PNGs with the image tool: 56 round-11 captures and one round-10 capture for comparison. I also measured pixels with a pure-Python PNG decoder (Pillow is not installed), run with `python3 -I` from a scratch directory. The captures cover both themes and the default, `-min`, `-wide` and `-full` sizes:

- **Settings pages:** General (dark, min light, full light), `focus-5-dark`, `focus-9-light`, `hover-button-light`, Advanced, `advanced-limit-confirm-dark`, Post Process (min dark), `postprocessing-discard-light`, Models (light, min dark, wide dark), Usage (dark), Storage (light, min dark), `storage-error-light`, Debug (light), `debug-logs-dark`, About (dark).
- **History:** default, `history-selected-min-dark`.
- **Meetings:** `main-meetings-light`, detail (dark, min light), transcript (dark), `meetings-tab-focus-dark`, processing (dark), empty (light), `meetings-recording-wide-light`.
- **People:** `meetings-people-light`, `mpeople-person-light`, `mpeople-person-min-dark`.
- **Meeting settings:** shell (dark), full-height (light), Setup (min light), Meeting notes, You, Recording, Transcription, Speakers, Linear, Integrations, AI (cloud min and wide; local default, which I also opened from round 10, and local min dark).
- **Onboarding and startup:** welcome, permissions (min), picker, cloud setup (min dark), `startup-app-light`, `startup-app-min-dark`, `startup-error-dark`.
- **HUDs:** overlay, widget and expanded widget.

### Diff against round 10

I byte-compared all 242 PNGs with round 10. 188 are identical. I diffed the other 54 by region, splitting at the sidebar edge (x 400 px):

| Capture | Sidebar | Content |
|---|---|---|
| `msettings-*`, `meetings-settings*` | y 533–1151 px: the category rows' new geometry and the new bottom hairline | Only the slider and AI-description changes below |
| `startup-app*` | y 310–456 px: the new order | Same |
| `main-storage*` | Same | Storage bar only (y 148–201 px) |
| `main-debug*`, `msettings-speakers*`, `main-general-full*` | Same | Slider tracks only |
| `msettings-ai-local*` | Changed | Model descriptions and badge |
| `advanced-limit-confirm-*`, `postprocessing-discard-*` | Same | The alert moved again (capture timing) |

Every content change maps to a logged item. The folder's files are timestamped 05:33–05:37, all before I began this review.

### Scope rules applied as before

- These are out of scope or recorded decisions, so they don't count against the score: the status bar, the stock 1 pt focus edges, the `kit::confirm` Return default, the fixed 200 pt pop-up column, and Meeting settings as a top-level row with its categories under it.
- The fixture's Debug and Post Process rows are discounted.
- `msettings-*` and `mpeople-person*` render without the shell's toolbar content (harness framing, unchanged since round 2).

## The round-11 log checked against the snapshots

| Claim in `README.md` | Verdict | Evidence |
|---|---|---|
| Startup placeholder uses the real order and metrics | **Holds** | `startup-app-light`, `startup-app-min-dark`: General, History, Meetings, then People indented (label x 146 px). Models 338, Advanced 390, Meeting settings 442 px, top level with its label at x 94 px, the same x as the loaded row in `main-general-light`. Then Usage, Storage, About. Without the fixture's Post Process and Debug, that is the loaded order. |
| People and the category rows share one child geometry (18 pt slot, 16 pt glyph, 8 pt gap; categories 24 pt tall) | **Holds** | `msettings-you-light`: People's glyph spans x 95–124 and its label starts at 146 px. Setup, Meeting notes, Recording and Integrations have glyphs inside x 95–124 and labels at 145–146 px. At `-min` (`msettings-setup-min-light`), People's label is at 114 px and the categories' at 113–114 px. Pitch: People 52 px, categories 48 px, as logged. |
| Storage: other used space in the track grey, ≥ 3:1 from Guidy's fill and from free space | **Holds** | Light: Guidy `#523800` vs Other `#8f8f8a` 3.36:1; Other vs free `#ffffff` 3.25:1. Round 10 was 1.81:1. Dark: `#ffc72c` vs `#707070` 3.17:1; `#707070` vs `#222222` 3.21:1 (`main-storage-*`). |
| Slider backing ≥ 3:1 from the fill and the panel in both themes | **Holds** | Light: fill `#523800` vs remainder `#908b7c` 3.21:1; remainder vs panel 3.40:1. Round 10 was 2.69:1. Dark: `#ffc72c` vs `#7b7051` 3.15:1; `#7b7051` vs `#222222` 3.24:1. Round 10 was 2.47:1 (`msettings-speakers-*`). |
| Local model descriptions joined with " · "; RAM badge hidden when the description gives the advice | **Holds** | "Fast, good summaries · Works on any Apple silicon Mac" matches `main-models-wide-dark` word for word. The 8B row has no badge, and its description wraps cleanly at `-min` (`msettings-ai-local-min-dark`). |
| A 1 pt hairline at the sidebar's bottom edge when rows continue below | **Holds, with a new defect** | The line is there only when rows overflow: present in `msettings-*` at default and min sizes; absent in `main-*`, `meetings-settings-full-*` and `msettings-ai-wide-*`. But it is drawn in the control-boundary grey and sits directly on top of the status bar's own hairline (Regression 1). |

### My round-10 problems

| Round-10 problem | Status |
|---|---|
| 1. Startup placeholder in the old order | **Fixed.** |
| 2. Two child-row geometries | **Fixed.** |
| 3. Light accent tones; fill vs remainder | **Contrast fixed** (slider 3.21:1, storage 3.36:1). There are still four light accent tones: `#ffc72c` primary, `#a87500` switches, `#8a6000` marks and links, `#523800` sliders, progress and storage. |
| 4. Description punctuation; a fact said twice | **Fixed** in the AI pane. |
| 5. Light button hover reads as pressed | **Not fixed** (`hover-button-light` is byte-identical). |
| 6. Cloud and local status in opposite orders | **Not fixed** (`msettings-ai-min-light`: "Anthropic key needed · Claude Sonn…"). |
| 7. Post Process wraps two rows at minimum size | **Not fixed** (`main-postprocessing-min-dark`, byte-identical). |
| 8. Two delete treatments | **Not fixed.** A labeled Delete in `msettings-ai-local-*`; a bare trash glyph in `msettings-ai-*`. |
| 9. Onboarding picker punctuation | **Not fixed.** Two different joiners in one list (Problem 7). |
| 10. People emails truncate at 900 pt | **Not fixed** (`meetings-people-light`, byte-identical). |
| 11. Set Up… chip at 12 pt | **Not fixed** (the `msettings-setup-*` content is unchanged). |
| 12. "System Settings > …"; category titles out of context | **Not fixed.** |
| 13. Harness hover on the Meeting settings row; alert positions | **Not fixed.** In `meetings-settings-light` the parent row's fill is `#e1e1e0`, the hover fill. Both alerts moved again. |

---

## 1. Scores

`(k)` means a reference-app score from product knowledge. The reference columns are unchanged from round 10.

| # | Parameter | Guidy R11 | Guidy R10 | Codex | Claude | Reason for the round-11 score |
|---|---|---|---|---|---|---|
| 1 | Typography | 9 | 9 | 8 | 9 | Unchanged. One family; mono only for IDs, paths and model IDs (`main-usage-dark`, `msettings-ai-wide-light`). |
| 2 | Font weight usage | 9 | 9 | 8 | 8 | Unchanged. |
| 3 | Color contrast | 9 | 9 | 8 | 8 | All measured text passes. Track fills now pass too (see #41). |
| 4 | Information hierarchy | 9 | 9 | 8 | 8 | Unchanged (`meetings-detail-dark`, `msettings-ai-local-light`). |
| 5 | Typographic hierarchy | 9 | 9 | 8 | 8 | Unchanged. |
| 6 | Design language | 9 | 9 | 8 | 9 | Unchanged. |
| 7 | Padding and spacing | 9 | 9 | 8 | 8 | Child slot 18 pt with an 8 pt gap; the 26 / 24 pt rows hold. |
| 8 | Width and layout measure | 9 | 9 | 8 | 9 | Unchanged. |
| 9 | Color and theme | 9 | 9 | 8 | 8 | Fills now separate, but there are still four light accent tones. |
| 10 | Overall consistency | 9 | 9 | 8 (k) | 9 (k) | One child-row geometry, and one description joiner across Models and AI. Left: two delete forms, opposite status orders, the onboarding picker's mixed joiners. |
| 11 | Alignment and grid | **9** | 8 | 8 | 9 | Fixed. People and category labels share x 146 px (113–114 px at `-min`), and glyphs share x 95–124 px (`msettings-you-light`, `msettings-setup-min-light`). |
| 12 | Vertical rhythm | 9 | 9 | 8 | 9 | Unchanged. Post Process still wraps at minimum size. |
| 13 | Perceptual grouping | 9 | 9 | 8 | 8 | Unchanged. |
| 14 | Iconography | 9 | 9 | 8 | 8 | Child glyphs are one size now. Two delete glyph forms remain. |
| 15 | Control styling and sizing | 9 | 9 | 8 | 8 | Unchanged (12 pt chip is a nit). |
| 16 | Affordance and signifiers | 9 | 9 | 7 | 7 | Unchanged. |
| 17 | Interactive states | 8 | 8 | 8 | 8 | Light hover is still `#c5c5c1` (`hover-button-light`); no `pressed-*` captures. |
| 18 | Selection clarity | 9 | 9 | 8 | 8 | Selected and focused `#c8c8c7` vs selected `#d2d2d0` (`meetings-settings-light`, `msettings-you-light`). |
| 19 | Focus visibility | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`focus-5-dark`, `focus-9-light`, `meetings-tab-focus-dark`). |
| 20 | Target size | 9 | 9 | 8 | 8 | Unchanged. |
| 21 | Navigation and wayfinding | 9 | 9 | 8 | 8 | Two levels at most, and the placeholder now previews the real tree. |
| 22 | Information architecture | 9 | 9 | 7 (k) | 8 (k) | Unchanged; the rest is out of scope. |
| 23 | Toolbar and window chrome | 9 | 9 | 8 | 8 | Unchanged. |
| 24 | Surface elevation and layering | 9 | 9 | 8 | 8 | Unchanged. |
| 25 | Borders and dividers | **8** | 9 | 8 | 9 | New double edge. In every overflowing sidebar, a 3:1 grey line (`#8f8f8a` / `#707070`) sits on the status bar's `#e3e3df` / `#333333` hairline, so the sidebar bottom gets a 2 pt two-tone edge. It steps down to 1 pt at x 398 px, where the content starts (`msettings-you-light`, `meetings-settings-dark`, `msettings-setup-min-light`). |
| 26 | Corner radius consistency | 9 | 9 | 8 | 8 | Unchanged. |
| 27 | Empty states | 9 | 9 | 8 (k) | 8 (k) | Unchanged (`meetings-empty-light`, `debug-logs-dark`). |
| 28 | Loading and progress | **9** | 8 | 8 (k) | 8 (k) | Fixed. The placeholder rows sit where the loaded rows will (`startup-app-*`). |
| 29 | Error and status messaging | 9 | 9 | 7 (k) | 7 (k) | Unchanged (`startup-error-dark`, `storage-error-light`). |
| 30 | Data density | 9 | 9 | 8 | 8 | The 8B RAM fact is said once. |
| 31 | Scannability | 9 | 9 | 8 | 8 | Unchanged. |
| 32 | Microcopy and capitalization | 9 | 9 | 8 | 9 | AI pane and Models agree. Picker joiners and "Settings > …" remain. |
| 33 | Numeric and tabular formatting | 9 | 9 | 8 | 7 | Unchanged. |
| 34 | Minimum-size reflow (720×480) | 9 | 9 | 7 (k) | 8 (k) | Unchanged. Post Process still wraps. |
| 35 | Wide-size use of space | 9 | 9 | 8 | 8 | Unchanged (`msettings-ai-wide-light`, `main-models-wide-dark`). |
| 36 | Dark-mode quality | 9 | 9 | 8 | 8 | Dark remainders are now a warm grey that reads as an unfilled track. |
| 37 | Light-mode quality | 9 | 9 | 7 (k) | 8 (k) | The heavy hover and the near-black brown fills remain. |
| 38 | Native platform fit | 9 | 9 | 7 | 7 | Unchanged. |
| 39 | Content-first focus | 9 | 9 | 9 | 9 | Unchanged. |
| 40 | Visual noise and restraint | 9 | 9 | 8 | 9 | Unchanged. |
| 41 | Non-text indicators | 9 | 9 | 7 | 7 | Every measured fill/remainder and remainder/panel pair is now ≥ 3:1 in both themes. |
| 42 | Truncation and text accommodation | 9 | 9 | 8 | 6 | Unchanged: "Claude Sonn…" at 720 pt, emails at 900 pt. |
| 43 | Dialogs and modality | 9 | 9 | 7 (k) | 8 (k) | Unchanged in content. |
| 44 | HUD and overlay design | 9 | 9 | 7 (k) | 7 (k) | Byte-identical to round 10. |
| 45 | Motion and transitions | 9 | 9 | 8 (k) | 8 (k) | Placeholder to loaded is now a fill-in, not a reorder. |

| Column | Mean of 45 |
|---|---|
| Guidy round 11 | **9.0** (403 / 45 = 8.96) |
| Guidy round 10 | **8.9** (402 / 45 = 8.93) |
| Codex | **7.8** (351 / 45) |
| Claude | **8.0** (361 / 45) |

**Holistic "would the lead designer ship this" (Guidy round 11): 9.**

- Both of my round-10 regressions are fixed, and so are Codex's two contrast findings and the duplicated RAM advice. I checked each in pixels.
- The one new defect is the overflow cue's execution: a heavier line stacked on an existing divider. It is a two-line fix.
- What still separates Guidy from a 10 is a short list of local polish items. None blocks use.

---

## 2. Regressions since round 10

1. **The sidebar overflow cue doubles the bottom divider.**
   - **Evidence:**
     - Light: rows 1150–1151 px are `#8f8f8a`, then the status bar's hairline at 1152–1153 px is `#e3e3df`.
     - Dark: `#707070`, then `#333333`.
     - The line spans only x 0–397 px. The content column keeps a single 1 pt hairline, so the bottom edge steps from 2 pt to 1 pt at the sidebar divider (`msettings-you-light`, `msettings-recording-dark`, `meetings-settings-dark`). At `-min` it is the same at y 910 px (`msettings-setup-min-light`).
     - The cue line is 2.87:1 against the sidebar (light) and 3.29:1 (dark). That is control-boundary strength, the darkest divider in the window. The other dividers are 1.1–1.3:1.
   - **Why it is new:** round 10 had no cue. The last row was simply clipped.
   - **Holds down:** #25, #10.

Nothing else got worse.

---

## 3. Remaining problems, ranked

Problems 1 and 2 hold #25 and #17 below 9. The rest are 9-to-10 polish.

**1. The overflow cue stacks on the status-bar hairline. Blocks 9 on #25.**
- **Evidence:** Regression 1.
- **Fix:**
  - Draw one line, not two. Under the sidebar, recolour the status bar's existing top hairline instead of adding a second line above it.
  - Use a separator-strong token, not the control-boundary grey: about `#c9c9c5` light (1.5:1 on `#f1f1ef`) and `#4a4a4a` dark (1.6:1 on `#202020`). The cue is a hint, not a boundary someone has to find.
  - If that reads too quiet, the native idiom is the scroll-edge treatment: a hairline that appears only once the list can scroll. That is what the current behaviour already approximates.
- **File:** `ui/main_window.rs` (sidebar footer), `ui/status_bar.rs`, `ui/design.rs`.

**2. The light button hover reads as pressed. Blocks 9 on #17.**
- **Evidence:** `hover-button-light`: "Check for Updates" fills `#c5c5c1` on a `#fafaf9` page. That is the depth of a pressed state.
- **Holds down:** #17, #37.
- **Fix:**
  - Hover about `#ededeb`, keeping the 1 pt border. Keep `#c5c5c1` for pressed.
  - Add `pressed-*` captures so the two states can be compared.
- **File:** `ui/design.rs`, `ui/kit.rs`, the snapshot harness.

**3. Four light accent tones.**
- **Evidence:** `#ffc72c` (primary), `#a87500` (switches, `main-advanced-light`), `#8a6000` (focus ring, links, radio), `#523800` (sliders, progress, storage; `msettings-speakers-light`, `main-models-light`, `main-storage-light`). The contrast pairs now pass, so this is about restraint, not legibility.
- **Holds down:** #9, #37 (9 → 10).
- **Fix:**
  - Fold `#a87500` and `#523800` into `#8a6000` for every non-text accent.
  - Against `#8a6000`, lighten the slider remainder to about `#c9c6bc` (≥ 3:1 from the fill) and add a `#8f8f8a` track edge for the panel boundary.
- **File:** `assets/themes/guidy.json`, `ui/design.rs`.

**4. The cloud and local AI status lines use opposite orders.**
- **Evidence:** "Anthropic key needed · Claude Sonn…" (`msettings-ai-min-light`) vs "Qwen3 4B Instruct · This Mac · offline" (`msettings-ai-local-*`).
- **Holds down:** #10, #31, #42.
- **Fix:** lead with the model name in both. Put the condition after it, in the semantic colour, only when action is needed. Truncate the provider tail first.
- **File:** `pages/meetings/ai_settings.rs`.

**5. Post Process at minimum size wraps two rows.**
- **Evidence:** `main-postprocessing-min-dark`. API key and Model drop their controls to a second line, while every other page keeps the 200 pt control column inline at 720 pt.
- **Holds down:** #12, #34.
- **Fix:** keep the control inline (`min_w(200)`, `flex_1`) until the label column would drop below 180 pt.
- **File:** `pages/post_processing.rs`.

**6. Two delete treatments in one pane.**
- **Evidence:** a labeled "Delete" button (`msettings-ai-local-light`) vs a bare 12 pt trash glyph (`msettings-ai-min-light`, `msettings-ai-wide-light`).
- **Holds down:** #10, #14.
- **Fix:** one form in both places: a 24 pt icon button with a "Remove" tooltip, or a labeled "Remove".
- **File:** `pages/meetings/ai_settings.rs`.

**7. The onboarding picker mixes joiners.**
- **Evidence:** `onboarding-picker-light`:
  - "Fast and accurate. Supports 25 European languages." and "Very fast, English only." use sentences.
  - The next four rows use " · " fragments.
  - Models and the AI pane now all use " · " with no final period.
  - At `main-models-min-dark`, "Streaming ·" still ends a line.
- **Holds down:** #32, #10.
- **Fix:**
  - Build the picker descriptions with the same `kit::model_description` joiner.
  - Bind each " · " to the following fragment with a non-breaking space, so a line never ends on a dot.
- **File:** `ui/onboarding/model_picker.rs`, `ui/kit.rs`, `translation.json`.

**8. People emails truncate at 900 pt.**
- **Evidence:** `meetings-people-light`: three of four rows end "ada@example.c…", "bob@example…", "chen@example…".
- **Holds down:** #42.
- **Fix:** widen the list to 260 pt between 900 and 1280 pt, or drop the email to its own caption line.
- **File:** `pages/meetings/people.rs`.

**9. The Set Up… chip is 12 pt in a 13 pt toolbar.**
- **Evidence:** `msettings-setup-min-light` (next to the 13 pt Record).
- **Holds down:** #15.
- **Fix:** 13 pt label, 26 pt height, the same as Record.
- **File:** `pages/meetings/recorder.rs`.

**10. Minor copy.**
- "Turn it on in System Settings > Apple Intelligence & Siri." (`msettings-ai-local-*`): macOS writes menu and settings paths with "›". **File:** `translation.json`.
- Category titles such as "You" and "Recording" are ambiguous out of context. At wide sizes, consider a muted "Meeting settings ›" before the title. **File:** `ui/main_window.rs`.

**11. Track outline and Other share a colour (nit).**
- **Evidence:** `main-storage-*`: the Other segment and the bar's outline are both `#8f8f8a` (light) and `#707070` (dark). The filled run reads partly as a thick border.
- **Fix:** outline one step lighter than Other: about `#a9a9a4` light and `#5a5a5a` dark. The free remainder is still bounded at ≥ 3:1 by Other's own edge.
- **File:** `pages/storage.rs`.

**12. Harness.**
- `meetings-settings-*` still captures the pointer hovering the Meeting settings parent (`#e1e1e0`).
- `advanced-limit-confirm-*` and `postprocessing-discard-*` move between rounds.
- **Fix:** park the pointer outside the window before capture, and capture alerts after their transition settles.

---

## 4. What would raise each parameter still below 9

| # | Parameter | Next step |
|---|---|---|
| 17 | Interactive states | A calmer light hover and `pressed-*` captures (Problem 2). |
| 25 | Borders and dividers | One overflow line in a separator tone, not stacked (Problem 1). |

To get from 9 to 10:

- #9, #37: Problem 3.
- #10: Problems 4, 6 and 7.
- #12, #34: Problem 5.
- #14: Problem 6.
- #15: Problem 9.
- #32: Problems 7 and 10.
- #42: Problems 4 and 8.
- #41: Problem 11.

---

## 5. Functional findings (not visual)

1. "Generate Summary" is the yellow primary while a meeting records (`meetings-recording-wide-light`). Backlog #4.
2. The overlay timer reads 0:00 while text is mid-sentence (`overlay-live-light`). Backlog #7.
3. Older fixture meetings all start at 14:30 (`main-meetings-light`). Backlog #8.
4. History shows "Recording not found" for every fixture entry (`main-history-light`). Backlog #10.
5. Onboarding cloud setup keeps "Finish" enabled with an empty key (`onboarding-setup-cloud-min-dark`). Backlog #5.
6. The startup placeholder omits Post Process and Debug when they are enabled (`startup-app-*` vs `main-general-light`). Those rows appear on load.
7. Meeting settings pages carry the recorder toolbar ("Team standup · 15:00", Record). Removing it would remove an entry point, which is out of scope.
8. Meeting settings › Setup shows the "Set Up…" chip while Setup is open. The fixture's Setup badge reads "1" elsewhere and "3" on Setup itself (`msettings-setup-min-light`).
9. Out-of-scope IA, noted once:
   - voice enrollment has two entry points (Meeting settings › You and People);
   - API keys have two homes (Post Process and AI summaries).
