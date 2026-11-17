# Unlock and completion rules

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Goals (courses made of modules that unlock by
  prerequisites), Courses and content (Module: unlock date, prerequisite modules, optional
  sequential completion, completion rule; Item: requirement), Data model (`modules`,
  `module_prerequisites`, `module_items.requirement`), Security and privacy; `docs/plan/ui.md` ->
  Principles (state is always visible), Signature components (ProgressRail: locked items explain
  what unlocks them), Key layouts (course home: "Unlocks after Ground control")
- **Depends on:** `03-modules-and-items.md` (modules, items, publishing),
  `02-courses-and-enrollments.md` (`course.view_all`, archived courses),
  `05-progress-and-events.md` (the progress the rules read; see Issues for the order)
- **Review:** maintainer line-by-line (what content reaches a learner is decided here)

## Summary

This spec decides, on the server, when a learner may open a module or an item and what counts as
finishing it. A module can wait for a date, for other modules to be complete, or both; a module
can make its items go in order; each item can carry a requirement (view it, mark it done, watch
at least X% of a video, submit it, score at least X, or reach an activity's mastery rule); and a
module's completion rule says whether all or one of its requirements finish it. Lock state is
computed for each request from the rules, the learner's progress and the clock. A locked module or
item shows its title and what it is waiting for, and nothing else: no reference, URL or content
reaches the client.

## Behavior

### Rules on modules

- **B1.** A module has `unlock_at` (optional), a set of prerequisite modules
  (`module_prerequisites`), `require_sequential` (default false) and `completion_rule` (`all`, the
  default, or `one`; Q1). They are changed through `PATCH /api/v1/modules/{module_id}` by a holder
  of `perm::CourseContentEdit`, with `03` B2's archived refusal.
- **B2.** Prerequisites are modules of the same course (404 `module.not_found` otherwise), never
  the module itself, at most 20 per module, and never form a cycle (409 `module.prerequisite_cycle`;
  Q2). The write replaces the whole set, locks the course row (`03` B12) and checks for a cycle
  over the course's graph under that lock. Deleting a module (`03` B3) removes the prerequisite
  rows that name it.

### Requirements on items

- **B3.** An item has at most one `requirement`, a JSON object set through `PATCH
  /api/v1/items/{item_id}` (`null` removes it). The kinds and the item kinds they apply to:

  | Requirement | Shape | Item kinds | Met when |
  | --- | --- | --- | --- |
  | View | `{ "kind": "view" }` | page, file, external link, video | the learner opens it (spec 05 records the view) |
  | Mark done | `{ "kind": "mark_done" }` | page, file, external link, video | the learner marks it done |
  | Watch | `{ "kind": "watch", "min_percent": 1..100 }` | video | the learner's watched ranges cover at least that share of the video (spec 09) |
  | Submit | `{ "kind": "submit" }` | quiz | reserved for M2 |
  | Score | `{ "kind": "score", ... }` | quiz | reserved for M2, which defines the threshold |
  | Mastery | `{ "kind": "mastery" }` | activity | reserved for M3: the activity's mastery rule |

  A requirement on a header, a kind the item does not take, an unknown member, or `min_percent`
  outside 1 to 100 answers 422 `request.invalid` (`requirement.invalid` at `/requirement`).
  `submit`, `score` and `mastery` answer 422 `requirement.unavailable` until M2 and M3 define
  them; the names are reserved so the column needs no migration then.
- **B4.** A requirement is met once and stays met (spec 05 Q1): the item's progress row records
  it as completed, and later changes to the requirement do not undo it.

### Who the rules apply to

- **B5.** Lock state is computed for the caller when the caller lacks `perm::CourseViewAll` at
  the course. A holder of `CourseViewAll` (authors, instructors, admins) sees every module and
  item unlocked, with the rules shown as data so the editor can display them; they never record
  progress unless also enrolled with `course.participate`, in which case the learner view is what
  their progress routes see (spec 05) but reads still show everything.
- **B6.** Unpublished modules and items take no part in any rule (Q3): an unpublished prerequisite
  counts as met, an unpublished item never blocks sequential order, and neither counts toward
  completion. A learner never sees them (`03` B13).

### Evaluating locks

- **B7.** `stepclimb_core::unlock::evaluate` is a pure function from the course's published rule
  data, the learner's progress (the set of completed item ids, spec 05) and the time from the
  request's `Clock`, to each module's and item's state: `locked` with its reasons, or `available`.
  It reads no database and no clock of its own, so it is tested with plain values.
- **B8.** A module is locked when any of these holds, each a reason:
  - `unlock_at` is set and later than now: `{ "kind": "unlock_at", "at": <time> }`;
  - a published prerequisite is not satisfied: `{ "kind": "prerequisites", "module_ids": [...] }`
    (the unsatisfied ones, in course order).
  A module is satisfied when it is complete (B11) or, having no published requirement, when it is
  itself unlocked (Q4). A module's reasons list every condition that holds, not only the first.
- **B9.** An item is locked when its module is locked (`{ "kind": "module_locked" }`) or, in a
  module with `require_sequential`, when an earlier published item with a requirement is not yet
  complete: `{ "kind": "sequential", "item_id": <the first such item> }`. An item without a
  requirement in a sequential module is locked until every earlier requirement is met, and never
  blocks a later item itself.
- **B10.** `unlock_at` is compared with the request clock's time at each request; nothing is
  stored when a date passes, so no job runs to unlock a module.

### Completion

- **B11.** An item is complete when its progress row is `completed` (spec 05). A module is
  complete when its published items with requirements are complete: all of them under `all`, at
  least one under `one`. A module with no published requirement is never complete, never emits
  `module.completed`, and is not counted toward the course (Q4). A course is complete when every
  published module that has a published requirement is complete; a course with none never
  completes. Spec 05 stores these and emits the events; this spec defines them, in
  `stepclimb_core::unlock` beside B7.

### What a locked module or item shows

- **B12.** To a caller under B5, a locked module shows `id`, `title`, `position`, its items'
  `id`, `kind`, `title`, `position` and `requirement`, and `lock: { reasons: [...] }` (Q5). A
  locked item shows the same item fields and its reasons. Neither shows `ref_id` or `url`, and no
  content route answers for it.
- **B13.** Every route that hands a learner an item's content or accepts progress for it checks
  the lock first, with the same evaluator, and answers 403 `item.locked` (with the reasons in the
  problem's `reasons` member) when it is locked: spec 05's progress route, spec 06's file URL,
  spec 07's page body, spec 08's video session (`POST /api/v1/items/{item_id}/video-session`),
  spec 09's watch reports, and M2 and M3's starts of an
  attempt or session. `GET /api/v1/items/{item_id}` answers 200 with B12's fields, since a learner
  may open a locked item to read why. A test sweeps the OpenAPI document for every route whose
  path holds `item_id` or a content id and checks it is in the lock suite's table (T13), either
  as a lock-checked route or as a staff-only route that requires `perm::CourseViewAll`, so a
  later content route cannot skip the check.
- **B14.** An unlocked item and module carry `lock: null`. Reasons reveal only what the learner
  could already see: a prerequisite's id names a published module in the same response, and a
  sequential reason names a published item before it.

## API

Changed routes from `03-modules-and-items.md`; errors as there.

| Method and path | Change | Errors added | Permission |
| --- | --- | --- | --- |
| `PATCH /api/v1/modules/{module_id}` | adds `unlock_at?` (nullable), `prerequisite_ids?: uuid[]`, `require_sequential?`, `completion_rule?` | 404 `module.not_found` (a prerequisite), 409 `module.prerequisite_cycle`, 422 `request.invalid` | `perm::CourseContentEdit` (the module's course) |
| `PATCH /api/v1/items/{item_id}` | adds `requirement?` (nullable) | 422 `request.invalid` (`requirement.invalid`, `requirement.unavailable`) | `perm::CourseContentEdit` (the item's course) |
| `GET /api/v1/courses/{course_id}/modules`, `GET /api/v1/modules/{module_id}`, `GET /api/v1/items/{item_id}` | add `lock` and, for authors, the rule fields; withhold B12's fields when locked | none | `perm::CourseView` (course) |
| Content and progress routes of specs 05 to 09, M2, M3 | check the lock (B13) | 403 `item.locked` | their own |

Schemas: `Module` gains `unlock_at`, `prerequisite_ids`, `require_sequential`, `completion_rule`
and `lock: Lock | null`; `Item` gains `requirement: Requirement | null` and `lock: Lock | null`;
`Lock { reasons: LockReason[] }`; `LockReason` is one of `{ kind: "unlock_at", at }`, `{ kind:
"prerequisites", module_ids }`, `{ kind: "module_locked" }`, `{ kind: "sequential", item_id }`;
`Requirement` is the tagged union of B3. Learners see `prerequisite_ids` and the rule fields too:
they are needed to draw ProgressRail's "unlocks after" and are not secret.

## Data

Migration `migrations/<version>_unlock_rules.sql` (I1). The version is a placeholder, fixed when
the issue is implemented so it sorts after every migration on `next` and every open branch.

```sql
ALTER TABLE modules
    ADD COLUMN unlock_at          timestamptz,
    ADD COLUMN require_sequential boolean NOT NULL DEFAULT false,
    ADD COLUMN completion_rule    text    NOT NULL DEFAULT 'all' CHECK (completion_rule IN ('all', 'one'));

ALTER TABLE module_items
    ADD COLUMN requirement jsonb CHECK (requirement IS NULL OR (jsonb_typeof(requirement) = 'object' AND requirement ? 'kind')),
    ADD CONSTRAINT module_items_header_no_requirement CHECK (kind <> 'header' OR requirement IS NULL);

CREATE TABLE module_prerequisites (
    org_id       uuid        NOT NULL,
    course_id    uuid        NOT NULL,
    module_id    uuid        NOT NULL,
    requires_id  uuid        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (module_id, requires_id),
    FOREIGN KEY (org_id, course_id, module_id)   REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id, requires_id) REFERENCES modules (org_id, course_id, id) ON DELETE CASCADE,
    CHECK (module_id <> requires_id)
);
CREATE INDEX module_prerequisites_requires ON module_prerequisites (requires_id);
```

`org_id` and `course_id` on `module_prerequisites` (the data model lists `module_id, requires_id`)
let the two foreign keys hold both modules to one course of one organization. The cascade backs
B2's removal when a module is deleted; cycles are checked by the repository under the course lock
(B2), which SQL cannot express. Requirement shapes are validated by the typed `Requirement` in
`stepclimb-core`; the column check only refuses a non-object.

Repository functions: `modules::set_rules`, `items::set_requirement`, and
`unlock::load_course_rules(scope, course_id)`, one query returning the published rule data that
`evaluate` takes. `courses::copy` maps prerequisite ids to the copied modules.

## Permissions

No new catalog entry. Rule writes take `perm::CourseContentEdit`; `perm::CourseViewAll` decides
whether the lock applies (B5). Entitlements: none.

## Test cases

Evaluator tests (T1 to T8) are unit tests over plain values; route tests use `#[sqlx::test]`, a
`ManualClock` and the permission suite.

- **T1** (B8, B10). A module with `unlock_at` at 10:00 is locked at 09:59:59 with reason
  `unlock_at` and available at 10:00:00 (the boundary is inclusive).
- **T2** (B8). Module C requires A and B; with A complete it is locked with `module_ids: [B]`;
  with both complete it is available. With `unlock_at` in the future as well, both reasons are
  listed.
- **T3** (B8, Q4). A module with no requirements is satisfied as a prerequisite when it is itself
  unlocked, and not while its own `unlock_at` is in the future.
- **T4** (B9). In a sequential module with items [req1, plain, req2], with nothing done: req1
  available, plain and req2 locked by `sequential` naming req1; after req1 completes, all three
  available. In a module without `require_sequential`, all three are available.
- **T5** (B6). An unpublished prerequisite is met; an unpublished required item neither blocks
  sequential order nor counts toward `all`.
- **T6** (B11). Under `all`, a module with two requirements is complete only with both; under
  `one`, with either. A course with modules [requirements, none] is complete when the first is.
  A course with no requirement anywhere never completes.
- **T7** (B9). A locked module's items are locked with `module_locked` whatever their own state.
- **T8** (B7). `evaluate` is deterministic: the same inputs give the same output across 1,000
  random courses (a proptest over generated module graphs and progress sets), and every item whose
  module is locked is locked.
- **T9** (B2). A cycle A -> B -> A answers 409 `module.prerequisite_cycle`; a module requiring
  itself answers 422; a module of another course answers 404; 21 prerequisites answer 422. Two
  concurrent writes that each close half a cycle end with one 409 (two connections).
- **T10** (B3). Each row of B3's table: an allowed pair is stored; a requirement on a header, a
  `watch` on a page, `min_percent` 0 or 101, and an unknown member answer 422
  `requirement.invalid`; `submit`, `score` and `mastery` answer 422 `requirement.unavailable`.
- **T11** (B12, B14). A Student reading a course whose second module is locked gets that module's
  title, its items' titles and requirements, and its reasons, and the response text contains
  neither the items' `ref_id` values nor an external link's URL (searched in the raw body). The
  same read by a Course designer contains both and `lock: null`.
- **T12** (B13). A Student's `GET /api/v1/items/{id}` on a locked external link answers 200
  without `url`.
- **T13** (B13). The lock suite: for every route the OpenAPI walker finds with an `item_id`,
  `page_id`, `file_id`, `video_id` (and later quiz and activity ids) in its path, a Student whose
  item is locked gets 403 `item.locked` (or the B12 fields for `GET /items/{id}`), or, for a
  route listed as staff-only, 403 `permission.denied`; an unlisted such route fails the test,
  naming it. Mutation: removing
  the lock check from spec 05's progress route turns this test red.
- **T14** (B5). A user enrolled as Student who also holds `CourseViewAll` through an account
  assignment reads every item unlocked.
- **T15** (rule routes). Four cases and cross-organization for the changed `PATCH` routes, as in
  `03` T12 and T13, including a prerequisite id from org B (404) and a module of org B (404).

## Out of scope

- Storing progress and emitting completion events: spec 05.
- Watch tracking: spec 09. Quiz submit and score: M2. Activity mastery: M3.
- Expiry that re-locks dependent content (plan: an expired completion "re-locks anything that
  depends on it"): spec 14, which clears the expired progress so B8's `prerequisites` and B9's
  `sequential` reasons apply again; no new reason kind.
- Per-learner overrides of unlock dates and accommodations: M2 (quizzes) and not in the plan for
  modules.
- Prerequisites across courses: not in the plan.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. What a module's "completion rule" can be.** The plan names the rule and no values.
  Options: (a) `all` requirements (default) or `one`; (b) `all` only; (c) a count or percentage
  of requirements. Recommendation: (a), the two rules Canvas offers, a single column.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Which modules may be prerequisites.** Options: (a) any module of the same course, as long
  as the graph has no cycle; (b) only modules positioned earlier, as Canvas does, which then
  constrains reordering; (c) any module, cycles allowed (a cycle locks forever). Recommendation:
  (a); the cycle check runs once per write over at most 200 modules.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Unpublished content and rules.** Options: (a) unpublished modules and items take no part
  (an unpublished prerequisite is met, an unpublished requirement does not count); (b) they count,
  so a learner can be blocked by something they cannot see; (c) publishing is refused while
  something depends on unpublished content. Recommendation: (a). A learner is never blocked by a
  thing they cannot see; the author's choice to unpublish decides.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Modules without requirements.** The plan does not say when such a module is "complete".
  Options: (a) it is never complete and is left out of course completion, but satisfies a
  prerequisite once it is itself unlocked; (b) it is complete as soon as it is unlocked, and emits
  `module.completed` then, which needs a job when an unlock date passes; (c) prerequisites on it
  are refused. Recommendation: (a). It needs no job, emits no event no one earned, and a reading
  module placed before a graded one does not hold the learner back.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. How much a locked module shows.** The plan's course home shows a locked module's title and
  "Unlocks after Ground control", and ProgressRail keeps locked items focusable to explain their
  requirement. Options: (a) titles, kinds and requirements of the module and its items, plus the
  reasons, and no references or content; (b) only the module title and reasons; (c) everything
  but the content. Recommendation: (a), what the plan's layouts draw and nothing more.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

### Decisions made building I1

Coordinator decisions (overnight, 2026-11-17), pending owner review.

- **Copy mapping.** `courses::copy` (`02` B12, B13) copies each module's `unlock_at`,
  `require_sequential` and `completion_rule`, each item's `requirement`, and every
  `module_prerequisites` row with both ids mapped to the copied modules, in the copy's
  transaction. A row whose modules the copy cannot map fails the copy rather than being
  dropped.
- **Author reads are the write responses.** The `GET` routes are `03` I2. Until then, every
  route that answers a `Module` or `Item` (`03`'s create, patch, reorder and move) carries the
  rule fields: `unlock_at`, `prerequisite_ids` (in course order), `require_sequential`,
  `completion_rule`, and `requirement` (`null` when there is none). `lock` arrives with I3.
- **Field codes** besides the spec's: 422 `module.prerequisite_self`,
  `module.prerequisite_duplicate` (an id named twice) and `module.prerequisites_too_many` at
  `/prerequisite_ids`; 422 `module.date_out_of_range` at `/unlock_at` for a date before 1970
  or from 2200 on, the course dates' range (`02`). A prerequisite of another course or
  organization answers 404 `module.not_found`, as B2 says.
- **Order of the requirement checks.** The shape is checked first, so `submit`, `score` and
  `mastery` answer `requirement.unavailable` on any item; then the item's kind, so `view` on a
  header answers `requirement.invalid`. A `requirement` that is not a JSON object (a string,
  an array) answers 400 `request.malformed`, as any other non-object in a body does. A
  requirement is set only through `PATCH`, as the API table lists, not on item create.
- **The course lock needs `READ COMMITTED`.** Every module and item write (`03` and these
  rules) takes the course lock through one function, which refuses a transaction at any other
  isolation level (a 500 to the route), as the account tree lock and the reporting-line lock
  do: at `REPEATABLE READ` a write would read positions and prerequisites from the snapshot
  taken before it waited.
- **`unlock_at` is kept to the microsecond**, PostgreSQL's resolution; a date sent with more
  digits is truncated before it is compared and stored.
- **No-op writes.** A rule patch naming the current values, or a prerequisite list equal to the
  current set in any order, writes nothing; any rule change sets `updated_at`. Rule changes are
  not audited (`02` B27: content edits are not).

### Decisions made building I2

Coordinator decisions (overnight, 2026-11-16), pending owner review.

- **Progress is an argument.** `evaluate` and `unlock::require_unlocked` take the learner's
  completed item ids as a set; spec 05 I1 reads them from its progress rows and passes them in.
  An item is complete when it is in the set, whatever its requirement now is (B4); only items
  with a published requirement count toward a module's completion.
- **`require_unlocked` always applies the lock.** Whether a caller is held to it is the
  route's decision (B5): content reads skip it for a holder of `perm::CourseViewAll`, and
  progress routes call it for every caller who records progress.
- **An item the learner cannot see answers `item.not_found`.** `require_unlocked` refuses an
  unpublished item, an item of an unpublished module, and another organization's item as not
  found (404), as `03` B13 does for reads.
- **A locked module's items carry `module_locked` alone** (T7), not also their `sequential`
  reason.
- **`reasons` is a member of the shared problem body**, beside `errors`, `limit` and `max`, so
  every content and progress route answers `item.locked` without a problem schema of its own.
- **Fail closed on bad stored data.** A prerequisite cycle, which the write path refuses,
  leaves every module on it locked if one is ever stored; a stored requirement this release
  cannot read is a server error, never a missing requirement.

## Issues

Order across specs 04 and 05: 04 I1, 04 I2, 05 I1, 04 I3, 05 I2 and on.

1. **I1. Rule data and authoring.** Migration, module rule fields, prerequisites with the cycle
   check, item requirements with the typed `Requirement`, copy mapping, author reads showing the
   rules. Covers B1 to B3; T9, T10, T15. Depends on: `03-modules-and-items.md` I1.
2. **I2. The evaluator.** `stepclimb_core::unlock::{load_course_rules, evaluate}` and the
   completion definitions, with unit and property tests, plus the `item.locked` problem (403 with
   `reasons`) and `unlock::require_unlocked`, the check every content and progress route calls, so
   `05-progress-and-events.md` I1 can use it before I3. Covers B4, B6 to B11, B13 (the problem and
   the helper); T1 to T8. Depends on: I1.
3. **I3. Enforcement in learner reads and the lock suite.** `lock` on module and item reads,
   withholding of references and URLs, the `CourseViewAll` bypass, and
   the OpenAPI-driven lock suite that later content routes join. Covers B5, B12 to B14; T11 to
   T14. Depends on: I2; `05-progress-and-events.md` I1 (the progress the evaluator reads, and the
   first route the suite covers).
