---
"@stepclimb/ui": patch
---

`base.css` draws no focus ring on an `h1` with `tabindex="-1"` while it has focus. The web app
focuses a view's `h1` by script after a client-side navigation so a screen reader announces the
new view; the heading is not a control, so a ring around it is noise.
