---
paths:
  - "packages/ui/**"
  - "packages/tokens/**"
  - "web/**"
  - "apps/ui-docs/**"
---
# UI Rules (not loaded at startup; read in full before writing a component, a token, a page or UI copy)

The source is the plan's UI tab (`docs/plan/ui.md`): principles, tokens, component conventions,
the pitfalls we refuse and the definition of done. This file is the checklist.

---

## Tokens and styling

- **Semantic tokens only.** Colors, radii, type, space and motion come from `packages/tokens`
  (`bg-canvas`, `text-fg-strong`, `border-line-strong`, `bg-brand`). Tailwind's default palette
  is removed; a raw hex, `rgb()`, `oklch()` or arbitrary color outside `packages/tokens` is a
  defect.
- `className` is for layout only. Color and type changes go through a component's variants.
- **Logical properties only**: `ms-`, `me-`, `ps-`, `pe-`, `start-`, `end-`, never `ml-`, `pl-`,
  `left-`.
- A token change is made in `packages/tokens/tokens/*.json` and rebuilt; never edit `build/`.

## Components

- **React Aria Components only** as the primitive layer, never mixed with another primitive
  library. No `asChild`. Links go through React Aria's `RouterProvider` wired to TanStack Router.
- One file per component, exporting its parts and its `tv()` recipe; `data-slot` on every part;
  style React Aria's `data-*` states directly.
- Every component works controlled and uncontrolled.
- **No hard-coded strings**: labels come from `useStepclimbStrings()` (ICU messages, with the
  organization's terminology overrides). Components never hard-code nouns.
- Phosphor icons only; no emoji in JSX.

## Pitfalls we refuse (plan, UI tab)

The default shadcn look, raw colors, gradient text and glows, glass as decoration, card grids as
structure, hero-metric dashboards, eyebrow labels and fake numbering, colored side stripes, emoji
or mixed icon sets, sparkle or "AI" decoration on features that do not use AI, animation
everywhere (one moment only: the rail fill), modals for work, monospace as costume, browser
defaults left in, vague copy, and web-view tells in the apps (bounce, zoom on focus, unsafe areas,
selectable controls).

## Definition of done for a component

| Check | Requirement |
| --- | --- |
| States | Default, hover, pressed, focus-visible, disabled, loading, error, empty, long content |
| Themes | Light, dark, high contrast, the default brand and two extreme brands, both densities |
| Accessibility | Zero axe violations, keyboard-only walkthrough, VoiceOver names, contrast at least 4.5:1 text and 3:1 UI |
| Responsive and touch | 1280 and 375 px stories, 44 px targets, iOS Safari and the Tauri iOS web view |
| Tests | Vitest behavior tests and Playwright screenshots of every story |
| Docs | A Storybook page with usage, do and don't, and generated props |

A component that does not meet a row yet is `experimental` in Storybook and says which rows are
missing.

## Feedback and assessment UI

What a quiz or activity shows after an answer follows the server's response only; the UI never
holds or computes an answer the server did not send (`assessment-integrity.md`).
