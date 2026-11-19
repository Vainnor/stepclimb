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
  core from M1); the owner's answers of 2026-11-16 (Q5, Q6)
- **Depends on:** `13-assignments-due-dates-reminders-and-escalation.md` (assignments, per-person
  rows, reminders), `05-progress-and-events.md` (progress, roll-up, final completion and its two
  exceptions, enrollment deletion), `04-unlock-and-completion-rules.md` (locks),
  `09-linked-video-and-watch-tracking.md` (`video_progress`), `01-job-queue-and-outbox.md`
  (periodic jobs, outbox), `11-announcements-and-in-app-notifications.md` (`notify`)
- **Review:** maintainer line-by-line (an expiry takes a completion away and re-locks content)

## Summary

Some training has to be repeated: a course assigned "every 12 months" or "every January" starts a
new cycle for each person, with its own due date and reminders, and the history of every cycle is
kept. A course or module can also say how long its completion stays valid; when that time passes
the completion expires, the learner's progress in it is cleared so they take it again, anything
that required it locks again through spec 04's rules, and the next cycle of a recurring assignment
starts. A new cycle also clears the learner's progress when it starts, so a retake can begin
before the old completion lapses. Expiry and a cycle start are the two places where progress
recorded by spec 05 is taken away, and neither loses it: each reset first copies what it clears
into a progress archive that staff holding `progress.view` can read. Deleting an enrollment
archives the same way. Nothing prunes archives, completions or timelines automatically: the
owner's retention requirement (Q6) is that a school keeps such records for years.

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
- **B4.** Item completions do not expire (Q5): only courses and modules take a validity. An item's
  completion is part of its module's, and a module of one item covers the case.

### Expiry

- **B5.** The periodic job `completions.expire` (every 15 minutes, `01` B17) finds completions
  with `expires_at <= now` and `expired_at` null, and for each, in one transaction under the
  learner's `course_progress` lock (`05` B6):
  1. sets `completions.expired_at`;
  2. when no reset happened since the completion, resets it (B6 for a module, B7 for a course),
     archiving it first (B15). A reset happened since when the learner has a `progress_archives`
     row in the course with `archived_at >= completions.completed_at` whose target is the course
     or, for a module completion, that module: a course reset clears every module, a module reset
     only its own;
  3. unless the learner has a newer completion of the same target (a higher `cycle`, so they
     retook it before this one lapsed), emits `completion.expired` (`01` B19) with `{ user_id,
     course_id, target_type, target_id, cycle, expired_at }`, an internal topic for this spec's
     subscribers and spec 11's notification, whose publication by M4 webhooks is M4's; with a
     newer completion, it emits nothing and B8 sends nothing;
  4. for a course, starts the next cycle of every recurring assignment of the course for that
     learner whose next cycle has not started (B10).
  It is idempotent: a completion already expired is skipped.
- **B6.** Resetting a module for a learner archives (B15) and then deletes their `item_progress`,
  `video_progress` and `watch_reports` rows for the module's items and their `module_progress` row,
  then recomputes their course progress counts (`05` B6). The course's `completed_at` is kept: a
  course completion has its own validity (B3). Modules that list the reset module as a prerequisite,
  and so every item in them, lock again for that learner through `04` B8 and B9, because the
  prerequisite is no longer complete. This is the plan's "re-locks anything that depends on it".
- **B7.** Resetting a course for a learner archives (B15) and then deletes all their progress rows
  in the course (items, video, modules, course), as B6 does for every module; the course is then not
  complete and every module with a prerequisite locks again.
- **B8.** The learner gets a `completion_expired` notification (in-app and email, not required;
  `11` B10, `12` B8) naming the target and linking to it, when B5 step 3 emits the event.

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
  relative due rule, schedule its reminders (`13` B11), write `assignment_events`
  `cycle_started`, and reset the learner's course (B7), which archives their progress first
  (B15) (Q6). The reset is skipped when the person already has another open `assignment_users`
  row of the same course, from any assignment, whose `assigned_at` is later than their latest
  completion of the course: a retake for that other assignment is under way, and the new cycle
  joins it rather than clearing it. A person with two recurring assignments of one course (a
  direct one and a group one, or two `every_year_in` months) therefore resets once per retake.
  The reset runs under the learner's `course_progress` lock (`05` B6), as an expiry's
  does, so a completion and a cycle start take turns. A learner who had not finished the previous
  cycle starts the new one from the beginning too; what they had done is in the archive. "Each
  cycle is a new assignment" (plan) is this per-person row (Q4).
- **B10a.** A cycle start leaves `completions` alone: the previous cycle's completion stays valid
  until its own `expires_at`, so a certificate or a compliance check reads it as valid while the
  learner retakes the course. When that completion later expires, B5 step 2 finds a reset since
  and clears nothing, and step 4 finds the next cycle already started and starts none. A
  recurring assignment needs no validity: without one, nothing expires and only cycle starts reset
  progress.
- **B11.** With `every_months: 12`, `start_days_before: 30` and a 12-month validity, a learner who
  completes cycle 1 on 2026-03-10 starts cycle 2 on 2027-02-08, 30 days before the completion
  expires on 2027-03-10: their progress is archived and cleared then, cycle 2 is due
  `relative_days` later, and the cycle 1 completion stays valid until 2027-03-10. With
  `start_days_before: 0`, cycle 2 starts on the expiry date and the cycle start does the reset.
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

### Progress archives

- **B15.** Every reset (a cycle start, B10; a module or course expiry, B6, B7) first writes one
  `progress_archives` row for the learner, the course and the reset, with `reason` (`cycle_start`,
  `expiry` or `enrollment_deleted`), `target_type` and `target_id` (the course, or the expiring
  module), the `assignment_users` row that the cycle start closed (null otherwise), and
  `archived_at`. In the same transaction it copies every row the reset is about to delete into the
  archive tables, keyed by that archive: `item_progress`, `module_progress`, `course_progress` and
  `video_progress`, each with every stored column, its own `created_at` and `updated_at` included
  (as `live_created_at` and `live_updated_at`), and each item and module row with the item's or
  module's title at that moment, so the record reads the same after the content is renamed or
  deleted. `watch_reports` (idempotency keys, `09`) are not copied. A reset that finds no progress
  row writes no archive. Deleting an enrollment (`05` B15) archives the same way, with `reason`
  `enrollment_deleted` and the course as target. The copy and the delete commit together, so a reset
  never loses a row.
- **B16.** Archives, `completions`, `assignment_users`, `assignment_events` and `reminders_sent`
  are never pruned by any job and survive enrollment deletion (`05` B15): the owner's retention
  requirement (Q6) is that a school keeps a student's records until graduation plus two years,
  about six years, and Stepclimb does not guess a shorter time. The only way to erase them is M4's
  per-user data deletion (plan, Privacy). The email delivery status a timeline shows comes from
  the 90-day email log (`email.prune`, `12` B16) and is the one part of the history that ages
  out.
- **B17.** `GET /api/v1/courses/{course_id}/progress/{user_id}/archives` lists one learner's
  archives in the course, newest first, paginated (`02` B3): each with `id`, `user` (id and display
  name), `reason`, `target` (type, id and the title stored at reset time), `assignment` (id and
  cycle, or null), `archived_at`, and the archived course row's counts and `completed_at` when
  there is one. The learner is found while they have any enrollment in the course (active or
  concluded), any archive or any completion there, so a learner whose enrollment was deleted
  still lists; anyone else answers 404 `progress.not_found`.
- **B18.** `GET /api/v1/progress-archives/{archive_id}` returns one archive with its archived
  item, module, course and video rows, each with the title stored at reset time.
  `ProgressArchiveTarget` resolves `archive_id` to its course and then acts as `CourseTarget` with
  the code `progress_archive.not_found`.
- **B19.** Both routes need `perm::ProgressView` at the course, which an organization or account
  grant covers through the course's account path (`02` B23): an admin of the organization or of
  the course's account, or a course role holding `progress.view`, reads every learner's archives.
  Observers (`progress.view_observed`) and learners do not read archives (Q6). Deleting an
  enrollment keeps the learner's archives and completions and adds an `enrollment_deleted`
  archive of any live progress (`05` B15), so neither concluding nor deleting an enrollment loses
  history.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/completions` | `?user_id&target_type&cursor&limit` | 200 `Page<Completion>` | 404 `course.not_found` | `perm::CourseView` (course), then `progress.view` or `progress.view_observed` (`05` B20) |
| `GET /api/v1/me/completions` | `?course_id&cursor&limit` | 200 `Page<Completion>` | | authenticated |
| `GET /api/v1/courses/{course_id}/progress/{user_id}/archives` | `?cursor&limit` | 200 `Page<ProgressArchiveSummary>` | 404 `course.not_found`, 404 `progress.not_found`, 400 `pagination.invalid_cursor` | `perm::ProgressView` (course) |
| `GET /api/v1/progress-archives/{archive_id}` | | 200 `ProgressArchive` | 404 `progress_archive.not_found` | `perm::ProgressView` (the archive's course) |

Changed: `PATCH /api/v1/courses/{course_id}` takes `completion_valid_months` (`perm::CourseManage`);
`PATCH /api/v1/modules/{module_id}` takes `completion_valid_months`
(`perm::CourseContentEdit`); the assignment `POST` and `PATCH` (`13`) take `recurrence` (422
`assignment.recurrence_needs_course`, `assignment.recurrence_needs_relative_due`,
`request.invalid`). Schemas: `Completion { id, user: { id, display_name }, target: { type, id,
title }, cycle, completed_at, expires_at, expired_at }`; `Recurrence` as in B9;
`ProgressArchiveSummary { id, user: { id, display_name }, reason, target: { type, id, title },
assignment: { id, cycle } | null, archived_at, course: { required_count, completed_count,
completed_at } | null }`; `ProgressArchive` adds `modules: [{ module_id, title, required_count,
completed_count, completed_at }]`, `items: [{ item_id, module_id, title, state, score, started_at,
completed_at }]` and `videos: [{ item_id, watched_s, percent, duration_s, position_s,
last_report_at }]`. Titles are the ones stored at reset time.

## Data

Migrations `migrations/<version>_completions.sql` (I1), `migrations/<version>_progress_archives.sql`
(I4) and `migrations/<version>_recurrence.sql` (I3). Versions are placeholders, fixed when each
issue is implemented so each sorts after every migration on `next` and every open branch.

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

-- <version>_progress_archives.sql (not in the data model: B15, Q6)
CREATE TABLE progress_archives (
    id                  uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL,
    user_id             uuid        NOT NULL,
    course_id           uuid        NOT NULL,
    reason              text        NOT NULL
                                    CHECK (reason IN ('cycle_start', 'expiry', 'enrollment_deleted')),
    target_type         text        NOT NULL CHECK (target_type IN ('course', 'module')),
    target_id           uuid        NOT NULL,
    target_title        text        NOT NULL,                -- the title at reset time (B17)
    assignment_user_id  uuid,                                -- the closed cycle's row (B15)
    archived_at         timestamptz NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CHECK ((reason = 'cycle_start') = (assignment_user_id IS NOT NULL)),
    CHECK (reason <> 'enrollment_deleted' OR target_type = 'course')
);
CREATE INDEX progress_archives_by_learner
    ON progress_archives (org_id, course_id, user_id, archived_at DESC);

-- Each archived row keeps every column of the live row it copies; live_created_at and
-- live_updated_at are the live row's own timestamps, created_at and updated_at the copy's.
CREATE TABLE archived_item_progress (
    archive_id       uuid        NOT NULL,
    org_id           uuid        NOT NULL,
    item_id          uuid        NOT NULL,                   -- no foreign key: the item may go
    module_id        uuid        NOT NULL,
    title            text        NOT NULL,                   -- the item's title at reset time
    state            text        NOT NULL CHECK (state IN ('started', 'completed')),
    score            numeric,
    started_at       timestamptz NOT NULL,
    completed_at     timestamptz,
    live_created_at  timestamptz NOT NULL,
    live_updated_at  timestamptz NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (archive_id, item_id),
    FOREIGN KEY (org_id, archive_id) REFERENCES progress_archives (org_id, id) ON DELETE CASCADE
);

CREATE TABLE archived_module_progress (
    archive_id       uuid        NOT NULL,
    org_id           uuid        NOT NULL,
    module_id        uuid        NOT NULL,                   -- no foreign key, as above
    title            text        NOT NULL,                   -- the module's title at reset time
    required_count   integer     NOT NULL,
    completed_count  integer     NOT NULL,
    completed_at     timestamptz,
    live_created_at  timestamptz NOT NULL,
    live_updated_at  timestamptz NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (archive_id, module_id),
    FOREIGN KEY (org_id, archive_id) REFERENCES progress_archives (org_id, id) ON DELETE CASCADE
);

CREATE TABLE archived_course_progress (
    archive_id       uuid        PRIMARY KEY,
    org_id           uuid        NOT NULL,
    required_count   integer     NOT NULL,
    completed_count  integer     NOT NULL,
    completed_at     timestamptz,
    live_created_at  timestamptz NOT NULL,
    live_updated_at  timestamptz NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, archive_id) REFERENCES progress_archives (org_id, id) ON DELETE CASCADE
);

CREATE TABLE archived_video_progress (
    archive_id       uuid           NOT NULL,
    org_id           uuid           NOT NULL,
    item_id          uuid           NOT NULL,
    watched_ranges   int4multirange NOT NULL,
    duration_s       integer,
    percent          smallint       NOT NULL,
    position_s       integer        NOT NULL,
    last_report_at   timestamptz,
    live_created_at  timestamptz    NOT NULL,
    live_updated_at  timestamptz    NOT NULL,
    created_at       timestamptz    NOT NULL DEFAULT now(),
    updated_at       timestamptz    NOT NULL DEFAULT now(),
    PRIMARY KEY (archive_id, item_id),
    FOREIGN KEY (org_id, archive_id) REFERENCES progress_archives (org_id, id) ON DELETE CASCADE
);

-- <version>_recurrence.sql
ALTER TABLE assignment_users ADD COLUMN outcome text CHECK (outcome IN ('completed', 'incomplete'));
ALTER TABLE assignment_users ADD COLUMN closed_at timestamptz;
CREATE INDEX assignment_users_open ON assignment_users (assignment_id, user_id) WHERE closed_at IS NULL;
```

`target_id` has no foreign key (course or module by type); a module's deletion is already refused
while it has progress (`03` Q3), and its completions are kept as history when the module is later
deleted. `assignments.recurrence` (created by `13`) holds B9's JSON, validated by the typed
`Recurrence` in `stepclimb-core`. Spec 05's "a completion is final" (`05` B4, Q1) gains two
exceptions, expiry (stated in `05` by I2's PR) and a cycle start (by I3's PR).

The archive tables hold copies, so they have no foreign key to items or modules: deleting an item
or module later leaves its archived rows, and the reads show the title stored at reset time.
`assignment_user_id` has no foreign key because I4 does not depend on `13`'s migration;
`progress::archive_and_reset` checks that the row is in the organization and the course when it
is set. Repository functions (each takes `OrgScope`): `progress::archive_and_reset`
(B15, called by B6, B7 and B10), `progress_archives::{list_for_learner, get}`.

## Permissions

No new catalog entry: validity is set with `perm::CourseManage` or `perm::CourseContentEdit`,
recurrence with `perm::AssignmentManage` (`13`), completion history read with
`perm::ProgressView` or `progress.view_observed`, progress archives with `perm::ProgressView`
only (B19). Entitlements: none.

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
- **T5** (B9, B10, B10a, B11). `every_months: 12`, `start_days_before: 30`, `relative_days: 30`
  on a course valid 12 months: a completion on 2026-03-10 starts cycle 2 on 2027-02-08, due
  2027-03-10; at that start the learner's progress rows are gone, one `cycle_start` archive holds
  them, cycle 1 is closed `completed`, and the cycle 1 completion is still valid. On 2027-03-10
  the completion expires without a second reset or a third cycle. Reminders of cycle 2 follow
  `13`. A recurring assignment on a course without validity is accepted.
- **T6** (B9, B10). `every_year_in: 1`: on 1 January cycle 2 starts for an assignee who never
  completed cycle 1 (closed `incomplete`), and their partial progress moves to the archive; for one
  who completed cycle 1 in March, cycle 2 starts on 1 January as well.
- **T7** (B10, B15, Q6). Starting a cycle archives and then deletes the learner's
  `item_progress`, `module_progress`, `course_progress` and `video_progress` rows in the course,
  and another learner's rows are untouched. Mutation: skipping the archive step turns the test
  red, and so does skipping the delete.
- **T7a** (B15). A module expiry archives only that module's rows (`target_type` `module`,
  `reason` `expiry`); a course expiry after a cycle start finds the reset (a `cycle_start` archive
  with `archived_at` after the completion) and writes no archive; a module expiry after a course
  reset writes none either; a reset with no progress writes none. Each archived row equals the
  live row read just before the reset, column for column (the live `created_at` and `updated_at`
  as `live_created_at` and `live_updated_at`, `position_s` and `last_report_at` for video), with
  the item's and module's titles of that moment. Renaming the item afterwards leaves the archived
  title as it was.
- **T7e** (B5, B8). A learner who completes cycle 2 before cycle 1's completion expires gets no
  `completion.expired` event and no notification for cycle 1, whose `expired_at` is still set; a
  learner without a newer completion gets one of each.
- **T7f** (B10). A person holding a group assignment and a direct `every_year_in` assignment of
  the same course: the second cycle start, while the retake opened by the first is in progress,
  closes and opens rows as usual and leaves the learner's progress and archives unchanged.
  Mutation: dropping the "newer open row" check turns the test red.
- **T7b** (B16). Running every periodic job (`completions.expire`, `assignments.cycles`,
  `jobs.prune`, `email.prune`) with the manual clock moved 7 years ahead deletes no archive,
  completion, `assignment_users`, `assignment_events` or `reminders_sent` row.
- **T7c** (B17, B18, B19). An organization admin, an account admin of the course's account and an
  Instructor of the course (all holding `progress.view`) list a learner's archives newest first
  and read one with its item, module, course and video rows; a Student, the learner themself and
  an Observer of the learner get 403 `permission.denied`; a caller with every permission at a
  sibling account gets 404. After the archived item is deleted, the archive still reads, with the
  stored title. A user never enrolled answers 404 `progress.not_found`; a concluded learner's
  archives still list.
- **T7d** (B19, `05` B15). Deleting the learner's only Student enrollment while they have live
  progress writes one `enrollment_deleted` archive holding it and deletes the live rows; their
  earlier archives and their `completions` stay, and still list through B17 although they have
  no enrollment. Deleting an enrollment with no live progress writes no archive. Concluding an
  enrollment changes nothing. Mutation: deleting archives with the enrollment turns the test
  red.
- **T8** (B12). Completing the course after cycle 2 started completes cycle 2's row, not cycle 1's.
- **T9** (B3). Changing a course's validity leaves earlier `expires_at` values as they were.
- **T10** (B13). An Instructor lists a learner's history across cycles; the learner reads their
  own; another learner gets none of it.
- **T10a** (B13, `GET /courses/{id}/completions`). The route decides in the handler, so the
  permission suite has one entry for `progress.view` and one for `progress.view_observed`, each with
  its own cases. Explicit tests: a Student gets 403 `permission.denied`; a caller holding only
  `course.view` gets 403 `permission.denied`; an Observer sees only observed learners' rows.
- **T11** (routes). Four cases for `GET /courses/{id}/completions`, the two archive routes and the
  changed `PATCH` fields; `AuthOnly` cases for `/me/completions`.
- **T12** (routes, jobs). Cross-organization: org A's admin with org B's course, user or archive id
  gets 404;
  the expiry and cycle jobs, run with both organizations' rows due, change each organization's
  rows only within it.

## Out of scope

- Certificates and their expiry date: a later spec (plan, Progress and grades).
- The compliance report (compliant, due soon, overdue, expired): M4.
- Item-level expiry: not built (Q5).
- Cross-course prerequisites: not in the plan.
- A retention policy that deletes archives, completions and timelines after a configured time
  (for example graduation plus two years): a later concern. Until then nothing is pruned (B16).
- A learner reading their own archived progress, and Observers reading archives: not in M1 (Q6).
  Learners read their own completion history (B13).

## Open questions

Q1 to Q4 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q5 and Q6 narrowed or extended the plan, so they were the owner's; the
owner answered them on 2026-11-16.

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
- **Q5. Item-level expiry.** The plan says "completions can expire" without
  naming which. This spec builds course and module validity; leaving items out narrows the plan,
  which is the owner's call. Options: (a) course and module only: an item's completion is part of
  its module's, and a module of one item covers the case; (b) items too, with B6's reset applied to
  one item and sequential order re-locking what follows; (c) courses only. Recommendation: (a).
  Owner decision (2026-11-16): (a), course and module expiry only (B4).
- **Q6. Whether a new cycle clears progress before expiry.** The plan
  re-locks content only when a completion expires ("An expired completion re-locks anything that
  depends on it"), and `05` Q1 and `04` B4 make completion final apart from that. Clearing progress
  when a cycle starts would re-lock content before any expiry, which the plan does not say, so this
  is the owner's. Options: (a) a cycle start clears the learner's live progress in the course, so a
  retake can start before the old completion lapses (`start_days_before`), with history kept as
  completions per cycle and the assignment rows and timeline per cycle; (b) expiry only: a cycle
  starts no earlier than the expiry that clears progress, and recurrence requires a validity
  (B10a); (c) keep every progress row per cycle, adding a cycle column to spec 05's tables.
  Recommendation: (a), since compliance training is usually retaken before it lapses.
  Owner decision (2026-11-16): (a), a cycle start clears the learner's live progress, and the
  previous cycle's data must be kept and viewable by an admin or anyone holding the right
  permission. Schools keep student records until graduation plus two years, which can be about
  six years, so cycle history is never pruned automatically (B10, B15 to B19).
  Owner decision (2026-11-16), on retention: deleting an enrollment keeps history. It archives
  any live progress (reason `enrollment_deleted`, no row when there is none) and deletes only the
  live rows; `completions` and archives stay and join `05` B15's "Kept in every case" list.
  Erasing them is possible only through M4's per-user data deletion (B16, B19).
  Coordinator decisions (2026-11-16), pending owner review, for the points the answer leaves open:
  - The kept data is a progress archive per reset: one row naming the reset and copies of the
    item, module, course and video progress it cleared (B15), rather than a cycle column on spec
    05's live tables (option (c)), so spec 05's reads and locks stay as they are.
  - Expiry resets archive too, so no reset loses progress, whichever starts it (B6, B7).
  - "The right permission" is `progress.view` at the course, through an organization, account or
    course grant (B19). Observers and learners do not read archives in M1: the owner named admins
    and permission holders, and the archive holds more than the completion history Observers
    already read (B13).
  - Archives copy every live column and store titles at reset time, so a record read years
    later shows what the learner saw (B15).
  - "No reset since" is an archive of the course, or of the same module, dated at or after the
    completion (B5 step 2). An expiry with a newer completion of the same target sends no event
    or notification (B5 step 3).
  - A cycle start does not reset while another open cycle row of the course, newer than the
    person's latest completion, exists for them (B10).
  - A configurable retention policy is out of scope; nothing is pruned until one exists.

Coordinator decisions (2026-11-19, I1), pending owner review, for points the spec leaves open
(numbered as in PR #379):

1. `expires_at` (B13) is on module and course progress, the roll-ups whose completions are
   recorded. Item progress gains none: an item has no completion of its own (B4, Q5).
2. `Completion.target.title` is the course's or module's title now, and null for a module deleted
   since; a module's title at completion time is not stored (archives store titles, B15).
3. Coordinator decision (2026-11-19), after the security review of PR #379.
   `GET /courses/{course_id}/completions` leaves out completions of unpublished or deleted
   modules for a caller without `course.view_all`, as `05` B14's staff reads leave out
   unpublished content. It lists every user with a completion in the course, enrolled or not,
   suspended or not. This differs on purpose from `05` B14's progress list, which leaves out
   suspended users and non-learners: completions are history (B16), kept after enrollment
   deletion. `GET /me/completions` lists all of the caller's own rows, but answers
   `target.title` null where the caller does not hold `course.view_all` at the completion's
   course and either the target is a module that is unpublished or deleted, or the course is a
   draft or a template (for a module target too, since such a course is hidden whole, `02`
   B10). `05` B14 and `02` B10 hide those titles from learners.
4. Both lists run newest first by `completed_at`, then id.
5. A copy (`02` B12) takes the source's course and module `completion_valid_months`.
6. A validity outside 1 to 120 answers 422 `request.invalid` with the field code
   `completion.validity_out_of_range` at `/completion_valid_months`, on courses and modules.
7. Coordinator decision (2026-11-19): an API key with no scopes may call `GET /me/completions`,
   as it may call `GET /me`: it reads only the owner's own rows and changes nothing about how
   they sign in (`m0/08` B44 lists it).

## Issues

1. **I1. Completion history and validity.** Migration, recording completions in spec 05's roll-up
   transaction, validity on courses and modules, history routes, `expires_at` in reads, and
   `completions` added to `05` B15's "Kept in every case" list (with its T12 case). Covers B1
   to B4, B13; T1, T9 to T12 (routes). Depends on: `05-progress-and-events.md` I3,
   `02-courses-and-enrollments.md` I1.
2. **I2. Expiry and re-locking.** The `completions.expire` job, module and course resets through
   I4's `progress::archive_and_reset`, the `completion.expired` event and notification, and the
   texts that state completion is final, updated to name expiry as an exception: `05` B4, `05` B10
   (events once per learner and target become once per cycle) and `04` B4, the "no reset since"
   test and the skip for a newer completion. Covers B5 to B8; T2, T3, T7a (expiry), T7e, T12
   (jobs). Depends on: I1, I4, `04-unlock-and-completion-rules.md` I3,
   `11-announcements-and-in-app-notifications.md` I2, `12-email.md` I3.
3. **I3. Recurrence and cycles.** Migration, recurrence validation (no validity needed), the
   `assignments.cycles` job, cycle start and closing with the course reset through
   `progress::archive_and_reset`, cycles started by expiry, timeline entries, and `05` B4 and `04`
   B4 updated to name a cycle start as the second exception, and the rule for several recurring
   assignments of one course. Covers B9 to B12, B14; T4 to T8, T7f.
   Depends on: I2, `13-assignments-due-dates-reminders-and-escalation.md` I4.
4. **I4. Progress archives.** Migration `<version>_progress_archives.sql`,
   `progress::archive_and_reset` (archive, then delete, in one transaction under the learner's
   course lock, with titles and every live column), the two read routes with
   `ProgressArchiveTarget`, the `enrollment_deleted` archive in `05` B15's enrollment deletion,
   and the no-prune test. Covers B15 to B19; T7a (copy), T7b to T7d, T11 and
   T12 for these routes. Depends on: I1, `05-progress-and-events.md` I3,
   `09-linked-video-and-watch-tracking.md` I2 (`video_progress`). Maintainer line-by-line
   review (a new read of learner data).
