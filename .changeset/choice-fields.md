---
"@stepclimb/ui": minor
---

Add the choice fields: `Select` and `ComboBox` (a popover at 768 px and wider, a bottom sheet
under it; ComboBox's sheet holds its input, and `isLoading` shows a spinner and a loading empty
state), `Checkbox` and `CheckboxGroup` (with the indeterminate state), `RadioGroup` and `Radio`,
and `Switch`. Labels, descriptions and errors use the shared `Field` parts.

On phones the ComboBox's button carries the field's description, error and invalid state, and its
input stays in the page, hidden, so a form submits its `name` and checks `isRequired`. The loading
spinner waits 300 ms. A Checkbox without a label is still a 44 px target on touch screens.
