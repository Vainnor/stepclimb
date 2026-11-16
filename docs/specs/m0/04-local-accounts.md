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

  Coordinator decision (2026-11-15), for issue I7: TOTP counts as offered only while the
  password is offered too (`LocalSignIn::offers`), since there is no step for it to follow
  otherwise. With TOTP's switch on and the password's off, `require_method(Totp)` answers 404
  `auth.method_disabled` and `LocalSignIn::offered()` leaves TOTP out, as the discovery document
  of `08-sessions-and-csrf.md` I9 does.
- **B3.** A local account is a `users` row plus an `identities` row for the local provider. The
  identity's `subject`: pending Q4. The local row needs `kind = 'local'`, which widens the `kind`
  check of `03-users-and-identities.md` B13 by migration. This spec provides
  `local::has_credential(scope, user_id)` (a passkey or a password exists), which
  `03-users-and-identities.md` B19 calls before removing a user's last identity.
  `identities::unlink_unless_last` calls it (issue I2). No passkey or password can be stored
  before issues I3 and I6, so until then it answers `false`; each of those issues adds its
  check and a test that the credential lets the user's last outside identity go
  (coordinator decision, 2026-11-14).
  Issue I6 added the password check (a `local_logins` row with a hash) and counts the password
  in `local::delete_passkey_unless_last` too (Q10). Issue I3 added the passkey check: a stored
  passkey counts whether or not the passkey switch is on, since B2 keeps it and switching the method back on restores it (coordinator decision,
  2026-11-14). `stepclimb admin enrollment-link` reads the same function, so a user with a
  passkey needs `--force` there.

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

  Coordinator decisions (2026-11-14), for issue I3:

  - The relying party ID is the public URL's host exactly (`lms.example.org`, not a parent
    domain), the only accepted origin is the public URL's origin (no subdomain, no other port),
    and the relying party's name is the organization's name. A public URL whose host is an IP
    address cannot be a relying party: the passkey routes answer 500 and log why. Changing the
    public URL's host strands every registered passkey (ADR 0026).
  - Q12 as recommended: user verification required, attestation `none`, a resident key not
    required (`residentKey: discouraged`), and the user's existing passkeys in
    `excludeCredentials`. The WebAuthn user handle is the user's id, the user name the email.
  - The start route's flow (kind `passkey_register`, the link's user) seals the ceremony state
    and the link's id. The passkey route takes the flow first and deletes it in its transaction,
    so a challenge is used once whatever the outcome; every later refusal (`auth.enrollment_*`,
    `auth.passkey_invalid`, `auth.suspended`, a flow started for another link or user, which is
    400 `auth.flow_invalid`) rolls back a savepoint, so the link stays usable and nothing is
    stored, and commits the flow's deletion. `auth.flow_invalid` and `auth.flow_expired` are
    400 here, like the route's other refusals. Every answer after the flow is taken clears the
    flow cookie.
  - The route locks the flow's user (`identities::lock_user`, which refuses any isolation level
    but `READ COMMITTED`) before `consume_enrollment` locks the link, the order
    `create_enrollment` takes them in, so a registration and a new link for the same user cannot
    deadlock; two registrations with one link take turns and the second answers
    `auth.enrollment_invalid`.
  - `name` is optional, trimmed, 1 to 200 characters without NUL (the shared name rule), and
    `Passkey` when absent. A bad name answers 422 `request.invalid` before the flow is taken.
  - A credential id the organization already has answers `auth.passkey_invalid`.
  - Admission (B5): after storing the passkey and linking the local identity, the route
    completes the sign-in through `07-admission.md`'s local entry (`admission::complete_local`,
    issue I1 there), keyed by the link's user. Step 1 refuses a suspended user with 403
    `auth.suspended`, and the route rolls its savepoint back, so the passkey, the link's use
    and the local identity are not stored.
  - The route writes `passkey.create` (`09-permissions-and-roles.md` B22) with the user and the
    name, and `identity.link` when it creates the local identity, both by the user from the
    client address.
  - B24: `webauthn-rs` logs a ceremony's state, its challenge included, at `trace`. The server's
    log subscriber (`logging::subscriber`) keeps events from the `webauthn_rs` crates at `info`
    and above whatever the `log` setting says.
- **B9.** `POST /auth/local/enrollment/password`, when password sign-in is on, sets the user's
  password from the token instead (policy pending Q8), marks the token used, creates the local
  identity if absent, and completes sign-in.

  Coordinator decisions (2026-11-15), for issue I6:

  - The route takes `RateLimited<SignIn>` first, answers 404 `auth.method_disabled` while
    passwords are off, and checks Q8's policy before it reads the link: a weak password is 422
    `request.invalid` with `local.password_weak` at `/password`, and the link stays usable.
  - It reads the link (`check_enrollment`), hashes the password outside any transaction, then in
    one transaction locks the link's user before `consume_enrollment` locks the link (the order
    `create_enrollment` takes), stores the password, links the local identity if absent and
    completes the sign-in through `admission::complete_local`. A refusal (an
    `auth.enrollment_*` code, `auth.suspended`) rolls everything back, so the link stays usable
    and nothing is stored. The answer is 200 `{return_to: "/"}` with the session and device
    cookies and `Cache-Control: no-store`.
  - A user who has a password already gets the new one: an enrollment link is Q10 (a)'s way back
    in. Such a reset ends every other session of the user in the enrollment's transaction,
    keeping only the session it creates, and moves the password's generation, which revokes the
    user's older device cookies (`08-sessions-and-csrf.md` B42).
  - Storing a password writes `password.set` (`09-permissions-and-roles.md` B22) with the user,
    never the hash, by the user from the client address; a change through B17 writes it too.
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

  Coordinator decisions (2026-11-14), for issue I4:

  - Q9 (a) as decided. The start route finds the user with `users::find_by_email` (the
    organization and the email in any letter case, `users_email`'s comparison) and always
    answers 200 `{options}` with a new `passkey_sign_in` flow. An address with no account,
    another organization's, one that holds NUL, and an account without a passkey all get
    `allowCredentials: []`, after the same queries (the credentials query runs with the nil id
    when there is no user), and their flow can finish no sign-in. An account with passkeys
    still differs from these, since its options list its credential ids: pending Q14.
    Discoverable sign-in (Q9 (b)) and fake credential ids are not built in M0 (owner, 14 Nov
    2026).
  - Both routes take `RateLimited<SignIn>` first, then answer 404 `auth.method_disabled` while
    passkeys are off, before they read the body, so a switched-off method answers 404 whatever
    the body. Both answer `Cache-Control: no-store`.
  - The finish route takes the flow first and deletes it in its transaction, so a challenge is
    used once whatever the outcome. In a savepoint it then finds the passkey by `(org_id,
    credential_id)` with `FOR UPDATE` (for a flow with no account too), which must belong to
    the flow's user, and verifies the
    assertion against the flow's state (challenge, the public URL's origin exactly, the
    relying party ID, user verification required, the signature) and against the stored
    credential. A user handle, when the authenticator sends one, must be the flow user's id.
    A flow for no account, an unknown credential, another user's passkey, a bad signature, a
    user handle mismatch, a counter that did not go up and a body that is not an assertion all
    answer the same 401 `auth.passkey_invalid`. `auth.flow_invalid` and `auth.flow_expired` are
    400, as in B8. A refusal rolls the savepoint back and commits the flow's deletion; every
    answer after the flow is taken clears the flow cookie. A server fault (500) rolls the whole
    transaction back and leaves the flow usable; nothing a client sends reaches that path.
  - Q12's counter rule: a counter must go up unless both the stored and the presented counter
    are 0. This is the W3C rule `webauthn-rs` applies, and it is stricter than Q12's wording in
    one case: a stored counter above 0 with a presented 0 is refused. `webauthn-rs` cannot
    apply the literal rule, and refusing fails closed; whether to keep it is pending Q15. The
    check runs against the row read under the lock, not only the ceremony's copy, so two
    sign-ins cannot pass with one counter value.
  - "Refuse and flag" (Q12): a counter refusal logs a warning with the passkey's id and sets
    `passkeys.counter_regressed_at` (migration `20261114200000_passkey_counter_flag.sql`) to the
    first time it happened, in the outer transaction after the savepoint's rollback, so the
    flag commits although the sign-in is refused. The flag blocks nothing: a flagged passkey
    whose counter goes up again still signs in. Whether a flagged passkey is blocked is pending
    Q15.
  - Admission (B5): as in B8, the route completes the sign-in through `07-admission.md`'s
    local entry, keyed by the passkey's user, only after the assertion verified, so a refusal
    (403 `auth.suspended`) reveals the suspension only to the passkey's holder. Nothing is
    saved for a refused sign-in. The route links no identity: a user who signs in through an
    outside provider and added a passkey while signed in has no local identity, and signing in
    with that passkey does not give them one, so a rule that gates local accounts by their
    local identity keeps applying to them (coordinator decision, 2026-11-15). Only the
    enrollment route (B8) creates a local identity.
  - On success the route saves the credential with its new counter and backup state, sets
    `last_used_at` and `updated_at`, and creates the session with `session::start_on`, which
    ends the sessions the browser's old cookies name (`08-sessions-and-csrf.md` B7).
  - A sign-in writes no audit entry: `09-permissions-and-roles.md` B22 lists none, and
    `08-sessions-and-csrf.md` Q14 keeps routine sign-ins to the log. The route logs the user
    and passkey ids at `info`, never the email, the assertion, the flow token or the state.

### Passkey management

- **B13.** A signed-in user lists, adds, renames and deletes their own passkeys under
  `/api/v1/me/passkeys`. Adding uses the same ceremony as B8 with the session's user instead of
  a token. A passkey id that is not the caller's answers 404. Whether deleting the user's last
  way to sign in is allowed: pending Q10.

  Decided by the owner (15 Nov 2026): whether a user who signs in through an outside provider
  (OpenID Connect, OAuth 2, the VATSIM preset) may add a passkey is a setting of each provider,
  off by default. A user with no identity at the local provider (the local account only an
  enrollment link creates) whose providers all have it off is refused `POST
  /api/v1/me/passkeys/start` and `POST /api/v1/me/passkeys` with 403
  `local.passkeys_not_allowed`. A user with a local account is unaffected. The setting is the
  provider config's `allow_local_passkeys` member, a boolean, `false` when absent
  (`05-oidc.md` B1, `06-oauth2-and-vatsim-preset.md` B4).

  Coordinator decisions (2026-11-15), for issue I5:

  - Only an identity at an enabled provider counts, as in `03-users-and-identities.md` B19:
    a disabled provider that allows passkeys lets no one add one. The member must be exactly
    `true`; anything else is off. A user with no identity at all is refused. Both routes apply
    the rule, the add again under the user's row lock (`identities::lock_user`), so a provider
    switched off between the two requests refuses the add.
  - Adding a passkey links no local identity, so the provider setting stays the gate for
    every later passkey too. Passkey sign-in applies the same rule (Q16, the owner's
    decision).
  - The session-only rule: every `/api/v1/me/passkeys` route answers 403
    `auth.session_required` to a request carrying `Authorization: Bearer`, valid or not, before
    it reads anything else. A bearer request is authenticated by its bearer credential alone
    (`08-sessions-and-csrf.md` B16), so this refuses API keys and app access tokens alike, now
    and once they exist. No credential answers 401.
  - Every route, list and delete included, answers 404 `auth.method_disabled` while passkeys
    are off, as the API section says of every route of a method. The stored passkeys stay (B2).
  - The start route's `passkey_register` flow names the session's user and holds only the
    ceremony state. The add refuses a flow that holds an enrollment link, or one started for
    another user or in another organization, with 400 `auth.flow_invalid`, and the enrollment
    route refuses a flow that holds no link the same way. Every refusal after the flow is taken
    commits its deletion and clears the flow cookie, as in B8. The WebAuthn user name is the
    user's email, or their display name when they have none.
  - Q10 as recommended: deleting a passkey is refused with 409 `local.last_method` when the
    user keeps no other passkey, no password and no identity at an enabled provider that is not
    local. `local::delete_passkey_unless_last` locks the user's row first, as
    `identities::unlink_unless_last` does, so two removals take turns. Issue I6 adds the
    password to this count. A known gap: the count reads each provider's `enabled` without a
    lock on the provider's row, so a provider disabled in a transaction that commits while the
    deletion runs can leave the user with no enabled way to sign in, as it can for B19's
    identity removal. Recovery is an admin's new enrollment link (Q10 (a)).
  - The list, the add and the rename answer one shape, `{id, name, created_at, last_used_at}`,
    with `last_used_at` `null` before the first sign-in. A rename to the stored name (after
    trimming) writes nothing (`09-permissions-and-roles.md` B22). The routes write
    `passkey.create`, `passkey.rename` (the name before and after) and `passkey.delete` (the
    user and the name) by the user from the client address.
  - The routes are not rate limited: they need a session, and B23 covers `/auth/local/...`.

### Password

- **B14.** `POST /auth/local/password` with email and password checks the password against the
  stored argon2id hash. A wrong password, an unknown email, a user with no password, or
  password sign-in switched off for the user's organization all answer 401
  `auth.credentials_invalid`, and the server runs one argon2id verification in every case so
  the response time does not reveal which (enumeration handling as a whole: Q9).

  Coordinator decisions (2026-11-15), for issue I6:

  - Password sign-in switched off answers 404 `auth.method_disabled` before the body is read,
    as B2 and the API section say of every method route and as passkey sign-in does; the
    request's organization is the user's, so the 401 case above cannot arise.
  - The account is found through `local_logins.email_normalized` (Q5 (a)), so another
    organization's address, an address holding NUL and a user without a password are the
    "unknown email" case. Those verify the password against a fixed argon2id hash at Q8's
    parameters (`stepclimb_auth::password::verify_dummy`), so every failure costs one
    verification; the 401 bodies are identical.
  - After a right password the route locks the user and re-reads the hash; a hash changed since
    it was read (a concurrent change or enrollment) answers the same 401, so a sign-in cannot
    outlive the password change that ended the other sessions. Admission (B5) then refuses a
    suspended user with 403 `auth.suspended`. The route links no identity.
  - `return_to` is sanitized as a flow's is (`08-sessions-and-csrf.md` B19). A sign-in writes no
    audit entry (`08-sessions-and-csrf.md` Q14); it logs the user id at `info`.
- **B15.** When the password is right and the user has confirmed TOTP, the response is 200
  `{next: "totp"}` with a sign-in flow, and no session exists yet. Otherwise sign-in completes
  (B5). Whether TOTP is required with a password: pending Q11.

  Coordinator decisions (2026-11-15), for issue I7:

  - Q11 (a) as decided, without its organization setting: TOTP is optional per user. The
    setting that would require it of every password user needs a way to enroll during sign-in,
    which issue I7 does not list; it is left for a later issue (I9, below).
  - The step runs in the sign-in transaction after the user's lock, the "password changed"
    check and the rehash: when the organization offers TOTP (B2's rule above) and the user's
    secret is confirmed, the route starts a `password_totp` flow (`08-sessions-and-csrf.md` B17)
    naming the user, sealing the password's generation and keeping the request's `return_to`,
    and answers 200 `{next: "totp"}` with the flow cookie and `Cache-Control: no-store`, without a
    session or a device cookie. The 200 body is `PasswordSignedIn` in the OpenAPI document, one
    of `SignedIn` and `CodeRequired`.
  - While TOTP is switched off, a confirmed secret asks for nothing and the password signs in
    alone; switching TOTP back on asks for the same secret again (B2: credentials of a
    switched-off method stop working and come back unchanged). The admin who switches it off
    must hold every catalog permission (B2's escalation rule).

  Coordinator decisions (2026-11-15), for issue I9 (#192), Q11 (a)'s organization setting:

  - The setting is `totp_required`, a fifth member of the `local_sign_in` settings object
    (default `false`), read and written with the switches through `/api/v1/admin/local-sign-in`,
    whose `PUT` now names six members. Changing it is a change of the switches: the same
    escalation rule (every catalog permission at organization scope), the same
    `organization.update_settings` audit entry. `local_sign_in::update` refuses it on while the
    password or TOTP is switched off: 422 `request.invalid` with `local.totp_required_needs_totp`
    at `/totp_required`, writing nothing. Turning either off while it is on is the same refusal,
    so an admin turns the requirement off first.
  - What follows a right password (`totp::next_after_password`): a user with confirmed TOTP gives
    the code (`{next: "totp"}`) while TOTP is offered or required; a user without it, while TOTP
    is required, sets it up (`{next: "totp_enroll"}`); otherwise the password signs in. Required
    means `totp_required` and the password offered (`LocalSignIn::requires_totp`), whatever TOTP's
    own switch says, so a stored state the route refuses to write (required with TOTP off) asks
    for a step whose route answers 404 and the password alone never signs in: fail closed.
  - Set-up during sign-in: the password route, in its sign-in transaction, stores a new
    unconfirmed secret (as `POST /api/v1/me/totp` does, replacing an unconfirmed one) and starts
    a `password_totp_enroll` flow (a new `auth_flows.kind`, migration
    `20261116120000_totp_enroll_flow.sql`) naming the user, the password's generation and the
    SHA-256 of the sealed secret it stored, which binds the flow to that secret. It
    answers 200 `{next: "totp_enroll", otpauth_uri, secret}` (`EnrollmentRequired`, a third member
    of `PasswordSignedIn`) with the flow cookie and `no-store`, without a session or a device
    cookie. The secret is in this answer because no session exists to fetch it with.
  - `POST /auth/local/totp/enrollment` with `{code}` takes `RateLimited<SignIn>` first, answers
    404 `auth.method_disabled` unless TOTP is offered, takes only a `password_totp_enroll` flow (so
    neither step's flow stands in for the other's), and in a savepoint locks the user and login.
    A flow whose password generation changed, whose user confirmed TOTP meanwhile, or whose
    waiting secret is gone or was replaced without being confirmed (by a second password sign-in's
    set-up or by `POST /api/v1/me/totp` from an earlier session) answers 400 `auth.flow_invalid`:
    no code for the secret it showed can work any more, so the user starts again rather than
    retrying (from the security review of #205). The code counts against B18's budgets
    and is checked against the waiting secret (no recovery code: none exist yet); a right code
    confirms TOTP with ten new recovery codes (`totp.enable` with the user as actor), completes the
    sign-in through admission (B5), and answers 200 `{return_to, recovery_codes}` with the session,
    a device cookie, the flow cookie cleared and `no-store`. The body names no user: the flow can
    set TOTP only for the user whose password it followed.
  - A wrong set-up code answers 401 `auth.totp_invalid` and rolls everything back, so the flow
    stays usable until its 10-minute expiry; the failure stays counted against the budgets. Unlike
    B20's code step, the person has already proved the password and the code only shows their app
    holds the secret they were just shown, so a typo must not cost them a new secret to scan. A
    spent budget (429) and a fault roll back too; an admission refusal (403) commits only the
    flow's deletion, so TOTP stays unset.
  - Users who signed in before the requirement was turned on keep their sessions until those end;
    they set TOTP up at their next password sign-in. The spec asks for no session to end, and
    ending every password user's sessions at once is a larger change than the setting.
  - Turning the requirement off removes no TOTP: confirmed TOTP keeps asking for the code.
  - The requirement is about password sign-in only. Passkey sign-in, provider sign-in and the
    sign-in an enrollment link completes (B19's decision) are unchanged; the next password
    sign-in after an enrollment link sets TOTP up.
  - A user may still turn their own TOTP off with a code (B21) while it is required; their next
    password sign-in sets it up again.
  - For administrators (from the security review of #205): with `totp_required` on, whoever
    knows an account's password first sets up its TOTP, so turn it on only for accounts whose
    passwords are the users' own. The recovery runbook for an account set up by someone else, or
    a user who lost both the app and the recovery codes, is the administrator's removal of the
    user's TOTP (B21's I9 decision, which also ends the user's sessions) and then an enrollment
    link, which resets the password; the user's next password sign-in sets TOTP up again.
    Enforcement lags for sessions that existed before the setting was turned on: they last until
    they end, at most the 30-day session limit (`08-sessions-and-csrf.md` B8).
- **B16.** Passwords are hashed with argon2id using the parameters of Q8, with a per-hash random
  salt, in the PHC string format. When a stored hash uses older parameters, a successful sign-in
  rehashes it. The number of hashes computed at once is capped (pending Q8) so sign-in bursts
  cannot exhaust memory on the 1 GB budget (`docs/plan/technical.md` -> Hosting and
  operations).

  Coordinator decisions (2026-11-15), for issue I6:

  - Parameters: argon2id, version 0x13, 19,456 KiB, 2 passes, 1 lane, a 32-byte output and a
    16-byte salt from the operating system's CSPRNG (Q8, the OWASP minimum). "Older parameters"
    means any stored string that does not name exactly these; the rehash runs only after a right
    password, and replaces the hash only if it is still the one that was verified.
  - The cap is 4 (Q8), a semaphore around tokio's blocking pool
    (`stepclimb_server::password_hasher`): a request waits for a permit rather than being
    refused. Every hash and verification, the dummy one included, takes a permit, and the
    permit moves into the blocking task, so a request dropped mid-hash keeps it until the hash
    ends and no more than 4 ever run.
  - Q8's policy (c): 12 to 256 characters counted as Unicode scalar values, no composition rule,
    and not in a list of 10,000 passwords compared in lowercase. Of SecLists' "10k most common"
    list only 10 entries have 12 or more characters, so that list alone would refuse 10
    passwords. The bundled list is those 10, then entries of 12 to 256 characters drawn from
    SecLists' xato-net one-million list in that file's order, after filters that drop every
    entry with `@`, a web or mail domain, or a run of 10 or more digits that is not a sequence
    or repetition (phone-number-like), entries of letters followed by a plausible birth date,
    and entries matching a short list of offensive substrings; the 10 from the literal list are
    kept whatever the filters say (five of them are offensive), so the server refuses at least
    what Q8's literal list would (coordinator decision, 2026-11-15; source and filters in
    `crates/stepclimb-auth/THIRD_PARTY.md`, MIT). Passwords are not Unicode-normalized before
    hashing.
- **B17.** A signed-in user changes their password with `PUT /api/v1/me/password`, giving the
  current one. A wrong current password answers 401 `auth.credentials_invalid`. Whether other
  sessions end: pending Q8.

  Coordinator decisions (2026-11-15), for issue I6: the route takes a browser session only (403
  `auth.session_required` to a bearer request, as `/api/v1/me/passkeys` does), answers 404
  `auth.method_disabled` while passwords are off, refuses a weak new password with 422
  `local.password_weak` at `/new_password`, and checks the current password exactly as sign-in
  does, the budgets of B18 included. A user without a password answers 401
  `auth.credentials_invalid` after the dummy verification. On success, under the user's row
  lock, it re-reads the hash and answers the same 401 if it is no longer the one verified (a
  change or reset in between); otherwise it stores the new hash (`password.set`) and ends every
  other session of the user (Q8), keeping the session that asked; the answer is 204.
- **B18.** Failed password checks are rate limited per client IP and per account
  (`08-sessions-and-csrf.md` B22, B23).

  Coordinator decisions (2026-11-15), for issue I6: the account identifier is
  `stepclimb_core::local_logins::LoginEmail`, the address trimmed and folded by the database's
  `lower()`, which `local_logins.email_normalized` and the `users_email` index use too, so the
  match and the budget fold the same way whatever the locale (`08-sessions-and-csrf.md` B41). An
  unknown address has a budget of its own. The device cookie of `08-sessions-and-csrf.md` B42
  replaces the account's budget by the cookie's own.

  Coordinator decisions (2026-11-15), from the security review of I6: `PUT
  /api/v1/me/password` takes `RateLimited<SignIn>` first, like the `/auth/` routes, and its
  check counts against a budget of the user (by id, 10 failures per 15 minutes) besides the
  account's or the device cookie's, so a user without an email or with a device cookie is
  never unlimited. `password_check::check_password` takes the request's `RateLimited<SignIn>`
  as an argument, so a route cannot check a password without the per-address budget, and the
  router walk of `08-sessions-and-csrf.md` B41 covers every operation that documents a 429 as
  well as every `/auth/` and `/oauth/` route.

### TOTP

- **B19.** A signed-in user with a password starts TOTP enrollment with `POST /api/v1/me/totp`.
  The server generates a secret, stores it encrypted (`08-sessions-and-csrf.md` B3) as
  unconfirmed, and returns an `otpauth://` URI and the secret once. `POST
  /api/v1/me/totp/confirm` with a valid code confirms it. Starting again before confirming
  replaces the unconfirmed secret.

  Coordinator decisions (2026-11-15), for issue I7:

  - The secret is 20 bytes (160 bits) from the operating system's CSPRNG, answered in unpadded
    base32 and in `otpauth://totp/<issuer>:<account>?secret=...&issuer=...&algorithm=SHA1&digits=6&period=30`,
    where the issuer is the organization's name and the account the user's email (their display
    name without one), percent-encoded. It is sealed with the key ring bound to
    `[org_id, "local_logins", user_id, "totp_secret"]`.
  - The three `/api/v1/me/totp` routes take a browser session only (403 `auth.session_required`
    to a bearer request, as B13 and B17 do) and answer 404 `auth.method_disabled` unless TOTP is
    offered. A user without a password answers 409 `local.password_required`. Once TOTP is
    confirmed, starting again answers 409 `local.totp_enabled`: replacing a working secret needs
    a code, so the user disables it first.
  - Confirming checks the code against the waiting secret as sign-in does (B20: the window, the
    replay step, the budgets of the user and of the account or device cookie, as B17's check
    does) and answers 200 `{recovery_codes}` (Q11's ten codes, below) with `Cache-Control:
    no-store`, not 204. No waiting secret answers 409 `local.totp_not_started`; a wrong code 401
    `auth.totp_invalid` with nothing changed. Confirming writes `totp.enable`
    (`09-permissions-and-roles.md` B22). Starting writes no entry: an unconfirmed secret signs
    no one in.
  - A password reset through an enrollment link (B9) keeps the user's TOTP: the link sets the
    password, and its sign-in completes without a code, as every enrollment link's does. A user
    who lost both the app and the recovery codes gets back in through a new enrollment link,
    which can also register a passkey (Q10 (a)); their TOTP stays on until they disable it.
- **B20.** `POST /auth/local/totp` with a code and the flow from B15 completes sign-in when the
  code is valid for the current time step, within the window of Q11, read from the
  controllable clock. A code already accepted for that user is refused when presented again,
  as RFC 6238 section 5.2 requires. A wrong code answers 401 `auth.totp_invalid` and counts
  against the account's rate limit (B18).

  Coordinator decisions (2026-11-15), for issue I7:

  - Q11's parameters: HMAC-SHA-1, 6 digits, a 30-second step, and one step either way of the
    controllable clock's current one. `local_logins.totp_last_step` holds the step of the last
    code accepted for the user (a sign-in or the confirmation), and a code is accepted only for
    a later step, so neither the same code nor an older one of the window passes again. The
    code is compared in constant time against every step of the window. The algorithm is
    `stepclimb_auth::totp` over `hmac` and `sha1`, checked against RFC 4226 Appendix D and RFC
    6238 Appendix B; no TOTP crate.
  - The route takes `RateLimited<SignIn>` first, answers 404 `auth.method_disabled` unless TOTP
    is offered, then takes the `password_totp` flow and, in a savepoint, locks the flow's user
    and login. A flow whose password generation is no longer the user's, or whose user's TOTP
    is no longer confirmed (a change, a reset or disabling in between), answers 400
    `auth.flow_invalid`. The check counts against the budgets of B18: the account's, or the
    device cookie's in its place (`08-sessions-and-csrf.md` B42). Admission (B5) then refuses
    a suspended user with 403 `auth.suspended`. On success the answer is 200 `{return_to}` with
    the session and a device cookie, the flow cookie cleared and `Cache-Control: no-store`.
  - The flow is used once whatever the outcome, as a passkey sign-in's is (B12): a wrong code
    commits the flow's deletion and clears its cookie, so every further guess needs the password
    again. Only a spent budget (429) and a server fault roll it back and leave the flow usable.
  - A recovery code (B21) stands in for a code here, in any letter case, with hyphens and spaces
    ignored, once.
  - A sign-in writes no audit entry (`08-sessions-and-csrf.md` Q14); the route logs the user id
    at `info`, and a sign-in with a recovery code says so, never the code.
- **B21.** Disabling TOTP, recovery codes, and the parameters (algorithm, digits, period):
  pending Q11.

  Coordinator decisions (2026-11-15), for issue I7, Q11 as decided:

  - Recovery codes: ten single-use codes made at confirmation and answered once, each 10 random
    bytes (80 bits) in base32, shown as `ABCD-EFGH-IJKL-MNOP`. The server stores the SHA-256 of
    the 16 characters (`totp_recovery_codes.code_hash`); 80 random bits need no slow hash. A
    used code keeps its row with `used_at`. M0 has no route to make new ones: disabling and
    enabling again does.
  - `POST /api/v1/me/totp/disable` with `{code}` (a current code or an unused recovery code)
    removes the secret, the replay step and every recovery code and writes `totp.disable`; the
    answer is 204. It takes `RateLimited<SignIn>`, a browser session and the budgets of B17's
    check. TOTP that is not confirmed answers 409 `local.totp_not_enabled`; a wrong code 401
    `auth.totp_invalid`. A `POST` with a body rather than `DELETE`, since the request carries a
    code.

  Coordinator decisions (2026-11-15), for issue I9 (#192), from the security review of I7: an
  administrator removes another user's TOTP with `DELETE /api/v1/admin/users/{user_id}/totp`, for a
  user who lost both the app and the recovery codes. It needs `user.manage` at organization scope
  and the user routes' escalation rule (`03-users-and-identities.md` B6 as the owner decided it:
  403 `permission.escalation` when the user holds a permission the caller does not hold at
  organization scope), and, as every admin user-write route, refuses an API key acting on its own
  owner with 403 `auth.session_required` (`08-sessions-and-csrf.md` B44). It removes the secret,
  the replay step and every recovery code (`local_totp::disable`) and writes `totp.disable` with
  the administrator as actor; the answer is 204, with 409 `local.totp_not_enabled` when the user
  has no confirmed TOTP. The caller's own TOTP is refused with 409 `local.cannot_remove_own_totp`:
  it goes through B21's route with a code, so a stolen admin session cannot drop its own second
  factor without one. The route does not check that TOTP is offered, so TOTP can be cleaned up
  while it is switched off. In the same transaction it ends every session of the user
  (`sessions::delete_others_of_user` keeping none) and every app sign-in of the user with the
  user's unused authorization codes (`oauth_refresh_tokens::revoke_all_of_user`,
  `08-sessions-and-csrf.md` B46), so a session that passed the removed second factor, or a
  thief's, does not outlive it (from the security review of #205).

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
| `PUT /api/v1/admin/local-sign-in` | same shape, with `totp_required` (B15's I9 decisions) | 200 | `local.magic_link_needs_email` (on before an email sender exists), `local.totp_required_needs_totp` (422), `local.last_method` (409, B2), `permission.denied`, `permission.escalation` | `perm::ProviderManage`, organization |
| `POST /api/v1/admin/users/{user_id}/enrollment-links` | none | 201 `{url, expires_at}` | `user.not_found`, `local.email_required` (409), `permission.denied`, `permission.escalation` | `perm::UserManage`, organization (see Permissions) |
| `POST /auth/local/enrollment/start` | `{token}` | 200 `{email, methods: [...], passkey_options?}`; with `passkey_options`, sets the flow cookie | `auth.enrollment_invalid`, `auth.enrollment_expired` | none |
| `POST /auth/local/enrollment/passkey` | `{token, credential, name?}` | 200 `{return_to}`; sets `__Host-sc_session` | `auth.enrollment_invalid`, `auth.enrollment_expired`, `auth.flow_invalid`, `auth.flow_expired`, `auth.passkey_invalid` (all 400), `auth.suspended` (403), `request.invalid` (422, `name`) | none |
| `POST /auth/local/enrollment/password` | `{token, password}` | 200 `{return_to}`; sets the session | `auth.enrollment_invalid`, `auth.enrollment_expired`, `local.password_weak` | none |
| `POST /auth/local/passkey/start` | `{email, return_to?}` | 200 `{options}`; sets the flow cookie | none beyond the common ones (Q9 (a): every address gets 200) | none |
| `POST /auth/local/passkey/finish` | `{credential}` | 200 `{return_to}`; sets the session | `auth.flow_invalid`, `auth.flow_expired` (400), `auth.passkey_invalid` (401), `auth.suspended`, `local.passkeys_not_allowed` (403, Q16) | none |
| `POST /auth/local/password` | `{email, password, return_to?}` | 200 `{return_to}` with the session, or 200 `{next: "totp"}` or `{next: "totp_enroll", otpauth_uri, secret}` with the flow cookie | `auth.credentials_invalid` | none |
| `POST /auth/local/totp` | `{code}` (a code or a recovery code) | 200 `{return_to}`; sets the session | `auth.flow_invalid`, `auth.flow_expired` (400), `auth.totp_invalid` (401), `auth.suspended` (403) | none |
| `POST /auth/local/totp/enrollment` | `{code}` | 200 `{return_to, recovery_codes}`; sets the session | `auth.flow_invalid`, `auth.flow_expired` (400), `auth.totp_invalid` (401, the set-up still waits), `auth.suspended` (403) | none |
| `POST /auth/local/magic-link` | `{email, return_to?}` | 202 | none beyond the common ones | none |
| `POST /auth/local/magic-link/finish` | `{token}` | 200 `{return_to}`; sets the session | `auth.magic_link_invalid` | none |
| `GET /api/v1/me/passkeys` | none | 200 `[{id, name, created_at, last_used_at}]` | `auth.session_required` (403) | signed in |
| `POST /api/v1/me/passkeys/start` | none | 200 `{options}`; flow cookie | `auth.session_required`, `local.passkeys_not_allowed` (403, B13) | signed in |
| `POST /api/v1/me/passkeys` | `{credential, name?}` | 201 `{id, name, created_at, last_used_at}` | `auth.flow_invalid`, `auth.flow_expired`, `auth.passkey_invalid` (400), `auth.session_required`, `local.passkeys_not_allowed` (403), `request.invalid` (422, `name`) | signed in |
| `PATCH /api/v1/me/passkeys/{passkey_id}` | `{name}` | 200 `{id, name, created_at, last_used_at}` | `passkey.not_found`, `auth.session_required` (403), `request.invalid` (422) | signed in |
| `DELETE /api/v1/me/passkeys/{passkey_id}` | none | 204 | `passkey.not_found`, `local.last_method` (409, Q10), `auth.session_required` (403) | signed in |
| `PUT /api/v1/me/password` | `{current_password, new_password}` | 204 | `auth.credentials_invalid`, `local.password_weak` | signed in |
| `POST /api/v1/me/totp` | none | 200 `{otpauth_uri, secret}` | `local.password_required`, `local.totp_enabled` (409), `auth.session_required` (403) | signed in |
| `POST /api/v1/me/totp/confirm` | `{code}` | 200 `{recovery_codes}` (B19's decision) | `auth.totp_invalid` (401), `local.totp_not_started`, `local.totp_enabled` (409), `auth.session_required` (403) | signed in |
| `POST /api/v1/me/totp/disable` | `{code}` (a code or a recovery code) | 204 | `auth.totp_invalid` (401), `local.totp_not_enabled` (409), `auth.session_required` (403) | signed in |
| `DELETE /api/v1/admin/users/{user_id}/totp` | none | 204 | `user.not_found` (404), `local.totp_not_enabled`, `local.cannot_remove_own_totp` (409), `permission.denied`, `permission.escalation`, `auth.session_required` (403) | `perm::UserManage`, organization |

The magic-link routes are built only if Q1 and Q13 put them in M0. Issue I7 added the disabling
route (B21); recovery codes are answered by the confirmation and used by the sign-in step and the
disabling route, with no route of their own.

## Data

Keys are UUID v7 generated by the server; every table has `created_at` and `updated_at`.
Migrations are forward-only, one file per issue.

**`passkeys`** (plan columns): `id uuid PK`, `user_id uuid NOT NULL REFERENCES users(id) ON
DELETE CASCADE`, `credential jsonb NOT NULL` (the credential `webauthn-rs` serializes, including
its public key and counter; no secret), `name text NOT NULL`, `created_at`, `updated_at`.
Proposed, pending Q3: `credential_id bytea NOT NULL UNIQUE` (so one authenticator cannot be
registered twice, and for discoverable sign-in if Q9 adds it) and `last_used_at timestamptz
NULL`. Organization: derived through `user_id` -> `users.org_id`. Built by issue I3
(`migrations/20261114180000_passkeys.sql`) with both columns and two changes: an `org_id` column
with the composite foreign key `(org_id, user_id) REFERENCES users (org_id, id) ON DELETE
CASCADE`, as every table but `organizations` has, and `UNIQUE (org_id, credential_id)` in place
of a unique `credential_id`, so one organization's registrations never collide with, or reveal,
another's (coordinator decision, 2026-11-14). `credential_id` is 1 to 1023 bytes (WebAuthn
Level 3's limit). Issue I4 adds `counter_regressed_at timestamptz NULL`, the first time a
sign-in was refused for its sign counter (B12, Q15).

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
  Built by issue I6 (`migrations/20261115120000_local_logins.sql`, coordinator decision
  2026-11-15) with the password columns only: the composite foreign key `(org_id, user_id)
  REFERENCES users (org_id, id) ON DELETE CASCADE`, a check that `password_hash` starts with
  `$argon2id$`, and one that `password_changed_at` is set exactly when `password_hash` is.
  `password_generation bigint NOT NULL DEFAULT 1` counts the passwords set (a rehash keeps it),
  for the device cookies of `08-sessions-and-csrf.md` B42.
  `email_normalized` is `lower(users.email)`, written on insert and kept equal by an `AFTER
  UPDATE OF email` trigger on `users`, so every writer of the email keeps it, and is NULL while
  the user has no email: the password stays stored and works again once an email is set.
  `users_email` already keeps `lower(email)` unique per organization, so the unique pair can
  never refuse a row. The TOTP columns arrive with issue I7.
  Built by issue I7 (`migrations/20261115180000_totp.sql`): `totp_secret`, `totp_confirmed_at`
  and `totp_last_step` as proposed, with a check that a secret needs a password and one that a
  confirmation needs a secret.
- Recovery codes, if Q11 adds them: `totp_recovery_codes(id, user_id, code_hash, used_at,
  created_at, updated_at)`, organization derived through `user_id`. Built by issue I7 with an
  `org_id` column and the composite foreign key `(org_id, user_id) REFERENCES users (org_id, id)
  ON DELETE CASCADE`, as every table but `organizations` has, `code_hash` checked to 32 bytes,
  and `UNIQUE (org_id, user_id, code_hash)` (coordinator decision, 2026-11-15).

## Permissions

| Entry | Scope | Built-in roles |
| --- | --- | --- |
| `perm::ProviderManage` (`09-permissions-and-roles.md` B2, which lists 04 as a checker): change the local method switches, pending Q2 | organization | Organization admin |
| `perm::UserManage` (`09-permissions-and-roles.md` B2): create enrollment links, remove a user's TOTP | organization (account scope only if `09-permissions-and-roles.md` Q2 widens it) | Organization admin |

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
- **T22.** (B15, B21, issue I9) With TOTP required, a right password of a user without TOTP
  answers `{next: "totp_enroll"}` with no session; a wrong set-up code keeps the set-up waiting;
  the current code signs in once with ten recovery codes; the set-up's flow and the code step's
  are not interchangeable; a changed password or TOTP confirmed meanwhile ends the set-up; users
  signed in before keep their sessions and set TOTP up at their next sign-in; turning the
  requirement off keeps confirmed TOTP; a stored requirement with TOTP off never signs in with
  the password alone. The requirement needs every catalog permission and the password and TOTP
  switched on. An administrator's removal of a user's TOTP is audited, refuses the caller's own,
  a more privileged user's and a key's owner, and ends every session of the user; a set-up whose
  secret was replaced answers 400 `auth.flow_invalid`; four-case and cross-organization cases for it and
  the set-up route come from the permission suite.

`/api/v1/me/...` routes have no permission to withhold, so their four-case test is three cases
(401 anonymous, 404 for another user's id, 2xx for the owner) plus the API-key 403 of T9.

## Out of scope

- Sending email (enrollment links, magic links, security notices): M1 (platform and
  organization senders, `docs/plan/technical.md` -> Email and notifications).
- Admin creation and editing of user records: `03-users-and-identities.md`.
- The sign-in, enrollment and security-settings pages: `11-app-shell.md` (the sign-in and
  enrollment screens, and the account page at `/account` whose Passkeys section manages B13's
  passkeys, issue #179).
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
- **Q14. Which addresses have passkeys.** Raised by issue I4. With Q9 (a), the start route's
  options list an account's credential ids, so anyone can tell an address with a passkey from
  one without (an unknown address, or an account without a passkey, both get an empty list).
  (a) Discoverable sign-in (Q9 (b)): the start route takes no email and allows no credential,
  and the browser offers the passkeys it holds; email-first then goes or stays as a fallback.
  (b) Fake credential ids for an address without passkeys, derived from the address so they
  are stable (`webauthn-rs` 0.6.2 has `fake::WebauthnFakeCredentialGenerator`). (c) Accept the
  difference. Registration today sets `residentKey: discouraged` (Q12), so some passkeys
  registered now will not be discoverable and could not sign in under (a) without registering
  again; choosing (a) later may mean changing registration to `preferred` now. Not built in
  M0 (owner, 14 Nov 2026). **Open:** the owner's.
- **Q15. The sign counter's edge case and the flag.** Raised by issue I4. Q12 says to refuse
  only when both counters are non-zero; the server also refuses a stored counter above 0
  followed by a presented 0, the W3C rule `webauthn-rs` applies, since `webauthn-rs` cannot
  apply the literal one and refusing fails closed. Keep it, or accept that case? And should a
  passkey with `counter_regressed_at` set be blocked from sign-in (and then how is it
  cleared: an admin, deleting the passkey), or only shown? Today it is only stored. **Open:**
  the owner's.
- **Q16. Passkeys after a provider's setting goes off.** Raised by issue I5. Switching a
  provider's `allow_local_passkeys` off stops its users adding passkeys (B13), but the passkeys
  they already added still sign in. (a) Keep them; (b) refuse sign-in with them while none of
  the user's providers allows passkeys and they have no local account; (c) delete them when the
  setting goes off. Recommendation: (b), which fails closed and keeps the rows, so switching the
  setting back on restores them as B2 does for a method.
  **Decided:** (b) (owner, 15 Nov 2026). `POST /auth/local/passkey/finish` applies B13's rule
  after the assertion verifies and before admission: a user with no identity at the local
  provider is refused with 403 `local.passkeys_not_allowed` unless they have an identity at an
  enabled provider whose `allow_local_passkeys` is exactly `true`. Nothing is saved for the
  refused sign-in; the passkey rows stay and sign in again once the setting is back on. The
  sign-in screen shows the code's message (`11-app-shell.md` B17).

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
   `11-app-shell.md` (pages the test drives), Q10. T20 landed with `11-app-shell.md`'s sign-in
   and enrollment screens (`web/e2e/sign-in.spec.ts`); I5 extends it with a passkey added
   through `/api/v1/me/passkeys` that then signs in alone (coordinator decision, 2026-11-15).
6. **I6. Password.** Adds `argon2`; `local_logins` with Q5's `email_normalized` and the
   password columns (pending Q3), hashing with the concurrency
   cap, enrollment by password, sign-in with uniform failures, rehash, change password,
   per-account rate limit. Covers B9, B14, B16 to B18; T5, T8 (password part), T10, T12 to T14.
   Depends on: I2, Q1, Q8.
7. **I7. TOTP.** TOTP crate after dependency research, TOTP columns (pending Q3), enrollment and
   confirmation, the second sign-in step with replay refusal, and Q11's recovery codes and
   disabling. Covers B15, B19 to B21; T11, T15, T17. Depends on: I6, `08-sessions-and-csrf.md`
   I1 (encryption), Q11. The dependency research found no TOTP crate worth its size: the
   algorithm is `stepclimb_auth::totp` over `hmac`, `sha1` and `data-encoding`, which the
   lockfile already held (coordinator decision, 2026-11-15). The organization setting that
   would require TOTP (Q11 (a)) is not part of this issue (B15's decision).
8. **I8. Magic link.** Request and finish routes, token lifetime, the landing-page button rule.
   Covers B22; T16. Depends on: I2, the M1 email spec (`docs/specs/m1.md`), Q13. Scheduled per
   Q1.
9. **I9. Required TOTP (#192).** Q11 (a)'s organization setting `totp_required`, TOTP set up during
   a password sign-in while it is required, and an administrator's removal of a user's TOTP.
   Covers B15's and B21's I9 decisions. Depends on: I7. Stays in M0 (coordinator decision,
   2026-11-15: Q11 (a), which the owner decided, includes the setting).
