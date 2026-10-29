# Guidy desktop redesign spec

The single source of truth for the redesign. Every UI change follows it; if
something here turns out impossible in GPUI, change this file and say why.

Goal: Guidy looks, lays out and behaves like a native macOS app. Dense,
calm, no wasted space, primary actions and key information visible without
scrolling, nothing "web-like" (pill buttons, centered narrow columns,
pointer cursors, uppercase letter-spaced labels, decorative glows).

Visual check: `docs/UI_SNAPSHOTS.md` (headless PNGs of every page, both
themes). Every change to a visible surface is checked there.

## 1. Tokens (`ui/design.rs`, `assets/themes/guidy.json`)

| Token | Value | Notes |
|---|---|---|
| `SCALE` | `1.0` | Sizes are macOS points. |
| `HUD_SCALE` | `1.2` | Recording widget, overlay and notch geometry keep today's size. |
| theme `font.size` / `mono_font.size` | 13 / 12 | rem = 13, so stock components match macOS. |
| `TEXT_BODY` | 13 | Body, row titles, controls. |
| `TEXT_SECONDARY` | 11 | Descriptions, metadata, captions. |
| `TEXT_TITLE` | 13 semibold | Toolbar titles, group headers that need weight. |
| `TEXT_HEADLINE` | 17 semibold | Empty states, onboarding headings, About. |
| `TEXT_DISPLAY` | 24–26 | Onboarding hero only. |
| `TEXT_MONO` | 11.5–12 | Numbers, sizes, times, ids only. |
| `RADIUS_CHIP` | 3 | Chips, key caps, list/sidebar selection. |
| `RADIUS_CONTROL` | 4 | Buttons, inputs, selects, segmented controls. Theme `radius` = 4. |
| `RADIUS_PANEL` | 5 | Groups, panels, banners, popovers, dialogs. Theme `radius.lg` = 5. |
| capsule | HUD only | `rounded_full` stays only for dots, switch knobs, avatars, HUD capsules. |

HUD surfaces (recording widget, overlay) use `design::hud()`, the `HUD_*`
type tokens, `HUD_RADIUS_PANEL`, and set the window rem to `HUD_REM`
(16.8) each frame, so stock icons and spacing keep their size too.

Spacing: 4-pt grid. Page padding 20 horizontal / 16 top. Group gap 20.
Group header → panel gap 6. Setting row padding 6×12, min height 32 (one
line) / ~44 (with description).

Control heights: small 20, regular 24 (default), large 28 (onboarding CTA
only).

No `.pill()`, no rim lights or sheens on ordinary surfaces, no blueprint
grids on ordinary pages (onboarding/About hero may keep one illustration),
no uppercase letter-spaced mono "eyebrows": section headers are sentence
case, 11–12 pt semibold, muted. Default arrow cursor on controls (macOS
does not use the hand cursor except on links).

Contrast: `text_faint` must reach ≥ 4.5:1 for text it renders; light
`accent_text` darkened to pass on white.

## 2. Window shell (`ui/main_window.rs`, `windows.rs`)

```
┌●●●──────────┬─ Page title ──────────────────── [page toolbar actions] ┐
│ General     │ [pinned banners]                                        │
│ History     │ page body: owns its own scroll region(s), full width    │
│ Meetings    │                                                         │
│ Models      │                                                         │
│ …           │                                                         │
├─────────────┴─ status bar 22pt: model · downloads · update ───────────┤
```

- Full-height source-list sidebar (traffic lights sit over it), 200 wide,
  items 28 high, `RADIUS_CHIP` selection with visible contrast in both
  themes, accent-tinted icon, keyboard focusable, ↑/↓, ⌘1…⌘9.
- Unified title bar/toolbar over the content: page title (no "Guidy /"
  prefix) left, page actions right. Window drag and double-click zoom kept.
- No shared page scroll. The shell gives each page a definite height
  (`size_full`/`flex_1().min_h_0()` chain). Pages use `kit` layout helpers:
  a scrolling form page, or a fill page that owns its regions (split views,
  lists).
- Forms: left-aligned, `max_w 640`; long paragraphs `max_w 560`. Lists,
  tables and split views use the full width.
- Window: default 900×600, minimum ~720×480, frame saved and restored.
- Menu bar: App (About, Settings… ⌘, , Services, Hide ⌘H, Hide Others ⌥⌘H,
  Show All, Quit ⌘Q), File (Close Window ⌘W), Edit (standard), View (Show
  Sidebar, sections ⌘1…), Window (Minimize ⌘M, Zoom, Bring All to Front),
  Help.
- Nested scroll regions must not chain: every inner scroll area inside
  another scroll area uses `kit::ScrollTrap::scroll_trap` (stop propagation
  when it can scroll). Prefer layouts with no nesting at all.

### How the shell is built (for page work)

- Title bar: one `TitleBar` (gpui-component) spans the window, 38 pt
  (`design::TOOLBAR_H`); its left 200 pt is painted as sidebar so the
  sidebar reads full height, with the traffic lights at (12, 12) over it.
  It keeps drag-to-move and the system double-click action. During
  onboarding it is empty and borderless.
- Pages implement `ui::pages::Page`: `title()` overrides the title-bar
  title (default: the section name), `toolbar()` returns the page's
  actions (right end of the title bar). The page body must not repeat the
  title. Tests render a page with its toolbar via `ui_tests::open_page`.
- Page bodies: `kit::scroll_page(id, kit::page()…)` for forms (the page owns
  its scroll handle; `page()` is the 640 pt column; `.max_w_full()` for
  lists), or `kit::fill_page(id)` for pages that own their regions. Banners
  render above the page body and never scroll.
- Sidebar: Tab-focusable list (key context `GuidySidebar`); ↑/↓ move the
  selection; selection is neutral gray, accent-tinted while focused.
  ⌘1…⌘9 are fixed per section (General, History, Meetings, Models,
  Advanced, Post Process, Usage, Debug, About); hidden sections ignore them.
  "Show Sidebar" (collapsing it) is not implemented yet.
- ⌘W closes an open dialog or sheet first, then the window.
- The window frame and section are saved in `window-state.json` in the app
  data directory (`window_state.rs`), restored when they still fit a display.
- Menus are translated (`menu.*`). AppKit only adopts a menu titled exactly
  "Window" as the window menu (gpui-pre-macos matches the title), so in
  other languages the open-window list is missing from it; its own items
  (Minimize, Zoom, Bring All to Front) work in every language. "Guidy Help"
  opens `GUIDY_HELP_URL` (build-time env) and is disabled without one.

## 3. Behavior rules

- Destructive actions: confirmation alert or undo toast. ⌫ deletes the
  selected item where there is a selection.
- Text fields save on blur/Enter (debounced), never disable themselves
  while the user types, never act destructively per keystroke.
- Errors that belong to a control are shown inline next to it; toasts only
  for background events.
- Loading: no blank windows and no layout jumps. Draw chrome immediately,
  skeleton rows for content, spinners only for short in-place work.
- Empty states: one sentence + the primary action.
- Keyboard: every control reachable by Tab with a visible focus ring;
  Esc cancels; Enter confirms the default button; ⌘F focuses search.
- Copy: Title Case for buttons and menu items, sentence case for labels,
  "…" on commands that open a dialog. All strings through i18n.

## 4. Models, downloads and onboarding

- One download manager (guidy-core `downloads.rs` tracker) reports every
  kind: speech-to-text, LLM, speaker/segmentation. Status bar shows global
  progress for all.
- `logic/requirements.rs`: pure plan of what each feature needs (local
  options, cloud options, sizes) — used by onboarding, Models page badges
  ("Required by Dictation · Meetings") and Meetings setup.
- Models page covers all model kinds with disk usage, required-by badges,
  per-item progress/cancel/retry/delete, and recommended sets.
- Onboarding: Welcome (choose Dictation / Meeting notes) → Permissions →
  Choose your setup (per-feature requirements with local vs cloud choice,
  total download size) → continue while downloads run. Onboarding never
  blocks on a multi-GB download.
  - Built as a macOS assistant pane (`ui/onboarding/assistant.rs`): one
    600 pt card centered in the window, icon + title + one line, a
    scrolling body, a footer bar with Back on the left and the default
    button (Enter) on the right. State (step, choices) lives on
    `AppState.startup`, so Back and closing the window keep it.
  - Permissions: Accessibility only for dictation, skippable ("Continue
    Without": the text is copied for the user to paste). Meeting
    permissions are asked at the first recording.
  - Setup rows come from `logic/onboarding.rs`, which asks
    `logic/requirements.rs` with the settings the choices would write.
    With meeting notes, one model covers both features when it speaks the
    language (Parakeet v3). "Choose Another…" opens a compact picker
    (language filter, search). "Set Up Later" finishes with no model; the
    app then shows a banner pointing to Models.
  - Finish writes the settings, queues the downloads, marks onboarding
    complete and opens the app; the dictation model is selected (and
    loaded) when its download finishes. After onboarding, the selected
    model is loaded in the background unless it unloads immediately.
  - Migration (retired model): Download / Choose Another… / Later (asked
    again next launch).
- Startup: the core's managers are built off the main thread while the
  window draws the shell it will show (the app for a returning user, the
  assistant for a new one); a core that can't start shows an error pane
  (Show Logs, Quit) instead of crashing. A new user's window is shown even
  when Guidy was launched hidden.

## 5. Meetings

Full-height destination with a pinned section toolbar (Meetings | People |
Settings, plus the always-visible recorder: record / timer / pause / stop,
next calendar event). Meetings is a split view (list | detail with pinned
header, player and independent notes/transcript scroll regions). Settings
is a category list with one scroll region per pane. People is list |
detail. See the meetings implementation notes in the PR/agent reports.
