---
"@stepclimb/ui": minor
---

Add `TagField`, `Slider` and `ColorField`. `TagField` takes a list of strings, each once: Enter
or its Add button adds the typed tag and announces it; a tag already in the list in any case stays
in the input with an "Already added" note, also announced. Each tag is removed with its button,
Backspace or Delete, and the caller's error shows after the tags. `Slider` sets one number with a
thumb and a number field beside it for an exact value; a typed value ends a change as a released
thumb does, and a label, `aria-labelledby` or `aria-label` names both. `ColorField` takes six hex
digits with or without "#" and shows the committed color in a swatch hidden from screen readers
and not drawn while the field is empty; its values are React Aria's `Color`.
