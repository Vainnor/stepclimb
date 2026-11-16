# Email

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Email and notifications (platform email from Stepclimb's own
  sender, "Stepclimb LMS"; organization email from the organization's verified domain, through
  DKIM and SPF, or its own SMTP, and "<Org> via Stepclimb" until then, with admins prompted to
  finish setup; editable templates with the organization's branding; each user's notification
  preferences; one-click unsubscribe for anything that isn't required), Data model
  (`email_senders(org_id, kind ['domain','smtp'], config, verified_at)`), Architecture and stack
  (Email: SMTP / SES from the job workers), Security and privacy (secrets encrypted, never
  logged), Decided (Email); ADR 0008, ADR 0025; `docs/plan/ui.md` -> Component inventory (Managed
  hosting: SenderDomainSetup)
- **Depends on:** `01-job-queue-and-outbox.md` (the `email` queue, outbox subscribers),
  `11-announcements-and-in-app-notifications.md` (kinds, preferences, the email channel),
  `07-pages-and-the-editor.md` (the document model, renderer and editor),
  `../m0/04-local-accounts.md` (enrollment links B10, magic links B22 and Q13, the audit actions
  of local credentials), `../m0/08-sessions-and-csrf.md` (encryption, CSRF exemptions),
  `../m0/12-tokens-and-theme-generator.md` (the organization's brand)
- **Review:** maintainer line-by-line (sender credentials, sign-in links by email, unsubscribe
  tokens)

## Summary

Stepclimb sends two kinds of email (ADR 0008). Platform email comes from the instance's own
sender, configured by the operator; in M1 that is security notices about a user's sign-in
methods. Organization email (announcements, sign-in links, and from spec 13 reminders and
escalations) comes from the organization's own SMTP server or verified domain once an admin sets
one up, and until then from the platform sender as "<Org> via Stepclimb". Every organization
email kind has a default template an admin can edit in the page editor, is laid out with the
organization's brand color, respects each user's email preference, and, unless required, carries
a one-click unsubscribe that mail clients show next to the sender.

## Behavior

### The platform sender

- **B1.** The instance's platform sender is configured by `smtp_url` (`smtp://host:port` with
  STARTTLS required, or `smtps://host:port`), `smtp_username`, `smtp_password` (with `_FILE`),
  `mail_from` (an address) and `mail_from_name` (default "Stepclimb LMS"). Email is optional, as
  storage is (`06` B1a, Q1): without `smtp_url` the server starts, logs once at `warn` that email
  is not configured naming `STEPCLIMB_SMTP_URL`, and `check-config` prints `email: not configured
  (STEPCLIMB_SMTP_URL)`. A route whose purpose is to send email (B19, B20) then answers 503
  `email.not_configured`; event-driven email (announcements, notices) is recorded as `skipped` with
  that reason (B13) and the in-app notification still arrives. With `smtp_url` set, `mail_from` is
  required (startup stops naming it), and `smtp_insecure` (default `false`) must be `true` to
  allow plain SMTP without STARTTLS, for a local test relay.
- **B2.** SMTP is spoken by `lettre` (Q2), with a 10 s connect and 30 s send limit. The platform
  connection is to the operator's host, which the operator trusts; no address policy applies to
  it.

### Organization senders

- **B3.** An organization has at most one sender (`email_senders`), `smtp` or `domain` (Q3), each
  `pending` until verified (`verified_at`). While it has none, or it is not verified, or it is
  disabled, organization email goes out through the platform sender with `From: "<Org name> via
  Stepclimb" <mail_from>` (not configurable per organization) and `Reply-To` unset. Admins see a
  prompt to finish setup on the email settings screen and in the admin navigation until a sender is
  verified.
- **B4.** An `smtp` sender stores host, port, TLS mode (`starttls` or `tls`; plain is refused),
  username, password (sealed with the key ring, context `[org_id, b"email_senders", id,
  b"password"]`, never returned), and the `From` address and name. Its host is checked by the
  outbound address policy of ADR 0025 (`../m0/05-oidc.md` Q4) at save and at every connection: no
  loopback, link-local, multicast or reserved address, and no private address on an instance
  with other than one organization (422 `email.sender_host_refused`).
- **B5.** Verifying an `smtp` sender: `POST /api/v1/admin/email/sender/verify` connects,
  authenticates and sends a test message with a 6-digit code to the calling admin's email address;
  `POST /api/v1/admin/email/sender/confirm` with the code (valid 30 minutes, 5 tries) sets
  `verified_at`. Any later change of host, port, username, password or `From` clears
  `verified_at` until verified again.
- **B6.** A `domain` sender (I6) names a domain and a `From` local part. The server generates a
  DKIM key (RSA 2048, Q3), keeps the private key sealed and shows the DNS records to publish: the
  DKIM `TXT` record at `<selector>._domainkey.<domain>`, and the SPF include from the instance
  setting `mail_spf_include` (the platform relay's SPF, which the operator knows; the screen shows
  it only when set). `POST /api/v1/admin/email/sender/verify` looks the records up through the
  system resolver (5 s limit) and sets `verified_at` when the DKIM record matches the key and,
  when `mail_spf_include` is set, the domain's SPF record includes it. Mail is then sent through
  the platform relay with `From: <name> <local@domain>` and a DKIM signature for the domain. The
  verification is repeated daily by a periodic job; a failure clears `verified_at` and notifies
  the organization's admins in-app.
- **B7.** Sender administration needs `perm::EmailManage` (organization). Creating or changing a
  sender, and verifying it, also needs the `custom_email_sender` feature (Q4); reading, disabling
  and deleting do not, so a downgrade leaves a sender the organization can still turn off (`07`
  I5's pattern). Changes write audit entries `email_sender.create`, `.update`, `.verify`,
  `.delete` (never the password or the private key).

### Templates

- **B8.** Each email kind is declared in code with: its category (`platform` or `organization`),
  whether it is required (no preference, no unsubscribe), its notification kind for preferences
  (`11` B10) when not required, its variables (name, description, example), and its default
  subject and body. M1 kinds:

  | Kind | Category | Required | Variables |
  | --- | --- | --- | --- |
  | `announcement` | organization | no (preference `announcement`) | `announcement_title`, `announcement_body`, `scope_name`, `author_name`, `link` |
  | `enrollment_link` | organization | yes | `user_name`, `link`, `expires_at` |
  | `magic_link` | organization | yes | `link`, `expires_at` |
  | `security_notice` | platform | yes | `user_name`, `event`, `at`, `ip` |
  | `sender_verification` | platform | yes | `code` |
  | `signup_verification` | organization | yes | `link`, `expires_at` (B22b) |
  | `signup_existing` | organization | yes | `link` (the sign-in page, B22b) |

  Every template can also use `org_name`, `user_name` and `preferences_link`. Spec 13 adds
  `due_reminder` and `escalation`; M2 adds `grade_released`.
- **B9.** An organization can override the subject and body of each `organization` kind; platform
  kinds are not editable (they are the instance's). A subject is plain text with `{{variable}}`
  placeholders (at most 200 characters after rendering). A body is a document in spec 07's model,
  restricted to paragraphs, headings, lists, links, bold, italic and callouts, plus a `variable`
  inline node naming a variable of the kind. A placeholder or variable the kind does not declare,
  and a required variable missing from a required kind's body (`link` for the sign-in links),
  answers 422 `email.template_invalid` with the pointer. `DELETE` restores the default.
- **B10.** Rendering (`stepclimb_core::email::render`): variables are filled with values escaped
  for HTML (and as plain text in the text part); `announcement_body` is the announcement's
  rendered HTML (`07` B14) and its text form. The HTML part is the body inside a fixed layout
  (a single-column table with inline styles, since mail clients ignore style sheets) using the
  organization's brand color and on-brand text color from its stored branding (`m0/12`), its name,
  and a footer with the preferences link and, for non-required kinds, the unsubscribe link. The
  text part is generated from the same document. Both pass `ammonia` (`07` B14). `POST
  /api/v1/admin/email/templates/{kind}/preview` renders a draft with the kind's example values.
- **B11.** The template editor is spec 07's editor configured with B9's node list and a variable
  picker; previews show both parts.

### Sending

- **B12.** `stepclimb_core::email::queue(tx, scope, Message)` records an `email_messages` row
  (kind, recipient user, address, sender used, status `queued`, the non-secret variables) and
  enqueues `email.send` on the `email` queue (`01`) in the caller's transaction. A secret variable
  (a sign-in link) is stored sealed (`[org_id, b"email_messages", id, b"secret"]`) and only in
  that row; no rendered body is stored, so no secret sits in plaintext in the database, a job
  payload or a log (`m0/08` B4).
- **B13.** Before queueing an email to a user, `queue` checks: the user is `active` and has an
  email address; for non-required kinds, their email preference for the kind is on (`11` B10);
  email is configured (B1). A failed check records the row as `skipped` with the reason
  (`no_address`, `suspended`, `preference_off`, `email_not_configured`) and enqueues nothing.
- **B14.** The `email.send` job renders the message with the template current at send time,
  picks the sender (B3), opens the secret, sends, and marks the row `sent` with the server's
  `Message-ID`; an SMTP 4xx or a connection failure retries (`01` B6, the default backoff); an SMTP
  5xx for the recipient marks it `failed` with the reply code and does not retry. When an
  organization sender fails 3 times in a row with a connection or authentication error, its
  `verified_at` is cleared, the organization's admins get an in-app notice, and mail falls back to
  the platform sender (B3); the failure is logged with the sender id, never the password.
- **B15.** Every email carries `Auto-Submitted: auto-generated`; non-required kinds also carry
  `List-Unsubscribe: <https://<public_url>/email/unsubscribe/<token>>` and
  `List-Unsubscribe-Post: List-Unsubscribe=One-Click` (RFC 8058), and with a domain sender both
  headers are covered by the DKIM signature, which RFC 8058 section 4 requires ("The
  List-Unsubscribe and List-Unsubscribe-Post headers MUST be covered by the signature";
  https://www.rfc-editor.org/rfc/rfc8058.html, accessed 2026-11-16). Mail sent through an SMTP
  sender is signed, if at all, by that server, which this spec does not control.
- **B16.** `email_messages` rows are kept 90 days, then deleted by the periodic job `email.prune`
  (daily, `01` B17) (Q5). Spec 13's
  reminder timeline reads them.

### Unsubscribe and preferences

- **B17.** An unsubscribe token seals `{ org_id, user_id, kind }` with the key ring (context
  `[b"email_unsubscribe"]`); it does not expire, and stops working when the key that sealed it
  leaves the ring. `POST /email/unsubscribe/<token>` (no session, exempt from CSRF's Fetch Metadata
  check because mail clients post it cross-site; `m0/08`'s exemption list gains it) turns the email
  channel of that kind off for that user and answers 200 with no body; an invalid token answers 404
  `email.unsubscribe_invalid`. It is rate limited per client address like the sign-in routes
  (`m0/08` B22).
- **B18.** `GET /email/unsubscribe/<token>` serves the web app's confirmation page, which shows the
  kind and the organization and a "Unsubscribe" button that sends the `POST`; a `GET` never changes
  anything, so link scanners cannot unsubscribe anyone. The page also links to the preferences.

### Callers in M1

- **B19.** Announcements: spec 11's event gets a second subscriber, `email.announcement_fan_out`,
  which queues one `announcement` email per recipient (B13's checks applying the preference),
  idempotent per `(announcement, user)`.
- **B20.** Enrollment links (`m0/04` B10): `POST /api/v1/admin/users/{user_id}/enrollment-links`
  gains a body `{ "send_email": true }`, which queues an `enrollment_link` email to the user's
  address with the link instead of (also) returning it, and answers 503 `email.not_configured` when
  email is not configured. Magic links (`m0/04` B22, I8) queue `magic_link` emails through `queue`;
  `m0/04` I8 builds its routes on this spec.
- **B21.** Security notices: when a user's password is set (`password.set`), a passkey is added
  or removed (`passkey.create`, `passkey.delete`), or TOTP is turned on or off (`totp.enable`,
  `totp.disable`), the same transaction queues a `security_notice` to that user, from the platform
  sender (Q6), naming the event, the time in the user's time zone and the client address recorded
  in the audit entry.

### Local self sign-up

- **B22.** `m0/04` Q6, decided by the owner, says local self sign-up "waits for M1 email
  verification", and no M1 spec but this one sends email. The plan lists self sign-up "if
  enabled" as a way in and says nothing of its shape. The owner decided it (Q8 (b)): sign-up
  creates the user at once, inactive until the address is verified. The rest of this section,
  B22a to B22g, is how that works; the points the plan and the owner's answer leave open are
  coordinator decisions pending owner review (Q8).
- **B22a. The `unverified` status.** `users.status` gains a third value, `unverified`, next to
  `active` and `suspended` (`m0/03` B7). Only self sign-up creates it and only verification (B22d)
  leaves it: every other status write refuses it, and refuses to touch an unverified user. `PATCH
  /api/v1/users/{user_id}` with `status: "unverified"` answers 422 `request.invalid`
  (`user.status_invalid` at `/status`), and so do `stepclimb admin` commands and the M4 sync API and
  SCIM when they write `status`; the repository's `users::set_status` takes only `active` and
  `suspended`, so no caller can write the value. An unverified user has no credential, no identity,
  no role assignment, no enrollment and no `verified_email`, and none can be given to it: every
  route that reads or names users other than sign-up and verification treats an unverified user as
  absent (404 `user.not_found` on `GET`, `PATCH`, enrollment, assignment, role assignment and
  enrollment-link routes and as a `manager_id`; left out of `GET /api/v1/users` and of
  `users::count_for_limit`, the users limit's count and the only user count the server computes in
  M1). The one intended exception is the email log: `GET /api/v1/admin/email/messages` lists the
  `signup_verification` messages with their `user_id` and `to_address`, since an admin who supports
  sign-up needs to see whether the link went out. The `user_id` resolves to nothing else. It cannot
  sign in by any method: it has no passkey, password or TOTP; a magic-link request for its address
  answers 202 and sends nothing (`m0/04` B22); admission step 1 (`m0/07` B4) refuses any user whose
  status is not `active`; email matching (`m0/07` B5) never links an identity to it, since matching
  needs `verified_email`. The request authenticator (`m0/08`) admits only an `active` user. I7
  changes every status check in the server from "is suspended" to "is not active", so a user in any
  other status is refused (fail closed). An unverified user does not count toward the users limit
  (`m0/10` B10), which counts `active` and `suspended` users.
- **B22b. Signing up.** When the organization allows sign-up (`admission.signup_allowed`, `m0/07`
  B20), sync-only mode is off (`m0/07` B11) and the local provider is enabled, `POST
  /auth/local/sign-up` with `{ email, display_name }` (`m0/03` B2's rules for each; 422
  `request.invalid` otherwise) answers 202 with an empty body whatever the address. Otherwise it
  answers 403 `auth.signup_closed` (sign-up off or sync-only mode on) or 404
  `auth.method_disabled` (local provider off), which depend only on the organization's settings.
  It is rate limited per client address and per email address (`m0/08` B22) and passes the CSRF
  check (`m0/04` B23). It runs in one transaction under the organization row's lock (`FOR NO KEY
  UPDATE`, as `m0/03` B7's suspension takes it, so two sign-ups for one address cannot both
  insert). First it checks the users limit with one more user (403 `entitlement.limit_reached`)
  for every address, so the answer cannot tell a known address from a new one. Then:
  - **a new address** (no user with it under `lower()`): inserts a `local` user with status
    `unverified`, the given `email` and `display_name`, the organization's default time zone and
    `verified_email` null, and a `signup_verification` token (B22c), and queues a
    `signup_verification` email to it;
  - **an `active` or `suspended` user's address**: writes no user or token and queues a
    `signup_existing` email to that user, which says an account exists and links to the sign-in
    page;
  - **an `unverified` user's address**: keeps the user, sets its `display_name` to the newest
    request's, replaces its unused token with a new one and queues a new `signup_verification`
    email. Signing up again is the resend; there is no other resend route. The person who opens
    the link therefore gets the name they last typed, not one a stranger chose first.
  The branches do different work (a new address inserts two rows; an existing one inserts none),
  so their response times can differ by a few database writes. That difference is accepted: the
  status and body are the same, both queue one email in one transaction, and the per-address and
  per-client rate limits bound how many addresses anyone can probe.
- **B22c. The verification token.** A `local_tokens` row of kind `signup_verification`, stored as
  the SHA-256 of a 256-bit token like `m0/04`'s enrollment tokens, valid 24 hours by the request's
  clock, one-time, with no creator (`created_by` null, `created_by_instance` false); `m0/04` B6's
  creator re-check applies to enrollment links only. A new token removes the user's earlier
  unused one of the same kind (`m0/04` B6). The link opens a web app page whose button posts the
  token, as `m0/04` B22 does for magic links, so a mail scanner that follows links cannot use it.
- **B22d. Verifying.** `POST /auth/local/sign-up/verify` with `{ token }`, rate limited and
  CSRF-checked as B22b, takes the organization row's lock as B22b does, locks the token's user row
  and checks, in order: the token is known,
  unused, unexpired and of kind `signup_verification`, and its user is `unverified` (else 400
  `auth.signup_invalid`, one answer for all of them); sign-up is still allowed and sync-only
  mode still off (403 `auth.signup_closed`); the users limit, counting this user as new (403
  `entitlement.limit_reached`, the user stays unverified and the token unused). Then it sets the
  user `active` with `verified_email = lower(email)`, marks the token used, writes one
  `user.verify_signup` audit entry (actor: the user), and creates an enrollment link for the user
  as the instance (`created_by_instance`, `m0/04` B6, 7-day lifetime) whose token it answers to
  the page, which continues into the enrollment flow that sets a passkey or password (`m0/04` B7).
  No session is created until that flow finishes. A person who leaves before setting a
  credential has an active user with no way to sign in; a magic link (when the method is on) or
  an admin's enrollment link gets them in, as for any user without a credential.
- **B22e. Expiry.** A periodic job, `users.prune_unverified` (hourly, `01` B17's scheduler, by the
  controllable clock), deletes every user with `status = 'unverified'`, `source = 'local'`, no
  enrollment, no role assignment and no identity that has no unused, unexpired `signup_verification`
  token, with its tokens and its `email_messages` rows, organization by organization. The extra
  conditions are a second guard: a user that somehow has any of them is left alone and logged as an
  error, never deleted. An unverified user therefore lasts at most 24 hours after its newest sign-up
  request, plus up to one hour. Signing up again after expiry but before the prune reuses the row
  (B22b); after the prune it is a new address again.
- **B22f. An address a verified source claims.** Every write that gives an `active` or
  `suspended` user an address held by an `unverified` user deletes the unverified user (B22e's
  deletion) in the same transaction first: an admin's `POST /api/v1/users` or `PATCH` of
  `email` (`m0/03` B9), `stepclimb admin create`, and just-in-time creation at an identity
  provider sign-in (`m0/07` B20). An unverified sign-up therefore never blocks an admin or a
  provider from using the address, and its link stops working.
- **B22g. What an unverified user cannot reach, in one list.** Sign-in by any method, sessions,
  API keys, app tokens, admin reads and writes about it, enrollments, assignments, role
  assignments, announcements and notifications, every email kind except `signup_verification`,
  and the users limit. B22a states each.

## API

Common errors as in `../m0/02-organizations-and-accounts.md` -> API; unsafe methods also answer
`m0/08`'s CSRF refusals except the unsubscribe `POST` (B17).

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/admin/email/sender` | | 200 `EmailSender` or 200 `{ sender: null, fallback: true }` | | `perm::EmailManage` (organization) |
| `PUT /api/v1/admin/email/sender` | `{ kind, smtp?: {...}, domain?: {...}, from_name, enabled? }` | 200 `EmailSender` | 422 `email.sender_invalid`, `email.sender_host_refused`, 403 `entitlement.feature_unavailable` | `perm::EmailManage`; `custom_email_sender` unless only disabling (B7) |
| `DELETE /api/v1/admin/email/sender` | | 204 | 404 `email.sender_not_found` | `perm::EmailManage` |
| `POST /api/v1/admin/email/sender/verify` | | 202 `EmailSender` | 404, 409 `email.sender_verify_failed` (with the reason), 503 `email.not_configured`, 403 feature | `perm::EmailManage`; `custom_email_sender` |
| `POST /api/v1/admin/email/sender/confirm` | `{ code }` | 200 `EmailSender` | 404, 422 `email.code_invalid`, 429 | `perm::EmailManage`; `custom_email_sender` |
| `GET /api/v1/admin/email/templates` | | 200 `EmailTemplate[]` (every kind, `customized` flag) | | `perm::EmailManage` |
| `GET /api/v1/admin/email/templates/{kind}` | | 200 `EmailTemplate` | 404 `email.kind_unknown` | `perm::EmailManage` |
| `PUT /api/v1/admin/email/templates/{kind}` | `{ subject, body }` | 200 `EmailTemplate` | 404, 409 `email.template_not_editable`, 422 `email.template_invalid` | `perm::EmailManage` |
| `DELETE /api/v1/admin/email/templates/{kind}` | | 204 | 404 | `perm::EmailManage` |
| `POST /api/v1/admin/email/templates/{kind}/preview` | `{ subject, body }` | 200 `{ subject, html, text }` | 404, 422 | `perm::EmailManage` |
| `GET /api/v1/admin/email/messages` | `?user_id&kind&status&cursor&limit` | 200 `Page<EmailMessage>` | | `perm::EmailManage` |
| `POST /email/unsubscribe/{token}` | `List-Unsubscribe=One-Click` (form) | 200 | 404 `email.unsubscribe_invalid`, 429 | none: the token is the credential |
| `GET /email/unsubscribe/{token}` | | 200 the web app | | none (serves the SPA) |
| `POST /auth/local/sign-up` | `{ email, display_name }` | 202 | 403 `auth.signup_closed`, 403 `entitlement.limit_reached`, 404 `auth.method_disabled`, 422 `request.invalid`, 429 `auth.rate_limited` | none (B22b) |
| `POST /auth/local/sign-up/verify` | `{ token }` | 200 `{ enrollment_token }` | 400 `auth.signup_invalid`, 403 `auth.signup_closed`, 403 `entitlement.limit_reached`, 429 `auth.rate_limited` | none: the token is the credential (B22d) |

Changed: `POST /api/v1/admin/users/{user_id}/enrollment-links` (`m0/04`) takes `send_email` (B20).
Schemas: `EmailSender { kind, status: "pending" | "verified" | "disabled", from_name, from_address,
smtp?: { host, port, tls, username }, domain?: { domain, dns_records: [{ type, name, value }] },
verified_at, last_error }` (never the password or private key); `EmailTemplate { kind, category,
required, variables: [{ name, description }], subject, body, customized }`; `EmailMessage { id,
kind, user_id, to_address, sender: "platform" | "fallback" | "organization", status:
"queued" | "sent" | "failed" | "skipped", reason, created_at, sent_at }`.

Configuration added (ADR 0022; secrets with `_FILE`; all in `check-config`): `smtp_url`,
`smtp_username`, `smtp_password(_FILE)`, `smtp_insecure` (`false`), `mail_from`, `mail_from_name`
("Stepclimb LMS"), `mail_spf_include` (unset).

## Data

Migrations `migrations/<version>_email.sql` (I1) and `migrations/<version>_email_senders.sql`
(I4). Versions are placeholders, fixed when each issue is implemented so each sorts after every
migration on `next` and every open branch.

```sql
-- <version>_email.sql
CREATE TABLE email_templates (                 -- not in the data model: B9
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    kind        text        NOT NULL,
    subject     text        NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 400),
    body        jsonb       NOT NULL CHECK (jsonb_typeof(body) = 'object'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, kind)
);

CREATE TABLE email_messages (                  -- not in the data model: B12 to B16
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    kind         text        NOT NULL,
    to_address   text,
    sender       text        CHECK (sender IN ('platform', 'fallback', 'organization')),
    status       text        NOT NULL CHECK (status IN ('queued', 'sent', 'failed', 'skipped')),
    reason       text,
    variables    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    secret       bytea,                       -- sealed (B12)
    source_id    uuid,                        -- the announcement or other object: B19's idempotency
    message_id   text,
    sent_at      timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    UNIQUE (user_id, kind, source_id)
);
CREATE INDEX email_messages_by_org ON email_messages (org_id, created_at DESC);

-- <version>_email_senders.sql
CREATE TABLE email_senders (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL UNIQUE REFERENCES organizations (id),
    kind         text        NOT NULL CHECK (kind IN ('domain', 'smtp')),
    config       jsonb       NOT NULL CHECK (jsonb_typeof(config) = 'object'),  -- no secrets
    secret       bytea,                       -- sealed SMTP password or DKIM private key
    enabled      boolean     NOT NULL DEFAULT true,
    verified_at  timestamptz,
    last_error   text        CHECK (char_length(last_error) <= 500),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE email_sender_codes (             -- B5; one live code per sender
    sender_id   uuid        PRIMARY KEY REFERENCES email_senders (id) ON DELETE CASCADE,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    code_hash   bytea       NOT NULL,
    tries       smallint    NOT NULL DEFAULT 0,
    expires_at  timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
```

I7's migration `migrations/<version>_self_sign_up.sql` widens two checks: `users.status IN
('active', 'suspended', 'unverified')` and `local_tokens.kind IN ('enrollment', 'magic_link',
'signup_verification')`, with a check that a `signup_verification` token has no creator
(`created_by IS NULL AND NOT created_by_instance`). A partial index on `users (org_id) WHERE
status = 'unverified'` serves the prune (B22e).

`email_messages.secret` is set only for required sign-in kinds and cleared (set to null) when the
message is sent, failed or skipped, so a sealed link lives only while it is waiting.

## Permissions

| Permission | Key | Grantable at | Built-in roles (M1) |
| --- | --- | --- | --- |
| `perm::EmailManage` | `email.manage` | organization | Organization admin |

Entitlement feature: `custom_email_sender` (`feature::CustomEmailSender`), added to the catalog
(`m0/10` Q1's pattern), checked on sender setup and verification (B7). Templates are not gated: the
plan gives every organization editable templates.

## Test cases

SMTP tests run against an SMTP sink started by the test on `127.0.0.1` (written for the tests, or a
crate after `dependency-researcher`), never the network; DNS tests against a stub resolver.

- **T1** (B1). Without `smtp_url`, `serve` starts, `check-config` prints `email: not configured`,
  an enrollment link with `send_email` answers 503 `email.not_configured`, and an announcement
  records `skipped` rows with reason `email_not_configured` while in-app notifications arrive.
  `smtp_url` without `mail_from` stops startup naming it; `smtp://` without STARTTLS support at the
  sink fails sending unless `smtp_insecure`.
- **T2** (B3). With no sender, an announcement email arrives from `"<Org> via Stepclimb"
  <mail_from>`; with a verified SMTP sender, from the organization's address through the
  organization's sink.
- **T3** (B4). A sender host of `127.0.0.1`, `169.254.169.254` or a private address on a
  two-organization instance answers 422 `email.sender_host_refused`; the password never appears
  in `GET`, logs or audit data.
- **T4** (B5). Verification sends a code to the admin's address at the sink; a wrong code five
  times locks the code; the right code sets `verified_at`; changing the host clears it.
- **T5** (B6). Against the stub resolver: matching DKIM and SPF records verify the domain; a
  wrong DKIM key fails with the reason; a message sent through the platform sink carries a DKIM
  signature that verifies with the published key and covers `List-Unsubscribe`; a daily
  re-check that fails clears `verified_at` and notifies admins.
- **T6** (B9). A subject with `{{nope}}`, a body with an image, and a sign-in template without
  `link` each answer 422 `email.template_invalid`; a platform kind answers 409; `DELETE` restores
  the default.
- **T7** (B10). Golden files: each default template renders to the committed HTML and text with
  example values; a variable value `<script>` appears escaped; the brand color of the vZDC example
  theme appears in the layout.
- **T8** (B12). A queued magic link message stores its link only in `secret`, sealed; no job
  payload, `variables` value or captured log line contains the token; after sending, `secret` is
  null.
- **T9** (B13). A suspended user, a user without email, and a user who turned announcement email
  off get `skipped` rows with the reasons; a required kind is sent despite the preference.
- **T10** (B14). A sink answering 451 retries the job; 550 marks the message `failed` without
  retry; three connection failures of an organization sender clear `verified_at`, notify admins,
  and the next message goes through the fallback.
- **T11** (B15, B17). An announcement email carries both RFC 8058 headers; posting the one-click
  body to the URL turns the user's announcement email preference off; a tampered token answers 404;
  a required kind's email has no such headers.
- **T12** (B18). `GET` of the unsubscribe URL changes nothing (a crawler-like GET leaves the
  preference on); Playwright: the confirmation page's button unsubscribes.
- **T13** (B19). An announcement to 3 recipients queues 3 emails; the fan-out run twice queues 3.
- **T14** (B21). Setting a password, adding a passkey and disabling TOTP each queue one
  `security_notice` from the platform sender; failing changes queue none.
- **T15** (B16). Messages older than 90 days are deleted (manual clock).
- **T16** (every route). Four cases for the admin routes: anonymous 401; every permission but
  `email.manage` 403; the wrong-scope case recorded as not applicable (organization-only
  permission, `m0/09` B20); `email.manage` alone 2xx. With a test plan lacking
  `custom_email_sender`, sender setup answers 403 `entitlement.feature_unavailable` while `GET` and
  `DELETE` work.
- **T17** (every route). Cross-organization: org A's admin's sender, templates and messages never
  include org B's; org B's sender and templates are unchanged; an unsubscribe token of org B
  changes only org B's user.

- **T18** (B22b). Sign-up with a new address answers 202, creates one `unverified` user and one
  token, and queues one `signup_verification` email; with an active user's address and with a
  suspended user's address it answers the same 202 with the same body, writes no user or token,
  and queues one `signup_existing` email to that user. With sign-up off or sync-only mode on it
  answers 403 `auth.signup_closed` and writes nothing; with the users limit reached it answers
  403 `entitlement.limit_reached` for a new address and for an existing one alike. Two
  concurrent sign-ups for one new address create one user (two connections).
- **T18a** (B22b, B22c). Signing up again for an unverified address answers 202, takes the second
  request's `display_name`, invalidates the first link (400 `auth.signup_invalid`) and sends a
  working second one.
- **T18b** (B22d). The link activates the user once: `status` becomes `active`,
  `verified_email` is set, the response carries an enrollment token that starts `m0/04` B7's
  flow, and one `user.verify_signup` entry is written; a second use, an unknown token and a token
  25 hours old (manual clock) each answer 400 `auth.signup_invalid` and change nothing. With
  sign-up turned off, or with the users limit reached, after the request and before
  verification, verification answers 403 with the matching code and the user stays unverified.
- **T18c** (B22a, B22g). An unverified user is refused or absent everywhere: `GET` and `PATCH
  /api/v1/users/{id}` and the enrollment-link route answer 404 `user.not_found`; `GET
  /api/v1/users` and the users-limit count leave it out; enrolling, assigning or giving it a role
  answers 404; a magic-link request for its address answers 202 and queues nothing; an identity
  provider sign-in with email matching on and the same address does not link to it; a session
  row inserted for it by the test is refused by the request authenticator; naming it as another
  user's `manager_id` answers 404 `user.not_found`. `PATCH /api/v1/users/{id}` with `status:
  "unverified"` on an active user answers 422 `user.status_invalid` at `/status` and changes
  nothing. `GET /api/v1/admin/email/messages` lists its `signup_verification` message (B22a's
  exception). Mutations: changing the authenticator's check back to "is suspended" turns the
  session case red; accepting `unverified` in `users::set_status` turns the status case red.
- **T18d** (B22e). With the manual clock, an unverified user whose token expired is deleted by
  `users.prune_unverified` with its token and email rows; one with an unexpired token is kept; an
  active or suspended user is never touched; an unverified user given an enrollment or a non-local
  `source` by the test in SQL is kept and logged; a run for org A deletes nothing of org B's.
- **T18e** (B22f). An admin creating a user, or a provider sign-in creating one, with the address
  of an unverified user succeeds, the unverified user is gone, and its link answers 400
  `auth.signup_invalid`.

## Out of scope

- Bounce and complaint handling (webhooks from providers, suppression lists): after M1.
- Amazon SES API sending and managed-hosting domain onboarding: M6 (`docs/plan/ui.md`
  SenderDomainSetup is listed under Managed hosting); M1's domain sender works through the
  instance's relay.
- Billing, plan, sign-up and operator emails: M6.
- Due reminders, escalations and weekly reports: spec 13 and later; grade releases: M2.
- Organization logos in emails: with logo upload (`../m0/12-tokens-and-theme-generator.md` Q6).
- Translated templates: English only at 1.0 (ADR 0012).
- Admin invites by email (`../m0/03-users-and-identities.md` Out of scope): an enrollment link
  with `send_email` covers the M1 case.

## Open questions

Q1 to Q7 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q8 was the owner's; the owner answered it on 2026-11-16.

- **Q1. An instance without email.** As with storage (`06` Q1), requiring SMTP would stop every
  M0 installation at its first M1 start. Options: (a) optional: the server starts, sending routes
  answer 503 `email.not_configured`, event email is recorded as skipped, `check-config` says so;
  (b) required; (c) a default of logging emails instead of sending. Recommendation: (a), the same
  explicit error as storage; (c) is a silent fallback.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. The SMTP library.** Options: (a) `lettre` with its rustls and DKIM features, through
  `dependency-researcher`; (b) our own SMTP client; (c) a sending API (SES, Postmark) only.
  Recommendation: (a); it covers SMTP, STARTTLS and DKIM signing, and SMTP works with every
  provider a self-hoster has.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Domain senders in M1.** The plan's domain path is "DKIM and SPF, through SES or another
  provider". Options: (a) M1 builds SMTP senders first and a domain sender last (its own issue,
  I6): Stepclimb signs with a DKIM key it generates and sends through the instance relay, with DNS
  checks through a resolver dependency researched first; SES-backed onboarding is M6; (b) SMTP only
  in M1, domains in M6; (c) both through SES only. Recommendation: (a). It delivers both sender
  kinds the plan lists without tying self-hosters to SES; RSA 2048 because Ed25519 DKIM is not yet
  verified by every large mailbox provider (evidence points at this; I6 checks current support
  before building).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Gating custom senders.** Options: (a) a `custom_email_sender` feature on sender setup,
  everything enabled on `unlimited`; (b) no feature. Recommendation: (a), as `m0/10` Q1 gates other
  capabilities a paid tier could hold back; the fallback keeps email working without it.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. Keeping the email log.** Options: (a) metadata only, 90 days; (b) full rendered bodies, 90
  days; (c) none. Recommendation: (a): enough for "was the reminder sent" (spec 13's timeline) and
  support, with no stored body and no secret.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q6. Which security notices M1 sends, and from where.** The plan lists security notices as
  platform email and names no events. Options: (a) changes to a user's sign-in methods (password
  set, passkey added or removed, TOTP on or off), from the platform sender; (b) also every new
  sign-in; (c) none in M1. Recommendation: (a): the changes an account takeover would make, with
  no mail on every sign-in.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q7. Sign-in link emails' category.** The plan does not place enrollment and magic links.
  Options: (a) organization email, required (the person signs in to the organization, so it comes
  from its sender or "via Stepclimb"); (b) platform email. Recommendation: (a).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

- **Q8. Local self sign-up.** `m0/04` Q6 (owner, 13 Nov 2026) put local self
  sign-up after M1 email verification; the plan says only "self sign-up (if enabled)". The shape is
  the owner's. Options: (a) B22: a verify-your-address email whose link creates the user and goes
  on into passkey or password setup, gated by `admission.signup_allowed` and sync-only mode;
  (b) the user is created at once, inactive until the address is verified; (c) no local self
  sign-up in M1 (only through identity providers and admins). Recommendation: (a): no account
  exists for an unverified address, and every existing gate (sign-up switch, sync-only, users
  limit) applies.
  Owner decision (2026-11-16): (b), self sign-up creates the user at once, inactive until the
  address is verified (B22).
  Coordinator decisions (2026-11-16), pending owner review, for the points (b) leaves open:
  - "Inactive" is a new status, `unverified`, rather than `suspended`, so an admin's suspension
    and an unconfirmed sign-up stay distinct and the prune never deletes a suspended user (B22a).
  - An unverified user is absent from every admin route and from the users limit, and holds an
    address only until a verified source claims it (B22a, B22f). Being visible to admins would
    let them enroll or assign a person whose address nobody confirmed, and counting it would let
    anyone fill the limit with sign-ups.
  - The answer is 202 for every address, the limit is checked before the lookup, and an
    existing account gets a `signup_existing` email, so the route reveals no address (B22b).
  - Resend is signing up again; the newest request's `display_name` wins, so a stranger who signs
    up first cannot choose the name on the victim's account (B22b).
  - The timing difference between the branches is accepted under the rate limits rather than
    padded away (B22b).
  - Only sign-up writes `unverified`; every other status write refuses it, and the prune also
    requires a local user with no enrollment, role assignment or identity (B22a, B22e).
  - The admin email log is the one place an unverified sign-up shows (B22a).
  - The link lasts 24 hours and the prune runs hourly, so an unconfirmed address is held at most
    about 25 hours (B22c, B22e).
  - Verification activates the user and hands over an instance-made enrollment link, so the
    existing `m0/04` flow sets the first credential (B22d).

## Issues

1. **I1. Platform sender, messages and the send job.** `lettre` (pinned, own commit), the SMTP
   settings and the not-configured behavior, `email_templates` and `email_messages`, kind
   declarations with default templates, `queue`, rendering with the layout, `email.send` with
   retries and the 90-day prune. Covers B1, B2, B8, B10, B12 to B16; T1, T7 to T10, T15. Depends
   on: `01-job-queue-and-outbox.md` I4, `11-announcements-and-in-app-notifications.md` I2,
   `07-pages-and-the-editor.md` I2.
2. **I2. Unsubscribe.** Tokens, the RFC 8058 headers, the `POST` route with its CSRF exemption and
   rate limit, the confirmation page. Covers B17, B18; T11, T12. Depends on: I1.
3. **I3. Callers.** Announcement emails, enrollment links by email, security notices, and the
   `queue` hook `m0/04` I8 uses for magic links. Covers B19 to B21; T13, T14. Depends on: I1, I2.
4. **I4. SMTP senders.** `email_senders` and codes migration, `email.manage` and the
   `custom_email_sender` feature, sender routes, the address policy, verification by code, the
   fallback and failure handling, the email messages list. Covers B3 to B5, B7; T2 to T4, T16,
   T17. Depends on: I1.
5. **I5. Template editing.** Template routes with validation, preview, and the editor screen with
   the variable picker. Covers B9, B11; T6. Depends on: I1, `07-pages-and-the-editor.md` I3.
6. **I6. Domain senders.** DKIM key generation and signing, DNS records, verification through a
   resolver (pinned after `dependency-researcher`, own commit), the daily re-check. Covers B6;
   T5. Depends on: I4.
7. **I7. Local self sign-up.** Migration adding `unverified` to `users.status` and
   `signup_verification` to `local_tokens.kind`; every server status check changed to "is not
   active"; every other status write refusing `unverified` (422 `user.status_invalid`); `unverified`
   users left out of the admin user routes, the users limit, enrollment, assignment, role-assignment
   and manager lookups, magic links and email matching; the sign-up route with its rate limits; the
   `signup_verification` and `signup_existing` kinds and templates; the verification route and page
   with the hand-off to `m0/04`'s enrollment flow; the `users.prune_unverified` job with its second
   guard; the address hand-over in user creation, email changes and just-in-time creation. Covers
   B22 to B22g; T18 to T18e. Depends on: I3, `m0/04` I2, `01-job-queue-and-outbox.md` I3 (periodic
   jobs). Q8 is answered (owner, 2026-11-16). Maintainer line-by-line review.
