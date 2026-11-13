# @stepclimb/tokens

Stepclimb UI's design tokens: W3C design-token (DTCG 2025.10) JSON in `src/tokens/`, built with
[Terrazzo](https://terrazzo.app) (settings in `src/config.ts`) into:

- `build/css/variables.css`: every theme as `--sc-*` CSS variables. The default brand is on
  `:root` and `[data-sc-theme="light"]`, dark mode follows `[data-sc-theme="dark"]` and then
  `prefers-color-scheme`, high contrast is `[data-sc-theme="high-contrast"]`, and the vZDC
  example brand is on `[data-sc-brand="vzdc"]`. `[data-sc-density="compact"]` on any element
  sets the compact control heights for that element and its children.
- `build/css/tailwind-theme.css`: Tailwind 4's `@theme inline` mapped onto those variables, with
  Tailwind's own palette removed.

The sources are `base.json` (the scales), `learning.json` (the learning states, aliases of other
color tokens), `themes/<brand>-<appearance>.json` (every color and the UI font) and
`density/compact.json`, combined by `resolver.json`.

`pnpm --filter @stepclimb/tokens build` (part of `just web-build`); `pnpm --filter
@stepclimb/tokens test` checks the output. The values are the plan's (UI tab) and those decided in
`docs/specs/m0/12-tokens-and-theme-generator.md`. The theme generator (`src/generate.ts`: brand
color to light, dark and high-contrast palettes) is not built yet; see that spec.

This package stays in the repository and is never published (`docs/adr/`).
