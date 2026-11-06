# Agreed rubric

Merged from the two Phase 1 proposals (`round-0/opus.md`: 45 parameters,
`round-0/codex.md`: 40 parameters). Parameters 1–10 are the product owner's.
11–45 are the researched additions both reviewers proposed (or one proposed
and the other covered under a neighbouring item). Each is scored 0–10 on the
scale in `COUNCIL_BRIEF.md`. The overall score is the unweighted mean, one
decimal. The holistic "would the lead designer ship this" score is reported
separately.

The target column is the checkable bar for a 9. Where the two reviewers
proposed different numbers, the target states the range both accept.

| # | Parameter | Target for a 9 | Sources |
|---|---|---|---|
| 1 | Typography | One UI family; 13 pt controls, 14 pt reading text, 12 pt helpers, nothing essential under 11 pt; mono only for IDs, paths, code and clocks. | HIG Typography, WCAG 1.4.4 |
| 2 | Font weight usage | 400/500/600 only (700 for a display title); weight marks role, never whole paragraphs. | HIG Typography |
| 3 | Color contrast | Text ≥ 4.5:1 (body ideally ≥ 7:1); every necessary control boundary, track and state mark ≥ 3:1 in both themes. | WCAG 1.4.3, 1.4.11 |
| 4 | Information hierarchy | One clear primary object and one dominant action per region; secondary actions grouped or in overflow. | Nielsen #8, Gestalt figure–ground |
| 5 | Typographic hierarchy | 4–5 distinct roles, ≥ 2 pt between adjacent levels; page title visibly larger than rows. | HIG text styles |
| 6 | Design language | Every surface uses the same tokens; HUD is a deliberate exception; no dev-tool fragments in settings. | HIG, design-system practice |
| 7 | Padding and spacing | All gaps from {2, 4, 8, 12, 16, 20, 24, 32}; identical row padding across pages. | 4/8-pt grid, Gestalt proximity |
| 8 | Width and layout measure | Forms 560–680 pt; prose 55–80 characters; names keep ≥ 180 pt; controls sized to content. | Typographic measure, HIG Layout |
| 9 | Color and theme | Neutral base, one accent with ≤ 4 jobs, semantic colours only for status, light/dark parity. | HIG Color, WCAG 1.4.1 |
| 10 | Overall consistency | One component per concept (status, selection, shortcut, badge, reset, copy, delete). | Nielsen #4 |
| 11 | Alignment and grid | Shared leading edges and a shared trailing control column; extra icons in fixed slots. | Gestalt continuity, HIG Layout |
| 12 | Vertical rhythm | Row heights from a small set (≈36 one-line, ≈52 with description); constant header/group gaps. | Typographic rhythm |
| 13 | Perceptual grouping | Related items 8–12 pt apart, unrelated groups ≥ 24 pt; no unrelated controls in one group. | Gestalt proximity, common region |
| 14 | Iconography | One stroke family, 14–16 pt glyphs, one meaning per glyph; uncommon actions labeled. | HIG SF Symbols |
| 15 | Control styling and sizing | Regular controls 24–26 pt; pop-ups hug content; three button styles (primary, secondary, plain). | HIG Controls |
| 16 | Affordance and signifiers | Clickables look clickable; main commands named; no unlabeled toolbar of > 4 icons. | Norman, Nielsen #6 |
| 17 | Interactive states | Rest, hover, pressed, selected, disabled, busy all designed and distinct. | HIG, WCAG 1.4.11 |
| 18 | Selection clarity | Selection visible in both themes, the same treatment in every list, focused vs unfocused distinct. | HIG source lists |
| 19 | Focus visibility | 2 pt ring ≥ 3:1 with clearance, same on every focusable control. | WCAG 2.4.7, 2.4.11, 2.4.13 |
| 20 | Target size | ≥ 24×24 pt hit areas; ≥ 4 pt between icon buttons. | WCAG 2.5.8 |
| 21 | Navigation and wayfinding | ≤ 2 navigation levels visible at once; location always titled; explicit back paths. | Nielsen #1, #3, HIG Sidebars |
| 22 | Information architecture | Settings grouped by intent; destinations grouped (library vs preferences vs app); one home per setting. | System Settings, card sorting |
| 23 | Toolbar and window chrome | Title left, ≤ 5 toolbar items, labeled where not universal; chrome recedes. | HIG Toolbars |
| 24 | Surface elevation and layering | 3 tonal levels distinguishable in both themes; no card-in-card. | HIG materials |
| 25 | Borders and dividers | 1 px hairlines only where tone can't separate; no double borders. | Gestalt common region |
| 26 | Corner radius consistency | Small family (chip / control / panel / dialog / capsule) applied by rule. | HIG |
| 27 | Empty states | One headline, one sentence, one primary action; no duplicated messages. | Nielsen #10 |
| 28 | Loading and progress | Named work, progress when known, stable layout, one progress style. | Nielsen #1 |
| 29 | Error and status messaging | Plain cause, consequence and action; icon + text + semantic colour; diagnostics secondary. | Nielsen #9, WCAG 1.4.1 |
| 30 | Data density | Right amount per area; nothing explained twice; names outrank badges. | Tufte, Xcode inspector |
| 31 | Scannability | Stable positions for name, state, value, action; one-line descriptions where possible. | F-pattern research |
| 32 | Microcopy and capitalization | Sentence case labels and headers, Title Case buttons and menus, no duplicated fragments or jargon. | HIG Writing |
| 33 | Numeric and tabular formatting | Tabular figures, right-aligned quantities, stated units and price basis. | Typographic convention |
| 34 | Minimum-size reflow (720×480) | Nothing essential truncates; toolbars collapse to overflow; panes give way. | WCAG 1.4.10, HIG |
| 35 | Wide-size use of space (1440×900) | Forms keep their measure; lists and panes grow usefully; no dead stretches. | HIG Layout |
| 36 | Dark-mode quality | Designed, not inverted: lifted surfaces, neutral selection, no glare. | HIG Dark Mode |
| 37 | Light-mode quality | Crisp separation; controls and state marks visible on white. | HIG |
| 38 | Native platform fit | Reads as a Mac app: source list, toolbar, controls, alerts and sheets behave as expected. | HIG macOS |
| 39 | Content-first focus | Content dominates chrome; reading starts near the top of its region. | Codex/Claude practice |
| 40 | Visual noise and restraint | ≤ 1 accent element per region; no decorative chips; boxes only where grouping is needed. | Gestalt, Rams |
| 41 | Non-text indicators and use of colour | State never by colour alone; dots, toggles, sliders ≥ 3:1. | WCAG 1.4.1, 1.4.11 |
| 42 | Truncation and text accommodation | Primary text never truncates before decoration; ≤ 1 truncation per screen; longer text wraps. | WCAG 1.4.12, Nielsen #6 |
| 43 | Dialogs and modality | Platform alert/sheet placement, clear destructive styling, Esc cancels. | HIG Alerts and Sheets |
| 44 | HUD and overlay design | State, time and stop readable at a glance; upright text; indicators ≥ 3:1. | HIG Panels |
| 45 | Motion and transitions | Purposeful, brief, no perpetual decorative loops, Reduce Motion honored. | HIG Motion, WCAG 2.3.3 |

Notes for scorers:

- Interaction-only parameters (17, 19, 20, 45) are judged from what stills
  and source show; say so when evidence is thin.
- `(k)` marks a reference-app score from product knowledge.
- Functional findings are not scored; list them separately.
