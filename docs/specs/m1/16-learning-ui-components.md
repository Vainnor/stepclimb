# Learning UI components

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/ui.md` -> Component inventory (Learning, M1; Forms and Data, M0 to M1;
  Authoring, M1 to M2), Signature components (ProgressRail, PageReader), Principles, Key layouts
  (course home), Component conventions, Definition of done for a component, Pitfalls we refuse;
  `docs/plan/technical.md` -> Web app and native apps (phone-first layouts)
- **Depends on:** `../m0/13-base-ui-components.md` (the M0 library, Q1's M0/M1 split, Q8's
  hand-checked rows, its issue stages), `../m0/12-tokens-and-theme-generator.md` (learning-state
  tokens), `../m0/11-app-shell.md` (strings, routes), and the API of specs 02 to 15
- **Review:** standard

## Summary

This spec is the M1 part of Stepclimb UI and the learning screens built from it. It adds the M1
Forms and Data components that M0 left out (date and time fields, tag field, slider, color field,
file drop, tree, status chip), the Learning row (ProgressRail, module and item rows, Next up, due
badges and the rest), the M1 Authoring components not owned by another spec (ModuleBoard,
BrandPreview), and the screens that tie the M1 API together: course list and home, item view,
course settings, roster, authoring, files and assignments. Components owned by other specs are
listed here with a pointer so the inventory is complete in one place.

## Behavior

### Rules for every component and screen

- **B1.** Every component follows `ui.md`'s conventions and `../m0/13-base-ui-components.md`: React
  Aria Components underneath, `tailwind-variants` recipes, semantic tokens only, `data-slot` on
  every part, controlled and uncontrolled use, logical properties, strings from
  `useStepclimbStrings()` with the organization's terminology (a "course" is whatever the
  organization calls it), Phosphor icons only.
- **B2.** Every component meets `ui.md`'s definition of done: the listed states, the three themes
  with the default brand, vZDC and the two extreme brands in both densities, axe with no violation,
  1280 and 375 px stories, 44 px touch targets, Vitest behavior tests and Playwright story
  screenshots in `just ci` (`m0/13` I2). The iOS Safari and Tauri iOS web-view row stays
  experimental until the iPhone test (`17`), as `m0/13` Q8 decided.
- **B3.** Learning states have one shape, icon and token each everywhere (`ui.md` Principles):
  locked (`state-locked`, lock icon), available (`state-available`, outline circle, regular
  weight), in progress (`state-progress`, half circle), complete (`state-complete`, filled check,
  fill weight), overdue (`state-overdue`, warning), needs review (`state-review`, eye; used from
  M2). Color is never the only signal: each carries its icon and a text label, visible or
  visually hidden.

### Owned by other specs

- **B4.** These inventory components are built by the spec that first needs them and only listed
  here: PageReader (`07` I4), Editor, SlashMenu and BlockToolbar (`07` I3), VersionDiff (`07` I4),
  VideoUploader (`08` I5), VideoPlayer (`09` I3), the notification bell and list (`11` I3), and
  the dashboard page (`10` I2), which uses NextUp and the course rows from this spec.

### Forms and Data (the M1 part of `m0/13` Q1)

- **B5.** DatePicker, DateRangePicker and TimeField (React Aria's date components) take and give
  values with an explicit time zone (`ZonedDateTime`), show the zone's short name beside the
  value, and default to the user's time zone (`users.tz`) (Q2). Due dates (`13`) and unlock dates
  (`04`) use them. Coordinator decisions (overnight, 2026-11-16), pending owner review: each field
  takes a required `timeZone` for a value entered into the empty field and leaves a given value
  in its own zone; the calendar opens in a Popover at 768 px and wider and in a bottom sheet under
  it, as `m0/13` Q10 does for Select; number segments show leading zeros. Every segment but the
  literals is a target: at least 24 px square with a fine pointer (axe's WCAG 2.5.8 check) and at
  least 44 px wide and tall under a coarse pointer, the plan's touch floor (`ui.md`), with the
  field wrapping at 375 px where that does not fit. React Aria Components 1.22.1 keeps
  the old UTC offset when the day segment is stepped with the arrow keys across a daylight-saving
  change (09:00 EST stepped to the next day gives 10:00 EDT); typing the day or picking it in the
  calendar gives 09:00 EDT. T3 is tested through typing and the calendar.
- **B6.** TagField (a list of removable tags with text entry), Slider (single value, with a
  number field for exact entry), ColorField (hex entry with a swatch; the branding screen) and
  FileDrop (drop zone plus a "Choose file" button, accepting types and a size limit, reporting
  progress) follow React Aria's patterns. FileDrop is what spec 06's upload screen and spec 07's
  image insertion use. Coordinator decisions for FileDrop (overnight, 2026-11-16), pending owner
  review: it checks type (MIME type, `type/*` group or extension) and size before calling
  `onSelect`, lists each refused file with the reason in a live region, and refuses all files
  when several are dropped on a zone that takes one; these checks save a wasted upload and are
  not a control, since the server refuses only a file above the instance's `max_file_bytes` or
  over the storage limit and records its sniffed type without refusing any (`06` B4, B6); a drag
  that carries no file is refused while it is over the zone; the caller reports upload progress
  (`upload`) and the server's refusal (`errorMessage`); under a coarse pointer the drag prompt is
  visually hidden, leaving the button.
- **B7.** Tree (React Aria `Tree`) shows the account tree and course file folders, with keyboard
  expansion. StatusChip shows one learning state (B3) as a chip with icon and label.
  Coordinator decision (overnight, 2026-11-16), pending owner review: StatusChip's label can be
  visually hidden (`labelHidden`) where the row already says the state; it stays in the
  accessibility tree.

### Learning

- **B8.** ProgressRail (plan's signature component): a module's items as an ordered list of links,
  drawn as a line with one node per item; completed segments fill with the brand color. Node
  states: complete (filled with a check), current (ring with a dot), available (outline), locked
  (lock icon and, on focus or hover, what unlocks it from `04` B12's reasons), overdue (danger ring
  and due label). Locked nodes stay focusable and announce their requirement; arrow keys move
  between nodes. On completion the segment fills over 320 ms; when the course completes the whole
  rail fills once; with reduced motion both are instant. Under 768 px it opens as a bottom sheet
  from the item view. It is the one celebratory motion in the product (`ui.md`).
- **B9.** ModuleRow shows a module's number, title, StatusChip, progress (`3 / 5`) and, when
  locked, the reason in words ("Unlocks after Ground control", "Opens on 3 March"); ItemRow shows
  an item's kind icon, title, RequirementLabel, StatusChip and DueBadge. CourseHeader shows the
  course title, code, cover, the learner's progress bar and, for staff, the status.
- **B9a.** Coordinator decisions for B9 (overnight, 2026-11-17), pending owner review:
  - ModuleRow is a heading (level 2 by default) holding the module's number and title, then its
    StatusChip and, unless it is locked, the learner's progress, drawn "3 / 5" and read "3 of 5
    complete". It is not a control, a toggle or a landmark; its items follow as ItemRows in an
    ordered list, including a locked module's (`04` B12 lists them). A locked module says its
    reasons in one line, through the same phrases and unit list as the rail (B10a), with the date
    and time in the user's zone as B10a gives them ("Opens on Mar 3, 2026, 10:00 AM EST").
  - ItemRow's title is the link to the item view, named by the title alone, so no two links on a
    course home read the same; the kind (in words), requirement, state and due label are its
    description. The link's hit area covers the row, so the target is the row's height, at least
    44 px under a coarse pointer, and hover and keyboard focus mark the whole row. Pressed draws
    as hover. A locked item stays a link, as on the rail (B10a), and shows "Locked" without its
    reasons: the item view and the rail say them. A header item (`03` B8) is a heading one level
    below its module's, with no icon, state or link.
  - Kind icons (Phosphor): page FileText, file Paperclip, external link ArrowSquareOut, video
    Video, quiz ListChecks, activity PuzzlePiece. None is a circle, so none reads as a learning
    state's icon.
  - CourseHeader's title is the page's `h1` by default, with the code and, when the caller gives
    it, the status ("Draft", "Published", "Archived") on the line below; no eyebrow label. The
    caller gives the status where `Course.permissions` holds `course.view_all`. The cover is
    decorative (`alt=""`), as the title beside it names the course, and is left out when it fails
    to load. On the course home the header carries the course progress bar, so the page leaves
    NextUp's `progress` out there (I11) and draws one bar.
- **B10.** RequirementLabel says a requirement in words ("Mark as done", "Watch 80%"); DueBadge
  shows "Due in 3 days", "Due today" or "3 days overdue" in the user's time zone with the overdue
  state when past; NextUp shows Next up (`10` B2) with the one primary "Continue" action, or the
  waiting and completed states.
- **B10a.** Coordinator decisions for B8 and B10 (overnight, 2026-11-17), pending owner review:
  - The rail takes lock reasons with their module and item ids resolved by the caller to the
    titles the learner already sees (`04` B14), and says each as a phrase: "Unlocks after Ground
    control", "Unlocks after Ground and Delivery", "Unlocks when its module unlocks", "Opens on
    Mar 3, 2026, 10:00 AM EST" (the user's zone, with its short name, as Q2 does for date
    fields). A locked node needs at least one reason.
  - A locked node stays a link to its item view, which explains the lock (`04` B13 answers 200
    there). Its reasons, joined as the locale lists units (`Intl.ListFormat`, `type: "unit"`),
    are its accessible description at all times, and a Tooltip shows them on hover and keyboard
    focus; a touch screen shows no tooltip, so a tap opens the item view.
  - A node's name is its title and its state through one message ("Radio basics, Complete");
    an overdue node's state is its due label ("Clearance delivery, 3 days overdue"), so the name
    holds all the text the link draws.
  - The segment after an item fills when that item is complete; the last item has none. A
    transition fills it over the `motion.slow` token (320 ms) with the `ease.out-expo` token.
  - Up and Down move between nodes, Home and End jump to the ends; every node stays a tab stop,
    as links in a list do.
  - The "Module" button and the 768 px switch to a bottom sheet are the item view's (B16, I11);
    the rail works inside a bottom `Sheet`.
  - DueBadge counts calendar days in the user's time zone, from a `now` the caller passes from its
    clock. It is overdue only after the due instant; 1 day reads "Due tomorrow", and a due time
    already past today reads "Overdue".
  - RequirementLabel covers the requirement kinds the API accepts (`04` B3): `view` reads "View",
    `mark_done` "Mark as done", `watch` "Watch 80%". `submit`, `score` and `mastery` come with
    M2 and M3.
  - Coordinator decision (2026-11-17), on the plan's "current (ring with a dot)": the current
    node is ringed in the brand color in every state, with a dot while available. An overdue
    current node keeps its overdue ring and gets a second, outer brand ring, so it is marked as
    current by more than the title's weight.
  - Coordinator decision (2026-11-17), on the plan's "the whole rail fills once": once the course
    is complete every segment is drawn filled, including those after items a `one` completion
    rule left incomplete. When `courseComplete` turns true while the rail is shown, every segment
    fills from empty, one after another, all within the `motion.slow` token, each with the
    `ease.out-expo` token; a rail first drawn complete does not animate. Both motions are CSS, so
    `base.css`'s reduced-motion rule makes them instant.
- **B11.** Callout (`info`, `tip`, `warning`, `danger`) is an icon on a tinted surface, never a
  colored side stripe (`ui.md` pitfalls), used by PageReader and announcements. Announcement shows
  an announcement's title, author, date and body HTML.
- **B11a.** Coordinator decisions for B10 (NextUp) and B11 (overnight, 2026-11-17), pending owner
  review:
  - NextUp takes one of three answers from the server (`10` B1): the next item (its title, its
    module's title, the deep link and whether it is started), waiting until a date
    (`waiting_until`), or complete (`completed_at`). Spec 10 gives `waiting_until` only when a
    date is what blocks the learner, so there is no waiting state without a date; what a page
    shows when the server answers none of the three is spec 10 I2's.
  - The next item shows its module and item titles, a StatusChip ("Available", or "In progress"
    once started), the course progress bar when the caller passes `CourseProgress`, and the
    course title only where the page does not already say it (the dashboard). The requirement is
    not repeated; the item view shows it. Waiting reads as the `unlock_at` lock reason does (B10a,
    "Opens on Mar 3, 2026, 10:00 AM EST"); complete reads "Completed Mar 3, 2026". Both are in
    the user's time zone and have no action.
  - Coordinator decision (2026-11-17), on the dashboard's several "Continue" links (`10` B10):
    the link shows "Continue" and is named "Continue: Taxi instructions", so each link says which
    item it opens and the name starts with the visible label (axe's label-in-name check). The
    item's title is in the name, so it is not given again as a description.
  - Callout's kinds take the status tints, although the plan keeps status colors for feedback and
    grades: the plan names the kinds after the statuses and asks for a tinted surface. `info`,
    `warning` and `danger` take their own; `tip` takes `success-soft`, not `brand-soft`, since a
    brand can be any hue (vZDC's is red, and its tint matched `danger-soft`). The icon and the
    optional title use `<status>-fg`, the pair the generator holds at 4.5:1 on the tint; the bare
    `warning` color is 2.80:1 on `warning-soft` in light, under the 3:1 for UI. The text is
    `fg-default`, at least 10.2:1 on every tint in both example brands' themes. A transparent
    border draws the edge in forced colors. The icons are Info, Lightbulb, WarningCircle and
    Prohibit: not Warning, which is the overdue state's (B3), and not WarningOctagon, which
    looks like WarningCircle at 20 px. The callout is a `note` whose icon is named by its kind
    ("Note", "Tip", "Warning", "Danger").
  - Callout is a React component for content the app builds (announcements, screens). How the
    callouts in a page's server-rendered HTML take the same look is PageReader's (`07` I4).
  - Announcement reads "Posted by Pat Lee on Mar 3, 2026" (the date in the user's time zone) and
    does not show `edited_at`, which B11 does not list. Its body is the server's HTML, inserted
    as given through Prose's `html` property, so base.css's element styles reach it.
  - Coordinator decision (2026-11-17), while no server sanitizer exists yet: Prose's and
    Announcement's `html` is `ServerHtml`, a branded string that only `serverHtml()` makes, so a
    plain string fails `tsc`. `serverHtml()` checks nothing; it is called only where the
    api-client's response for a server-rendered field (`Announcement.html`, a page's render) is
    parsed, once those fields exist.
- **B12.** AssignmentList lists the caller's assignments (`13` B18) with DueBadge and StatusChip,
  overdue first; DownloadButton (`15` B8) shows a course's download state (not downloaded,
  downloading with progress, downloaded with date and size, update available) and its actions.

### Authoring

- **B13.** ModuleBoard is the course structure editor: modules as sections, items as rows, with
  drag-and-drop reordering of modules and of items within and between modules (React Aria's
  drag-and-drop on `GridList`, Q3) and the same moves from the keyboard (a "Move" menu with up,
  down and "move to module"). Each move calls `03`'s order or move route and reloads on 409
  `order.stale`. It shows publish state, requirements and unlock rules per row, and opens item
  editing in a side sheet (not a modal, `ui.md` pitfalls).
- **B14.** BrandPreview shows a sample screen (top bar, a course row, a button, a callout, text) in
  a draft brand in light, dark and high contrast side by side, with the contrast report of the
  generator (`m0/12`). The branding screen gains it and ColorField, replacing M0's minimal screen
  (`m0/12` Q14 (b)).

### Screens

- **B15.** Course list (`/courses`): the caller's courses (`02` B11) as a DataList with filters for
  status and account for staff; a "New course" action for holders of `course.create`.
- **B16.** Course home (`/c/:course`): CourseHeader, NextUp, then the modules as ModuleRows with
  their items; the home page instead when `settings.home` is a page (`07` B19). Item view
  (`/c/:course/m/:module/i/:item`, `03` B16): the item's content view (PageReader, file preview,
  external link card, VideoPlayer) with the ProgressRail beside it at 1024 px and wider and behind
  a "Module" button under 768 px, plus Previous and Next.
- **B17.** Course settings (`/c/:course/settings`): details, dates, home page, cover, status
  changes with confirmation, copy (`02` B12), move to another account (`02` B9), validity (`14` B3).
  Each action shows only when `Course.permissions` holds its key; the server decides regardless.
- **B18.** Roster (`/c/:course/people`): enrollments (`02` B20) with role, status and actions to
  enroll, conclude and remove (remove confirms that it deletes the learner's progress, `05` B15),
  Observer enrollments with their observed learners (`05` B17), and a progress table (`05` B14:
  course and module progress per learner, DataTable) (Q4).
- **B19.** Course authoring (`/c/:course/edit`): ModuleBoard with item creation for each available
  kind, page creation into the editor (`07`), file and video pickers, requirement and unlock rule
  editing (`04`).
- **B20.** Files (`/c/:course/files`): the folder Tree and file list (`06` B12) with FileDrop
  upload, rename, move and delete, for holders of `course.view_all`.
- **B20a.** Coordinator decisions for B20 (overnight, 2026-11-17), pending owner review:
  - The route takes the course's id, as `03` B16's routes do, and the folder on screen is the
    `folder` search parameter (`/c/<id>/files?folder=/handouts`), so each folder has an address.
    The files area is a course's page, not a destination of its own, so the shell's navigation
    gets no entry; the course pages of I11 link to it, and until they exist it is reached by its
    address.
  - The page reads the course (`02` B10) for its title, code, status and `permissions`. Upload
    and the row actions show only when `permissions` holds `course.content_edit` and the course
    is not archived; an archived course says its files cannot be changed. A 403 on the listing
    says in words that the files area is for staff and that learners reach files through items
    (`06` Q5), with no "Try again"; a 404 `course.not_found` says the course could not be found.
  - The Tree sits beside the list at 768 px and wider and loads a folder's sub-folders when the
    folder is opened. Under 768 px the Tree is hidden; the breadcrumbs, shown at every width, and
    the folder rows of the list lead between folders.
  - The list has three columns, so it fits 375 px: the name, with the type the server read and
    the last change on a line under it; the size; and an actions menu (Rename, Move and Delete
    for a file, Rename for a folder). Rename and move take one field in a dialog, as the account
    page's passkey rename does; delete asks first. Move takes a typed folder path, and moving a
    file into a folder that does not exist makes it, since a folder exists while it holds a file
    (`06` Q4). A folder rename changes the folder's own name and keeps it in its parent.
  - Several files can be chosen at once. They upload one after another into the folder on
    screen; the first refusal stops the rest and is shown in the drop zone with the number of
    files not sent. The page sets no size limit of its own, since the API does not publish
    `max_file_bytes`; the server's 413 is shown in words. The upload goes out through
    `XMLHttpRequest`, the only browser API that reports a request body's progress, as the API
    client's per-request `fetch`, so the request is still built from the OpenAPI document.
  - Opening or downloading a file waits for `06` I3's signed URLs, which are not built yet.
- **B21.** Assignments (`/c/:course/assignments`): the list (`13`), a creation sheet with target,
  assignees, due rule, reminders and recurrence (`14`), and each assignment's people with their
  status and timeline (`13` B16).

## API

None added. The components and screens use the routes of specs 02 to 15 through
`packages/api-client`.

## Data

None.

## Permissions

None added. Screens show actions by the keys in `Course.permissions` (`02` B10) and the caller's
grants in `/me`; every route checks on the server.

## Test cases

- **T1** (B2, every component). Storybook stories for every state named in `ui.md`'s definition of
  done, screenshots in every theme and brand and both densities at 1280 and 375 px, axe with zero
  violations, in `just ci`.
- **T2** (B3). StatusChip, ProgressRail nodes, ModuleRow and ItemRow render each learning state
  with its icon and an accessible label; a test with color removed (forced colors) still tells the
  states apart.
- **T3** (B5). A DatePicker set to 2026-03-08 09:00 in `America/New_York` gives the right UTC
  instant across the DST change and shows "EDT" or "EST" as appropriate.
- **T4** (B8). ProgressRail: arrow keys move focus across nodes, locked nodes announce their reason,
  the fill animation runs over 320 ms and not at all under `prefers-reduced-motion`.
- **T5** (B13). ModuleBoard: a drag and a keyboard "Move" each send one order request; a 409
  reloads and keeps the user's place; screen readers hear the move announced.
- **T6** (B14). BrandPreview shows the generator's contrast failures for a brand the generator
  refuses, and the branding screen cannot save it.
- **T7** (B15 to B21). Playwright journeys against the Compose stack: an organization admin creates
  a course, adds a module with a page and a file, sets a requirement, publishes, enrolls a learner,
  assigns the course with a due date; the learner opens the course home, follows Next up, reads,
  marks done and sees the rail fill; the organization admin sees the progress in the roster.
- **T8** (B17, B18). Actions the caller lacks are absent (a Course designer sees no status
  change or roster actions), and the server's 403 is shown as an error, not a crash, when a stale
  page tries one.
- **T9** (B2). The iOS rows are recorded as experimental until spec 17 runs.

## Out of scope

- Assessment, Activities and Reporting components: M2 and M3.
- Certificate and ComplianceTable: listed for M1 in the inventory but used by features specified
  later; each is built with its feature (Q1): Certificate with the certificates spec,
  ComplianceTable with M4's compliance report.
- The App shell row (ServerPicker and the rest): M5.
- Managed hosting components (PlanBadge, UpgradePrompt, SenderDomainSetup ...): M6; spec 12's
  sender screen uses plain forms in M1.
- RosterHeatmap: Reporting, M2 to M3 (Q4).

## Open questions

Q2 to Q4 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q1 moved work out of the milestone the plan names, so it was the
owner's; the owner answered it on 2026-11-16.

- **Q1. Certificate and ComplianceTable in M1.** The inventory lists both in
  the M1 Learning row, but no M1 spec issues certificates (the plan's completion PDF) or builds the
  compliance report (M4). Building them now makes components nothing uses (`AGENTS.md`
  non-negotiable 4); building them later moves them out of the milestone the plan names, which is
  the owner's call. Options: (a) build each with its feature: Certificate with the certificates
  spec, ComplianceTable with M4's compliance report; (b) build both in M1 as stories only; (c) add
  a certificates spec to M1. Recommendation: (a).
  Owner decision (2026-11-16): (a), each component is built with its feature; neither is in this
  spec's issues.
- **Q2. Time zones in date fields.** Options: (a) values carry their zone, shown beside the value,
  defaulting to the user's zone; (b) the organization's zone always; (c) the browser's zone.
  Recommendation: (a): due dates and unlock dates are instants, and a learner abroad must see when
  that is for them.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. How ModuleBoard reorders.** Options: (a) React Aria's drag-and-drop on `GridList`, with a
  keyboard "Move" menu as well; (b) a drag-and-drop library (dnd-kit); (c) keyboard and buttons
  only. Recommendation: (a): one primitive layer (`ui.md`), accessible drag-and-drop built in.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. The roster's progress view in M1.** The plan's "instructor roster heat map" is
  RosterHeatmap in the Reporting row (M2 to M3). Options: (a) a DataTable of course and module
  progress per learner in M1, the heat map with Reporting; (b) build RosterHeatmap now.
  Recommendation: (a), which shows the same data without pulling a Reporting component forward.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

In dependency order, in stages as `../m0/13-base-ui-components.md` grouped its issues. Each is one
PR with stories, tests and screenshots.

**Stage 1: Forms and Data (M1 part).**

1. **I1. Date and time fields.** DatePicker, DateRangePicker, TimeField with zones. Covers B5; T3.
   Depends on: `m0/13` I10 and I11 (M0 fields, merged).
2. **I2. TagField, Slider and ColorField.** Covers B6 (these three). Depends on: I1's patterns.
3. **I3. FileDrop.** Covers B6 (FileDrop). Depends on: none beyond M0.
4. **I4. Tree and StatusChip.** Covers B3, B7; T2 (StatusChip). Depends on: none beyond M0.

**Stage 2: Learning.**

5. **I5. ProgressRail, RequirementLabel and DueBadge.** Covers B8, B10 (labels and badges); T2, T4.
   Depends on: I4.
6. **I6. ModuleRow, ItemRow and CourseHeader.** Covers B9. Depends on: I5.
7. **I7. NextUp, Callout and Announcement.** Covers B10 (NextUp), B11. Depends on: I4.
8. **I8. AssignmentList and DownloadButton.** Covers B12. Depends on: I5.

**Stage 3: Authoring.**

9. **I9. ModuleBoard.** Covers B13; T5. Depends on: I6.
10. **I10. BrandPreview and the branding screen.** Covers B14; T6. Depends on: I2.

**Stage 4: Screens** (each depends on its API spec's issues as listed).

11. **I11. Course list, course home and item view.** Covers B15, B16; T7 (learner part). Depends
    on: I6, I7, `03-modules-and-items.md` I3, `05-progress-and-events.md` I1,
    `07-pages-and-the-editor.md` I4, `06-files.md` I3.
12. **I12. Course settings.** Covers B17; T8 (settings). Depends on: I11,
    `02-courses-and-enrollments.md` I3, I1 of this spec.
13. **I13. Roster and progress table.** Covers B18; T8 (roster). Depends on: I11,
    `05-progress-and-events.md` I3.
14. **I14. Course authoring.** Covers B19; T7 (authoring part). Depends on: I9,
    `04-unlock-and-completion-rules.md` I1, `07-pages-and-the-editor.md` I3.
15. **I15. Files area.** Covers B20. Depends on: I3, I4, `06-files.md` I2.
16. **I16. Assignment screens.** Covers B21; T7 (assignment part). Depends on: I8, I1,
    `13-assignments-due-dates-reminders-and-escalation.md` I4,
    `14-recurrence-and-expiring-completions.md` I3.
