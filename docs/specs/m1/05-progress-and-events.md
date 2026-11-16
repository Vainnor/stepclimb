# Progress and events

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content ("Progress is stored per user and
  item and rolled up to module and course when it changes. Each change emits `item.completed`,
  `module.completed` or `course.completed`."), Progress and grades (Progress views), Integration
  platform (Events), Data model (`item_progress`, `module_progress / course_progress`), API
  surface (`POST /items/{id}/progress`)
- **Depends on:** `01-job-queue-and-outbox.md` (outbox, jobs), `02-courses-and-enrollments.md`
  (`course.participate`, enrollments, archived courses), `03-modules-and-items.md` (items),
  `04-unlock-and-completion-rules.md` (requirements, locks, completion definitions)
- **Review:** standard, except issue I3 (enrollment deletion's progress removal, B15, deletes
  learner data, and B17 to B20 add the Observer's permission), which gets the maintainer's
  line-by-line review

## Summary

This spec records what each learner has done: one progress row per learner and item, updated when
they open an item or mark it done (and, through the same function, when a video is watched far
enough in spec 09, a quiz is submitted in M2 or an activity is mastered in M3). Each change rolls up
in the same transaction to the learner's module and course progress, and every newly finished
item, module and course writes an event to the outbox (`item.completed`, `module.completed`,
`course.completed`) that later specs and M4's webhooks consume. Learners read their own progress
in the course outline; instructors read every learner's, and an Observer the learners they are
enrolled to observe.

## Behavior

### Recording progress

- **B1.** A learner is a user holding `perm::CourseParticipate` at the course, which only an
  active enrollment gives (`02` Permissions). Only learners have progress; every progress write
  needs it and is refused for anyone else with 403 `permission.denied`.
- **B2.** `POST /api/v1/items/{item_id}/progress` with `{ "action": "view" }` or
  `{ "action": "mark_done" }` records the learner's step. The item must be visible to them (`03`
  B14) and unlocked (`04` B13: 403 `item.locked`), and the course not archived (409
  `course.archived`). `view` is accepted for page, file and external link items (spec 09 records
  video views itself, except for oEmbed videos, whose `view` comes through this route from `09`
  I4, `09` B12a); `mark_done` for any item whose requirement is `mark_done` (422
  `request.invalid`, `progress.action_invalid` at `/action`, otherwise).
- **B3.** An item's progress is one `item_progress` row per learner and item, created at the
  first step with `state = 'started'` and `started_at`, and set to `state = 'completed'` with
  `completed_at` when the item's requirement is met (`04` B3): a `view` meets `view`, a
  `mark_done` meets `mark_done`. A step on an item without a requirement records `started` and
  never completes it. A step that changes nothing (a second view, a view after completion)
  writes nothing and answers 200 with the current progress.
- **B4.** Completion is final (Q1): no route moves a row from `completed` back, and a later change
  to the item's requirement leaves a completed row as it is. A learner cannot unmark "done".
- **B5.** Every way an item completes goes through one repository function,
  `progress::complete_item(tx, scope, user, item, Completion)`, where `Completion` carries the
  requirement kind met and, for M2, a score. Spec 09 (watch), M2 (submit, score) and M3 (mastery)
  call it; none writes `item_progress` itself. It checks the lock and the archived course again
  inside its transaction and refuses with the same codes.

### Rolling up

- **B6.** In the transaction that completes an item, `complete_item` locks the learner's
  `course_progress` row (creating it if absent), reads the course's rules (`04` B7) and the
  learner's completed items, and recomputes, using `04` B11's definitions:
  - for the item's module: `required_count`, `completed_count` and, when newly complete,
    `completed_at`;
  - for the course: the same over its counted modules.
  Locking the course row makes two completions of one learner in one course take turns, so the
  counts and the "newly complete" decision see each other's writes.
- **B7.** `module_progress` and `course_progress` rows store the counts and `completed_at`, so the
  dashboard and the roster read them without recomputing. A module or course `completed_at`, like
  an item's, is final (Q1).
- **B8.** When a course's rules change in a way that changes counts (a requirement added, removed
  or changed, an item or module published, unpublished, moved or deleted, a completion rule or
  prerequisite changed), the writing transaction enqueues `progress.recompute_course` for the course
  with `dedupe_key` = the course id (`01` B3), run 30 s later so a burst of edits collapses into
  one run. The job recomputes every learner's module and course rows from the stored item rows,
  in batches of 100 learners per transaction, and emits `module.completed` and
  `course.completed` for any module or course that becomes complete; it never clears a
  `completed_at` (Q1). It is idempotent: running it twice changes nothing the second time.
- **B9.** Concluding an enrollment keeps the learner's progress; reactivating it continues from
  there (`02` Q8).

### Events

- **B10.** In the transaction that completes it, an item emits `item.completed`; a module that
  becomes complete emits `module.completed`; a course that becomes complete emits
  `course.completed` (`01` B19). Each is emitted once per learner and target, because it is
  emitted only when `completed_at` goes from null to a value, and that happens once (B4, B7).
- **B11.** Event `data` (`01` B20), schema version 1:
  - `item.completed`: `{ "user_id", "course_id", "module_id", "item_id", "item_kind",
    "requirement_kind", "completed_at" }`, plus `"score"` when M2 adds it;
  - `module.completed`: `{ "user_id", "course_id", "module_id", "completed_at" }`;
  - `course.completed`: `{ "user_id", "course_id", "completed_at" }`.
  No names, emails or titles: receivers resolve ids through the API with their own rights.
- **B12.** Events are not emitted for anyone who is not a learner, and no event reports a step
  that does not complete something.

### Reading progress

- **B13.** A learner's own progress joins the course reads of `03` B13 and B14: each item carries
  `progress: { state, started_at, completed_at } | null`, each module `progress: {
  required_count, completed_count, completed_at } | null`, and `GET /api/v1/courses/{course_id}`
  adds the learner's `progress` for the course. The fields are null for a caller with no row and
  absent for one who is not a learner.
- **B14.** `GET /api/v1/courses/{course_id}/progress` lists every learner of the course with their
  course progress and per-module progress, sorted by display name then user id, paginated (`02`
  B3), with `?status=active|concluded` over their enrollment. It needs `perm::ProgressView` at the
  course. `GET /api/v1/courses/{course_id}/progress/{user_id}` returns one learner's item rows as
  well; a user with no enrollment in the course answers 404 `progress.not_found`.

### Deleting progress

- **B15.** Deleting an enrollment (`02` B18), in the same transaction:
  - always deletes the enrollment's own `enrollment_observees` rows (an Observer's links, B17)
    and every `enrollment_observees` row that names the user in this course when the user keeps no
    learner enrollment there (B20);
  - when no other active or concluded enrollment of the user in the course has
    `course.participate`, deletes the user's live progress in the course: `item_progress`,
    `module_progress` and `course_progress` (this spec), `video_progress` and `watch_reports` for
    the course's items (spec 09, added by 09 I2). From 14 I4 it first archives those rows (all but
    `watch_reports`) as one progress archive with reason `enrollment_deleted` (spec 14 B15),
    writing no archive when there is no live progress;
  - otherwise keeps all of it.
  Kept in every case (owner decision, 2026-11-16: deleting an enrollment keeps history):
  `completions` for the course and its modules (spec 14, added by 14 I1), the user's progress
  archives and archived rows (spec 14), `assignment_users`, `reminders_sent` and `assignment_events`
  (spec 13: the record that the person was assigned and reminded), `email_messages` (spec 12,
  deleted by its 90-day prune) and `audit_events`. Erasing completions and archives is possible only
  through M4's per-user data deletion. Deleting writes no event. The `enrollment.delete` audit entry
  gains the count of rows removed per table. Each later spec that adds learner data for a course
  adds its table to this list and to T12, in the issue named above.
- **B16.** Deleting an item that has any `item_progress` row answers 409 `item.has_progress`
  (`03` B9); the check and the delete run under the course lock.

### Observers

`../m0/09-permissions-and-roles.md` B5, accepted by the owner, gives the Observer "read-only view
of named students' progress" and says its scope arrives with progress in M1. This section is that
scope (`02-courses-and-enrollments.md` Q9).

- **B17.** An Observer is enrolled in a course with role Observer and a list of observed learners,
  `observed_user_ids` (1 to 100 users, each holding an enrollment in the course whose role has
  `course.participate`, active or concluded; 422 `request.invalid`,
  `enrollment.observed_invalid` at the entry otherwise). The list is set on `POST
  /api/v1/courses/{course_id}/enrollments` and replaced whole by `PATCH
  /api/v1/enrollments/{enrollment_id}` with `{ "observed_user_ids" }`; only an enrollment whose
  role holds `progress.view_observed` takes the member (422 otherwise). This lifts `02` B17's
  refusal of Observer enrollments.
- **B18.** The built-in Observer role holds `course.view` and `perm::ProgressViewObserved`
  (`progress.view_observed`, grantable at course scope only, so only an enrollment gives it). A
  custom role may hold it too. An Observer reads the course as a learner without progress would
  (locks apply, `04` B5) and records no progress (no `course.participate`).
- **B19.** Naming observed learners, on create or `PATCH`, needs `perm::ProgressView` at the
  course in addition to `perm::EnrollmentManage` (403 `permission.escalation` otherwise): whoever
  points an Observer at learners must already be able to read those learners' progress. This is
  the check `02` B17 and Q12 leave to this spec in place of the general escalation rule.
- **B20.** The two staff progress routes (B14) take `Require<perm::CourseView, CourseTarget>` and
  then decide in the handler: a caller holding `progress.view` at the course reads every learner;
  a caller holding only `progress.view_observed` reads only the learners named by their active
  Observer enrollments in that course (the list route filters to them; the detail route answers
  404 `progress.not_found` for anyone else); any other caller gets 403 `permission.denied`. A
  concluded Observer enrollment grants nothing (`02` B16). When an observed learner's enrollment is
  deleted, the observer link to them is deleted with it.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /api/v1/items/{item_id}/progress` | `{ action: "view" \| "mark_done" }` | 200 `ItemProgress` | 404 `item.not_found`, 403 `item.locked`, 409 `course.archived`, 422 `request.invalid` | `perm::CourseParticipate` (the item's course) |
| `GET /api/v1/courses/{course_id}/progress` | `?status&cursor&limit` | 200 `Page<LearnerProgress>` | 404 `course.not_found`, 400 `pagination.invalid_cursor` | `perm::CourseView` (course), then `perm::ProgressView`, or `perm::ProgressViewObserved` filtered to observed learners (B20) |
| `GET /api/v1/courses/{course_id}/progress/{user_id}` | | 200 `LearnerProgressDetail` | 404 `course.not_found`, 404 `progress.not_found` | as above (B20) |

Changed from `02-courses-and-enrollments.md`: the enrollment `POST` and `PATCH` take
`observed_user_ids` (B17, B19) and the `Enrollment` schema gains `observed_user_ids` (present for
roles holding `progress.view_observed`).

Changed: the reads of `03` (outline, module, item) and `GET /api/v1/courses/{course_id}` add the
caller's own progress (B13); `DELETE /api/v1/items/{item_id}` adds 409 `item.has_progress`;
`DELETE /api/v1/enrollments/{enrollment_id}` removes progress (B15).

Schemas: `ItemProgress { item_id, state: "started" | "completed", started_at, completed_at }`;
`ModuleProgress { module_id, required_count, completed_count, completed_at }`;
`CourseProgress { required_count, completed_count, completed_at }`; `LearnerProgress { user: { id,
display_name }, enrollment_status, course: CourseProgress | null, modules: ModuleProgress[] }`;
`LearnerProgressDetail` adds `items: ItemProgress[]`.

Events (documented as OpenAPI schema components, as `07` documents its outgoing webhook body):
`ItemCompletedEvent`, `ModuleCompletedEvent`, `CourseCompletedEvent`, the envelope of `01` B20
around B11's `data`.

## Data

Migration `migrations/<version>_progress.sql` (I1). The version is a placeholder, fixed when the
issue is implemented so it sorts after every migration on `next` and every open branch.

```sql
CREATE TABLE item_progress (
    org_id        uuid        NOT NULL,
    user_id       uuid        NOT NULL,
    course_id     uuid        NOT NULL,       -- not in the data model: B6, B15 read by course
    item_id       uuid        NOT NULL,
    state         text        NOT NULL CHECK (state IN ('started', 'completed')),
    score         numeric,                    -- written from M2
    started_at    timestamptz NOT NULL,
    completed_at  timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, item_id) REFERENCES module_items (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CHECK ((state = 'completed') = (completed_at IS NOT NULL))
);
CREATE INDEX item_progress_by_course ON item_progress (org_id, course_id, user_id);
CREATE INDEX item_progress_by_item ON item_progress (item_id);

CREATE TABLE module_progress (
    org_id           uuid        NOT NULL,
    user_id          uuid        NOT NULL,
    course_id        uuid        NOT NULL,
    module_id        uuid        NOT NULL,
    required_count   integer     NOT NULL CHECK (required_count >= 0),
    completed_count  integer     NOT NULL CHECK (completed_count >= 0),
    completed_at     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, module_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id, module_id) REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE
);
CREATE INDEX module_progress_by_course ON module_progress (org_id, course_id, user_id);

CREATE TABLE course_progress (
    org_id           uuid        NOT NULL,
    user_id          uuid        NOT NULL,
    course_id        uuid        NOT NULL,
    required_count   integer     NOT NULL CHECK (required_count >= 0),
    completed_count  integer     NOT NULL CHECK (completed_count >= 0),
    completed_at     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, course_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id)
);
CREATE INDEX course_progress_by_course ON course_progress (org_id, course_id);
```

The `item_progress` foreign key to `module_items` has the default `NO ACTION`, which backs B16:
an item with progress cannot be deleted. `module_progress` cascades when its module is deleted,
which `03` B3 allows only for an empty module, so no item progress is lost. Columns beyond the
data model (`org_id` everywhere, `course_id`, `started_at`, the counts) are named in the comments
and B6, B7. `score` stays null until M2.

Observed learners: migration `migrations/<version>_enrollment_observees.sql` (I3), version fixed
the same way:

```sql
CREATE TABLE enrollment_observees (            -- not in the data model: B17, Q4
    org_id         uuid        NOT NULL,
    enrollment_id  uuid        NOT NULL REFERENCES enrollments (id) ON DELETE CASCADE,
    course_id      uuid        NOT NULL,
    user_id        uuid        NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (enrollment_id, user_id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
CREATE INDEX enrollment_observees_by_user ON enrollment_observees (org_id, course_id, user_id);
```

The repository checks that `course_id` is the enrollment's course and that each user is a learner
of it (B17); deleting a learner's enrollment deletes their rows here when they have no other
learner enrollment in the course (B20).

Repository functions (each takes `OrgScope`): `progress::{record_step, complete_item,
for_learner_in_course, list_for_course, recompute_course, delete_for_enrollment}`. Job kind
`progress.recompute_course` on the `default` queue.

## Permissions

| Permission | Key | Grantable at | Built-in roles (M1) |
| --- | --- | --- | --- |
| `perm::ProgressView` | `progress.view` | organization, account, course | Organization admin, Account admin, Instructor (Q2) |
| `perm::ProgressViewObserved` | `progress.view_observed` | course | Observer (B18; `m0/09` B5) |

Recording progress takes `perm::CourseParticipate` (`02`). Entitlements: none.

## Test cases

Database tests use `#[sqlx::test]` and a `ManualClock`; route tests use the permission suite.

- **T1** (B1, B2). A Course designer (no `course.participate`) posting `view` gets 403
  `permission.denied`; a Student gets 200 with `state = "started"`.
- **T2** (B2, B3). For a page with requirement `view`, one `view` completes it; for one with
  `mark_done`, a `view` starts it and `mark_done` completes it; `mark_done` on a `view` item
  answers 422 `progress.action_invalid`; a second `view` changes no `updated_at`.
- **T3** (B2, `04` B13). A Student posting progress to a locked item gets 403 `item.locked` with
  reasons, and no row is written; in an archived course, 409 `course.archived`.
- **T4** (B4). Removing a completed item's requirement, then changing it to `mark_done`, leaves
  the row completed with its original `completed_at`.
- **T5** (B6, B10). Completing the last required item of the last counted module, in one request,
  writes exactly one `item.completed`, one `module.completed` and one `course.completed` outbox row,
  with B11's `data` and the manual clock's time; repeating the request writes none.
- **T6** (B6). Two concurrent completions of the two required items of one module by one learner
  (two connections) end with `completed_count = 2` and exactly one `module.completed`.
- **T7** (B6, `04` B11). Under completion rule `one`, the first of two requirements completes the
  module; the second completes its item only and emits only `item.completed`.
- **T8** (B8). Publishing a new required item in a module that a learner completed leaves its
  `completed_at` and raises `required_count` after the recompute job runs; removing the only
  unmet requirement of an incomplete module completes it in the job and emits `module.completed`
  once. Ten edits within 30 s enqueue one job. Running the job twice emits nothing new.
- **T9** (B11, B12). No event payload contains a display name, email or title (checked against
  the test data's values); a step that only starts an item writes no event.
- **T10** (B13). A Student's outline carries their item and module progress; another Student's
  progress never appears in it.
- **T11** (B14). An Instructor lists both learners' progress; `?status=concluded` lists the
  concluded one only; a user with no enrollment answers 404 `progress.not_found`.
- **T12** (B15). Deleting a Student enrollment removes that learner's progress rows for the course
  (and, as 09 I2 adds them, video progress and watch reports) and leaves their rows in another
  course, their assignment rows and timeline, and (as 14 I1 and I4 add them) their completions and
  archives, with one `enrollment_deleted` archive of the removed rows; it removes the Observer links
  that named them; with a second participating enrollment in the course, no learner data is removed.
  Deleting an Observer enrollment removes its links only. The audit entry records the counts.
  Mutation: skipping the delete turns the test red.
- **T13** (B16). Deleting an item with one learner's `started` row answers 409
  `item.has_progress`.
- **T14** (every route). Four cases: anonymous 401; every permission but the route's at the course
  403 (for `POST /progress`, a user with every grantable permission through an account
  assignment, which cannot hold `course.participate`, and a concluded Student); every permission at
  a sibling account 404; only the route's permission 2xx.
- **T14a** (B20, the two staff progress routes). These take `Require<perm::CourseView>` and decide
  in the handler, so the permission suite has one entry per granting permission: an entry for
  `progress.view` (2xx with every learner) and an entry for `progress.view_observed` (2xx with only
  the observed learners), each with its own 401, 403 and 404 cases. Explicit tests: a Student
  (holding `course.view` and `course.participate`) gets 403 `permission.denied` on both routes; a
  caller holding only `course.view` at the course gets 403 `permission.denied`; an Observer with no
  active observed learner gets 200 with an empty list.
- **T15** (every route). Cross-organization: org A's admin with org B's course, item or user id
  gets 404, and org B's progress rows and outbox are unchanged.
- **T16** (B17, B19). An Instructor enrolls an Observer naming two Students (201); naming a user
  with no learner enrollment answers 422 `enrollment.observed_invalid`; a caller with
  `enrollment.manage` but not `progress.view` naming learners answers 403
  `permission.escalation`; `observed_user_ids` on a Student enrollment answers 422.
- **T17** (B18, B20). The Observer lists progress and sees exactly the two named Students; the
  detail of a third Student answers 404 `progress.not_found`; posting progress answers 403; after
  the Observer enrollment is concluded, the list answers 403 `permission.denied`. Mutation:
  dropping the observed-learner filter turns the test red.
- **T18** (B20). Deleting a named Student's enrollment removes them from the Observer's list.

## Out of scope

- Watch tracking (`video_progress`) and video view recording: spec 09, which calls
  `complete_item`.
- Quiz and activity completion: M2 and M3.
- The dashboard and Next up: spec 10. The roster heat map and per-student timelines (plan,
  Progress views): their screens in spec 16 and later; this spec gives their API.
- Expiring completions and cycles: spec 14, which adds a cycle to completion and the re-lock.
- Delivering events anywhere: subscribers in specs 11, 12 and 14; webhooks in M4.
- Gradebook and certificates: M2 and later.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. Whether completion can be undone.** The plan names only `*.completed` events, no reverse.
  Options: (a) item, module and course completion are final: a rule change or an unmark never
  clears them, and a recompute only adds; (b) recompute both ways, so adding a requirement makes
  a completed module incomplete again (Canvas's model), with no event for it; (c) learners may
  unmark "done", but module and course completion are final. Recommendation: (a). An event that
  reached an integration (a certificate, a roster update) is never contradicted, and only expiry
  (spec 14), which the plan names, takes a completion away.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Who reads other learners' progress.** The plan: Instructor and Account admin see progress
  (roster heat map), Observer named students (M1: B17 to B20, `02` Q9). Options: (a) a new
  `progress.view` for Organization admin, Account admin and Instructor; (b) reuse `enrollment.view`;
  (c) also Course designer. Recommendation: (a). A separate key lets an organization give a reviewer
  progress without enrollment changes, and designers edit content, not people.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Which steps the client reports.** Options: (a) the client posts `view` for pages, files and
  external links and `mark_done`; video views come from spec 09's watch reports, and later kinds
  from their own engines; (b) the server records a view whenever it serves content (a page body, a
  file URL), with no client post; (c) both. Recommendation: (a). One route records every
  learner-reported step, and a view is not an integrity-sensitive requirement; requirements that
  are (watch, score, mastery) are decided by the server's own engines.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

- **Q4. How an Observer's named learners are recorded.** `m0/09` B5 fixes the Observer's scope as
  named students and its milestone as M1; the shape is open. Options: (a) per course, on the
  Observer's enrollment (`enrollment_observees`), so an Observer sees named learners in the courses
  they are enrolled in; (b) per organization, an observer-to-learner link covering every course
  of the learner; (c) a role assignment whose scope is a user. Recommendation: (a). It reuses the
  enrollment grant (`02` Q1), needs no new scope type in the evaluator, and an admin decides per
  course who oversees whom; (b) can come with M4's Manager scope, which is per person.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Progress tables, recording and roll-up.** Migration, `record_step`, `complete_item` with
   the course-row lock and roll-up, the three events and their topics, the progress route with
   the lock check (`04-unlock-and-completion-rules.md` I2's `require_unlocked` and `item.locked`),
   the learner's own progress in reads, and the `item.has_progress` refusal.
   Covers B1 to B7, B10 to B13, B16; T1 to T7, T9, T10, T13, T14 and T15 for `POST`. Depends on:
   `01-job-queue-and-outbox.md` I4, `04-unlock-and-completion-rules.md` I2,
   `02-courses-and-enrollments.md` I2.
2. **I2. Recomputation after rule changes.** The `progress.recompute_course` job, its enqueue from
   every rule-changing write in specs 03 and 04, and batching. Covers B8; T8. Depends on: I1,
   `04-unlock-and-completion-rules.md` I3.
3. **I3. Staff and Observer progress reads, and enrollment deletion.** `progress.view` and
   `progress.view_observed` in the catalog and roles, the two roster routes with the observed
   filter, Observer enrollments with `enrollment_observees` (lifting `02` B17's Observer refusal),
   and progress removal on enrollment delete (maintainer line-by-line: permissions and learner data
   deletion), with one permission-suite entry per granting permission. Covers B9, B14, B15, B17
   to B20; T11, T12, T14, T14a, T15 to T18 for these routes. Depends on:
   I1, `02-courses-and-enrollments.md` I2.
