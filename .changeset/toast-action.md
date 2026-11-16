---
"@stepclimb/ui": minor
---

`Toast` takes one optional `action` (`{ label, onAction }`), drawn as a small button under its
text. A toast with an action has no timer: it stays until the action is taken, which also closes
it, or it is dismissed. The web app's "A new version is available" toast uses it for "Reload".
