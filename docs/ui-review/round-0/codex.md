# Round 0 UI review: codex

Reviewer: `codex` · 5 November 2026 · Independent Phase 1 review

## Evidence and method

I would not sign off on this version. Guidy has a coherent shell, readable body text, and useful recovery states. Its weakest screens spend scarce width on badges, controls, and nested navigation while cutting off the information people need to make a choice. The result feels more difficult than the underlying tasks warrant.

I inspected all 98 snapshot states in both themes, 196 PNGs in total, using image tools and in-memory contact sheets. This includes every main section at default and minimum size, every supplied wide variant, meeting states, People, meeting settings, onboarding, startup, dialogs, and both HUDs. Tall `-full` images were also inspected in crops showing content below the default fold. I reopened originals and inspected larger crops to verify model truncation, toolbar composition, onboarding overflow, dialogs, and HUD controls. Contact-sheet scaling was not used to diagnose individual glyph defects. The three supplied reference PNGs were inspected separately.

Source reads were limited to UI implementation, theme tokens, snapshot documentation, and the brief. No source files were edited, no app was launched, and no Cargo command was run. I did not read the other reviewer's report. Only this report was written.

All snapshot names below are relative to `target/ui-review/round-0/`. `{light,dark}` explicitly means both PNGs. Screenshots are 2×; 26 image pixels of text correspond to a 13 pt UI size. The supplied `-full` images are 1800×4800 pixels, taller than the shorthand description in the snapshot guide suggests. Their unused lower area is not evidence of ordinary-window wasted space. Missing traffic lights and opaque HUD capture backgrounds are known harness limitations, not defects.

Static evidence cannot establish keyboard reachability, actual hit bounds, VoiceOver semantics, hover timing, animation smoothness, latency, or scrolling behavior. Related scores are provisional assessments of the visible design, supplemented by identified source reads. They are not accessibility certification. Reference scores marked `(k)` rely on product knowledge because the supplied screenshot does not show that parameter. These are estimates, not fresh interaction tests or claims about every release of either app. Rendered artifacts in the Codex browser pane are user content and are excluded from scoring the app's chrome.

## 1. Build the rubric

### Research and interpretation

The sources below were checked for this review. Apple pages expose official Markdown versions; those were read when the web reader could not extract the JavaScript pages.

| Key | Source | Application in this review |
| --- | --- | --- |
| A1 | [Apple HIG: Typography](https://developer.apple.com/design/human-interface-guidelines/typography) | Mac defaults, readable weights, role-based typography, custom font legibility. |
| A2 | [Apple HIG: Layout](https://developer.apple.com/design/human-interface-guidelines/layout) | Alignment, grouping, adaptation, progressive disclosure. |
| A3 | [Apple HIG: Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars) | Navigation hierarchy, width costs, collapse behavior, familiar symbols. |
| A4 | [Apple HIG: Toolbars](https://developer.apple.com/design/human-interface-guidelines/toolbars) | Deliberate action selection, titles, overflow, content visibility. |
| A5 | [Apple HIG: Dark Mode](https://developer.apple.com/design/human-interface-guidelines/dark-mode) | Semantic colors, legibility in both appearances, accessibility appearances. |
| A6 | [Apple HIG: Motion](https://developer.apple.com/design/human-interface-guidelines/motion) | Brief, purposeful, optional motion. |
| W1 | [WCAG 2.2: Contrast minimum, 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) | Text contrast and inactive-control exceptions. |
| W2 | [WCAG 2.2: Non-text contrast, 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html) | Necessary control and state indicators, not every decorative border. |
| W3 | [WCAG 2.2: Target size minimum, 2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html) | Pointer target size and spacing exceptions. |
| W4 | [WCAG 2.2: Focus appearance, 2.4.13](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance.html), [Focus visible, 2.4.7](https://www.w3.org/WAI/WCAG22/Understanding/focus-visible.html), [Focus not obscured, 2.4.11](https://www.w3.org/WAI/WCAG22/Understanding/focus-not-obscured-minimum.html) | Distinguishable focus; distinguish AAA appearance goals from AA visibility requirements. |
| W5 | [WCAG 2.2: Text spacing, 1.4.12](https://www.w3.org/WAI/WCAG22/Understanding/text-spacing.html), [Reflow, 1.4.10](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html) | Text accommodation and avoiding loss of content when space changes. |
| W6 | [WCAG 2.2: Use of color, 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html) | State and meaning must survive loss of hue. |
| W7 | [WCAG 2.2: Animation from interactions, 2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html) | Optional nonessential interaction animation; AAA criterion. |
| N | [Nielsen's ten usability heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/) | Status, recognition, consistency, error prevention, recovery, minimalism. |
| G | [NN/g: Gestalt proximity](https://www.nngroup.com/articles/gestalt-proximity/) | Relationships expressed through grouping and relative distance. |
| R | Supplied `codex.png`, `claude.png`, `t3-code.png`; Xcode and System Settings product knowledge | Reference examples, not normative standards. |

Apple lists 13 pt as the macOS default and 10 pt as its minimum; 11 pt is not automatically a defect. My 12 pt helper-text target below is a Guidy design decision for description-heavy pages. Similarly, WCAG defines CSS pixels, not AppKit points. I use its criteria as accessibility benchmarks and set 24 logical pt as Guidy's own minimum target goal. A 20 pt drawing can still have a larger hit area. A screenshot does not prove a target-size violation. WCAG's web reflow requirement is not a mandate to make this native window 320 pt wide.

The numerical values in the last column are proposed acceptance criteria for a 9/10 Guidy implementation, not measurements or requirements attributed wholesale to Apple. Focus's two-pixel perimeter benchmark is AAA. Decorative separators need not reach 3:1 when they carry no necessary control or state information.

### Forty review parameters

| # | Parameter | Definition | Source | What a 9 looks like |
| --- | --- | --- | --- | --- |
| 1 | Typography | Text remains comfortable to read in each content role. | A1 | One UI family; 13 pt controls, 14 pt long-form notes, 12 pt helpers; no essential text below 11 pt. |
| 2 | Font weight usage | Weight communicates role without making the whole screen shout. | A1 | Regular body, medium row titles, semibold headings; no thin small text or bold paragraphs. |
| 3 | Color contrast | Text and necessary visual indicators remain distinguishable in both themes. | W1, W2 | Normal text ≥4.5:1; necessary icons and state marks ≥3:1 on their actual surfaces. |
| 4 | Information hierarchy | The screen presents purpose and the next useful action before secondary configuration. | A2, N | Task or current object clear first; one dominant action per region; secondary actions disclosed. |
| 5 | Typographic hierarchy | Type roles make titles, labels, body, and metadata visibly different. | A1 | Page title 17/22 semibold; section 12/16 semibold; body 13/18; metadata 12/16 regular. |
| 6 | Design language | Shell, content, and HUDs share an intentional visual vocabulary. | A2, R | Shared semantic tokens; deliberate HUD exception; no isolated page-specific button or card dialect. |
| 7 | Padding and spacing | Internal and external whitespace makes controls comfortable and relationships clear. | A2, G | 4 pt base; 12 pt panel inset, 20 pt page inset, 24 pt group gap; no accidental gaps. |
| 8 | Width and measure | Each content type gets a useful line length and column width. | A2, R | Forms ≤640 pt; reading text roughly 55–80 characters; model names retain ≥180 pt before wrapping. |
| 9 | Color and theme | Accent and neutral colors have clear roles with equivalent meaning across themes. | A5, W6 | Neutral navigation, amber primary action, separate error/success roles; no yellow-on-white state dependency. |
| 10 | Overall consistency | The same role or action is rendered and named predictably across screens. | N, R | One treatment for reset, copy, delete, status, and settings rows; shared tokens used throughout. |
| 11 | Alignment and grid | Related edges and baselines provide stable scanning paths. | A2 | Labels share a leading edge; controls share a trailing column; icons and labels align optically. |
| 12 | Vertical rhythm | Repeated rows and text blocks follow a predictable height and gap cadence. | A2, G | 36 pt one-line rows; descriptive rows grow naturally; 4/8/12/16/24 pt spacing roles. |
| 13 | Perceptual grouping | Proximity, common regions, and similarity reveal functional relationships. | G | Related controls 8–12 pt apart; unrelated groups ≥24 pt apart; no unrelated controls sharing one group. |
| 14 | Iconography | Symbols are recognizable, consistent in stroke and meaning, and readable at their size. | A3, R | One icon family; 14–16 pt main glyphs; uncommon actions have labels or reliable explanations. |
| 15 | Control styling | Buttons, fields, switches, and segmented controls have consistent geometry. | A4, R | 26 pt standard controls, 30 pt setup actions; consistent 4 pt control radius and text baseline. |
| 16 | Affordance | Visible cues let people recognize what can be done without guessing. | N | Main commands named; selected/default states differentiated; editable content has an evident entry point. |
| 17 | Interactive states | Rest, hover, pressed, selected, unavailable, and busy states remain distinct. | N, W2 | Every shared control has a documented state set; unavailable labels remain readable; busy state retains its name. |
| 18 | Focus visibility | Keyboard users can identify the active element and follow focus without obstruction. | W4 | Visible 2 pt outline, ≥3:1 same-pixel change, no clipping by scroll containers; test Tab through each screen. |
| 19 | Target size | Pointer actions can be selected without precision aiming or adjacent misclicks. | W3 | ≥24×24 pt hit areas; 28 pt for primary toolbar actions; validate actual bounds, including icon-only buttons. |
| 20 | Navigation and wayfinding | Position and scope remain clear as people move among tasks and settings. | A3, N | Library and app preferences grouped; nested settings explicitly scoped; compact detail has a clear return path. |
| 21 | Toolbar and chrome | Window controls support the task without crowding the content. | A4 | 44 pt unified toolbar; at most four visible secondary commands in a narrow meeting header; sensible overflow. |
| 22 | Surface layering | Background, content, and transient surfaces have a clear depth relationship. | A5, R | Three neutral surface levels; dialogs clearly separate; shadows limited to transient surfaces and HUDs. |
| 23 | Borders and dividers | Lines separate content with the minimum necessary visual weight. | W2, A2 | 1 pt decorative separators; stronger necessary state edges; no grids around every paragraph. |
| 24 | Corner radii | Shape consistently identifies controls, groups, and floating surfaces. | R | 3 pt chips, 4 pt controls, 6 pt panels; capsules reserved for switches, avatars, and compact HUDs. |
| 25 | Empty states | Missing content is explained with one useful next step. | N | One short heading, one sentence, one contextual action; no duplicated empty announcements. |
| 26 | Loading and progress | People can tell what is happening and whether they can proceed. | N | Named work plus progress when known; stable layout; startup includes a readable status rather than skeletons alone. |
| 27 | Errors and recovery | Failures explain the effect and point to a useful existing recovery action. | N | Plain-language cause and consequence; diagnostics secondary; errors remain near affected content. |
| 28 | Data density | The interface exposes enough useful information without compressing essential content. | A2, R | Typical lists show 8–12 useful rows when space permits; detail height follows content; names outrank badges. |
| 29 | Scannability | Repeated content can be compared along stable visual paths. | A2, G | Name, state, size, and action occupy predictable positions; wrapping preserves identity. |
| 30 | Microcopy and capitalization | Labels are concise, understandable, and grammatically consistent. | N | Sentence case; explicit action verbs; no repeated model capability text; familiar Mac terminology. |
| 31 | Numeric formatting | Times, prices, sizes, and counts remain comparable and interpretable. | R, N | Tabular digits; right-aligned quantities; units and price basis stated; precision serves the task. |
| 32 | Responsive layout | Minimum and wide windows preserve important information and useful task structure. | A2, W5 | At 720×480 no essential names collapse to fragments; at 1440×900 layouts use space without excessive measure. |
| 33 | Dark-mode quality | Dark surfaces, type, and interaction states are deliberately tuned. | A5 | Text passes on raised surfaces; needed boundaries survive; large dark areas avoid needless maximum contrast. |
| 34 | Light-mode quality | Light surfaces retain clear state and control distinctions without looking washed out. | A5, W2 | Accent marks have dark edges or glyphs; fields and focus remain identifiable on white and warm gray. |
| 35 | Native platform fit | The app follows recognizable macOS conventions for windows, navigation, and interaction. | A1–A4, R | Familiar source list and toolbar, clear shortcuts, expected menu/keyboard behavior; no mobile-sized card inflation. |
| 36 | Content-first focus | The material people came to read or change receives the largest share of attention. | A4, N | Notes start within roughly 110 pt of the content-region top; playback and configuration do not dominate reading. |
| 37 | Visual noise | Repetition and decoration do not compete with content. | N, R | At most one prominent status badge per compact row; no repeated tinted tags for obvious attributes. |
| 38 | Non-text meaning | Color and graphical marks carry redundant, understandable semantics. | W2, W6 | Recording has text plus a stable symbol; storage segments have a legend; success/error include words or icons. |
| 39 | Motion restraint | Motion explains change and respects comfort and power constraints. | A6, W7 | No perpetual decorative pulse, shimmer, or rotating loader; brief transitions; Reduce Motion honored; meters follow real levels. |
| 40 | Text accommodation | Longer text, changed spacing, and larger reading sizes do not hide required information. | A1, W5 | Expanded labels wrap; a 200% reading-size exercise remains usable; text-spacing stress causes no clipping or lost controls. |

The W5 text-spacing stress values are 1.5× line height, 2× font size paragraph spacing, 0.12× letter spacing, and 0.16× word spacing. These are accommodation tests, not a prescription to set the default UI this loosely. A larger-text control would be an implementation decision, not something proved absent by these screenshots.

### Reference lessons

Claude's supplied screen gives the document a broad, stable reading column and uses very little chrome inside it. Codex keeps working context, conversation, and artifact separated, though its extra rail, tab strip, and composer produce more competing surfaces. T3 Code shows how a multi-column settings screen can still provide enough width for both the category and the form. It also contains faint metadata; it is not a contrast gold standard.

From Xcode I would borrow the discipline of inspector columns and adaptable toolbars. From System Settings I would borrow grouped rows and familiar navigation, while avoiding a second permanent category list when the remaining pane is already too narrow. These last two comparisons are product knowledge, not supplied screenshot evidence. Guidy does not need to reproduce Liquid Glass to become clear or native in use.

## 2. Score three apps on the rubric

Scores use the brief's 0–10 scale. Each cell contains its score and one-line reason. `(k)` marks a reference estimate beyond the supplied screenshot. Guidy interaction and text-accommodation scores remain provisional as described above.

| # | Parameter | Guidy | Codex | Claude |
| --- | --- | --- | --- | --- |
| 1 | Typography | 7: Geist is readable at 13 pt; extensive 11 pt explanation text feels small. | 8: Conversation and sidebar text are comfortable and distinct. | 8: Document text is comfortable across long paragraphs and tables. |
| 2 | Weight usage | 7: Mostly restrained; small group headings depend heavily on semibold. | 8: Weight separates table headers, prose emphasis, and navigation. | 8: Restrained emphasis keeps long content calm. |
| 3 | Contrast | 5: Strong body text; weak light control marks and marginal focus token. | 7: Main text is clear, but composer placeholder and minor metadata are faint. | 7: Main text is clear; footer/model metadata is visibly subdued. |
| 4 | Information hierarchy | 5: Model tags and settings machinery compete with the current task. | 8: Work context and conversation stay understandable despite many regions. | 9: The document is clearly the main object. |
| 5 | Typographic hierarchy | 5: Page title and row body both use 13 pt; helpers and group titles both use 11 pt. | 8: Navigation, headers, body, and secondary text separate well. | 8: Body, table labels, and app chrome are easy to distinguish. |
| 6 | Design language | 7: Tokens and grouped panels are coherent; HUDs are a deliberate variant. | 8: Rails, tabs, composer, and content use a consistent family. | 8: Sidebar, document, and composer share a quiet neutral treatment. |
| 7 | Padding and spacing | 7: Main forms have consistent insets; narrow nested panes squeeze text. | 8: Conversation gutters and composer padding are balanced. | 9: Document gutters and paragraph spacing give content room. |
| 8 | Width and measure | 4: Names truncate at minimum size; wide model rows stretch too far. | 8: Separate conversation and artifact columns keep useful measures. | 9: Centered reading measure is exceptionally clear in the supplied screen. |
| 9 | Color and theme | 7: Semantic roles exist, but amber is reused across many unrelated states. | 8 (k): Dark screenshot is coherent; light parity uses product knowledge. | 8 (k): Dark palette is calm; light parity uses product knowledge. |
| 10 | Overall consistency | 7: Shared row/card system works; labels and several configuration patterns vary. | 8 (k): Visible shell is consistent; broader surfaces are knowledge-based. | 8 (k): Visible shell is consistent; broader surfaces are knowledge-based. |
| 11 | Alignment | 8: Leading edges and control columns are mostly disciplined. | 8: Sidebar and conversation align predictably. | 9: Tables and document margins provide very stable reading paths. |
| 12 | Vertical rhythm | 7: Regular form rows are sound; some inline detail heights are arbitrary. | 8: Prose and table sections have a useful cadence. | 8: Long document sections maintain a steady cadence. |
| 13 | Grouping | 7: Panels communicate settings groups; repeated navigation complicates scope. | 8: Surface boundaries separate three distinct work regions. | 8: Paragraphs and tables are grouped with little enclosing decoration. |
| 14 | Iconography | 7: Lucide strokes are coherent; several meeting actions are obscure when unlabeled. | 8: Most visible navigation symbols are familiar. | 8: Sparse symbols support familiar navigation and actions. |
| 15 | Control styling | 6: Small fields and buttons are consistent but often look compressed. | 8 (k): Composer is polished; settings controls are not shown. | 8 (k): Composer is polished; settings controls are not shown. |
| 16 | Affordance | 5: Meeting action strip requires guessing; status and edit cues can be faint. | 7: Main entry points are clear; several shell icons need learned meaning. | 8: New, navigation, sharing, and reply entry points are clear. |
| 17 | States | 6: Selection, disabled, processing, and playing are shown; light marks are weak. | 8 (k): Selected item is visible; hover/pressed/disabled states are knowledge-based. | 8 (k): Selected chat is visible; other states are knowledge-based. |
| 18 | Focus | 5: Find field has visible focus, but token contrast and one-point custom outlines need work. | 7 (k): No useful focus-state sample; estimate requires a keyboard audit. | 7 (k): No useful focus-state sample; estimate requires a keyboard audit. |
| 19 | Target size | 5: Dense tiny action glyphs warrant a bounds audit; large hit areas are unproven. | 7 (k): Visible controls appear usable; actual bounds were not tested. | 7 (k): Visible controls appear usable; actual bounds were not tested. |
| 20 | Navigation | 5: Ten peers mix work and preferences; meeting settings add another category list. | 8: Project and thread hierarchy stays legible. | 8: Task entries and recent conversations have clear groups. |
| 21 | Chrome | 6: Unified shell works; meeting header has too many stacked control bands. | 7: Many rails and tabs compete for attention. | 8: Slim titlebar and simple sidebar keep chrome secondary. |
| 22 | Layering | 7: Panels and dialogs separate adequately; dark surfaces can be overly close. | 8: Sidebar, conversation, and composer have clear surface relationships. | 8: Sidebar and composer separate from the document cleanly. |
| 23 | Borders | 6: Hairlines are restrained, but necessary state boundaries need stronger treatment. | 8: Dividers separate content without drawing a full grid. | 8: Tables use quiet horizontal rules and little surrounding framing. |
| 24 | Radii | 8: 3/4/5 pt token system is disciplined; capsules stay mostly in HUDs. | 8: Shape is consistent across selection, composer, and shell. | 8: Shape is consistent across selection, composer, and shell. |
| 25 | Empty states | 6: Useful Record action exists; two empty messages describe the same absence. | 8 (k): No empty-state screenshot; estimated from product knowledge. | 8 (k): No empty-state screenshot; estimated from product knowledge. |
| 26 | Loading | 6: Download percent and processing stage are good; startup lacks named status. | 8 (k): Work-duration disclosure is visible; live progress relies on knowledge. | 8 (k): No loading example; estimated from product knowledge. |
| 27 | Errors | 5: Inline retry is useful; startup exposes technical diagnostics as the main explanation. | 7 (k): No error example; estimated from product knowledge. | 7 (k): No error example; estimated from product knowledge. |
| 28 | Density | 6: Some lists are efficient, but the compact AI catalogue is starved of height. | 8: Dense work context remains readable. | 8: Long sidebar is dense while the document remains relaxed. |
| 29 | Scannability | 5: Fragmented model names and multiple tag columns break comparison. | 8: Conversation tables and project rows scan well. | 9: Clean tables and stable body alignment support fast scanning. |
| 30 | Microcopy | 5: Title case, jargon, and repeated capabilities add unnecessary reading. | 8 (k): Visible app labels are concise; broader copy is knowledge-based. | 8 (k): Visible app labels are concise; broader copy is knowledge-based. |
| 31 | Numbers | 6: Tabular mono helps; AI prices lack an explicit visible basis. | 8: Currency columns and capacity values are easy to compare. | 8: Numeric tables show units and align consistently. |
| 32 | Responsiveness | 4: Adaptation exists, but minimum size sacrifices names and form measure. | 8 (k): Multi-pane screenshot is good; narrow behavior is knowledge-based. | 8 (k): Wide screenshot is good; narrow behavior is knowledge-based. |
| 33 | Dark mode | 7: Readable primary text; near-black layers and faint small affordances reduce ease. | 8: Dark surfaces provide useful separation. | 8: Softer dark document surface supports long reading. |
| 34 | Light mode | 6: Warm neutrals work; control-state contrast is materially weaker. | 8 (k): Supplied evidence is dark only. | 8 (k): Supplied evidence is dark only. |
| 35 | Native fit | 6: Source list and groups fit Mac conventions; custom control density feels uneven. | 7 (k): Desktop shell is familiar, with a fairly elaborate custom chrome system. | 7 (k): Desktop conventions are familiar; exact native behavior is untested. |
| 36 | Content focus | 5: Meeting controls and configuration consume too much space before useful content. | 8: Conversation and artifact remain central despite the chrome. | 9: The reading task dominates the window. |
| 37 | Noise | 6: Usually calm; model catalogue tags and repeated explanatory text are distracting. | 7: Rails, tabs, browser controls, and composer create competing clusters. | 8: Very little decoration competes with the document. |
| 38 | Non-text meaning | 6: Recording and errors often have labels; storage bar and compact HUD are less explicit. | 8 (k): Visible selection has redundant treatment; unseen statuses use knowledge. | 8 (k): Visible selection has redundant treatment; unseen statuses use knowledge. |
| 39 | Motion | 5: Source confirms decorative repeat animations; timing and performance were not observed. | 7 (k): Stills cannot establish motion quality; estimate only. | 7 (k): Stills cannot establish motion quality; estimate only. |
| 40 | Text accommodation | 5: Baseline truncation is already substantial; larger-text/spacing behavior was not tested. | 7 (k): Screenshot is readable; text stress behavior untested. | 7 (k): Screenshot is readable; text stress behavior untested. |

### Overall and ship scores

| App | Sum / 40 parameters | Overall mean, one decimal | Holistic ship score | Decision |
| --- | --- | --- | --- | --- |
| Guidy round 0 | 238 / 40 | **6.0 / 10** | **5.0 / 10** | Do not sign off; fix the main choice and configuration screens first. |
| Codex | 310 / 40 | **7.8 / 10** | **7.5 / 10** | Professional desktop product; visible chrome complexity keeps it below 9. |
| Claude | 319 / 40 | **8.0 / 10** | **8.0 / 10** | Best reading composition of the three supplied examples; unshown states remain uncertain. |

The means are equally weighted arithmetic scores, rounded half up. Guidy's unrounded mean is 5.95. The holistic score weighs the primary tasks more heavily than radii or alignment. The owner's 0/10 sentiment is context, not an instruction to score readable, functioning screenshots as broken. None of the evidence justifies calling a reference flawless.

## 3. Diagnose Guidy

### The 25 most important UI problems

Severity reflects likely task impact, not a claim that each issue is a release blocker. Items 1–5 are high priority, 6–16 are medium priority, and 17–25 are refinements or evidence-backed concerns to settle before a 9/10 sign-off. Paths in this section start at `crates/guidy-app/src/ui/` unless otherwise stated. Proposed changes are recommendations only.

#### 1. Model identity loses to badges and metadata

**Evidence:** `main-models-min-{light,dark}.png`, `msettings-ai-{light,dark}.png`. The active speech model becomes `Pa…`; the selected cloud catalogue row becomes `Claude…`, including its ID. Both were confirmed in larger crops of the originals. In the cloud catalogue, the extra default badge makes the selected row harder to identify than its neighbors.

**Why it matters:** Recognition and comparison fail at the point of choosing a model. A2, N, rubric 8/29/32. This is lost primary information, not harmless metadata truncation.

**Fix:** Reserve at least 180 pt for the name. Put badges below it or remove repeated capability badges at compact widths. Show the full display name on up to two lines, with size and controls on a separate line when necessary. Cloud price/context metadata should not have an unconditional 96 pt claim on a 420 pt pane. Use 56–72 pt or move it below the name. Keep the exact ID available in details without requiring it to compete in every row.

**Likely source:** `model_card.rs::ModelRow`, `pages/meetings/ai_settings.rs::model_list`, `pages/models.rs`.

#### 2. Meeting settings retain two navigation columns when the form needs the width

**Evidence:** `meetings-settings-{light,dark}.png`, `msettings-general-{light,dark}.png`, `msettings-recording-{light,dark}.png`, `msettings-speakers-{light,dark}.png`. At 900 pt, the 200 pt app sidebar and 240 pt category list leave roughly 460 pt before pane padding. A 200 pt control then leaves narrow strips for multi-line explanations. “Keep meeting audio” and threshold descriptions wrap into four or more short lines.

**Why it matters:** The apparent density is caused by column allocation, not useful information. A2/A3 and Gestalt proximity favor preserving the relationship between explanation and control.

**Fix:** Collapse meeting categories based on remaining pane width, not only the current shared 640 pt content breakpoint. Require a form pane of at least 520 pt to retain the category list. Below that, use the existing category popup. Stack the control under the label when the remaining label width would be below 220 pt. Keep global preferences forms capped at 640 pt.

**Likely source:** `pages/meetings/mod.rs` split constants, `pages/meetings/settings.rs` compact decision and `CONTROL_W`, `kit.rs::SettingRow`.

#### 3. Light appearance uses low-contrast yellow for necessary state marks

**Evidence:** `msettings-ai-light.png`, `onboarding-welcome-light.png`, `main-advanced-light.png`; compare their dark counterparts. Yellow rings, checked fills, and switch fills are much less distinct on white. `#ffc72c` against white is **1.56:1**. A white interior against that yellow has the same ratio. Dark appearance does not have this problem to the same extent.

**Why it matters:** W2 requires adequate contrast for the information necessary to identify a state. Label legibility does not rescue an indistinct radio selection. Not every yellow button is a failure: its dark text can already identify the command.

**Fix:** Use `#8a6000` for light selection strokes, checks, radio outlines, and necessary track edges. Yellow may remain a fill, with a dark check or knob outline. On dark surfaces use `#ffc72c`. Validate both on/off states against their actual adjacent colors, including the switch thumb. Keep optional decorative separators quiet.

**Likely source:** `assets/themes/guidy.json`, `design.rs`, custom checkbox/radio builders in `pages/meetings/ai_settings.rs`, shared switch styling.

#### 4. The meeting header is an unlabeled tool strip above another tool strip

**Evidence:** `meetings-detail-{light,dark}.png`, `meetings-detail-min-{light,dark}.png`, `meetings-processing-{light,dark}.png`. The title shares space with redo/menu, scissors, copy/menu, export, sparkles, delete, and info, followed by playback controls, then content tabs. Processing crowds the title further. At wide size several labels reappear, showing that meaning depends on available width.

**Why it matters:** A4 and N favor recognizable commands and deliberate prioritization. People should not need a tooltip tour before reading a note. Small glyphs also warrant a real hit-bound audit under W3; their drawing size alone is not proof of failure.

**Fix:** At compact detail widths show a labeled Copy command, one labeled primary content action, and a More menu. Put trim, redo, export variants, delete, and diagnostics in the menu. Keep all commands available. Use 28 pt hit areas, 14–16 pt glyphs, and 8 pt between action groups. Aim for two header bands including tabs rather than three permanent bands plus section chrome.

**Likely source:** `pages/meetings/detail.rs` header/actions, `pages/meetings/player.rs`, `pages/meetings/mod.rs` compact layout.

#### 5. Compact cloud onboarding hides the configuration while keeping Finish prominent

**Evidence:** `onboarding-setup-cloud-min-{light,dark}.png`, compared with `onboarding-setup-cloud-{light,dark}.png`. At minimum height, the provider/key row begins below the initial body viewport. The footer still presents a yellow Finish button. The optional speaker-turn setting is also below the fold on combined setup screens.

**Why it matters:** The most consequential choice on this screen is easy to miss. A2 and N support making relevant requirements visible at the decision point. This is an initial-viewport finding: `Assistant` does have a scrolling body, so I am not claiming the fields are inaccessible or painted under an overlapping footer.

**Fix:** In the compact cloud layout, place the provider and key directly beneath the selected Cloud option; collapse the already-chosen speech model to a 36 pt summary; move optional speaker enhancements into a disclosure. Keep a clear lower scroll-edge cue. Show readiness beside the existing Finish action. Whether Finish should be disabled is a separate functional decision below.

**Likely source:** `onboarding/setup.rs`, `onboarding/assistant.rs` body/header allocation.

#### 6. Focus treatment has a marginal light token and thin custom outlines

**Evidence:** `meetings-find-{light,dark}.png`, `history-selected-min-{light,dark}.png`; source in `design.rs::segment` and `kit.rs::foldable_heading` applies a one-point border on focus. The light `ring` token `#c08a00` is **2.95:1** against `#fbfbfa`, below a 3:1 benchmark. The focused Find field is visible, so focus is not wholly absent.

**Why it matters:** W4 distinguishes basic visibility from a strong focus indicator. The stronger two-pixel-perimeter criterion is AAA; a one-point border is not automatically an AA failure.

**Fix:** Use an opaque `#8a6000` light focus token and `#ffc72c` dark token. Give custom focus indicators a 2 pt outline with 2 pt clearance. Preserve layout when it appears. Audit the actual focused/unfocused pixels and scroll clipping after implementation.

**Likely source:** `assets/themes/guidy.json`, `design.rs::segment`, `kit.rs`, `pages/history.rs` focus treatment.

#### 7. Too much essential explanation uses the smallest general text role

**Evidence:** `main-advanced-{light,dark}.png`, `msettings-recording-{light,dark}.png`, `msettings-setup-{light,dark}.png`. Descriptions explaining privacy, model behavior, and setup requirements all share the 11 pt secondary role with captions and minor metadata.

**Why it matters:** A1 permits this size, but a size suitable for an occasional caption is tiring across whole paragraphs. Several pages require reading these descriptions to configure the feature correctly.

**Fix:** Split `TEXT_SECONDARY` into 12/16 pt helper text and 11/14 pt compact metadata. Use 13/18 pt for longer setup/privacy explanations. Reserve 11 pt for timestamps, counts, and brief badges. Do not enlarge the entire interface indiscriminately.

**Likely source:** `design.rs` type roles, `kit.rs::SettingRow`, meeting settings and onboarding row builders.

#### 8. Titles, section labels, and rows have too few distinct type roles

**Evidence:** `main-general-{light,dark}.png`, `main-postprocessing-{light,dark}.png`, `main-storage-{light,dark}.png`. The 13 pt toolbar title is the body size with weight added; 11 pt group headings are also the helper size. The page hierarchy is conveyed mostly through placement and boxes.

**Why it matters:** A1 and rubric 5 call for role distinction. The current hierarchy works, but it has little reserve when a screen becomes dense.

**Fix:** Set page titles to 17/22 semibold, group headings to 12/16 semibold, row labels to 13/18 regular or medium, and helpers to 12/16 regular. Use the larger title once per page. Keep list headings compact rather than promoting every label.

**Likely source:** `design.rs::TEXT_TITLE`, `section_header`, `main_window.rs` toolbar title, `pages/meetings/detail.rs` object header.

#### 9. Repeated descriptions make model and settings screens harder to scan

**Evidence:** `onboarding-picker-{light,dark}.png` repeats “English only” for Moonshine and repeats language coverage for Parakeet. `main-general-{light,dark}.png` repeats “The keyboard shortcut…” beneath already explicit shortcut labels. The People “You” screen explains voice recording both above and inside its card.

**Why it matters:** N's minimalism and familiar-language heuristics favor information that helps a decision, not restatement.

**Fix:** Keep one capability statement per model. Replace repetitive shortcut helpers with the behavior difference, where one exists. Put the voice-recording explanation in one place. Apply sentence case consistently: “Dictation key”, “Mute while recording”, “Choose your setup”. Keep Mac terms such as “menu bar” instead of “tray”.

**Likely source:** `onboarding/model_picker.rs`, `kit.rs::model_description`, `pages/general.rs`, `pages/meetings/people.rs`, `assets/locales/en/translation.json` and equivalent localized strings.

#### 10. The top-level sidebar gives work and maintenance equal visual status

**Evidence:** Every `main-*-{light,dark}.png` shows ten uninterrupted peers: General, History, Meetings, Models, Advanced, Post Process, Usage, Storage, Debug, About. Debug and About have the same spacing and prominence as the two libraries.

**Why it matters:** A3 and N support navigation organized around how people look for work. This is an information architecture cost, not an argument to remove destinations.

**Fix:** Introduce quiet groups such as Library, Preferences, and App, with 12–16 pt separation. Keep History and Meetings together, General/Models/Post Process/Advanced together, and maintenance destinations together. Retain 28–30 pt source-list rows and the existing familiar icons. A larger move to a dedicated preferences window is a later functional idea, not necessary for this pass.

**Likely source:** `main_window.rs` section ordering and source-list rendering.

#### 11. Playback is permanently prominent even when the task is reading notes

**Evidence:** `meetings-detail-{light,dark}.png` and the wide variants keep the player row directly below the header at 0:00, above Notes and Summary. Combined with action controls, it delays the document.

**Why it matters:** A4 and rubric 36 favor the current task. Playback is useful but does not need to dominate a note that is not playing.

**Fix:** Use a compact play/time control integrated into the header while idle, and reveal the full seek strip during playback or on explicit request. Keep a consistent place for it so it is discoverable. Aim for the first note text within roughly 110 pt of the meeting content top.

**Likely source:** `pages/meetings/detail.rs`, `pages/meetings/player.rs`.

#### 12. Transcript metadata consumes too much of the narrow reading column

**Evidence:** `meetings-transcript-{light,dark}.png`, `meetings-find-{light,dark}.png`. Speaker and time use a separate left column, a play glyph uses another position, and the sentence starts well into the remaining detail width. Short utterances often wrap over several lines.

**Why it matters:** A2 and rubric 8/28: the transcript, rather than its scaffolding, should receive the width.

**Fix:** Below a 560 pt transcript width, place speaker and timestamp on a compact metadata line above the utterance. Put play in a 24–28 pt leading target or reveal it on row focus. Keep the wide-column treatment where it produces useful reading measure. Use 14/21 pt utterance text.

**Likely source:** `pages/meetings/detail.rs` transcript segment rendering and fixed metadata gutter.

#### 13. Compact History expands short text into a fixed-height block

**Evidence:** `main-history-min-{light,dark}.png`, `history-selected-min-{light,dark}.png`. One sentence occupies a large empty selected card before the remaining history rows. Source confirms a fixed 200 pt detail height in the inline layout.

**Why it matters:** Density and task continuity suffer. People are scanning a history list, not entering a full document workspace.

**Fix:** Let inline detail grow with content from roughly 96 pt to a 240 pt maximum, with internal scrolling only for longer transcripts. Hide the scrubber when recording is unavailable, retaining a short textual availability message. Keep copy/save actions consistently positioned.

**Likely source:** `pages/history.rs::render_row`, inline detail height and player layout.

#### 14. Wide model lists stretch the comparison distance unnecessarily

**Evidence:** `main-models-wide-{light,dark}.png`. Model identity is at the left edge while sizes, state, and actions sit near the far right of a nearly window-wide row. Narrow names and verbose descriptions are still truncated in other sizes, so this width is not resolving the actual row priorities.

**Why it matters:** A2 and rubric 8/29 favor a stable comparison path. Available width should improve understanding rather than increase eye travel.

**Fix:** Cap the model catalogue at 960 pt. Use a flexible name/details column plus stable size, state, and action columns. At narrow widths wrap secondary information. Keep the catalogue's heading, search, and filters aligned with its content rather than the far window edge.

**Likely source:** `pages/models.rs` page/group layout, `model_card.rs` column allocation.

#### 15. Important key-sharing and readiness messages are ellipsized

**Evidence:** `msettings-ai-{light,dark}.png`. The top instruction ends at “API…” and the note beneath key entry ends at “shared with Post…”. The latter is an explanation of where a credential is reused. The originals confirm this is actual truncation.

**Why it matters:** N favors recognition and clear consequences. Instructions and privacy-relevant scope are not disposable metadata.

**Fix:** Put readiness above the Cloud/On this Mac switch, with two wrapping lines available. Put the key-storage/sharing note on its own full-width line; place the provider link on the next line or in a trailing menu. Remove unconditional truncation from these explanatory roles.

**Likely source:** `pages/meetings/ai_settings.rs` active-summary banner and provider form.

#### 16. The compact AI model catalogue has almost no usable height

**Evidence:** `msettings-ai-min-{light,dark}.png`. The banner, saved-model list, provider form, search row, and provenance footer leave roughly one catalogue row visible. A row sliver suggests more content, but the main comparison task is confined to a tiny viewport.

**Why it matters:** A2 and rubric 28. A catalogue should be usable as a list, not a slot through which one item is viewed at a time.

**Fix:** Give the catalogue at least 180–220 pt at minimum window height. Collapse provider configuration after it is available and summarize saved models compactly. Move catalogue provenance into an info disclosure. If the form needs more height, let the whole pane scroll with a sticky search rather than pinning every upstream block.

**Likely source:** `pages/meetings/ai_settings.rs` flexible list/pinned form composition, `pages/meetings/settings.rs` pane layout.

#### 17. AI prices show numbers without a visible price basis

**Evidence:** `msettings-ai-{light,dark}.png`, `msettings-ai-wide-{light,dark}.png` show pairs such as `$3 / $15` beside `200K ctx`. The visible list has no clear input/output or per-million-token heading. Usage also uses terse “Avg”, “Work”, and “Load” headings.

**Why it matters:** Rubric 31 and N's real-world language rule. A precise number is not useful if its meaning must be inferred.

**Fix:** Add one list-level label, “USD per 1M tokens, input / output”, without repeating it on every row. Spell out context in a heading or detail. In Usage use “Avg / call”, “Audio”, and an explanatory label for model load time. Keep adaptive decimal precision where sub-cent costs would otherwise become zero.

**Likely source:** `pages/meetings/ai_settings.rs` price metadata, `pages/usage.rs` columns, localized labels.

#### 18. Summary statistics look like another row of small metadata

**Evidence:** `main-usage-{light,dark}.png`, `main-storage-{light,dark}.png`. The headline totals use a small mono treatment close in visual importance to labels. The accent amount is easy to find, but the other equally useful values barely stand out.

**Why it matters:** A1 and rubric 4/5. Summary numbers should answer the page's main question quickly.

**Fix:** Set total values to 17/22 medium with tabular digits; labels to 12/16 regular. Keep the four-column summary at supported widths and preserve 12 pt insets. Do not turn every metric into a separate large card.

**Likely source:** `design.rs::stat_grid`, `pages/usage.rs`, `pages/storage.rs`.

#### 19. The storage bar's segments are not directly labeled

**Evidence:** `main-storage-{light,dark}.png`, including minimum variants. Yellow, medium gray, and pale/dark remainder form one bar, with Capacity/Occupied/Free/Guidy values beneath. “Guidy” is also part of occupied space, so the segment relationship needs interpretation.

**Why it matters:** W6 and rubric 38. Color should not carry the mapping alone, and overlapping totals should not imply disjoint categories.

**Fix:** Add a compact legend: Guidy, Other used, Free. Associate each segment with the label through position or a symbol and provide values. Keep Capacity as the total, with Guidy explicitly included in Occupied. Preserve the existing useful explanation about purgeable space.

**Likely source:** `pages/storage.rs` capacity bar and stats layout.

#### 20. Startup failure leads with internal diagnostics and a bright Quit action

**Evidence:** `startup-error-{light,dark}.png` and minimum variants. The main message says something went wrong, then a red monospaced block exposes “history manager”, OS error 13, and a database path. Quit is the amber primary action.

**Why it matters:** N's error-recovery heuristic. The user needs to understand what failed and what action helps, not parse a subsystem error.

**Fix:** For the supplied permission-denied case, lead with “Guidy can't open its history folder” and a concise consequence. Put the diagnostic string in an expandable details block. Give the existing Show Logs action useful prominence and keep Quit neutral. Do not promise a repair or Retry action until it exists.

**Likely source:** `onboarding/startup.rs::startup_error`, localized startup messages.

#### 21. Startup skeletons do not tell the user what the app is doing

**Evidence:** `startup-app-{light,dark}.png`, `startup-new-{light,dark}.png`, including minimum variants. Both are entirely placeholder geometry, with an empty title area and no readable loading label.

**Why it matters:** N's visibility-of-status heuristic. Skeleton structure can prevent layout jumps, but it does not explain a delay to a first-time user.

**Fix:** Add “Starting Guidy…” in the stable header or footer. Show a more specific stage only when backed by real state. Keep static placeholders; do not add an animated spinner or shimmer to make the screen feel active. The duration and escalation policy require later runtime testing.

**Likely source:** `onboarding/startup.rs` skeleton builders, `main_window.rs` startup branch.

#### 22. The empty meeting library announces the same absence twice

**Evidence:** `meetings-empty-{light,dark}.png`. The list says “No meetings yet.” while the detail says “No Meetings Yet”, with both a central Record button and the toolbar's Record button.

**Why it matters:** N and rubric 25/37. The empty layout spends two columns and repeated copy on one simple message.

**Fix:** Use one central state spanning the empty library content, with “No meetings yet”, one sentence, and one Record action. Keep the global sidebar and normal toolbar geometry. Let the list/detail split appear when content exists.

**Likely source:** `pages/meetings/mod.rs`, `pages/meetings/list.rs` empty presentation.

#### 23. Transcript speaker-edit cues look nearly unavailable at rest

**Evidence:** `meetings-transcript-{light,dark}.png` and full variants. The small person/edit glyph at each utterance's right edge is much fainter than the readable text. Source applies `text_faint` with 0.35 opacity until row hover.

**Why it matters:** Rubric 16/17 and W2 where the icon is needed to discover the action. Making an affordance barely visible is different from cleanly disclosing it.

**Fix:** Either show a readable neutral 14 pt glyph with a 24 pt target, or reveal it on both keyboard focus and pointer hover while offering a clear row/context command. Use full-contrast focus treatment. Do not render an enabled-looking ghost at 35% opacity and rely solely on hover for discovery.

**Likely source:** `pages/meetings/detail.rs` segment speaker-edit action near the row's trailing edge.

#### 24. The collapsed recording HUD carries little explicit recording context

**Evidence:** `recording-widget-{light,dark}.png`, compared with expanded variants. The compact pill shows level bars, Pause, and a red square. It has no elapsed time or text label in the captured state; the expanded panel has both. A waveform may be mistaken for a general audio control when the window is encountered without context.

**Why it matters:** Visibility of system status and W6. Recording deserves stable, recognizable feedback beyond changing levels and color.

**Fix:** Reuse the existing timer in the compact HUD with tabular digits and a stable recording symbol. Keep Pause and Stop at 28 pt targets and distinct silhouettes. Preserve the compact footprint as far as possible; offer the explicit expanded state on focus/activation. The opaque canvas surrounding the pill is excluded from this finding.

**Likely source:** `recording_widget.rs` compact pill/wings, timer presentation, state model already supplying the expanded panel.

#### 25. Repeating decorative motion conflicts with the desired quiet desktop behavior

**Evidence:** Stills `overlay-live-{light,dark}.png`, `recording-widget-{light,dark}.png`, and startup images show only a single phase. Source confirms repeat loops in `design.rs::sheen`, `kit.rs::spinner`, `overlay.rs::status_dot` and waveform fallback, plus recording-widget pulse code. A still cannot prove frame rate, CPU/GPU cost, or Reduce Motion behavior elsewhere.

**Why it matters:** A6, W7, and the owner's explicit preference against continuously repainting decorative effects. This is a source-supported concern, not a measured performance regression.

**Fix:** Use stable status dots and static named progress states. Drive meters from actual audio changes and stop updates when hidden or idle. Keep necessary timer updates to one per second. Restrict entrance/exit transitions to roughly 120–180 ms and honor Reduce Motion. Remove perpetual decorative sheen/pulse rather than merely slowing it down.

**Likely source:** `design.rs`, `kit.rs`, `overlay.rs`, `recording_widget.rs`, `download_control.rs` for any indeterminate presentation using those helpers.

### Target design language

Keep the neutral Mac source list, grouped settings, restrained amber identity, and compact footprint. Give actual content more room. Claude is the guide for reading measure, Codex for separating different work regions, and T3 Code for settings-column allocation. This proposal is an implementation specification for later mock selection, not an approved component redesign.

#### Type and weight

Keep Geist for this pass. A system-font switch alone will not repair the layout failures, and Apple permits a legible custom font. Use Geist Mono only for exact IDs, code-like values, and clocks; use tabular proportional digits for financial summaries where supported.

| Role | Size / line height | Weight | Use |
| --- | --- | --- | --- |
| Setup heading | 20 / 26 pt | 600 | One per assistant screen. |
| Page / meeting title | 17 / 22 pt | 600 | One clear object or page title. |
| Reading body | 14 / 21 pt | 400 | Notes, summaries, transcript, longer explanations. |
| Control / setting label | 13 / 18 pt | 400; 500 for list names | Normal desktop controls and rows. |
| Group heading | 12 / 16 pt | 600 | Above its group, with an 8 pt gap. |
| Helper / metadata | 12 / 16 pt | 400 | Important explanation, dates, secondary values. |
| Compact caption | 11 / 14 pt | 400 or 500 | Short noncritical counts and badges only. |
| Summary value | 17 / 22 pt | 500 | Usage and storage totals; tabular digits. |
| HUD transcript | 15 / 22 pt | 400 | Upright text, not a whole sentence in italics. |
| Exact ID / clock | 12 / 16 pt | 400 | Monospaced, with enough width or a detail view. |

No thin weights. Bold should identify the group or object, not an entire explanation. Sentence case applies to headings and actions. Use the same capitalization in navigation, title, and empty states.

#### Spacing, geometry, and width

Use a `4, 8, 12, 16, 20, 24, 32` pt spacing scale. Keep 20 pt outer page insets and 12 pt panel insets. Put 8 pt between a heading and its group and 24 pt between unrelated groups. Use 4 pt label-to-helper spacing and 8–12 pt between related controls. Do not force decorative artwork into otherwise clear forms.

| Element | Target |
| --- | --- |
| App sidebar | 192–208 pt; 200 pt default; 28–30 pt rows. Group destinations without inflating every row. |
| Unified toolbar | 44 pt; preserve actual macOS traffic-light clearance. |
| Status bar | 24 pt; 12 pt text, useful model/download state, quiet update action. |
| Settings form | 520–640 pt preferred; 640 pt cap; stack rows when label measure would be under 220 pt. |
| Standard control | 26 pt drawing height; ≥24×24 pt actual hit target. |
| Toolbar action | 28 pt hit target; 14–16 pt glyph; 8 pt between unrelated groups. |
| Assistant action | 30 pt drawing height, 13 pt label; footer 48–52 pt. |
| Setting row | 36 pt minimum; typically 52–64 pt with explanation; grow for wrapping. |
| History / meeting list row | Roughly 44–52 pt for title plus one metadata line; extra lines only when useful. |
| Model row | 48–56 pt regular; 64–80 pt compact if details/actions wrap. Identity always outranks badges. |
| Catalogue | 960 pt maximum; consistent quantity/action columns. |
| Reading column | About 520–680 pt, adjusted to roughly 55–80 characters per line. |
| Chip / control / panel radius | 3 / 4 / 6 pt. |
| Expanded HUD radius | 14 pt; compact pill remains a capsule. |
| Divider | 1 logical pt for now; align cleanly at 2×. No shadow on routine settings panels. |

Breakpoints must use the space available to the component. With the global 200 pt sidebar present, a secondary 220–240 pt category list should remain only when its form can still be at least 520 pt wide after padding. Otherwise use the existing category popup. For meeting reading, retain list/detail only while the detail remains useful; retain the existing compact back navigation. Show notes and transcript side by side only when each reading pane has at least roughly 440 pt after gutters. Never silently squeeze one pane to preserve a nominal multi-column layout.

For 720×480 onboarding, keep the fixed header compact, the footer outside the body scroller, and the currently relevant configuration visible first. Use optional disclosures rather than shrinking type. For History's inline selected item, use content-based height with a cap, rather than a fixed 200 pt block for a sentence.

#### Surfaces and text in both themes

These are opaque sRGB proposals. Ratios were calculated with the WCAG relative-luminance formula from hex values, before anti-aliasing. Surface-to-surface ratios are intentionally subtle; they do not substitute for a necessary control/state indicator. Final composited alpha colors must be recalculated on their actual backgrounds.

| Role | Light hex | Primary-text contrast | Dark hex | Primary-text contrast |
| --- | --- | --- | --- | --- |
| Canvas | `#f7f7f5` | 16.06:1 with `#1b1b1b` | `#171717` | 15.31:1 with `#ededed` |
| Frame / sidebar | `#eeeee9` | 14.80:1 | `#212121` | 13.75:1 |
| Content panel | `#ffffff` | 17.22:1 | `#202020` | 13.92:1 |
| Raised / hover | `#ebebe7` | 14.41:1 | `#2a2a2a` | 12.26:1 |
| Selected row fill | `#fff0bd` | 15.13:1 | `#3b321c` | 10.81:1 |

Light frame, panel, and raised surface differ from the canvas by 1.09:1, 1.07:1, and 1.11:1 respectively. Dark frame, panel, and raised surface differ from the canvas by 1.11:1, 1.10:1, and 1.25:1. Selected fills differ from their content panel by 1.14:1 light and 1.29:1 dark. Thus selection also needs a persistent shape/mark and weight change. These fills alone are insufficient as the sole necessary state cue.

| Semantic role | Light | Contrast on white / raised | Dark | Contrast on panel / raised |
| --- | --- | --- | --- | --- |
| Primary text | `#1b1b1b` | 17.22 / 14.41 | `#ededed` | 13.92 / 12.26 |
| Secondary text | `#575751` | 7.27 / 6.09 | `#b0b0b0` | 7.51 / 6.62 |
| Tertiary text | `#62625d` | 6.13 / 5.13 | `#989898` | 5.65 / 4.98 |
| Necessary neutral control edge | `#85857e` | 3.71 / 3.11 | `#767676` | 3.59 / 3.16 |
| Accent text / focus / state stroke | `#8a6000` | 5.59 / 4.68 | `#ffc72c` | 10.44 / 9.20 |
| Error text / icon | `#b42318` | 6.57 / 5.50 | `#ff8178` | 6.72 / 5.92 |
| Success text / icon | `#216f3d` | ≥4.5 on both | `#84d89a` | 9.52 / 8.39 |

On the dark selected fill, use secondary `#b0b0b0` for all helper/ID text: it gives 5.84:1. Tertiary `#989898` would give only 4.39:1 there, so it must not carry small selected-row text. On light selection, tertiary `#62625d` gives 5.39:1 and accent ink gives 4.92:1. This explicit selected-surface override avoids repeating the current design's “one faint token works everywhere” assumption.

Decorative separators can use `#dcdcd6` light and `#343434` dark. Do not use them as the sole outline for an otherwise unidentified radio, checkbox, focused item, or critical state. Necessary neutral control edges use the stronger tokens above; labeled buttons need not all acquire heavy outlines.

#### Accent and state rules

- Keep `#ffc72c` as the primary action fill, with `#121212` text: **12.01:1**. One visually dominant action per region is enough.
- On light surfaces, yellow never serves alone as a necessary outline, selection ring, small text color, or check mark. Use `#8a6000` for those roles. It gives 5.22:1 on the proposed canvas.
- On dark surfaces, use amber for primary actions and selected marks. Keep ordinary navigation neutral. A selected row also gets a stable 2 pt leading mark or a check, with clear text weight; the tint is supplementary.
- Restrict model rows to one compact state badge at narrow widths. Put optional capabilities in a detail line or disclosure. Status words such as “Downloading”, “Paused”, “Failed”, and “Default” must not depend on hue alone.
- Keep recording red plus a stable recording symbol and timer. Use amber for a primary action or selection, not every live indicator, monetary total, badge, and transcript identity simultaneously.
- Error messages use the error foreground on a subtle tint with an icon and words. Ensure text still passes after compositing. Disabled controls are visually distinct, but explanatory labels should remain readable; inactive controls have WCAG contrast exceptions and should not be counted as ordinary text failures.
- Focus is an opaque 2 pt outline with clearance, using accent ink light and amber dark. Ensure the ring survives adjacent fills and keyboard scrolling. Do not confuse a selected row with focused navigation.
- Prefer static progress with a named operation. Meters reflect real input, and decorative animation ends. The recording widget already contains a `reduce_motion()` guard on its live dot; retain that protection while removing the unnecessary normal-mode repeat.

#### Baseline contrast checks behind the diagnosis

| Existing pair | Ratio | Interpretation |
| --- | --- | --- |
| Dark `#ededed` on raised `#242424` | 13.26:1 | Strong primary text. |
| Dark muted `#9a9a9a` on `#242424` | 5.52:1 | Passes normal text benchmark. |
| Light muted `#5c5c57` on `#ebebe7` | 5.62:1 | Passes normal text benchmark. |
| Light accent ink `#8a6000` on white | 5.59:1 | Existing light accent-text strategy is sound. |
| Yellow `#ffc72c` on white | 1.56:1 | Weak necessary state mark; good fill when paired with readable dark content. |
| Light focus `#c08a00` on canvas `#fbfbfa` | 2.95:1 | Below the 3:1 benchmark; rounding to 3.0 would conceal the miss. |
| Light input border `#d6d6d1` on white | 1.46:1 | Too weak if it is the sole necessary control-identification cue. |
| Dark input border `#4a4a4a` on `#141414` | 2.08:1 | Same conditional concern; inspect actual control context. |
| Light faint `#6f6f6a` on raised `#ebebe7` | 4.23:1 | This token is not safe for normal text on every advertised surface. |
| Dark faint `#858585` on raised `#242424` | 4.21:1 | Same token-level limitation. |
| Light decorative hairline `#e3e3df` on white | 1.29:1 | Fine for optional separation; not sufficient as a necessary state cue. |

These calculations separate demonstrable palette limitations from impressions of faintness. The faint-token rows expose an unsafe contract in `design.rs`, not proof that every current text placement fails. Dark selected-row IDs and low-opacity transcript affordances need checking against the final composited fill rather than just a raw foreground token.

### Functional bugs or improvement ideas for later

These are deliberately outside the visual pass. No runtime bug is claimed solely from a fixture screenshot.

1. **Clarify shortcut precedence.** General shows both “Dictation key” and “Transcribe shortcut” set to Right Option. Verify whether they overlap, supplement each other, or have different activation behavior. If both are needed, their names should explain the distinction. Evidence: `main-general-{light,dark}.png`.
2. **Settle cloud Finish validation.** Finish appears enabled while the API key is empty in the cloud setup fixture. Determine whether the app intentionally permits incomplete setup and what state it opens into. If incomplete setup is supported, make that outcome explicit; otherwise validate or guide the user to the missing field. Evidence: `onboarding-setup-cloud-{light,dark}.png` and minimum variants.
3. **Distinguish configured from usable models.** An AI default can be listed while the banner asks for its provider key. Define “saved”, “default”, “downloaded”, and “ready” separately in the state model, so UI labels can be accurate. This is a product-state question, not evidence that the backend is presently wrong. Evidence: `msettings-ai-{light,dark}.png`.
4. **Review recording retention against retranscription.** History offers retranscription while the fixture reports “Recording not found”; meeting settings can delete raw audio after three days. Verify whether unavailable actions are disabled with an explanation and whether the retention choice states what is lost. Evidence: `main-history-{light,dark}.png`, `msettings-general-{light,dark}.png`.
5. **Define generation during an active recording.** The wide recording fixture exposes a Generate summary action beside an unfinished transcript. Confirm whether it summarizes the current portion, waits, or replaces the later summary, and communicate that behavior. Evidence: `meetings-recording-wide-{light,dark}.png`.
6. **Improve startup recovery options when supported.** For a known unwritable history location, consider a supported retry or data-location recovery flow. Do not add a cosmetic Retry button without a safe operation behind it. Evidence: `startup-error-{light,dark}.png`.
7. **Make person merging reviewable and reversible.** The People screen describes moving voiceprints and speaker names into another person. Verify confirmation, affected-meeting scope, and undo semantics before enabling a merge. Evidence: `mpeople-person-{light,dark}.png`. The fixture shows the unchosen, disabled state; it cannot prove confirmation is absent.
8. **Audit keyboard and assistive technology flows.** Exercise category changes, model selection, dialogs, nested scroll regions, transcript find, speaker naming, and HUDs. Verify actual target bounds, labels/roles, tab order, and focus retention after resizing. Existing source includes focus and ARIA work; this review does not establish its end-to-end behavior.
9. **Measure animation and idle power.** Profile a recording and a background download on a high-refresh display after the decorative repeats are removed. Stills and repeat-loop source are sufficient to recommend a simpler design, not to report GPU usage or prove an idle-power regression.
10. **Consider a separate preferences destination later.** A library-focused main window and a conventional preferences window could reduce the current overlap of work and configuration. It would change navigation, window persistence, and shortcuts, so it should be a product decision rather than an incidental styling change.

### Scope of the next round

First compare static alternatives for model rows, meeting settings at 900/720 pt, and the compact meeting header, using both themes. The owner should choose among those before source implementation. Evaluate the next screenshot round with the same 40 scores, and give special attention to the five high-priority findings. A 9/10 sign-off needs readable identity and configuration at minimum size, a clear reading hierarchy, and verified interaction accessibility. Raising the average through cosmetic refinements alone would not earn it.
