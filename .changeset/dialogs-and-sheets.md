---
"@stepclimb/ui": minor
---

Add `Dialog` (with `DialogTrigger`, `DialogBody` and `DialogFooter`), `AlertDialog` (the confirm
button names the action, focus starts on Cancel, the scrim does not dismiss it) and `Sheet` (at
the inline end from 768 px, from the bottom under it, inside the safe areas). Their scrim is
`bg-overlay` with a blur.

Modal overlays, the Menu's sheet included, are as tall as the visual viewport
(`--visual-viewport-height`, which React Aria updates as the on-screen keyboard opens) rather than
the layout viewport, which iOS does not shrink for the keyboard.
