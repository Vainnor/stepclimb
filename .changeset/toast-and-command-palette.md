---
"@stepclimb/ui": minor
---

Add `Toast`: `createToastQueue()` for the app's one queue and `ToastRegion` to render it. A toast
is announced without taking focus, its timer pauses while the region is hovered or focused, a
toast that is not an error stays at least 5 seconds (6 by default), and an error toast stays
until it is dismissed. React Aria's `UNSTABLE_` toast exports are imported in `toast.tsx` only
(ADR 0024).

Add `CommandPalette`: Ctrl+K or ⌘K opens it, typing filters by label and keywords, the arrow keys
move through the matches while focus stays in the search box, Enter runs a command, Escape clears
the search and then closes it.
