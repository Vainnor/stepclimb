# Modules and items

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Module, Item), Integration platform
  (Embedding: deep links with a return URL), Data model (`modules`, `module_items`), API surface
  (`/courses/{id}/modules`, `/modules/{id}/items`, reordering); `docs/plan/ui.md` -> Component
  inventory (Learning: ModuleRow, ItemRow; Authoring: ModuleBoard), Key layouts (course home)
- **Depends on:** `02-courses-and-enrollments.md` (courses, `CourseTarget`, course permissions,
  archived courses), `../m0/11-app-shell.md` (B7: the deep-link route is M1; `return_to`)
- **Review:** standard

## Summary

A course is a list of modules, and each module is an ordered list of items: a page, a video, a
file, a quiz, an activity, an external link or a header. This spec adds the two tables, the
authoring routes that create, change, publish, reorder and delete them, the learner reads, and
the deep link `/c/:course/m/:module/i/:item` that a host site uses to send someone straight to an
item, with an optional link back. What each item points at (a page, a video, a file) is built by
specs 06 to 09; quizzes are M2 and activities M3, and until then those two kinds are reserved
names that cannot be created. Unlock and completion rules are spec 04.

## Behavior

### Modules

- **B1.** A module belongs to one course and has `title` (trimmed, 1 to 200 characters),
  `position` (0-based, dense within the course) and `published` (Q1). A new module is
  unpublished and goes last. A course holds at most 200 modules (409 `module.limit_reached`).
- **B2.** Creating, renaming, publishing, unpublishing and deleting a module needs
  `perm::CourseContentEdit` at its course; every write in an archived course answers 409
  `course.archived` (`02` B5).
- **B3.** Deleting a module is allowed only when it has no items (409 `module.not_empty`) (Q3).
  The remaining modules' positions close the gap in the same transaction.

### Items

- **B4.** An item belongs to one module and has `kind`, `title` (trimmed, 1 to 200 characters),
  `position` (0-based, dense within the module), `published` (Q1) and, by kind, a reference
  (`ref_id`) or a URL. A module holds at most 200 items (409 `item.limit_reached`). A new item is
  unpublished and goes last unless the request names `position`.
- **B5.** The kinds, what each points at, and which spec makes it creatable:

  | Kind | Points at | Creatable from |
  | --- | --- | --- |
  | `header` | nothing; a label that groups the items below it | this spec |
  | `external_link` | `url` | this spec |
  | `file` | a `course_files` row of the same course (`ref_id`) | spec 06 |
  | `page` | a `pages` row of the same course (`ref_id`) | spec 07 |
  | `video` | a `videos` row of the organization, attached to the course (`ref_id`) | specs 08, 09 |
  | `quiz` | a quiz (`ref_id`) | M2: reserved, refused until then |
  | `activity` | an activity (`ref_id`) | M3: reserved, refused until then |

  Creating an item of a kind that is not creatable yet answers 422 `request.invalid`
  (`item.kind_unavailable` at `/kind`). `quiz` and `activity` are in the table's check
  constraint so M2 and M3 add no migration to the kind list; no code path in M1 creates or reads
  them. Each content spec registers its kind with the function that checks its `ref_id`.
- **B6.** A `ref_id` must name a row of the same course (or for a video, attached to it) in the
  caller's organization; otherwise 404 with that kind's code (`file.not_found`, `page.not_found`,
  `video.not_found`), the same as for a row that does not exist. An item's `kind` and `ref_id`
  never change after creation; to point elsewhere, an author adds a new item. An item may be
  renamed, published, unpublished, moved and deleted.
- **B7.** An external link's `url` is an absolute `https` or `http` URL of at most 2,048
  characters with a host, no user name or password, and no control or white-space character
  (422 `item.url_invalid` at `/url`); it may be changed. The server never fetches it. The web app
  opens it in a new browsing context with `rel="noopener noreferrer"`.
- **B8.** A header carries no reference, URL or requirement (spec 04 refuses one on it).
- **B9.** Deleting an item removes its row and closes the positions gap. It does not delete what
  the item points at (a page, a file or a video stays in the course). From spec 05, an item that
  has learner progress cannot be deleted (409 `item.has_progress`); it is unpublished instead
  (Q3).

### Ordering

- **B10.** `PUT /api/v1/courses/{course_id}/modules/order` takes the full list of the course's
  module ids in their new order; `PUT /api/v1/modules/{module_id}/items/order` the full list of
  the module's item ids. A list that is not exactly the current set (a missing, extra or repeated
  id) answers 409 `order.stale` and changes nothing, so two authors reordering at once cannot
  lose an item; the client reloads and tries again. A list equal to the current order writes
  nothing.
- **B11.** `POST /api/v1/items/{item_id}/move` with `{ "module_id", "position" }` moves an item to
  another module of the same course (404 `module.not_found` for a module of another course) at a
  position from 0 to the destination's item count (422 `request.invalid` otherwise), closing the
  gap in the source and opening one in the destination.
- **B12.** Every structural write (create, delete, reorder, move) locks the course row (`FOR NO
  KEY UPDATE`) before reading positions, so positions stay dense under concurrent writes. A
  deferrable unique constraint on `(module_id, position)` and `(course_id, position)` backs it.

### Reading

- **B13.** `GET /api/v1/courses/{course_id}/modules` returns the course's modules in order, each
  with its items in order, in one response (a course is at most 200 x 200 items). It needs
  `perm::CourseView` at the course and follows `02` B10's draft rule. A caller without
  `perm::CourseViewAll` sees only published modules and, in them, only published items; spec 04
  adds lock state and withholds locked items' references, and spec 05 adds the caller's
  progress.
- **B14.** `GET /api/v1/modules/{module_id}` and `GET /api/v1/items/{item_id}` return one module
  (with its items) or one item with the same rules. An unpublished module or item, or one in an
  unpublished module, answers 404 (`module.not_found`, `item.not_found`) to a caller without
  `CourseViewAll`, as for an id that does not exist.
- **B15.** An item response carries `id`, `module_id`, `course_id`, `kind`, `title`, `position`,
  `published`, and by kind `ref_id` or `url`. Each content spec says what a learner then calls to
  get the content (spec 06's file URL, spec 07's page body, and so on), each repeating B14 and
  spec 04's lock check.

### Deep links and the return URL

- **B16.** The web app route `/c/:course/m/:module/i/:item` (`m0/11` B7) takes ids. It loads
  `GET /api/v1/items/{item}`; when the item's `module_id` and `course_id` match the path it opens
  the item in the course layout, and otherwise, or on any 404, it shows the not-found view (`m0/11`
  B27). Without a session it goes through sign-in with the deep link as `return_to` (`m0/08` B19,
  `m0/11`), so a link from a host site lands on the item after sign-in. `/c/:course` and
  `/c/:course/m/:module` open the course home and the module.
- **B17.** A deep link may carry `?return=<url>`, the plan's optional return URL (Q2). The web app
  keeps it for the browser tab's session (`sessionStorage`) and shows a "Back to <host>" link in
  the course layout only when the URL's origin is in the organization's `embedding.return_origins`
  setting, a list of `https` origins an admin of the organization sets through `PATCH
  /api/v1/organization/settings` (`02` B14; `perm::OrgManage`). The link is followed only by the
  person clicking it; the app never redirects to it on its own. Any other value is dropped
  silently, never shown. The setting holds at most 20 origins, each `https://host[:port]`
  without a path (422 `request.invalid`, `settings.return_origin_invalid` at the entry).
- **B18.** `return` survives the sign-in round trip because the deep link, query included, is the
  `return_to` path; `m0/08` B19's rules for that path still apply.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals; every write in an archived course answers 409 `course.archived`.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/modules` | | 200 `{ modules: Module[] }` | 404 `course.not_found` | `perm::CourseView` (course) |
| `POST /api/v1/courses/{course_id}/modules` | `{ title }` | 201 `Module`, `Location` | 404 `course.not_found`, 409 `module.limit_reached`, 422 `request.invalid` | `perm::CourseContentEdit` (course) |
| `PUT /api/v1/courses/{course_id}/modules/order` | `{ module_ids: uuid[] }` | 200 `{ modules: Module[] }` | 404 `course.not_found`, 409 `order.stale` | `perm::CourseContentEdit` (course) |
| `GET /api/v1/modules/{module_id}` | | 200 `Module` | 404 `module.not_found` | `perm::CourseView` (the module's course) |
| `PATCH /api/v1/modules/{module_id}` | `{ title?, published? }` | 200 `Module` | 404 `module.not_found`, 422 `request.invalid` | `perm::CourseContentEdit` (the module's course) |
| `DELETE /api/v1/modules/{module_id}` | | 204 | 404 `module.not_found`, 409 `module.not_empty` | `perm::CourseContentEdit` (the module's course) |
| `POST /api/v1/modules/{module_id}/items` | `{ kind, title, ref_id?, url?, position? }` | 201 `Item`, `Location` | 404 `module.not_found` and the kind's code, 409 `item.limit_reached`, 422 `request.invalid` (`item.kind_unavailable`, `item.url_invalid`) | `perm::CourseContentEdit` (the module's course) |
| `PUT /api/v1/modules/{module_id}/items/order` | `{ item_ids: uuid[] }` | 200 `Module` | 404 `module.not_found`, 409 `order.stale` | `perm::CourseContentEdit` (the module's course) |
| `GET /api/v1/items/{item_id}` | | 200 `Item` | 404 `item.not_found` | `perm::CourseView` (the item's course) |
| `PATCH /api/v1/items/{item_id}` | `{ title?, published?, url? }` | 200 `Item` | 404 `item.not_found`, 422 `request.invalid` | `perm::CourseContentEdit` (the item's course) |
| `POST /api/v1/items/{item_id}/move` | `{ module_id, position }` | 200 `Item` | 404 `item.not_found`/`module.not_found`, 409 `item.limit_reached`, 422 `request.invalid` | `perm::CourseContentEdit` (the item's course) |
| `DELETE /api/v1/items/{item_id}` | | 204 | 404 `item.not_found`, 409 `item.has_progress` (spec 05) | `perm::CourseContentEdit` (the item's course) |

Schemas: `Module { id, course_id, title, position, published, items: Item[], created_at,
updated_at }`; `Item { id, module_id, course_id, kind, title, position, published, ref_id?, url?,
created_at, updated_at }`. `url` is present only for `external_link`, `ref_id` only for kinds
that have one. `ModuleTarget` and `ItemTarget` resolve `module_id` and `item_id` to their course
and then act as `CourseTarget` with codes `module.not_found` and `item.not_found`. `PATCH` with a
`url` on an item that is not an external link answers 422 `request.invalid`.

`OrgSettings` gains `embedding.return_origins` (B17), read and written through the existing
settings routes.

## Data

Migration `migrations/<version>_modules_and_items.sql` (I1). The version is a placeholder, fixed
when the issue is implemented so it sorts after every migration on `next` and every open branch.

```sql
CREATE TABLE modules (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    position    integer     NOT NULL CHECK (position >= 0),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    published   boolean     NOT NULL DEFAULT false,                   -- not in the data model: Q1
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, course_id, id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    CONSTRAINT modules_position UNIQUE (course_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE module_items (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,                                 -- denormalized: B12, ItemTarget
    module_id   uuid        NOT NULL,
    position    integer     NOT NULL CHECK (position >= 0),
    kind        text        NOT NULL CHECK (kind IN ('page', 'video', 'file', 'quiz', 'activity', 'external_link', 'header')),
    ref_id      uuid,
    url         text        CHECK (char_length(url) <= 2048),         -- not in the data model: B7
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    published   boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, course_id, module_id) REFERENCES modules (org_id, course_id, id),
    CONSTRAINT module_items_position UNIQUE (module_id, position) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((kind = 'external_link') = (url IS NOT NULL)),
    CHECK ((kind IN ('external_link', 'header')) = (ref_id IS NULL))
);
CREATE INDEX module_items_ref ON module_items (org_id, kind, ref_id) WHERE ref_id IS NOT NULL;
```

The three-column foreign key keeps an item's `course_id` equal to its module's course, so the
denormalized column cannot drift and a move (B11) stays inside one course. `ref_id` has no foreign
key, because it points at a different table per kind; the repository checks it (B6) and each
content spec's deletion refuses a row an item references (`module_items_ref`). `requirement`, the
data model's last column, is spec 04's.

Repository functions (each takes `OrgScope`): `modules::{create, get, list_for_course, update,
delete, reorder}`, `items::{create, get, update, delete, reorder, move_to, referencing}`.
`courses::copy` (`02` B13) copies modules and items with new ids, keeping `ref_id` for kinds whose
spec copies the target in the same transaction and mapping it to the copy.

## Permissions

No new catalog entry: reads take `perm::CourseView` (with `perm::CourseViewAll` deciding what
unpublished content is shown) and writes `perm::CourseContentEdit`, both from
`02-courses-and-enrollments.md`. Entitlements: none.

## Test cases

- **T1** (B1). Three modules created in a course get positions 0, 1, 2 and are unpublished; the
  201st answers 409 `module.limit_reached`.
- **T2** (B2, `02` B5). Every write route answers 409 `course.archived` in an archived course;
  reads still answer 200.
- **T3** (B3). Deleting a module with an item answers 409 `module.not_empty`; deleting the middle
  of three empty modules leaves positions 0, 1.
- **T4** (B5). Creating `quiz`, `activity`, and (before their specs) `page`, `file` and `video`
  items answers 422 `item.kind_unavailable`; `header` and `external_link` succeed.
- **T5** (B7). `javascript:alert(1)`, `//evil.example`, `https://user:pw@host/`, a URL with a
  tab, and a 2,049-character URL each answer 422 `item.url_invalid`; `https://example.com/a?b=1`
  is stored as sent.
- **T6** (B10). Reordering with the current set in a new order applies it; a list missing one id,
  with an extra id, with a repeated id, or with an id of another course answers 409 `order.stale`
  and changes nothing.
- **T7** (B11). Moving an item from module A position 1 to module B position 0 leaves both
  modules dense; a module of another course answers 404 `module.not_found`.
- **T8** (B12). Twenty concurrent item creates in one module (on separate connections) end with
  positions 0 to 19 and no error.
- **T9** (B13, B14). A Student sees only published modules and their published items; an
  unpublished item's id answers 404 `item.not_found` to the Student and 200 to a Course
  designer; an item in an unpublished module answers 404 to the Student.
- **T10** (B16). Playwright: an anonymous visit to `/c/<course>/m/<module>/i/<item>` goes to
  sign-in and, after mock-OIDC sign-in, opens the item; a path whose module does not hold the
  item shows the not-found view.
- **T11** (B17). Playwright: with `https://host.example` in `embedding.return_origins`, a deep link
  with `?return=https://host.example/x` shows "Back to host.example" linking there; with
  `?return=https://other.example/` no link appears and nothing navigates; a settings `PATCH` with
  `http://host.example` or `https://host.example/path` answers 422
  `settings.return_origin_invalid`.
- **T12** (every route). Four cases from the permission suite with `CourseTarget`,
  `ModuleTarget` and `ItemTarget`: anonymous 401; every permission but the route's at the course
  403 (a Student on every write route); every permission at a sibling account 404; only the
  route's permission at the course 2xx.
- **T13** (every route). Cross-organization: an organization admin of org A using org B's course,
  module and item ids (in the path, and as `module_id` in a move body and ids in an order list)
  gets 404 or 409 `order.stale` and org B's `modules` and `module_items` rows are unchanged.

## Out of scope

- Unlock dates, prerequisites, sequential completion, completion rules and item requirements:
  spec 04.
- What each kind points at and its learner route: specs 06 (file), 07 (page), 08 and 09 (video);
  quizzes M2, activities M3; Assignment and LTI items 1.x (plan).
- The ModuleBoard authoring screen, ModuleRow and ItemRow components and the course home layout:
  spec 16 and the screen specs.
- Indentation, item due dates (spec 13) and module-level descriptions: not in the plan.
- Copying a module or item into another course: not in the plan; a whole course copies (`02` B12).

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. Publishing inside a course.** The plan gives the course a status and its modules and
  items none. Without a per-row switch, a module added to a published course appears to learners
  while it is half written. Options: (a) `published` on modules and items, default unpublished,
  hidden from learners; (b) only course status; (c) a whole-course draft copy that replaces the
  live one. Recommendation: (a), the Canvas model, two boolean columns.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Which return URLs a deep link may carry.** The plan says "an optional return URL, so a
  host site can send people straight to an item". An arbitrary URL would let anyone craft a
  Stepclimb link that offers a trip to a phishing page. Options: (a) shown only when its origin is
  on an organization allowlist, as a link the user clicks; (b) any `https` URL, shown with its host
  name; (c) same-origin paths only. Recommendation: (a). It serves a host site the organization
  trusts (vZDC's "Back to vZDC") and nothing else.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Deleting content with learner progress.** Options: (a) a module deletes only when empty,
  an item only without learner progress, and authors unpublish otherwise; (b) deletion removes
  the item and its learner progress; (c) soft deletion. Recommendation: (a), which never deletes
  learner records as a side effect of editing.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Size limits.** The plan sets none. Options: (a) 200 modules per course and 200 items per
  module, 200-character titles (the `m0/02` Q6 name limit); (b) no limits. Recommendation: (a). One
  outline response and one copy transaction stay bounded (40,000 items at most).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

### Decisions made building I1

Coordinator decisions (overnight, 2026-11-16), pending owner review. None depends on `02` Q1
or Q12.

- **Course copy waits for `02` I3.** `courses::copy` does not exist yet (`02` I3 is not
  built), so I1 adds no copy code: a copy function nothing calls would be scaffolding. Whichever
  of `02` I3 and the copy part of this spec lands second adds modules and items to the one copy
  transaction (`02` B13).
- **Field codes.** A refused title answers 422 at `/title` with `name.empty`, `name.too_long` or
  `text.contains_nul`, as courses do. Besides the spec's `item.kind_unavailable` and
  `item.url_invalid`: `item.url_required` at `/url` (an external link without a URL),
  `item.url_unavailable` at `/url` (a URL on another kind, on create or patch),
  `item.ref_id_unavailable` at `/ref_id` (a reference on `header` or `external_link`; no
  creatable kind takes one in I1) and `item.position_out_of_range` at `/position` (B4, B11).
  A kind name that is not one of the seven is 400 `request.malformed`: the body does not match
  the schema.
- **B7's "absolute" is checked on the string as sent.** The URL must start with `http://` or
  `https://` (any case) and parse to a host. The parser reads `https:host` as
  `https://host/`, but a browser resolves that string against the page, so the stored value
  would not be the link the parser accepted.
- **Every write locks the course**, not only the structural ones of B12: renames, publish
  switches and URL changes too, so a course archived meanwhile (`02` B5) is seen by every
  write. The route checks `course.content_edit` again on the course's path as its transaction
  reads it, as course writes do (`02` I1).
- **Positions within one module (B11).** A move inside the item's own module takes 0 to the
  module's item count minus one; to another module, 0 to its count. The item's own place
  writes nothing.
- **`updated_at`** changes with a row's title, `published`, URL or module, not with a position
  shifted by a sibling's create, delete, reorder or move.
- **Write responses carry every item.** `PUT .../modules/order`, `PATCH /modules/{id}` and
  `PUT .../items/order` return each module with all its items, published or not: the caller
  holds `course.content_edit` and can change every one of them. B13's filtering is for the
  read routes of I2.
- **No `items::referencing` yet.** The spec lists it for the content specs' deletion checks;
  nothing calls it until spec 06, which adds it.

## Issues

1. **I1. Modules and items tables and authoring routes.** Migration, repositories,
   `ModuleTarget`, `ItemTarget`, create, patch, delete, publish, the kind registry with `header`
   and `external_link`, reordering and moving, course copy of modules and items, archived-course
   refusals. Covers B1 to B12; T1 to T8, T12 and T13 for the write routes. Depends on:
   `02-courses-and-enrollments.md` I1.
2. **I2. Learner reads.** `GET` outline, module and item, with published filtering and the draft
   course rule. Covers B13 to B15; T9, T12 and T13 for the read routes. Depends on: I1;
   `02-courses-and-enrollments.md` I2 (enrollment grants, so a Student can be tested).
3. **I3. Deep links and the return link.** The web route with the id consistency check, the
   sign-in round trip, the `embedding.return_origins` setting and the "Back to" link. Covers B16
   to B18; T10, T11. Depends on: I2.
