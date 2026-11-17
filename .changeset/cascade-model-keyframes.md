---
"@stepclimb/tokens": patch
---

The tests' cascade model skips `@keyframes` blocks, which style no element, so `base.css` can
hold ProgressRail's course-completion keyframes.
