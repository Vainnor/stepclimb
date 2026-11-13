# @stepclimb/tokens

Stepclimb UI's design tokens: W3C design-token (DTCG 2025.10) JSON in `src/tokens/`, built with
[Terrazzo](https://terrazzo.app) into:

- `build/css/variables.css`: every theme as `--sc-*` CSS variables. The default brand is on
  `:root` and `[data-sc-theme="light"]`, dark mode follows `[data-sc-theme="dark"]` and then
  `prefers-color-scheme`, and the vZDC example brand is on `[data-sc-brand="vzdc"]`.
- `build/css/tailwind-theme.css`: Tailwind 4's `@theme inline` mapped onto those variables, with
  Tailwind's own palette removed.

`pnpm --filter @stepclimb/tokens build` (part of `just web-build`); `pnpm --filter
@stepclimb/tokens test` checks the output. The values are the plan's (UI tab). The theme
generator (`src/generate.ts`: brand color to light, dark and high-contrast palettes) is not built
yet; see `docs/specs/m0/12-tokens-and-theme-generator.md`.

This package stays in the repository and is never published (`docs/adr/`).
