---
"@stepclimb/ui": patch
---

`Sheet`'s scrolling body no longer sets `overscroll-behavior: contain`. React Aria's ModalOverlay
already keeps the page from scrolling while the sheet is open, and base.css keeps the document
from bouncing. With the property set, Chromium gave the body a composited layer of its own even
when nothing overflowed, and the fields in it rendered a pixel differently from run to run.
