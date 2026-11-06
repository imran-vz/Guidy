# Phase 1: research the rubric and review the baseline

Read `docs/ui-review/COUNCIL_BRIEF.md` first. It describes your role, the
product, the evidence and the scoring scale. Screenshots for this phase are
in `target/ui-review/round-0/`.

## 1. Build the rubric

The product owner requires these parameters:

1. Typography (typeface choice, sizes, legibility)
2. Font weight usage
3. Color contrast (text and non-text, both themes)
4. Information hierarchy
5. Typographic hierarchy
6. Design language (one coherent visual system)
7. Padding and spacing
8. Width and layout measure (content widths, line length, use of space)
9. Color and theme (palette, accent use, light/dark parity)
10. Overall consistency

Research about 30 more parameters that a senior desktop-app design review
should cover. Draw on Apple HIG (macOS), WCAG 2.2 (contrast, focus
appearance, target size, text spacing, reflow), Nielsen's heuristics,
Gestalt principles, and how Codex, Claude, T3 Code, Xcode and System
Settings handle layout, chrome and density. Examples of areas to consider,
not a fixed list: alignment and grid, vertical rhythm, iconography,
control styling and sizing, affordance and signifiers, interactive states
(hover, pressed, selected, disabled), focus visibility, navigation and
wayfinding, toolbar and chrome design, surface elevation and layering,
borders and dividers, corner radius consistency, empty states, loading and
progress states, error and status messaging, data density, scannability,
microcopy and capitalization, numeric/tabular formatting, responsiveness at
min/wide sizes, dark-mode quality, light-mode quality, native platform
fit, content-first focus, visual noise, accessibility of non-text
indicators, motion (as far as stills show it).

For each parameter give: a short name, a one-sentence definition, the
source or standard it comes from, and what a 9 looks like in concrete,
checkable terms (numbers where possible: pt sizes, ratios, spacing steps).

## 2. Score three apps on your rubric

Score Guidy (round 0), Codex and Claude on every parameter (0–10, one-line
justification each). Mark knowledge-based reference scores `(k)`. Give
each app an overall score (mean of parameters, one decimal) and, separately,
a holistic "would the lead designer ship this" score.

## 3. Diagnose Guidy

- The 25 most important UI problems, most severe first. For each: what is
  wrong, the evidence (snapshot file names, both themes where relevant),
  why it matters (which standard), and the fix in concrete terms (tokens,
  sizes, colors, layout), with the likely source file.
- A short "target design language" proposal: the type scale (sizes,
  weights, line heights), spacing scale, surface/elevation tones for both
  themes with hex values and their contrast ratios, accent usage rules,
  control sizes, radii, and layout widths you would set to reach 9/10,
  using the references as a guide. Be specific enough to implement.
- Functional bugs or functional improvement ideas you notice (anything that
  is not purely visual) in a separate list. These will not be worked on
  in this pass; they are recorded for later.

## Output

Write one Markdown file: `docs/ui-review/round-0/<your-reviewer-id>.md`
(your id is given in your task). Keep it structured with the headings
above and the score tables as Markdown tables. Your final message should be
a 10-line summary: the three overall scores and the five most severe
problems.
