# Learner dashboard and Next up

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Goals ("what's next" always visible), Progress and grades
  (Progress views: a learner dashboard across courses), API surface (`GET /api/v1/me`,
  `/me/dashboard`: current user, permissions, courses, next-up items), Hosting and operations
  (performance budget: page load p95 at most 150 ms); `docs/plan/ui.md` -> Principles (one
  primary action per view), Component inventory (Learning: NextUp), Key layouts (course home's
  Next up), Pitfalls (hero-metric dashboards: the dashboard leads with Next up and deadlines)
- **Depends on:** `02-courses-and-enrollments.md` (enrollments, course list),
  `04-unlock-and-completion-rules.md` (the evaluator), `05-progress-and-events.md` (progress),
  `../m0/11-app-shell.md` (Home, navigation)
- **Review:** standard

## Summary

The dashboard is the first thing a learner sees: for every course they take, the next thing to do
and how far along they are, with one "Continue" action per course and the most urgent course
first. It replaces M0's empty Home. The server computes Next up from the same rules that lock
content, so the dashboard never points at something the learner cannot open. Deadlines join it
when spec 13 adds due dates, and recent announcements when spec 11 adds them.

## Behavior

### Next up

- **B1.** For a learner (`course.participate`, `05` B1) in a published course, Next up is the
  first item in course order (module position, then item position) that is published, unlocked
  for them (`04` B7), has a requirement, and is not complete (Q1). When every requirement they
  can reach is complete but the course is not (later modules are locked by date), Next up is
  `null` with `waiting_until`: the earliest `unlock_at` among the locked modules, when that is
  what blocks them. When the course is complete, Next up is `null` and the course shows its
  completion date.
- **B2.** Next up carries the item's `id`, `kind`, `title`, `module_id`, the module's `title`, the
  requirement, the learner's progress on it (`started` or none), and the deep link
  `/c/<course>/m/<module>/i/<item>` (`03` B16).
- **B3.** `stepclimb_core::next_up::for_course` is a pure function over the course's rules, the
  learner's progress and the time, next to `04`'s evaluator, so the course home's Next up (`03`'s
  outline response gains a `next_up` member) and the dashboard use the same code.

### The dashboard

- **B4.** `GET /api/v1/me/dashboard` answers, for the caller, `courses`: one entry per course in
  which they hold an active enrollment with `course.participate` and which is published or
  archived (Q2), each with the course's `id`, `title`, `code`, `status`, `cover_url` (`06` B19),
  the learner's course progress (`05` B7) and Next up. Spec 13 adds `deadlines` and per-course
  due information; spec 11 adds `announcements`.
- **B5.** Order (Q3): courses with Next up first, those with progress started before those not
  started, then by the time of the learner's last progress in the course (most recent first),
  then by title; then courses waiting on a date; then completed courses; archived courses last.
  Spec 13 puts overdue and due-soon courses first when it lands.
- **B6.** At most 50 courses are returned (the most relevant by B5); `has_more` tells the web app
  to link the full list (`GET /api/v1/courses`, `02` B11).
- **B7.** The route needs only authentication (`AuthOnly`, `09` B21): every member reads their own
  dashboard, and the answer is built only from the caller's own enrollments and progress through
  `OrgScope`.
- **B8.** The dashboard loads with a bounded number of queries whatever the number of courses:
  one for the enrollments and course rows, one for the rule data of those courses, one for the
  learner's item progress in them, one for course progress. It is within the plan's page budget
  (p95 at most 150 ms at 100 concurrent users on 1 vCPU) for a learner with 50 courses of 200
  items each; I1 measures it with a seeded database and records the number in the PR (the k6
  check is a release task).
- **B9.** Staff who take no course see an empty `courses` list; the web app then shows the
  staff empty state (B11).

### The page

- **B10.** Home (`/`, `11` Q2) becomes the dashboard: a Next up panel for the first course in B5's
  order with the one primary "Continue" button (brand color), then the list of the other courses,
  each a row with its title, a progress bar, the next item's title and a secondary "Continue"
  link. Under 768 px the same order stacks in one column. No statistics tiles (plan pitfall).
- **B11.** Empty states: a learner with no course sees "You have no {courses} yet." (M0's text,
  with the organization's terminology); a course waiting on a date shows "Opens on <date>" in the
  user's time zone; a completed course shows "Completed <date>".
- **B12.** The dashboard refetches when the window regains focus and after any progress the
  learner records in the session, so Next up is current after they finish an item.

## API

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/me/dashboard` | | 200 `Dashboard` | 401 `auth.unauthenticated` | authenticated (`AuthOnly`) |

Changed: `GET /api/v1/courses/{course_id}/modules` (`03`) gains `next_up: NextUp | null` for a
learner. Schemas: `Dashboard { courses: DashboardCourse[], has_more }`; `DashboardCourse {
course: { id, title, code, status, cover_url }, progress: CourseProgress | null, next_up: NextUp |
null, waiting_until: string | null, completed_at: string | null }`; `NextUp { item: { id, kind,
title, requirement }, module: { id, title }, progress: ItemProgress | null, link }`.

## Data

No migration. The dashboard reads `enrollments`, `courses`, `modules`, `module_items`,
`module_prerequisites`, `item_progress` and `course_progress`. One index is added if I1's
measurement needs it (`item_progress (org_id, user_id, course_id)`), in a migration named then.
Repository function: `dashboard::load(scope, user_id, now)`.

## Permissions

No new catalog entry; the route is `AuthOnly`. Entitlements: none.

## Test cases

- **T1** (B1). In a course [M1: page(view), link(none), page(mark_done)] with nothing done, Next up
  is the first page; after it is viewed, the third item (the link has no requirement); after all,
  `null` with the course complete.
- **T2** (B1). With M2 requiring M1 and M1 complete, Next up is M2's first requirement; with M2's
  `unlock_at` tomorrow, Next up is `null` and `waiting_until` is tomorrow.
- **T3** (B1). An unpublished item and an item in a sequential module behind an incomplete one are
  never Next up.
- **T4** (B3). The course outline's `next_up` and the dashboard's agree for 1,000 generated
  courses and progress sets (property test).
- **T5** (B4, B5). With five courses (one draft, one archived, one completed, one waiting, two
  active, one of them started yesterday and one never), the dashboard lists the started active
  course first, then the unstarted, then waiting, completed, archived, and leaves out the draft.
- **T6** (B4). Concluded enrollments, and enrollments as Instructor only, add no course.
- **T7** (B6). With 60 courses, 50 are returned with `has_more: true`.
- **T8** (B7). A user of org B sees only org B's courses; two learners of one course see their own
  progress only.
- **T9** (B8). The dashboard for a learner with 50 courses runs at most 4 queries (counted with a
  query-counting test hook).
- **T10** (B10, B11). Playwright: a learner signs in and sees Next up with one primary button;
  "Continue" opens the item; marking it done and returning shows the next item. Screenshots in the
  default and vZDC themes, light and dark, at 1280 and 375 px, with and without courses; axe finds
  no violation.
- **T11** (route). The `AuthOnly` cases: anonymous 401, any member 200; cross-organization: a
  member of org A gets org A's dashboard while org B has courses and enrollments.

## Out of scope

- Deadlines, due badges and overdue ordering: spec 13.
- Recent announcements on the dashboard: spec 11.
- The NextUp, ProgressRail and other Learning components themselves: spec 16 I6 and I7 (this
  spec's page uses them; I2 depends on them).
- Calendar and Grades navigation entries: later specs.
- Instructors' and managers' dashboards (roster heat map, team view): later specs and M4.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. What Next up points at.** Options: (a) the first unlocked, incomplete item with a
  requirement, in course order; (b) the first unlocked item the learner has not opened, with or
  without a requirement; (c) the item after the last one the learner touched. Recommendation:
  (a). It always points at work that moves completion, and it agrees with the locks; items without
  requirements stay reachable from the course outline.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Which courses the dashboard lists.** Options: (a) published and archived courses with an
  active learner enrollment, archived last; (b) published only; (c) every enrollment, staff roles
  included. Recommendation: (a). Learners keep a path to finished history (`02` Q4), and staff
  work belongs to a staff view.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Course order.** The plan says the dashboard "leads with Next up and deadlines". Before
  deadlines exist: (a) B5's order (active and started first, by recent activity); (b) by title;
  (c) by enrollment date. Recommendation: (a), with spec 13 putting due courses first.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Next up and the dashboard API.** `next_up::for_course`, `next_up` in the outline,
   `dashboard::load` with the bounded queries and the measurement, the route. Covers B1 to B9;
   T1 to T9, T11. Depends on: `05-progress-and-events.md` I1, `04-unlock-and-completion-rules.md`
   I3, `06-files.md` I4 (cover URLs).
2. **I2. The dashboard page.** Home as the dashboard with the NextUp panel, the course list, empty
   states and refetching. Covers B10 to B12; T10. Depends on: I1, `16-learning-ui-components.md`
   I6 (course rows) and I7 (NextUp).
