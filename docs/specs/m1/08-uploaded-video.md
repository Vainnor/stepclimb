# Uploaded video

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Video, Uploaded: "resumable uploads
  (tus) up to a plan or instance limit. A transcode job (ffmpeg) produces HLS at 360p, 720p and
  1080p in H.264/AAC, plus a poster image, a thumbnail strip and an MP4 for offline download.
  Stored through `object_store` and served through short-lived signed URLs, so files can't be
  hot-linked. Captions are WebVTT, uploaded per language. Chapters are set in the editor."),
  Architecture and stack (`stepclimb-media`), Data model (`videos`, `video_captions`,
  `video_chapters`), Hosting and operations (Scaling: `--role transcoder` with concurrency and CPU
  limits; the image bundles ffmpeg), Plans and billing (limits: video storage, transcode
  minutes), Decided (Video); `docs/plan/ui.md` -> Component inventory (VideoUploader)
- **Depends on:** `01-job-queue-and-outbox.md` (the `transcode` queue, roles),
  `06-files.md` (storage, signed delivery, sniffing, `storage.not_configured`),
  `03-modules-and-items.md` (`video` items), `02-courses-and-enrollments.md` (permissions, copy),
  `../m0/10-entitlements-framework.md` (limits)
- **Review:** maintainer line-by-line (untrusted media handed to ffmpeg, signed delivery)

## Summary

Authors upload a video file to a course through tus, so a large upload survives a dropped
connection. A transcode job runs ffmpeg on it, on any worker or on a dedicated `transcoder`
process, and produces HLS renditions at up to 1080p, an MP4 for offline download, a poster and a
thumbnail strip. Learners watch through signed, expiring URLs. Authors add WebVTT captions per
language and chapters. Plan limits bound upload size, stored video and transcoding minutes. The
player and watch tracking are spec 09; this spec makes a video ready to play.

## Behavior

### Videos and courses

- **B1.** A video is one `videos` row of the organization with `source` (`upload` here; spec 09
  adds the linked sources), `status`, `title`, `duration_s`, `renditions`, `poster_key` and
  `storage_bytes`. A video is attached to courses through `course_videos`; it is created attached
  to the course it was uploaded to, and a course copy attaches the same video to the copy (`02`
  B13), so stored renditions are never duplicated.
- **B2.** Status moves `uploading` -> `processing` -> `ready`, or to `failed` with an error code
  (`video.probe_failed`, `video.not_a_video`, `video.too_long`, `video.transcode_failed`,
  `entitlement.limit_reached`). A failed video can be deleted or uploaded again as a new video;
  `POST /api/v1/videos/{video_id}/retry` re-enqueues a `video.transcode_failed` one when its
  original is still stored.
- **B3.** `video` items become creatable (`03` B5) for a video attached to the item's course in
  any status; learners see a non-ready video as "processing" with nothing to play.

### Upload (tus)

- **B4.** `POST /api/v1/courses/{course_id}/video-uploads` starts a tus 1.0.0 upload (core plus
  the creation and termination extensions; Q1) with `Upload-Length` and `Upload-Metadata`
  (`filename`, `title`), needing `perm::CourseContentEdit`. It creates the `videos` row
  (`uploading`) and a `video_uploads` row, and answers 201 with `Location:
  /api/v1/video-uploads/{upload_id}`. Before creating anything it refuses: storage not configured
  (503 `storage.not_configured`, `06` B1a); a length above `max_video_bytes` (instance setting,
  default 5 GiB, 1 MiB to 50 GiB; 413 `file.too_large`); a length that does not fit
  `Limit::VideoStorage` (403 `entitlement.limit_reached`); an archived course (409).
- **B5.** `HEAD /api/v1/video-uploads/{upload_id}` answers `Upload-Offset` and `Upload-Length`;
  `PATCH` with `Content-Type: application/offset+octet-stream` and `Upload-Offset` equal to the
  stored offset appends the body (409 `video.upload_offset_mismatch` otherwise) and answers 204
  with the new offset; `DELETE` ends the upload and deletes the video and its parts. Each request
  needs `perm::CourseContentEdit` at the video's course and runs the same CSRF checks as every
  unsafe route (`m0/08`); the tus `Tus-Resumable` header is required and answered.
- **B6.** Each `PATCH` body is stored as its own object `orgs/<org>/videos/<id>/upload/<offset>`
  and its length added to the row in one statement under the row's lock, so any server of the
  instance can take the next chunk and no chunk is buffered whole in memory. A chunk is at most 64
  MiB (413 otherwise). Incomplete uploads untouched for 24 h are deleted with their parts by the
  periodic `videos.cleanup` job.
- **B7.** When the offset reaches the length, the server sniffs the first chunk (`06` B6's table,
  MP4, WebM, QuickTime and Matroska signatures added): anything else fails the video with
  `video.not_a_video`. Otherwise the video becomes `processing` and a `video.transcode` job is
  enqueued on the `transcode` queue.

### Transcoding

- **B8.** The `transcode` queue runs on processes started with `--role all` or `--role
  transcoder` (`01` B13); `--role worker` leaves it out. A transcoder runs at most
  `transcode_concurrency` jobs (default 1, 1 to 16) and passes `transcode_threads` (default 0,
  meaning ffmpeg's choice) to ffmpeg. The ffmpeg and ffprobe binaries are found on `PATH` or at
  `ffmpeg_path` and `ffprobe_path`; a process that serves the queue checks at startup that both run
  and report a version, and refuses to start otherwise, naming the setting.
- **B9.** The job concatenates the parts into a temporary file in `transcode_dir` (default the
  OS temporary directory), runs ffprobe on it, and fails the video when ffprobe finds no video
  stream (`video.not_a_video`) or cannot read it (`video.probe_failed`). Duration above
  `max_video_seconds` (default 4 h) fails it with `video.too_long`. It then calls
  `Entitlements::check_room(Limit::TranscodeMinutes, ceil(duration / 60))` (Q4) and fails it with
  `entitlement.limit_reached` when that does not fit.
- **B10.** ffmpeg runs as a child process with: input read only from the local file with
  `-protocol_whitelist file` and no network protocol, a fixed argument list built by the job (no
  author string reaches the command line; the title is never passed), a wall-clock limit of 4 x
  the duration plus 10 minutes, and output only into the job's temporary directory. Its stderr is
  kept (last 2 KiB) for the error log, never for the learner.
- **B11.** Outputs (Q2): HLS with MPEG-TS segments of 6 s (Q2) at 360p (800 kbit/s), 720p (2.8
  Mbit/s) and 1080p (5 Mbit/s), each H.264 High with AAC 128 kbit/s stereo, including only
  renditions no taller than the source (a 480p source gets 360p only; a source below 360p gets one
  rendition at its own height); a master playlist; an MP4 at 720p
  (or the source height if lower) with `+faststart`; a poster JPEG from 10% into the video; a
  thumbnail strip (one 160 px wide frame every 10 s, packed into JPEG sprites of 10 x 10) with a
  WebVTT index of the sprite regions.
- **B12.** The job uploads outputs to `orgs/<org>/videos/<id>/v<n>/...` (`n` the transcode run, so
  a retry never overwrites what a player may be reading), writes `renditions` (heights, bitrates,
  playlist and MP4 keys, sprite index key), `duration_s`, `poster_key` and `storage_bytes`, sets
  `ready`, and deletes the upload parts and the temporary directory (Q3). It re-checks
  `Limit::VideoStorage` with the real output size and fails the video when it does not fit,
  deleting the outputs. Run twice (`01` B12), the second run finds `ready` and does nothing.
- **B13.** Transcode minutes are recorded per organization and calendar month (UTC) in
  `transcode_usage` when a transcode succeeds, by the job's clock.

### Captions and chapters

- **B14.** `PUT /api/v1/videos/{video_id}/captions/{lang}` with a WebVTT body (at most 1 MiB)
  stores captions for a BCP 47 language tag (`en`, `en-US`, `pt-BR`; validated, 422 otherwise),
  replacing any for that tag; `DELETE` removes them. The server parses the file: a missing
  `WEBVTT` header, a malformed cue timing or a cue past the duration is refused (422
  `video.captions_invalid` with the line number). Cue text is stored as given and is served only as
  `text/vtt`, never inserted as HTML. Captions are stored as course files of the video's first
  course in the folder `/_captions` (`06`), so they count toward `storage`.
- **B15.** `PUT /api/v1/videos/{video_id}/chapters` with a list of `{ at_s, title }` (at most 100,
  `at_s` strictly increasing from 0 and below the duration, titles 1 to 200 characters) replaces
  the chapters. The video editor in the course authoring screen sets them.
- **B16.** Both need `perm::CourseContentEdit` at a course the video is attached to.

### Delivery

- **B17.** `POST /api/v1/items/{item_id}/video-session` returns, for a ready uploaded video, the
  playback URLs: `hls_url`, `mp4_url` (for download, `Content-Disposition: attachment`),
  `poster_url`, `thumbnails_url`, the captions (`lang`, `url`) and chapters, with `expires_at`.
  It needs `perm::CourseView` at the course and runs `03` B14's visibility and `04` B13's lock
  check (403 `item.locked`); it joins `04`'s lock suite. `POST
  /api/v1/videos/{video_id}/preview-session` does the same for staff (`perm::CourseViewAll`), for
  the authoring screen.
- **B18.** The HLS URLs use a prefix token (Q5): `/media/<token>/master.m3u8`, where the sealed
  token (`06` B16's mechanism, context `[b"media_url"]`) names the organization, the video, the
  output run `n` and an expiry 2 hours from issue. `GET /media/<token>/<path>` serves any object
  under `orgs/<org>/videos/<id>/v<n>/` whose path matches the renditions' file names (no `..`,
  no other prefix), with `06` B17's headers (`nosniff`, CORP `same-origin`, `no-referrer`, ranges)
  and the HLS types (`application/vnd.apple.mpegurl`, `video/mp2t` or `video/mp4`). Playlists hold
  only relative segment names, so every segment URL carries the same token and nothing is
  rewritten. An expired token answers 404; the player asks for a new session and resumes.
- **B18a.** Cross-origin clients. The web app plays media from its own origin, so it needs no CORS.
  hls.js fetches playlists and segments with `fetch`/XHR, which a client on another origin (the
  Tauri app's bundle origin) can read only with CORS. `/media/` and `/files/` responses therefore
  add `Access-Control-Allow-Origin: <origin>` and `Vary: Origin` when the request's `Origin` is in
  the server's allowed origins (`m0/08`'s allowlist, `STEPCLIMB_ALLOWED_ORIGINS`), and answer an
  `OPTIONS` preflight for `GET` with the `Range` header allowed and `Content-Range` and
  `Content-Length` exposed; no credentials are allowed (the token is in the path). Evidence points
  at `Cross-Origin-Resource-Policy: same-origin` blocking a cross-origin `<video>` or `<img>` load
  in WebKit, which would block the Tauri app even with CORS; spec 17 I1 checks it on the device
  (Q5), and until then CORP stays `same-origin`.
- **B19.** Every route that serves bytes answers 503 `storage.not_configured` when storage is not
  configured (`06` B1a).

### Deleting and limits

- **B20.** `DELETE /api/v1/videos/{video_id}` deletes the video. It is allowed only when no item in
  any course references it (409 `video.in_use`), and deletes the row, its captions' course files,
  its chapters and, after the commit, its objects. Detaching from one course (`DELETE
  /api/v1/courses/{course_id}/videos/{video_id}`) is allowed when no item of that course references
  it; the last detach deletes the video.
- **B21.** `Limit::VideoStorage` (`video_storage`, bytes) counts `storage_bytes` of the
  organization's videos plus incomplete upload lengths; `Limit::TranscodeMinutes`
  (`transcode_minutes`, per calendar month) counts `transcode_usage` for the current month. `m0/10`
  B2 names both; this spec adds them to `feature::Limit`.

### VideoUploader

- **B22.** The VideoUploader component (`packages/ui`) uploads through tus with resumable chunks
  of 16 MiB, shows progress, pause, resume and cancel, resumes after a page reload from the
  upload URL kept in local storage, and then shows the processing state until the video is ready
  (polling `GET /api/v1/videos/{video_id}` every 10 s).

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals; writes in an archived course answer 409 `course.archived`; byte routes
answer 503 `storage.not_configured` (B19).

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /api/v1/courses/{course_id}/video-uploads` | tus creation headers | 201, `Location` | 404 `course.not_found`, 413 `file.too_large`, 403 `entitlement.limit_reached`, 503 | `perm::CourseContentEdit` (course) |
| `HEAD /api/v1/video-uploads/{upload_id}` | | 200 with `Upload-Offset`, `Upload-Length` | 404 `video_upload.not_found` | `perm::CourseContentEdit` (the upload's course) |
| `PATCH /api/v1/video-uploads/{upload_id}` | `Upload-Offset`, chunk body | 204 with `Upload-Offset` | 404, 409 `video.upload_offset_mismatch`, 413, 503 | `perm::CourseContentEdit` |
| `DELETE /api/v1/video-uploads/{upload_id}` | | 204 | 404 | `perm::CourseContentEdit` |
| `GET /api/v1/courses/{course_id}/videos` | `?cursor&limit` | 200 `Page<Video>` | 404 | `perm::CourseViewAll` (course) |
| `GET /api/v1/videos/{video_id}` | | 200 `Video` | 404 `video.not_found` | `perm::CourseViewAll` (a course it is attached to) |
| `PATCH /api/v1/videos/{video_id}` | `{ title? }` | 200 `Video` | 404, 422 | `perm::CourseContentEdit` |
| `POST /api/v1/videos/{video_id}/retry` | | 202 `Video` | 404, 409 `video.status_invalid` | `perm::CourseContentEdit` |
| `DELETE /api/v1/videos/{video_id}` | | 204 | 404, 409 `video.in_use` | `perm::CourseContentEdit` (every course it is attached to) |
| `DELETE /api/v1/courses/{course_id}/videos/{video_id}` | | 204 | 404, 409 `video.in_use` | `perm::CourseContentEdit` (course) |
| `PUT /api/v1/videos/{video_id}/captions/{lang}` | `text/vtt` body | 200 `Caption` | 404, 413, 422 `video.captions_invalid` | `perm::CourseContentEdit` |
| `DELETE /api/v1/videos/{video_id}/captions/{lang}` | | 204 | 404 | `perm::CourseContentEdit` |
| `PUT /api/v1/videos/{video_id}/chapters` | `{ chapters: [{ at_s, title }] }` | 200 `Chapter[]` | 404, 422 | `perm::CourseContentEdit` |
| `POST /api/v1/items/{item_id}/video-session` | | 200 `VideoSession` | 404 `item.not_found`, 403 `item.locked`, 409 `video.not_ready`, 503 | `perm::CourseView` (the item's course) |
| `POST /api/v1/videos/{video_id}/preview-session` | | 200 `VideoSession` | 404, 409 `video.not_ready`, 503 | `perm::CourseViewAll` |
| `GET /media/{token}/{path}` | `Range?` | 200, 206 with bytes | 404 `file.not_found`, 416 | none: the token is the credential |

`VideoTarget` resolves `video_id` to the courses it is attached to and passes when the permission
holds at any of them (for `DELETE /videos/{id}`, at every one); code `video.not_found`. Schemas:
`Video { id, source, title, status, error_code?, duration_s?, renditions?: { heights }, captions:
[{ lang }], chapters, created_at, updated_at }`; `VideoSession { hls_url, mp4_url, poster_url,
thumbnails_url, captions: [{ lang, url }], chapters, duration_s, expires_at }`.

Configuration added (ADR 0022): `max_video_bytes` (5 GiB), `max_video_seconds` (14400),
`transcode_concurrency` (1), `transcode_threads` (0), `transcode_dir` (OS temporary directory),
`ffmpeg_path` and `ffprobe_path` (found on `PATH`), each with its `STEPCLIMB_` variable.

## Data

Migration `migrations/<version>_videos.sql` (I1), `migrations/<version>_video_captions.sql` (I4).
Versions are placeholders, fixed when each issue is implemented so each sorts after every
migration on `next` and every open branch.

```sql
CREATE TABLE videos (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    source         text        NOT NULL CHECK (source IN ('upload', 'youtube', 'vimeo', 'oembed', 'url')),
    title          text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),  -- not in the data model
    file_id        uuid,                       -- unused for uploads: renditions are not `files` rows (B12)
    url            text,                       -- spec 09
    status         text        NOT NULL CHECK (status IN ('uploading', 'processing', 'ready', 'failed')),
    error_code     text,
    duration_s     numeric(10, 3) CHECK (duration_s >= 0),
    renditions     jsonb       NOT NULL DEFAULT '{}'::jsonb,
    poster_key     text,
    output_run     integer     NOT NULL DEFAULT 0,                  -- B12's v<n>
    storage_bytes  bigint      NOT NULL DEFAULT 0 CHECK (storage_bytes >= 0),
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    CHECK ((status = 'failed') = (error_code IS NOT NULL))
);

CREATE TABLE course_videos (                    -- not in the data model: B1
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    video_id    uuid        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (course_id, video_id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE
);

CREATE TABLE video_uploads (                    -- not in the data model: B4 to B6
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL,
    video_id      uuid        NOT NULL UNIQUE,
    course_id     uuid        NOT NULL,
    length        bigint      NOT NULL CHECK (length > 0),
    received      bigint      NOT NULL DEFAULT 0 CHECK (received >= 0 AND received <= length),
    terminating   text        CHECK (terminating IN ('parts', 'video')),   -- D13
    created_by    uuid        NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, created_by) REFERENCES users (org_id, id)
);

CREATE TABLE video_chapters (
    org_id      uuid        NOT NULL,
    video_id    uuid        NOT NULL,
    at_s        numeric(10, 3) NOT NULL CHECK (at_s >= 0),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (video_id, at_s),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE
);

CREATE TABLE transcode_usage (                  -- not in the data model: B13
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    month       date        NOT NULL CHECK (extract(day FROM month) = 1),
    minutes     integer     NOT NULL CHECK (minutes >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, month)
);

-- <version>_video_captions.sql
CREATE TABLE video_captions (
    org_id          uuid        NOT NULL,
    video_id        uuid        NOT NULL,
    lang            text        NOT NULL CHECK (char_length(lang) BETWEEN 2 AND 35),
    course_file_id  uuid        NOT NULL,       -- the data model's file_id, through course_files (B14)
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (video_id, lang),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_file_id) REFERENCES course_files (org_id, id)
);
```

`video` items reference `videos.id` through `module_items.ref_id`; the repository checks that the
video is attached to the item's course. Captions' course files are added to `06` B11's in-use
check. The media crate holds the ffmpeg runner and the HLS layout (`stepclimb-media`); the SQL
stays in `stepclimb-core`.

## Permissions

No new catalog entry: uploads, captions and chapters take `perm::CourseContentEdit`, staff reads
`perm::CourseViewAll`, learner playback `perm::CourseView` with the lock check. Entitlements:
`Limit::VideoStorage` and `Limit::TranscodeMinutes` (B21), named by `m0/10` B2; `unlimited` leaves
both `null`. No feature flag: video is core (plan, Decided).

## Test cases

Transcoding tests use short synthetic clips generated by ffmpeg in the test (`testsrc` and `sine`
sources, 3 s), never a downloaded file. Storage tests use the gate's MinIO and a local directory.

- **T1** (B4). A creation with a length over `max_video_bytes` answers 413; one over a test plan's
  `video_storage` answers 403 `entitlement.limit_reached`; without storage configured, 503.
- **T2** (B5, B6). An upload in three `PATCH` chunks, the second sent to a second server instance
  sharing the database and storage, reaches the full offset; a `PATCH` with a stale offset answers
  409; `HEAD` after a dropped connection reports the stored offset and the upload resumes from it.
- **T3** (B6). An incomplete upload untouched for 24 h (manual clock) is deleted with its parts by
  `videos.cleanup`; one touched 23 h ago is kept.
- **T4** (B7, B9). A PNG uploaded as a video fails with `video.not_a_video`; a truncated MP4 with
  `video.probe_failed`; a clip longer than `max_video_seconds` (set to 2 s in the test) with
  `video.too_long`.
- **T5** (B10). The ffmpeg argument list built for a video whose title and file name contain
  `-i http://example.com` and `$(touch x)` contains neither; an HLS input that references
  `http://` is refused by the protocol whitelist (a crafted playlist file as input fails without a
  network attempt, checked with no network namespace).
- **T6** (B11). A 1280x720 source produces 360p and 720p renditions only; a 1920x1080 source all
  three; a 320x240 source a 240p rendition; each playlist plays in ffprobe; the MP4 has its `moov`
  atom first; the sprite index covers the duration.
- **T7** (B12). A second run of a completed job changes nothing; a retry after a failure writes to
  `v2` and the `v1` keys a session holds stay readable until the job deletes them after the commit.
- **T8** (B9, B13, B21). With a test plan's `transcode_minutes` at 1 and 1 used this month, a new
  video fails with `entitlement.limit_reached`; the next month (manual clock) it succeeds and
  `transcode_usage` records its minutes.
- **T9** (B14). Captions without the header, with a malformed timing, or a cue past the end
  answer 422 with the line; a valid file is served as `text/vtt` with `nosniff`; a cue holding
  `<script>` is served unchanged as text.
- **T10** (B15). Chapters out of order or at the duration answer 422.
- **T11** (B17, B18). A Student's session for an unlocked item plays the master playlist and every
  segment through the same token path; a path with `..` or another video's id under the token
  answers 404; a token 1 s past expiry answers 404; a locked item answers 403 `item.locked`.
- **T12** (B20). Deleting a video an item uses answers 409; detaching from the last course deletes
  the row and, after the commit, every object under its prefix.
- **T13** (B8). A `--role worker` process never claims a `transcode` job; a `--role transcoder`
  process claims only `transcode` jobs and serves no API route; a missing ffmpeg stops a transcoder
  at startup naming `ffmpeg_path`.
- **T14** (B22). Vitest and Playwright: the uploader pauses and resumes, survives a reload, and
  shows processing then ready. Screenshots in the default and vZDC themes, light and dark, at 1280
  and 375 px; axe finds no violation.
- **T15** (every `/api/v1` route). Four cases: anonymous 401; every permission but the route's at
  the course 403; every permission at a sibling account 404; only the route's permission 2xx.
- **T16** (every route). Cross-organization: org A's admin with org B's course, video, upload or
  item ids gets 404; org B's videos, uploads, objects and usage rows are unchanged; a media token
  issued in org B cannot be edited to reach org A's objects.
- **T17** (I6). `bin/check-image.sh` runs `ffmpeg -version` and `ffprobe -version` in the amd64 and
  arm64 images and transcodes a 1 s `testsrc` clip to a 360p HLS rendition inside each; the image's
  `LICENSES/` holds ffmpeg's license and source offer.
- **T18** (B18a). A `GET` of a media playlist with `Origin` in the allowlist gets
  `Access-Control-Allow-Origin` echoing it and `Vary: Origin`; with another `Origin`, no CORS
  header; a preflight for `GET` with `Range` succeeds; no response allows credentials.

## Out of scope

- The player, linked video and watch tracking: spec 09.
- Offline download of the MP4 in the browser: spec 15; in the Tauri app: M5.
- Adaptive bitrate beyond the three renditions, other codecs (HEVC, AV1), DRM: not in the plan.
- Automatic captions: not in the plan.
- Videos inside pages (embeds of uploaded videos): not in the plan; pages embed YouTube and
  Vimeo only (spec 07).
- Presigned direct-to-S3 uploads and CDN delivery: not built (`06` Q6).

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. How much of tus to build, and with what.** Options: (a) our own tus 1.0.0 core with the
  creation and termination extensions, chunks stored as objects; (b) a tus server crate, after
  `dependency-researcher`; (c) the `tusd` sidecar, a second process. Recommendation: (a). The core
  protocol is a few headers, the chunk-as-object layout works on every `object_store` backend and
  across replicas, and no crate we could pin covers that layout.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Rendition details the plan leaves open.** Bitrates, segment length and container. Options:
  (a) B11's ladder (800 kbit/s, 2.8 Mbit/s, 5 Mbit/s), 6 s segments, MPEG-TS segments, no
  upscaling, a 720p MP4; (b) fMP4 segments; (c) the MP4 at 1080p. Recommendation: (a). TS segments
  are an HLS segment format (RFC 8216 section 3.2), which hls.js transmuxes to fMP4 ("MPEG-2 TS ...
  segments are transmuxed into fMP4", hls.js README, https://github.com/video-dev/hls.js,
  accessed 2026-11-16) and Safari plays natively; that every Safari and the iOS web view play
  them is unverified here and checked by 09 T9 and spec 17. A 720p MP4 keeps offline downloads
  small. B11 is written
  for (a).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Keeping the original upload.** Options: (a) delete the original parts once the transcode
  succeeds; the MP4 is the downloadable copy; (b) keep the original, which a re-transcode could
  use, at the cost of storage; (c) keep it 30 days. Recommendation: (a): the plan stores the HLS
  and MP4 outputs, and the original can be twice their size.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. How transcode minutes are counted.** The plan names the limit only. Options: (a) per
  calendar month in UTC, `ceil(duration / 60)` per successful transcode, checked before running;
  (b) actual ffmpeg wall time; (c) per rolling 30 days. Recommendation: (a): predictable for an
  admin, known before the job runs, and independent of the host's speed.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Signed delivery for HLS.** A playlist names many segments. Options: (a) a token in the
  path that covers one video's output prefix for 2 hours, so relative segment names work
  unchanged; (b) the server rewrites each playlist with a signed URL per segment; (c) a cookie
  scoped to `/media/`. Recommendation: (a). One token per session and no playlist rewriting; 2
  hours covers a long lecture, and the player renews the session on 404. Evidence points at it
  working in Safari's native player, since relative segment URLs keep the token; for the Tauri app
  it depends on B18a's CORS and on whether CORP `same-origin` blocks a cross-origin media load,
  which spec 17 I1 checks. If it blocks, an issue against this spec sets CORP to `cross-origin` on
  `/media/` and `/files/`, leaving access control to the token.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q6. Where the videos table's courses come from.** The data model's `videos` has no course.
  Options: (a) a `course_videos` join, so copies share renditions; (b) `videos.course_id`, with
  copies duplicating rows that point at shared objects; (c) no link, any video of the
  organization usable anywhere. Recommendation: (a), the shape `course_files` uses.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q7. How ffmpeg gets into the image.** The plan says "the image bundles ffmpeg" and the images
  are distroless and non-root; today's final stage holds only the binary. Options: (a) copy a
  pinned static ffmpeg and ffprobe build (version and checksum pinned, per architecture) into the
  distroless image; (b) build ffmpeg from source in a Dockerfile stage with only the codecs B11
  needs, a long cold build for two architectures (ADR 0021's long jobs); (c) a second image for
  `--role transcoder` on a Debian base with Debian's ffmpeg, leaving the main image without it.
  Recommendation: (a), the plan's single image with the smallest change, subject to
  `dependency-researcher` finding a static build with a published source and license; I6 records
  the choice in an ADR for the owner.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review and I6's ADR.

### Points I1 settled

Coordinator decision (2026-11-17), pending owner review: the spec left each of these open while
building I1.

- **D1. The creation's body.** 201 carries `Location` and the body `VideoUploadCreated
  { upload_id, video: Video }`, so the uploader learns the video's id without a second request.
- **D2. The title.** `Upload-Metadata`'s `title`, or else its `filename`; with neither, 422
  `request.invalid` at `/title` (`name.empty`). The file name is not stored.
- **D3. The tus version.** A request without `Tus-Resumable: 1.0.0` answers 412
  `tus.version_unsupported` with `Tus-Version: 1.0.0`; every response of the tus routes carries
  `Tus-Resumable`. `OPTIONS` discovery is not served; B4 does not name it.
- **D4. What `video_storage` counts.** Every `video_uploads` row's whole length, from its
  creation until the row is deleted, complete or not, plus `storage_bytes`: a started upload always
  has room to finish, and a complete one waiting for its transcode still counts the parts it
  stores. I2 deletes the upload row when it deletes the parts (Q3).
- **D5. How a chunk is stored.** The body streams to a temporary object
  (`orgs/<org>/uploads/<uuid>`, the one `06` B5 uses), then, under the upload row's lock, the
  offset is checked again, the object is copied to `upload/<offset>` and the length added. So
  two chunks at one offset never both write that key, and a chunk is stored whole or not at all:
  a dropped connection stores nothing and `HEAD` reports the offset before it.
- **D6. Chunk bounds.** A chunk that runs past `Upload-Length` answers 413 `file.too_large`; an
  empty chunk at the stored offset answers 204 and stores nothing (D15 sets the smallest chunk
  that stores something).
- **D7. A video that is not a video.** The chunk that completes it commits `failed` with
  `video.not_a_video` and its upload marked terminating (D13), then deletes its parts and its
  upload row; the failed video stays for the author to see. `videos.cleanup` deletes the parts
  of a video failed as `video.not_a_video` only, so a delete that failed is retried; the original
  of a video that failed for any other reason (`video.transcode_failed`, ...) is kept for B2's
  retry. Coordinator decision (overnight, 2026-11-17).
- **D8. Ending an upload.** `DELETE` on an upload whose video is `processing` or `ready` answers
  409 `video.status_invalid` (delete the video instead, I3). A chunk or an ending in an archived
  course answers 409 `course.archived`.
- **D9. Renaming.** `PATCH /videos/{id}` answers 409 `course.archived` when every course through
  which the caller holds `course.content_edit` is archived. In I1 `Video` has no `renditions`
  and no `captions`: I2 writes the first and I4 adds the table of the second.
- **D10. The video signatures.** QuickTime (`ftyp` brand `qt  `, or a first `moov`, `mdat`,
  `wide` or `free` atom) and Matroska are added to the video check only; a course file of either
  still sniffs as `application/octet-stream`.
- **D12. What B7 sniffs.** The upload's first 8 KiB (`06` B6's window), read across parts
  in offset order, not only the first chunk: a client whose first chunk is a few bytes long still
  uploads a video.
- **D11. `VideoTarget` with several courses.** None passing answers 403 when one course is
  visible without the permission, otherwise 404 `video.not_found`.

Coordinator decision (overnight, 2026-11-17), after the security review of I1:

- **D13. Terminating uploads.** `video_uploads.terminating` (`parts` or `video`) marks an upload
  whose parts are being deleted. A termination, a not-a-video discard and `videos.cleanup` set it
  under the upload row's lock and commit; then the parts are deleted holding no lock, and a short
  transaction deletes the upload row (`parts`) or the video (`video`). A chunk takes the same
  row lock and answers 404 `video_upload.not_found` for a marked upload before it copies
  anything, so no part is written after the parts are listed; `HEAD` and `DELETE` answer 404 too.
  A delete that fails leaves the mark, and `videos.cleanup` finishes every marked upload. Parts
  are deleted 16 at a time; object_store's `delete_stream` (bulk deletes on S3) needs the
  `futures` crate as a direct dependency, which I1 does not add.
  Known limit, coordinator decision (overnight, 2026-11-17): a chunk whose `CopyObject` to its
  part is still running in the store when its client disconnects can land that part after a
  termination has listed the parts, leaving an object under `upload/` that no row names and that
  no quota counts. A cleanup sweep for such orphaned `upload/` objects is tracked in a follow-up
  issue.
- **D14. The course lock of a chunk.** A chunk reads its course row `FOR SHARE`: an archive or
  move, which updates the row, waits for chunks in flight, and chunks of different uploads of one
  course do not wait for each other. Creation, termination and rename keep `FOR NO KEY UPDATE`.
- **D15. The smallest chunk.** A chunk that does not complete the upload must be at least 1 MiB
  (`MIN_CHUNK_BYTES`), or the server answers 400 `video.chunk_too_small`: the chunk is
  well-formed but breaks a protocol bound, like a bad `Upload-Offset`. So a 50 GiB upload has at
  most 51,200 parts to store and delete. The completing chunk may be any size; an empty chunk
  stays a no-op (D6).
- **D16. Who may touch an upload.** Any holder of `course.content_edit` at the upload's course may
  send its chunks, read its offset or end it, not only the user who started it: course content is
  shared by its editors, as course files are. `created_by` records who started it and gates
  nothing.
- **Notes for I2.** ffprobe and ffmpeg get the demuxer pinned with `-f` (from the sniffed
  container: `mov,mp4,m4a,3gp,3g2,mj2` or `matroska,webm`) and `-protocol_whitelist file`, so a
  file that only claims a container (D10's QuickTime atoms) cannot pick another demuxer or reach a
  network protocol. The transcode deletes the upload row with the parts (D4). Coordinator decision
  (overnight, 2026-11-17): `Terminating::Video` deletes only the `upload/` prefix, which is all a
  video has in I1; once transcodes write renditions under `v<n>/`, ending a failed video must
  delete the whole `orgs/<org>/videos/<id>/` prefix.
### Points I5 settled

Coordinator decision (2026-11-17), pending owner review: the spec left each of these open while
building I5.

- **D17. A tus client of our own.** tus-js-client 4.3.1 (MIT, released 2025-01-16; the 5.0 line
  is still pre-release) ships 86,773 bytes minified (22,246 gzip) in its browser build and pulls 7
  direct and 7 further packages, among them `proper-lockfile` and `graceful-fs` for Node, and
  `combine-errors`, which declares no license in any version and was republished as 3.0.4 on
  2026-10-31 after ten years without a release (npm registry, accessed 2026-11-17). The server
  speaks only creation, `HEAD`, `PATCH` and termination, so `packages/ui/src/video/tus.ts` (270
  lines) implements those; no dependency is added.
- **D18. Where the uploader's requests go.** VideoUploader lives in `packages/ui` and takes the
  caller's `send` (one request, with its progress) and `getVideo`, so the package needs no API
  client. The web app sends through `XMLHttpRequest`, as file uploads do, since `fetch` reports no
  upload progress.
- **D19. Resuming after a reload.** A browser does not hand a page a file again after a reload, so
  local storage keeps each unfinished upload's address with the file's name, size, type and last
  change, per course, and the uploader lists them; choosing the same file again resumes it from
  the offset `HEAD` answers, and an upload the server no longer has starts again. An entry not
  advanced for 24 h is dropped, as B6 deletes the upload. Discard ends one with `DELETE`.
- **D20. The title.** The uploader sends `filename` and a `title` made of the file name without
  its extension, cut to 200 characters (D2). Renaming is `PATCH /videos/{id}`, not this issue.
- **D21. Failures.** A request that got no answer, a 408, a 429 or a 5xx stops the upload
  resumable: it goes on when the browser fires `online`, or on Resume. A 409
  `video.upload_offset_mismatch` takes the offset from `HEAD` and goes on, up to three times in a
  row. Any other refusal fails the upload with its own message; a code without one is shown
  with the code.
- **D22. One upload at a time** per uploader; a file chosen while one runs or waits is refused
  with a message naming the running one.
- **D23. Cancel** sends `DELETE` at once, with no confirmation, and stays available while paused
  or stopped. When the server answers 409 `video.status_invalid` (the last chunk completed the
  upload first), the uploader says so and shows the video processing.
- **D24. Announcements.** A polite status region says each phase and the progress in 10% steps;
  a refusal goes to the upload's alert region.
- **D25. The video panel.** The course authoring screen (`16` B19, `16` I14) does not exist yet.
  Until it does, the panel is `/c/:course/videos`, reached by its address like the files area
  (`16` B20a): VideoUploader for holders of `course.content_edit` in a course that is not
  archived, then the course's videos with their status and why one failed, read again every 10 s
  while one is processing. I14 places the same panel in its screen. The page editor's video panel
  (`07` B12) takes YouTube and Vimeo addresses only, and uploaded videos in pages are out of scope
  here, so the two do not meet.
- **D26. T14's Playwright journey** answers the video routes in the browser, as the files area's
  test does: the gate's server has no transcoder until I2, so a video never becomes ready there.

- **Note for user deletion (M4).** `video_uploads.created_by` references `users` without
  `ON DELETE`, so deleting a user who started an upload still in progress is refused by the
  foreign key; per-user deletion has to end or reassign such uploads first.


## Issues

1. **I1. Video rows, tus upload and limits.** Migration for `videos`, `course_videos`,
   `video_uploads`, `video_chapters`, `transcode_usage`; tus creation, `HEAD`, `PATCH` and
   `DELETE`; `Limit::VideoStorage`; `VideoTarget`; `videos.cleanup` for stale uploads; video list,
   read, rename. Covers B1 to B7, B21 (storage); T1 to T3, T15 and T16 for these routes. Depends
   on: `06-files.md` I2, `01-job-queue-and-outbox.md` I3.
2. **I2. Transcoder.** `stepclimb-media`'s ffmpeg runner with the B10 limits, the
   `video.transcode` job, outputs and their upload, `--role transcoder` and its settings, the
   startup ffmpeg check, `Limit::TranscodeMinutes` with `transcode_usage`, retry. Developed and
   tested with the dev shell's ffmpeg; the release image gets ffmpeg in I6. Covers B8 to B13, B21
   (minutes); T4 to T8, T13. Depends on: I1, `01-job-queue-and-outbox.md` I2.
3. **I3. Playback sessions and media delivery.** The two session routes, prefix tokens,
   `/media/<token>/<path>` with B18a's CORS, `video` items with the lock suite entry, detach and
   delete. Covers B3, B17 to B20, B18a; T11, T12, T15 and T16 for these routes. Depends on: I2, I6,
   `06-files.md` I3, `04-unlock-and-completion-rules.md` I3.
4. **I4. Captions and chapters.** `video_captions` migration, WebVTT validation, caption and
   chapter routes, captions in sessions, copy attaching videos. Covers B14 to B16, B1 (copy); T9,
   T10. Depends on: I3, `02-courses-and-enrollments.md` I3.
5. **I5. VideoUploader.** The component with tus-js-client (pinned after `dependency-researcher`,
   own commit) or a small client of our own if research finds it too large, and the authoring
   screen's video panel. Covers B22; T14. Depends on: I1.
6. **I6. ffmpeg and ffprobe in the image.** The final image stage of `deploy/Dockerfile` is
   `gcr.io/distroless/cc-debian12` with only the `stepclimb` binary; nothing bundles ffmpeg today.
   This issue adds `ffmpeg` and `ffprobe` (Q7), pinned by version and checksum, at
   `/usr/local/bin`, for amd64 and arm64; extends `bin/check-image.sh` to run `ffmpeg -version`
   and `ffprobe -version` in both architectures and to transcode a 1 s `testsrc` clip to one HLS
   rendition inside the image; and writes an ADR (Proposed, for the owner) on the build used and
   its license. A build with libx264 is GPL, a separate program the AGPL server runs as a child
   process: the ADR records the license, the source offer the image must carry (`LICENSES/` in
   the image and the release notes) and why it is compatible, after `dependency-researcher`.
   Covers T17. Depends on: I2. I3 waits for it, so no release ships uploads without a
   transcoder.
