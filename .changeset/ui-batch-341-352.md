---
"@stepclimb/ui": patch
---

Fix four small defects:

- A menu opened with a mouse under 768 px no longer runs the bottom-sheet item that the release
  of the opening press lands on.
- Under 768 px, a control reached with Tab or Shift+Tab scrolls into view clear of the bottom tab
  bar and the sticky top bar, as WCAG 2.2 SC 2.4.11 requires.
- A hovered `FileDrop` draws the drop target's border while a file is dragged over it.
- A `Select` draws its chosen value on the middle line of its box.
