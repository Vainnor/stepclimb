---
"@stepclimb/ui": patch
---

Fix `Select` and `ComboBox` inside a `Dialog` or `Sheet` under 768 px: their bottom sheet took
the surrounding overlay's open state, so it was drawn as soon as the Sheet opened, listed every
option twice and made the Sheet inert under it. The sheet now opens and closes with the field's
own state.
