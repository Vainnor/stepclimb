---
"@stepclimb/ui": minor
---

Add the page navigation: `Breadcrumbs` and `Breadcrumb` (the last crumb is the current page),
`Tabs`, `TabList`, `Tab` and `TabPanel`, and `Pagination` (Previous and Next by cursor, with an
optional result count). `MENU_POPOVER_QUERY` now reads the shared 768 px breakpoint; its value
is unchanged.

Pagination announces the page that arrived, and moves focus to the other button when the one
pressed is disabled on the new page. Only the breadcrumbs `nav` is named.
