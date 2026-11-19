# Files

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (File: "Stored once per unique file
  (SHA-256) in per-course folders. Served through short-lived signed URLs with an inline PDF
  preview."), Architecture and stack (Storage: the `object_store` crate), Data model (`files`,
  `course_files`), Plans and billing (limits: storage), Security and privacy (uploads type-sniffed,
  served with `nosniff`, optional separate download origin, optional ClamAV hook), Hosting and
  operations (Compose with optional MinIO); ADR 0019
- **Depends on:** `02-courses-and-enrollments.md` (courses, course permissions, archived courses,
  cover), `03-modules-and-items.md` (the `file` item kind), `04-unlock-and-completion-rules.md`
  (locks, the lock suite), `01-job-queue-and-outbox.md` (periodic cleanup),
  `../m0/10-entitlements-framework.md` (limits, `check_room`), `../m0/08-sessions-and-csrf.md`
  (the encryption key ring)
- **Review:** maintainer line-by-line (signed delivery and who may fetch a file)

## Summary

Course authors upload files (handouts, PDFs, images) into folders of a course; learners reach them
through `file` items in modules. Each distinct file is stored once per organization, keyed by its
SHA-256, in object storage through the `object_store` crate (a local directory, S3, MinIO and the
other backends it supports), and the same bytes uploaded twice cost the storage limit once. Files
are never served from a public path: the server hands out a short-lived signed URL, checks it on
each fetch, and serves the bytes with headers that stop the browser from running them. PDFs open
inline in a preview. A plan's `storage` limit bounds what an organization stores.

## Behavior

### Storage

- **B1.** The server writes and reads file bytes through one `object_store` store built from
  configuration at startup (Q1): `storage_url` (`file:///path` or `s3://bucket[/prefix]`) and, for
  S3-compatible stores, `storage_s3_endpoint`, `storage_s3_region`,
  `storage_s3_access_key_id`, `storage_s3_secret_access_key` (with `_FILE`) and
  `storage_s3_allow_http` (for a local MinIO). When `storage_url` is set, `serve` checks at
  startup that it can write, read and delete a probe object under `<prefix>/.probe/` and refuses
  to start, naming the probe failure, when it cannot; S3 settings without `storage_url`, or a
  `storage_url` that does not parse, stop startup naming the setting. `check-config` validates the
  values without connecting.
- **B1a.** Storage is optional, so an M0 installation upgrades without new settings (Q1). Without
  `storage_url` the server starts, logs once at `warn` that storage is not configured and names
  `STEPCLIMB_STORAGE_URL`, and `check-config` prints `storage: not configured
  (STEPCLIMB_STORAGE_URL)`. Every route that reads or writes stored bytes (upload, the signed URL
  routes, `GET /files/<token>`, and spec 08's uploads) answers 503 `storage.not_configured`, whose
  `detail` names `STEPCLIMB_STORAGE_URL`; a course's `cover_url` is `null`. This is an explicit
  error on each use, not a fallback: nothing is written anywhere else. Routes that touch only rows
  (listing, rename, delete of course files that exist) keep working, and `files.cleanup` skips its
  object deletes and logs once per run. `/readyz` does not depend on storage.
- **B2.** A stored file's key is `orgs/<org_id>/files/<sha256 hex>`. Keys never contain a client
  name or folder, so no client string reaches a storage path.

### Uploading

- **B3.** `POST /api/v1/courses/{course_id}/files?folder=<path>&name=<name>` takes the file as the
  raw request body (`Content-Type` is the client's guess and is not trusted) with a
  `Content-Length`, and needs `perm::CourseContentEdit` at the course; an archived course answers
  409 `course.archived`. A missing `Content-Length` answers 411 `file.length_required`.
- **B4.** Before reading the body the server refuses, in order: a length above `max_file_bytes`
  (instance setting, default 100 MiB, 1 MiB to 5 GiB; 413 `file.too_large`, Q2); a `name` or
  `folder` that breaks B9's rules (422); a name already used in that folder (409 `file.name_taken`);
  and a length that does not fit the plan's `storage` limit
  (`Entitlements::check_room(Limit::Storage, length)`, 403 `entitlement.limit_reached`, `m0/10`
  B10). The route's body limit is `max_file_bytes`, replacing axum's 2 MiB for this route only.
- **B5.** The server streams the body to a temporary key `orgs/<org_id>/uploads/<uuid>` with a
  multipart upload while computing its SHA-256 and counting its bytes; a body whose byte count
  differs from `Content-Length` answers 400 `request.malformed` and the temporary object is
  deleted. Nothing is buffered whole in memory.
- **B6.** The server sniffs the type from the first 8 KiB (Q3): a fixed list of signatures (PDF,
  PNG, JPEG, GIF, WebP, MP4, WebM, MP3, OGG, ZIP-based office formats, plain UTF-8 text). The
  stored `mime` is the sniffed type, or `application/octet-stream` when none matches; the
  client's `Content-Type` and the name's extension are ignored for it.
- **B7.** Then the server stores the file in this order:
  1. In one transaction, lock the organization's `org_plans` row and run `check_room` again with
     the real size, or 0 when the organization already has a `files` row for the hash (read
     `FOR KEY SHARE`, so cleanup cannot delete it meanwhile, B21).
  2. When the hash is new, copy the temporary object to the file's key and insert the `files`
     row (`UNIQUE (org_id, sha256)`); then insert the `course_files` row, and commit. A committed
     `files` row therefore always has its object.
  3. Delete the temporary object.

  A failure before the commit deletes the temporary object and, when the hash was new, the
  copied object, unless another upload committed the same hash meanwhile (checked by the row's
  existence after the rollback). Two uploads of the same new bytes race on the unique index: the
  loser retries step 1 once, finds the row, and adds 0.
- **B8.** Temporary objects older than 24 h (a crashed upload) are deleted by the periodic job
  `files.cleanup` (hourly, `01` B17), which lists each organization's `uploads/` prefix.

### Folders and names

- **B9.** A course file has a `folder` and a `name`. `folder` is `/` or a path of 1 to 8 segments
  (`/handouts/week-1`), each 1 to 100 characters, not `.` or `..`, with no `/`, `\`, NUL or other
  control character, and no Unicode format character (general category Cf: bidirectional
  overrides and isolates, zero-width characters, the byte order mark), which would let one name
  show as another; `name` follows the segment rule and is at most 255 characters. Both are
  stored NFC-normalized and compared exactly; `(course_id, folder, name)` is unique. A folder
  exists while a file is in it (Q4); there are no empty folders.
- **B10.** `PATCH /api/v1/course-files/{course_file_id}` with `{ folder?, name? }` renames or moves
  a file within its course (B9's checks, 409 `file.name_taken`). `POST
  /api/v1/courses/{course_id}/folders/rename` with `{ from, to }` renames a folder and everything
  under it in one statement (409 `file.name_taken` if any moved path collides). Both need
  `CourseContentEdit`.
- **B11.** `DELETE /api/v1/course-files/{course_file_id}` removes the course file, needing
  `CourseContentEdit`. A course file that a `file` item references, or that is the course's
  cover, answers 409 `file.in_use`. The `files` row and its object are removed by `files.cleanup`
  once nothing references them (B21).
- **B12.** `GET /api/v1/courses/{course_id}/files?folder=<path>&cursor&limit` lists the files and
  sub-folders directly in a folder, sub-folders first by name, then files by name, and needs
  `perm::CourseViewAll` (Q5). Each file carries `id`, `folder`, `name`, `size`, `mime`, `sha256`
  and timestamps.

### Reaching a file

- **B13.** `file` items become creatable (`03` B5): `ref_id` is a `course_files` id of the same
  course (404 `file.not_found` otherwise).
- **B14.** `POST /api/v1/items/{item_id}/file-url` with `{ "disposition": "inline" | "attachment"
  }` returns a signed URL for a `file` item's file. It needs `perm::CourseView` at the course and
  runs `03` B14's visibility and `04` B13's lock check (403 `item.locked`) for a caller without
  `CourseViewAll`. It is a `POST` because each call issues a new credential.
- **B15.** `POST /api/v1/course-files/{course_file_id}/url` does the same for any course file and
  needs `perm::CourseViewAll` (authors and staff, for the files area and the editor). It is
  listed as staff-only in `04`'s lock suite.
- **B16.** A signed URL is `/files/<token>`, where the token seals `{ org_id, file_id,
  course_file_id, disposition, expires_at }` with the encryption key ring
  (`stepclimb_auth::encryption`, context `[b"file_url"]`) and is base64url-encoded (Q6). It is valid
  until `expires_at`, 10 minutes after issue by the request's clock (Q7), and the response carries
  `expires_at`. Anyone holding the URL can fetch the file until then; that is what lets `<img>`,
  `<iframe>` and the browser's PDF viewer load it without a cookie. Key rotation keeps working while
  an old key stays in the ring.
- **B17.** `GET /files/<token>` (no session, no CSRF check; a public route for the walker, `09`
  B21) opens the token, refuses one that does not open or has expired with 404 `file.not_found`
  (one answer for both, so a token reveals nothing), checks that the course file still exists in
  the token's organization (404 otherwise), and streams the object. Headers:
  `Content-Type` the stored `mime`; `X-Content-Type-Options: nosniff`; `Content-Disposition`
  `attachment` with the course file's name (RFC 6266 `filename*`), or `inline` only when the
  token says so and the type is PDF or a PNG, JPEG, GIF or WebP image (otherwise `attachment`
  whatever the token says); `Content-Security-Policy: default-src 'none'; style-src
  'unsafe-inline'; sandbox` for every type but PDF, which gets `default-src 'none'` without
  `sandbox` (Q8); `Cache-Control: private, max-age=<seconds until expiry>`; `ETag` the SHA-256;
  `Cross-Origin-Resource-Policy: same-origin`; `Referrer-Policy: no-referrer`. It honors one
  `Range` (206 with `Content-Range`, 416 for an unsatisfiable range) and `If-None-Match` (304).
- **B18.** The web app shows a PDF item inline (Q8): an `<iframe>` whose `src` is an inline
  signed URL, with a download button that asks for an `attachment` URL, and a fallback message
  with the download button where the browser shows no PDF. Images are shown with `<img>`; every
  other type offers the download only.

### Cover

- **B19.** `PATCH /api/v1/courses/{course_id}` accepts `cover_course_file_id` (`02` B7): a course
  file of the same course whose type is PNG, JPEG or WebP (422 `course.cover_invalid` otherwise),
  or `null`. `GET` a course returns `cover_url`, a signed inline URL (B16) issued for the caller
  when a cover is set and the caller can read the course, or `null`.

### Copy and cleanup

- **B20.** `courses::copy` (`02` B13) copies every `course_files` row to the new course, pointing
  at the same `files` rows, and maps the `ref_id` of `file` items and the cover to the copies. The
  storage limit is not charged again, since no byte is added.
- **B21.** `files.cleanup` (B8) also deletes `files` rows that no `course_files` row and no row of
  a later referencing table (spec 08's videos and captions add theirs to one list in
  `files::unreferenced`) has referenced for 24 hours, then deletes their objects after the commit.
  The delete re-checks under `FOR UPDATE` of the `files` row; an upload that reuses the hash takes
  `FOR KEY SHARE` on it (B7), so the two cannot interleave. An object whose delete fails is logged
  and retried on the next run (a key for which no row exists).
- **B22.** `Limit::Storage` (`storage`, bytes) counts the sum of `size` over the organization's
  `files` rows (`files::count_for_limit`), so a file shared by several courses counts once.
  `m0/10` B2 names the limit; this spec adds it to `feature::Limit`.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals; writes in an archived course answer 409 `course.archived`.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /api/v1/courses/{course_id}/files` | `?folder&name`, raw body, `Content-Length` | 201 `CourseFile`, `Location` | 503 `storage.not_configured`, 404 `course.not_found`, 409 `file.name_taken`, 411 `file.length_required`, 413 `file.too_large`, 403 `entitlement.limit_reached`, 422 `request.invalid` | `perm::CourseContentEdit` (course) |
| `GET /api/v1/courses/{course_id}/files` | `?folder&cursor&limit` | 200 `Page<FolderEntry>` | 404 `course.not_found`, 422 `request.invalid` | `perm::CourseViewAll` (course) |
| `POST /api/v1/courses/{course_id}/folders/rename` | `{ from, to }` | 200 `{ moved: integer }` | 404 `course.not_found`, 404 `folder.not_found`, 409 `file.name_taken`, 422 `request.invalid` | `perm::CourseContentEdit` (course) |
| `GET /api/v1/course-files/{course_file_id}` | | 200 `CourseFile` | 404 `file.not_found` | `perm::CourseViewAll` (the file's course) |
| `PATCH /api/v1/course-files/{course_file_id}` | `{ folder?, name? }` | 200 `CourseFile` | 404 `file.not_found`, 409 `file.name_taken`, 422 `request.invalid` | `perm::CourseContentEdit` (the file's course) |
| `DELETE /api/v1/course-files/{course_file_id}` | | 204 | 404 `file.not_found`, 409 `file.in_use` | `perm::CourseContentEdit` (the file's course) |
| `POST /api/v1/course-files/{course_file_id}/url` | `{ disposition }` | 200 `SignedUrl` | 404 `file.not_found` | `perm::CourseViewAll` (the file's course) |
| `POST /api/v1/items/{item_id}/file-url` | `{ disposition }` | 200 `SignedUrl` | 404 `item.not_found`, 403 `item.locked`, 422 `request.invalid` (not a file item) | `perm::CourseView` (the item's course) |
| `GET /files/{token}` | `Range?`, `If-None-Match?` | 200, 206 or 304 with the bytes | 404 `file.not_found`, 416 `file.range_invalid` | none: the token is the credential |

Every route above that reads or writes stored bytes (upload, both URL routes, `GET /files/{token}`)
also answers 503 `storage.not_configured` when storage is not configured (B1a).

Schemas: `CourseFile { id, course_id, folder, name, size, mime, sha256, created_at, updated_at }`;
`FolderEntry` is `{ kind: "folder", path, name }` or `{ kind: "file", file: CourseFile }`;
`SignedUrl { url, expires_at, disposition }`. `CourseFileTarget` resolves `course_file_id` to its
course and then acts as `CourseTarget` with code `file.not_found`. `Course` gains
`cover_course_file_id` and `cover_url`.

Configuration added (ADR 0022; secrets with `_FILE`; all in `check-config`):

| Variable | File key | Default |
| --- | --- | --- |
| `STEPCLIMB_STORAGE_URL` | `storage_url` | none: unset means "not configured" (B1a, Q1) |
| `STEPCLIMB_STORAGE_S3_ENDPOINT` | `storage_s3_endpoint` | the provider's |
| `STEPCLIMB_STORAGE_S3_REGION` | `storage_s3_region` | `us-east-1` |
| `STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID` | `storage_s3_access_key_id` | required with an `s3://` URL |
| `STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY(_FILE)` | `storage_s3_secret_access_key(_file)` | required with an `s3://` URL |
| `STEPCLIMB_STORAGE_S3_ALLOW_HTTP` | `storage_s3_allow_http` | `false` |
| `STEPCLIMB_MAX_FILE_BYTES` | `max_file_bytes` | 104857600 (100 MiB) |

The Compose files set `storage_url`: the development stack to MinIO (`s3://stepclimb` with the
MinIO endpoint, ADR 0019), the production example to `file:///var/lib/stepclimb/storage` on a
named volume, or MinIO under its profile.

Coordinator decisions (overnight, 2026-11-16, I1), pending owner review:

- The `storage_s3_*` settings are refused, naming the first one set, when `storage_url` is unset
  or a `file://` URL, so a half-configured store never starts as "not configured". An `s3://` URL
  needs both access key settings: the server reads no AWS environment variables or instance
  profile, so what it connects with is always what `check-config` shows.
- `serve` opens and probes the store before it connects to the database. A `file://` directory
  must exist; the local store writes with `fsync`, so an object a committed `files` row names
  survives a crash (B7). The S3 client is the server's own reqwest client with
  `stepclimb_auth::outbound`'s TLS setup (rustls, ring, the platform verifier), no proxy and no
  redirects, 5 s to connect and 30 s per read, and object_store's retry capped at 3 retries within
  15 s, so a store that is down stops startup within seconds.
- I1 sets the Compose files as above. The MinIO service of `deploy/compose/compose.yaml` makes
  the bucket `stepclimb` with `mkdir /data/stepclimb` before MinIO starts (a top-level directory
  of the data directory is a bucket to MinIO, on a new volume and an existing one; the image has
  no `mc`) and has a health check the server waits for. The gate's own project
  (`deploy/compose/ci.yaml`) runs the same service in memory on its slot's port, for the storage
  tests and the e2e server.
- I1 adds `files::{find_by_hash, insert, count_for_limit}`, which `Limit::Storage` and B7 use,
  and the `course_files` table. `files::{unreferenced, delete}` and the `course_files` repository
  come with I2, the first issue that calls them, and so does `max_file_bytes`, which only the
  upload route reads.
- The hash and size of a `files` row are the server's (B5): `files::NewFile` takes a
  `files::Hashed`, which only `files::Sha256Hasher::finish` makes, over the bytes fed to it.
  I2's upload route feeds the hasher every chunk of the body as it streams it to the temporary
  key, and builds the `NewFile` from what `finish` returns; it never builds a digest from
  anything the client sent.
- A plan row without a `storage` key reads as 0 (`m0/10`'s reading of a missing limit), so a
  non-`unlimited` plan refuses uploads until its row sets `storage`. Self-hosted instances run
  `unlimited`, which leaves it `null`.

Coordinator decisions (overnight, 2026-11-16, I2), pending owner review:

- B4's limit check before the body, read with T3, would refuse re-uploading a stored file whose
  bytes do not fit as new bytes, which B7 then accepts at no cost. The check before the body
  therefore refuses only when the length does not fit and the organization holds no file of
  exactly that length; B7's check with the real size refuses the rest after the body is
  streamed, and the upload leaves no object behind.
- B7's order changes: under the hash lock alone the upload looks the hash up and, for a new hash,
  copies the temporary object to the file's key; only then does it lock the course, run
  `check_room` (with the `org_plans` lock) and insert the rows. The store copies up to 5 GiB, and
  no course or plan row stays locked meanwhile. A `storage` limit refused after the copy deletes
  the copy under the hash lock, as any other failure before the commit does.
- Uploads of the same bytes take turns on a transaction-scoped advisory lock on the
  organization's hash (`files::lock_hash`), taken before the `files` row is looked up and held
  to the commit. The loser of a race finds the winner's row and adds 0, so B7's retry after a
  unique-index failure never happens, and a failed upload deletes the copy of a new hash while
  it still holds the lock, when no other upload can have committed a row naming it. Without the
  lock, an upload that failed while another upload of the same new bytes had copied but not yet
  committed would delete an object the other's row is about to name.
- `files.cleanup` holds the same lock as a session lock from before it rechecks a row until
  the row's object is deleted after the commit, so an upload that reuses a hash cleanup has just
  deleted copies the object again rather than losing it. Its third step lists
  `orgs/<org_id>/files/` and deletes each object no row names, under the lock and after
  checking again: the retry of B21's failed deletes, and the copy of an upload whose commit
  failed. It deletes at most 1,000 unreferenced rows per organization per run.
- "Unreferenced for 24 hours" is read from a new column, `files.unreferenced_since`, which
  deleting a course file stamps with the time of the delete (migration
  `20261117110000_files_unreferenced_since.sql`). A row that never lost a reference counts from
  its `created_at`.
- `folder` may be omitted on the upload and the listing and means `/`. The 422 field codes are
  `file.folder_invalid`, `file.folder_too_long` (a segment over 100 characters),
  `file.folder_too_deep`, `file.name_invalid`, `file.name_too_long`, and, for the folder rename,
  `file.folder_root`: neither `from` nor `to` may be `/`. Names sort as bytes (`COLLATE "C"`).
- B6's sniffer reads OGG as `audio/ogg` when its first page starts a Vorbis or Opus stream and
  `application/ogg` otherwise, MP4 only for MP4 `ftyp` brands, WebM only for an EBML `webm`
  document type, and office files only for an Office Open XML or OpenDocument archive; a plain
  ZIP is `application/octet-stream`. Text that starts with `<` (HTML, SVG, XML) and an empty
  file are `application/octet-stream`, so no markup is ever labeled as text a browser shows.
- `max_file_bytes` is at most 5 GiB, the largest object one S3 copy request takes (B7 copies the
  temporary object to its file's key), and the server carries it with the store, since only the
  upload route reads it. NFC normalization uses `unicode-normalization` 0.1.26
  (`(MIT OR Apache-2.0) AND Unicode-3.0`), which the lockfile already held through sqlx and
  webauthn-rs.
- An upload body that sends no frame for 30 seconds answers 408 `file.body_timeout`, and its
  multipart upload is aborted, so a client cannot hold an upload open forever. One server
  streams at most 16 uploads at once; one more answers 503 `file.uploads_busy` without reading
  its body. An upload holds at most three 5 MiB parts and one body frame in memory, about 15 MiB,
  so 16 hold about 240 MiB. Both bounds are constants (`storage::UploadLimits::DEFAULT`), not
  settings, until an operator needs another value.
- B9 refuses Unicode format characters (Cf) in folder segments and names, as item URLs and group
  keys do, in `course_files::{Folder, Name}` and in a table check
  (`course_files_no_format_characters`).
- A server that stops in the middle of an S3 upload leaves an incomplete multipart upload, which
  is not an object and which `files.cleanup` cannot see. `deploy/compose/README.md` asks AWS S3
  operators for an `AbortIncompleteMultipartUpload` lifecycle rule.
- B11's `file.in_use` covers `file` items here; the cover joins it with I4, which adds the
  column. `GET /api/v1/course-files/{course_file_id}` from the API table comes with I2, as part
  of the files area.

Coordinator decisions (2026-11-19, I3), pending owner review:

- `SignedUrl` also carries the course file's `name`, `mime` and `size`. A learner cannot read
  the course file (`perm::CourseViewAll`, Q5), and the item view needs the type to choose a
  frame, an image or a download (B18); the holder of the URL gets the same three in the served
  headers anyway. `url` is absolute, the server's public URL then `/files/<token>`, so a client
  on another origin (the Tauri shell) can use it as it is.
- The token seals `{ o, f, c, d, e }` (organization, file, course file, disposition, expiry in
  whole seconds since the epoch) as JSON under the context `file_url`; it is valid strictly
  before `e`. Tokens longer than 1,024 characters are refused before decoding.
- `GET /files/<token>` answers 503 `storage.not_configured` first, then resolves the request's
  organization (`RequestOrg`, as every unauthenticated route does) and refuses a token sealed for
  another organization with the same 404 as any other unusable token, so a token is bound to its
  organization twice: in the sealed payload and by the host it is fetched from. The course
  file's `file_id` must still be the token's.
- For a caller without `course.view_all`, the item URL route checks visibility and the lock
  before the item's kind, so an unpublished or locked item answers 404 or 403 whatever its kind.
  An item that is not a `file` item answers 422 `request.invalid` with the field code
  `item.not_a_file` at the body's root (``""``). An Observer, who has no progress, is held to
  the lock as a learner who has done nothing (`05` B18).
- A `Range` this server does not take (several ranges, another unit, a malformed one) is
  ignored and the whole file is served, as RFC 9110 allows; `bytes=-0` and a range starting at
  or after the end answer 416 with `Content-Range: bytes */<size>`. `If-None-Match` compares
  weakly and `*` matches; the 304 carries the same headers but the body's. The `ETag` is the
  quoted SHA-256 in hexadecimal. Every answer carries `Accept-Ranges: bytes`.
- `Content-Disposition` carries both `filename`, an ASCII stand-in (every other character,
  `"` and `\` become `_`), and `filename*` (RFC 6266, UTF-8, percent-encoded). The PDF policy
  (no `sandbox`) applies to PDFs whatever the disposition.
- The request log span writes `/files/{token}` for every path under `/files/`, so a token never
  reaches a log line.
- A `file` item without `ref_id` answers 422 `item.ref_id_required` at `/ref_id`; a `ref_id`
  that is not a course file of the module's course answers 404 `file.not_found`. The check runs
  under the course lock, which deleting a course file also takes.
- B20's copy of course files and of `file` items' references comes with I3, not I4, so a course
  built through the API stays copyable (`items::Kind::copyable`): `courses::copy` copies every
  course file of the source (same stored file, folder and name) and points each copied `file`
  item at its course file's copy. The cover's mapping stays with I4, which adds the column.
- The web app's file view asks for an inline URL when it opens and reuses it for 9 minutes
  without refetching on focus, since a new URL would reload the preview; the download button
  asks for a new attachment URL on each press and hands it to the browser
  (`location.assign`), which saves it without leaving the page. Whether the browser shows PDFs
  is read from `navigator.pdfViewerEnabled`.

## Data

Migration `migrations/<version>_files.sql` (I1), and `migrations/<version>_course_cover.sql`
(I4). Versions are placeholders, fixed when each issue is implemented so each sorts after every
migration on `next` and every open branch.

```sql
-- <version>_files.sql
CREATE TABLE files (
    id           uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    sha256       bytea       NOT NULL CHECK (octet_length(sha256) = 32),
    size         bigint      NOT NULL CHECK (size >= 0),
    mime         text        NOT NULL CHECK (char_length(mime) BETWEEN 3 AND 100),
    storage_key  text        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, sha256),
    UNIQUE (org_id, id)
);

CREATE TABLE course_files (
    id          uuid        PRIMARY KEY,                    -- not in the data model: items and routes name a course file
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    file_id     uuid        NOT NULL,
    folder      text        NOT NULL CHECK (folder = '/' OR folder ~ '^(/[^/]+){1,8}$'),
    name        text        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255 AND position('/' in name) = 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (course_id, folder, name),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, file_id) REFERENCES files (org_id, id)
);
CREATE INDEX course_files_by_folder ON course_files (org_id, course_id, folder, name);
CREATE INDEX course_files_by_file ON course_files (file_id);

-- <version>_course_cover.sql
ALTER TABLE courses ADD COLUMN cover_course_file_id uuid,
    ADD FOREIGN KEY (org_id, cover_course_file_id) REFERENCES course_files (org_id, id);
```

`files` is per organization (`UNIQUE (org_id, sha256)`, Q9). `course_files.file_id`'s foreign key
with the default `NO ACTION` stops a referenced `files` row from being deleted (B21). `file`
items point at `course_files.id` through `module_items.ref_id`, checked by the repository and by
B11's `file.in_use`. Rows with `org_id` everywhere; the cover foreign key backs B11 for covers.

I2 adds `files.unreferenced_since timestamptz` (B21, decision above).

Repository functions (each takes `OrgScope`): `files::{find_by_hash, insert, count_for_limit,
unreferenced, delete}`, `course_files::{create, get, list_folder, update, rename_folder, delete,
referenced}`, and with I2 `files::{lock_hash, lock_hash_session, unlock_hash_session, exists,
any_of_size}` and `course_files::{course_of, name_taken}`. `stepclimb-media` is not involved: it
is spec 08's.

## Permissions

No new catalog entry: uploads and edits take `perm::CourseContentEdit`, the files area and staff
URLs `perm::CourseViewAll`, learner URLs `perm::CourseView` with the lock check
(`02-courses-and-enrollments.md`). Entitlements: `Limit::Storage` (`storage`, bytes), named by
`m0/10` B2 and added here; `unlimited` leaves it `null`.

## Test cases

Storage tests run against a MinIO in the gate's own Compose project (I1 adds it: today the gate's
stack has PostgreSQL only, and `deploy/compose/compose.yaml`'s MinIO serves `just up`) and against a
temporary local directory; no test reaches the network outside the stack. The permission suite gains
`CourseFileTarget`.

- **T1** (B1). With a store it cannot write, `serve` exits naming the probe failure; S3 settings
  without `storage_url` exit 2 naming it; `check-config` prints the storage settings with the
  secret redacted.
- **T1a** (B1a). Without `storage_url`, `serve` starts and answers `/readyz` 200; `check-config`
  prints `storage: not configured (STEPCLIMB_STORAGE_URL)` and exits 0; an upload, both signed URL
  routes and `GET /files/<token>` answer 503 `storage.not_configured` with `detail` naming the
  variable; listing course files answers 200. Mutation: making the upload route skip the check
  turns the test red.
- **T2** (B3, B4). Upload without `Content-Length` answers 411; with a length 1 byte over
  `max_file_bytes`, 413 before any byte is read (the test server sees the body unread); a name
  with `/`, a folder with `..` or 9 segments, and a 256-character name answer 422; a duplicate name
  in the folder answers 409 `file.name_taken`.
- **T3** (B4, B22). With a test plan whose `storage` is 1,000 bytes and 900 used, a 200-byte upload
  answers 403 `entitlement.limit_reached` with `limit` and `max`; re-uploading an existing 900-byte
  file's bytes into another course succeeds (it adds 0).
- **T4** (B5). A body shorter than its `Content-Length` answers 400 and leaves no object under
  `uploads/`.
- **T5** (B6). A PDF named `x.png` is stored as `application/pdf`; an HTML file sent as
  `image/png` is stored as `application/octet-stream`.
- **T6** (B7). The same bytes uploaded to two courses make one `files` row and one object and two
  `course_files` rows; two concurrent uploads of new identical bytes (two connections) end with one
  `files` row and its object present.
- **T7** (B7). A failure injected after the copy and before the commit leaves no `files` row and
  no object for a hash no one else has, and leaves the object for a hash another upload committed.
- **T8** (B8, B21). After the clock passes 24 h, `files.cleanup` deletes a stale temporary object
  and a `files` row whose last course file was deleted, with its object; it keeps a row still
  referenced by another course and one unreferenced for 23 h.
- **T9** (B10). Renaming folder `/a` to `/b` moves `/a/x` and `/a/c/y`; a collision with an
  existing `/b/x` answers 409 and moves nothing.
- **T10** (B11). Deleting a course file a `file` item uses, or the cover, answers 409
  `file.in_use`.
- **T11** (B12). The folder listing shows sub-folders, then files, each by name, paginated.
- **T12** (B14, `04` B13). A Student requesting a URL for a locked file item gets 403
  `item.locked`; for an unlocked one, a URL that serves the bytes; a Student requesting
  `/course-files/{id}/url` gets 403 `permission.denied`.
- **T13** (B16, B17). A token whose bytes are changed, one from org A's key ring opened on a server
  with another ring, and one fetched 1 s after `expires_at` (manual clock) each answer 404
  `file.not_found` with the same body; a token for a course file deleted since answers 404.
- **T14** (B17). Headers: a PDF with an inline token gets `inline`, `application/pdf` and no
  `sandbox`; an SVG or HTML file with an inline token gets `attachment`, `nosniff` and `sandbox`; a
  name with non-ASCII characters is encoded with `filename*`; `Range: bytes=0-99` answers 206 with
  100 bytes; `Range: bytes=999999-` answers 416; `If-None-Match` with the hash answers 304.
- **T15** (B18, Q8). Playwright on Chromium, Firefox and WebKit: a PDF item shows the preview
  frame with the PDF rendered (checked by a screenshot that is not the fallback) or, where a
  browser shows no inline PDF, the fallback with a working download; the download button's URL has
  `attachment`; an image item shows the image. Each browser's outcome is recorded in the PR, and a
  browser where neither works fails the test. Screenshots in the
  default and vZDC themes, light and dark, at 1280 and 375 px. The iOS WKWebView check runs with
  the spec 17 iPhone test and is recorded as not run until then.
- **T16** (B19). A PDF as cover answers 422 `course.cover_invalid`; a PNG cover gives `cover_url`
  that serves the image inline.
- **T17** (B20). A copied course lists the same files, its file items point at its own course
  files, and the organization's storage count is unchanged.
- **T18** (every `/api/v1` route above). Four cases: anonymous 401; every permission but the
  route's at the course 403; every permission at a sibling account 404; only the route's
  permission 2xx.
- **T19** (every route above). Cross-organization: org A's admin using org B's course, course file
  or item id gets 404, and org B's `files`, `course_files` and objects are unchanged. A signed URL
  issued in org B, fetched by anyone, serves org B's file only and cannot be edited into org A's:
  the sealed payload is authenticated, so an edited token answers 404.

## Out of scope

- Uploaded video (tus, transcoding, renditions, captions): spec 08. Linked video: spec 09.
- Images inside pages (the editor's uploads): spec 07, which stores them as course files.
- Organization logos and favicons (`../m0/12-tokens-and-theme-generator.md` Q6 (b)): a branding
  issue after this spec, not here.
- A separate download origin and the ClamAV hook, both "optional" in the plan: after M1, each its
  own spec when an owner asks.
- Presigned direct-to-S3 downloads and uploads: not built (Q6).
- Offline downloads of files: spec 15.
- Per-file quotas per course: not in the plan.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. Storage configuration and its default.** The plan names `object_store` and its backends,
  not settings. Options: (a) `storage_url` required, with no default, so `serve` refuses to start
  without it; (b) default to a local directory such as `/var/lib/stepclimb/storage`; (c)
  optional: the server starts without it, and every route that needs stored bytes answers 503
  `storage.not_configured` naming the setting, with `check-config` saying storage is not
  configured. Recommendation: (c). (a) would stop every M0 installation at its first M1 start,
  and (b) is a silent fallback that loses uploads when a container is replaced. (c) keeps the
  missing value an error naming the field (`code-quality.md`), raised where it matters, while the
  rest of the server keeps working. The Compose files set it, so new installations have storage.
  Coordinator decision (overnight, 2026-11-16, revised): (c), pending owner review. It replaces
  the earlier (a), which broke upgrades from M0.
- **Q2. A per-file size limit.** The plan's limits are per organization (`storage`); a single
  upload needs a bound too. Options: (a) an instance setting `max_file_bytes`, default 100 MiB;
  (b) a new plan limit; (c) none beyond `storage`. Recommendation: (a). It protects the server's
  disk and request handling whatever the plan, and a plan limit can be added in M6 if tiers want
  one.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. How types are sniffed.** Options: (a) a small signature table in our code for the types
  B6 lists; (b) the `infer` crate (a new dependency, through `dependency-researcher`); (c) trust
  the client's type. Recommendation: (a). The list is short, the result only decides headers and
  previews, and every unmatched file is served as an attachment.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. What a folder is.** The data model's `course_files.folder` is a column, not a table.
  Options: (a) a path string; a folder exists while it holds a file; (b) a `course_folders` table
  with empty folders; (c) no folders. Recommendation: (a), the data model's shape. Authors who want
  an empty folder upload into it.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Whether learners browse course files.** The plan says files are in per-course folders and
  that locked content never reaches the client. A files area open to learners would hand out a
  locked item's file. Options: (a) learners reach files only through `file` items, and the files
  area is for holders of `course.view_all`; (b) learners browse every file of a published course;
  (c) a per-folder "visible to learners" switch. Recommendation: (a), which keeps `04`'s guarantee
  without a second rule.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q6. How URLs are signed and who serves the bytes.** Options: (a) the server seals a token with
  the existing encryption key ring and serves `/files/<token>` itself, streaming from the store;
  (b) the store's own presigned URLs (S3, MinIO), which the local-directory backend lacks and
  whose headers depend on the store; (c) an HMAC key of its own, a new secret. Recommendation:
  (a). It works for every backend, keeps `nosniff`, disposition and CSP under our control, adds no
  secret and no dependency, and a token cannot be read or edited by its holder. The cost is that
  bytes pass through the server; presigned downloads can come later for large media.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q7. Signed URL lifetime.** "Short-lived" has no number. Options: (a) 10 minutes; (b) 1 minute;
  (c) 1 hour. Recommendation: (a). Long enough for a PDF viewer's range requests while the reader
  starts reading, short enough that a pasted link stops working within the hour; the web app asks
  for a new URL when one expires.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q8. How the inline PDF preview is drawn.** Options: (a) the browser's own PDF viewer in an
  `<iframe>` of an inline URL, with a download fallback; (b) PDF.js (`pdfjs-dist`, Apache-2.0)
  rendering in the page, a large dependency; (c) server-rendered page images. Recommendation: (a),
  with no dependency. Evidence points at Chromium refusing to show a PDF in a sandboxed frame,
  which is why B17 leaves `sandbox` off PDFs only; the issue verifies the headers in Chromium,
  Firefox and WebKit before relying on them, and the iOS web view with spec 17. If (a) fails on
  a target, the issue stops and reports, and (b) goes through `dependency-researcher`.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
  Coordinator decision (2026-11-19), from building I3: (a) stays, and the item view always
  shows the download beside the preview. Playwright's Chromium and Firefox report no PDF viewer
  (`navigator.pdfViewerEnabled` is false) and show the fallback with a working download.
  Playwright's Linux WebKit reports a viewer but draws an empty frame, with or without B17's
  `Content-Security-Policy` and `Cross-Origin-Resource-Policy`; evidence points at the test
  engine, not the product. T15's WebKit case therefore requires the frame and a working download
  only. Real Safari and the iOS web view still need checking, which joins the owner's spec 17
  iPhone run; if the iPhone shows a blank preview, the fallback is PDF.js (b), through
  `dependency-researcher` first.
- **Q9. The scope of "stored once".** Options: (a) once per organization (`UNIQUE (org_id,
  sha256)`); (b) once per instance, which saves space across organizations but lets one
  organization learn, from upload timing or limit accounting, that another holds a given file,
  and ties organizations' deletion and export together. Recommendation: (a). The plan's
  per-organization restore and deletion need files that belong to one organization.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q10. The MinIO image (ADR 0019).** ADR 0019 asks the M1 media spec to revisit Chainguard's
  MinIO image against another S3-compatible server. This spec needs multipart uploads, which every
  candidate supports, and not presigned URLs (Q6). Options: (a) keep ADR 0019's Chainguard image
  for the Compose stacks; (b) switch to Garage or SeaweedFS. Recommendation: (a); spec 08 revisits
  it if transcoding needs something MinIO lacks. ADR 0019 stays as it is.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

Routes of I2 and I3 that read or write stored bytes take `Require<P>` before the `Storage`
extractor (`stepclimb_server::storage`), so the permission check runs first: an anonymous caller
gets 401 and a caller without the permission 403, whether or not storage is configured, and only
an allowed caller learns that storage is missing (503 `storage.not_configured`). axum runs
extractors in argument order.

1. **I1. Storage and the files table.** `object_store` (pinned after `dependency-researcher`, in its
   own commit), the storage configuration and startup probe, Compose settings, the `files` and
   `course_files` migration and repositories, `Limit::Storage`, the not-configured answer, and a
   MinIO service with its own slot port in the gate's per-run Compose project (`bin/ci_gate.py`),
   with the bucket created at start, so storage tests run inside `just ci`. Covers B1, B1a, B2, B22;
   T1, T1a (for the routes that exist by then; I2 and I3 extend T1a to theirs). Depends on:
   `02-courses-and-enrollments.md` I1.
2. **I2. Upload and the files area.** The upload route with streaming, hashing, sniffing,
   dedupe and the limit check; listing, rename, folder rename, delete; `CourseFileTarget`;
   `files.cleanup` for temporary objects and unreferenced files. Covers B3 to B12, B21; T2 to
   T11, T18 and T19 for these routes. Depends on: I1, `01-job-queue-and-outbox.md` I3.
3. **I3. Signed delivery and file items.** Token sealing, `/files/<token>` with its headers,
   ranges and conditional requests, the two URL routes, `file` items, the lock suite entries,
   the web app's file item view and PDF preview. Covers B13 to B18; T12 to T15, T18 and T19 for
   these routes. Depends on: I2, `03-modules-and-items.md` I2, `04-unlock-and-completion-rules.md`
   I3.
4. **I4. Cover and copy.** The cover column and `PATCH` field with `cover_url`, and copying course
   files in `courses::copy`. Covers B19, B20; T16, T17. Depends on: I3,
   `02-courses-and-enrollments.md` I3.
