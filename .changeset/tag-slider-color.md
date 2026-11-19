---
"@stepclimb/ui": minor
---

Add `TagField`, `Slider` and `ColorField`. `TagField` takes a list of strings: Enter or its Add
button adds the typed tag, refusing a blank one or one already in the list in any case, and each
tag is removed with its button, Backspace or Delete. `Slider` sets one number with a thumb and a
number field beside it for an exact value, both bound to React Aria's slider state; a typed value
ends a change as a released thumb does. `ColorField` takes six hex digits with or without "#" and
shows the committed color in a swatch hidden from screen readers; its values are React Aria's
`Color`.
