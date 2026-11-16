# Pages and the editor

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Page: "Versioned Tiptap JSON:
  headings, callouts, tables, code, images, YouTube or Vimeo embeds, phraseology blocks, ungraded
  'check your understanding' questions"), Architecture and stack (Rich text: Tiptap/ProseMirror
  JSON as the source, sanitized HTML rendered by the server with `ammonia`), Data model (`pages`,
  `page_versions`), API surface (`/api/v1/pages/{id}`, `/pages/{id}/versions`), Security and
  privacy (rich content sanitized on the server); `docs/plan/ui.md` -> Library architecture
  (`src/editor`: Tiptap wrapper, toolbar, slash menu, embeds), Component inventory (Authoring:
  Editor, SlashMenu, BlockToolbar, VersionDiff; Learning: PageReader, Callout), Signature
  components (PageReader)
- **Depends on:** `02-courses-and-enrollments.md` (courses, permissions, home page),
  `03-modules-and-items.md` (`page` items), `04-unlock-and-completion-rules.md` (locks),
  `06-files.md` (images are course files, signed URLs)
- **Review:** standard

## Summary

A page is a course's long-form reading: text with headings, callouts, tables, code, images,
YouTube and Vimeo embeds and transcript ("phraseology") blocks. Authors write it in a Tiptap
editor; the server stores the editor's JSON as an immutable version on every save, validates it
against a fixed schema, and renders it to sanitized HTML for readers, so no script or markup an
author typed ever reaches a learner's browser unchecked. Readers see pages through `page` items
(with spec 04's locks) or as the course home page.

## Behavior

### Pages and versions

- **B1.** A page belongs to one course and has `title` (1 to 200 characters), `slug` (unique in
  the course, `^[a-z0-9]+(-[a-z0-9]+)*$`, 1 to 100 characters, derived from the title when not
  given), and `current_version_id`. A page always has at least one version: creating a page
  creates version 1 with the given body, or an empty document.
- **B2.** A version is one `page_versions` row: the page, a `number` (1, 2, ... per page), the
  body (Tiptap JSON), the author and `created_at`. Versions are never changed or deleted while the
  page exists.
- **B3.** `PUT /api/v1/pages/{page_id}/body` with `{ "body", "base_version_id" }` saves a new
  version and makes it current, needing `perm::CourseContentEdit`. When `base_version_id` is not
  the current version, the save answers 409 `page.version_conflict` with the current version's id
  and author, and writes nothing (Q1). A body equal to the current one writes nothing and answers
  200. Archived courses refuse writes (`02` B5).
- **B4.** Saves are explicit (Q1): the editor saves when the author presses Save (or `Ctrl`/`Cmd`
  + `S`); it keeps unsaved changes in the browser's local storage, keyed by page and base version,
  and offers to restore them when the page is opened again.
- **B5.** `GET /api/v1/pages/{page_id}/versions` lists versions newest first, without bodies
  (`02` B3 pagination); `GET /api/v1/pages/{page_id}/versions/{version_id}` returns one with its
  body; `POST /api/v1/pages/{page_id}/versions/{version_id}/restore` saves that version's body as a
  new version (B3's conflict check against `base_version_id` in the request). All need
  `perm::CourseViewAll` (reads) or `perm::CourseContentEdit` (restore).
- **B6.** `PATCH /api/v1/pages/{page_id}` changes `title` and `slug` (409 `page.slug_taken`).
  `DELETE` removes a page and its versions only when no item references it and it is not the
  course's home page (409 `page.in_use`).

### The document schema

- **B7.** A body is a ProseMirror document as Tiptap serializes it (`{ "type": "doc", "content":
  [...] }`), at most 1 MiB serialized and 20,000 nodes. On save the server parses it into a typed
  model in `stepclimb-core` (`page_doc`) that lists every allowed node, mark and attribute; anything
  else (an unknown node or mark, an unknown attribute, a wrong type, a node in a place the schema
  does not allow) answers 422 `request.invalid` with `page.body_invalid` and the JSON Pointer of the
  first offense. Nothing is dropped silently.
- **B8.** Allowed nodes in M1: paragraph; heading (levels 2 to 4; the page title is the level 1);
  bullet and ordered lists; blockquote; horizontal rule; hard break; code block with an
  optional language from a fixed list; table with header cells, cell spans of at most 20; image;
  callout (`kind` in `info`, `tip`, `warning`, `danger`); video embed; transcript block (B11).
  Marks: bold, italic, underline, strike, inline code, link, subscript, superscript. Text is plain
  Unicode; NUL and other control characters except tab and newline are refused.
- **B9.** A link's `href` is an absolute `https`, `http` or `mailto` URL, or a path on the same
  origin starting with one `/` (`08` B19's rules); anything else is refused. Links to other
  origins render with `rel="noopener noreferrer"` and `target="_blank"`.
- **B10.** An image names a course file of the same course (`course_file_id`, 404
  `file.not_found` otherwise; type PNG, JPEG, GIF or WebP, 422 otherwise), with required `alt`
  text (an empty `alt` is allowed only with `decorative: true`). Authors upload images through
  spec 06's upload route into the folder `/_page-images` by default.
- **B11.** The transcript block is the plan's "phraseology block", kept generic (Q2): a list of
  lines, each with a `speaker` label (1 to 40 characters) and text. An organization names labels
  per block (for vZDC, "Controller" and "Pilot"); nothing in the core knows a label.
- **B12.** A video embed names `provider` (`youtube` or `vimeo`) and the video's id, which must
  match the provider's id format (YouTube 11 characters of `[A-Za-z0-9_-]`, Vimeo digits). Authors
  paste a URL; the editor extracts the id; the server never stores a URL for an embed. An embed is
  not tracked: video with watch tracking is a `video` item (spec 09).
- **B13.** Ungraded "check your understanding" questions need `stepclimb-assess`, which is M2;
  the schema has no question node in M1, and M2 adds it (Q3).

### Rendering

- **B14.** `stepclimb_core::page_doc::render` turns a validated body into HTML with our own
  renderer: each node maps to fixed elements and classes (`data-slot` hooks for PageReader's
  styles), text is escaped, and attributes come only from the typed model. The output then passes
  through `ammonia` with an allowlist equal to the renderer's elements and attributes, so a bug in
  the renderer cannot let other markup through.
- **B15.** Images render as `<img>` with a signed inline URL (`06` B16) issued for the request, and
  `width` and `height` from the stored file when known. A rendered body is cached per version
  with placeholders for image URLs, which are filled per request, so the cache never holds a
  credential.
- **B16.** Video embeds render as `<iframe>` with `src` built by the server from the id
  (`https://www.youtube-nocookie.com/embed/<id>`, `https://player.vimeo.com/video/<id>`),
  `sandbox="allow-scripts allow-same-origin allow-presentation"`, `allow="fullscreen"`,
  `referrerpolicy="strict-origin-when-cross-origin"` and a `title` from the block.
- **B17.** Code blocks render as text with a language class; highlighting is done in the browser
  by the PageReader component (no server highlighter).

### Reading

- **B18.** `GET /api/v1/pages/{page_id}` returns `{ id, course_id, title, slug, version: { id,
  number, created_at }, html }` to a caller holding `perm::CourseView` at the course, when one of
  these holds: the caller holds `perm::CourseViewAll`; the page is the course's home page and the
  course is visible to the caller (`02` B10); or a published item referencing the page, in a
  published module, is unlocked for the caller. When the page is referenced only by items locked for
  the caller, it answers 403 `item.locked` with the reasons of the first such item; otherwise 404
  `page.not_found`. With `?format=json` and `perm::CourseViewAll`, it returns the body JSON
  instead of HTML (the editor's load).
- **B19.** `page` items become creatable (`03` B5): `ref_id` is a page of the same course.
  `settings.home` gains `{ "kind": "page", "page_id" }` (`02` B6); setting it to a page of another
  course answers 404 `page.not_found`.
- **B20.** `courses::copy` (`02` B13) copies every page with its current version only (as version
  1 of the new page, authored by the copier) and maps `page` items' `ref_id` and the home page.
  Image references keep working because course files are copied with the same ids mapped (`06`
  B20), and the copy rewrites `course_file_id` in the bodies to the copies.

### The editor

- **B21.** The editor is `src/editor` in `packages/ui` (plan UI tab), a wrapper around Tiptap with
  only the extensions B8 lists, configured from the same schema the server enforces: a toolbar, a
  slash menu (`/` inserts a block) and a block toolbar, all React Aria and keyboard-operable. Its
  strings come from `useStepclimbStrings()`. It loads the body with `?format=json` and saves with
  B3.
- **B22.** The PageReader renders the server's HTML in a reading column of about 66 characters
  (plan UI tab) with "Mark as done" after the last paragraph when the item's requirement is
  `mark_done`, followed by Previous and Next. It inserts the HTML as given (the server sanitized
  it) and attaches no event handlers from it.
- **B23.** VersionDiff shows two versions side by side as rendered HTML with changed blocks marked;
  M1 compares top-level blocks by their serialized JSON (Q4).

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`08`'s CSRF refusals; writes in an archived course answer 409 `course.archived`.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/pages` | `?cursor&limit` | 200 `Page<PageSummary>` | 404 `course.not_found` | `perm::CourseViewAll` (course) |
| `POST /api/v1/courses/{course_id}/pages` | `{ title, slug?, body? }` | 201 `PageSummary`, `Location` | 404 `course.not_found`, 409 `page.slug_taken`, 422 `request.invalid` | `perm::CourseContentEdit` (course) |
| `GET /api/v1/pages/{page_id}` | `?format=html\|json` | 200 `PageView` | 404 `page.not_found`, 403 `item.locked` | `perm::CourseView` (the page's course), then B18 |
| `PATCH /api/v1/pages/{page_id}` | `{ title?, slug? }` | 200 `PageSummary` | 404 `page.not_found`, 409 `page.slug_taken`, 422 | `perm::CourseContentEdit` |
| `PUT /api/v1/pages/{page_id}/body` | `{ body, base_version_id }` | 200 `PageSummary` | 404, 409 `page.version_conflict`, 422 `page.body_invalid`, 404 `file.not_found` | `perm::CourseContentEdit` |
| `DELETE /api/v1/pages/{page_id}` | | 204 | 404, 409 `page.in_use` | `perm::CourseContentEdit` |
| `GET /api/v1/pages/{page_id}/versions` | `?cursor&limit` | 200 `Page<PageVersionSummary>` | 404 | `perm::CourseViewAll` |
| `GET /api/v1/pages/{page_id}/versions/{version_id}` | `?format=html\|json` | 200 `PageVersion` | 404 `page_version.not_found` | `perm::CourseViewAll` |
| `POST /api/v1/pages/{page_id}/versions/{version_id}/restore` | `{ base_version_id }` | 200 `PageSummary` | 404, 409 `page.version_conflict` | `perm::CourseContentEdit` |

Schemas: `PageSummary { id, course_id, title, slug, current_version: { id, number, author_id,
created_at }, created_at, updated_at }`; `PageView { id, course_id, title, slug, version, html? ,
body? }`; `PageVersionSummary { id, number, author_id, created_at }`; `PageVersion` adds `html` or
`body`. `PageTarget` resolves `page_id` to its course and acts as `CourseTarget` with code
`page.not_found`. `GET /api/v1/pages/{page_id}` joins `04`'s lock suite as a lock-checked route.
The `PageDoc` JSON schema (B7, B8) is published as a component so API clients can generate
bodies.

## Data

Migration `migrations/<version>_pages.sql` (I1). The version is a placeholder, fixed when the
issue is implemented so it sorts after every migration on `next` and every open branch.

```sql
CREATE TABLE pages (
    id                  uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL,
    course_id           uuid        NOT NULL,
    slug                text        NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND char_length(slug) <= 100),
    title               text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    current_version_id  uuid,       -- set in the creating transaction; deferred FK below
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (course_id, slug),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id)
);

CREATE TABLE page_versions (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    page_id     uuid        NOT NULL,
    number      integer     NOT NULL CHECK (number >= 1),        -- not in the data model: B2
    body        jsonb       NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    author_id   uuid        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (page_id, number),
    FOREIGN KEY (org_id, page_id) REFERENCES pages (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, author_id) REFERENCES users (org_id, id)
);

ALTER TABLE pages ADD CONSTRAINT pages_current_version
    FOREIGN KEY (current_version_id) REFERENCES page_versions (id) DEFERRABLE INITIALLY DEFERRED;
```

A trigger refuses `UPDATE` of `page_versions.body`, `number`, `page_id` and `author_id`, so
versions stay immutable (B2). `course_files` referenced by bodies are recorded in
`page_version_files(org_id, version_id, course_file_id)` (written on save, from the parsed body)
so `06` B11's `file.in_use` covers images used by any version and B20's copy maps them without
reparsing; this table is not in the data model.

## Permissions

No new catalog entry: reads take `perm::CourseView` with B18's rules or `perm::CourseViewAll`,
writes `perm::CourseContentEdit`. Entitlements: none.

## Test cases

- **T1** (B1, B2). Creating a page creates version 1 and sets it current; a slug is derived from
  "Ground Control 101" as `ground-control-101`; a duplicate slug answers 409.
- **T2** (B3). Two editors save from the same base: the first gets version 2, the second 409
  `page.version_conflict` naming version 2; saving an unchanged body writes no version.
- **T3** (B2). An `UPDATE` of a version's body in SQL fails on the trigger.
- **T4** (B7, B8). Golden cases: every allowed node and mark validates and renders to the
  committed HTML file; a `script` node, an `onclick` attribute on a paragraph, a heading level 1,
  a 21-column span, a 1 MiB + 1 body and a body with NUL each answer 422 `page.body_invalid` with
  the pointer.
- **T5** (B9). Links `javascript:alert(1)`, `data:text/html,...`, `//evil.example` and
  `/\evil.example` are refused; `https://example.com`, `mailto:a@example.com` and `/c/x` render.
- **T6** (B14). A property test generates random valid documents (proptest, 10,000 cases) and
  checks that `ammonia` changes nothing in the renderer's output, so the renderer emits only
  allowlisted markup; mutating the renderer to emit an attribute outside the allowlist turns it
  red.
- **T7** (B10, B15). An image of another course answers 404 `file.not_found`; a PDF as image 422;
  an image without `alt` and not decorative 422. A rendered image's URL serves the file; two
  requests get two different URLs; the cache row contains no `/files/` token.
- **T8** (B12, B16). A YouTube id with 12 characters is refused; a valid one renders the
  `youtube-nocookie.com` iframe with the sandbox attribute and nothing from the author but the id.
- **T9** (B18). A Student reads a page through an unlocked item (200, HTML); through a locked one
  403 `item.locked`; a page no visible item references 404; the home page 200; `?format=json`
  answers 403 `permission.denied` to the Student.
- **T10** (B6). Deleting a page an item uses, or the home page, answers 409 `page.in_use`.
- **T11** (B5). Restoring version 1 creates version 3 equal to version 1.
- **T12** (B20). A copied course has each page with one version equal to the source's current,
  items and home page pointing at the copies, and image references to the copied course files.
- **T13** (B21, B22, B23). Vitest: the editor's schema refuses pasting a `<script>`; the slash menu
  inserts each block with the keyboard only; Playwright: an author writes a page with a callout
  and an image, saves, and a Student reads it in PageReader with "Mark as done". Screenshots of
  PageReader and the editor in the default and vZDC themes, light and dark, at 1280 and 375 px;
  axe finds no violation.
- **T14** (every route). Four cases: anonymous 401; every permission but the route's at the course
  403; every permission at a sibling account 404; only the route's permission 2xx (for `GET
  /pages/{id}`, `course.view_all`).
- **T15** (every route). Cross-organization: org A's admin with org B's course, page, version or
  course file id (in the path or inside a body's image node) gets 404; org B's pages and versions
  are unchanged.

## Out of scope

- "Check your understanding" questions: M2 (Q3).
- Real-time collaborative editing: not in the plan.
- Pages outside courses (account-wide pages): not in the plan.
- Email template editing reuses this editor with its own node list: spec 12.
- Offline reading of pages: spec 15.
- Mentions, comments and footnotes: not in the plan.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. When a version is made, and two authors at once.** Options: (a) every explicit save is a
  version, with a conflict check on the base version and unsaved work kept in the browser; (b)
  autosave every few seconds, coalescing versions by author within 10 minutes (versions are then
  not immutable); (c) a separate draft that is published as a version. Recommendation: (a): it
  keeps versions immutable and history readable, and no save silently overwrites another author.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. What a "phraseology block" is.** The plan's name is ATC-specific, and nothing VATSIM- or
  aviation-specific may be in the core. Options: (a) a generic transcript block with free speaker
  labels per block, which the UI calls "Transcript" and an organization can rename through
  terminology; (b) a block with fixed "Controller" and "Pilot" lines in the core; (c) move it to
  the ATC pack. Recommendation: (a).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. "Check your understanding" questions in M1.** They need question types from
  `stepclimb-assess` (M2). Options: (a) leave them to M2, which adds the node; (b) a simple
  multiple-choice node in M1 graded in the browser; (c) a placeholder node. Recommendation: (a);
  (b) would build a second question model that M2 replaces.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. How VersionDiff compares.** Options: (a) top-level blocks compared as JSON, changed blocks
  marked; (b) a text diff inside blocks; (c) no diff in M1. Recommendation: (a), no dependency, and
  enough to see what changed between saves.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Page storage, versions and the document model.** Migration, `page_doc` typed model and
   validation, page CRUD, saves with the conflict check, versions list, read and restore,
   `PageTarget`, `page_version_files`. Covers B1 to B11 (validation), B12; T1 to T5, T10, T11,
   T14 and T15 for these routes. Depends on: `03-modules-and-items.md` I1, `06-files.md` I2.
2. **I2. Rendering and reading.** The renderer, `ammonia` (pinned after `dependency-researcher`, in
   its own commit), the per-version cache with image placeholders, signed image URLs, embeds,
   `GET /pages/{id}` with B18's access rules and the lock suite entry, `page` items and the home
   page option, copying pages. Covers B14 to B20; T6 to T9, T12. Depends on: I1, `06-files.md`
   I3, `04-unlock-and-completion-rules.md` I3, `02-courses-and-enrollments.md` I3.
3. **I3. The editor.** `packages/ui/src/editor` with Tiptap (pinned after `dependency-researcher`,
   in its own commit), toolbar, slash menu, block toolbar, local unsaved-work recovery, the
   authoring screen. Covers B4, B21; T13 (editor). Depends on: I1.
4. **I4. PageReader and VersionDiff.** The reading view with Mark as done, Previous and Next, and
   the version history screen. Covers B22, B23; T13 (reader). Depends on: I2,
   `05-progress-and-events.md` I1.
