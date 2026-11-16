---
"@stepclimb/ui": minor
---

Add `DatePicker`, `DateRangePicker` and `TimeField`. Each takes and gives `ZonedDateTime` values
from `@internationalized/date`, shows the zone's short name after the time, and takes a
`timeZone` (the user's own) for a value typed into the empty field. The calendar opens in a
popover at 768 px and wider and in a bottom sheet under it. Number segments have leading zeros.
Every segment but the literals is at least 24 px square, and 44 px under a coarse pointer, where
the field wraps when it must.
