---
"@stepclimb/ui": patch
---

The editor's slash menu decides from where ProseMirror inserts the typed `/`, not from the
selection, which ProseMirror updates only when the browser's `selectionchange` event arrives. A
`/` typed into an empty paragraph right after moving the caret there opened no menu and was
inserted as text. The block chosen from the menu now goes where the `/` was typed.
