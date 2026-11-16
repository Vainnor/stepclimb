---
"@stepclimb/ui": minor
---

Add `FileDrop`: a drop zone with a "Choose file" button on React Aria's DropZone and FileTrigger.
It refuses a drag that carries no file, and lists each file of a type or size the caller does
not accept with the reason. It hands the rest to `onSelect`, shows an upload's progress and an
error from outside, and hides the drag prompt on touch screens.
