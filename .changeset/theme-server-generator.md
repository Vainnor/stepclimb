---
"@stepclimb/tokens": minor
---

The theme generator takes a reading font (`serif` or `sans`) and a corner style (`square`,
`default`, `round`), and every theme carries `font.read` and the `xs`, `sm` and `md` radii, which
`variables.css` now writes in each brand's theme rules. `brandStylesheet()` writes a brand's light,
dark and high-contrast themes as the stylesheet the server serves at `/theme.css`. An `oklch()`
primary with a chroma or hue too large to be finite is refused. `test/golden/theme-vectors.json`
holds the generator's output for the server's Rust port to match.
