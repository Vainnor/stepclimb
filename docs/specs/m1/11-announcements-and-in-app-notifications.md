# Announcements and in-app notifications

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Courses and content (Announcement: "Course- or account-wide.
  Sent by email and push according to each user's preferences."), Email and notifications ("Push
  (Web Push, APNs) and in-app notifications follow the same preferences"; one-click unsubscribe
  for anything that isn't required), Data model (`notifications(id, user_id, kind, payload,
  read_at)`), Web app (Web Push for announcements, M4); `docs/plan/ui.md` -> Component inventory
  (Learning: Announcement), Key layouts (the bell in the top bar; Notifications tab under 768 px)
- **Depends on:** `01-job-queue-and-outbox.md` (outbox, jobs), `02-courses-and-enrollments.md`
  (courses, enrollments, accounts), `07-pages-and-the-editor.md` (the document model and renderer
  for announcement bodies)
- **Review:** standard

## Summary

Instructors post announcements to a course, and account admins to every course in an account's
subtree. Posting writes an event; a job turns it into one in-app notification per recipient, and
spec 12 sends the email copy to those whose preferences allow it. Each user has a notification
list with an unread count behind the bell, and one place to choose, per kind of notification and
per channel, what they receive. Push notifications are M4; their channel is already in the
preferences so nothing changes shape then.

## Behavior

### Announcements

- **B1.** An announcement has a scope (`course` and its id, or `account` and its id), a `title`
  (1 to 200 characters), a `body` (a document in spec 07's model, restricted to paragraphs,
  headings, lists, links, bold, italic and callouts; no images or embeds in M1), the author, and
  `published_at`. It is created published (Q1); edits change the text and set `edited_at`, and do
  not notify again.
- **B2.** Posting to a course needs `perm::AnnouncementPost` at the course; to an account, at the
  account. An archived course refuses posts (409 `course.archived`).
- **B3.** Recipients (Q2), computed when the fan-out job runs: for a course announcement, every
  user with an active enrollment in the course (learners and staff); for an account announcement,
  every user with an active enrollment in a published course of the account's subtree, each once.
  Suspended users are left out. The author gets no notification of their own post.
- **B4.** Reading: a course's announcements, newest first, to any holder of `perm::CourseView` at
  the course (with `02` B10's draft rule); an account announcement to its recipients and to holders
  of `perm::AnnouncementPost` at the account. `GET /api/v1/me/announcements` lists the
  announcements addressed to the caller across courses and accounts, newest first. The body is
  served as HTML from spec 07's renderer.
- **B5.** Deleting an announcement (`perm::AnnouncementPost` at its scope) deletes it and the
  notifications it made. Every post, edit and delete writes an audit entry
  (`announcement.create`, `announcement.update`, `announcement.delete`; `09` B22), because an
  account-wide post reaches many people.

### From event to notification

- **B6.** Creating an announcement emits `announcement.published` (`01` B19) with `data: {
  announcement_id, scope_type, scope_id, author_id }`. Its subscriber `notifications.fan_out`
  (`01` B21) computes the recipients and inserts one `notifications` row per recipient whose
  in-app preference for the kind is on (B10), in batches of 500 per transaction, idempotent per
  `(announcement, user)` so a rerun inserts nothing twice. Spec 12 subscribes its own email job to
  the same event.
- **B7.** A notification has `kind` (`announcement` in this spec; spec 13 adds `due_reminder` and
  `escalation`, M2 `grade_released`), a `payload` holding ids and the title shown (no body), a
  link (a web app path), `created_at` and `read_at`.
- **B8.** `stepclimb_core::notifications::notify(tx, scope, user, kind, payload)` is the one
  function that creates a notification; it applies the in-app preference. Later specs call it.

### The notification list

- **B9.** `GET /api/v1/me/notifications` lists the caller's notifications, newest first,
  paginated, with `?unread=true`; `GET /api/v1/me/notifications/unread-count` answers the count
  (capped at 100, shown as "99+" past 99). `POST /api/v1/me/notifications/{id}/read` and `POST
  /api/v1/me/notifications/read-all` mark them read. A notification id of another user answers
  404. Notifications older than 180 days are deleted by a periodic job (Q3).

### Preferences

- **B10.** Each user has preferences per kind and channel: channels `in_app`, `email` and `push`
  (push takes effect in M4). Each kind declares, in code, its default per channel and whether it
  is required (cannot be turned off; for example a security notice in spec 12). Unset means the
  default. M1 kinds and defaults: `announcement` (in-app on, email on, push on); spec 13's kinds
  declare theirs.
- **B11.** `GET /api/v1/me/notification-preferences` lists every kind with its channels, the
  effective value and whether it can be changed; `PATCH` takes `{ kind: { channel: bool } }`
  partially (422 `notification.required` for turning off a required channel, 422
  `notification.kind_unknown`). An organization cannot change a user's preferences in M1 (Q4).
- **B12.** The bell in the top bar shows the unread count (polled every 60 s and on focus) and
  opens the list in a popover on wide screens and as the Notifications tab under 768 px (`11`'s
  layout). Each entry links to its target; opening it marks it read. The preferences live on the
  account page (`/account`).
- **B13.** `GET /api/v1/me/dashboard` (`10` B4) gains `announcements`: the three most recent
  announcements addressed to the caller in the last 14 days (title, scope, `published_at`, link),
  and the dashboard page shows them below Next up.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`08`'s CSRF refusals.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/courses/{course_id}/announcements` | `?cursor&limit` | 200 `Page<Announcement>` | 404 `course.not_found` | `perm::CourseView` (course) |
| `POST /api/v1/courses/{course_id}/announcements` | `{ title, body }` | 201 `Announcement` | 404, 409 `course.archived`, 422 | `perm::AnnouncementPost` (course) |
| `GET /api/v1/accounts/{account_id}/announcements` | `?cursor&limit` | 200 `Page<Announcement>` | 404 `account.not_found` | `perm::AnnouncementPost` (account) |
| `POST /api/v1/accounts/{account_id}/announcements` | `{ title, body }` | 201 `Announcement` | 404, 422 | `perm::AnnouncementPost` (account) |
| `GET /api/v1/announcements/{announcement_id}` | | 200 `Announcement` | 404 `announcement.not_found` | B4: a recipient, `CourseView` at its course, or `AnnouncementPost` at its scope |
| `PATCH /api/v1/announcements/{announcement_id}` | `{ title?, body? }` | 200 `Announcement` | 404, 422 | `perm::AnnouncementPost` (its scope) |
| `DELETE /api/v1/announcements/{announcement_id}` | | 204 | 404 | `perm::AnnouncementPost` (its scope) |
| `GET /api/v1/me/announcements` | `?cursor&limit` | 200 `Page<Announcement>` | | authenticated |
| `GET /api/v1/me/notifications` | `?unread&cursor&limit` | 200 `Page<Notification>` | | authenticated |
| `GET /api/v1/me/notifications/unread-count` | | 200 `{ count }` | | authenticated |
| `POST /api/v1/me/notifications/{notification_id}/read` | | 204 | 404 `notification.not_found` | authenticated (own notifications only) |
| `POST /api/v1/me/notifications/read-all` | | 204 | | authenticated |
| `GET /api/v1/me/notification-preferences` | | 200 `NotificationPreferences` | | authenticated |
| `PATCH /api/v1/me/notification-preferences` | `{ <kind>: { <channel>: bool } }` | 200 `NotificationPreferences` | 422 `notification.required`, `notification.kind_unknown` | authenticated |

Schemas: `Announcement { id, scope: { type, id }, title, html, author: { id, display_name },
published_at, edited_at }` (and `body` JSON for holders of `AnnouncementPost`, for editing);
`Notification { id, kind, title, link, created_at, read_at }`; `NotificationPreferences { kinds:
[{ kind, channels: [{ channel, enabled, required }] }] }`. `AnnouncementTarget` resolves an
announcement to its scope; the read route's handler applies B4 after `Require<perm::CourseView>`
or the recipient check, as documented in the permission suite. Event schema:
`AnnouncementPublishedEvent`.

## Data

Migration `migrations/<version>_announcements.sql` (I1) and
`migrations/<version>_notifications.sql` (I2). Versions are placeholders, fixed when each issue is
implemented so each sorts after every migration on `next` and every open branch.

```sql
CREATE TABLE announcements (                    -- not in the data model: B1
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL REFERENCES organizations (id),
    scope_type    text        NOT NULL CHECK (scope_type IN ('course', 'account')),
    scope_id      uuid        NOT NULL,
    title         text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    body          jsonb       NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    author_id     uuid        NOT NULL,
    published_at  timestamptz NOT NULL,
    edited_at     timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, author_id) REFERENCES users (org_id, id)
);
CREATE INDEX announcements_by_scope ON announcements (org_id, scope_type, scope_id, published_at DESC);

CREATE TABLE notifications (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL,
    user_id     uuid        NOT NULL,
    kind        text        NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,49}$'),
    payload     jsonb       NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    source_id   uuid,                       -- the announcement (or later object) it came from: B5, B6
    read_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    UNIQUE (user_id, kind, source_id)
);
CREATE INDEX notifications_by_user ON notifications (org_id, user_id, created_at DESC);
CREATE INDEX notifications_unread ON notifications (org_id, user_id) WHERE read_at IS NULL;

CREATE TABLE notification_preferences (       -- not in the data model: B10
    org_id      uuid        NOT NULL,
    user_id     uuid        NOT NULL,
    kind        text        NOT NULL,
    channel     text        NOT NULL CHECK (channel IN ('in_app', 'email', 'push')),
    enabled     boolean     NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, channel),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id)
);
```

`scope_id` has no foreign key (course or account by `scope_type`), as `role_assignments` (`09`);
the repository checks it, and `courses`/`accounts` deletion paths check for announcements (`02`
B21's delete refuses an account with announcements: 409 `account.has_announcements`, added here).
Only rows that differ from the default are stored in `notification_preferences`.

## Permissions

| Permission | Key | Grantable at | Built-in roles (M1) |
| --- | --- | --- | --- |
| `perm::AnnouncementPost` | `announcement.post` | organization, account, course | Organization admin, Account admin, Instructor |

Entitlements: none.

## Test cases

- **T1** (B1, B2). An Instructor posts to their course (201); a Student gets 403; a body with an
  image node answers 422 `page.body_invalid`; an archived course answers 409.
- **T2** (B3, B6). A course announcement notifies every active enrollment's user once (a user
  with two roles gets one), not concluded ones, not suspended users, not the author; an account
  announcement notifies users enrolled in published courses of the subtree, once each.
- **T3** (B6). Running the fan-out twice inserts nothing new; 1,200 recipients are written in three
  transactions.
- **T4** (B10). A user who turned in-app announcements off gets no notification row; the default
  user gets one.
- **T5** (B9). The list is newest first and only the caller's; another user's notification id
  answers 404 to `read`; the unread count drops after `read` and is 0 after `read-all`; rows older
  than 180 days are deleted (manual clock).
- **T6** (B11). Turning a required channel off answers 422 `notification.required`; an unknown
  kind 422; a partial patch keeps other values.
- **T7** (B4). A Student reads their course's announcements; a user enrolled elsewhere gets 404
  for that course; `GET /me/announcements` lists both course and account announcements addressed
  to the caller.
- **T8** (B5). Deleting an announcement deletes its notifications and writes one audit entry.
- **T9** (B1). Editing does not create notifications and sets `edited_at`.
- **T10** (B12). Playwright: an Instructor posts; a Student sees the bell count, opens the list,
  follows the link and the count drops. Screenshots of the bell, list and preferences in the
  default and vZDC themes, light and dark, at 1280 and 375 px; axe finds no violation.
- **T11** (every route). Four cases for the permission routes (anonymous 401; every permission but
  `announcement.post` at the scope 403; every permission at a sibling account 404; only the
  permission 2xx) and `AuthOnly` cases for the `/me` routes.
- **T12** (every route). Cross-organization: org A's admin with org B's course, account or
  announcement id gets 404; org A's members never receive org B's notifications; org B's rows are
  unchanged.

## Out of scope

- The email copy of an announcement and email preferences' unsubscribe link: spec 12.
- Web Push and APNs: M4 and M5.
- Scheduled announcements, attachments and replies: not in the plan.
- Notification kinds of later specs (reminders, escalations, grade releases): 13 and M2.

## Open questions

Each question below was delegated by the owner to the coordinator; each recommendation is taken
and stands until the owner reviews it.

- **Q1. Drafts and scheduling.** Options: (a) posting publishes at once, edits never re-notify;
  (b) drafts and a scheduled publish time; (c) (a) plus drafts. Recommendation: (a), the plan's
  behavior with nothing added.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. Who receives an announcement.** The plan says "course- or account-wide". Options: (a)
  every active enrollment of the course, or of published courses in the account's subtree; (b)
  learners only; (c) for accounts, every user with any role in the subtree as well. Recommendation:
  (a): staff in the course see what learners were told, and an account announcement reaches the
  people taking training there.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. How long notifications are kept.** Options: (a) 180 days; (b) forever; (c) 30 days.
  Recommendation: (a): the list stays useful, and the table stays bounded.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Organization defaults for preferences.** Options: (a) code defaults per kind, users change
  their own, organizations cannot; (b) organizations set defaults; (c) organizations can force
  channels. Recommendation: (a) in M1; organization defaults can come with M4's notification
  settings if asked.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Announcements.** Migration, `announcement.post` in the catalog and roles, the course and
   account routes, the read route's rules, the body subset on spec 07's model and renderer, the
   event, audit entries, account deletion check. Covers B1 to B5; T1, T7 to T9, T11 and T12 for
   these routes. Depends on: `07-pages-and-the-editor.md` I2, `01-job-queue-and-outbox.md` I4,
   `02-courses-and-enrollments.md` I2.
2. **I2. Notifications and preferences.** Migration, `notify`, the fan-out subscriber, the list,
   count, read routes, preferences with kind declarations, the retention job. Covers B6 to B11;
   T2 to T6, T11 and T12 for these routes. Depends on: I1.
3. **I3. Bell, list and preferences UI.** The top bar bell, the popover and the Notifications tab,
   the preferences section of the account page, the course's announcements view, and the
   dashboard's announcements. Covers B12, B13; T10. Depends on: I2,
   `10-learner-dashboard-and-next-up.md` I2.
