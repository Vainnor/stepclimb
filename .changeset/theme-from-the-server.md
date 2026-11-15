---
"@stepclimb/tokens": minor
"@stepclimb/ui": minor
---

Let a page take its theme from the server's `/theme.css`. The tokens build writes two more files:
`structure.css` (`@stepclimb/tokens/structure.css`), every variable no organization's theme sets
(space, type, control heights, motion, the pill radius, the floating shadow, the compact density),
and `theme.css` (`@stepclimb/tokens/theme.css`), the default brand's stylesheet as `/theme.css`
serves it. Stepclimb UI adds `@stepclimb/ui/app.css`, the same styles as `styles.css` with
`structure.css` in place of `variables.css`, so a bundle loaded after `/theme.css` never overrides
the organization's colors, fonts or radii. Storybook keeps `styles.css` and every theme.
