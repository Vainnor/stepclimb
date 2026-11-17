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
  (`m0/02` B3 pagination); `GET /api/v1/pages/{page_id}/versions/{version_id}` returns one with its
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
  origin starting with one `/` (`m0/08` B19's rules); anything else is refused. Links to other
  origins render with `rel="noopener noreferrer"` and `target="_blank"`.
- **B10.** An image names a course file of the same course (`course_file_id`, 404
  `file.not_found` otherwise), with required `alt` text (an empty `alt` is allowed only with
  `decorative: true`). Authors upload images through spec 06's upload route into the folder
  `/_page-images` by default.
- **B10a.** The server refuses an image node whose course file is not an image (Q5): the file's
  stored `mime`, the type spec 06 B6 sniffed from its bytes, must be `image/png`, `image/jpeg`,
  `image/gif` or `image/webp`. Any other type, `application/octet-stream` (no signature matched)
  and SVG included, answers 422 `request.invalid` with `page.image_type` at the image node's
  `/attrs/course_file_id`, and the save writes nothing. The file's name and the client's
  `Content-Type` play no part, since spec 06 ignores both. The check runs on every save and every
  restore (B3, B5), so an old version that names a non-image file cannot be restored as current.
  Uploading is unchanged: spec 06 still stores any file, and only inserting it as an image is
  refused. The editor shows the refusal at the image block.
- **B11.** The transcript block is the plan's "phraseology block", kept generic (Q2): a list of
  lines, each with a `speaker` label (1 to 40 characters) and text. An organization names labels
  per block (for vZDC, "Controller" and "Pilot"); nothing in the core knows a label.
- **B12.** A video embed names `provider` (`youtube` or `vimeo`) and the video's id, which must
  match the provider's id format (YouTube 11 characters of `[A-Za-z0-9_-]`, Vimeo digits). Authors
  paste a URL; the editor extracts the id; the server never stores a URL for an embed. An embed is
  not tracked: video with watch tracking is a `video` item (spec 09).
- **B13.** Ungraded "check your understanding" questions need `stepclimb-assess` and move to M2
  (Q3): the M1 schema has no question node, so a body containing one answers 422
  `page.body_invalid` like any unknown node (B7). M2 adds the node.

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
`m0/08`'s CSRF refusals; writes in an archived course answer 409 `course.archived`.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/pages` | `?cursor&limit` | 200 `Page<PageSummary>` | 404 `course.not_found` | `perm::CourseViewAll` (course) |
| `POST /api/v1/courses/{course_id}/pages` | `{ title, slug?, body? }` | 201 `PageSummary`, `Location` | 404 `course.not_found`, 409 `page.slug_taken`, 422 `request.invalid` | `perm::CourseContentEdit` (course) |
| `GET /api/v1/pages/{page_id}` | `?format=html\|json` | 200 `PageView` | 404 `page.not_found`, 403 `item.locked` | `perm::CourseView` (the page's course), then B18 |
| `PATCH /api/v1/pages/{page_id}` | `{ title?, slug? }` | 200 `PageSummary` | 404 `page.not_found`, 409 `page.slug_taken`, 422 | `perm::CourseContentEdit` |
| `PUT /api/v1/pages/{page_id}/body` | `{ body, base_version_id }` | 200 `PageSummary` | 404, 409 `page.version_conflict`, 422 `request.invalid` (`page.body_invalid`, `page.image_type`), 404 `file.not_found` | `perm::CourseContentEdit` |
| `DELETE /api/v1/pages/{page_id}` | | 204 | 404, 409 `page.in_use` | `perm::CourseContentEdit` |
| `GET /api/v1/pages/{page_id}/versions` | `?cursor&limit` | 200 `Page<PageVersionSummary>` | 404 | `perm::CourseViewAll` |
| `GET /api/v1/pages/{page_id}/versions/{version_id}` | `?format=html\|json` | 200 `PageVersion` | 404 `page_version.not_found` | `perm::CourseViewAll` |
| `POST /api/v1/pages/{page_id}/versions/{version_id}/restore` | `{ base_version_id }` | 200 `PageSummary` | 404, 409 `page.version_conflict`, 422 `request.invalid` (`page.image_type`) | `perm::CourseContentEdit` |

Schemas: `PageSummary { id, course_id, title, slug, current_version: { id, number, author_id,
created_at }, created_at, updated_at }`; `PageView { id, course_id, title, slug, version, html? ,
body? }`; `PageVersionSummary { id, number, author_id, created_at }`; `PageVersion` adds `html` or
`body`. `PageTarget` resolves `page_id` to its course and acts as `CourseTarget` with code
`page.not_found`. `GET /api/v1/pages/{page_id}` joins `04`'s lock suite as a lock-checked route.
The `PageDoc` JSON schema (B7, B8) is published as a component so API clients can generate
bodies.

## Data

Migration `migrations/20261117130000_pages.sql` (I1).

```sql
CREATE TABLE pages (
    id                  uuid        PRIMARY KEY,
    org_id              uuid        NOT NULL,
    course_id           uuid        NOT NULL,
    slug                text        NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND char_length(slug) <= 100),
    title               text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    current_version_id  uuid        NOT NULL,  -- written in the creating transaction; deferred FK below
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, course_id, id),
    UNIQUE (course_id, slug),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id)
);

CREATE TABLE page_versions (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    page_id     uuid        NOT NULL,
    number      integer     NOT NULL CHECK (number >= 1),        -- not in the data model: B2
    body        jsonb       NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    author_id   uuid        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, course_id, id),
    UNIQUE (page_id, id),
    UNIQUE (page_id, number),
    FOREIGN KEY (org_id, course_id, page_id) REFERENCES pages (org_id, course_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (org_id, author_id) REFERENCES users (org_id, id)
);

ALTER TABLE pages ADD CONSTRAINT pages_current_version
    FOREIGN KEY (id, current_version_id) REFERENCES page_versions (page_id, id)
    DEFERRABLE INITIALLY DEFERRED;
```

Migration `migrations/<version>_page_render_cache.sql` (I2), version fixed the same way:

```sql
-- in <version>_pages.sql (I1): the course files a version's body names (not in the data model)
CREATE TABLE page_version_files (
    org_id          uuid        NOT NULL,
    course_id       uuid        NOT NULL,
    version_id      uuid        NOT NULL,
    course_file_id  uuid        NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (course_id, version_id, course_file_id),
    FOREIGN KEY (org_id, course_id, version_id) REFERENCES page_versions (org_id, course_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id, course_file_id) REFERENCES course_files (org_id, course_id, id)
);
CREATE INDEX page_version_files_by_file ON page_version_files (org_id, course_id, course_file_id);

-- <version>_page_render_cache.sql (I2): B15's rendered HTML per version (not in the data model)
CREATE TABLE page_renders (
    org_id            uuid        NOT NULL,
    version_id        uuid        PRIMARY KEY REFERENCES page_versions (id) ON DELETE CASCADE,
    renderer_version  integer     NOT NULL,      -- a renderer change invalidates every row
    html              text        NOT NULL,      -- image URLs as placeholders, never a token
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
```

A trigger refuses `UPDATE` of `page_versions.body`, `number`, `page_id`, `course_id` and
`author_id`, so versions stay immutable (B2). `page_version_files` is written on save from the parsed body, so
`06` B11's `file.in_use` covers images used by any version (its foreign key's `NO ACTION` refuses
deleting such a course file) and B20's copy maps them without reparsing. A `page_renders` row is
written on the first read of a version and replaced when `renderer_version` in code is higher;
its `html` holds `data-sc-image="<course_file_id>"` placeholders that the read fills with signed
URLs (B15).

Coordinator decisions (overnight, 2026-11-17, I1), pending owner review:

- The page's current version is enforced by the schema: `current_version_id` is NOT NULL and its
  deferred foreign key names `(id, current_version_id)`, so the current version is always one of
  the page's own (the course columns are in the decisions after the review, below). The trigger
  is `page_versions_immutable`.
- Node and mark names are Tiptap's (`paragraph`, `heading`, `bulletList`, `orderedList`,
  `listItem`, `blockquote`, `horizontalRule`, `hardBreak`, `codeBlock`, `table`, `tableRow`,
  `tableHeader`, `tableCell`, `image`, `text`; marks `bold`, `italic`, `underline`, `strike`,
  `code`, `link`, `subscript`, `superscript`). The nodes Tiptap does not ship are `callout`
  (`kind`), `videoEmbed` (`provider`, `video_id`, `title`), `transcript` and `transcriptLine`
  (`speaker`); `image` takes `course_file_id`, `alt` and `decorative` (`false` when omitted).
  The other attributes are `heading.level`, `orderedList.start` (0 or more, 1 when omitted),
  `codeBlock.language` and the cells' `colspan`, `rowspan` and `colwidth`; `link` takes `href`
  only, since B9's `rel` and `target` are the renderer's. `stepclimb_core::page_doc` holds the
  list, and the OpenAPI component `PageDoc` describes it.
- Where blocks go: the document takes every block; a blockquote, a callout, a list item and a
  table cell take paragraphs, bullet and ordered lists and code blocks only, and a list item
  starts with a paragraph. Images, embeds, tables, callouts and transcripts are top-level only.
  A node that holds blocks, rows, cells or lines (document, lists, list items, blockquote,
  callout, table, rows, cells, transcript) holds at least one.
- The code block languages are `bash`, `c`, `cpp`, `csharp`, `css`, `go`, `html`, `java`,
  `javascript`, `json`, `kotlin`, `markdown`, `php`, `python`, `ruby`, `rust`, `sql`, `swift`,
  `typescript`, `xml` and `yaml`, or `null`. A code block's text nodes are stored joined as one.
- Column widths are not kept in M1: `colwidth` must be `null` (what Tiptap writes for a column
  nobody resized), so I3 turns table resizing off.
- A video embed's `title`, its iframe's accessible name (B16), is required: 1 to 200 characters,
  not blank. A Vimeo id is 1 to 20 digits. A speaker label, a video title and an image's `alt`
  hold no control character at all; an `alt` of only white space counts as empty.
- The stored body is the parsed document written back with every attribute spelled out, so B3's
  "equal body" compares canonical forms.
- Slugs: B1's derivation takes the title's compatibility decomposition, keeps ASCII letters and
  digits (lowercased), drops other non-ASCII characters and joins the runs with single hyphens,
  cut to 100. A title that derives no slug (only non-ASCII letters, say) answers 422
  `page.slug_invalid` at `/slug`, which also names a given slug that breaks B1's pattern. A
  derived slug that is taken answers 409 `page.slug_taken` like a given one; the server adds no
  suffix.
- `GET /api/v1/courses/{course_id}/pages` lists by slug, compared as bytes.
- `page.version_conflict` names the current version in a `current_version` member of the
  problem (`{ id, number, author_id }`), added to the shared `Problem` schema beside `limit`
  and `max`.
- A restore's `page.image_type` and `page.body_invalid` name the member under `/body`, the
  restored version's body, as a save's do.
- `GET /api/v1/pages/{page_id}/versions/{version_id}` answers the body JSON in I1; I2 adds
  `?format=html` with the renderer.
- B6's home page refusal comes with I2, which adds the `{ "kind": "page" }` home option (B19);
  until then `DELETE` refuses a page that a `page` item references.
- `course_files::referenced` also reads `page_version_files`, so deleting a course file any
  version names answers 409 `file.in_use` (`06` B11) rather than the foreign key's error.

Coordinator decisions (overnight, 2026-11-17, after the review of #320), pending owner review:

- Coordinator decision (overnight, 2026-11-17): B7's 1 MiB applies to the stored form, the
  compact JSON of the document with every attribute written out, measured when a body is saved
  or created. A body that is under 1 MiB as submitted but over it once stored (9,900 cells
  without attributes, say) answers 422 `page.body_invalid` at `/body`, so every stored version
  can be restored and saved again. Bodies read back from the database (a restore) get the
  structural parse only, not the size check. The submitted JSON is bounded by the request body
  limit, and serde_json refuses nesting deeper than 128 levels (400 `request.malformed`).
- Coordinator decision (overnight, 2026-11-17): link targets are stored normalized. An absolute
  `https`, `http` or `mailto` URL is stored as the `url` crate serializes it (`Url::as_str`:
  scheme and host lowercased, and in a path, query or fragment some characters
  percent-encoded), so `https://a.example/"onmouseover=alert(1)//` is stored as
  `https://a.example/%22onmouseover=alert(1)//`. That encoding does not cover every character
  that matters in HTML: a `mailto:` address is an opaque path where only control characters are
  encoded, a host may hold `"`, `'` and a backtick (and `%22` in a host is decoded to `"`), and
  `'` is never encoded in a path or fragment. So every target, absolute or a same-origin path, is
  checked once more in the form it would be stored in: one holding `"`, `'`, `<`, `>`, a
  backtick, `\` or a control character is refused (`/"><svg/onload=alert(1)>`,
  `mailto:a@example.com"><svg/onload=alert(1)>`, `https://a"onmouseover=alert(1).example/`,
  `https://a.example/it's`). I2's renderer must still escape every attribute it writes, and its
  T4 and T6 golden files must include these cases.
- Coordinator decision (overnight, 2026-11-17): `page_versions` and `page_version_files` carry
  `course_id`, held equal to the page's course by three-column foreign keys
  (`(org_id, course_id, page_id)` to `pages`, `(org_id, course_id, version_id)` to
  `page_versions`, `(org_id, course_id, course_file_id)` to `course_files`, which gains
  `UNIQUE (org_id, course_id, id)`). The database itself refuses a version naming another
  course's file. `page_version_files`'s key is `(course_id, version_id, course_file_id)`.
- Coordinator decision (overnight, 2026-11-17): the server stays strict about attributes. For
  I3: the editor strips Tiptap Link's default `target`, `rel` and `class` attributes, and
  `orderedList`'s `type` where the Tiptap version writes it, before it saves; the server refuses
  them as unknown attributes.

Coordinator decisions (overnight, 2026-11-17, I3), pending owner review:

- Coordinator decision (overnight, 2026-11-17): the authoring screen is
  `/c/:course/pages/:page/edit`. Until I2 adds `GET /api/v1/pages/{page_id}?format=json`, it loads
  the newest version from `GET /pages/{page_id}/versions?limit=1` and its body from
  `GET /pages/{page_id}/versions/{version_id}`: every save and restore makes the version it writes
  current and numbers it after the others (B3, B5), so the newest version is the current one. The
  versions routes do not answer the page's title, so the screen is titled "Edit page". I2 may move
  the load to `?format=json`.
- Coordinator decision (overnight, 2026-11-17): the editor uses `@tiptap/core` 3.32.2 and the
  extension packages for B8's nodes and marks, without `@tiptap/react` (the editor mounts the core
  editor itself and draws its blocks as plain DOM) and without `@tiptap/extension-link` (its own
  link mark has only `href`, so `linkifyjs` stays out). `toPageDoc` keeps exactly the attributes
  `page_doc` lists, written out, so Link's `target`, `rel` and `class`, `orderedList`'s `type` and
  the cells' `align` never reach a save. The editor is the subpath `@stepclimb/ui/editor`, and the
  web app loads the screen lazily, so Tiptap is in the screen's own chunk.
- Coordinator decision (overnight, 2026-11-17): `crates/stepclimb-core/tests/fixtures/page_doc/`
  holds the editor's contract with the server: a document with every node, mark and attribute, the
  output of inserting every block, the output of a hostile paste, and the link cases. Vitest
  writes or compares them from the editor and `tests/page_doc_editor.rs` parses them with
  `PageDoc::parse_new` and requires the stored form to equal the file.
- Coordinator decision (overnight, 2026-11-17): the slash menu opens for `/` typed at the start of
  an empty paragraph, and is the same block list as the toolbar's "Insert" (a popover from 768 px,
  a bottom sheet under it). Escape types the `/`. A block that may be nested (paragraph, lists,
  code block) goes after the block holding the caret; any other goes after the top-level block
  holding it. Either takes the place of the empty paragraph the `/` was typed in, where it may
  stand. A table is inserted with three columns, a header row and two rows; table resizing is
  off. A new transcript's first speaker is "Speaker" from the strings catalog, and a new line keeps
  the speaker of the line it was split from.
- Coordinator decision (overnight, 2026-11-17): the block toolbar is a bar under the main toolbar,
  not a floating one, so it is reachable from the keyboard and on a phone. The actions (Save and
  the save state) and the main toolbar stay at the top while the author scrolls at every width; the
  block bar stays too from 768 px. Under 768 px the main toolbar is one row that scrolls sideways,
  so the sticky part is two rows high when the software keyboard is open.
- Coordinator decision (overnight, 2026-11-17): an image is uploaded through spec 06's upload
  route into `/_page-images` under its own name after a random 8-hex-digit prefix, so two images
  with the same name do not collide (`file.name_taken`). The editor inserts it only when the type
  the server sniffed is one of B10a's, and asks for its text alternative or "decorative" first.
  Spec 06's signed URL route is not built yet, so the editor draws an image as a labelled block
  with its text alternative, not a preview.
- Coordinator decision (overnight, 2026-11-17): paste is parsed into the editor's schema, so
  markup it does not name is left out (a script, event handlers, styles, an `<img>` or an
  `<iframe>` from elsewhere). `<h1>`, `<h5>` and `<h6>` paste as paragraphs, a code language off
  the list as none, and an image only when it carries a course file id (copied from a Stepclimb
  page). Control characters other than tab and newline are taken out of pasted text.
- Coordinator decision (overnight, 2026-11-17): unsaved work (B4) is one draft per page in local
  storage, `stepclimb.page-draft.<page_id>`, holding the body and its base version. It is written
  1 s after the author's last change while the body differs from the saved one, cleared on a save
  or on "Discard", and offered when the page is opened again with a body that differs from the
  loaded one. A restored draft keeps its base version, so saving a draft based on an older version
  meets B3's conflict check. Ctrl or Cmd + S saves from anywhere on the screen.
- Coordinator decision (overnight, 2026-11-17): on `page.version_conflict` the screen keeps the
  author's work and offers "Save my changes as the newest version", which saves again with the
  conflict's `current_version` as the base (the other author's version stays in the history), or
  "Load version N", which asks first and then drops the author's changes.

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
- **T7** (B10, B10a, B15). An image of another course answers 404 `file.not_found`; an image
  without `alt` and not decorative 422. A rendered image's URL serves the file; two
  requests get two different URLs; the cache row contains no `/files/` token.
- **T7a** (B10a). Course files uploaded through spec 06's route with fixture bytes: a PNG, a JPEG, a
  GIF and a WebP each save as an image (200). A PDF, an SVG, a plain-text file, a file whose bytes
  match no signature (`application/octet-stream`) and a PDF named `photo.png` uploaded with
  `Content-Type: image/png` each answer 422 `request.invalid` with `page.image_type` at the image
  node's `/attrs/course_file_id`, and no version is written. Restoring a version that names the PDF
  (inserted by the test in SQL) answers the same 422. Mutation: removing the type check turns this
  test red.
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

Q1, Q2 and Q4 were delegated by the owner to the coordinator; each recommendation is taken and
stands until the owner reviews it. Q3 narrowed the plan's M1 page blocks, and Q5 was put to the
owner after a review; the owner answered both on 2026-11-16.

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
- **Q3. "Check your understanding" questions in M1.** They need question types
  from `stepclimb-assess` (M2). Options: (a) leave them to M2, which adds the node; (b) a simple
  multiple-choice node in M1 graded in the browser; (c) a placeholder node. Recommendation: (a); (b)
  would build a second question model that M2 replaces. The plan lists these questions among M1 page
  blocks, so moving them to M2 is the owner's call.
  Owner decision (2026-11-16): (a), check-your-understanding questions move to M2, which adds the
  node (B13).
- **Q4. How VersionDiff compares.** Options: (a) top-level blocks compared as JSON, changed blocks
  marked; (b) a text diff inside blocks; (c) no diff in M1. Recommendation: (a), no dependency, and
  enough to see what changed between saves.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Whether inserting an image refuses a file that is not an image.** Spec 06 sniffs a
  file's type and never refuses one, so without a check a page image could be any file the
  uploader picked. Options: (a) the server refuses a non-image file when it is inserted as an
  image, and only there; (b) no check, and a non-image file shows as a broken image.
  Recommendation: (a).
  Owner decision (2026-11-16): (a), inserting an image refuses non-image types on the server
  (B10a).

## Issues

1. **I1. Page storage, versions and the document model.** Migration, `page_doc` typed model and
   validation, page CRUD, saves with the conflict check, versions list, read and restore,
   `PageTarget`, `page_version_files` (in I1's migration), the image type check on save and
   restore. Covers B1 to B11 (validation), B10a, B12, B13; T1 to T5, T7a, T10, T11, T14 and T15 for
   these routes. Depends on: `03-modules-and-items.md` I1,
   `06-files.md` I2.
2. **I2. Rendering and reading.** The renderer, `ammonia` (pinned after `dependency-researcher`, in
   its own commit), the `page_renders` cache migration with image placeholders, signed image URLs,
   embeds, `GET /pages/{id}` with B18's access rules and the lock suite entry, `page` items and the
   home page option, copying pages. Covers B14 to B20; T6 to T9, T12. Depends on: I1, `06-files.md`
   I3, `04-unlock-and-completion-rules.md` I3, `02-courses-and-enrollments.md` I3.
3. **I3. The editor.** `packages/ui/src/editor` with Tiptap (pinned after `dependency-researcher`,
   in its own commit), toolbar, slash menu, block toolbar, local unsaved-work recovery, the
   authoring screen. Covers B4, B21; T13 (editor). Depends on: I1.
4. **I4. PageReader and VersionDiff.** The reading view with Mark as done, Previous and Next, and
   the version history screen. Covers B22, B23; T13 (reader). Depends on: I2,
   `05-progress-and-events.md` I1.
