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
- **B3.** A local account is a `users` row plus an `identities` row for the local provider. The
  identity's `subject`: pending Q4. The local row needs `kind = 'local'`, which widens the `kind`
  check of `03-users-and-identities.md` B13 by migration. This spec provides
  `local::has_credential(scope, user_id)` (a passkey or a password exists), which
  `03-users-and-identities.md` B19 calls before removing a user's last identity.
- **B4.** Local sign-in finds the user by the organization and the email address. Uniqueness
  and case handling: pending Q5.
- **B5.** Every completed local sign-in goes through admission (`07-admission.md` B1) with the
  local provider, the identity's subject and no mapped attributes, and creates a session
  (`08-sessions-and-csrf.md` B7). A suspended user is refused there.

### Enrollment links

- **B6.** An admin holding the user-management permission creates an enrollment link for a user
  of the organization. The server returns the link (the web app's enrollment route with a
  random token) once, stores only the token's hash, and invalidates any earlier unused
  enrollment link for that user. Link lifetime: pending Q6. Until email exists (M1), the admin
  hands the link over; emailing it is M1.
- **B7.** `POST /auth/local/enrollment/start` with the token answers with the user's email, the
  methods the user may set up (from B1's switches) and, when passkeys are on, WebAuthn
  credential creation options bound to a new sign-in flow (`08-sessions-and-csrf.md` B17). An
  unknown or used token answers 400 `auth.enrollment_invalid`; an expired one 400
  `auth.enrollment_expired`.
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
  create` exists (`02-organizations-and-accounts.md` B27) and creates the admin without a link;
  issue I2 adds the printed enrollment link (coordinator decision, 2026-11-13).

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
| `PUT /api/v1/admin/local-sign-in` | same shape | 200 | `local.magic_link_needs_email` (on before an email sender exists), `permission.denied` | `perm::ProviderManage`, organization |
| `POST /api/v1/admin/users/{id}/enrollment-links` | none | 201 `{url, expires_at}` | `user.not_found`, `permission.denied` | `perm::UserManage`, organization (see Permissions) |
| `POST /auth/local/enrollment/start` | `{token}` | 200 `{email, methods: [...], passkey_options?}`; sets the flow cookie | `auth.enrollment_invalid`, `auth.enrollment_expired` | none |
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
  `created_at`, `updated_at`. Index on `(user_id, kind)` for B6's invalidation.
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
2. **I2. Enrollment links.** Migration for `local_tokens` and the email part of `local_logins`
   (pending Q3, Q5), the admin endpoint, `POST /auth/local/enrollment/start` without the passkey
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
6. **I6. Password.** Adds `argon2`; password columns (pending Q3), hashing with the concurrency
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
