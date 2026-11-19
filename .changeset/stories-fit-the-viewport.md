---
"@stepclimb/ui": patch
---

Stories fit the viewport they are drawn in. SideNav's stories and AppShell's Default take the
screen's height less the story padding instead of the full screen, and the Dialog, AlertDialog and
Sheet matrices drop the screen-tall room their closed triggers left for an overlay, so the story
suite captures each of them at 375 px as a touch render.
