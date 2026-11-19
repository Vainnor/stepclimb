---
"@stepclimb/ui": patch
---

`StringsProvider` gives React Aria its locale through `I18nProvider`: the browser's locale when
it shares the UI's language (an English UI in a British browser formats dates the British way),
otherwise the UI locale (an English UI in a German browser stays English). Date and number
formats, date-segment names and drag-and-drop announcements follow it instead of the browser's
language alone. `reactAriaLocale(uiLocale, browserLocale)` is the rule.
