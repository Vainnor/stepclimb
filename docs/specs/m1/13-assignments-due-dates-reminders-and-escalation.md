# Assignments, due dates, reminders and escalation

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Goals (training people can prove: due dates, reminders and
  escalation), Tenancy and structure (reporting lines: each user can have a manager, set in the UI
  or through sync), Progress and grades (Due dates, recurrence and expiry: "Assignments: a course,
  module or item assigned to people or groups, with a due date that is either fixed or relative
  ('within 14 days of joining') ... Reminders and escalation: rules per assignment, for example
  'remind 7 and 1 days before, then on the due date; if overdue 3 days, email their manager'.
  Sent by the jobs queue, and visible on a timeline."), Data model (`assignments`,
  `reminders_sent`), Email and notifications; `docs/plan/ui.md` -> Component inventory (Learning:
  DueBadge, AssignmentList), Principles (overdue has its own state)
- **Depends on:** `01-job-queue-and-outbox.md` (periodic jobs, outbox subscribers),
  `02-courses-and-enrollments.md` (courses, enrollments), `05-progress-and-events.md` (completion
  events), `10-learner-dashboard-and-next-up.md` (deadlines on the dashboard),
  `11-announcements-and-in-app-notifications.md` (`notify`, preferences), `12-email.md` (`queue`,
  templates, the email log), `../m0/03-users-and-identities.md` (`users.manager_id`, B8),
  `../m0/07-admission.md` I7 (groups, for group assignees)
- **Review:** maintainer line-by-line (a new permission, and what an escalation lets a manager
  see)

## Summary

An assignment says who must finish which course, module or item, and by when: a fixed date, or a
number of days after the person was assigned. Each assignee gets their own due date and status
(not started, in progress, completed, overdue). Reminder rules send the assignee a reminder before
and on the due date and, once overdue, escalate to their manager (`users.manager_id`). A periodic
job sends them through the email and in-app channels, each step is recorded, and a timeline shows
everything that happened for one person and one assignment. Recurrence and expiry are spec 14.

## Behavior

### Assignments

- **B1.** An assignment has a target (`course`, `module` or `item` and its id), an assignee
  (`user` or `group` and its id), a due rule (B4), a reminder rule (B10), the author and
  `ended_at`. The target's course is the assignment's course. Creating, changing and ending one
  needs `perm::AssignmentManage` at that course; assigning a user who is not enrolled also needs
  `perm::EnrollmentManage` there (B3).
- **B2.** Each person an assignment reaches gets an `assignment_users` row: `assigned_at`,
  `due_at`, `completed_at`, `status`, and spec 14's `cycle` (1 here). For a `user` assignee that is
  one row, made when the assignment is created. For a `group` assignee, a row per active member
  when the assignment is created, and a row for each member who joins the group later, made by the
  `assignments.sync_groups` job (B6) with `assigned_at` = the time the job sees them; a member who
  leaves keeps their row, marked `removed_at`, and gets no more reminders. Group assignees wait for
  groups to exist (`../m0/07-admission.md` I7, Q4).
- **B3.** An assignee without an active enrollment holding `course.participate` in the course is
  enrolled as Student (source `local`) in the same transaction that makes their row (Q2); the
  enrollment is not removed when the assignment ends. A template course or an archived course
  refuses assignments (409 `course.template`, `course.archived`).
- **B4.** A due rule is either `{ "fixed": <RFC 3339 time> }` or `{ "relative_days": 1..3650 }`.
  A relative due date is `assigned_at` plus that many calendar days in the organization's default
  time zone (`../m0/03-users-and-identities.md`, `default_tz`), at 23:59:59 local time of the last
  day (Q1). An assignment may have no due rule; it then has no due date, no reminders and is never
  overdue.
- **B5.** Changing the due rule recomputes `due_at` for every row not completed and reschedules
  its reminders; reminders already sent stay sent and are not sent again for the same step.
  Ending an assignment (`ended_at`) stops all its reminders and hides it from learners; its rows
  and timeline stay (Q3).
- **B6.** The periodic job `assignments.sync_groups` (every 15 minutes, `01` B17) adds rows for
  new members of group assignees and marks members who left, for assignments not ended.

### Status

- **B7.** A row's status, computed when read from the stored times and the request clock:
  `completed` when `completed_at` is set; else `overdue` when `due_at` has passed; else
  `in_progress` when the learner has any progress on the target (`05` B3); else `not_started`.
- **B8.** `completed_at` is set from the target's completion: the course's (`course_progress`), the
  module's (`module_progress`) or the item's (`item_progress`) `completed_at`, when it falls on or
  after `assigned_at` of the row's cycle (spec 14) or, in cycle 1, at any time, so an assignee who
  already finished the course is complete at once (Q5). The subscriber `assignments.on_completed`
  to `item.completed`, `module.completed` and `course.completed` (`05` B10) sets it, and creating a
  row checks it directly.
- **B9.** A row completed after its `due_at` keeps `completed_at` and shows "completed late" in
  the timeline; the status is `completed`.

### Reminders and escalation

- **B10.** A reminder rule is a list of up to 10 steps, each `{ "offset_days": -365..365, "to":
  "assignee" | "manager" }`: a negative offset is before the due date, 0 on it, a positive one
  after it. The plan's example is `[{-7, assignee}, {-1, assignee}, {0, assignee}, {3, manager}]`.
  A rule needs a due rule (422 `assignment.reminders_need_due`). Each organization has a default
  rule for new assignments (`OrgSettings` key `assignments.default_reminders`, default the plan's
  example), which an assignment may replace or clear.
- **B11.** A step's send time for a row is 09:00 in the assignee's time zone (`users.tz`) on the
  day `offset_days` calendar days from the local date of `due_at` (Q6); an offset-0 step whose
  09:00 falls after `due_at` (a due time before 09:00) is sent at `assigned_at` or 24 hours before
  `due_at`, whichever is later. Steps whose send time is before `assigned_at` are skipped for that
  row (a person assigned 3 days before the due date gets no "7 days before" reminder).
- **B12.** The periodic job `assignments.reminders` (every 5 minutes) finds rows with a step due
  (stored as `next_step_at`), not completed, not removed, whose assignment is not ended, and for
  each due step writes a `reminders_sent` row (unique per row and step, so a rerun sends nothing
  twice) and then:
  - `to: assignee`: `notify` (`11` B8) with kind `due_reminder` and `email::queue` (`12` B12) with
    kind `due_reminder`, each following the assignee's preferences;
  - `to: manager`: when the assignee has a `manager_id` whose user is active, `notify` and
    `email::queue` with kind `escalation` to the manager; otherwise the `reminders_sent` row records
    `skipped: no_manager`.
  It then sets `next_step_at` to the next step's time. Steps whose time passed more than 24 hours
  ago when the job finds them (the server was down) are recorded as `missed` and not sent, so a
  restart does not flood people with stale reminders.
- **B13.** An escalation names the assignee, the target, the due date and how long it is overdue,
  in the message itself. The escalation is the message only (Q9): the manager gets no read access
  to anything of the assignee's, and the message links to nothing behind a login. The Manager
  role's scope over reports is M4.
- **B14.** Notification and email kinds added: `due_reminder` (not required; defaults in-app on,
  email on) and `escalation` (not required; defaults in-app on, email on), with templates (`12` B8)
  whose variables are `assignee_name`, `target_title`, `course_title`, `due_at` (in the recipient's
  time zone), `days_overdue` and `link` (the target's deep link, for `due_reminder` only; an
  escalation carries no link, B13).

### Managers

- **B15.** `users.manager_id` exists (`../m0/03-users-and-identities.md` B8) and nothing writes it
  in M0. This spec adds `manager_id` to `PATCH /api/v1/users/{user_id}` (`perm::UserManage`,
  organization) (Q7): the manager is an active user of the organization, not the user themself,
  and not someone whose management chain reaches the user (409 `user.manager_cycle`); `null`
  clears it. A user whose `source` is not `local` keeps an editable manager in M1, since no sync
  writes it before M4. The change writes `user.set_manager` (`m0/09` B22).

  Coordinator decisions (overnight, 2026-11-16), for I2, pending owner review:
  - `User` gains `manager_id` (uuid or `null`), so `GET /api/v1/users` and the PATCH answer show
    the manager that was set. Reading it needs `user.view` at organization scope, as every other
    member of `User`.
  - The user themself is refused with 409 `user.manager_cycle`: managing yourself is the
    shortest cycle. A suspended manager is refused with 409 `user.manager_inactive`.
  - Changing `manager_id` falls under the escalation rule of `email` and `status` (403
    `permission.escalation`, `../m0/03-users-and-identities.md`): the caller cannot redraw the
    reporting line of a user who holds a permission the caller lacks. An API key or app token
    cannot change its own user's manager (403 `auth.session_required`, `m0/08` B44).
  - A `user.manage` holder cannot change their own `manager_id` (Q10), so nobody picks who
    receives their own escalations: a `PATCH /api/v1/users/{user_id}` on the caller's own user
    that names `manager_id`, setting it, clearing it with `null` or naming the current value,
    answers 403 `user.manager_self` with any credential and writes nothing, members other than
    `manager_id` included. An API key or app token still gets `auth.session_required` first.
    Another user's manager is set as above. Built in I6, after I2 merged.
  - `users::set_manager` locks the organization row `FOR NO KEY UPDATE`, as a suspension does,
    so two concurrent changes cannot close a cycle between them; the lock also keeps the new
    manager from being suspended between the check and the write. The PATCH changes the manager
    before the status, so it never holds the user row while it waits for the organization row.
  - Being someone's manager grants no permission and no read access (B13, Q9 (b)); no
    permission check reads `manager_id`.

### Timeline

- **B16.** The timeline of one row lists, oldest first: assigned (with the due date then), due date
  changes, each reminder or escalation step (sent, skipped and why, missed) with the email's status
  from `email_messages` (`12` B12) when one was queued, completion (on time or late) and removal
  from a group. It is built from `assignment_users`, `assignment_events` and `reminders_sent`.
- **B17.** Readers: the assignee (their own), and holders of `perm::AssignmentManage` or
  `perm::ProgressView` at the course. The assignee's manager is not a reader (Q9).

### Learners and the dashboard

- **B18.** `GET /api/v1/me/assignments` lists the caller's rows of assignments not ended, with the
  target, due date and status, ordered overdue first, then by due date, then without one.
- **B19.** `GET /api/v1/me/dashboard` (`10` B4) gains `deadlines` (the caller's rows due within 14
  days or overdue, at most 10) and each course entry its nearest `due_at` and status; B5 of `10`
  then orders overdue courses first, then those due within 7 days by due date, before its other
  rules (`10` Q3).
- **B20.** The course outline (`03` B13) marks items and modules that a row of the caller targets
  with their due date and status, for the DueBadge.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/assignments` | `?include_ended&cursor&limit` | 200 `Page<Assignment>` | 404 `course.not_found` | `perm::AssignmentManage` (course) |
| `POST /api/v1/courses/{course_id}/assignments` | `{ target, assignee, due?, reminders? }` | 201 `Assignment` | 404 (course, target, user, group), 403 `permission.denied` (enrollment needed, B3), 409 `course.template`/`course.archived`, 422 `request.invalid` (`assignment.reminders_need_due`, ...) | `perm::AssignmentManage` (course), and `perm::EnrollmentManage` when it enrolls |
| `GET /api/v1/assignments/{assignment_id}` | | 200 `Assignment` | 404 `assignment.not_found` | `perm::AssignmentManage` (its course) |
| `PATCH /api/v1/assignments/{assignment_id}` | `{ due?, reminders? }` | 200 `Assignment` | 404, 422 | `perm::AssignmentManage` |
| `POST /api/v1/assignments/{assignment_id}/end` | | 200 `Assignment` | 404 | `perm::AssignmentManage` |
| `GET /api/v1/assignments/{assignment_id}/users` | `?status&cursor&limit` | 200 `Page<AssignmentUser>` | 404 | `perm::AssignmentManage` or `perm::ProgressView` (its course) |
| `GET /api/v1/assignments/{assignment_id}/users/{user_id}/timeline` | | 200 `TimelineEntry[]` | 404 `assignment_user.not_found` | B17: the assignee, or `AssignmentManage`/`ProgressView` at its course |
| `GET /api/v1/me/assignments` | `?cursor&limit` | 200 `Page<MyAssignment>` | | authenticated |

Changed: `PATCH /api/v1/users/{user_id}` takes `manager_id` (B15, 409 `user.manager_cycle`, 409
`user.manager_inactive`, 403 `user.manager_self` on the caller's own user, 404 `user.not_found`
for the manager); `GET /api/v1/me/dashboard` gains `deadlines` (B19); the outline
gains due information (B20); `OrgSettings` gains `assignments.default_reminders`. Schemas:
`Assignment { id, course_id, target: { type, id, title }, assignee: { type, id, name }, due:
DueRule | null, reminders: ReminderStep[], ended_at, counts: { not_started, in_progress,
completed, overdue }, created_at }`; `AssignmentUser { user: { id, display_name }, assigned_at,
due_at, status, completed_at, removed_at }`; `TimelineEntry { at, kind, data }`.

## Data

Migration `migrations/<version>_assignments.sql` (I1). The version is a placeholder, fixed when
the issue is implemented so it sorts after every migration on `next` and every open branch.

```sql
CREATE TABLE assignments (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL,
    course_id      uuid        NOT NULL,          -- the target's course: B1
    target_type    text        NOT NULL CHECK (target_type IN ('course', 'module', 'item')),
    target_id      uuid        NOT NULL,
    assignee_type  text        NOT NULL CHECK (assignee_type IN ('user', 'group')),
    assignee_id    uuid        NOT NULL,
    due_fixed      timestamptz,
    due_relative   integer     CHECK (due_relative BETWEEN 1 AND 3650),   -- days: B4
    recurrence     jsonb,                          -- spec 14
    reminder_rule  jsonb       NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(reminder_rule) = 'array'),
    cycle          integer     NOT NULL DEFAULT 1, -- the data model's column; per-person cycles: spec 14
    created_by     uuid        NOT NULL,
    ended_at       timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, created_by) REFERENCES users (org_id, id),
    CHECK (due_fixed IS NULL OR due_relative IS NULL)
);
CREATE INDEX assignments_by_course ON assignments (org_id, course_id) WHERE ended_at IS NULL;

CREATE TABLE assignment_users (                    -- not in the data model: B2
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL,
    assignment_id  uuid        NOT NULL,
    user_id        uuid        NOT NULL,
    cycle          integer     NOT NULL DEFAULT 1 CHECK (cycle >= 1),
    assigned_at    timestamptz NOT NULL,
    due_at         timestamptz,
    completed_at   timestamptz,
    removed_at     timestamptz,
    next_step_at   timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assignment_id, user_id, cycle),
    FOREIGN KEY (org_id, assignment_id) REFERENCES assignments (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
CREATE INDEX assignment_users_next_step ON assignment_users (next_step_at)
    WHERE next_step_at IS NOT NULL AND completed_at IS NULL AND removed_at IS NULL;
CREATE INDEX assignment_users_by_user ON assignment_users (org_id, user_id);

CREATE TABLE reminders_sent (
    org_id              uuid        NOT NULL,
    assignment_user_id  uuid        NOT NULL REFERENCES assignment_users (id),  -- the data model's (assignment_id, user_id), per cycle
    step                smallint    NOT NULL CHECK (step >= 0),
    kind                text        NOT NULL CHECK (kind IN ('reminder', 'escalation')),
    outcome             text        NOT NULL CHECK (outcome IN ('sent', 'skipped', 'missed')),
    reason              text,
    email_message_id    uuid,
    at                  timestamptz NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (assignment_user_id, step)
);

CREATE TABLE assignment_events (                   -- not in the data model: B16's other entries
    id                  uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL,
    assignment_user_id  uuid        NOT NULL REFERENCES assignment_users (id),
    kind                text        NOT NULL CHECK (kind IN ('assigned', 'due_changed', 'completed', 'removed')),
    data                jsonb       NOT NULL DEFAULT '{}'::jsonb,
    at                  timestamptz NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX assignment_events_by_row ON assignment_events (assignment_user_id, at);
```

`target_id` and `assignee_id` have no foreign key (they point at different tables by type, as
`role_assignments.scope_id`); the repository checks both in the organization and the course.
Deleting an item or module that an assignment targets is refused (409 `item.assigned`,
`module.assigned`, added to `03` B3 and B9's checks).

## Permissions

| Permission | Key | Grantable at | Built-in roles (M1) |
| --- | --- | --- | --- |
| `perm::AssignmentManage` | `assignment.manage` | organization, account, course | Organization admin (through `perm::ALL`), Account admin, Instructor (Q8) |

An Account admin holds it through an assignment at an account, so it covers every course in its
subtree; an Instructor holds it through an enrollment, so it covers that course only. Course
designer, Student, Observer and Grader do not hold it. Assigning someone not yet enrolled also
needs `enrollment.manage` (B3), which both roles hold.

Entitlements: none.

## Test cases

Tests use a `ManualClock`; reminder tests run the periodic job by hand.

- **T1** (B1, B3). A user holding `assignment.manage` and `enrollment.manage` assigns a course to an
  unenrolled user: a Student enrollment and one row appear; without `enrollment.manage` the same
  request answers 403 and creates nothing.
- **T2** (B4). `relative_days: 14` for a user assigned 2026-03-01 10:00 UTC in an organization in
  `America/New_York` gives a due time of 2026-03-15 23:59:59 local; a fixed due date is kept as
  given; across a DST change the local time stays 23:59:59.
- **T3** (B7, B8). A row is `not_started`, then `in_progress` after a view, then `overdue` after
  the due time, then `completed` (late) when the course completes; an assignee who completed the
  course before being assigned is `completed` at once.
- **T4** (B10, B11, B12). With the plan's rule and a due date 10 days out: reminders arrive at
  09:00 local 7 days and 1 day before and on the due date, and the manager's escalation 3 days
  after; running the job twice sends each once; a row completed after the first reminder gets no
  more.
- **T5** (B11). A user assigned 3 days before the due date gets no 7-days-before reminder.
- **T6** (B12). With no manager, the escalation step is recorded `skipped: no_manager`; with a
  suspended manager the same; after a 2-day outage, steps older than 24 h are recorded `missed`.
- **T7** (B12, B14). A user who turned `due_reminder` email off gets the in-app notification and
  an email row `skipped: preference_off`; the timeline shows both.
- **T8** (B5). Moving the due date later reschedules unsent steps and does not resend sent ones;
  ending the assignment stops all steps.
- **T9** (B2, B6). A group assignment reaches current members; a member added later is assigned by
  `assignments.sync_groups` with `assigned_at` at that run; one who leaves stops receiving
  reminders and keeps their timeline. (Runs once groups exist, `m0/07` I7.)
- **T10** (B15). Setting a manager forms no cycle (A manages B, B manages A answers 409
  `user.manager_cycle`); a manager from org B answers 404; the change writes one audit entry.
- **T10a** (B15, Q10). A session caller holding `user.manage` at the organization who sets,
  clears or repeats their own `manager_id` gets 403 `user.manager_self`, and their row and
  `audit_events` are unchanged; a patch of their own user naming `manager_id` and `display_name`
  changes neither. The same caller sets another user's manager (200). An API key of the caller
  gets 403 `auth.session_required`. Mutation: removing the self check turns this test red.
- **T11** (B16, B17). The timeline lists assigned, reminders with email status, escalation and
  late completion in order; the assignee reads it; the escalated manager gets 403 (Q9).
- **T11a** (B17, the timeline and `GET /assignments/{id}/users`). These decide in the handler, so
  the permission suite has one entry per granting permission (`assignment.manage`,
  `progress.view`) plus the assignee's own entry for the timeline, each with its own cases.
  Explicit tests: another Student of the course gets 403 `permission.denied` on both routes; a
  caller holding only `course.view` gets 403 `permission.denied`; the assignee gets 403 on
  another assignee's timeline.
- **T11b** (Permissions, Q8). The built-in lists hold `assignment.manage` for Account admin and
  Instructor and for no other built-in role but Organization admin. An Account admin of Tower
  assigns a Tower course to an unenrolled user (201) and gets 404 for an Enroute course; an
  Instructor assigns their course (201) and gets 403 on a course where they hold only a Student
  enrollment; a Course designer gets 403 on their course.
- **T12** (B18, B19). `GET /me/assignments` orders overdue first; the dashboard puts the overdue
  course first and lists deadlines.
- **T13** (every route). Four cases: anonymous 401; every permission but the route's at the course
  403; every permission at a sibling account 404; only the route's permission 2xx; `AuthOnly`
  cases for `/me/assignments`.
- **T14** (every route). Cross-organization: org A's admin with org B's course, assignment, user or
  group ids gets 404; no org B row changes; org A's reminder job never reads org B's rows (a job
  run with both organizations sends only to their own users).

## Out of scope

- Recurrence, cycles after the first, and expiring completions: spec 14.
- The compliance report and scheduled reports to managers: M4.
- The Manager role's scope over reports, and managers set through sync: M4.
- Enrollment rules creating assignments automatically: M4 with enrollment rules.
- Web Push reminders: M4.
- The AssignmentList and DueBadge components: spec 16; the assignment screens use them.

## Open questions

Q1 to Q7 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q8, Q9 and Q10 extended the plan's role definitions, so they were the
owner's; the owner answered them on 2026-11-16.

- **Q1. What a relative due date counts from and ends at.** The plan's example is "within 14 days
  of joining". Options: (a) from the moment the assignment reaches the person (creation, or joining
  the group), ending at 23:59:59 of the last day in the organization's time zone; (b) from the
  person's enrollment; (c) from the user's account creation. Recommendation: (a), which is
  "joining" for both direct and group assignments and gives people the whole last day.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Assigning someone not enrolled.** Options: (a) the assignment enrolls them as Student,
  needing `enrollment.manage` too; (b) refuse until they are enrolled; (c) leave them unable to
  finish. Recommendation: (a): an assignment is how training reaches people, and (b) makes every
  group assignment a two-step task.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Removing assignments.** Options: (a) end, keeping rows and timeline; (b) delete with its
  history; (c) both. Recommendation: (a); the timeline is the record the plan asks for.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Group assignees before groups exist.** The plan assigns to "people or groups"; groups come
  with `../m0/07-admission.md` I7, which is not merged. Options: (a) build user assignees first and
  group assignees in their own issue that depends on 07 I7; (b) wait for groups before any of
  this spec. Recommendation: (a).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Completions before the assignment.** Options: (a) in cycle 1 a target completed at any
  time completes the row; (b) only completions after `assigned_at` count, so people retake what
  they already did. Recommendation: (a) for cycle 1; spec 14 uses cycle start for later cycles.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q6. When in the day reminders go out.** Options: (a) 09:00 in the assignee's time zone on the
  step's day; (b) the same time of day as the due time; (c) as soon as the day starts.
  Recommendation: (a), the working-day time people read email, in their own zone.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q7. Setting managers in M1.** The plan sets a manager "in the UI or through sync" and escalation
  emails "their manager"; M0 never writes `manager_id`. Options: (a) admins set it through `PATCH
  /api/v1/users/{id}` (`user.manage`) now, sync writes it in M4; (b) only sync in M4, so
  escalations are always skipped in M1. Recommendation: (a), the smallest change that makes
  escalation work in M1.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

- **Q8. Which built-in roles manage assignments.** The plan's role table
  does not name assignments; giving `assignment.manage` to Account admin and Instructor extends
  their definitions. Options: (a) Account admin and Instructor as well as Organization admin; (b)
  Organization admin only, others through custom roles; (c) (a) plus Manager in M4.
  Recommendation: (a): assigning training is what account admins and instructors in the plan's
  vZDC mapping do.
  Owner decision (2026-11-16): (a), Account admin and Instructor get `assignment.manage`, as well
  as Organization admin.
- **Q9. What an escalated manager may read.** The plan escalates overdue
  work to "their manager", and the Manager role ("see and remind their own reports, receive
  escalations") arrives in M4. Reading through `users.manager_id` before then gives Manager-like
  scope without the role. Options: (a) the manager reads the timeline of rows escalated to them;
  (b) the escalation message carries the facts and grants no read access until M4's Manager role;
  (c) no escalation to managers until M4. Recommendation: (b): escalation works in M1 as the plan
  asks, and every read right stays with the roles the owner defines.
  Owner decision (2026-11-16): (b), the escalation is the message only, with no read access until
  M4's Manager role (B13, B17).
- **Q10. What escalations may carry, and who may draw reporting lines.**
  Raised by I2's review. Anyone holding `user.manage` can make themself the manager of a user who
  holds no permission they lack, and then receive that user's escalations, which name the
  target, the course and how overdue it is (B13), for courses where the manager may hold no
  `progress.view`. Two parts:
  (1) What an escalation may contain when its recipient lacks progress access to the course.
  Options: (a) the facts B13 lists, whatever the recipient holds; (b) only that something is
  overdue, without target or course, unless the recipient holds `progress.view` there; (c) skip
  the escalation (`skipped: no_access`) for such a recipient. Recommendation: (a) until M4's
  Manager role defines the scope over reports, since B13 already limits it to a message with no
  link; the owner may prefer (b).
  (2) Whether a `user.manage` holder may change their own `manager_id` (and so pick who receives
  their escalations). Options: (a) yes, as I2 builds for a browser session (an API key or app
  token on its own user is refused, B15); (b) no, 409 or 403 for your own user, as
  suspending yourself is refused. Recommendation: (a): it grants the caller nothing, and admins
  set managers for everyone else anyway.
  Owner decision (2026-11-16) on (2): (b), a `user.manage` holder cannot change their own
  `manager_id` (B15), built in I6.
  Coordinator decision (2026-11-16), pending owner review, on (1): the owner's answer names only
  (2), so (1) stays (a): the escalation carries the facts B13 lists, as a message with no link
  and no read access (Q9). With (2) answered, a `user.manage` holder can still name themself the
  manager of another user who holds no permission they lack; that write is audited
  (`user.set_manager`).
  Coordinator decision (2026-11-16), pending owner review: the refusal is 403 with the code
  `user.manager_self`, and it applies to any `manager_id` member on the caller's own user, the
  current value included, so the rule has no exception to test.

## Issues

1. **I1. Assignments to users with due dates.** Migration, `assignment.manage` in the catalog and
   roles (Account admin and Instructor, Q8), the assignment routes for user assignees, enrollment
   on assignment, due rules with time zones, status, the completion subscriber, the lock on deleting
   targeted items and modules. Covers B1 to B5, B7 to B9; T1 to T3, T8 (due changes), T11b, T13 and
   T14.
   Depends on: `05-progress-and-events.md` I1, `01-job-queue-and-outbox.md` I4,
   `02-courses-and-enrollments.md` I2.
2. **I2. Managers.** `manager_id` on the user `PATCH` with the cycle check and audit entry. Covers
   B15; T10. Depends on: none beyond M0. Merged; the self refusal is I6.
3. **I3. Reminders and escalation.** Reminder rules and the organization default, step scheduling,
   the `assignments.reminders` job, `due_reminder` and `escalation` notification and email kinds
   with templates. Covers B10 to B14; T4 to T8. Depends on: I1, I2, `12-email.md` I3,
   `11-announcements-and-in-app-notifications.md` I2.
4. **I4. Timeline, learner lists and the dashboard.** `assignment_events`, the timeline route with
   B17's readers, `/me/assignments`, dashboard deadlines and ordering, due marks in the outline.
   Covers B16 to B20; T11, T12. Depends on: I3, `10-learner-dashboard-and-next-up.md` I1.
5. **I5. Group assignees.** Group targets, `assignments.sync_groups`, removal. Covers B2 (groups),
   B6; T9. Depends on: I1, `../m0/07-admission.md` I7.
6. **I6. Refuse a user changing their own `manager_id` (#305).** B15's self refusal with 403
   `user.manager_self`, in the OpenAPI document and the client. Covers B15 (Q10); T10a. Depends
   on: I2.
