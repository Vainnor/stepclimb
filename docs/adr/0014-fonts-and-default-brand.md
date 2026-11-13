# 0014. Fonts and the default brand

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/ui.md` -> Library architecture (Type), UI decisions; `packages/tokens`

## Context

The default theme is what self-hosters see first, and the reading experience is the product's
core.

## Decision

UI text is Atkinson Hyperlegible Next, long-form reading is Source Serif 4 (organizations can
switch it to sans), data and codes are Atkinson Hyperlegible Mono. The default brand color is the
deep sea-green of the plan's own page (`oklch(0.50 0.09 195)` in light mode). Organizations bring
their own brand color, logo and font; the theme generator derives accessible light, dark and
high-contrast themes.

## Consequences

The fonts are OFL and installed from `@fontsource`. The values are in
`packages/tokens/tokens/themes/`.
