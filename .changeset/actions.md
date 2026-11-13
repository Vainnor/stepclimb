---
"@stepclimb/ui": minor
---

Add the Actions and two Overlays: `Button`'s loading state (`isPending`: after 300 ms the label
turns invisible under a spinner named "In progress", keeping the button's width and accessible
name), `IconButton` (required label, shown as a tooltip), `ButtonGroup`, `ToggleButton` and
`ToggleButtonGroup` (selected drawn in the brand tint inside a 2 px brand frame), `Menu` (a
popover at 768 px and wider, a bottom sheet under it; submenus and checkable items), `Toolbar`
(one tab stop, arrow keys inside), `CopyButton` (Clipboard API; the result is announced in a live
region, and a failure is also shown as text), `Tooltip` and `Popover`. Popovers, tooltips and the
menu's sheet repeat the surrounding ThemeProvider's theme, density and brand, since React Aria
portals them out of it. Touch screens give menu radio and checkbox items, grid cells and tree
items 44 px targets too.
