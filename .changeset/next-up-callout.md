---
"@stepclimb/ui": minor
---

Add `NextUp`, a course's next item with the one primary "Continue" link, or when the course opens
further or that it is complete, in the user's time zone; `Callout`, an info, tip, warning or
danger note drawn as an icon on a tinted surface; and `Announcement`, an announcement's title,
author, date and server-rendered body. `Prose` takes the server's sanitized HTML as `html`, typed
`ServerHtml`, which only `serverHtml()` makes, so a plain string does not type-check.
