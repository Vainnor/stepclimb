---
"@stepclimb/ui": patch
---

DatePicker, DateRangePicker and TimeField wrap a narrow field only between the date, the time and
the time zone, never inside the time, so "AM EST" no longer falls to a line of its own away from
the hour. `Forms/DatePicker / Narrow` shows the field in a container as narrow as a sheet on a
phone.
