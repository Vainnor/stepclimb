# Admission rules and the admission webhook

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access -> Admission, Sign-in providers
  (identities, email matching), Provisioning; Tenancy and structure (groups); Integration
  platform -> Events (Standard Webhooks); Data model (`users`, `identities`,
  `identity_providers`, `groups`, `group_members`); vZDC deployment (sync-only mode)
- **Depends on:** `02-organizations-and-accounts.md` (organizations, settings, `OrgScope`),
  `03-users-and-identities.md` (users, identities, `identity_providers`),
  `08-sessions-and-csrf.md` (sessions, flow state, encryption, clock),
  `09-permissions-and-roles.md` (`Require<P>`)
- **Review:** maintainer line-by-line

## Summary

Admission decides, after a provider has proved who someone is, whether that person may sign in
to this organization and as which user. It runs the plan's five steps in order: a known
identity loads its user (suspended users refused), declarative rules on mapped attributes,
optional sync-only mode, an optional signed admission webhook that fails closed after 3 s, and
just-in-time user creation only when the organization allows sign-up. Every sign-in method in
`04-local-accounts.md`, `05-oidc.md` and `06-oauth2-and-vatsim-preset.md` ends here.

## Behavior

### The sign-in outcome

- **B1.** A provider hands admission a verified sign-in: the organization (from the request,
  `08-sessions-and-csrf.md` B5), the `identity_providers` row, the `subject`, the mapped
  profile fields (`given_name`, `family_name`, `email`, and for OIDC `email_verified`), the
  mapped `attributes` object and the mapped group keys, if any. Admission returns either an
  admitted user id or a refusal code. Only an admitted outcome creates a session
  (`08-sessions-and-csrf.md` B7).
- **B2.** A refusal at a browser callback redirects to the web app's sign-in route
  (`11-app-shell.md`) with `?error=<code>`. A refusal at a JSON endpoint (local sign-in)
  answers 403 `application/problem+json` with the code. The response never says which rule
  refused or what the webhook answered; the server logs the provider key, the step and the
  rule id, without the subject's attribute values (B4 of `08-sessions-and-csrf.md`).
- **B3.** Every step runs inside one database transaction with the identity and user writes, so
  a refusal leaves no user, identity or group change behind.

### Step 1: known identity

- **B4.** If an `identities` row exists for `(provider_id, subject)` in the organization, its user
  is the candidate. A candidate whose `users.status` is suspended is refused with
  `auth.suspended`, before any later step runs.
- **B5.** If no identity matches, the candidate is found as follows, in this order, and if none
  applies the person is "new":
  1. a provisioned user matched to the identity by the rule Q4 chooses (pending Q4; an identity
     an admin pre-provisioned through `03-users-and-identities.md` B18 is already a known
     identity and is handled by B4);
  2. when the organization's `email_matching` setting (`03-users-and-identities.md` B17, off by
     default as the plan says) is on, a user in the organization with the same email, under the
     conditions `03-users-and-identities.md` Q2 sets (decided: option A; until issue 4 builds
     the matching, the setting cannot be turned on, `03-users-and-identities.md` B17).
  A match creates the `identities` row linking `(provider_id, subject)` to that user, inside
  B3's transaction, so it is rolled back if a later step refuses.
- **B6.** With email matching off, an identity is never linked by email, whatever the provider
  says about the address.
- **B7.** On every admitted sign-in, the identity's `attributes` are replaced with the newly
  mapped attributes (merged with the webhook's, B19). Whether `users` profile fields are also
  updated: pending Q6.

### Step 2: declarative rules

- **B8.** An organization can define admission rules over the mapped attributes. Each rule
  states a condition on attribute values and refuses the sign-in when the condition fails, with
  `auth.admission_refused`. Rule syntax, storage and whether rules are per provider or per
  organization: pending Q1 and Q2.
- **B9.** An attribute a rule references that is absent from the mapped attributes makes the
  condition false (fails closed). A rule that cannot be evaluated (wrong value type) refuses.
- **B10.** Which sign-ins rules run on (new people only, or every sign-in including known
  identities): pending Q3. With no rules defined, step 2 admits.

### Step 3: sync-only mode

- **B11.** When the organization's sync-only mode is on, a person with no candidate user after
  B4 and B5 is refused with `auth.not_provisioned`. Only users already provisioned (by the sync
  API, SCIM, a connector or an admin) can sign in. Step 5 never runs in sync-only mode.
- **B12.** Sync-only mode is off by default. It is the `OrgSettings` key `admission.sync_only`
  (`02-organizations-and-accounts.md` B14).

### Step 4: admission webhook

- **B13.** When the organization has an admission webhook configured and enabled, the server
  POSTs `{"provider": <identity_providers.key>, "subject": <subject>, "attributes": {...}}` as
  JSON to its URL, signed with the webhook's secret. Signing scheme: pending Q7 (recommended:
  Standard Webhooks, as the plan uses for outgoing events). Which sign-ins call it: pending Q3.
- **B14.** The webhook has 3 seconds from the start of the connection to the end of the response
  body. Any of these refuses the sign-in with `auth.admission_unavailable`: a timeout, a
  connection or TLS error, a status other than 200, a body that is not JSON, a body missing
  `allow` or with `allow` not a boolean, `groups` not an array of strings, `attributes` not an
  object, or a body over the size limit (64 KiB proposed, Q7).
- **B15.** A reply with `"allow": false` refuses with `auth.admission_refused`.
- **B16.** The server does not follow redirects from the webhook URL (a 3xx is a failure, B14).
- **B17.** The webhook URL is fetched under the same outbound URL policy as provider URLs
  (`05-oidc.md` Q4).
- **B18.** The webhook's `groups` are applied to the admitted user as group memberships (pending
  Q8 for where they are stored, the `source` value, and what happens to unknown group keys).
- **B19.** The webhook's `attributes` are merged over the mapped attributes (webhook keys win)
  before B7 stores them.

### Step 5: just-in-time creation

- **B20.** A new person (no candidate after B4 and B5) who passed steps 2 to 4 is created as a
  user only when the organization allows sign-up (the `OrgSettings` key
  `admission.signup_allowed`, off by default). The user
  gets `source` = the identity-provider source of `03-users-and-identities.md`, `status`
  active, `display_name` from `given_name` and `family_name` (or the subject when both are
  absent), `email` if mapped, and an `identities` row for `(provider_id, subject)`. Before the
  insert it calls the users limit check (`10-entitlements-framework.md` B10); a reached limit
  refuses the sign-in with that spec's code (`entitlement.limit_reached`, pending its Q2).
- **B21.** When sign-up is not allowed, a new person is refused with `auth.signup_closed`.

### Groups from claims

- **B22.** Group keys mapped from provider claims (`05-oidc.md`, `06-oauth2-and-vatsim-preset.md`)
  are applied the same way as webhook groups (B18), pending Q8.

### Administration

- **B23.** An organization admin reads and changes the organization's admission rules through
  `/api/v1/admin/admission`, which also returns the two `OrgSettings` switches (sign-up allowed,
  sync-only mode). Which endpoint writes the switches: pending Q2.
- **B24.** An organization admin configures the webhook (URL, enabled) through
  `/api/v1/admin/admission/webhook`. The server generates the signing secret and returns it
  once, when the webhook is created or its secret is rotated. Reads never return it. It is
  stored encrypted (`08-sessions-and-csrf.md` B3).

## API

Every endpoint has `#[utoipa::path]` and appears in `openapi/stepclimb.json`. Errors are RFC
9457 `application/problem+json`. All routes also answer the authentication errors of
`08-sessions-and-csrf.md` and 403 `auth.csrf_refused` on unsafe methods.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/admin/admission` | none | 200 `{signup_allowed, sync_only, rules: [...]}` (rule shape pending Q1) | `auth.unauthenticated`, `permission.denied` | `perm::AdmissionManage`, organization |
| `PUT /api/v1/admin/admission` | `{rules}`, plus the switches if Q2 says this endpoint writes them | 200 the stored settings | `admission.rule_invalid` (with the rule index), `admission.attribute_unknown`, `admission.provider_unknown`, `permission.denied`, `permission.escalation` (a rule change, I3) | `perm::AdmissionManage`, organization |
| `GET /api/v1/admin/admission/webhook` | none | 200 `{url, enabled, created_at}` or 404 | `admission.webhook_not_found`, `permission.denied` | `perm::AdmissionManage`, organization |
| `PUT /api/v1/admin/admission/webhook` | `{url, enabled}` | 200 `{url, enabled, secret?}` (`secret` only when the webhook was created by this call) | `admission.webhook_url_invalid`, `permission.denied` | `perm::AdmissionManage`, organization |
| `POST /api/v1/admin/admission/webhook/rotate-secret` | none | 200 `{secret}` | `admission.webhook_not_found`, `permission.denied` | `perm::AdmissionManage`, organization |
| `DELETE /api/v1/admin/admission/webhook` | none | 204 | `admission.webhook_not_found`, `permission.denied` | `perm::AdmissionManage`, organization |

Refusal codes produced by admission (B2): `auth.suspended`, `auth.admission_refused`,
`auth.not_provisioned`, `auth.admission_unavailable`, `auth.signup_closed`.

The outgoing webhook request (not a Stepclimb endpoint, documented in the admin guide and as a
schema component in the OpenAPI document): `POST <url>`, `Content-Type: application/json`, body
`{provider: string, subject: string, attributes: object}`, signature headers per Q7. Expected
reply: 200 `{allow: boolean, groups?: string[], attributes?: object}`.

## Data

Admission writes `users`, `identities` and (pending Q8) `group_members`, defined in
`03-users-and-identities.md`. The settings and the webhook need storage the plan's data model
does not name; Q2 proposes it. Proposed, pending Q2:

- **Switches: `OrgSettings` keys** (`02-organizations-and-accounts.md` B14, which assigns them
  to this spec): `admission.signup_allowed` and `admission.sync_only`, both boolean, default
  false. No migration. `email_matching` is `03-users-and-identities.md`'s key.
- **Rules, as Q2 recommends:** per provider in `identity_providers.config` under `admission_rules`
  (shape pending Q1), plus an optional organization-wide list in `organizations.settings` under
  `admission.rules`. The `GET /api/v1/admin/admission` response returns both, the per-provider
  lists keyed by provider key. Built so in I3; the shape is in I3's decisions.
- **`admission_webhooks`** (proposed): `id uuid PK`, `org_id uuid NOT NULL UNIQUE REFERENCES
  organizations(id)` (one webhook per organization), `url text NOT NULL`, `secret bytea NOT
  NULL` (AES-256-GCM), `enabled boolean NOT NULL DEFAULT true`, `created_at`, `updated_at`. The
  plan's `webhook_endpoints` table is for outgoing events (M4) with delivery retries, which do
  not fit a synchronous call that must answer within 3 seconds.

Keys are UUID v7 generated by the server. Migrations are forward-only, one file per issue.

## Permissions

| Entry | Scope | Built-in roles |
| --- | --- | --- |
| `perm::AdmissionManage` (`admission.manage`, in the catalog of `09-permissions-and-roles.md` B2) | organization | Organization admin |

Admission itself runs before anyone is signed in and checks no permission. Just-in-time creation
checks the users limit (B20). Webhook administration checks the `admission_webhook` feature if
`10-entitlements-framework.md` Q1 adds it; the webhook call at sign-in does not, so a downgrade
cannot leave sign-in without its configured gate.

## Test cases

Database tests use `#[sqlx::test]`. Webhook tests run against a local HTTP server started by the
test on `127.0.0.1`, never the network. Time-dependent tests use the controllable clock and
`tokio`'s paused time where the 3-second limit is tested.

- **T1.** (B4) A known identity whose user is active is admitted as that user; the same with a
  suspended user is refused `auth.suspended`, and the webhook test server records no call.
- **T2.** (B3) A refusal at step 4 after a B5 email match leaves no `identities` row behind.
- **T3.** (B5, B6) With email matching off, a new identity whose email equals an existing user's
  email is treated as new (and with sign-up off, refused `auth.signup_closed`); no identity is
  linked.
- **T4.** (B5) With email matching on and `03-users-and-identities.md` Q2's conditions met, the
  identity is linked to the existing user; with them unmet (for example `email_verified` false), it
  is not.
- **T5.** (B5) The provisioned-user match of Q4 links a synced user's first sign-in to that user.
- **T6.** (B7) A second sign-in with changed attributes replaces the stored `attributes`.
- **T7.** (B8, B9) A rule requiring attribute `subdivision` to equal `USA` (synthetic data)
  admits a matching person, refuses another value with `auth.admission_refused`, and refuses
  when the attribute is absent.
- **T8.** (B10) Rule scope per Q3: a known user who no longer meets a rule is refused (or
  admitted) as Q3 decides.
- **T9.** (B11) In sync-only mode a new person is refused `auth.not_provisioned` even with
  sign-up allowed; a provisioned user is admitted.
- **T10.** (B13) The webhook receives the documented body and valid signature headers; the test
  server verifies the signature with the secret returned at creation.
- **T11.** (B14) Each failure refuses with `auth.admission_unavailable`: a reply after 3.0 s
  (paused time), connection refused, status 500, status 302, non-JSON body, `allow` missing,
  `allow` as the string `"true"`, `groups` as a string, a 65 KiB body.
- **T12.** (B14) A reply at 2.9 s with `allow: true` admits.
- **T13.** (B15) `allow: false` refuses `auth.admission_refused`.
- **T14.** (B18, B19, B22) Webhook `groups` and claim groups become memberships per Q8; webhook
  `attributes` override mapped attributes of the same key in the stored identity.
- **T15.** (B20) With sign-up allowed, a new person is created with the identity-provider
  source, the mapped names and email, and one identity; with both names absent,
  `display_name` is the subject.
- **T16.** (B21) With sign-up off, a new person is refused `auth.signup_closed` and no user row
  exists afterwards.
- **T17.** (B2) The refusal redirect carries only `error=<code>`; a captured log line names the
  step and rule id and contains no attribute value.
- **T18.** (B24) Creating the webhook returns the secret once; `GET` never contains it; the
  database column is not the plaintext secret; rotating returns a new secret and the old one no
  longer verifies at the test server.
- **T19.** Four-case test for every `/api/v1/admin/admission*` route: anonymous 401; a user
  without `perm::AdmissionManage` 403; wrong scope 404 (the permission held only below
  organization scope, if `09-permissions-and-roles.md` allows that grant); an organization
  admin 2xx.
- **T20.** Cross-organization: organization A's admin changes A's settings and webhook; the test
  then signs in to organization B and confirms B's settings and webhook are unchanged, and that
  A's rules never run for B's sign-ins.
- **T21.** (B1) Order: a test with every step configured records the sequence of steps run and
  asserts it is 1, 2, 3, 4, 5, and that a refusal at step n runs no step after n.

## Out of scope

- The sync API, SCIM and connectors that provision users: M4 (`docs/specs/m4.md`). Sync-only
  mode works in M0 with admin-provisioned users.
- Enrollment rules (group -> course): M4.
- Group-mapped role assignments and delegated permissions: M4.
- The user-facing linking of a second identity from a signed-in profile: pending Q5; if the
  owner wants it in M0, it becomes an issue here.
- The sign-in UI that shows refusal messages: `11-app-shell.md`.

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Admission rule syntax.** The plan says "declarative rules on the mapped attributes" and
  nothing more. (a) A JSON list of conditions `{attribute, op, value}` with `op` in `equals`,
  `in`, `not_in`, `exists`, `matches` (anchored regex), all of which must hold; (b) (a) plus
  `any` and `all` groups for OR logic; (c) an expression language such as CEL. Recommendation:
  (b); it covers "rating in [S1, S2] and subdivision = USA" without a parser and validates on
  save.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Where rules live and which endpoint writes the switches.**
  `02-organizations-and-accounts.md` B14 makes sign-up and sync-only `OrgSettings` keys, written
  through `PATCH /api/v1/organization/settings` with `perm::OrgManage`, while
  `09-permissions-and-roles.md` gives admission `perm::AdmissionManage`. Switches: (a) written only
  through 02's settings endpoint (`OrgManage`), shown read-only here; (b) written here with
  `AdmissionManage` and refused by 02's endpoint; (c) both. Rules: (a) per provider in
  `identity_providers.config` under `admission_rules`, plus an optional organization-wide list as an
  `OrgSettings` key; (b) only organization-wide; (c) a new `admission_rules` table. Recommendation:
  switches (b), so one permission covers who may sign in; rules (a), since attribute names differ
  between providers. The webhook goes in the proposed `admission_webhooks` table either way.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Which sign-ins steps 2 and 4 apply to.** The plan lists the steps in order but does not
  say whether a known identity skips rules and the webhook. (a) Every sign-in runs steps 2 and
  4, so a member whose rating or roster status changes is refused next time; (b) only new
  people; (c) a per-organization choice. Recommendation: (a); it fails closed, and the webhook's
  `groups` and `attributes` stay current.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. Matching a first sign-in to a provisioned user.** Sync-only mode admits "users already
  provisioned", but a synced user may have no identity until their first sign-in. M0 already
  has (b) below for admins (`03-users-and-identities.md` B18). (a) A per-provider setting
  "match subject to `users.external_id`" (the vZDC case: VATUSA syncs CIDs, VATSIM's subject is
  the CID); (b) only pre-provisioned identities, created by admins now and by the sync API in
  M4; (c) email matching only. Recommendation: (b) in M0, and decide (a) with the M4 sync API
  spec, since the VATUSA connector could send `(provider, subject)` itself.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Linking a second identity on purpose.** What email matching does is
  `03-users-and-identities.md` Q2; `03-users-and-identities.md` B20 leaves the signed-in linking
  flow to the sign-in specs. (a) A signed-in user starts `/auth/{provider}/start?link=1` from
  their profile, and the callback links the identity to the session's user instead of running
  admission steps 1 to 5 (refused with `identity.taken` when the identity belongs to someone
  else); (b) only admins link (03 B18); (c) both. Recommendation: (c), with the self-service
  part built in M1 with the profile page.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. Profile updates on sign-in.** Should a sign-in overwrite `users.given_name`,
  `family_name`, `email` and `display_name` from the provider? (a) Always; (b) only when the
  user's `source` is that provider (a user owned by sync keeps the sync's values, matching the
  plan's "records owned by an external source are read-only"); (c) never after creation.
  Recommendation: (b).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. Webhook signing and limits.** The plan says "signed" without a scheme. (a) Standard
  Webhooks (`webhook-id`, `webhook-timestamp`, `webhook-signature` headers, HMAC-SHA256 over
  `id.timestamp.body`, a `whsec_` secret), as the plan uses for outgoing events, so receivers
  use one library for both; (b) a single `X-Stepclimb-Signature` HMAC header; (c) mutual TLS.
  Also: should the reply be signed (TLS alone authenticates it today), and the reply size limit
  (64 KiB proposed). Recommendation: (a), an unsigned reply over HTTPS, 64 KiB.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. Groups from claims and the webhook in M0.** The plan's `groups` and `group_members`
  tables have no M0 spec, and the provisioning sources list ("local, identity provider, sync
  API, SCIM or a connector") has no value for the webhook. (a) This spec adds both tables with
  the plan's columns, creates unknown group keys on first use, and records `source` as
  identity provider for claims and a new `admission_webhook` value for the webhook; (b) store
  the keys in `identities.attributes` until groups are specified in M4; (c) refuse unknown
  group keys. Membership removal: a group the claims or webhook no longer return is removed
  from the user (only memberships with that same source), or kept. Recommendation: (a), with
  removal of same-source memberships on each sign-in.
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **I1. Admission pipeline core and sign-in completion.** The function every provider calls
   (B1): step 1 (known identity, suspended refused), step 5 (just-in-time creation when sign-up
   is allowed, refusal otherwise), the transaction of B3, refusal redirects and problem bodies
   (B2), attribute replacement (B7), and the hand-off to session creation. Reads
   `signup_allowed` from `organizations.settings` (default false). Covers B1 to B4, B7, B20,
   B21; T1, T6, T15 to T17. Depends on: `08-sessions-and-csrf.md` I2 and I4,
   `03-users-and-identities.md` (users, identities), Q2 (switch storage), Q6.

   Coordinator decisions (2026-11-15), for this issue:

   - The pipeline is `stepclimb_server::admission`: `admit` decides, `complete` also creates
     the session on the caller's transaction (B1, `08-sessions-and-csrf.md` B7). A local
     sign-in (`04-local-accounts.md` B5) uses `complete_local`, keyed by the user the
     credential belongs to: it runs step 1 (lock the user, refuse a suspended one) and the
     session, and reads or writes no identity, so a passkey sign-in never creates a local
     identity. Its input
     (`Verified`) carries the provider row, the subject, the profile fields and the
     attributes; `email_verified` and group keys join it with the issues that read them (I4,
     I7). Steps 2 to 4 and B5's matching are not built, so a known user goes from step 1 to
     B7's writes and an unknown identity to step 5. The issues that add them insert them in
     `admit`'s steps, in order.
   - `admit` runs in a savepoint of the caller's transaction (B3): a refusal rolls back its
     writes even when the caller commits a flow's deletion. Any error is a 500 that admits no
     one: a provider that is not an enabled provider of the request's organization (read again
     before step 1), an invalid subject, a failed query, an `identities` row that cannot be
     stored.
   - Two first sign-ins of one subject at once: step 5 inserts the user and the identity in a
     savepoint of their own. The second insert waits on the identity's unique index; when the
     first commits it fails as taken, the savepoint is rolled back, and step 1 runs again and
     admits the first one's user.
   - Step 1 locks the candidate user's row (`identities::lock_user`, the lock
     `users::set_status` takes) and reads the status again, so a suspension that commits
     first is always seen.
   - `admission.signup_allowed` is stored as `{"admission": {"signup_allowed": bool}}` in
     `organizations.settings`; `GET /api/v1/organization/settings` shows it and `PATCH` lists
     it as `settings.read_only` (Q2 (b)); I2 adds the route that writes it and `sync_only`.
   - A local sign-in (`04-local-accounts.md` B5) never reaches step 5: its account exists first
     (`04-local-accounts.md` B3, and self sign-up with a local account is M1), so a local
     subject without an identity is a server fault, never a new user.
   - Q6 (b) reads "the user's source is that provider" as: the user is `idp` and the identity
     signing in is their oldest one, the identity just-in-time creation made. `users` records
     no provider, and with two providers the second would otherwise overwrite the first's
     values. When the oldest identity is unlinked, the next oldest takes over: the profile
     then follows that identity's provider. Only fields the provider sent change; the display name is rebuilt from the names
     when the provider sent either.
   - A profile value `users` cannot store (over 200 characters, NUL, an email with a control
     or invisible character) is treated as absent, as `05-oidc.md` B13 treats a value of the
     wrong type. So is an email another user of the organization has: `users.email` is
     unique (`03-users-and-identities.md` B12), and with email matching off (B6) the sign-in
     must neither link nor fail on it. A just-in-time user is created without it; a known
     user keeps theirs. A display name made from names or the subject is cut to 200
     characters.
   - The just-in-time identity's `identity.link` entry names the new user as the actor, from
     the request's client address (`09-permissions-and-roles.md` B22, B23); creating the user
     writes no entry (B23). A refusal logs, at info, the provider key, the step
     (`known_identity`, `just_in_time`) and the code; no line carries the subject, a profile
     value or an attribute value.
   - T17's redirect is `Refusal::redirect`: 303 to `/sign-in?error=<code>` with
     `Cache-Control: no-store`. `entitlement.limit_reached` refusals redirect the same way.
2. **I2. Admission settings endpoint.** `GET` and `PUT /api/v1/admin/admission` for the three
   switches, with the four-case and cross-organization tests. Covers B12, B23; T19, T20 (for
   these routes). Depends on: I1, `09-permissions-and-roles.md` (`Require<P>`),
   Q2.

   Coordinator decisions (2026-11-15), for this issue:

   - The switches are the two B12, B20 and B23 name, `signup_allowed` and `sync_only`, stored
     as members of the `admission` settings key; "the three switches" above counts one too
     many. `email_matching` stays `03-users-and-identities.md`'s key, written through
     `PATCH /api/v1/organization/settings`, as Data says. The rules are I3's: I2's `GET`
     answers `{signup_allowed, sync_only}`, and its `PUT` takes those two members only.
   - The `PUT` body is partial: send only what you change. An absent member keeps its stored
     value, and `{}` changes nothing, so a `PUT` from one admin cannot write back a stale
     value of a switch another admin changed in the meantime. A member sent as `null`, an
     unknown member or a non-boolean answers 400 `request.malformed`. The answer is every
     switch after the change.
   - Both routes need `perm::AdmissionManage` at organization scope and nothing more. No
     escalation rule applies, unlike `PUT /api/v1/admin/local-sign-in`
     (`04-local-accounts.md` I1): the owner's rule covers writes that affect another user's
     sign-in, and these switches only decide whether a person the organization has no user
     for gets in. They never refuse a known identity (step 1 runs first, B11 applies after
     B4 and B5), and a just-in-time user gets no role. The API table lists no
     `permission.escalation` for this route. I3's rules run on every sign-in (Q3), so they
     can lock out an existing user, and I3 decides whether its rule writes take the rule.
   - `sync_only` is stored and returned, but the `PUT` refuses to set it to `true` (422
     `request.invalid`, `errors[].code = "admission.sync_only_unavailable"` at `/sync_only`)
     until I4 makes admission apply it, and I4 lifts the refusal. A `true` that no sign-in
     reads would tell an admin that only provisioned users get in while anyone with an
     identity still does when sign-up is on. This is `03-users-and-identities.md` B17's rule
     for `email_matching`. In M0 sign-up off already refuses every person with no user.
   - `stepclimb_core::admission_settings::update` is the only writer. Under the organization
     row's lock it compares the body with the stored switches: equal, it writes nothing (no
     settings change, no `updated_at`, no audit entry) and answers 200. Different, it merges
     the switches into the stored `admission` object, keeping members this release does not
     know, and writes one `organization.update_settings` entry with `admission` before and
     after (`09-permissions-and-roles.md` B22).
   - A release older than I4 ignores a stored `sync_only: true` (I4's note on rollback).
3. **I3. Declarative rules.** Rule model, validation on save, evaluation at step 2, storage per
   Q2. Covers B8 to B10; T7, T8, T21 (steps 1, 2, 5). Depends on: I2, Q1, Q3.

   Coordinator decisions (2026-11-15), for this issue:

   - The rule model is Q1 (b), in `stepclimb_core::admission_rules`. A list is a JSON array
     and every rule in it must hold. A rule is a condition
     `{"attribute": <name>, "op": <op>, "value": <value>}` with `op` one of `equals`, `in`,
     `not_in`, `exists` (no `value`) and `matches`, or a group `{"any": [...]}` or
     `{"all": [...]}`. A value is a string, an integer (64-bit) or a boolean; the values of one
     `in` or `not_in` list share a type. A `matches` pattern uses the Rust `regex` syntax
     (linear-time matching) and is anchored at both ends. Limits: 50 rules per list, 50 per
     group, groups nested at most 4 deep, 100 values per list, 1,000 characters per name,
     string value or pattern, and 1 MiB per compiled pattern. Empty groups are refused.
   - Evaluation (B9) fails closed. An absent attribute, or one present as `null`, fails its
     condition whatever the `op`, so `not_in` with the attribute absent refuses. A present
     attribute of a type the rule cannot compare (an array, an object, a number against a
     string, a non-integer number against an integer) makes the rule unevaluable, which
     refuses. A group evaluates every rule in it, so one unevaluable rule anywhere makes the
     group unevaluable, even an `any` whose other rule holds; the outcome never depends on the
     order of the rules. Both outcomes refuse with `auth.admission_refused`.
   - Storage is Q2 (a): the organization's list in `organizations.settings` as
     `admission.rules`, and each outside provider's in its `identity_providers.config` as
     `admission_rules`. An empty list is stored as no member. A sign-in through an outside
     provider must pass the organization's list, then that provider's list. A stored list
     that does not parse (a newer release's form, corruption) is a server fault: the sign-in
     gets a 500 and no one is admitted through the providers it applies to.
   - Step 2 runs after step 1 on every sign-in through an outside provider, known identity or
     new person (Q3 (a)), before step 1's attribute and profile writes, which a refusal rolls
     back with the savepoint (B3). It reads the rules in the sign-in's transaction, from the
     provider row `admit` reads again before step 1.
   - Local sign-ins (`admission::complete_local`, and `admit` with the local provider) run no
     rules. They carry no mapped attributes, so under B9 any organization-wide rule would
     refuse every local account, the break-glass administrator's included. The local provider
     cannot have a list of its own (`admission.provider_unknown`). A passkey added by a user
     of an outside provider (`04-local-accounts.md` B13) signs in without that provider's
     fresh attributes and so without the rules; the per-provider `allow_local_passkeys`
     switch, off by default, is the control the owner chose for that.
   - Rules see attributes exactly as mapped. The `email_verified` rule (`05-oidc.md` B13,
     `06-oauth2-and-vatsim-preset.md` B10) drops only the profile email; an admin who maps an
     email claim as an attribute and writes a rule on it gets the provider's value, verified
     or not.
   - The admin surface is the I2 route. `GET /api/v1/admin/admission` adds
     `rules: {organization: [...], providers: {<key>: [...]}}`, with every outside provider of
     the organization, enabled or not, by key (`[]` for one without rules) and never the local
     provider. `PUT` takes `rules: {organization?, providers?}`, partial like the switches: an
     absent `rules`, `rules.organization` or `rules.providers`, and a provider the map leaves
     out, keep their stored lists, and a named list is replaced whole. `null` for any of them,
     a non-object `rules` and an unknown member of `rules` are 400 `request.malformed`.
   - Validation on save answers 422 `request.invalid` with at most 20 `errors`, each a JSON
     Pointer and a code: `admission.rule_invalid` (not one of the forms, an unknown member, a
     broken limit, a pattern that does not compile), `admission.attribute_unknown` and
     `admission.provider_unknown`. A provider's list may name only attributes that provider's
     config maps (`claims.attributes`); the organization's list only attributes some outside
     provider of the organization maps. A provider whose stored config does not read maps
     none. A stored list's names are not checked again at sign-in: an attribute a provider
     stops mapping is then absent, and its condition fails.
   - Rule writes take the escalation rule I2 left to this issue: since rules run on every
     outside-provider sign-in, a `PUT` that changes any list is refused with 403
     `permission.escalation` unless the caller holds every catalog permission at organization
     scope, the rule of `PUT /api/v1/admin/local-sign-in` (`04-local-accounts.md` I1). A `PUT`
     whose lists equal the stored ones needs only `perm::AdmissionManage` and writes nothing.
     The switches keep I2's rule; a body that changes both is refused whole.
   - The write locks the organization's row and then every outside provider's row
     (`FOR NO KEY UPDATE`), compares, and writes only the lists that differ. Each changed list
     gets one audit entry in the same transaction: `organization.update_settings` with
     `admission.rules` before and after, or `identity_provider.update` on the provider with
     its `admission_rules` before and after (`09-permissions-and-roles.md` B22).
   - A rule refusal logs, at info, the provider key, the step `rules`, the code, the rule's
     id (its JSON Pointer in the `GET` body, such as `/rules/providers/sso/1`) and the
     outcome (`fails` or `unevaluable`), never an attribute value. Each step logs at debug as
     it starts (`admission runs a step`, with `step`), which T21 reads.
   - Every writer of `identity_providers.config` keeps the stored `admission_rules` member:
     `stepclimb.toml` providers (`06-oauth2-and-vatsim-preset.md` I4), which rewrite the
     config on each start, and the provider administration API (06 I5), whose config
     validation refuses unknown members. A writer that drops the member silently removes that
     provider's rules, which admits people the rules refused.
4. **I4. Unknown-identity resolution and sync-only mode.** The provisioned-user match (Q4),
   email matching under `03-users-and-identities.md` Q2's conditions (which lifts the refusal to
   set `email_matching` to `true`, `03-users-and-identities.md` B17), sync-only refusal. Covers
   B5, B6, B11; T2 to T5, T9. Depends on: I3, Q4, `03-users-and-identities.md` Q2.

   Coordinator decisions (2026-11-15), carried from I2, for this issue:

   - I4 lifts I2's refusal of `sync_only: true` (`admission.sync_only_unavailable`) when
     admission applies it, and keeps the partial `PUT` of I2: a member the body leaves out
     keeps its stored value. I3's `rules` member, when it is added to the same body, follows
     the same rule and never replaces the switches: a `PUT` naming only `rules` leaves
     `signup_allowed` and `sync_only` as stored.
   - Rollback: after I4 an organization can store `sync_only: true`. A release older than I4
     reads the key as `AdmissionSwitches` does (a stored member is kept and its value read),
     but its admission never applies it, so after a rollback a person with no user is
     admitted again whenever sign-up is allowed. The release notes of I4's release say so,
     and an operator who rolls back with sync-only mode on turns sign-up off first.
5. **I5. Webhook administration.** Migration for `admission_webhooks` (pending Q2), the four
   webhook routes, server-generated secret returned once, rotation, encryption at rest. Covers
   B24; T18, T19 and T20 (for these routes). Depends on: I2, `08-sessions-and-csrf.md` I1
   (encryption), Q7.
6. **I6. Admission webhook call.** The signed POST, the 3-second limit, every fail-closed case,
   `allow`, merged attributes, no redirects, outbound URL policy. Covers B13 to B17, B19; T10
   to T13, T21 (all steps). Depends on: I4, I5,
   `05-oidc.md` I1 (outbound URL policy, Q4 there), Q3, Q7.
7. **I7. Groups from claims and the webhook.** Group tables or storage per Q8, applying and
   removing memberships at sign-in. Covers B18, B22; T14. Depends on: I6, Q8.
