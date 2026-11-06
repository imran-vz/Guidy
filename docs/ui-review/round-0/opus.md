# Guidy UI review, round 0: reviewer `opus`

Evidence: 58 Guidy snapshots from `target/ui-review/round-0/`, covering every main section in both themes at default, `-min`, `-wide` and `-full`, all meetings states, People, Meeting Settings panes, every onboarding and startup step, both dialogs, both error states, the overlay and the widget. I also looked at the three references, the tokens in `crates/guidy-app/src/ui/design.rs` and `assets/themes/guidy.json`, and `docs/design/REDESIGN.md`. Contrast figures were computed from token hex values and from sampled screenshot pixels (WCAG 2.x relative luminance). Type sizes were measured from cap heights at 2x (Geist cap height ≈ 0.71 em).

Measured facts used throughout:

| What | Measured |
|---|---|
| Page title (toolbar) | 13 pt semibold |
| Row title, sidebar item, list title, summary body | 13 pt (all the same size) |
| Description, section header, status bar | 11 pt |
| Meeting title | ~15.5 pt semibold; onboarding headline ~18 pt bold |
| Dark surfaces | ink `#0d0d0d`, surface `#141414` (1.05:1), frame `#1b1b1b` (1.13:1), hairline `#262626` on surface 1.22:1 |
| Switch off track vs surface | 1.42:1 light, 1.32:1 dark |
| Switch on: white knob on `#ffc72c`, track vs white | 1.56:1 and 1.56:1 |
| Input border vs surface | 1.46:1 light, 2.08:1 dark |
| Slider fill `#e6ad00` vs white | 2.03:1 |
| Light sidebar selection vs sidebar | 1.23:1 |
| Dark accent selection (`#ffc72c33` on ink) | renders olive `#3d3213`; muted text on it 4.48:1 |
| Overlay live dot (light) | 1.31:1 |
| Destructive "Delete" label in alert (light) | 3.31:1 at 11 pt |
| Text tokens | muted 6.5–6.7:1, faint 5.0:1, accent text `#8a6000` 5.4–5.6:1: all pass |

Body-text contrast is not the problem. The problems are hierarchy, accent discipline, information architecture, non-text contrast, consistency and macOS fit.

---

## 1. Rubric

Parameters 1–10 are required by the product owner. Parameters 11–45 come from Apple HIG (macOS), WCAG 2.2, Nielsen's heuristics, Gestalt principles, and the way Codex, Claude, T3 Code, Xcode and System Settings handle chrome and density.

| # | Parameter | Definition | Source | What a 9 looks like (checkable) |
|---|---|---|---|---|
| 1 | Typography | Typeface choice, sizes and legibility across the UI. | HIG Typography; WCAG 1.4.4 | One UI family (SF Pro or a tuned equivalent), body 13 pt, reading text 14 pt, nothing under 11 pt; mono used only for code, IDs and paths. |
| 2 | Font weight usage | Weights carry meaning, not decoration. | HIG Typography | At most 3 weights (400/500/600). Bold only on display text. Emphasis never relies on weight alone where size could carry it. |
| 3 | Color contrast | Text and non-text contrast in both themes. | WCAG 1.4.3, 1.4.6, 1.4.11 | Body text ≥ 7:1, secondary ≥ 4.5:1, every control boundary, track and state indicator ≥ 3:1 against what it sits on. |
| 4 | Information hierarchy | The most important information and action are seen first. | Nielsen #8; Gestalt figure–ground | Each screen has one clear primary object and at most one primary (filled) action. Secondary actions grouped or in an overflow menu. |
| 5 | Typographic hierarchy | Distinct size and weight steps for levels of meaning. | HIG text styles | 4–5 steps with ≥ 2 pt between adjacent levels (for example 20/15/13/12/11). The page title is visibly larger than row titles. |
| 6 | Design language | One coherent visual system. | HIG; design-system practice | Every surface uses the same tokens. No dev-tool fragments (mono chips) inside a settings app. Onboarding, pages and HUD read as one product. |
| 7 | Padding and spacing | Consistent spacing on a scale. | 4/8-pt grid; Gestalt proximity | All gaps come from {4, 8, 12, 16, 20, 24, 32}. Row padding is identical across pages. Related items are closer than unrelated ones. |
| 8 | Width and layout measure | Content widths, line length, use of space. | Typographic measure; WCAG 1.4.8 | Prose 50–75 characters (≤ 640 pt at 13–14 pt). Forms 560–680 pt and centered or anchored by rule. Controls sized to content. |
| 9 | Color and theme | Palette, accent use, light/dark parity. | HIG Color | Neutral base and one accent with ≤ 4 jobs. Semantic colors (red, green, orange) used only for status. The two themes match in structure. |
| 10 | Overall consistency | The same thing looks and behaves the same everywhere. | Nielsen #4 | One component per concept (status, selection, shortcut, badge). No state rendered two ways. |
| 11 | Alignment and grid | Elements share edges and columns. | Gestalt continuity; HIG Layout | Labels, controls and icons align to shared x-columns. Trailing controls share a right edge. Reset or extra icons sit in a fixed slot. |
| 12 | Vertical rhythm | Predictable vertical cadence. | Typographic rhythm | Row heights come from a small set (36/52). Header→panel and group→group gaps are constant on every page. |
| 13 | Iconography | Clear, consistent, unambiguous icons. | HIG SF Symbols | One stroke style at 16 pt in a 24 pt slot. One meaning per glyph. Icon-only buttons only for universal verbs. |
| 14 | Control styling and sizing | Native-proportioned controls with a consistent style. | HIG Controls | Regular controls 22–24 pt tall, mini 18–20. Pop-ups size to content (120–240 pt). One button hierarchy: primary, secondary, plain. |
| 15 | Affordance and signifiers | It is obvious what can be clicked or edited. | Norman; Nielsen #6 | Clickables look clickable (bordered or tinted). Plain-text actions are links. No unlabeled icon toolbars of more than 4 items. |
| 16 | Interactive states | Hover, pressed, selected and disabled states are designed. | HIG; WCAG 1.4.11 | Each state differs by ≥ 3:1 or by shape. Disabled is 38–45% opacity and still identifiable. |
| 17 | Selection clarity | The selected item is unmistakable. | HIG source lists | Selection fill visible in both themes (≥ 1.3:1 plus weight or icon change). Focused selection uses accent. Identical across lists. |
| 18 | Focus visibility | Keyboard focus is clearly shown. | WCAG 2.4.7, 2.4.11, 2.4.13 | 2–3 pt ring at ≥ 3:1, offset 1–2 pt, the same on every focusable control. |
| 19 | Target size | Click targets are large enough. | WCAG 2.5.8; HIG (≥ 20×20 pt on macOS) | Every target ≥ 24×24 pt including padding, with spacing ≥ 4 pt between icon buttons. |
| 20 | Navigation and wayfinding | Users know where they are and how to move. | Nielsen #1, #3 | ≤ 2 navigation levels visible at once, the current location titled, back paths explicit. |
| 21 | Information architecture | Settings and features are grouped by user intent. | Card sorting; System Settings | Every setting has one home. Diagnostics are separated from preferences. No duplicate entry points. |
| 22 | Toolbar and window chrome | Unified title bar, toolbar and status areas. | HIG Toolbars | Title left, ≤ 5 toolbar items, labeled where meaning is not universal. Chrome ≤ 15% of window height. |
| 23 | Surface elevation and layering | Surfaces separate by tone, not only by lines. | Material/HIG materials | 3 tonal levels (frame, content, panel) separated by ≥ 1.08:1 each in dark and ≥ 1.04:1 in light. No card-in-card. |
| 24 | Borders and dividers | Lines are used sparingly and consistently. | Gestalt common region | 1 px hairlines only where tone cannot separate. No double borders. Inset row dividers. |
| 25 | Corner radius consistency | A small, consistent radius family. | HIG | 3 radii (control 5–6, panel 8–10, HUD capsule) applied by rule. |
| 26 | Empty states | Empty screens explain and offer the next step. | Nielsen #10 | One headline, one sentence, one primary action, consistent casing, no duplicate messages. |
| 27 | Loading and progress | Waiting is communicated without layout jumps. | Nielsen #1 | Skeletons match the final layout. Determinate progress has % and time left. One progress style. |
| 28 | Error and status messaging | Errors and statuses are clear, actionable and semantically colored. | Nielsen #9; WCAG 1.4.1 | Inline error with cause, fix and action. Status uses icon + text + color (green ok, orange attention, red failure). |
| 29 | Data density | Information per area is right for the task. | Tufte; Xcode inspector | Settings rows 36–52 pt. Lists show title + meta in ≤ 44 pt. Nothing explained twice. |
| 30 | Scannability | Users can skim and find things fast. | F-pattern research | Left-aligned labels, ≤ 1-line descriptions, key value right-aligned, emphasis on nouns. |
| 31 | Microcopy and capitalization | Concise, consistent, correctly cased text. | HIG Writing; REDESIGN.md §3 | Title Case for buttons and menus, sentence case for labels and headers, no duplicated fragments, no jargon in primary labels. |
| 32 | Numeric and tabular formatting | Numbers are readable and aligned. | Typographic convention | Tabular figures (tnum) in the UI font, right-aligned columns, unit with a thin space, no letter-spaced mono. |
| 33 | Minimum-size reflow | The UI works at minimum window size. | WCAG 1.4.10; HIG | At 720×480 nothing important truncates, toolbars collapse into overflow, sidebars can collapse. |
| 34 | Wide-size use of space | Large windows add value, not emptiness. | HIG Layout | At 1440 pt, forms keep their measure and center or add a column. Lists and detail panes grow. No 1000 pt row gaps. |
| 35 | Dark-mode quality | Dark theme is designed, not inverted. | HIG Dark Mode | Elevated surfaces lighter, no pure black, accent desaturated or limited, selection neutral. |
| 36 | Light-mode quality | Light theme has clean, crisp separation. | HIG | Controls visible on white, accent passes 3:1 for non-text, warm neutrals consistent. |
| 37 | Native platform fit | It feels like a Mac app. | HIG macOS | System font or close match, standard switches, pop-ups and alerts, source-list sidebar, sheets for modal tasks. |
| 38 | Content-first focus | Content dominates chrome. | Codex/Claude practice | Content area ≥ 65% of window width at default size. Chrome recedes in tone. |
| 39 | Visual noise and restraint | Nothing competes without reason. | Gestalt; Dieter Rams | ≤ 1 accent element per region, no decorative chips, boxes only where grouping is needed. |
| 40 | Non-text indicators and use of color | State is not conveyed by color alone and indicators are visible. | WCAG 1.4.1, 1.4.11 | Dots, toggles, sliders and focus ≥ 3:1. Speaker or status color is backed by an icon or label. |
| 41 | Truncation and overflow | Important text never truncates before decoration. | HIG; Nielsen #6 | Names win over chips. Chips wrap or collapse. Tooltips on truncation. ≤ 1 truncation per screen at default size. |
| 42 | Dialogs and modality | Alerts and sheets follow platform patterns. | HIG Alerts and Sheets | Centered alert or attached sheet, icon, ≤ 2 lines of body text, destructive button red with its own label, Cancel on Esc. |
| 43 | Error prevention and recovery | Destructive actions are guarded and reversible. | Nielsen #5, #9 | Undo for single deletes, confirmation for bulk deletes, required items cannot be deleted silently. |
| 44 | HUD and overlay design | Floating HUDs are legible, minimal and glanceable. | HIG Panels; Dynamic Island conventions | State, time and stop are readable at a glance. Text upright, ≥ 13 pt, ≥ 4.5:1. Indicators ≥ 3:1 in both themes. |
| 45 | Motion and transitions | Motion explains change and respects Reduce Motion. | HIG Motion; WCAG 2.3.3 | 150–250 ms ease-out, no gratuitous loops, Reduce Motion honored (stills show little: scored with care). |

---

## 2. Scores

`(k)` means scored from product knowledge because one screenshot is too little evidence.

| # | Parameter | Guidy | Guidy note | Codex | Codex note | Claude | Claude note |
|---|---|---|---|---|---|---|---|
| 1 | Typography | 5 | Geist 13/11 is legible but wide. Geist Mono is used for status, sizes and keycaps. | 8 | System sans at ~14 pt, clean prose. | 9 | Calm 14–15 pt reading type, good tables. |
| 2 | Font weight | 5 | Semibold toolbar title weighs the same as the rows. Meetings list is semibold, History list regular. | 8 | Bold for in-prose emphasis, regular for UI. | 8 | Medium for headers, regular body. |
| 3 | Color contrast | 5 | Text passes (≥ 5:1). Switches 1.3–1.6:1, inputs 1.46/2.08:1, slider 2.03:1. | 8 | High text contrast, muted meta legible. | 8 | Soft greys still ≥ 4.5:1. |
| 4 | Information hierarchy | 4 | 9 equal icon buttons on the meeting header. Every setting row has equal weight. | 8 | Conversation dominates, actions recede. | 8 | Single column of content, quiet chrome. |
| 5 | Typographic hierarchy | 4 | Page title = row title = sidebar = 13 pt. Only two steps in settings. | 8 | Clear title/body/meta steps. | 8 | Headers vs body distinct. |
| 6 | Design language | 4 | macOS settings + dev-tool mono chips + web cards + card-in-window onboarding. | 8 | One dark, minimal system. | 9 | Very coherent. |
| 7 | Padding and spacing | 5 | 4-pt grid in tokens. Rows vary 32–57 pt. Meeting settings rows are tighter. | 8 | Generous, even. | 8 | Even table rhythm. |
| 8 | Width and measure | 4 | Models stretches to ~1200 pt at wide. 200 pt fixed pop-ups squeeze descriptions. | 8 | Prose capped ~850 pt. | 9 | ~760 pt centered column. |
| 9 | Color and theme | 4 | One yellow plays ~15 roles. Dark tones collapse. | 8 | Neutral + gold accent, restrained. | 8 | Near-monochrome. |
| 10 | Overall consistency | 3 | Casing, status, selection and shortcut display differ page to page. | 8 (k) | Consistent within the shot. | 9 (k) | Consistent. |
| 11 | Alignment and grid | 5 | Reset icons at three x-positions. Checkbox vs icon text start differs by 3 pt. | 8 | Tight column alignment. | 9 | Table columns aligned. |
| 12 | Vertical rhythm | 5 | Uneven row heights and group gaps. | 8 | Steady. | 9 | Very steady. |
| 13 | Iconography | 5 | One stroke family, but sparkles mean 4 things. Hollow ring for Record. | 8 | Clear, sparse. | 8 | Sparse, clear. |
| 14 | Control styling | 5 | Near-invisible off toggles, yellow on toggles, mono keycaps, button styles mixed. | 8 | Composer and pickers refined. | 8 | Minimal, consistent. |
| 15 | Affordance | 4 | Icon-only toolbars. "Manage" looks like a label. "Speakers ›" is unclear. | 7 | A few icon-only items. | 7 | Icon-only top-right actions. |
| 16 | Interactive states | 5 | Disabled is fine. Hover and pressed not visible in evidence. | 8 | Selected and hover shown well. | 8 | Selected row clear. |
| 17 | Selection clarity | 4 | Light sidebar 1.23:1. History grey in light but olive in dark. | 8 | Neutral pill, visible. | 8 | Neutral pill, visible. |
| 18 | Focus visibility | 5 | Find field shows a heavy double ring. Elsewhere not shown. | 7 (k) | Typical web focus. | 7 (k) | Typical web focus. |
| 19 | Target size | 6 | Icon buttons ~24 pt. Small text links in the status bar. | 8 | Comfortable. | 8 | Comfortable. |
| 20 | Navigation | 4 | Three levels in Meetings (sidebar › segmented › secondary list). | 8 | Rail + list, clear. | 8 | Single sidebar. |
| 21 | Information architecture | 3 | Sound Theme and update toggle live in Debug. Two API-key homes. "Check for updates" in 3 places. | 7 (k) | Good projects/chats split. | 8 (k) | Clear. |
| 22 | Toolbar and chrome | 5 | Unified title bar is good. Status bar is redundant. Meetings toolbar is crowded. | 8 | Compact, labeled tabs. | 8 | Minimal title bar. |
| 23 | Elevation | 4 | Dark surface vs ink 1.05:1. Summary is a card inside a page. | 8 | Sidebar/content/composer tiers. | 8 | Composer raised, content flat. |
| 24 | Borders and dividers | 5 | Hairlines everywhere, double borders in onboarding. | 8 | Row rules only in tables. | 9 | Lines only where needed. |
| 25 | Corner radius | 7 | 3/4/5 pt family applied consistently. | 8 | Consistent. | 8 | Consistent. |
| 26 | Empty states | 4 | "No meetings yet." and "No Meetings Yet" together. Blank pages on error. | 8 (k) | Good. | 8 (k) | Good. |
| 27 | Loading and progress | 6 | Skeleton matches the shell. Progress bars yellow and consistent. | 8 (k) | "Worked for 4m 59s" pattern. | 8 (k) | Streaming is clear. |
| 28 | Error and status | 4 | "Granted" yellow in one place, green in another. Raw red mono startup error. | 7 (k) | Adequate. | 7 (k) | Adequate. |
| 29 | Data density | 5 | Verbose 2–3 line descriptions. 440 pt of chrome in Meeting Settings. | 8 | Dense but airy. | 8 | Good. |
| 30 | Scannability | 4 | Equal weights, long prose, truncated names. | 8 | Bold lead-ins help. | 8 | Tables scan well. |
| 31 | Microcopy and case | 3 | Title Case labels vs sentence case. Duplicated fragments ("English only. · English only"). | 8 | Plain. | 9 | Plain, consistent. |
| 32 | Numeric formatting | 5 | Letter-spaced mono ("5 h", "494.0 GB"). Mono inside prose chips. | 8 | Right-aligned prices. | 7 | Numbers left-aligned in tables. |
| 33 | Min reflow | 4 | "Pa…" model name truncated. Cloud key field hidden at min. | 7 (k) | Panels collapse. | 8 (k) | Sidebar collapses. |
| 34 | Wide use of space | 5 | Meetings wide adds a transcript pane (good). Models and forms leave dead space. | 8 | Adds side panel. | 8 | Centered column. |
| 35 | Dark mode | 4 | Collapsed tones, olive selection, yellow glare. | 8 | Well-tuned greys. | 8 | Well-tuned greys. |
| 36 | Light mode | 5 | Warm neutrals are nice. Toggles and inputs fade into white. | 7 (k) | Fine. | 8 (k) | Fine. |
| 37 | Native fit | 4 | Geist, custom toggles, top-floating alerts, card onboarding. | 7 | Electron, well disguised. | 7 | Electron, well disguised. |
| 38 | Content-first | 5 | At 900 pt, 200 sidebar + 240 list leave 460 pt for content. | 9 | Content dominates. | 9 | Content dominates. |
| 39 | Visual noise | 4 | Boxes, chips, yellow and mono compete. | 8 | Quiet. | 9 | Very quiet. |
| 40 | Non-text indicators | 4 | Overlay dot 1.31:1. Toggles fail 3:1. "You" shown by color only. | 7 | Thin indicators. | 7 | Thin indicators. |
| 41 | Truncation | 3 | Names truncate before chips. Several truncations per screen. | 8 | Ellipsis on long titles only. | 6 | Sidebar titles hard-clipped. |
| 42 | Dialogs | 4 | Alerts float at the top over the title bar. Pale destructive button (3.31:1). | 7 (k) | Standard. | 8 (k) | Standard. |
| 43 | Error prevention and recovery | 6 | Confirmations and an undo toast exist. | 7 (k) | — | 8 (k) | — |
| 44 | HUD and overlay | 6 | Capsule is decent. Italic text, faint dot in light, icon-only pill. | 7 (k) | Voice mode fine. | 7 (k) | Quick entry fine. |
| 45 | Motion | 5 | Sheen and shimmer loops. Reduce Motion is honored in code. Little evidence. | 8 (k) | Subtle. | 8 (k) | Subtle. |

| App | Mean of 45 parameters | Holistic "would the lead designer ship this" |
|---|---|---|
| Guidy (round 0) | **4.6** | **3** |
| Codex | **7.8** | **8** |
| Claude | **8.0** | **8** |

---

## 3. Diagnosis

### 3.1 The 25 most important problems (most severe first)

**1. Information architecture and navigation depth.**
- **What:** The sidebar flattens 10 items that mix daily destinations (History, Meetings), preferences (General, Advanced, Post Process, Storage) and diagnostics (Usage, Debug). Meetings adds a segmented control (Meetings | People | Settings) and then a third-level category list (Setup … Integrations), so 440 pt of navigation chrome sits beside a 460 pt pane at the default 900 pt width. Settings are scattered:
  - Sound Theme, Check for Updates and paste delays sit in Debug.
  - Launch on Startup and Overlay sit in Advanced.
  - API keys are set in both Post Process and Meetings › AI Summaries.
  - "Check for updates" appears in three places: status bar, About and Debug.
- **Evidence:** `main-debug-*`, `main-advanced-full-*`, `meetings-settings-light`, `msettings-*`, `main-postprocessing-*`, `main-about-*`.
- **Why it matters:** Nielsen #4 and #6. System Settings has one home per setting.
- **Fix:**
  - Group the sidebar into labeled sections. *Library*: History, Meetings, People. *Settings*: General, Dictation, Meeting Notes, AI & Models, Storage. *Diagnostics*: Usage, Debug, shown only with Experimental on.
  - Move the meeting-settings categories into the main settings area as one page with sections, which removes the third level.
  - Move Sound Theme and Check for Updates to General, keep Debug for logs and timing, and use one AI-provider page for all API keys.
- **Files:** `ui/main_window.rs`, `ui/pages/mod.rs`, `ui/pages/meetings/settings.rs`, `ui/pages/debug.rs`, `ui/pages/advanced.rs`.

**2. Accent overload and broken color semantics.**
- **What:** Signal yellow `#ffc72c` is the fill for primary buttons, Record, every on-toggle, sliders, progress bars, dark-mode selection, checkboxes, find matches and the focus ring. It is also the text color for links, minute timestamps, the "You" speaker, totals ("$3.68", "18.3 GB"), and badges (Recommended, Required by…, default, summaries, Processing, Granted). With that many roles it marks nothing. "Granted" is yellow in onboarding but green in Meeting Settings, and Record is yellow while Stop is red.
- **Evidence:** `main-usage-dark`, `main-storage-light`, `onboarding-permissions-dark` vs `msettings-setup-dark`, `msettings-ai-*`, `meetings-transcript-*`.
- **Why it matters:** HIG Color ("use accent sparingly"), WCAG 1.4.1.
- **Fix:** Limit the accent to four jobs: the primary button, focused selection, the focus ring and text links. Everything else becomes neutral or semantic: success `#2f9a58`/`#6fd08c`, attention `#c86b00`/`#ff9e45`, danger `#d6403a`/`#ff6b61`, live recording red `#e5372d`. Totals and values use the primary text color.
- **Files:** `design.rs` (`chip`, `Tone`), the pages listed in Evidence.

**3. Monospace overuse gives a dev-tool feel.**
- **What:** Geist Mono with wide tracking is used for sizes, durations, list times, keycaps ("⌥ Right Option"), status badges ("Processing", "Recording", "Granted", "Old format", "1 speech model", "summaries", "reasoning"), usage totals and storage values. The result reads like letter-spaced output ("5 h", "494.0 GB").
- **Evidence:** `main-models-*`, `main-storage-*`, `main-usage-*`, `onboarding-welcome-*`, `meetings-processing-*`, `msettings-ai-*`, `main-general-*`.
- **Why it matters:** HIG Typography. REDESIGN.md says "mono for numbers, ids only", and even that is too much.
- **Fix:**
  - Set numbers in the UI font with tabular figures (`FontFeatures` `tnum`).
  - Make `chip()` use the UI font at 11 pt medium.
  - Keep mono only for model IDs (`claude-sonnet-4-5`), file paths and raw error text.
- **Files:** `design.rs` `mono()`, `chip()`, `TEXT_MONO`.

**4. Flat typographic hierarchy.**
- **What:** The toolbar page title (13 pt semibold), row titles, sidebar items, list titles and summary body all render at 13 pt. Settings pages have exactly two sizes (13 and 11), so nothing says "this is the page and these are its parts".
- **Evidence:** cap-height measurements on `main-general-light`, `meetings-detail-light`.
- **Why it matters:** HIG text styles. Gestalt hierarchy.
- **Fix:** Use the scale in §3.2: page title 20/26 semibold at the top of the content (or 15/20 in the toolbar), group headers 13 semibold primary text (not 11 muted), row titles 13 regular, descriptions 12/16 secondary, metadata 11.
- **Files:** `design.rs` (`TEXT_*`, `section_header`), `kit.rs` (`group`, `SettingRow`).

**5. Dark-mode surfaces collapse and selection turns muddy.**
- **What:** In dark mode the surfaces are nearly the same tone:
  - ink `#0d0d0d` vs surface `#141414` is 1.05:1, and vs frame `#1b1b1b` is 1.13:1;
  - hairline `#262626` is 1.22:1 on the surface, so panels are faint outlines on near-black;
  - focused selection `#ffc72c33` renders olive `#3d3213`, which looks dirty, and muted text on it is 4.48:1.
- **Evidence:** `history-selected-dark`, `meetings-playing-dark` (highlighted line), `msettings-ai-wide-dark` (selected model row), `main-models-wide-dark`.
- **Why it matters:** HIG Dark Mode (elevated surfaces lighter, avoid pure black).
- **Fix:** Use the dark tones in §3.2 (content `#1a1a1a`, sidebar `#222222`, panel `#242424`, raised `#2e2e2e`) with neutral selection `#383838` unfocused and `#4a3d12` plus a 2 pt accent leading bar when focused. Better: a neutral selection with an accent icon, as Codex and Claude do.
- **Files:** `design.rs` `tone()`, `assets/themes/guidy.json`.

**6. Non-text contrast failures (WCAG 1.4.11).**
- **What:**
  - Off switch track vs surface: 1.42:1 light, 1.32:1 dark.
  - On switch: white knob on yellow 1.56:1, and the yellow track vs white 1.56:1.
  - Input and pop-up borders: 1.46:1 light, 2.08:1 dark.
  - Slider fill vs white: 2.03:1.
  - Live dot in the light overlay: 1.31:1.
  - Disabled-looking "Copy All" vs enabled items is hard to tell apart.
- **Evidence:** `main-general-light/dark`, `main-advanced-full-*`, `overlay-live-light`, `main-debug-light`.
- **Fix:**
  - Light switch: off track `#e6e6e2` with a `#8e8e89` 1 px ring (3.29:1); on track `#a87500` (4.03:1 vs white) with a white knob.
  - Dark switch: off track `#3a3a3a` with a `#707070` ring; on track `#ffc72c` with a `#121212` knob (12:1).
  - Input border: `#8e8e89` light, `#707070` dark.
  - Slider fill: `#a87500` light.
  - Overlay live dot: recording red `#e5372d` (4.27:1 on white).
- **Files:** `guidy.json` (`switch.*`, `input.border`, `slider.*`), `kit.rs` `toggle`, overlay view.

**7. Meeting detail toolbar is an icon soup.**
- **What:** The meeting header has 9 icon-only buttons: Redo with a split chevron, Trim (scissors), Copy with a split chevron, Export, AI (sparkles), Delete and Info. Below them sit three player buttons and then a segmented tab bar. Labels appear only at 1440 pt ("Redo", "Copy notes"). The scissors and sparkles meanings are not guessable.
- **Evidence:** `meetings-detail-*`, `meetings-detail-min-light`, `meetings-detail-wide-dark`.
- **Why it matters:** Nielsen #6 (recognition over recall), HIG Toolbars.
- **Fix:** Show two labeled actions, "Copy Notes" (split button) and "Share…", plus a "…" overflow menu with Regenerate, Trim Recording…, Rename Speakers…, Show Info and Delete (⌫). Use a 24 pt slot with 8 pt gaps.
- **Files:** `ui/pages/meetings/detail.rs`.

**8. Two stacked segmented controls used as navigation.**
- **What:** A Meetings | People | Settings segmented control sits in the title bar, and another (Notes | Action Items 3 | Tickets | Transcript) sits in the content. The count reads as part of the label ("Action Items 3"). Segmented controls stacked twice compete with the sidebar's own selection.
- **Evidence:** `meetings-detail-*`, `meetings-transcript-*`.
- **Why it matters:** HIG Segmented controls (for view modes, not for hierarchy).
- **Fix:**
  - Put People in the sidebar under Library and drop the top segmented control once Meetings Settings moves (Problem 1).
  - Make content tabs a plain tab strip: 13 pt medium labels, 2 pt underline in primary text color, 28 pt tall, with counts as 11 pt neutral badges (`raised` fill, 16 pt tall, radius 8).
- **Files:** `ui/pages/meetings/mod.rs`, `detail.rs`.

**9. Capitalization and duplicated microcopy.**
- **What:**
  - Title Case labels on General, Advanced, Debug and About ("Mute While Recording", "Application Theme") vs sentence case in Meeting Settings ("Keep meeting audio", "Meeting notes").
  - Title Case headings in onboarding ("Choose Your Setup") over sentence-case sections ("Meeting notes").
  - "No meetings yet." in the list and "No Meetings Yet" in the pane at the same time.
  - Duplicated description fragments: "Very fast, English only. · English only" and "Supports 25 European languages. · 25 languages".
  - The People intro paragraph repeats inside its card.
- **Evidence:** `main-general-*`, `msettings-general-*`, `meetings-empty-dark`, `onboarding-picker-light`, `meetings-people-light`.
- **Why it matters:** HIG Writing. REDESIGN.md §3 already says sentence case for labels.
- **Fix:** Use sentence case for every label, header and window heading. Keep Title Case for buttons and menu items. Fix `kit::model_description` concatenation so the description and language summary never repeat. Show one empty message only.
- **Files:** i18n strings, `kit.rs` `model_description`.

**10. Primary information truncates before decoration.**
- **What:**
  - `main-models-min-light` shows "Pa…" for Parakeet V3 while the chip "Required by Dictation · Meetings" is shown in full.
  - `msettings-ai-light` shows "Claude…" / "claude…" in the selected row because three chips take the space.
  - "Add your Anthropic API…" and "Saved in your Keychain and shared with Post…" are cut off.
  - `meetings-processing-light` shows "Customer call: Nort…" next to a large "Processing" badge and Cancel.
- **Why it matters:** Nielsen #6, WCAG 1.4.10.
- **Fix:** Names get `flex_shrink_0` priority up to 60% of the row. Chips move to the description line or collapse to "+2". Header status moves to the subtitle line. Truncated text gets a tooltip.
- **Files:** `ui/pages/models.rs`, `meetings/ai_settings.rs`, `meetings/detail.rs`.

**11. Width and measure.**
- **What:**
  - At 1440 pt, Models stretches panels to ~1200 pt, so sizes sit ~900 pt away from their names.
  - General, About and Storage stay left-aligned at 640 pt and leave the right half empty.
  - At 900 pt, Meeting Settings gives content only 460 pt.
  - Fixed 200 pt pop-ups ("Auto", "Default") push descriptions to 3 lines at min ("Hold to speak… stop.").
- **Evidence:** `main-models-wide-dark`, `main-general-full-light`, `meetings-settings-light`, `main-general-min-dark`.
- **Fix:** Settings column max 680 pt centered in the content area (or left at 32 pt with a max). Lists and tables get a 960 pt max with value columns adjacent. Pop-ups hug content (min 120, max 240). Descriptions max 72 characters.
- **Files:** `kit.rs` `page()`, `Dropdown::min_width`, `design.rs` `FORM_MAX_W`.

**12. One concept, many controls.**
- **What:**
  - "Right Option" appears as a pop-up for Dictation Key and as a mono keycap for Transcribe Shortcut; the post-processing hotkey is three separate keycaps.
  - Buttons are mixed: "Manage" is plain text, "Show in Finder" bordered, "Preview" a filled grey, "Generate titles (4)" plain with an icon, "Test Connection" outlined in the toolbar.
  - Reset icons (↺) float at x = 1271/1437/1547 px depending on control width.
- **Evidence:** `main-general-*`, `main-postprocessing-*`, `main-storage-light`, `main-about-light`, `main-debug-dark`.
- **Why it matters:** Nielsen #4, Gestalt similarity.
- **Fix:**
  - One `ShortcutField` (bordered, 24 pt, UI font, key glyphs as 11 pt raised caps inside one field).
  - Three button styles only: primary (filled), secondary (bordered `raised` fill), plain (accent text link, for in-row actions like Manage).
  - Reset as a fixed 24 pt slot immediately left of the control, visible only when the value is not the default.
- **Files:** `kit.rs` (`reset_button`, `SettingRow`), `pages/general.rs`, `pages/post_processing.rs`.

**13. Selection is rendered four ways.**
- **What:**
  - Light sidebar: `#00000017` on `#f0f0ed` = 1.23:1.
  - History list: grey in light but olive accent in dark.
  - The expanded History card at min: yellow border in light, grey border in dark.
  - The selected model in the AI catalog uses a yellow wash. The picker uses grey.
- **Evidence:** `main-history-light`, `history-selected-dark`, `history-selected-min-light` vs `main-history-min-dark`, `msettings-ai-light`, `onboarding-picker-light`.
- **Fix:** One token pair: `selection` (neutral: `#dcdcd7` light, `#383838` dark) with medium-weight text, and `selection_focused` (accent fill `#ffc72c` with `#121212` text in sidebars, as macOS does, or neutral with an accent icon). Use it in every list.
- **Files:** `design.rs`, `kit.rs`, list renderers.

**14. Status and badge semantics are inconsistent.**
- **What:**
  - Success: "Granted" is a yellow mono chip in onboarding, a green check + text in Setup, and "Ready" a green check in Models.
  - Attention uses orange dots without text in the Setup counter.
  - Processing is yellow mono; Recording is a grey mono chip in the header but a red dot in the list.
  - Stop is a red-outline button, Record a yellow fill.
- **Evidence:** `onboarding-permissions-dark`, `msettings-setup-dark`, `main-models-light`, `meetings-recording-dark`, `meetings-processing-light`.
- **Fix:** One `StatusLabel(icon + text + semantic color)`:
  - ok = green check;
  - attention = orange dot + text;
  - working = spinner + text in secondary color;
  - live = red dot + text;
  - error = red triangle.
  Record button: secondary style with a red ● glyph. Stop: secondary with a ■ glyph and "Stop".
- **Files:** `design.rs`/`kit.rs` new helper, `meetings/recorder.rs`, onboarding `permissions.rs`.

**15. Alerts are not macOS alerts.**
- **What:** The confirm alerts sit at the top of the content, overlapping the title bar and page title. They have no icon. The destructive button is pale pink with 11 pt red text (3.31:1).
- **Evidence:** `advanced-limit-confirm-light`, `postprocessing-discard-dark`.
- **Why it matters:** HIG Alerts.
- **Fix:** Center a 400 pt alert in the window (or attach a sheet below the title bar):
  - app icon 48 pt, 13 pt semibold title, 11–12 pt body;
  - buttons 24 pt; destructive is a solid `danger` fill with a white label (4.51:1), not the default button;
  - Cancel on Esc, overlay `#00000040`.
- **Files:** `kit.rs` `confirm`.

**16. Onboarding is a window inside a window.**
- **What:** A bordered 600 pt card sits inside the window, with a grey footer bar and 40–60% dead space (welcome, migration). On Permissions, "Continue Without" is the yellow primary while "Grant Permission" is secondary, so users are nudged away from the permission they need. At min size the cloud key field is below the fold with no scroll cue.
- **Evidence:** `onboarding-welcome-*`, `onboarding-permissions-dark`, `onboarding-migration-light`, `onboarding-setup-cloud-min-dark`.
- **Fix:**
  - Full-window assistant (Setup Assistant style): no card border, 560 pt centered column, 48 pt top margin, hero icon 64 pt, title 26/32 bold.
  - Footer pinned with a hairline only.
  - The primary is the recommended action: "Grant Access…" until granted, then "Continue". "Continue Without" becomes a plain button.
  - Scroll the key field into view on selecting Cloud.
- **Files:** `ui/onboarding/assistant.rs`, `permissions.rs`, `setup.rs`.

**17. Debug page clips content.**
- **What:** The settings panel is cut mid-row ("Paste Delay (Before)" description clipped) above a fixed Live Logs pane.
- **Evidence:** `main-debug-dark`.
- **Fix:** Use a resizable split with a visible splitter, or tabs (Logs | Tuning). Move user-facing settings out (Problem 1).
- **Files:** `pages/debug.rs`.

**18. The status bar is redundant chrome.**
- **What:** A full-width 22 pt bar runs under the sidebar. It shows the model name, a tiny 40 pt download bar and "Check for updates", which duplicates About and Debug.
- **Evidence:** every `main-*` shot.
- **Fix:** Remove the status bar. Show downloads as a toolbar progress item ("Downloading 2 models · 9%", opens Models) and model state in the menu-bar menu and General. Or keep it content-width only, with no link.
- **Files:** `ui/status_bar.rs`, `main_window.rs`.

**19. Empty and error states are thin.**
- **What:**
  - `meetings-empty-dark` shows two messages.
  - `storage-error-light` and `usage-error-dark` leave a blank page under a single banner, and "Clear Usage…" stays enabled and red while the database is locked.
  - `startup-error-dark` shows a raw red mono error with no copy action.
- **Fix:** A centered empty or error block with a 32 pt icon, 15 pt semibold title, one 13 pt sentence and one action. Disable toolbar actions that cannot work. Startup error: neutral text, error in a selectable mono box with "Copy Details", buttons Show Logs / Quit (Quit is not the yellow primary; "Try Again" is).
- **Files:** `pages/storage.rs`, `pages/usage.rs`, `onboarding/startup.rs`, `meetings/list.rs`.

**20. History detail is weak.**
- **What:**
  - The detail title is a timestamp ("March 10, 2026 14:30").
  - A dead player bar ("▷ 0:00 ⓘ Recording not found --:--") takes 74 px on every entry.
  - Four icon-only actions.
  - The "Delete entry" tooltip renders 56 px above its trigger, overlapping the toolbar.
  - The undo toast is a low-contrast strip at the bottom of the list.
- **Evidence:** `main-history-*`, `history-undo-light`, `history-selected-min-light`.
- **Fix:**
  - Detail header: first words or a title at 15 semibold, date as 11 pt metadata.
  - Hide the player when no audio exists.
  - Labeled "Copy" plus a "…" menu.
  - Anchor tooltips to the trigger.
  - The undo toast floats bottom-center over content with `raised` fill and a 4.5:1 "Undo" button.
- **Files:** `pages/history.rs`.

**21. Ambiguous iconography.**
- **What:** Sparkles mean Post Process (sidebar), AI Summaries, Generate titles, a meeting toolbar action and an onboarding tip. Other unclear glyphs: Usage (coin/link?), Advanced (gear-ish) beside General (microphone), and a hollow ring for Record. Icons are 14 pt at thin stroke next to 13 pt text.
- **Fix:** One glyph per concept (Lucide names in parentheses):
  - General: gear (`settings`);
  - Dictation: microphone (`mic`);
  - History: clock (`history`);
  - Meetings: calendar-with-lines (`notebook-text`);
  - People: two-person (`users`);
  - Models: cpu (`box`);
  - AI: sparkles, only here;
  - Storage: hard drive (`hard-drive`);
  - Usage: chart (`chart-column`);
  - Debug: `bug`;
  - About: `info`.
  Icons 16 pt, 1.5 stroke, `text_muted` color, accent only when selected.
- **Files:** `ui/main_window.rs`, `assets/icons`.

**22. Verbose, jargon-heavy descriptions.**
- **What:** Advanced explains option semantics in 2–3 line prose ("Choose how text is inserted. Direct: simulates typing via system input. None: skips paste…"). Primary labels use jargon: VAD tail, GPU/CPU, Q8_0, "Word Correction Threshold 0.18", similarity "0.65". Wrapping orphans appear ("Confirm\n?").
- **Evidence:** `main-advanced-full-dark`, `msettings-speakers-dark`, `main-debug-dark`.
- **Fix:** One-line descriptions ≤ 72 characters. Move option explanations into pop-up menu item descriptions (already supported by `DropdownOption::description`). Thresholds become labeled sliders ("Stricter … Looser") with the value in a tooltip.
- **Files:** i18n, `pages/advanced.rs`, `meetings/settings.rs`.

**23. Uneven rows and vertical rhythm.**
- **What:**
  - Setting rows run 32/44/57 pt.
  - Meeting Settings rows have ~8 pt top padding vs ~12 pt elsewhere.
  - The History list pitch is 45 pt vs 44 pt for Meetings with 34 pt extra at group headers.
  - The About header block is not aligned to the panel padding.
- **Fix:** Rows 36 pt single-line and 52 pt with a description (padding 10×14, title→description gap 2). Group header→panel 8, group→group 24. List rows 44 pt (8×12).
- **Files:** `design.rs` (`ROW_Y`, `SECTION_*`), `kit.rs`.

**24. HUD overlay and widget details.**
- **What:**
  - Live text is italic (slanted Geist, harder to read at a glance).
  - Pale-yellow live dot on white is 1.31:1.
  - The mono 0:00 timer competes with a centered level meter.
  - The collapsed widget is icon-only (bars, ‖, ■) with no time or state label.
  - The expanded panel's "Open notes" is grey 4.5:1-ish plain text.
- **Evidence:** `overlay-live-*`, `recording-widget-*`.
- **Fix:**
  - Upright text with the final words at primary and in-progress words at secondary.
  - Red live dot with a 2 pt halo.
  - Timer at the right in UI tabular figures.
  - Collapsed pill shows "● 12:34" plus ‖ and ■ (≥ 24 pt targets).
  - "Open Notes" as a secondary button.
- **Files:** overlay and widget views using `design::hud*`.

**25. Native platform fit.**
- **What:** Geist instead of the system font, so menus, alerts and the menu bar look different from the window. Custom 36×20 yellow switches, custom segmented tabs, no sidebar vibrancy, mono keycaps, a web-style status bar.
- **Why it matters:** HIG macOS. Codex and Claude get away with custom type because of their content focus; a settings-style Mac utility cannot.
- **Fix:** Use `.SystemUIFont` (SF Pro) for UI with `tnum` for numbers. Keep Geist only for the onboarding display title and the logo if brand matters. Match native control metrics: switch 32×18 regular, 26×15 mini; pop-up 22 pt; push button 22 pt with 12 pt horizontal padding.
- **Files:** `assets/themes/guidy.json` `font.family`, `crate::theme::MONO_FONT`, `kit.rs`.

### 3.2 Target design language (for 9/10)

**Type scale.** UI family: SF Pro (`.SystemUIFont`). Numbers use tabular figures. Mono (SF Mono / Geist Mono) only for IDs, paths and code.

| Style | Size / line height | Weight | Use |
|---|---|---|---|
| Display | 26 / 32 | 700 | Onboarding hero only |
| Title | 20 / 26 | 600 | Page title at top of content (or 15/20 in the toolbar if the page title stays there) |
| Headline | 15 / 20 | 600 | Meeting and History detail titles, empty-state titles, alert titles |
| Group header | 13 / 18 | 600 | Section headers above panels (primary text color, sentence case) |
| Body | 13 / 18 | 400 | Row titles, list titles (500 for list titles), controls |
| Reading | 14 / 22 | 400 | Summaries, transcripts, History text; max 640 pt measure |
| Secondary | 12 / 16 | 400 | Setting descriptions, list metadata |
| Caption | 11 / 14 | 500 | Badges, counts, column heads, timestamps in lists |
| Mono | 12 / 16 | 400 | Model IDs, paths, error details |

**Weights:** 400 / 500 / 600 only; 700 for Display only.

**Spacing scale (pt):** 2, 4, 8, 12, 16, 20, 24, 32, 48.

| Token | Value |
|---|---|
| Page padding | 24 horizontal, 20 top, 32 bottom |
| Group → group | 24 |
| Group header → panel | 8 |
| Setting row | padding 10 × 14; min height 36 (one line) / 52 (with description); title → description 2 |
| Trailing control gap | 8; reset slot 24 pt fixed, left of control |
| List row | padding 8 × 12, 44 pt two-line, 32 pt one-line |
| Toolbar | 38 pt tall, items 24 pt, gap 8 |

**Surfaces and text.** Contrast ratios are computed.

| Token | Light | Dark | Notes |
|---|---|---|---|
| frame (sidebar) | `#f1f1ef` | `#222222` | sidebar vs content 1.04 L / 1.09 D |
| content | `#fafaf9` | `#1a1a1a` | no pure black |
| panel | `#ffffff` | `#242424` | panel vs content 1.04 L / 1.11 D |
| raised (chips, hover, secondary button) | `#ebebe7` | `#2e2e2e` | |
| selection (unfocused) | `#dcdcd7` + 500 weight | `#383838` + 500 weight | 1.36:1 on dark sidebar |
| selection (focused, sidebar) | `#ffc72c` fill, `#121212` text (12.0:1) | same | macOS-style focused source list |
| hairline | `#e4e4e0` | `#333333` | dividers only; 1.24:1 on dark panel |
| control border | `#8e8e89` (3.29:1 on white) | `#707070` (≥ 3:1 on `#242424`) | inputs, pop-ups, off switches |
| text | `#161616` (17.3:1) | `#ededed` (14.9:1) | |
| text secondary | `#5c5c57` (6.72:1) | `#a3a3a3` (6.23:1) | |
| text tertiary | `#6b6b66` (≥ 4.74:1) | `#8c8c8c` (≥ 4.67:1) | |
| accent fill | `#ffc72c` | `#ffc72c` | primary button and focused selection only |
| accent on light non-text (switch on, slider, progress) | `#a87500` (4.03:1) | `#ffc72c` (10.1:1) | white knob light, `#121212` knob dark |
| accent text / link | `#8a6000` (5.59:1) | `#ffc72c` (10.1:1) | links only |
| success | `#23804a` (4.93:1) | `#6fd08c` (8.29:1) | check + text |
| attention | `#c86b00` | `#ff9e45` | dot + text |
| danger | `#c4312b` text (5.50:1), button `#d6403a` with white (4.51:1) | `#ff6b61` (5.63:1) | |
| live / recording | `#e5372d` (4.27:1) | `#ff6b61` | Record dot, overlay dot |

**Accent rules.**
1. The accent fill is allowed on the one primary button per view, the focused sidebar selection, the focus ring (2 pt `#c08a00` light / `#ffc72c80` dark, 1 pt offset) and on-state switches, sliders and progress.
2. Accent text is used only for links.
3. Never on values, totals, speaker names, badges or status.
4. "You" in transcripts is shown with bold or a "(you)" suffix plus neutral color.
5. Find matches use `#ffe58a` light / `#5c4a10` dark, with the current match outlined.

**Controls.**

| Control | Spec |
|---|---|
| Push button | 24 pt (regular), 20 pt (small), padding 12; primary filled, secondary `raised` with border, plain = text link |
| Pop-up | 24 pt, hugs content 120–240 pt, chevron 10 pt |
| Text field | 24 pt, border = control border |
| Switch | 32 × 18 regular; off = `raised` track with control-border ring; on = accent |
| Checkbox / radio | 14 pt |
| Segmented control | 24 pt (view modes only) |
| Tab strip | 28 pt, underline 2 pt |
| Icon button | 24 × 24 slot, 16 pt glyph; ≤ 4 icon-only per toolbar, others labeled or in "…" |

**Radii:** control 5, chip and badge 4, panel 8, alert/sheet 10, HUD capsule full. Keep to these three plus the capsule.

**Layout widths.**

| Area | Width |
|---|---|
| Sidebar | 200 (collapsible below 760 pt window width) |
| List column | 260–320, resizable |
| Settings column | max 680 centered (or left at 32 pt) |
| Prose | max 640 |
| Tables and lists | max 960 |
| Onboarding column | 560 centered, no card |
| Alerts | 400 |

At 720 pt the Meetings split becomes a stack (already done) and toolbar actions collapse into "…". At 1440 pt the content column stays centered and the Meetings transcript pane appears (keep).

**Elevation.** Separate by tone first (frame < content < panel in light by brightness, the reverse order of lightness in dark), hairline second, and never both a border and a different fill on nested containers. No card-in-card: the summary renders directly on the pane with reading styles.

### 3.3 Functional bugs and improvement ideas (not visual; for later)

1. The tooltip on the History delete button renders far from its trigger and overlaps the toolbar (`history-undo-light`).
2. Model description concatenation duplicates information ("English only. · English only", "Supports 25 European languages. · 25 languages") in the picker and Models.
3. Usage error state leaves "Clear Usage…" enabled while the database is locked (`usage-error-dark`); Storage error leaves Refresh and Try Again doing the same thing.
4. Post Process shows "Select a prompt to edit it." with one prompt present; it should auto-select the first prompt.
5. Post Process Model has both a free-text field and a "Choose" pop-up for the same value.
6. Two retention settings overlap (History Limit "5 entries" and Auto-Delete Recordings "Keep latest 5"), which is confusing; the confirm then speaks of "Keeping only 2".
7. API keys are entered in two places (Post Process and Meetings › AI Summaries, "shared with Post Process"); one provider store would avoid drift.
8. A model marked "Required by Dictation · Meetings" still shows an enabled trash button; deleting it should warn or be blocked.
9. History shows a player with "Recording not found" for every entry; hide the player when there is no audio, or offer re-record/import.
10. The "Processing" meeting shows a playable player and a truncated title while transcribing; consider showing the transcript progress inline instead.
11. "Generate summary" is offered as a primary while recording is still in progress (`meetings-recording-wide-light`); disable it until the transcript is final, or label it "Summarize so far".
12. Onboarding Permissions makes "Continue Without" the default (Enter) action even though Accessibility is needed for auto-paste.
13. At minimum size, the onboarding cloud setup hides the API key field below the fold with no scroll cue (`onboarding-setup-cloud-min-dark`).
14. The speaker threshold copy has a stray space ("Confirm ?") and exposes raw similarity numbers.
15. "Check for updates" exists as a status-bar link, an About button and a Debug toggle; keep one action plus one preference.
16. Search placement differs: History search in the toolbar, Meetings and People search at the top of the list.
17. Storage has no ⌘ shortcut because Debug takes ⌘8; demoting Debug (Problem 1) frees a slot.
18. The Debug "Copy All" and "Clear" disabled state is visually close to enabled in light mode (2.2:1 vs muted text).
19. The AI Summaries pane shows the default model in two lists (radio "Your models" and checkbox catalog), both marked "default"; merge them.
20. The overlay timer reads 0:00 while text is mid-sentence (possibly snapshot timing; verify the timer starts with audio, not with the first token).
21. In the Meetings fixture every older meeting starts at 14:30; check the list's time formatting against real start times.
