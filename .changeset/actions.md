---
"@stepclimb/ui": minor
---

Add the Actions and two Overlays: `Button`'s loading state (`isPending`: the label stays its
accessible name under a spinner named "In progress"), `IconButton` (required label, shown as a
tooltip), `ButtonGroup`, `ToggleButton` and `ToggleButtonGroup`, `Menu` (a popover at 768 px and
wider, a bottom sheet under it; submenus and checkable items), `Toolbar` (one tab stop, arrow keys
inside), `CopyButton` (Clipboard API, result announced in a live region), `Tooltip` and `Popover`.
Popovers, tooltips and the menu's sheet repeat the surrounding ThemeProvider's theme, density and
brand, since React Aria portals them out of it.
