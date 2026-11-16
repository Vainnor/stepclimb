---
"@stepclimb/ui": minor
---

Add `FileDrop`: a drop zone with a "Choose file" button on React Aria's DropZone and FileTrigger.
It refuses files of a type or size the caller does not accept and lists each with the reason,
hands the rest to `onSelect`, shows an upload's progress and an error from outside, and hides
the drag prompt on touch screens.
