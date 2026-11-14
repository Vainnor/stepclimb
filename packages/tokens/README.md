# @stepclimb/tokens

Stepclimb UI's design tokens: W3C design-token (DTCG 2025.10) JSON in `src/tokens/`, built with
[Terrazzo](https://terrazzo.app) (settings in `src/config.ts`) into:

- `build/css/variables.css`: every theme as `--sc-*` CSS variables. The default brand is on
  `:root` and `[data-sc-theme="light"]`, dark mode follows `[data-sc-theme="dark"]` and then
  `prefers-color-scheme`, high contrast is `[data-sc-theme="high-contrast"]` and then
  `prefers-contrast: more` (when the page sets neither light nor dark), and the vZDC
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
color tokens) and `density/compact.json`, combined by `resolver.json`, and the themes, which are
generated: `src/themes.ts` names the default brand's and the vZDC example's inputs, and the build
generates their light, dark and high-contrast themes (every color and the UI font) and puts them
into the theme contexts of a copy of `resolver.json` at `build/tokens/resolver.json`, which
Terrazzo reads. The docs tables' descriptions come from `descriptions.json` (every color token,
since the themes are generated per brand, and the scale tokens the JSON does not describe) or
from a token's `$description`; the build fails on a color without one, on an id that is not a
token, on a color whose JSON describes it differently, and on any other token described in both
places.

`pnpm --filter @stepclimb/tokens build` (part of `just web-build`); `pnpm --filter
@stepclimb/tokens test` checks the output. The values are the plan's (UI tab) and those decided in
`docs/specs/m0/12-tokens-and-theme-generator.md`.

The theme generator, `src/generate.ts` with its color math in `src/color.ts`, takes an
organization's primary color (hex or `oklch()`) and UI font and returns a theme per appearance
(light, dark, high contrast): every color token and `font.ui`, written as DTCG JSON
(`themeToDtcg`) or as one CSS rule with `variables.css`'s names (`themeToCss`). For each
appearance it builds a 12-step OKLCH brand scale and a neutral scale at the brand's hue (high contrast's
neutrals are grey), gamut-maps every color into sRGB (CSS Color 4) and picks `brand`, `brand-fg`
and `ring` by WCAG contrast. Status colors do not follow the brand. The default brand's input,
`oklch(0.5 0.09 195)`, reproduces the plan's values in all three appearances, with the
corrections below. It has no DOM or Node dependency (`tsconfig.generate.json`, part of
`typecheck`, covers it, `src/report.ts` and `src/themes.ts`).

`contrastReport(theme)` in `src/report.ts` measures every pair of spec 12's B20 table with the
WCAG 2 ratio on the displayed 8-bit sRGB colors: 4.5:1 text and 3:1 UI in light and dark, 7:1
and 4.5:1 in high contrast, the learning states through the tokens they alias. It also warns,
without failing, when the brand's hue is within 30 degrees of `danger`'s. The build fails with
every failing pair listed when a shipped theme does not pass (`checkThemes` in `src/themes.ts`).

Where the plan's example values fail those minimums, the principle wins (coordinator decision,
spec 12): light `warning` is 0.663 rather than 0.72, light `line-strong` 0.633 rather than 0.78,
light `fg-soft` 0.533 rather than 0.55, and dark `line-strong` 0.507 rather than 0.42, each the
smallest lightness change that passes for every brand hue. Generated colors are inside sRGB, so
`variables.css` has no `color-gamut` overrides.

This package stays in the repository and is never published (`docs/adr/`).
