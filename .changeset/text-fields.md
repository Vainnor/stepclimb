---
"@stepclimb/ui": minor
---

Add the text fields: the shared `Field` parts (`FieldLabel`, `FieldDescription`, `FieldError`
and the `field` recipe), `TextField`, `TextArea`, `NumberField` (locale formatting, tabular
numerals, step buttons named from the strings catalog) and `SearchField` (clear button named
from the strings catalog). An invalid field with no message of its own shows its `validate`
function's text, or a message from the strings catalog, never the browser's.
