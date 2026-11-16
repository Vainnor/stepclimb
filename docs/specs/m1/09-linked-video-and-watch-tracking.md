# Linked video and watch tracking

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Video: "Two sources, one player and
  the same tracking ... Linked: YouTube and Vimeo through their player APIs, plus other providers
  through oEmbed and direct MP4 or HLS URLs. Watched segments are recorded for both, so skipping
  ahead doesn't count. Watch data goes to the trainer view"), Item (requirement "watch >= X%"),
  Data model (`videos`, `video_progress(user_id, item_id, watched_ranges int4multirange,
  duration_s, percent)`), Integration platform (Data out: the `video-progress` report, M4);
  `docs/plan/ui.md` -> Component inventory (VideoPlayer: HLS and embeds, watch tracking); ADR 0025
- **Depends on:** `08-uploaded-video.md` (videos, sessions), `05-progress-and-events.md`
  (`complete_item`), `04-unlock-and-completion-rules.md` (the `watch` requirement, locks),
  `03-modules-and-items.md` (`video` items)
- **Review:** standard

## Summary

Authors can also make a video item from a link: a YouTube or Vimeo video, a video from an oEmbed
provider the organization trusts, or a direct MP4 or HLS URL. One VideoPlayer plays every source
and reports which seconds the learner actually watched. The server merges those reports into
watched ranges, caps what one report can add by the time that has passed, computes the watched
percentage itself, and completes the item through spec 05 when the `watch` requirement's
threshold is reached, so skipping to the end does not count. Instructors see each learner's
watched percentage.

## Behavior

### Linked videos

- **B1.** `POST /api/v1/courses/{course_id}/linked-videos` with `{ url, title? }` creates a
  `videos` row attached to the course (`08` B1) with a linked `source`, `status = 'ready'` and no
  stored bytes. It needs `perm::CourseContentEdit`. The server decides the source from the URL:
  - `youtube`: `youtube.com/watch?v=<id>`, `youtu.be/<id>`, `youtube.com/embed/<id>`,
    `youtube.com/shorts/<id>` (and `www.`, `m.` hosts); the 11-character id is stored, not the URL;
  - `vimeo`: `vimeo.com/<digits>` and `player.vimeo.com/video/<digits>`, with an optional unlisted
    hash, stored as id and hash;
  - `url`: an `https` URL whose path ends in `.mp4` or `.m3u8` (or whose type the author names in
    `type`), stored as given;
  - `oembed`: any other `https` URL whose host matches a provider in the organization's
    `video.oembed_providers` setting (Q1); otherwise 422 `video.url_unsupported`.
  Every URL is at most 2,048 characters, absolute, without user name or password (422
  `video.url_invalid`).
- **B2.** For `oembed`, the server fetches the provider's oEmbed endpoint for the URL with the
  outbound client and its URL policy (ADR 0025, `../m0/05-oidc.md` Q4): a 3 s limit, no redirects,
  a 64 KiB reply limit, JSON only. From the reply it keeps only `title`, `thumbnail_url` (an
  `https` URL) and the `src` of the first `<iframe>` in `html`, and only when that `src` is `https`
  with a host listed for the provider; the rest of `html` is discarded and never stored or shown.
  Anything else fails with 422 `video.oembed_failed`. The fetch happens once, at creation.
- **B3.** `duration_s` of a linked video is reported by the author's player when the author first
  previews it (`PUT /api/v1/videos/{video_id}/duration`, accepted once from a holder of
  `perm::CourseContentEdit`, later only by that permission with `{ force: true }`), because the
  server has no API key for the providers (Q2). Until a duration is set, a `watch` requirement on
  the video's items cannot be met and learners see "duration not set" in the player's details.
  For `url` sources the same rule applies. Uploaded videos have ffprobe's duration (`08` B9).
- **B4.** A linked video's playback session (`08` B17's route, extended) answers the source and
  what the player needs: the id (YouTube, Vimeo with hash), the iframe `src` (oEmbed), or the
  URL (direct). The lock check and visibility are the same as for uploaded videos.
- **B5.** Captions and chapters (`08` B14, B15) apply to `url` sources (the player draws them)
  and to uploaded videos; YouTube and Vimeo use their own captions, and chapters are shown in our
  chapter list beside the player and seek through the player API. oEmbed players get neither.

### The player

- **B6.** VideoPlayer (`packages/ui`) plays uploaded and `url` HLS with hls.js where the browser
  lacks native HLS, native HLS in Safari and the iOS web view, MP4 with `<video>`, YouTube through
  the IFrame Player API (`youtube-nocookie.com`), Vimeo through the Vimeo Player API, and oEmbed as
  a sandboxed iframe (`allow-scripts allow-same-origin allow-presentation`); how an oEmbed video is
  tracked is the owner's open question Q3, built by I4. Third-party player scripts are loaded only
  when a video of that source is on the page. The player is keyboard-operable, labels its controls
  from `useStepclimbStrings()`, shows captions, chapters and the poster, and respects reduced
  motion.
- **B7.** While playing, the player keeps the segments the learner actually played (contiguous
  playback at rate 0.25 to 2, from `timeupdate` or the provider API's progress events) and sends
  them every 15 s, on pause, on end and on page hide (with `fetch` `keepalive`) to `POST
  /api/v1/items/{item_id}/watch`. A seek starts a new segment; it never extends the old one.

### Recording watch data

- **B8.** `POST /api/v1/items/{item_id}/watch` with `{ "segments": [[start_s, end_s], ...],
  "position_s", "idempotency_key" }` needs `perm::CourseParticipate` at the item's course, an
  unlocked, visible `video` item (`04` B13, 403 `item.locked`) and a non-archived course (409).
  It answers the learner's `{ percent, watched_s, duration_s, completed }`. A key already used by
  this learner for this item answers the stored answer and records nothing (Q4).
- **B9.** The server checks and clamps each report: segments are rounded to whole seconds
  (`floor(start)`, `ceil(end)`), clamped to `[0, duration_s]` when the duration is known, and
  refused when `end <= start` or more than 20 segments are sent (422). The seconds a report may
  add are capped at `2 x (now - previous report's time) + 10 s` by the request's clock, for the
  first report `2 x (now - the session's issue time) + 10 s`; seconds beyond the cap are dropped,
  earliest segment first kept. This bounds a forged client to twice real time.
- **B10.** The server merges the accepted seconds into `video_progress.watched_ranges` (an
  `int4multirange` of watched seconds) with `range_agg`, and computes `percent = floor(100 x
  watched seconds / ceil(duration_s))`, capped at 100. `percent` is never sent by the client.
- **B11.** The first accepted report records a `view` step (`05` B3), so a `view` requirement on a
  video item is met by starting playback. When `percent` reaches the item's `watch.min_percent`,
  the request calls `progress::complete_item` (`05` B5) in the same transaction, which emits
  `item.completed` and rolls up.
- **B12.** The plan's rule stands: watched segments are recorded for linked video, so skipping
  ahead does not count. YouTube and Vimeo are tracked through their player APIs and direct URLs
  through `<video>` and hls.js (B6, B7), all reporting through B8. A generic oEmbed iframe exposes
  no player API, so how oEmbed videos meet that rule is open for the owner (Q3); until the owner
  answers, no oEmbed source is built (I4 waits on Q3) and this spec decides nothing about it.
  Changing a
  video's duration with `force` recomputes `percent` for every learner of the items that use it in
  a job (`video.recompute_progress`), which completes items whose new percent meets the
  requirement and never un-completes one (`05` Q1).

### Reading watch data

- **B13.** The learner's own `{ percent, watched_ranges }` joins the item read (`05` B13) and the
  playback session, so the player can mark watched parts and resume at `position_s`, which is
  stored per learner and item.
- **B14.** `GET /api/v1/items/{item_id}/watch-progress` lists every learner's percent, watched
  seconds and last report time for a video item, paginated, to holders of `perm::ProgressView`
  (or `progress.view_observed`, filtered as `05` B20). `05` B14's learner detail includes each
  video item's percent.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`08`'s CSRF refusals; writes in an archived course answer 409 `course.archived`.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /api/v1/courses/{course_id}/linked-videos` | `{ url, title?, type? }` | 201 `Video`, `Location` | 404 `course.not_found`, 422 `video.url_invalid`, `video.url_unsupported`, `video.oembed_failed` | `perm::CourseContentEdit` (course) |
| `PUT /api/v1/videos/{video_id}/duration` | `{ duration_s, force? }` | 200 `Video` | 404 `video.not_found`, 409 `video.duration_set`, 422 | `perm::CourseContentEdit` |
| `POST /api/v1/items/{item_id}/watch` | `{ segments, position_s, idempotency_key }` | 200 `WatchState` | 404 `item.not_found`, 403 `item.locked`, 409 `course.archived`, 409 `video.duration_missing` (only for completing), 422 | `perm::CourseParticipate` (the item's course) |
| `GET /api/v1/items/{item_id}/watch-progress` | `?cursor&limit` | 200 `Page<LearnerWatch>` | 404 | `perm::CourseView`, then `progress.view` or `progress.view_observed` (`05` B20) |

Changed: `08`'s `POST /api/v1/items/{item_id}/video-session` serves linked sources (B4) and
carries `watch: { percent, watched_ranges, position_s }` for a learner. Schemas: `WatchState {
percent, watched_s, duration_s, position_s, completed }`; `LearnerWatch { user: { id,
display_name }, percent, watched_s, last_report_at }`. `OrgSettings` gains
`video.oembed_providers`: a list of at most 20 `{ name, endpoint, hosts: [host] }` (endpoint an
`https` URL; hosts lowercase DNS names), written through the settings route with
`perm::OrgManage`.

## Data

Migration `migrations/<version>_video_progress.sql` (I2). The version is a placeholder, fixed when
the issue is implemented so it sorts after every migration on `next` and every open branch.
Linked sources use `videos.url` and `source`, which `08` created; I1's migration
`migrations/<version>_video_external_id.sql` adds `videos.external_id text` (the YouTube id, or
the Vimeo id and unlisted hash) and `videos.embed_src text` (the kept oEmbed iframe `src`).

```sql
CREATE TABLE video_progress (
    org_id          uuid           NOT NULL,
    user_id         uuid           NOT NULL,
    item_id         uuid           NOT NULL,
    watched_ranges  int4multirange NOT NULL DEFAULT '{}',
    duration_s      integer        CHECK (duration_s >= 0),
    percent         smallint       NOT NULL DEFAULT 0 CHECK (percent BETWEEN 0 AND 100),
    position_s      integer        NOT NULL DEFAULT 0 CHECK (position_s >= 0),   -- not in the data model: B13
    last_report_at  timestamptz,                                                   -- B9's cap
    created_at      timestamptz    NOT NULL DEFAULT now(),
    updated_at      timestamptz    NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, item_id) REFERENCES module_items (org_id, id)
);
CREATE INDEX video_progress_by_item ON video_progress (org_id, item_id);

CREATE TABLE watch_reports (                -- not in the data model: B8's idempotency, kept 7 days
    org_id           uuid        NOT NULL,
    user_id          uuid        NOT NULL,
    item_id          uuid        NOT NULL,
    idempotency_key  text        NOT NULL CHECK (char_length(idempotency_key) BETWEEN 8 AND 100),
    response         jsonb       NOT NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id, idempotency_key),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
```

`int4multirange` and `range_agg` exist from PostgreSQL 14; the plan requires 16. `watch_reports`
rows older than 7 days are deleted by `jobs.prune` (`01` B18). An item with `video_progress` rows
counts as having progress for `05` B16.

## Permissions

No new catalog entry: recording takes `perm::CourseParticipate`, linked-video authoring
`perm::CourseContentEdit`, reading others' data `perm::ProgressView` or `progress.view_observed`.
Entitlements: none (linked videos store nothing).

## Test cases

- **T1** (B1). Each YouTube form gives the same stored id; `youtube.com/watch?v=abc` (too short),
  `http://youtu.be/<id>` (not https) and `https://user@vimeo.com/1` are refused; a `.mp4` URL is a
  `url` source; `https://unknown.example/v/1` answers 422 `video.url_unsupported`.
- **T2** (B2). Against a local oEmbed test server: a reply whose iframe `src` host is listed is
  kept as `src` only; a reply with `<script>` in `html` stores no part of it; an iframe host not
  listed, a 4 s reply (paused time), a redirect and a 65 KiB reply each answer 422
  `video.oembed_failed`; a provider endpoint on a private address is refused by the URL policy.
- **T3** (B3). A second duration `PUT` without `force` answers 409; a Student's `PUT` answers 403.
- **T4** (B9, B10). With duration 100 s: a report of `[[0, 10]]` 10 s after the session gives 10%;
  a report of `[[0, 100]]` 5 s later adds at most 20 s; segments `[[5, 2]]` and 21 segments answer
  422; `[[90, 200]]` is clamped to 100; overlapping reports never count a second twice.
- **T5** (B10, B11). Watching 0-40 and 60-100 of a 100 s video at real pace gives 80%; with
  `min_percent: 80` the item completes once, emitting one `item.completed`; with `min_percent: 81`
  it does not; skipping to 95 and watching to the end gives 5%.
- **T6** (B8). Repeating a report with the same key returns the same answer and adds nothing.
- **T7** (B8). A locked video item answers 403 `item.locked`; a Course designer (no
  `course.participate`) answers 403 `permission.denied`.
- **T8** (B12). Forcing a shorter duration completes items whose learners now meet the
  threshold, in the recompute job. (oEmbed tracking tests come with I4, after Q3.)
- **T9** (B6, B7). Vitest: the tracker splits segments on seek and drops time at a playback rate
  above 2; Playwright (Chromium) with a local MP4 and HLS stream: playing 10 s sends segments that
  the server records; the YouTube and Vimeo adapters are tested against stub player objects, not
  the network. Screenshots in the default and vZDC themes, light and dark, at 1280 and 375 px; axe
  finds no violation.
- **T10** (B14). An Instructor lists both learners' percents; an Observer sees only observed
  learners.
- **T11** (every route). Four cases: anonymous 401; every permission but the route's at the course
  403; every permission at a sibling account 404; only the route's permission 2xx.
- **T12** (every route). Cross-organization: org A's admin with org B's course, video or item ids
  gets 404; org B's videos and video progress are unchanged.

## Out of scope

- Uploaded video, its delivery, captions and chapters: spec 08.
- The `video-progress` reporting API and exports: M4.
- Tracking embeds inside pages: not tracked (spec 07 B12).
- Offline watching and syncing watch data recorded offline: spec 15 (web) and M5 (Tauri).
- Active-time tracking: M3 activities.

## Open questions

Questions Q1, Q2 and Q4 were delegated by the owner to the coordinator; each recommendation is
taken and stands until the owner reviews it. Q3 is the owner's and stays open.

- **Q1. Which oEmbed providers are allowed.** The plan says "other providers through oEmbed".
  oEmbed replies carry HTML from the provider. Options: (a) an organization setting listing
  providers (endpoint and iframe hosts), with only the iframe `src` kept; (b) oEmbed discovery for
  any URL; (c) a fixed built-in list. Recommendation: (a). The organization decides whom to trust,
  no third-party HTML is ever rendered, and discovery would make the server fetch any URL an author
  pastes.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Where a linked video's duration comes from.** Options: (a) the author's player reports it
  once, editable with `force`; (b) the server calls the YouTube Data API and Vimeo API, which need
  API keys and an outbound call per video; (c) the learner's player reports it. Recommendation:
  (a): no keys, no third-party call from the server, and a learner never sets the denominator of
  their own percentage.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Tracking for oEmbed videos (open, for the owner).** The plan says watched segments are
  recorded for linked video, and lists oEmbed among the linked sources; a generic oEmbed iframe
  exposes no player API. Every answer but (d) changes what the plan promises for one source, so
  this is the owner's to decide, not the coordinator's. Options: (a) oEmbed videos take `view` and
  `mark_done` requirements only, and a `watch` requirement is refused for them; (b) count the time
  the iframe is visible and playing is assumed, which counts time nobody may have watched; (c) drop
  oEmbed from M1 and support YouTube, Vimeo and direct URLs; (d) support oEmbed only for providers
  whose player has a script API we add one adapter at a time (as for YouTube and Vimeo).
  Recommendation: (a), since YouTube, Vimeo and direct URLs are tracked as the plan says and (b)
  would record watching that did not happen. Until the owner answers, the spec keeps the plan's
  wording (B12), and I4 is blocked.
- **Q4. Report idempotency and the cap.** Options: (a) a client key per report, stored 7 days, and
  a cap of twice the elapsed time plus 10 s; (b) no key (merging ranges is already idempotent) and
  the same cap; (c) no cap. Recommendation: (a). Ranges merge idempotently, but the cap does not: a
  retried report would get a second allowance. The key makes a retry return the first answer.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Linked videos.** `videos.external_id` migration, the linked-video route with URL parsing,
   for YouTube, Vimeo and direct URLs (`oembed` URLs answer 422 `video.url_unsupported` until I4),
   durations, sessions for linked sources. Covers B1 (except `oembed`), B3 to B5; T1, T3, T11 and
   T12 for these routes. Depends on: `08-uploaded-video.md` I3.
2. **I2. Watch recording.** `video_progress` and `watch_reports`, the watch route with clamping,
   the cap, merging, percent, view and completion through `complete_item`, the recompute job, the
   staff read, the lock suite entry. Covers B8 to B14; T4 to T8, T10 to T12. Depends on: I1,
   `05-progress-and-events.md` I3.
3. **I3. VideoPlayer.** The component with hls.js (pinned after `dependency-researcher`, own
   commit), the YouTube and Vimeo adapters, the segment tracker and reporting, captions and
   chapters, resume. Covers B6, B7; T9. Depends on: I2, `08-uploaded-video.md` I4.
4. **I4. oEmbed sources.** The `video.oembed_providers` setting, the oEmbed fetch through the
   outbound client, the sandboxed oEmbed player, and the tracking or requirement rule the owner
   chooses in Q3. Covers B1 (`oembed`), B2, B12 (oEmbed); T2 and the Q3 tests. Depends on: I3;
   **blocked on Q3 (owner)**.
