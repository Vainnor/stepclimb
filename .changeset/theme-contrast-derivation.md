---
"@stepclimb/tokens": minor
"@stepclimb/ui": patch
---

The theme generator now derives dark and high-contrast themes from a brand color as well as
light, and `contrastReport()` measures every token pair spec 12 requires (4.5:1 text and 3:1 UI,
7:1 and 4.5:1 in high contrast), warning when the brand's hue is close to `danger`'s. The default
and vZDC themes are generated at build time instead of hand-written, and the build fails on a
theme with a failing pair, including `line-strong` on `bg-sunken`. Light `warning`, light and dark `line-strong` and light `fg-soft` are
darkened or lightened just enough to pass. `prefers-contrast: more` selects high contrast when
the page sets neither light nor dark, and `base.css` gives it light form controls.
