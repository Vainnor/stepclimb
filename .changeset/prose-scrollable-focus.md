---
"@stepclimb/ui": patch
---

`Prose` re-checks its tables and code blocks when web fonts finish loading, not only when the
column resizes, so a code block that overflows only once its mono font arrives still becomes a
tab stop. A scrolling code block is now a group named "Code block" (`prose.codeBlock`), and
`Prose` takes back only the attributes it added when a block fits again.
