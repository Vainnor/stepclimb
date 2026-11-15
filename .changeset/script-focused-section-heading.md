---
"@stepclimb/ui": patch
---

`base.css` draws no focus ring on any heading, `h1` to `h6`, with `tabindex="-1"` while it has
focus, where it covered only `h1`. The web app's account page moves focus to a section's `h2`
after a delete removes the row that held it; like a view's `h1`, that heading is not a control.
