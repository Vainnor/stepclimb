# Recurrence and expiring completions

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Goals (recurring training, completions that expire),
  Progress and grades (Due dates, recurrence and expiry: "Recurrence: a course can recur ('every 12
  months' or 'every January'). Each cycle is a new assignment, and history is kept per cycle.
  Expiry: completions can expire after a set time. An expired completion re-locks anything that
  depends on it and triggers the next cycle."; Certificates show an expiry date where one applies),
  Data model (`assignments.recurrence`, `assignments.cycle`, `completions(id, user_id,
  target_type, target_id, cycle, completed_at, expires_at?)`), Decided (recurring training in the
  core from M1)
- **Depends on:** `13-assignments-due-dates-reminders-and-escalation.md` (assignments, per-person
  rows, reminders), `05-progress-and-events.md` (progress, roll-up, final completion and its one
  exception), `04-unlock-and-completion-rules.md` (locks), `01-job-queue-and-outbox.md` (periodic
  jobs, outbox), `11-announcements-and-in-app-notifications.md` (`notify`)
- **Review:** maintainer line-by-line (an expiry takes a completion away and re-locks content)

## Summary

Some training has to be repeated: a course assigned "every 12 months" or "every January" starts a
new cycle for each person, with its own due date and reminders, and the history of every cycle is
kept. A course or module can also say how long its completion stays valid; when that time passes
the completion expires, the learner's progress in it is cleared so they take it again, anything
that required it locks again through spec 04's rules, and the next cycle of a recurring assignment
starts. Expiry is the one place where a completion recorded by spec 05 is taken away; whether a
cycle that starts before the completion expires also clears progress is the owner's open Q6, and
until it is answered only expiry does.

## Behavior

### Completion history

- **B1.** Every course and module completion is recorded in `completions` (target, user, `cycle`,
  `completed_at`, `expires_at`), in the transaction where spec 05 sets the module or course
  `completed_at` (`05` B6), so the history has one row per completion per cycle. `cycle` counts the
  user's completions of that target: 1 for the first, 2 after the first reset (B6, B7, B10), and so
  on. Item completions are not recorded here (Q5).
- **B2.** `expires_at` is `completed_at` plus the target's validity, when it has one (B3), in
  calendar months in the organization's default time zone; otherwise null.

### Validity

- **B3.** A course and a module each take `completion_valid_months` (1 to 120, or null for never;
  Q2), set by `PATCH` with `perm::CourseManage` (course) or `perm::CourseContentEdit` (module).
  Changing it applies to completions recorded afterwards; existing `expires_at` values stay (Q3).
- **B4.** Item completions do not expire in M1: whether they should is the owner's open question
  Q5, and the spec builds course and module validity only until it is answered.

### Expiry

- **B5.** The periodic job `completions.expire` (every 15 minutes, `01` B17) finds completions
  with `expires_at <= now` and `expired_at` null, and for each, in one transaction under the
  learner's `course_progress` lock (`05` B6):
  1. sets `completions.expired_at`;
  2. when the learner's current progress is still the expiring cycle's (no reset happened since,
     B10), resets it (B6 for a module, B7 for a course);
  3. emits `completion.expired` (`01` B19) with `{ user_id, course_id, target_type, target_id,
     cycle, expired_at }`, an internal topic for this spec's subscribers and spec 11's
     notification; whether M4 webhooks publish it is M4's;
  4. for a course, starts the next cycle of every recurring assignment of the course for that
     learner whose next cycle has not started (B10).
  It is idempotent: a completion already expired is skipped.
- **B6.** Resetting a module for a learner deletes their `item_progress`, `video_progress` and
  `watch_reports` rows for the module's items and their `module_progress` row, then recomputes
  their course progress counts (`05` B6). The course's `completed_at` is kept: a course completion
  has its own validity (B3). Modules that list the reset module as a prerequisite, and items after
  it in sequential order, lock again for that learner through `04` B8 and B9, because the
  prerequisite is no longer complete. This is the plan's "re-locks anything that depends on it".
- **B7.** Resetting a course for a learner deletes all their progress rows in the course (items,
  video, modules, course), as B6 does for every module; the course is then not complete and every
  module with a prerequisite locks again.
- **B8.** The learner gets a `completion_expired` notification (in-app and email, not required;
  `11` B10, `12` B8) naming the target and linking to it.

### Recurrence

- **B9.** A recurring assignment targets a course (422 `assignment.recurrence_needs_course`
  otherwise) and has a relative due rule (422 `assignment.recurrence_needs_relative_due`; `13`
  B4), since every cycle needs its own due date. Its `recurrence` is one of (Q1):
  - `{ "every_months": 1..120, "start_days_before": 0..365 }`: the next cycle starts that many
    months after the learner completed the current cycle, moved earlier by `start_days_before`;
  - `{ "every_year_in": 1..12, "start_days_before": 0..365 }`: the next cycle starts on the first
    day of that month every year (in the organization's time zone), moved earlier by
    `start_days_before`, whether or not the current cycle was completed.
- **B10.** Starting a cycle for a learner (by the periodic job `assignments.cycles`, every 15
  minutes, or by an expiry, B5): in one transaction, close the previous cycle's row
  (`assignment_users`, `13` B2) as it stands (completed, or recorded as `incomplete` when it was
  not), insert the new row with `cycle + 1`, `assigned_at` = the cycle's start and `due_at` from the
  relative due rule, schedule its reminders (`13` B11), and write `assignment_events`
  `cycle_started`. Starting a cycle does not touch progress: the plan re-locks only on expiry, so
  the learner retakes the course once their previous completion has expired (B5, B7). Whether a
  cycle start should also clear progress, so a retake can begin before the old completion lapses,
  is Q6 (open, for the owner). "Each cycle is a new assignment" (plan) is this per-person row
  (Q4).
- **B10a.** Until Q6 is answered, a recurring assignment needs a course with
  `completion_valid_months` (422 `assignment.recurrence_needs_validity`), so every cycle's retake
  is opened by an expiry, and a scheduled cycle start that finds the learner's course completion
  still valid waits until that completion expires; `start_days_before` is accepted and stored but
  has no effect.
- **B11.** With `every_months: 12` and a 12-month validity, cycle 2 starts when cycle 1's
  completion expires (B5 step 4, B10a), the expiry clears the learner's progress (B7), and cycle 2
  is due `relative_days` later. If the owner answers Q6 with its recommendation, `start_days_before:
  30` would start cycle 2 30 days before the expiry with progress cleared then, while the earlier
  completion stays valid in `completions` until its `expires_at`.
- **B12.** A cycle's row completes when the course completes on or after the cycle's
  `assigned_at` (`13` B8, Q5 there). Ending the assignment stops new cycles; adding a recurrence to
  an existing assignment applies from the next completion.

### Reading history

- **B13.** `GET /api/v1/courses/{course_id}/completions?user_id&target_type&cursor&limit` lists
  completion history (target, cycle, completed and expiry dates, expired or valid) to holders of
  `perm::ProgressView` (and observers for observed learners, `05` B20); `GET
  /api/v1/me/completions` lists the caller's own. Item responses and module progress (`05` B13)
  gain `expires_at` for the current completion so the UI can show "valid until".
- **B14.** The assignment timeline (`13` B16) shows each cycle's start, due date, reminders,
  completion and the completion's expiry.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/completions` | `?user_id&target_type&cursor&limit` | 200 `Page<Completion>` | 404 `course.not_found` | `perm::CourseView` (course), then `progress.view` or `progress.view_observed` (`05` B20) |
| `GET /api/v1/me/completions` | `?course_id&cursor&limit` | 200 `Page<Completion>` | | authenticated |

Changed: `PATCH /api/v1/courses/{course_id}` takes `completion_valid_months` (`perm::CourseManage`);
`PATCH /api/v1/modules/{module_id}` takes `completion_valid_months`
(`perm::CourseContentEdit`); the assignment `POST` and `PATCH` (`13`) take `recurrence` (422
`assignment.recurrence_needs_course`, `assignment.recurrence_needs_relative_due`,
`request.invalid`). Schemas: `Completion { id, user: { id, display_name }, target: { type, id,
title }, cycle, completed_at, expires_at, expired_at }`; `Recurrence` as in B9.

## Data

Migration `migrations/<version>_completions.sql` (I1) and
`migrations/<version>_recurrence.sql` (I3). Versions are placeholders, fixed when each issue is
implemented so each sorts after every migration on `next` and every open branch.

```sql
-- <version>_completions.sql
CREATE TABLE completions (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL,
    user_id       uuid        NOT NULL,
    course_id     uuid        NOT NULL,              -- not in the data model: reads by course
    target_type   text        NOT NULL CHECK (target_type IN ('course', 'module')),
    target_id     uuid        NOT NULL,
    cycle         integer     NOT NULL CHECK (cycle >= 1),
    completed_at  timestamptz NOT NULL,
    expires_at    timestamptz,
    expired_at    timestamptz,                       -- not in the data model: B5
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id, target_type, target_id, cycle),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CHECK (expires_at IS NULL OR expires_at > completed_at)
);
CREATE INDEX completions_due ON completions (expires_at) WHERE expired_at IS NULL AND expires_at IS NOT NULL;
CREATE INDEX completions_by_course ON completions (org_id, course_id, user_id);

ALTER TABLE courses ADD COLUMN completion_valid_months smallint CHECK (completion_valid_months BETWEEN 1 AND 120);
ALTER TABLE modules ADD COLUMN completion_valid_months smallint CHECK (completion_valid_months BETWEEN 1 AND 120);

-- <version>_recurrence.sql
ALTER TABLE assignment_users ADD COLUMN outcome text CHECK (outcome IN ('completed', 'incomplete'));
ALTER TABLE assignment_users ADD COLUMN closed_at timestamptz;
CREATE INDEX assignment_users_open ON assignment_users (assignment_id, user_id) WHERE closed_at IS NULL;
```

`target_id` has no foreign key (course or module by type); a module's deletion is already refused
while it has progress (`03` Q3), and its completions are kept as history when the module is later
deleted. `assignments.recurrence` (created by `13`) holds B9's JSON, validated by the typed
`Recurrence` in `stepclimb-core`. Spec 05's "a completion is final" (`05` B4, Q1) gains this one
exception, stated in `05` by I2's PR.

## Permissions

No new catalog entry: validity is set with `perm::CourseManage` or `perm::CourseContentEdit`,
recurrence with `perm::AssignmentManage` (`13`), history read with `perm::ProgressView` or
`progress.view_observed`. Entitlements: none.

## Test cases

Tests use a `ManualClock` and run the periodic jobs by hand.

- **T1** (B1, B2). Completing a module and its course with validities of 6 and 12 months writes
  two `completions` rows, cycle 1, with `expires_at` 6 and 12 calendar months later in the
  organization's time zone (31 January plus 1 month is the last day of February).
- **T2** (B5, B6). When the module's completion expires: its progress rows are gone, the course
  stays complete, a module requiring it is locked with reason `prerequisites` for that learner and
  not for another, one `completion.expired` event and one notification exist, and running the job
  again changes nothing. Mutation: skipping the reset leaves the dependent module unlocked and turns
  the test red.
- **T3** (B7). When the course's completion expires, every progress row of the learner in the
  course is gone and Next up (`10`) points at the first requirement again.
- **T4** (B9). Recurrence on a module-target assignment answers 422
  `assignment.recurrence_needs_course`; with a fixed due date 422
  `assignment.recurrence_needs_relative_due`.
- **T5** (B9, B10, B10a). `every_months: 12` with `relative_days: 30` on a course valid 12 months:
  a completion on 2026-03-10 expires on 2027-03-10, which clears the learner's progress and starts
  cycle 2, due 2027-04-09; cycle 1 is closed `completed`; reminders of cycle 2 follow `13`. A
  recurring assignment on a course without validity answers 422
  `assignment.recurrence_needs_validity`.
- **T6** (B9, B10a). `every_year_in: 1` on a course valid 12 months: on 1 January cycle 2 starts
  for an assignee who never completed cycle 1 (closed `incomplete`); for one whose completion is
  still valid, cycle 2 waits until that completion expires.
- **T7** (B10, Q6). Starting a cycle changes no `item_progress`, `module_progress` or
  `course_progress` row (until Q6 is answered); mutation: a reset at cycle start turns the test
  red.
- **T8** (B12). Completing the course after cycle 2 started completes cycle 2's row, not cycle 1's.
- **T9** (B3). Changing a course's validity leaves earlier `expires_at` values as they were.
- **T10** (B13). An Instructor lists a learner's history across cycles; the learner reads their
  own; another learner gets none of it.
- **T10a** (B13, `GET /courses/{id}/completions`). The route decides in the handler, so the
  permission suite has one entry for `progress.view` and one for `progress.view_observed`, each with
  its own cases. Explicit tests: a Student gets 403 `permission.denied`; a caller holding only
  `course.view` gets 403 `permission.denied`; an Observer sees only observed learners' rows.
- **T11** (routes). Four cases for `GET /courses/{id}/completions` and the changed `PATCH` fields;
  `AuthOnly` cases for `/me/completions`.
- **T12** (routes, jobs). Cross-organization: org A's admin with org B's course or user id gets 404;
  the expiry and cycle jobs, run with both organizations' rows due, change each organization's
  rows only within it.

## Out of scope

- Certificates and their expiry date: a later spec (plan, Progress and grades).
- The compliance report (compliant, due soon, overdue, expired): M4.
- Item-level expiry: the owner's Q5.
- Cross-course prerequisites: not in the plan.
- Keeping item-level progress of past cycles: not kept (Q6); the completion history is.

## Open questions

Q1 to Q4 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q5 and Q6 are the owner's and stay open.

- **Q1. How recurrence rules are written.** The plan's examples are "every 12 months" and "every
  January". Options: (a) the two forms of B9, each with an optional earlier start; (b) a cron-like
  or RFC 5545 rule; (c) only "every N months". Recommendation: (a), exactly the plan's two
  examples, with a lead time so a retake can finish before a completion lapses.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Where validity is set.** Options: (a) on courses and modules, in months; (b) per
  assignment; (c) in days. Recommendation: (a): validity describes the training, not who was told
  to take it, and months match "every 12 months".
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Changing validity later.** Options: (a) applies to new completions only; (b) recomputes
  every stored `expires_at`. Recommendation: (a); a learner's recorded expiry never moves under
  them.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. "Each cycle is a new assignment".** Options: (a) each cycle is a new per-person row of the
  same assignment (`assignment_users` with `cycle`), since cycles of "every 12 months" start at a
  different time for each person; (b) a new `assignments` row per cycle, which fits shared cycles
  ("every January") but not per-person ones. Recommendation: (a), with the data model's
  `assignments.cycle` left at 1.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Item-level expiry (open, for the owner).** The plan says "completions can expire" without
  naming which. This spec builds course and module validity; leaving items out narrows the plan,
  which is the owner's call. Options: (a) course and module only: an item's completion is part of
  its module's, and a module of one item covers the case; (b) items too, with B6's reset applied to
  one item and sequential order re-locking what follows; (c) courses only. Recommendation: (a).
  Until the owner answers, items do not expire (B4); answering (b) adds an issue.
- **Q6. Whether a new cycle clears progress before expiry (open, for the owner).** The plan
  re-locks content only when a completion expires ("An expired completion re-locks anything that
  depends on it"), and `05` Q1 and `04` B4 make completion final apart from that. Clearing progress
  when a cycle starts would re-lock content before any expiry, which the plan does not say, so this
  is the owner's. Options: (a) a cycle start clears the learner's live progress in the course, so a
  retake can start before the old completion lapses (`start_days_before`), with history kept as
  completions per cycle and the assignment rows and timeline per cycle; (b) expiry only: a cycle
  starts no earlier than the expiry that clears progress, and recurrence requires a validity
  (B10a); (c) keep every progress row per cycle, adding a cycle column to spec 05's tables.
  Recommendation: (a), since compliance training is usually retaken before it lapses. Until the
  owner answers, the spec builds (b), the plan's expiry-only re-lock.

## Issues

1. **I1. Completion history and validity.** Migration, recording completions in spec 05's roll-up
   transaction, validity on courses and modules, history routes, `expires_at` in reads, and
   `completions` added to `05` B15's enrollment deletion (with its T12 case). Covers B1
   to B4, B13; T1, T9 to T12 (routes). Depends on: `05-progress-and-events.md` I3,
   `02-courses-and-enrollments.md` I1.
2. **I2. Expiry and re-locking.** The `completions.expire` job, module and course resets, the
   `completion.expired` event and notification, and the texts that state completion is final,
   updated to name expiry as the exception: `05` B4, `05` B10 (events once per learner and target
   become once per cycle) and `04` B4. Covers B5 to B8;
   T2, T3, T12 (jobs). Depends on: I1, `04-unlock-and-completion-rules.md` I3,
   `11-announcements-and-in-app-notifications.md` I2, `12-email.md` I3.
3. **I3. Recurrence and cycles.** Migration, recurrence validation (including B10a's validity rule),
   the `assignments.cycles` job, cycle start and closing without touching progress, cycles started
   by expiry, timeline entries. Q6 (owner) decides whether a later issue adds the cycle-start
   reset. Covers B9 to B12,
   B14; T4 to T8. Depends on: I2, `13-assignments-due-dates-reminders-and-escalation.md` I4.
