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
- **Review:** standard (enrollment deletion's progress removal, B15, is learner data deletion and
  gets the maintainer's line-by-line review in its issue)

## Summary

This spec records what each learner has done: one progress row per learner and item, updated when
they open an item or mark it done (and, through the same function, when a video is watched far
enough in spec 09, a quiz is submitted in M2 or an activity is mastered in M3). Each change rolls up
in the same transaction to the learner's module and course progress, and every newly finished
item, module and course writes an event to the outbox (`item.completed`, `module.completed`,
`course.completed`) that later specs and M4's webhooks consume. Learners read their own progress
in the course outline; instructors read every learner's.

## Behavior

### Recording progress

- **B1.** A learner is a user holding `perm::CourseParticipate` at the course, which only an
  active enrollment gives (`02` Permissions). Only learners have progress; every progress write
  needs it and is refused for anyone else with 403 `permission.denied`.
- **B2.** `POST /api/v1/items/{item_id}/progress` with `{ "action": "view" }` or
  `{ "action": "mark_done" }` records the learner's step. The item must be visible to them (`03`
  B14) and unlocked (`04` B13: 403 `item.locked`), and the course not archived (409
  `course.archived`). `view` is accepted for page, file and external link items (spec 09 records
  video views itself); `mark_done` for any item whose requirement is `mark_done` (422
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

- **B15.** Deleting an enrollment (`02` B18) deletes, in the same transaction, the user's
  `item_progress`, `module_progress` and `course_progress` rows for the course when no other
  active or concluded enrollment of theirs in the course has `course.participate`; otherwise it
  keeps them. Deleting writes no event. The `enrollment.delete` audit entry gains the count of
  progress rows removed.
- **B16.** Deleting an item that has any `item_progress` row answers 409 `item.has_progress`
  (`03` B9); the check and the delete run under the course lock.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /api/v1/items/{item_id}/progress` | `{ action: "view" \| "mark_done" }` | 200 `ItemProgress` | 404 `item.not_found`, 403 `item.locked`, 409 `course.archived`, 422 `request.invalid` | `perm::CourseParticipate` (the item's course) |
| `GET /api/v1/courses/{course_id}/progress` | `?status&cursor&limit` | 200 `Page<LearnerProgress>` | 404 `course.not_found`, 400 `pagination.invalid_cursor` | `perm::ProgressView` (course) |
| `GET /api/v1/courses/{course_id}/progress/{user_id}` | | 200 `LearnerProgressDetail` | 404 `course.not_found`, 404 `progress.not_found` | `perm::ProgressView` (course) |

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

Repository functions (each takes `OrgScope`): `progress::{record_step, complete_item,
for_learner_in_course, list_for_course, recompute_course, delete_for_enrollment}`. Job kind
`progress.recompute_course` on the `default` queue.

## Permissions

| Permission | Key | Grantable at | Built-in roles (M1) |
| --- | --- | --- | --- |
| `perm::ProgressView` | `progress.view` | organization, account, course | Organization admin, Account admin, Instructor (Q2) |

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
- **T12** (B15). Deleting a Student enrollment removes that learner's three kinds of rows for the
  course and leaves their rows in another course; with a second participating enrollment in the
  course, nothing is removed. The audit entry records the count. Mutation: skipping the delete
  turns the test red.
- **T13** (B16). Deleting an item with one learner's `started` row answers 409
  `item.has_progress`.
- **T14** (every route). Four cases: anonymous 401; every permission but the route's at the course
  403 (for `POST /progress`, a user with every grantable permission through an account
  assignment, which cannot hold `course.participate`, and a concluded Student); every permission at
  a sibling account 404; only the route's permission 2xx.
- **T15** (every route). Cross-organization: org A's admin with org B's course, item or user id
  gets 404, and org B's progress rows and outbox are unchanged.

## Out of scope

- Watch tracking (`video_progress`) and video view recording: spec 09, which calls
  `complete_item`.
- Quiz and activity completion: M2 and M3.
- The dashboard and Next up: spec 10. The roster heat map and per-student timelines (plan,
  Progress views): their screens in spec 16 and later; this spec gives their API.
- Expiring completions and cycles: spec 14, which adds a cycle to completion and the re-lock.
- Delivering events anywhere: subscribers in specs 11, 12 and 14; webhooks in M4.
- The Observer's view of named students: M4 (`02` Q9).
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
  (roster heat map), Observer named students (M4, `02` Q9). Options: (a) a new `progress.view`
  for Organization admin, Account admin and Instructor; (b) reuse `enrollment.view`; (c) also
  Course designer. Recommendation: (a). A separate key lets an organization give a reviewer
  progress without enrollment changes, and designers edit content, not people.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Which steps the client reports.** Options: (a) the client posts `view` for pages, files and
  external links and `mark_done`; video views come from spec 09's watch reports, and later kinds
  from their own engines; (b) the server records a view whenever it serves content (a page body, a
  file URL), with no client post; (c) both. Recommendation: (a). One route records every
  learner-reported step, and a view is not an integrity-sensitive requirement; requirements that
  are (watch, score, mastery) are decided by the server's own engines.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Progress tables, recording and roll-up.** Migration, `record_step`, `complete_item` with
   the course-row lock and roll-up, the three events and their topics, the progress route with
   the lock check, the learner's own progress in reads, and the `item.has_progress` refusal.
   Covers B1 to B7, B10 to B13, B16; T1 to T7, T9, T10, T13, T14 and T15 for `POST`. Depends on:
   `01-job-queue-and-outbox.md` I4, `04-unlock-and-completion-rules.md` I2,
   `02-courses-and-enrollments.md` I2.
2. **I2. Recomputation after rule changes.** The `progress.recompute_course` job, its enqueue from
   every rule-changing write in specs 03 and 04, and batching. Covers B8; T8. Depends on: I1,
   `04-unlock-and-completion-rules.md` I3.
3. **I3. Staff progress reads and enrollment deletion.** `progress.view` in the catalog and roles,
   the two roster routes, and progress removal on enrollment delete (maintainer line-by-line).
   Covers B9, B14, B15; T11, T12, T14 and T15 for these routes. Depends on: I1.
