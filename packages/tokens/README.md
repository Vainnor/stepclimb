# @stepclimb/tokens

Stepclimb UI's design tokens: W3C design-token (DTCG 2025.10) JSON in `src/tokens/`, built with
[Terrazzo](https://terrazzo.app) (settings in `src/config.ts`) into:

- `build/css/variables.css`: every theme as `--sc-*` CSS variables. The default brand is on
  `:root` and `[data-sc-theme="light"]`, dark mode follows `[data-sc-theme="dark"]` and then
  `prefers-color-scheme`, high contrast is `[data-sc-theme="high-contrast"]`, and the vZDC
  example brand is on `[data-sc-brand="vzdc"]`. A brand's dark and high-contrast themes match only
  when `data-sc-brand` and `data-sc-theme` sit on the same element: a `data-sc-theme` on an inner
  element without `data-sc-brand` gets the default brand's theme. `[data-sc-density="compact"]` on any element
  sets the compact control heights for that element and its children.
- `build/css/tailwind-theme.css`: Tailwind 4's `@theme inline` mapped onto those variables, with
  Tailwind's own palette removed.
- `build/docs/tokens.json` and `build/docs/tokens.md` (`src/docs.ts`): the docs tables, one per
  group of the plan's Tokens tables, with each token's name, CSS variable, usual Tailwind utility,
  description and value in the default brand's light, dark and high-contrast themes (and under
  compact density where it differs). The values are the ones written to `variables.css`. The
  JSON, exported as `@stepclimb/tokens/docs/tokens.json`, is the data Storybook's Tokens page
  (spec 13) reads. A token that is in no group of `src/docs.ts` fails the build.

The sources are `base.json` (the scales), `learning.json` (the learning states, aliases of other
color tokens), `themes/<brand>-<appearance>.json` (every color and the UI font) and
`density/compact.json`, combined by `resolver.json`. The docs tables' descriptions come from
`descriptions.json` (every color token, since the theme files are per brand, and the scale tokens
the JSON does not describe) or from a token's `$description`; the build fails on a color without
one, on an id that is not a token, on a color whose theme file describes it differently, and on
any other token described in both places.

`pnpm --filter @stepclimb/tokens build` (part of `just web-build`); `pnpm --filter
@stepclimb/tokens test` checks the output. The values are the plan's (UI tab) and those decided in
`docs/specs/m0/12-tokens-and-theme-generator.md`.

The theme generator, `src/generate.ts` with its color math in `src/color.ts`, takes an
organization's primary color (hex or `oklch()`) and UI font and returns a theme: every color token
and `font.ui`, written as DTCG JSON (`themeToDtcg`, the shape of `themes/*.json`) or as one CSS
rule with `variables.css`'s names (`themeToCss`). It builds 12-step OKLCH brand and neutral scales
at the brand's hue, gamut-maps every color into sRGB (CSS Color 4) and picks `brand`, `brand-fg`
and `ring` by WCAG contrast. The default brand's input, `oklch(0.5 0.09 195)`, reproduces
`default-light.json`. It has no DOM or Node dependency (`tsconfig.generate.json`, part of
`typecheck`). Only the light appearance is generated so far, and the build does not use it yet;
dark and high contrast, the contrast report and the build wiring are the spec's I3 to I5.

This package stays in the repository and is never published (`docs/adr/`).
