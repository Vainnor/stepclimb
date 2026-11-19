---
"@stepclimb/ui": patch
---

A focused field keeps its brand border while the pointer is over it. The hover border applied
after the focus border, so a field built on Field's control (TextField, TextArea, NumberField,
SearchField, Select, ComboBox, the date and time fields, TagField, ColorField) clicked into with a
mouse showed the hover color until the pointer moved away. Hover now leaves a focused
or invalid field's border alone. `Forms/TextField / Focused Hovered` shows a field in both states.
