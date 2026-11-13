---
"@stepclimb/ui": minor
---

Add `ThemeProvider` and `useAppearance()`: the theme, density and brand attributes on one element.
With no theme set, the outermost provider follows `prefers-contrast: more` to high contrast, then
`prefers-color-scheme`; a nested provider keeps its parent's values for whatever it does not set.
Every component's stories carry a status (experimental, stable, deprecated), and a `className`
color or type utility on a Stepclimb UI component now warns.
