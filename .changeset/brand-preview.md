---
"@stepclimb/ui": minor
"@stepclimb/tokens": minor
---

Add `BrandPreview` (a draft brand's light, dark and high-contrast themes side by side on a sample
screen) and `ContrastReport` (the generator's contrast report in words, failing pairs first) for
the branding screen. `@stepclimb/tokens` exports the generator, the contrast report and the color
functions to the browser (`@stepclimb/tokens/generate`, `/report`, `/color`), and
`themeDeclarations` gives a theme's custom properties, which `themeToCss` now writes.
