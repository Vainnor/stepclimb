# Installable web app and offline reading

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Web app and native apps (Web app: "Installable as a web app
  with a manifest and service worker. Learners can download a course for offline reading, cached
  in the browser's storage."; Web Push, M4), Architecture and stack (W: installable, offline
  reading); `docs/plan/ui.md` -> Component inventory (Learning: DownloadButton (offline)),
  Pitfalls (web-view tells); `.claude/rules/cross-platform.md` (browsers, iOS)
- **Depends on:** `03-modules-and-items.md`, `04-unlock-and-completion-rules.md` (what a learner
  may have), `05-progress-and-events.md` (progress), `06-files.md` (signed URLs),
  `07-pages-and-the-editor.md` (rendered pages), `../m0/11-app-shell.md` (B32: this is M1),
  `../m0/12-tokens-and-theme-generator.md` (brand colors)
- **Review:** standard

## Summary

The web app becomes installable: a manifest names it after the organization and a service worker
keeps the app's own files so it opens without a network. A learner can download a course for
offline reading: the pages and files they can open now are stored in the browser, and while
offline they read them and mark pages done, which is sent when the connection returns. Nothing
locked is ever downloaded, and signing out removes every download.

## Behavior

### Installing

- **B1.** The server serves `GET /manifest.webmanifest` (`application/manifest+json`, no
  authentication, from the request's organization, `m0/02` B11): `name` and `short_name` from the
  organization's name (`short_name` cut to 12 characters), `start_url: "/"`, `scope: "/"`,
  `display: "standalone"`, `theme_color` and `background_color` from the organization's light
  theme (`brand` and `bg-canvas`, `m0/12`), and Stepclimb's default icons (192 and 512 px, plus a
  maskable 512 px), which this spec adds to the web build from the default brand (ADR 0014).
  Organization icons arrive with logo upload (`m0/12` Q6), not here.
- **B2.** `web/index.html` links the manifest and sets `theme-color`, `viewport-fit=cover` (already
  set by `m0/11`) and the `apple-touch-icon`. The app adds no install prompt of its own in M1 (Q3);
  browsers offer installation.

### The service worker

- **B3.** The service worker (`/sw.js`, scope `/`, served with `Cache-Control: no-cache`) is our own
  code (Q1). At install it caches the app shell: `index.html` and every hashed asset of the build,
  listed in a precache manifest the web build writes; at activate it deletes caches of earlier
  builds.
- **B4.** Fetch handling: a navigation request goes to the network and, when that fails, answers
  the cached `index.html`; a request for a precached asset answers from the cache; a request for
  an offline download's synthetic URL (`/offline/...`, B8) answers from that download's cache or
  404; every other request, including all of `/api/`, `/auth/`, `/oauth/`, `/files/` and `/media/`,
  goes to the network untouched, so no API response or credential is cached by the worker.
- **B5.** A new build installs a new worker, which waits; the app shows a toast "A new version is
  available" with a "Reload" action that activates it and reloads. The server's own `/theme.css`
  is fetched from the network as today.
- **B6.** The worker is registered only in production builds and only on secure origins
  (`https`, or `localhost` in development).

### Downloading a course

- **B7.** `GET /api/v1/courses/{course_id}/offline` answers the course's offline package for the
  caller: the course, its modules and items as `03` B13 shows them to the caller (published only,
  with `04`'s locks), and for every unlocked item of kind `page` the rendered HTML (`07` B18) with
  each image as `{ course_file_id, url }`, for every unlocked `file` item its metadata and a signed
  URL (`06` B16), and for `external_link` and `header` items their outline fields. Video, quiz and
  activity items are listed with `offline: false` (Q2). The package carries `version`, a hash of
  everything in it but the signed URLs, and `size_bytes`. It needs `perm::CourseView` with `03`
  B14's rules. Locked items appear as in `04` B12, with no content.
- **B8.** The DownloadButton (`16`) shows the package's size, asks for persistent storage
  (`navigator.storage.persist()`), fetches every image and file through its signed URL within the
  URL's lifetime, and stores them in a Cache Storage cache per course under synthetic URLs
  `/offline/<course_id>/files/<course_file_id>`; page HTML is stored with each image's `src`
  rewritten to its synthetic URL. Package metadata and HTML go to IndexedDB. A download larger
  than 500 MiB, or larger than the browser's reported free quota, is refused before any byte is
  fetched with a message naming the size. A failed download removes what it stored.
- **B9.** While offline (or when a request to the API fails with a network error), the course
  routes read the downloaded package: the outline, page items in PageReader, file items through
  their synthetic URLs (PDF preview and images, `06` B18), with a banner "Offline: showing your
  downloaded copy from <date>". Items not in the package say they need a connection.
- **B10.** Offline progress: `view` and `mark_done` on downloaded items are written to an
  IndexedDB queue with the time they happened and sent in order to `POST
  /api/v1/items/{item_id}/progress` (`05` B2) when the connection returns. The server applies its
  usual checks at that moment; a step it refuses (the item locked since, the course archived, the
  enrollment concluded) is dropped and the learner is told how many steps could not be recorded.
  Steps unlock nothing offline: content that would unlock is not in the package (B7).
- **B11.** On every app start with a network, each downloaded course is checked: the package
  route answering 404 or 403 deletes the download (the learner lost access); a different `version`
  shows "Update available" on the course and the DownloadButton refreshes it on request. A
  download older than 30 days without a successful check is deleted (Q4).
- **B12.** Signing out (`m0/11` B12) deletes every offline cache, every package and the progress
  queue before the sign-in screen shows; when a different user signs in on the same browser, any
  data of another user found is deleted first. The account page lists downloads with their sizes
  and a "Remove" action.
- **B13.** Offline data is not encrypted by the app: it sits in the browser profile like any page
  the browser caches. The DownloadButton's help text says so, and organizations that cannot accept
  it switch the feature off with the `OrgSettings` key `offline.downloads_enabled` (default
  `true`; `false` makes the package route answer 403 `offline.disabled` and hides the button).

## API

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/offline` | | 200 `OfflinePackage` | 404 `course.not_found`, 403 `offline.disabled`, 503 `storage.not_configured` (only when the package would hold files) | `perm::CourseView` (course) |
| `GET /manifest.webmanifest` | | 200 manifest | 404 `org.not_found`, 503 `org.none` (`m0/02` B11) | none (public, listed in the walker) |
| `GET /sw.js` | | 200 the worker | | none (a static asset of the web build) |

Schemas: `OfflinePackage { version, size_bytes, course, modules: [Module with items: [Item &
{ offline: boolean, page?: { html, images: [{ course_file_id, url }] }, file?: { name, size, mime,
url } }]], generated_at }`. `OrgSettings` gains `offline.downloads_enabled`.

## Data

No migration. Offline data lives in the learner's browser. The package route reads existing
tables and issues signed URLs.

## Permissions

No new catalog entry: the package route takes `perm::CourseView` and applies `03` and `04`'s
rules. Entitlements: none (Q5).

## Test cases

- **T1** (B1). The manifest's name, colors and icons match the organization and its stored
  branding; Chromium's manifest parser in Playwright reports it installable.
- **T2** (B3, B4). Playwright (Chromium): after one visit, with the context offline, a reload
  shows the app shell; `/api/v1/me` is never served from the worker's caches (a request log shows
  every API request reached the network or failed).
- **T3** (B5). After a new build is deployed in the test, the update toast appears and Reload
  shows the new build.
- **T4** (B7). A Student's package contains unlocked pages and files only; a locked page's HTML
  and a locked file's URL are absent from the raw response; the `version` does not change between
  two calls with no content change and changes after a page save.
- **T5** (B8). Downloading a course with a page and two images and a PDF stores four cache entries
  and the package; a package over the size limit is refused before fetching; a failure midway
  leaves nothing stored.
- **T6** (B9, B10). Offline, the learner opens the page, sees images and the PDF preview, marks
  the page done; back online, the step reaches the server once and the outline shows it done. A
  queued step for an item locked since is reported as not recorded.
- **T7** (B11). After the learner's enrollment is concluded, the next online start deletes the
  download.
- **T8** (B12). Signing out leaves no Cache Storage entry under `/offline/` and an empty IndexedDB
  store; a second user signing in on the same browser finds nothing of the first.
- **T9** (B13). With `offline.downloads_enabled: false`, the package route answers 403 and the
  button is hidden.
- **T10** (route). Four cases for the package route: anonymous 401; every permission but
  `course.view` 403; a sibling account's grants 404; `course.view` alone 200.
  Cross-organization: org A's admin with org B's course id gets 404.
- **T11** (B2, cross-platform). WebKit in Playwright at 375 px: the installed-mode layout keeps
  fixed bars inside the safe areas (`m0/11`); the iOS home-screen check is by hand, recorded in the
  PR as run or not run.

## Out of scope

- Offline video: the plan gives offline video to the Tauri app (M5); the web package lists videos
  as needing a connection (Q2).
- Web Push and notification permission prompts: M4.
- Offline quizzes and activities: never offline in the plan (answers are checked on the server).
- Background sync APIs: not used (Safari lacks them); the queue replays when the app is open.
- The Tauri app's SQLite downloads: M5.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. How the service worker is written.** Options: (a) our own worker of a few hundred lines
  with a precache manifest from a small Vite plugin in the repo; (b) `vite-plugin-pwa` with
  Workbox, two large dependencies through `dependency-researcher`; (c) Workbox modules only.
  Recommendation: (a): B4's rules are short, and owning them keeps the "never cache the API" rule
  visible in one file.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. What "offline reading" includes.** Options: (a) pages and files, with video, quizzes and
  activities needing a connection; (b) also uploaded videos' MP4s in the browser, which can be
  hundreds of MiB per video; (c) pages only. Recommendation: (a), the plan's "reading", with video
  offline left to the Tauri app as the plan says.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. An install prompt.** Options: (a) none: browsers offer installation, and iOS needs "Add to
  Home Screen" from the share sheet; (b) a custom prompt with instructions per browser.
  Recommendation: (a) in M1; M4 Web Push needs the iOS home-screen install and can add instructions
  then.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. How long downloads live.** Options: (a) until removed, signed out, access lost, or 30 days
  without a successful check; (b) until removed; (c) 7 days. Recommendation: (a): content a
  learner no longer has a right to does not stay on the device for long.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Gating offline downloads.** Options: (a) an organization setting (B13), no entitlement
  feature; (b) an entitlement feature; (c) neither. Recommendation: (a): it is a data-handling
  choice an organization makes, not a paid capability.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

### Points I1 settled

The spec left these open while I1 was built. Each is a coordinator decision (2026-11-16), taken on
the owner's delegation and standing until the owner reviews it.

- **The default icons** are full-bleed squares of the default brand's light `brand` color
  (`#007373`), like the Tauri shell's placeholder icon, until a logo exists (`m0/12` Q6). The web
  build draws them from `web/index.html`'s `theme-color` (`web/build/icons.ts`), and a server
  test holds that value equal to the default brand's, so no PNG is committed. The
  `apple-touch-icon` is the 192 px icon.
- **`theme-color`** in the served `index.html` is the organization's light `brand`, written by
  the server as it writes the theme link (`spa`), so the page and the manifest agree.
  `index.html` itself carries the default brand's. Colors are `#rrggbb`, the 8-bit sRGB a display
  shows.
- **`short_name`** counts Unicode scalar values and drops trailing whitespace after the cut.
- **The worker answers no request under `/docs`** either, besides B4's prefixes, and matches
  every prefix segment by segment (`/apiary` is not `/api`).
- **The navigation fallback** answers the cached `index.html` only when the network fails, never
  for a 5xx answer, which the app shows itself.
- **The shell is fetched without cookies** (`credentials: "omit"`), so the cached `index.html` is
  the anonymous page and carries no user's theme or density. A shell file that answers with a
  redirect, or with anything but a same-origin answer, fails the install.
- **The worker does not claim open pages**: the first visit's page stays uncontrolled, and the
  next navigation is the worker's. A change of controller reloads a page only after its "Reload".
- **"Secure origins" (B6)** are `https`, or `http` on `localhost`, `127.0.0.1` or `[::1]`. This
  leaves out the Tauri shell (`tauri://localhost`, `http://tauri.localhost`), whose offline
  reading is M5's.
- **The update toast** needs an action, which `m0/13` had ruled out for Toast. Toast takes one
  action now, and a toast with one has no timer, so the action is still there when a keyboard
  or screen reader user reaches it (`m0/13`, Toast row).
- **Old tabs after "Reload"**: one tab's "Reload" activates the new worker for every tab and
  deletes the old shell. The build has one script chunk and no dynamic `import()`, so an old tab
  loads no script later; only a font subset its CSS asks for late can miss, and the browser then
  draws the fallback font. A build that adds lazy chunks must keep the previous shell until no
  client runs it.
- **An offline download's cache** is named `stepclimb-offline-<course_id>`, the name I3 stores
  under; activating a new build deletes only `stepclimb-shell-*` caches.
- **End-to-end tests** run with service workers blocked, except `web/e2e/pwa.spec.ts`: a worker
  answers navigations out of `page.route`'s reach.

## Issues

1. **I1. Manifest and app shell worker.** The manifest route, default icons, `index.html` links,
   the service worker with the precache manifest plugin, navigation fallback, the update toast.
   Covers B1 to B6; T1 to T3, T11. Depends on: none beyond M0.
2. **I2. Offline package.** The package route with locks, versions and sizes, the
   `offline.downloads_enabled` setting. Covers B7, B13 (server); T4, T9, T10. Depends on:
   `07-pages-and-the-editor.md` I2, `06-files.md` I3, `04-unlock-and-completion-rules.md` I3.
3. **I3. Downloading and reading offline.** The download into Cache Storage and IndexedDB, the
   offline course routes, the progress queue, checks on start, removal on sign-out, the downloads
   list on the account page. Covers B8 to B12; T5 to T8. Depends on: I1, I2,
   `16-learning-ui-components.md` I8 (DownloadButton), `05-progress-and-events.md` I1.
