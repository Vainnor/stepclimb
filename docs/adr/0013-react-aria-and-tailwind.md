# 0013. React Aria Components with Tailwind 4

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/ui.md` -> Library architecture; `.claude/rules/ui.md`

## Context

An LMS needs accessible dates with time zones, drag-to-reorder, trees, grids and number and
color fields.

## Decision

Stepclimb UI is built on React Aria Components as its one primitive layer, styled with Tailwind 4,
`tailwind-variants` and `tailwind-merge`, only through semantic tokens. Raw colors fail the lint.
Icons are Phosphor. No motion library; charts are hand-built SVG.

## Consequences

Tailwind's default palette is removed in the generated theme (`--color-*: initial`). The plan's
`Button` example needed one change for Tailwind 4 (`h-(--sc-control-h)`; see
`packages/ui/src/primitives/button.tsx`).
