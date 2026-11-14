# Local accounts: passkeys, password and TOTP

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access -> Sign-in providers (local accounts:
  email plus passkey first, password with argon2id and TOTP as fallback, optional magic link,
  each switchable per organization), Admission, Provisioning (admin invites, self sign-up);
  Architecture and stack (`webauthn-rs`, `argon2`); Data model (`users`, `identities`,
  `identity_providers`, `passkeys`); Security and privacy; Milestones (M0: passkeys; M1: email);
  Hosting and operations (CLI `admin create`)
- **Depends on:** `03-users-and-identities.md` (users, identities, `identity_providers`),
  `07-admission.md` (I1), `08-sessions-and-csrf.md` (sessions, flow state, encryption, rate
  limits, CSRF), `09-permissions-and-roles.md` (`Require<P>`, catalog)
- **Review:** maintainer line-by-line

## Summary

People without an outside identity provider get a Stepclimb account identified by email, and
sign in with a passkey first. A password (hashed with argon2id) with a TOTP code is the
fallback, and a magic link by email is optional. An organization can switch each method off.
Before email exists (M1), accounts are enrolled through one-time enrollment links that an admin
creates and hands over.

## Behavior

### Methods and switches

- **B1.** Each organization has one `identity_providers` row with `kind = 'local'` and `key =
  'local'`, holding four switches: passkey, password, TOTP and magic link. An organization
  admin reads and changes them through `/api/v1/admin/local-sign-in`. Where the switches are
  stored and their defaults: pending Q2. Which methods are built in M0: pending Q1.
- **B2.** A switched-off method's endpoints answer 404 `auth.method_disabled`, and existing
  credentials of that kind stop working for sign-in until the method is switched on again. They
  are not deleted. When every local method is off, or the local row is disabled, local sign-in
  is not offered (`08-sessions-and-csrf.md` B38).

  Coordinator decisions (2026-11-14), for issue I1. TOTP is a second step after a password,
  not a way in by itself: local sign-in counts as offered only while the passkey, the password
  or the magic link is offered (a primary method), and with TOTP alone it is not offered. A
  change through `PUT /api/v1/admin/local-sign-in` that would leave the organization no primary
  way to sign in, meaning no offered primary local method and no enabled provider that is not
  local, is refused with 409 `local.last_method` and writes nothing; disabling the local row is
  such a change. The check runs under the organization row's lock that the route already
  holds.
- **B3.** A local account is a `users` row plus an `identities` row for the local provider. The
  identity's `subject`: pending Q4. The local row needs `kind = 'local'`, which widens the `kind`
  check of `03-users-and-identities.md` B13 by migration. This spec provides
  `local::has_credential(scope, user_id)` (a passkey or a password exists), which
  `03-users-and-identities.md` B19 calls before removing a user's last identity.
  `identities::unlink_unless_last` calls it (issue I2). No passkey or password can be stored
  before issues I3 and I6, so until then it answers `false`; each of those issues adds its
  check and a test that the credential lets the user's last outside identity go
  (coordinator decision, 2026-11-14).

  Coordinator decisions (2026-11-14), for issue I1:

  - Storage (Q2 (b)): the four switches are one `OrgSettings` key, `local_sign_in`, an object
    `{passkey, password, totp, magic_link}` whose missing members read as their defaults. Only
    `local_sign_in::update` writes it; the generic settings patch cannot name it. The local
    row's `enabled` column is the `enabled` of `/api/v1/admin/local-sign-in`. A `PUT`
    names all five members; a missing, `null` or unknown member answers 400 `request.malformed`.
  - The local row is written from code with a UUID v7 key: `organizations::create` for a new
    organization, and `stepclimb serve` at startup for an organization created before the
    migration, as the `org_plans` rows are. The migration widens the `kind` check and adds a
    check that `kind = 'local'` exactly when `key = 'local'`, so no OpenID Connect or OAuth 2
    provider can take the key `local`, and `UNIQUE (org_id, key)` allows one local row.
  - Escalation (the owner's rule for writes that affect another user's sign-in): a `PUT` that
    changes anything is refused with 403 `permission.escalation` unless the caller holds every
    catalog permission at organization scope. This is `03-users-and-identities.md` B6's rule
    applied to every user of the organization at once; an Organization admin holds every
    catalog permission, so the two agree whenever one exists. A `PUT` equal to the current
    switches changes nothing and is answered 200 with `provider.manage` alone. The check reads
    the switches under the organization row's lock, which settings writes take.
  - `local.magic_link_needs_email` is 422 `request.invalid` with the code at `/magic_link`. M0
    has no email sender, so `local_sign_in::update` refuses every change switching the magic
    link on.
  - Audit: a change of the switches writes an `organization.update_settings` entry with
    `local_sign_in` before and after; a change of the row's `enabled` writes an
    `identity_provider.update` entry with `enabled` before and after
    (`09-permissions-and-roles.md` B22). A `PUT` that changes both writes both.
  - `auth.method_disabled` is the server's `local_sign_in::require_method(scope, method)`,
    which every method route of issues I2 to I8 calls first. The local entries of
    `/.well-known/stepclimb` are `LocalSignIn::offered()` in `stepclimb-core` (none when the row
    is disabled or no primary method is on); the endpoint itself is `08-sessions-and-csrf.md`
    I9. `identity_providers::list_enabled` leaves the local row out, so it lists the outside
    providers only.
  - T1's route-level half (`/auth/local/passkey/start` answering 404 `auth.method_disabled`)
    lands with issue I2, whose routes are the first to call `require_method`; I1 tests the
    switches and `require_method` itself.
  - Defaults: passkey, password and TOTP on, the magic link off, the row enabled.

- **B4.** Local sign-in finds the user by the organization and the email address. Uniqueness
  and case handling: pending Q5.

  Coordinator decision (2026-11-14), for issue I2: Q5 stands as the owner decided it, option
  (a). `local_logins`, including its `email_normalized` column, is created by issue I6 with the
  password columns, not by I2, since nothing in I2 reads or writes it; the enrollment start
  route answers the user's email from `users.email`, which (a) allows, and a sign-in lookup by
  email arrives with I4 or I6. PostgreSQL's `lower()` folds case by the database's `LC_CTYPE`
  (or the column's collation): under the `C` locale it lowercases ASCII letters only, so
  `ÉLODIE@example.org` and `élodie@example.org` stay distinct. The normalizing type of
  `08-sessions-and-csrf.md` B41, which the per-account rate limit keys on, must fold exactly as
  the stored `email_normalized` does, so I6 picks one function for both and tests it with a
  non-ASCII address.
- **B5.** Every completed local sign-in goes through admission (`07-admission.md` B1) with the
  local provider, the identity's subject and no mapped attributes, and creates a session
  (`08-sessions-and-csrf.md` B7). A suspended user is refused there.

### Enrollment links

- **B6.** An admin holding the user-management permission creates an enrollment link for a user
  of the organization. The server returns the link (the web app's enrollment route with a
  random token) once, stores only the token's hash, and invalidates any earlier unused
  enrollment link for that user. Link lifetime: pending Q6. Until email exists (M1), the admin
  hands the link over; emailing it is M1.
  Coordinator decision (2026-11-14), for issue I2: a link outlives the rights that allowed
  it, so its use re-checks them. `local_tokens::check_enrollment` (the start route) and
  `local_tokens::consume_enrollment` (the completions of I3 and I6) refuse a link an admin made
  unless, at the time of use, that admin is still an active user of the organization, still
  holds `user.manage` at organization scope, and holds at organization scope every permission
  the link's user now holds at any scope (`03-users-and-identities.md` B6's escalation rule,
  `Grants::lacks_any_held_by`). A refused link answers the same 400 `auth.enrollment_invalid`
  as an unknown one. `local_tokens.created_by_instance` marks the links `stepclimb admin
  create` and `stepclimb admin enrollment-link` make, which skip these checks; a link that is
  not the instance's and has a NULL `created_by` lost its creator (`ON DELETE SET NULL`) and is
  invalid, never trusted. `consume_enrollment` runs at `READ COMMITTED`, like the other
  row-locking repository functions: its row lock makes a second use wait and then find the
  link used.
- **B7.** `POST /auth/local/enrollment/start` with the token answers with the user's email, the
  methods the user may set up (from B1's switches) and, when passkeys are on, WebAuthn
  credential creation options bound to a new sign-in flow (`08-sessions-and-csrf.md` B17). An
  unknown or used token answers 400 `auth.enrollment_invalid`; an expired one 400
  `auth.enrollment_expired`.

  Coordinator decisions (2026-11-14), for issue I2:

  - The link is the public URL's `/enroll` page with the token in the fragment,
    `https://<host>/enroll#token=<token>`. A browser sends no fragment to a server, so the token
    stays out of access logs, proxy logs and `Referer` headers; the page posts it to the start
    route. The token is a 256-bit `stepclimb_auth::token::Token` (43 characters); the database
    holds its SHA-256 only.
  - Creating a link deletes the user's earlier unused enrollment links (B6), under a lock on
    the user's row, so two creations at once leave one live link.
  - A user without an email gets no link: 409 `local.email_required`, since B4 finds a user by
    email. A link whose user has lost the email since answers `auth.enrollment_invalid`.
  - Creating a link for another user is an escalation-guarded write: it lets the holder set up
    a way to sign in as that user, so it applies `03-users-and-identities.md` B6's rule (403
    `permission.escalation` for a user with a permission the caller does not hold at
    organization scope) under the identity admin routes' locks. It writes an
    `enrollment_link.create` entry (`09-permissions-and-roles.md` B22) with the user and the
    expiry, never the token or its hash.
  - The start route takes `RateLimited<SignIn>` first, then answers 404 `auth.method_disabled`
    unless the organization offers the passkey or the password (the methods a link can set
    up), before it reads the token. `methods` lists those offered, passkey first. It uses
    nothing up and checks no user status: the passkey and password routes (I3, I6) mark the
    link used in the transaction that stores the credential, and admission (B5) refuses a
    suspended user there. The answer carries `Cache-Control: no-store`.
  - Until issue I3 adds `passkey_options`, the start route starts no sign-in flow and sets no
    flow cookie: the flow exists only to hold the WebAuthn challenge.
  - An unknown token, one of another organization, one replaced by a newer link and a used one
    all answer the same 400 `auth.enrollment_invalid`; only an expired one answers
    `auth.enrollment_expired`, as this behavior says. Telling expired apart reveals only what
    the holder of the token already has.
- **B8.** `POST /auth/local/enrollment/passkey` with the browser's registration response and an
  optional passkey name verifies it with `webauthn-rs` against the flow's challenge, the
  organization's relying party ID and origin (`08-sessions-and-csrf.md` Q1), and the WebAuthn
  policy of Q12. On success, in one transaction, it stores a `passkeys` row, marks the token
  used, creates the local identity if absent, and completes sign-in (B5). A failed check
  answers 400 `auth.passkey_invalid` and leaves the token usable.
- **B9.** `POST /auth/local/enrollment/password`, when password sign-in is on, sets the user's
  password from the token instead (policy pending Q8), marks the token used, creates the local
  identity if absent, and completes sign-in.
- **B10.** How the first organization admin of a new instance gets an account (for example
  `stepclimb admin create` printing an enrollment link): pending Q7. Self sign-up with a local
  account needs email verification and waits for email (M1): pending Q6. `stepclimb admin
  create` exists (`02-organizations-and-accounts.md` B27); issue I2 adds the printed enrollment
  link (decided by the owner, 13 Nov 2026).

  Coordinator decisions (2026-11-14), for issue I2:

  - `stepclimb admin create` makes the first admin's link with the same function as the admin
    route (`enrollment::create_link`), by the instance (`created_by_instance`, `created_by`
    NULL), in the transaction that creates the organization, so a failure leaves neither. The
    link has Q6's 7-day lifetime and is printed once, as the last line of stdout, after a line
    naming its expiry (Q7 (a)). When stdout is not a terminal the command also prints one line
    to stderr: the link is an admin credential for 7 days, and any log that keeps the output
    keeps it. When the report cannot be written after the commit, the stderr message names the
    organization and the admin as created and gives the recovery command below.
  - `stepclimb admin enrollment-link --org <slug> --user <email or id>
    [--admin-required-override] [--force]` makes a new link from the host's shell, so an
    instance whose first admin's link expired unused, or whose `admin create` commit outcome
    was unknown, has a way back in. It exists only on the command line: no HTTP route,
    environment variable or setting triggers it. The organization and the user are always
    named. Under the user's row lock it refuses (exit 2, nothing written) a user who is not an
    Organization admin at organization scope unless `--admin-required-override`, and a user who
    can already sign in (`local::has_credential`, or an identity at an enabled provider that is
    not local) unless `--force`. It makes the link with `enrollment::create_link`, which deletes
    the user's earlier unused links and writes `enrollment_link.create` by the instance with
    `recovery: true`, commits, then prints with `admin create`'s output rules. When the print
    fails it says the link exists and that running the command again replaces it.

### Passkey sign-in

- **B11.** `POST /auth/local/passkey/start` with an email address answers with WebAuthn
  credential request options listing that user's passkeys, bound to a new sign-in flow. What
  the server answers for an email with no account or no passkey, and whether a
  username-less (discoverable) sign-in is offered as well: pending Q9.
- **B12.** `POST /auth/local/passkey/finish` with the browser's assertion verifies it against the
  flow's challenge and the stored credential. On success it saves the credential's updated
  sign counter and backup state, completes sign-in (B5) and answers 200 with the flow's
  `return_to`. A failed check answers 401 `auth.passkey_invalid`. A counter that goes
  backwards: pending Q12.

### Passkey management

- **B13.** A signed-in user lists, adds, renames and deletes their own passkeys under
  `/api/v1/me/passkeys`. Adding uses the same ceremony as B8 with the session's user instead of
  a token. A passkey id that is not the caller's answers 404. Whether deleting the user's last
  way to sign in is allowed: pending Q10.

### Password

- **B14.** `POST /auth/local/password` with email and password checks the password against the
  stored argon2id hash. A wrong password, an unknown email, a user with no password, or
  password sign-in switched off for the user's organization all answer 401
  `auth.credentials_invalid`, and the server runs one argon2id verification in every case so
  the response time does not reveal which (enumeration handling as a whole: Q9).
- **B15.** When the password is right and the user has confirmed TOTP, the response is 200
  `{next: "totp"}` with a sign-in flow, and no session exists yet. Otherwise sign-in completes
  (B5). Whether TOTP is required with a password: pending Q11.
- **B16.** Passwords are hashed with argon2id using the parameters of Q8, with a per-hash random
  salt, in the PHC string format. When a stored hash uses older parameters, a successful sign-in
  rehashes it. The number of hashes computed at once is capped (pending Q8) so sign-in bursts
  cannot exhaust memory on the 1 GB budget (`docs/plan/technical.md` -> Hosting and
  operations).
- **B17.** A signed-in user changes their password with `PUT /api/v1/me/password`, giving the
  current one. A wrong current password answers 401 `auth.credentials_invalid`. Whether other
  sessions end: pending Q8.
- **B18.** Failed password checks are rate limited per client IP and per account
  (`08-sessions-and-csrf.md` B22, B23).

### TOTP

- **B19.** A signed-in user with a password starts TOTP enrollment with `POST /api/v1/me/totp`.
  The server generates a secret, stores it encrypted (`08-sessions-and-csrf.md` B3) as
  unconfirmed, and returns an `otpauth://` URI and the secret once. `POST
  /api/v1/me/totp/confirm` with a valid code confirms it. Starting again before confirming
  replaces the unconfirmed secret.
- **B20.** `POST /auth/local/totp` with a code and the flow from B15 completes sign-in when the
  code is valid for the current time step, within the window of Q11, read from the
  controllable clock. A code already accepted for that user is refused when presented again,
  as RFC 6238 section 5.2 requires. A wrong code answers 401 `auth.totp_invalid` and counts
  against the account's rate limit (B18).
- **B21.** Disabling TOTP, recovery codes, and the parameters (algorithm, digits, period):
  pending Q11.

### Magic link

- **B22.** When the magic link method is on and the organization can send email (M1), `POST
  /auth/local/magic-link` with an email answers 202 whatever the address, and sends a one-time
  link to an account with that email. The link opens a web app page with a "Sign in" button
  that calls `POST /auth/local/magic-link/finish` with the token, so a mail scanner that
  follows links cannot use it. A valid token completes sign-in; an unknown, used or expired one
  answers 400 `auth.magic_link_invalid`. Lifetime and timing: pending Q13.

### Common

- **B23.** Every `/auth/local/...` route is rate limited (`08-sessions-and-csrf.md` B22) and
  passes the CSRF check (`08-sessions-and-csrf.md` B14).
- **B24.** No password, TOTP secret or code, enrollment token or magic-link token is logged
  (`08-sessions-and-csrf.md` B4).

## API

Every endpoint has `#[utoipa::path]` and appears in `openapi/stepclimb.json`. Errors are RFC
9457 `application/problem+json`. Every route also answers 404 `auth.method_disabled` when its
method is off, 429 `auth.rate_limited`, 403 `auth.csrf_refused` on unsafe methods, and on
`/api/v1` the authentication errors of `08-sessions-and-csrf.md`. Sign-in completions also
answer the admission refusals of `07-admission.md` as 403.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/admin/local-sign-in` | none | 200 `{enabled, passkey, password, totp, magic_link}` | `permission.denied` | `perm::ProviderManage`, organization |
| `PUT /api/v1/admin/local-sign-in` | same shape | 200 | `local.magic_link_needs_email` (on before an email sender exists), `local.last_method` (409, B2), `permission.denied`, `permission.escalation` | `perm::ProviderManage`, organization |
| `POST /api/v1/admin/users/{user_id}/enrollment-links` | none | 201 `{url, expires_at}` | `user.not_found`, `local.email_required` (409), `permission.denied`, `permission.escalation` | `perm::UserManage`, organization (see Permissions) |
| `POST /auth/local/enrollment/start` | `{token}` | 200 `{email, methods: [...], passkey_options?}`; sets the flow cookie once I3 adds `passkey_options` | `auth.enrollment_invalid`, `auth.enrollment_expired` | none |
| `POST /auth/local/enrollment/passkey` | `{token, credential, name?}` | 200 `{return_to}`; sets `__Host-sc_session` | `auth.enrollment_invalid`, `auth.enrollment_expired`, `auth.flow_invalid`, `auth.flow_expired`, `auth.passkey_invalid` | none |
| `POST /auth/local/enrollment/password` | `{token, password}` | 200 `{return_to}`; sets the session | `auth.enrollment_invalid`, `auth.enrollment_expired`, `local.password_weak` | none |
| `POST /auth/local/passkey/start` | `{email, return_to?}` | 200 `{options}`; sets the flow cookie | (Q9) | none |
| `POST /auth/local/passkey/finish` | `{credential}` | 200 `{return_to}`; sets the session | `auth.flow_invalid`, `auth.flow_expired`, `auth.passkey_invalid` | none |
| `POST /auth/local/password` | `{email, password, return_to?}` | 200 `{return_to}` with the session, or 200 `{next: "totp"}` with the flow cookie | `auth.credentials_invalid` | none |
| `POST /auth/local/totp` | `{code}` | 200 `{return_to}`; sets the session | `auth.flow_invalid`, `auth.flow_expired`, `auth.totp_invalid` | none |
| `POST /auth/local/magic-link` | `{email, return_to?}` | 202 | none beyond the common ones | none |
| `POST /auth/local/magic-link/finish` | `{token}` | 200 `{return_to}`; sets the session | `auth.magic_link_invalid` | none |
| `GET /api/v1/me/passkeys` | none | 200 `[{id, name, created_at, last_used_at?}]` | | signed in |
| `POST /api/v1/me/passkeys/start` | none | 200 `{options}`; flow cookie | | signed in |
| `POST /api/v1/me/passkeys` | `{credential, name?}` | 201 `{id, name}` | `auth.flow_invalid`, `auth.passkey_invalid` | signed in |
| `PATCH /api/v1/me/passkeys/{id}` | `{name}` | 200 | `passkey.not_found` | signed in |
| `DELETE /api/v1/me/passkeys/{id}` | none | 204 | `passkey.not_found`, `local.last_method` (pending Q10) | signed in |
| `PUT /api/v1/me/password` | `{current_password, new_password}` | 204 | `auth.credentials_invalid`, `local.password_weak` | signed in |
| `POST /api/v1/me/totp` | none | 200 `{otpauth_uri, secret}` | `local.password_required` | signed in |
| `POST /api/v1/me/totp/confirm` | `{code}` | 204 | `auth.totp_invalid`, `local.totp_not_started` | signed in |

The magic-link routes are built only if Q1 and Q13 put them in M0. Disabling TOTP and recovery
code routes are added once Q11 is answered.

## Data

Keys are UUID v7 generated by the server; every table has `created_at` and `updated_at`.
Migrations are forward-only, one file per issue.

**`passkeys`** (plan columns): `id uuid PK`, `user_id uuid NOT NULL REFERENCES users(id) ON
DELETE CASCADE`, `credential jsonb NOT NULL` (the credential `webauthn-rs` serializes, including
its public key and counter; no secret), `name text NOT NULL`, `created_at`, `updated_at`.
Proposed, pending Q3: `credential_id bytea NOT NULL UNIQUE` (so one authenticator cannot be
registered twice, and for discoverable sign-in if Q9 adds it) and `last_used_at timestamptz
NULL`. Organization: derived through `user_id` -> `users.org_id`.

**`identity_providers`** (from `03-users-and-identities.md`): one row per organization with
`kind = 'local'`, `key = 'local'`; its `config` holds the switches if Q2 chooses that.

**`identities`** (from `03-users-and-identities.md`): one row per local account, `subject` per Q4,
`attributes` `{}`.

Proposed, pending Q3 (none of these is in the plan's data model):

- **`local_logins`**: `user_id uuid PK REFERENCES users(id) ON DELETE CASCADE`, `org_id uuid NOT
  NULL REFERENCES organizations(id)`, `email_normalized text NOT NULL`, `password_hash text
  NULL` (PHC string), `password_changed_at timestamptz NULL`, `totp_secret bytea NULL`
  (AES-256-GCM), `totp_confirmed_at timestamptz NULL`, `totp_last_step bigint NULL` (B20's
  replay check), `created_at`, `updated_at`; `UNIQUE (org_id, email_normalized)`. `org_id` is
  direct for the unique index and must equal the user's (a composite foreign key to `users(id,
  org_id)` if `03-users-and-identities.md` adds that unique pair). Not needed if Q5 puts email
  uniqueness on `users` and Q3 puts the password and TOTP columns elsewhere.
- **`local_tokens`**: `id uuid PK`, `org_id uuid NOT NULL REFERENCES organizations(id)`,
  `user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE`, `kind text NOT NULL CHECK
  (kind IN ('enrollment', 'magic_link'))`, `token_hash bytea NOT NULL UNIQUE`, `created_by uuid
  NULL REFERENCES users(id)`, `expires_at timestamptz NOT NULL`, `used_at timestamptz NULL`,
  `created_at`, `updated_at`. Index on `(user_id, kind)` for B6's invalidation. Built by issue I2
  (`migrations/20261114150000_local_tokens.sql`) with four changes: the user's foreign key is
  the composite `(org_id, user_id) REFERENCES users (org_id, id) ON DELETE CASCADE`, so a
  token's user is always in the token's organization; `token_hash` checks
  `octet_length = 32`, as `sessions.token_hash` does; `created_by` is `ON DELETE SET NULL`;
  and the index is `(org_id, user_id, kind)`; and `created_by_instance boolean NOT NULL`
  marks a link the instance made, with a check that such a link has no `created_by`
  (coordinator decision, 2026-11-14, B6).
- Recovery codes, if Q11 adds them: `totp_recovery_codes(id, user_id, code_hash, used_at,
  created_at, updated_at)`, organization derived through `user_id`.

## Permissions

| Entry | Scope | Built-in roles |
| --- | --- | --- |
| `perm::ProviderManage` (`09-permissions-and-roles.md` B2, which lists 04 as a checker): change the local method switches, pending Q2 | organization | Organization admin |
| `perm::UserManage` (`09-permissions-and-roles.md` B2): create enrollment links | organization (account scope only if `09-permissions-and-roles.md` Q2 widens it) | Organization admin |

`/api/v1/me/...` routes need only a signed-in session: every user manages their own
credentials. They are not callable with an API key (a key must not add or remove its owner's
passkeys); such a request answers 403 `auth.session_required`. This spec checks no entitlement
feature (see `05-oidc.md` Q1).

## Test cases

Database tests use `#[sqlx::test]`. Expiry and TOTP tests use the controllable clock. WebAuthn
tests use a software authenticator in the test process (I3 picks it: a crate from the
`webauthn-rs` project if its license and release date pass the dependency rules, or recorded
registration and assertion fixtures); the Playwright test uses Chromium's virtual
authenticator.

- **T1.** (B1, B2) Switching passkeys off makes `/auth/local/passkey/start` answer 404
  `auth.method_disabled` and an existing passkey cannot sign in; switching it back on restores
  sign-in without re-enrollment.
- **T2.** (B6) An enrollment link is returned once; the database holds only its hash; creating a
  second link makes the first answer `auth.enrollment_invalid`.
- **T3.** (B7) A link used once answers `auth.enrollment_invalid`; a link past its lifetime
  (clock moved) answers `auth.enrollment_expired`.
- **T4.** (B8) Registering a passkey through a link stores the row, creates the local identity,
  sets the session cookie and marks the token used; a registration response for another
  challenge, origin or RP ID answers `auth.passkey_invalid` and the token still works.
- **T5.** (B9) Setting a password through a link signs in; a password failing Q8's policy
  answers 422 `local.password_weak`.
- **T6.** (B11, B12) Passkey sign-in with a registered credential creates a session and updates
  the stored counter; an assertion signed by another key answers 401 `auth.passkey_invalid`;
  replaying a finished flow answers `auth.flow_invalid`.
- **T7.** (B11) The answer for an unknown email matches Q9's decision (for the recommended
  option: the same response shape as a known email with no passkeys).
- **T8.** (B5) A suspended user's correct passkey or password is refused with `auth.suspended`.
- **T9.** (B13) A user lists, renames and deletes their passkeys; another user's passkey id
  answers 404 `passkey.not_found`; anonymous answers 401; an API key answers 403
  `auth.session_required`.
- **T10.** (B14) Wrong password, unknown email and a user without a password each answer 401
  `auth.credentials_invalid` with identical bodies, and the test's hash-call counter shows one
  argon2id verification in each case.
- **T11.** (B15, B20) With TOTP confirmed, the right password returns `{next: "totp"}` and no
  session; the right code then signs in; the same code again (same step) answers
  `auth.totp_invalid`; a code from two steps earlier answers `auth.totp_invalid` (window per
  Q11).
- **T12.** (B16) A hash stored with weaker parameters is replaced after a successful sign-in;
  the stored string starts with `$argon2id$`; with the concurrency cap at N, N+1 parallel
  checks run at most N at a time.
- **T13.** (B17) Changing the password needs the current one; the old password stops working.
- **T14.** (B18) The eleventh failed password in the window (numbers per
  `08-sessions-and-csrf.md` Q7) answers 429 for that account from a new IP.
- **T15.** (B19) TOTP enrollment returns the secret once; the stored column is not the base32
  secret; an unconfirmed secret does not trigger B15.
- **T16.** (B22) If built: the magic-link request answers 202 for known and unknown emails
  alike; the token works once; it expires per Q13; a GET of the landing URL changes nothing.
- **T17.** (B24) A captured log over T4 to T16 contains no password, code, secret or token.
- **T18.** Four-case test for `/api/v1/admin/local-sign-in` and
  `/api/v1/admin/users/{id}/enrollment-links`: anonymous 401; a user without the permission
  403; wrong scope 404 (the permission held on an account that does not contain the user, if
  09 allows account-scoped user management); an organization admin 2xx.
- **T19.** Cross-organization: organization A's admin creating an enrollment link for
  organization B's user id gets 404; organization B's enrollment token used on organization A
  answers `auth.enrollment_invalid`; organization B's user's email typed into organization A's
  password sign-in answers `auth.credentials_invalid`.
- **T20.** Playwright with Chromium's virtual authenticator: open an enrollment link, register a
  passkey, sign out, sign in with the passkey.
- **T21.** Contract: every route above is in `openapi/stepclimb.json`.

`/api/v1/me/...` routes have no permission to withhold, so their four-case test is three cases
(401 anonymous, 404 for another user's id, 2xx for the owner) plus the API-key 403 of T9.

## Out of scope

- Sending email (enrollment links, magic links, security notices): M1 (platform and
  organization senders, `docs/plan/technical.md` -> Email and notifications).
- Admin creation and editing of user records: `03-users-and-identities.md`.
- The sign-in, enrollment and security-settings pages: `11-app-shell.md` (sign-in page) and a
  later UI issue for the account security page.
- Passkey sign-in inside the Tauri app: it signs in through the system browser (M5).

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Which local methods land in M0.** The M0 milestone names passkeys; the sign-in table
  dates local accounts (all four methods) at 1.0; magic links need email, which is M1. (a)
  Passkeys and enrollment links in M0, password and TOTP in M0 after them, magic link in M1;
  (b) passkeys only in M0, the rest by 1.0; (c) all four in M0, with the magic link unusable
  until M1 email. Recommendation: (a); password and TOTP need no email and give the pilot a
  fallback.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Method switches: storage, endpoint and defaults.** `02-organizations-and-accounts.md`
  B14 names the switches as `OrgSettings` keys owned by this spec (written through `PATCH
  /api/v1/organization/settings` with `perm::OrgManage`), while `09-permissions-and-roles.md`
  B2 lists `perm::ProviderManage` as checked by this spec for them. (a) `OrgSettings` keys
  written through 02's endpoint only, and `/api/v1/admin/local-sign-in` is dropped; (b)
  `OrgSettings` keys written through `/api/v1/admin/local-sign-in` with `ProviderManage`, and
  refused by 02's endpoint; (c) the local row's `identity_providers.config`. Defaults: the plan
  says "optional magic link", which suggests off; it says nothing for the others.
  Recommendation: (b), so one permission covers every sign-in method, with passkey, password
  and TOTP on and magic link off.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Where password hashes, TOTP secrets and one-time tokens are stored.** The data model has
  no column for them. (a) The proposed `local_logins` and `local_tokens` tables in Data; (b)
  columns on `users` (adds secrets to the most-read table); (c) one table per secret kind.
  Also the proposed `passkeys.credential_id` and `last_used_at` columns. Recommendation: (a),
  with both `passkeys` columns.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. The local identity's subject.** (a) The user's `id` as text, stable when the email
  changes; (b) the normalized email, which ties the identity to an address. Recommendation: (a).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Email uniqueness and case.** Sign-in by email needs one account per address per
  organization; `users.email` is optional and its uniqueness is `03-users-and-identities.md`
  Q1. (a) `UNIQUE (org_id, email_normalized)` in `local_logins`, with the address
  lowercased in full; (b) a unique index on `users (org_id, lower(email))`, which also forbids
  an SSO user and a local user from sharing an address; (c) lowercase only the domain part.
  Recommendation: (a); the local part is case-insensitive at nearly every mail provider, and
  (b) is 03's call.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. Enrollment before email.** The plan lists admin invites and self sign-up as ways in, but
  email is M1. (a) M0 enrollment links shown to the admin to hand over, lifetime 7 days, and
  local self sign-up waits for M1 email verification; (b) links with a 24-hour lifetime; (c) M0
  allows local self sign-up without email verification when sign-up is on. Recommendation:
  (a); (c) lets anyone claim any address.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. Bootstrapping the first admin.** A new instance has no one who can create an enrollment
  link. (a) `stepclimb admin create --email <address>` (the plan's CLI) creates the user,
  grants Organization admin and prints an enrollment link to stdout; (b) the first person to
  sign in becomes admin; (c) an admin email in configuration whose first sign-in is granted
  admin. Recommendation: (a); (b) races on a public server. Which spec owns the `admin create`
  command (it also needs `09-permissions-and-roles.md` for the grant) is part of this question.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. Password policy and hashing.** Policy: (a) 12 to 256 characters, no composition rules,
  following NIST SP 800-63B's direction of length over composition; (b) 8 minimum; (c) (a) plus
  a check against a bundled list of common passwords. argon2id parameters: the OWASP Password
  Storage Cheat Sheet's minimum (19 MiB memory, 2 iterations, 1 lane) or more; concurrency cap:
  4 hashes at once (76 MiB peak at the minimum). On password change: end the user's other
  sessions or keep them. Recommendation: (c) with a list of the 10,000 most common passwords,
  the OWASP minimum, a cap of 4, and end other sessions.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q9. Email-first passkey sign-in and account enumeration.** The plan says "email plus passkey
  first". (a) Email first; for an unknown address, return request options with no credentials
  so the response looks like a known user with no passkey; (b) also offer username-less
  sign-in (discoverable credentials, browser autofill), with the WebAuthn user handle set to
  the user's `id`; (c) email first and answer 404 for unknown addresses. Recommendation: (a)
  in M0 and (b) later; (c) tells anyone who has an account.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q10. Recovery and the last method.** The plan does not say how someone who loses their only
  passkey gets back in. (a) An admin creates a new enrollment link (B6) and, when email exists,
  a magic link or emailed recovery link; (b) TOTP recovery codes also allow passkey
  re-enrollment; (c) self-service recovery by email only. Deleting one's last passkey or
  password: refuse with 409 `local.last_method`, or allow. Recommendation: (a), and refuse.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q11. TOTP.** (a) A second factor after the password, optional per user, with an
  organization setting to require it; (b) always required with a password; (c) an alternative
  to the password. Parameters: SHA-1, 6 digits, 30 seconds (what most authenticator apps
  accept), one step of drift either way. Recovery codes: 10 single-use codes shown once at
  confirmation, stored hashed, or none. Disabling: needs a current code, or the password. The
  crate: the plan names none, so the issue runs `dependency-researcher` (a candidate is
  `totp-rs`, unverified). Recommendation: (a), those parameters, 10 recovery codes, and
  disabling with a current code or a recovery code.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q12. WebAuthn policy.** User verification: required (the authenticator checks a PIN or
  biometric, so a passkey alone is two factors) or preferred. Attestation: none (no device
  vetting) or direct. A sign counter that goes backwards (a possible cloned authenticator):
  refuse and flag the passkey, or allow (synced passkeys often report 0). Recommendation:
  required, none, and refuse only when both stored and presented counters are non-zero.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q13. Magic links.** Lifetime: (a) 15 minutes; (b) 1 hour; (c) 10 minutes. Requests per
  address: at most one live link (a new request replaces it) or several. Build it in M0 with a
  test-only mail sink, or in M1 with email. Recommendation: (a), one live link, built in M1.
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **I1. Local provider row and method switches.** Creates the `local` row for each
   organization (and for new organizations), the switch storage of Q2,
   `GET`/`PUT /api/v1/admin/local-sign-in` (pending Q2), `auth.method_disabled`, the local
   entries in `/.well-known/stepclimb`, the migration widening 03's `kind` check. Covers B1 to
   B3; T1 (switch part), T18 (this route), T21. Depends on: `03-users-and-identities.md`
   (`identity_providers`), `02-organizations-and-accounts.md` (`OrgSettings`),
   `09-permissions-and-roles.md`, Q1, Q2.
2. **I2. Enrollment links.** Migration for `local_tokens` (the email part of `local_logins`
   moved to I6, B4's decision), the admin endpoint, `POST /auth/local/enrollment/start` without the passkey
   options, the function Q7's bootstrap calls, `local::has_credential`. Covers B3, B4, B6, B7, B10;
   T2, T3, T18 (this route), T19. Depends on: I1, `08-sessions-and-csrf.md` I4 and I5, Q3 to Q7.
3. **I3. Passkey registration.** Adds `webauthn-rs` (exact pin, at least 14 days old, `just
   deny` clean), the per-organization relying party (ID and origin from
   `08-sessions-and-csrf.md` Q1), migration for `passkeys` with Q3's columns, the enrollment
   passkey ceremony and sign-in completion. Covers B5, B8; T4, T8 (passkey part). Depends on:
   I2, `07-admission.md` I1, Q12.
4. **I4. Passkey sign-in.** Start and finish routes, counter and backup-state update,
   enumeration behavior of Q9. Covers B11, B12; T6, T7. Depends on: I3, Q9.
5. **I5. Passkey management.** `/api/v1/me/passkeys` routes, the session-only rule, the
   last-method rule of Q10, the Playwright passkey test. Covers B13; T9, T20. Depends on: I4,
   `11-app-shell.md` (pages the test drives), Q10.
6. **I6. Password.** Adds `argon2`; `local_logins` with Q5's `email_normalized` and the
   password columns (pending Q3), hashing with the concurrency
   cap, enrollment by password, sign-in with uniform failures, rehash, change password,
   per-account rate limit. Covers B9, B14, B16 to B18; T5, T8 (password part), T10, T12 to T14.
   Depends on: I2, Q1, Q8.
7. **I7. TOTP.** TOTP crate after dependency research, TOTP columns (pending Q3), enrollment and
   confirmation, the second sign-in step with replay refusal, and Q11's recovery codes and
   disabling. Covers B15, B19 to B21; T11, T15, T17. Depends on: I6, `08-sessions-and-csrf.md`
   I1 (encryption), Q11.
8. **I8. Magic link.** Request and finish routes, token lifetime, the landing-page button rule.
   Covers B22; T16. Depends on: I2, the M1 email spec (`docs/specs/m1.md`), Q13. Scheduled per
   Q1.
