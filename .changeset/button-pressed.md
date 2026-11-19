---
"@stepclimb/ui": patch
---

`Button` draws its pressed state from React Aria's `data-pressed`, one step past hover in each
intent: primary takes the brand scale's next step (`brand-fg`), secondary and danger keep hover's
fill with a stronger border (`fg-sub`, `danger`), and ghost keeps hover's fill with the strong
label color. New stories: `Primitives/Button / Pressed Primary`, `Pressed Secondary`,
`Pressed Ghost` and `Pressed Danger`.
