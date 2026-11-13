---
"@stepclimb/ui": minor
---

Add the Foundations: `Icon` (Phosphor, token sizes, regular for available and fill for complete),
`Text`, `Heading` (level required, size separate), `Link` (router navigation; "(external site)"
for screen readers on another origin), `Kbd`, `VisuallyHidden`, `Separator` and `Prose` (reading
face and measure; wide tables and code blocks scroll on their own and become tab stops). Every
recipe uses a `tv()` whose class merger knows the token theme's type names, so `text-caption`
and `font-heading` no longer drop a color or font-family class beside them.
