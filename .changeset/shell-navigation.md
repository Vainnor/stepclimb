---
"@stepclimb/ui": minor
---

Add the shell navigation: `AppShell` (skip link to `main`, the SideNav from 768 px and the
BottomTabs under it, switched in CSS), `SideNav` (current entry marked with
`aria-current="page"`, logo and footer slots, icons only with tooltips from 768 to 1023 px),
`BottomTabs` (links fixed to the bottom inside the safe area), `TopBar` (title and actions,
sticky and blurred on phones inside the top safe area) and `OrgSwitcher`. Navigation entries are
data: `NavEntry` is `{ id, href, label, icon }`.
