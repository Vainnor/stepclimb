# Courses and enrollments

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Course), Tenancy and structure,
  Identity and access (Permissions and roles, Provisioning: enrollment rules, sources), Data model
  (`courses`, `enrollments`, `role_assignments`), API surface (`/api/v1/courses`), Milestones (M1)
- **Depends on:** `../m0/02-organizations-and-accounts.md` (accounts, `OrgScope`, API
  conventions, B21's account deletion), `../m0/03-users-and-identities.md` (users, `Source`,
  `UserView` at account scope), `../m0/09-permissions-and-roles.md` (catalog, built-in roles,
  `Require<P, T>`, grants, the permission suite, audit entries)
- **Review:** maintainer line-by-line (permissions and course-scope grants)

## Summary

A course is what learners take: it belongs to one account, has a title, a code, a status (draft,
published or archived), dates and a home page, and can be copied, including as a template. An
enrollment puts a user in a course with a role (Student, Instructor, Course designer or a custom
role); it is the course-scope grant the M0 permission model left for this spec. This spec adds the
course permissions to the catalog, gives the built-in roles their M1 lists, adds `CourseTarget`
to `Require`, and makes the evaluator read enrollments. Modules and items are spec 03.

## Behavior

### Courses

- **B1.** A course is one `courses` row in one account (`account_id`) of the organization. It has
  `title` (trimmed, 1 to 200 characters), `code` (Q2), `status` (`draft`, `published`,
  `archived`), `template` (Q5), `start_at` and `end_at` (both optional; Q3), and `settings` (a
  JSON object read and written only through a typed `CourseSettings`, whose one M1 key is `home`,
  B6). A new course is `draft`.
- **B2.** Creating a course names `account_id`, `title` and `code`, and needs
  `perm::CourseCreate` at that account. The course is inserted under the organization's account
  tree lock (`02` B22, `accounts::lock_tree`), after the account's path is read again under the
  lock and the check repeated, so a move committed after `Require` cannot place the course outside
  the caller's reach.
- **B3.** `PATCH /api/v1/courses/{course_id}` changes `title`, `code`, `start_at`, `end_at` and
  `settings.home`, and needs `perm::CourseManage` at the course. It follows `02` B13's patch
  rules: an absent member keeps its value, `null` is accepted only for `start_at` and `end_at`
  (clearing them) and is 400 `request.malformed` elsewhere, and a patch naming only current
  values writes nothing (`09` B22). `end_at` before `start_at` answers 422 `request.invalid`
  (`course.dates_inverted` at `/end_at`).
- **B4.** Status changes go through `POST /api/v1/courses/{course_id}/status` with
  `{ "status": ... }` and need `perm::CourseManage`: draft to published, published to archived,
  archived to published, and published back to draft only while the course has no enrollment
  whose role holds `course.participate` (409 `course.has_learners`). Any other change, including
  to the current status, answers 409 `course.status_invalid` (the current status answers 200 and
  writes nothing). A template cannot be published (409 `course.template`, Q5).
- **B5.** An archived course is read-only (Q4): its content routes (spec 03 and later) answer 409
  `course.archived` to every write, progress is not recorded (spec 05), and learners can still
  read what they could read before archiving. `PATCH` and the status route still work on it.
- **B6.** `settings.home` says what a learner sees first when opening the course:
  `{ "kind": "modules" }` (the module list, the default). Spec 07 adds `{ "kind": "page",
  "page_id": ... }` (Q6).
- **B7.** A course's cover is an image from its files; spec 06 adds `cover_file_id` and its
  route, since files do not exist before spec 06.
- **B8.** Courses are not deleted in M1 (Q7): a course leaves use by being archived. An account
  that holds a course refuses deletion with 409 `account.has_courses`, the check `02` B21 left to
  this spec; it runs under the same tree lock as `02` B21's other checks.
- **B9.** A course stays in its account; moving a course between accounts is not in the plan and
  not built. An account moved by `02` B20 carries its courses with it, since a course's scope
  path is read through its account at every request.

### Reading and listing courses

- **B10.** `GET /api/v1/courses/{course_id}` returns the course to a caller holding
  `perm::CourseView` at the course. A `draft` course, and any template, is visible only to a
  caller who also holds `perm::CourseViewAll` there; to anyone else it answers 404
  `course.not_found`, as for a course that does not exist. The response carries `permissions`,
  the catalog keys effective for the caller at the course, so the web app can show or hide edit
  controls; the server checks every request itself.
- **B11.** `GET /api/v1/courses` lists the courses where the caller holds `CourseView`, through
  `grants.scopes_with::<CourseView>()` (organization, account subtrees and courses), with B10's
  draft rule, sorted by `title` then `id`, with `02` B3 pagination. Filters: `account_id` (that
  account's subtree; 404 `account.not_found` when not visible), `status` (one value) and
  `template` (boolean). Learners find their courses here; spec 10's dashboard builds on it.

### Copying

- **B12.** `POST /api/v1/courses/{course_id}/copy` with `{ "account_id", "title", "code",
  "template"? }` creates a new `draft` course in the named account, copying the source's `title`
  (unless given), dates, `settings` and content. It needs `perm::CourseViewAll` at the source (a
  copy carries everything, unpublished and locked content included) and `perm::CourseCreate` at
  the destination account, checked under the tree lock as in B2. `template: true` makes the copy a
  template; copying a template with `template` absent or `false` makes an ordinary course ("create
  from template").
- **B13.** A copy never copies enrollments, progress, attempts or any learner data, and never the
  source's id-bound history (page versions other than the current one, spec 07). Each content
  spec adds its part to the one copy function, `courses::copy`, which runs in one transaction:
  spec 03 modules and items (with prerequisites remapped to the new modules, spec 04), spec 06
  course files (new `course_files` rows pointing at the same stored files), spec 07 pages, and
  M2 and M3 quizzes and activities. In this spec the copy holds the course row only.
- **B14.** A copy is limited by spec 03's sizes (200 modules, 200 items per module), so it fits
  one transaction; it is synchronous and answers 201 with the new course.

### Enrollments

- **B15.** An enrollment is one `enrollments` row: a user, a course, a role and a `status`
  (`active` or `concluded`), with its `source` and optional `external_id` (`03` B4). The same
  `(course, user, role)` exists at most once; a user may hold several roles in one course.
- **B16.** An `active` enrollment grants its role's permissions at the course's scope (Q1); a
  `concluded` enrollment grants nothing and is kept as history (Q8). A permission whose
  `GRANTABLE` list does not include `course` has no effect through an enrollment (`09` B3).
- **B17.** Creating an enrollment names `user_id` and `role_id` and needs
  `perm::EnrollmentManage` at the course, then runs `09` B12's checks with the course as the
  scope, in order: the user exists in the organization (404 `user.not_found`); the role is
  built-in or the organization's (404 `role.not_found`); the role is not Manager or Observer (422
  `request.invalid`, `enrollment.role_unavailable` at `/role_id`, Q9); a built-in role this binary
  does not define is refused (409 `role.unknown_builtin`); the caller holds at the course every
  catalog permission of the role (403 `permission.escalation`); the enrollment does not exist (409
  `enrollment.exists`). A template course takes no enrollment whose role holds
  `course.participate` (409 `course.template`), and an archived course takes no new enrollment
  (409 `course.archived`, B5). Enrollments made through the API have source `local`.
- **B18.** `PATCH /api/v1/enrollments/{enrollment_id}` with `{ "status" }` concludes or
  reactivates an enrollment, needing what creating it would (B17's permission and escalation
  check). `DELETE` removes the enrollment with the same checks; from spec 05 it also deletes the
  user's progress in the course in the same transaction (Q8). An enrollment whose `source` is not
  `local` is read-only (409 `enrollment.externally_managed`), as `09` B14 rules for assignments.
- **B19.** Enrollment writes lock the course row (`FOR NO KEY UPDATE`) and the role row (`FOR
  KEY SHARE`, as `09` B12's decision does for assignments), so a role edit or a status change to
  draft cannot interleave with the check.
- **B20.** `GET /api/v1/courses/{course_id}/enrollments` lists a course's enrollments to a holder
  of `perm::EnrollmentView` at the course, with `?status&role_id&user_id&cursor&limit`, sorted by
  the user's `display_name`, then the enrollment id. Each item carries the user's id and display
  name (the minimum a roster needs; email needs `UserView`).
- **B21.** A user with an enrollment in a course inside account A counts as "in" A for
  `UserView` at account scope (`09` Q2 (B), `03`): an account admin of A finds them through
  `GET /api/v1/users`. The users list query adds the enrollment join.
- **B22.** Role assignments at course scope stay refused (`09` B12 step 3,
  `role_assignment.scope_unavailable`): enrollments are the one way to grant a role at a course
  (Q1). `09` B12's text is updated in the same PR as issue I2.

### Evaluating course permissions

- **B23.** `Grants::load` reads, in its one query, the user's role assignments (as now) and their
  `active` enrollments in the scope's organization, each with its course's account path. A course
  grant is checked at `Target::Course { account_path, course_id }`: a permission is effective
  there when it is effective at the organization, at any account on the path, or through an
  enrollment in that course. An enrollment whose course is not in the organization fails the load,
  as `09` B16 rules for assignments.
- **B24.** `CourseTarget` resolves the path parameter `course_id` inside the principal's
  organization to its account path, then applies `09` B17 steps 3 to 5 with code
  `course.not_found`. A course is visible to a principal holding any effective permission at it,
  so a learner who is refused an edit sees 403, while a user with no grant there sees 404.
- **B25.** API keys: a key's scopes are intersected with its owner's grants as now (`08` B27);
  enrollment grants are part of the owner's grants.
- **B26.** `GET /api/v1/me` keeps listing organization and account grants (`09` API); course
  grants are read from each course's `permissions` (B10) instead, since a learner may hold
  hundreds.

### Audit

- **B27.** These changes write one `audit_events` row each, in their transaction (`09` B22):
  `course.create` (`after`: account, title, code, template), `course.update` (`before` and
  `after` of each changed member), `course.set_status` (`before` and `after`), `course.copy`
  (on the new course; `after`: the source id, account, template), `enrollment.create` (`after`:
  user, role, status, source), `enrollment.update` (`before` and `after`: status) and
  `enrollment.delete` (`before`: user, role, status). Content edits in later specs are not
  audited (pages keep versions, spec 07).

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API. Unsafe methods also answer
`08`'s CSRF refusals. Every route is in the OpenAPI document.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses` | `?account_id&status&template&cursor&limit` | 200 `Page<Course>` | 400 `pagination.invalid_cursor`, 404 `account.not_found` | `perm::CourseView` (any scope; filtered, B11) |
| `POST /api/v1/courses` | `{ account_id, title, code }` | 201 `Course`, `Location` | 404 `account.not_found`, 409 `course.code_taken`, 422 `request.invalid` | `perm::CourseCreate` (the account in the body) |
| `GET /api/v1/courses/{course_id}` | | 200 `Course` | 404 `course.not_found` | `perm::CourseView` (course; drafts need `CourseViewAll`, B10) |
| `PATCH /api/v1/courses/{course_id}` | `{ title?, code?, start_at?, end_at?, settings?: { home? } }` | 200 `Course` | 404 `course.not_found`, 409 `course.code_taken`, 422 `request.invalid` | `perm::CourseManage` (course) |
| `POST /api/v1/courses/{course_id}/status` | `{ status }` | 200 `Course` | 404 `course.not_found`, 409 `course.status_invalid`, 409 `course.has_learners`, 409 `course.template` | `perm::CourseManage` (course) |
| `POST /api/v1/courses/{course_id}/copy` | `{ account_id, title?, code, template? }` | 201 `Course`, `Location` | 404 `course.not_found`, 404 `account.not_found`, 409 `course.code_taken`, 422 `request.invalid` | `perm::CourseViewAll` (course) and `perm::CourseCreate` (the account in the body) |
| `GET /api/v1/courses/{course_id}/enrollments` | `?status&role_id&user_id&cursor&limit` | 200 `Page<Enrollment>` | 404 `course.not_found`, 400 `pagination.invalid_cursor` | `perm::EnrollmentView` (course) |
| `POST /api/v1/courses/{course_id}/enrollments` | `{ user_id, role_id }` | 201 `Enrollment`, `Location` | 403 `permission.escalation`, 404 `course.not_found`/`user.not_found`/`role.not_found`, 409 `enrollment.exists`, 409 `role.unknown_builtin`, 409 `course.template`, 409 `course.archived`, 422 `request.invalid` | `perm::EnrollmentManage` (course) |
| `PATCH /api/v1/enrollments/{enrollment_id}` | `{ status }` | 200 `Enrollment` | 403 `permission.escalation`, 404 `enrollment.not_found`, 409 `enrollment.externally_managed`, 409 `role.unknown_builtin` | `perm::EnrollmentManage` (the enrollment's course) |
| `DELETE /api/v1/enrollments/{enrollment_id}` | | 204 | 403 `permission.escalation`, 404 `enrollment.not_found`, 409 `enrollment.externally_managed`, 409 `role.unknown_builtin` | `perm::EnrollmentManage` (the enrollment's course) |

Schemas: `Course { id, account_id, title, code, status, template, start_at, end_at, settings: {
home }, permissions: string[], created_at, updated_at }`; `Enrollment { id, course_id, user: {
id, display_name }, role_id, status, source, external_id, created_at, updated_at }`. Timestamps
use `09`'s format (RFC 3339 UTC, six fractional digits).

`EnrollmentTarget` resolves `enrollment_id` to its course and then behaves as `CourseTarget` with
code `enrollment.not_found`. Changed M0 routes: `DELETE /api/v1/accounts/{account_id}` adds 409
`account.has_courses` (B8); `GET /api/v1/users` widens its account-scope filter (B21).

## Data

Migrations `migrations/<version>_courses.sql` (I1) and `migrations/<version>_enrollments.sql`
(I2). Versions are placeholders here, fixed when each issue is implemented so that each sorts
after every migration on `next` and every open branch at merge time.

```sql
-- <version>_courses.sql
CREATE TABLE courses (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    account_id  uuid        NOT NULL,
    code        text        NOT NULL CHECK (char_length(code) BETWEEN 1 AND 50),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    status      text        NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    template    boolean     NOT NULL DEFAULT false,                  -- not in the data model: Q5
    start_at    timestamptz,                                         -- "dates": Q3
    end_at      timestamptz,
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(settings) = 'object'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, account_id) REFERENCES accounts (org_id, id),
    CHECK (end_at IS NULL OR start_at IS NULL OR end_at >= start_at),
    CHECK (NOT (template AND status = 'published'))
);
CREATE UNIQUE INDEX courses_code ON courses (org_id, lower(code COLLATE "C"));
CREATE INDEX courses_by_account ON courses (org_id, account_id, title, id);

-- <version>_enrollments.sql
CREATE TABLE enrollments (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    course_id    uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    role_id      uuid        NOT NULL REFERENCES roles (id),
    status       text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'concluded')),
    source       text        NOT NULL DEFAULT 'local'
                             CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector')),
    external_id  text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    UNIQUE (course_id, user_id, role_id)
);
CREATE INDEX enrollments_by_user ON enrollments (org_id, user_id) WHERE status = 'active';
CREATE INDEX enrollments_by_course ON enrollments (org_id, course_id, status);
```

The composite foreign keys keep a course in its account's organization and an enrollment's user
and course in one organization. A constraint trigger `enrollments_role_in_org`, the same as
`role_assignments_role_in_org` (`09` Data), refuses a custom role of another organization. The
account foreign key's default `NO ACTION` refuses deleting an account that holds a course, which
backs B8; the route checks first so it answers 409 `account.has_courses` rather than 500.
`courses.code` is unique per organization, compared ASCII-case-insensitively as `07` I4 compares
emails (Q2).

Repository functions (each takes `OrgScope`): `courses::{create, get, list_visible, update,
set_status, copy, path}`, `enrollments::{create, get, list_for_course, set_status, delete,
for_user_active}`; `accounts::delete` gains the course check;
`role_assignments::for_user_with_paths` becomes the grant query of B23 (or a sibling read in the
same statement).

## Permissions

New catalog entries (`stepclimb_core::perm`), and the M1 lists of the built-in roles (`09` B5).
Org admin holds every entry through `perm::ALL`.

| Permission | Key | Grantable at | What it allows |
| --- | --- | --- | --- |
| `perm::CourseView` | `course.view` | organization, account, course | Read a course and its published, unlocked content |
| `perm::CourseViewAll` | `course.view_all` | organization, account, course | Read drafts, templates, unpublished modules and items, and locked content (specs 03, 04) |
| `perm::CourseCreate` | `course.create` | organization, account | Create courses in an account, and copies into it |
| `perm::CourseManage` | `course.manage` | organization, account, course | Change a course's details and status |
| `perm::CourseContentEdit` | `course.content_edit` | organization, account, course | Edit modules, items, files and pages (specs 03, 06, 07); the plan's example `Require<perm::CourseContentEdit>` |
| `perm::CourseParticipate` | `course.participate` | course | Take the course as a learner: record progress (spec 05) |
| `perm::EnrollmentView` | `enrollment.view` | organization, account, course | Read a course's enrollments |
| `perm::EnrollmentManage` | `enrollment.manage` | organization, account, course | Enroll, conclude and remove users |

`course.participate` is grantable at course scope only, so it comes only from an enrollment:
an Organization admin holds the key but, assigned at the organization, does not participate
(`09` B3), and a staff member who wants to try a course as a learner is enrolled as a Student.

| Built-in role | M1 permissions (Q10) |
| --- | --- |
| Organization admin | every entry (`perm::ALL`) |
| Account admin | M0 list plus `course.view`, `course.view_all`, `course.create`, `course.manage`, `course.content_edit`, `enrollment.view`, `enrollment.manage` |
| Course designer | `course.view`, `course.view_all`, `course.content_edit` |
| Instructor | Course designer's, plus `enrollment.view`, `enrollment.manage` |
| Grader | none until M2 |
| Student | `course.view`, `course.participate` |
| Manager, Observer | none; not enrollable (Q9) |

Entitlements: none. The plan names no course feature or limit.

## Test cases

Route tests use the permission suite of `09` B20, which this spec extends with `CourseTarget` and
`EnrollmentTarget` setups (a course in a sibling account for the 404 case). "Org A" and "org B"
each have the tree root, Tower, Enroute.

- **T1** (B1, B2). An account admin of Tower creates a course in Tower (201, `draft`) and gets
  404 `account.not_found` for Enroute; a blank title and a 201-character title answer 422 at
  `/title`.
- **T2** (B2). A course create that waits on a concurrent move of Tower out of the caller's
  subtree answers 404 and inserts nothing (two connections).
- **T3** (Q2). Codes `S1` and `s1` collide (409 `course.code_taken`); `S1` in org A and org B
  both succeed.
- **T4** (B3). A patch naming only current values writes nothing and no audit entry; `end_at`
  before `start_at` answers 422 `course.dates_inverted`; `"title": null` answers 400.
- **T5** (B4). Each allowed status change succeeds once; draft to archived answers 409
  `course.status_invalid`; published to draft with a Student enrolled answers 409
  `course.has_learners` and succeeds after the enrollment is concluded; a template's publish
  answers 409 `course.template`.
- **T6** (B5). In an archived course, `PATCH` works and an enrollment create answers 409
  `course.archived` (content routes in specs 03 and later repeat this).
- **T7** (B8). Deleting Tower with a course in it answers 409 `account.has_courses`; the course
  and account are unchanged.
- **T8** (B10). A Student of a draft course gets 404 `course.not_found` on `GET`; a Course
  designer of it gets 200 with `permissions` listing exactly the designer's course keys.
- **T9** (B11). With courses in Tower, Enroute and the root: an organization admin lists all
  three; an account admin of Tower lists Tower's; a Student enrolled in Enroute's published
  course and a draft course lists only the published one; `account_id` of an invisible account
  answers 404.
- **T10** (B12, B13). Copying a course creates a draft with the given code in the destination,
  no enrollments, and a `course.copy` entry; a caller with `CourseViewAll` at the source but no
  `CourseCreate` at the destination gets 404 for the destination; `template: true` produces a
  template that cannot be published.
- **T11** (B15, B17). Enrolling a user as Student twice answers 409 `enrollment.exists`; as
  Student and Instructor gives two rows; as Observer or Manager answers 422
  `enrollment.role_unavailable`; a user of org B answers 404 `user.not_found`.
- **T12** (B17). An Instructor of a course cannot enroll someone with a custom role holding
  `course.manage` (403 `permission.escalation`), and can enroll another Instructor (201).
- **T13** (B16, B23). An active Student enrollment gives `course.view` at that course and not at
  another course in the same account; after concluding it, `GET` the course answers 404 and the
  enrollment row remains.
- **T14** (B16). An enrollment with a custom role holding `organization.manage` and `course.view`
  grants only `course.view` (`organization.manage` is not grantable at course scope).
- **T15** (B18). An enrollment with source `sync` (inserted by the test) answers 409
  `enrollment.externally_managed` to `PATCH` and `DELETE`.
- **T16** (B19). A role `PATCH` that adds a permission, racing an enrollment create that checks
  escalation, either commits first and is seen by the check, or waits (two connections).
- **T17** (B21). An account admin of Tower finds through `GET /api/v1/users` a user whose only
  link is a Student enrollment in a Tower course, and not one enrolled only in an Enroute course.
- **T18** (B22). `POST /api/v1/admin/role-assignments` with a course scope still answers 422
  `role_assignment.scope_unavailable`.
- **T19** (B24, every course and enrollment route). Four cases from the suite: anonymous 401; a
  user holding every catalog permission except the route's at the course 403; a user holding
  every permission at a sibling account 404; a user holding only the route's permission (plus the
  documented second permission for copy) 2xx. A Student gets 403 on `PATCH` of their own course.
- **T20** (every course and enrollment route). Cross-organization: an organization admin of org A
  calling each route with org B's course, enrollment, account, user or role id (in the path or
  body) gets 404 with the route's code, and org B's `courses`, `enrollments` and
  `audit_events` rows are unchanged.
- **T21** (B27). Each audited change writes exactly one entry with the documented `data`; a
  refused change writes none.
- **T22** (Permissions). `perm::ALL` and the built-in lists match the tables above (the catalog
  test `all_is_the_m0_catalog` becomes `all_is_the_catalog`).

## Out of scope

- Modules, items and content: specs 03, 06 to 09. Unlock and completion: spec 04. Progress:
  spec 05.
- Enrollment rules from groups (`enrollment_rules`): M4, with groups and sync.
- Self-enrollment and course catalogs: not in the plan.
- Deleting a course: Q7. Moving a course between accounts: not in the plan (B9).
- The Observer's named students and the Manager's reports: Q9.
- A "view as student" preview for staff: not in the plan.
- Course archive import and export: M4.
- The course pages of the web app (list, home, settings, roster): spec 16 and the specs that
  build each screen.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. How a role is granted at course scope.** The data model has both `enrollments(...,
  role_id, ...)` and `role_assignments(scope_type 'course')`, and `09` B12 refuses course-scope
  assignments until this spec. Options: (a) an active enrollment is the course-scope grant, and
  course-scope role assignments stay refused; (b) enrollments for learners only, with staff
  through course-scope role assignments; (c) an enrollment also writes a matching role
  assignment. Recommendation: (a). One table answers "who is in this course and as what", as
  Canvas's enrollments do, M4's enrollment rules (group to course as role R) write the same rows,
  and the evaluator reads one source per scope type.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Course code rules.** The plan names a code and no rule. Options: (a) required, 1 to 50
  characters, unique per organization ignoring ASCII case; (b) optional and not unique; (c)
  required and unique per account. Recommendation: (a). A code is how staff and reports name a
  course ("S1"), and a duplicate would make a report ambiguous; deep links use ids, so a code can
  still change.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. What a course's dates do.** The plan lists "dates" without a rule. Options: (a) shown to
  learners and staff only, with no access rule; (b) learners cannot open the course before
  `start_at`, and it becomes read-only after `end_at`; (c) (b) as an opt-in setting.
  Recommendation: (a). Module unlock dates (spec 04) and due dates (spec 13) already gate and
  schedule work, and a course that silently closes after a date surprises recurring training.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. What archiving means.** Options: (a) read-only for everyone, learners keep reading what
  they could, no progress is recorded, and it can be published again; (b) hidden from learners;
  (c) frozen and hidden. Recommendation: (a), which keeps history visible to the people it
  belongs to.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. What "copied as a template" means.** Options: (a) a `template` flag: a template is never
  published or given learners, is listed with `?template=true`, and copying it makes an ordinary
  course; (b) a template is any copy without dates; (c) a separate templates table.
  Recommendation: (a), a column the data model lacks but the smallest addition that keeps
  templates out of learners' reach.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q6. What the home page is.** Options: (a) the module list by default, or one of the course's
  pages (from spec 07); (b) always the module list; (c) a separate syllabus document.
  Recommendation: (a), stored in `settings.home` so spec 07 adds the page option without a
  migration.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q7. Deleting courses.** The plan lists statuses and copying and says nothing of deletion.
  Options: (a) no deletion in M1, archive instead; (b) delete only a draft course that never had
  an enrollment; (c) delete any course with its learner data. Recommendation: (a). Deletion of
  learner data belongs with M4's organization and user deletion; the cost is that an account
  holding a course cannot be deleted (B8).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q8. What concluding and deleting an enrollment do.** The plan's enrollment rules either
  conclude ("history kept") or delete. Options: (a) concluded grants nothing and keeps the row
  and progress; delete removes the enrollment and the user's progress in the course; (b)
  concluded keeps read-only access (`course.view`); (c) delete keeps progress. Recommendation:
  (a), the reading of "history kept" that fails closed. Delete is learner data deletion and gets
  the maintainer's line-by-line review in spec 05's issue.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q9. Manager and Observer in courses.** `09` B13 refuses both, and `09` B5 says the
  Observer's named-students scope arrives with progress in M1. Options: (a) refuse both as
  enrollment roles in M1 and specify the Observer's named students with the Manager's reports in
  M4, since both are scopes over people rather than courses; (b) add an observer link table to
  spec 05; (c) allow Observer enrollments that see every learner of the course.
  Recommendation: (a). vZDC's "staff oversight" use maps to Instructor or Account admin views
  meanwhile; this moves `09` B5's "M1" to M4, which the owner should confirm.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q10. Which roles publish and change courses.** The plan gives Course designer "edit content",
  Instructor "designer permissions plus grading, overrides, accommodations and enrollments", and
  Account admin "courses ... within an account subtree". Options: (a) `course.manage` and
  `course.create` for Account admin only, as the table above; (b) also Instructor; (c) also
  Course designer. Recommendation: (a), the least privilege the plan's wording supports; an
  organization that wants instructors to publish defines a custom role.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Courses table, catalog entries and `CourseTarget`.** Migration `<version>_courses.sql`,
   the eight catalog entries and the built-in lists, `courses` repository (create, get, update,
   status, list visible), `CourseTarget`, the course routes except copy, the account deletion
   check, audit entries for courses, and the permission-suite setups. Covers B1 to B11, B24,
   B27 (courses); T1 to T9, T19, T20, T22 for the course routes. Depends on: none (M0 complete).
2. **I2. Enrollments and course grants.** Migration `<version>_enrollments.sql` with its role
   trigger, enrollment routes, `EnrollmentTarget`, the evaluator's enrollment grants, API key
   intersection, the users list's account-scope widening, and `09` B12's text. Covers B15 to B23,
   B25, B26, B27 (enrollments); T11 to T18, T19 to T21 for these routes. Depends on: I1.
3. **I3. Copy and templates.** `courses::copy` (course row only; later specs add their parts),
   the copy route, `template` handling in create, list and status. Covers B12 to B14; T10.
   Depends on: I1.
