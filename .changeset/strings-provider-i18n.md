---
"@stepclimb/ui": patch
---

`StringsProvider` gives React Aria the UI locale through its `I18nProvider`, so date and number
formats, date-segment names and drag-and-drop announcements follow the locale `useStepclimbStrings()`
uses instead of the browser's language. It defaults to `en`, as `useStepclimbStrings()` does.
