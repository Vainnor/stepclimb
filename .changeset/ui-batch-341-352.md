---
"@stepclimb/ui": patch
---

Fix four small defects:

- A menu opened with a mouse under 768 px no longer runs the bottom-sheet item that the release
  of the opening press lands on.
- Under 768 px, a focused control scrolls into view clear of the bottom tab bar, as WCAG 2.2 SC
  2.4.11 requires.
- `FileDrop` has `isBusy`: while the caller works on files it took, "Choose file" stays focusable
  and says it is in progress, and the zone takes no files. A hovered zone now draws the drop
  target's border while a file is dragged over it.
- A `Select` draws its chosen value on the middle line of its box.
