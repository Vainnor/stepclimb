# Users and identities

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access (Sign-in providers, Admission,
  Provisioning), Tenancy and structure (reporting lines), Data model (identity), API surface
  (`GET /api/v1/me`), Security and privacy
- **Depends on:** `02-organizations-and-accounts.md` (`OrgScope`, API conventions),
  `08-sessions-and-csrf.md` (the authenticated principal), `09-permissions-and-roles.md`
  (`Require<P>`, grants in `/me`), `10-entitlements-framework.md` (the users limit, features in
  `/me`)
- **Review:** maintainer line-by-line

## Summary

This spec defines who a person is in Stepclimb: the `users` row (with its source, external id
and status), the identities that link a user to an identity provider as `(provider, subject)`,
and the `identity_providers` table those identities point at. It adds the admin API for users
and their identities, and `GET /api/v1/me` as M0 shapes it. Sign-in flows that create and use
these rows are in `04` to `07`.

## Behavior

### Users

- **B1.** A user belongs to exactly one organization (`users.org_id`). Every users repository
  function takes an `OrgScope` (`02-organizations-and-accounts.md` B8) and filters by it.
- **B2.** A user has `display_name` (required, trimmed, not empty), `given_name` and
  `family_name` (required columns, may be empty strings: some providers send neither), `email`
  (optional), `locale`, `tz`, `profile` (a JSON object; M0 defines no profile fields) and
  `status`. Length limits are pending `02-organizations-and-accounts.md` Q6. Every string field,
  and the list's `q` and cursor (B10), refuses the NUL character, which PostgreSQL `text` cannot
  store; `email` also refuses control characters and Unicode format characters (general category
  Cf, such as U+200B) (B12). The repository refuses them with typed errors; issue 3 maps them to
  the API's validation errors.

  Coordinator decision (2026-11-13), for issue 3. Spec 02 Q6 sets limits for organization and
  account names only, so the user fields take the same rule for names: `display_name`,
  `given_name` and `family_name` hold at most 200 characters, `email` at most 254 (the longest
  address an SMTP path carries, RFC 5321) and `external_id` at most 200. The repository refuses a
  longer value (422, `errors[].code = "text.too_long"` at the field).
- **B3.** `locale` defaults to `en`: Stepclimb is English-only at 1.0 with every string through
  ICU messages (`docs/plan/technical.md` -> Decided). Any other value is refused in M0 (422,
  `errors[].code = "user.locale_unsupported"`). A user created without a `tz` gets the
  organization's default time zone, an `OrgSettings` key that itself defaults to `UTC`, and every
  `tz` is validated as an IANA time zone with `jiff` (Q4). The column has no default; the caller
  resolves the setting before inserting.

  Coordinator decision (2026-11-13), for issue 3. The setting is `default_tz`. Both it and a
  user's `tz` are checked against the time zone database `jiff` bundles (`jiff-tzdb`), not the
  host's zoneinfo, so a name is accepted the same way on every host and in the distroless image;
  the database is updated with the monthly dependency update. A name is matched in any ASCII
  letter case and stored in the database's spelling (`america/new_york` is stored as
  `America/New_York`). `Etc/Unknown` and fixed offsets are refused. A name the database lacks
  answers 422 with `errors[].code = "tz.unknown"` at `/tz` or `/default_tz`.
- **B4.** Every user records its `source`: `local`, `idp` (identity provider), `sync` (sync API),
  `scim` or `connector`, with an optional `external_id`. The plan names these five sources
  (Identity and access -> Provisioning); the stored spellings are this spec's. A user created by
  an admin is `local`; a user created just-in-time at sign-in (`07-admission.md`) is `idp`. M0
  creates no `sync`, `scim` or `connector` users; M4 does. One `stepclimb_core::Source` enum is
  shared by users, role assignments (`09`) and, in M4, groups.
- **B5.** `external_id`, when set, is unique within the organization. M4's sync API keys its
  upserts on it.
- **B6.** Records owned by an external source are read-only in the UI, so the two never fight
  (Provisioning). Through the admin API, a user whose `source` is not `local` refuses changes to
  `display_name`, `given_name`, `family_name`, `email` and `external_id` (409
  `user.externally_managed`). Q3 is decided (option A): an admin may change such a user's
  `status`, `locale` and `tz`; the "suspended by" marker that keeps a sync from unsuspending
  them is M4's (coordinator decision, 2026-11-13, applying Q3 for issue 3).

  Owner decision (2026-11-14). Through the admin API, changing the `email` or `status` of a user
  is refused with 403 `permission.escalation` (the code and rule of `09-permissions-and-roles.md`
  B12 and Q4) when any of the permissions effective for the target, at any scope, is not
  effective for the caller at organization scope, the scope the user routes act on. Without it a
  holder of `user.manage` alone could change an Organization admin's email and take over the
  account through a password reset, or suspend them. Names, locale and time zone are not
  covered. Changing your own email passes, since your permissions are your own; but an API key
  acts with its owner's permissions narrowed to its scopes (`08-sessions-and-csrf.md`), and a key
  whose scopes do not cover every permission of its owner is refused when it changes its owner's
  own email or status (coordinator decision, 2026-11-14, fail closed). M0 accepts no API key
  yet; `08` issue 6 adds the key principal and a route test of this case.

  What the check is serialized with: the route takes the organization's account tree lock, which
  the role assignment routes take before they create or remove an assignment, then locks `FOR KEY
  SHARE` the custom roles the target holds, which an edit of a role's permissions
  (`roles::update_custom`) waits for, and only then reads the target's grants. Until the `PATCH`
  commits, neither write can change what it checked. Built-in roles change only when startup
  syncs them and are not locked; an assignment written by the repository without the tree lock
  (none through the API in M0; M4's sync API must take it) is not serialized. Course-scope
  assignments grant nothing until M1, which must revisit the check once they do.
- **B7.** `status` is `active` or `suspended` in M0. A suspended user is refused at sign-in
  (Admission step 1, enforced in `07-admission.md`), and every credential of theirs stops working
  on its next request: the request authenticator in `08-sessions-and-csrf.md` reads the user's
  status on each request, so suspension needs no session sweep. Unsuspending restores sign-in;
  sessions that expired meanwhile stay expired.

  Coordinator decision (2026-11-13), from `09-permissions-and-roles.md` issue 6. Suspending the
  organization's last active user holding an organization-scope Organization admin assignment is
  refused (409 `role_assignment.last_org_admin`, the code of `09` B14), whoever asks, so the rule
  holds for the sync API too. `users::set_status` takes the organization-row lock that assignment
  removals take before it counts, so a suspension and a removal cannot both pass, and refuses an
  isolation level above `READ COMMITTED`. The `PATCH /api/v1/users/{user_id}` route maps it to
  that 409.
- **B8.** `manager_id` (reporting lines) is created as a nullable self-reference within the
  organization. M0 never writes it and the API does not expose it; the sync API with managers
  (M4) and the Manager role define it.
- **B9.** Admins create users with `POST /api/v1/users`: source `local`, status `active`,
  optional `identities` to pre-provision (B14). The creation checks the users limit through
  `10-entitlements-framework.md` (B10 there) before inserting. A created user has no local
  credential; how they set one up is `04-local-accounts.md`.
- **B10.** `GET /api/v1/users` lists the organization's users sorted by `display_name` then
  `id`, with optional filters `status`, `source` and `q` (a case-insensitive prefix match on
  `display_name` or `email`).
- **B11.** Users are not deleted in M0. Per-user export and deletion are M4
  (`docs/plan/technical.md` -> Milestones).
- **B12.** `email` is unique within an organization, compared case-insensitively (Q1): two
  users of one organization cannot have addresses equal under `lower()`, while two organizations
  may each have a user with the same address. It is stored as given (trimmed), keeping its case,
  and is refused if it holds a NUL, a control character or a Unicode format character (B2), so
  an invisible character cannot make a second address that looks like the first.
  Nothing in M0 looks a user up by email except `04-local-accounts.md`. An address another user
  of the organization has answers 409 `user.email_taken` on `POST` and `PATCH` (coordinator
  decision, 2026-11-13).

### Identity providers and identities

- **B13.** An identity provider is a row in `identity_providers`: `key` (unique in the
  organization, used in `/auth/{provider}/...` URLs), `kind` (`oidc` or `oauth2` in M0; `saml`
  arrives in 1.x), `config` (a JSON object whose shape belongs to `05-oidc.md` and
  `06-oauth2-and-vatsim-preset.md`) and `enabled`. The admin API for providers
  (`/api/v1/admin/providers`) and the encryption of secrets inside `config` are in those two
  specs. Whether local accounts are also a provider row is `04-local-accounts.md`'s decision; this
  spec's `kind` check is widened by migration if it is. How providers declared in
  `stepclimb.toml` relate to this table is pending Q5.
- **B14.** An identity links one user to one provider by the provider's `subject` (for VATSIM,
  the CID at `/data/cid`). `(provider_id, subject)` is unique, so one external account maps to at
  most one user. A subject holds at most 255 characters (OIDC's limit for `sub`), so it always
  fits the unique index; a longer one answers 422 `text.too_long` (coordinator decision,
  2026-11-14). A user can have several identities, from different providers or the same one
  (a person with two accounts at one provider may link both).
- **B15.** `identities.attributes` holds the mapped attributes from the latest sign-in (for
  example `rating`, `subdivision`), written by `05`/`06` after each successful sign-in and read by
  admission rules (`07-admission.md`). It holds no token or secret.
- **B16.** Lookup at sign-in: `identities::find_user(scope, provider_id, subject)` returns the
  linked user (with status) or none. A known identity loads the user (Admission step 1); what
  happens with an unknown identity is `07-admission.md` (rules, sync-only mode, the webhook,
  just-in-time creation).
- **B17.** **Matching users by email is off by default.** With the setting off, an unknown
  identity is never linked to an existing user because their email addresses match, whatever the
  provider asserts. This prevents a takeover through another provider that lets anyone claim any
  email. The organization setting `email_matching` (an `OrgSettings` key,
  `02-organizations-and-accounts.md` B14) is `false` by default. What turning it on does (which
  providers, whether a verified-email claim is required, what happens with two matching users)
  is pending Q2; until Q2 is decided the setting is stored and returned but the server refuses to
  set it to `true` (422, `errors[].code = "settings.email_matching_unavailable"`).
- **B18.** An admin can pre-provision an identity for a user (`POST
  /api/v1/users/{user_id}/identities` with `provider_key` and `subject`), so that in sync-only
  mode (`07-admission.md`) a known person can sign in the first time. An identity already linked
  to any user answers 409 `identity.taken`; an unknown or other-organization provider key
  answers 422 (`errors[].code = "identity.unknown_provider"`).
- **B19.** Removing an identity (admin: `DELETE /api/v1/users/{user_id}/identities/{identity_id}`;
  self: `DELETE /api/v1/me/identities/{identity_id}`) deletes the row. It is refused with 409
  `identity.last_sign_in_method` when the user would be left with no identity and no local
  credential (`04-local-accounts.md` provides the "has a local credential" check). An identity
  whose user is not `local` source can be removed by an admin but not by the user (409
  `identity.externally_managed`), since its source re-creates it.
- **B20.** How a signed-in user links a further identity (starting a provider flow while signed
  in) is part of the sign-in flows in `05-oidc.md` and `06-oauth2-and-vatsim-preset.md`; they
  write through `identities::link`, which applies B14's uniqueness.

### `GET /api/v1/me`

- **B21.** `GET /api/v1/me` returns the signed-in user to any authenticated principal that is a
  user: a browser session or an app token, or an API key that has an owner user (answering for
  the owner, with the key's effective grants). A service-account key with no owner answers 403
  `auth.not_a_user`. Anonymous answers 401. In M0 only the browser-session case is built so
  far; B24 says when the others land and why `auth.not_a_user` is not answered.
- **B22.** The M0 shape:

  ```json
  {
    "user": {
      "id": "0190…", "display_name": "Sam Ortiz", "given_name": "Sam", "family_name": "Ortiz",
      "email": "sam@example.test", "locale": "en", "tz": "America/New_York",
      "status": "active", "source": "idp"
    },
    "organization": { "id": "0190…", "slug": "example", "name": "Example Org" },
    "grants": [
      { "scope": { "type": "organization", "id": "0190…" }, "permissions": ["account.view"] },
      { "scope": { "type": "account", "id": "0190…" }, "permissions": ["account.manage", "account.view"] }
    ],
    "features": ["…"]
  }
  ```

  `grants` lists each scope where the principal holds at least one effective permission, with
  the permissions sorted, as `09-permissions-and-roles.md` computes them (for an API key, after
  intersecting with its scopes). `features` lists the enabled entitlement features as
  `10-entitlements-framework.md` computes them. The web app uses both only to show or hide UI;
  the server checks every request itself. Courses and next-up items join the response in M1.
- **B23.** `GET /api/v1/me/identities` lists the user's identities with `id`, `provider_key`,
  `provider_label` (from the provider's config), `subject` and `created_at`; never `attributes`
  (they can hold personal data the provider sent, and the user's own copy belongs to M4's export).
- **B24.** Coordinator decision (2026-11-13), for issue 5. API keys arrive with
  `08-sessions-and-csrf.md` issue 6 and app access tokens with its issue 8, and until then the
  server accepts no bearer credential, so `GET /api/v1/me` answers browser sessions only and a
  bearer request is anonymous (401). The owned-key half of B21 and T17 lands with `08` issue 6,
  which adds the key principal to the route and its test; the app-token case lands with `08`
  issue 8 (access-token authentication), which does the same for app tokens. The ownerless half cannot arise in M0: `08` Q9 (a) makes every service
  account have an owner, so no M0 principal is not a user. The server therefore neither produces
  nor documents 403 `auth.not_a_user` until a principal without a user exists. `user` carries
  B22's nine members only (no `external_id` or timestamps), and `email` is always present, `null`
  when the user has none. The owner can overrule this.

## API

Common errors as in `02-organizations-and-accounts.md` -> API (400, 401, 415, 500; 403
`permission.denied` where a permission applies).

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/me` | | 200 `Me` | 403 `auth.not_a_user` (not answered in M0, B24) | authenticated user |
| `GET /api/v1/me/identities` | | 200 `MyIdentity[]` | 403 `auth.not_a_user` | authenticated user |
| `DELETE /api/v1/me/identities/{identity_id}` | | 204 | 404 `identity.not_found`, 409 `identity.last_sign_in_method`, 409 `identity.externally_managed` | authenticated user (own identities only) |
| `GET /api/v1/users` | `?status&source&q&cursor&limit` | 200 `Page<User>` | 400 `pagination.invalid_cursor`, 400 `request.malformed` | `perm::UserView` (organization) |
| `GET /api/v1/users/{user_id}` | | 200 `User` | 404 `user.not_found` | `perm::UserView` (organization) |
| `POST /api/v1/users` | `{ display_name, given_name?, family_name?, email?, locale?, tz?, external_id?, identities?: [{ provider_key, subject }] }` | 201 `User`, `Location` | 409 `identity.taken`, 409 `user.external_id_taken`, 409 `user.email_taken`, 422 `request.invalid`, limit error from `10` | `perm::UserManage` (organization) |
| `PATCH /api/v1/users/{user_id}` | `{ display_name?, given_name?, family_name?, email?, locale?, tz?, status? }` | 200 `User` | 404 `user.not_found`, 409 `user.externally_managed`, 409 `user.cannot_suspend_self`, 409 `user.email_taken`, 409 `role_assignment.last_org_admin`, 422 `request.invalid` | `perm::UserManage` (organization) |
| `GET /api/v1/users/{user_id}/identities` | | 200 `Identity[]` | 404 `user.not_found` | `perm::UserView` (organization) |
| `POST /api/v1/users/{user_id}/identities` | `{ provider_key, subject }` | 201 `Identity` | 404 `user.not_found`, 409 `identity.taken`, 422 `request.invalid` | `perm::UserManage` (organization) |
| `DELETE /api/v1/users/{user_id}/identities/{identity_id}` | | 204 | 404 `user.not_found`, 404 `identity.not_found`, 409 `identity.last_sign_in_method` | `perm::UserManage` (organization) |

Schemas: `User { id, display_name, given_name, family_name, email, locale, tz, status, source,
external_id, created_at, updated_at }`; `Identity { id, provider_key, subject, attributes,
created_at, updated_at }` (admins see `attributes`, since admission rules read them);
`MyIdentity` as in B23; `Me` as in B22.

Coordinator decisions (2026-11-13), for issue 3. The list's `limit` is spec 02's: 1 to 200,
default 50. In `POST`, `email` and `external_id` may be `null` (read as omitted); any other
member sent as `null` answers 400 `request.malformed`, as do unknown members. In `PATCH`, `null`
is accepted only for `email`, where it removes the address. `POST` takes at most 20
`identities` (422, `errors[].code = "identities.too_many"` at `/identities`); an unknown or other
organization's `provider_key` is 422 `identity.unknown_provider` at
`/identities/{index}/provider_key`, an empty subject 422 `identity.subject_empty`, and any
refusal creates neither the user nor any identity. Every check that needs no database (the
fields, the identities' count and subjects) runs before the users limit check, so an
organization at its limit still gets the 422 for a body that breaks a rule. The 422 field codes are `name.empty`
(`display_name`), `email.empty`, `email.invalid_character`, `text.contains_nul`,
`text.too_long`, `tz.unknown` and `user.locale_unsupported`. A `PATCH` naming nothing changes
nothing; the field changes and the status change of one `PATCH` run in one transaction, so a
refused suspension leaves the fields as they were. The status change runs first, so every `PATCH`
locks the organization row before the user row. User creation and field changes write no
audit entry (spec 09 Q5); a status change writes `user.set_status`.

`PATCH` with `status: "suspended"` on the caller's own user answers 409
`user.cannot_suspend_self`, so an admin cannot lock themselves out by accident. The users routes
take an organization target in M0, so a `user.view` grant at account scope
(`09-permissions-and-roles.md` Q2) does not reach them.

## Data

Migration `migrations/<timestamp>_users.sql`:

```sql
CREATE TABLE users (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL REFERENCES organizations (id),
    external_id   text,
    source        text        NOT NULL CHECK (source IN ('local', 'idp', 'sync', 'scim', 'connector')),
    manager_id    uuid,
    display_name  text        NOT NULL CHECK (btrim(display_name) <> ''),
    given_name    text        NOT NULL DEFAULT '',
    family_name   text        NOT NULL DEFAULT '',
    email         text,
    status        text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended')),
    profile       jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(profile) = 'object'),
    locale        text        NOT NULL DEFAULT 'en',
    tz            text        NOT NULL,                    -- no default: the org setting (Q4)
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    FOREIGN KEY (org_id, manager_id) REFERENCES users (org_id, id),
    CHECK (manager_id IS NULL OR manager_id <> id)
);
CREATE UNIQUE INDEX users_external_id ON users (org_id, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX users_by_name ON users (org_id, display_name, id);
CREATE UNIQUE INDEX users_email ON users (org_id, lower(email)) WHERE email IS NOT NULL;  -- Q1
```

Migration `migrations/<timestamp>_identities.sql`:

```sql
CREATE TABLE identity_providers (
    id          uuid        PRIMARY KEY,
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    key         text        NOT NULL,
    kind        text        NOT NULL CHECK (kind IN ('oidc', 'oauth2')),
    config      jsonb       NOT NULL CHECK (jsonb_typeof(config) = 'object'),
    enabled     boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    UNIQUE (org_id, key)
);

CREATE TABLE identities (
    id           uuid        PRIMARY KEY,
    org_id       uuid        NOT NULL,
    user_id      uuid        NOT NULL,
    provider_id  uuid        NOT NULL,
    subject      text        NOT NULL CHECK (subject <> ''),
    attributes   jsonb       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(attributes) = 'object'),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id),
    FOREIGN KEY (org_id, provider_id) REFERENCES identity_providers (org_id, id),
    UNIQUE (provider_id, subject)
);
CREATE INDEX identities_by_user ON identities (org_id, user_id);
```

The data model lists `identities(id, user_id, provider_id, subject, attributes)` without
`org_id`. This spec adds it because the plan's Tenancy section says every row carries an
`org_id`; the composite foreign keys then make it impossible for an identity to join a user and
a provider from different organizations. The key slug format for providers belongs to `05`/`06`.

Repository modules (every function takes `OrgScope`): `stepclimb_core::users` (`get`, `list`,
`create`, `update`, `set_status`, `count_for_limit` for `10`), `stepclimb_core::identities`
(`find_user`, `link`, `unlink`, `list_for_user`, `update_attributes`),
`stepclimb_core::identity_providers` (`get_by_key`, `list_enabled`; the write functions come with
`05`/`06`).

## Permissions

| Permission | Key | Grantable at | Built-in roles in M0 |
| --- | --- | --- | --- |
| `perm::UserView` | `user.view` | organization, account (`09` Q2) | Organization admin, Account admin |
| `perm::UserManage` | `user.manage` | organization (`09` Q2) | Organization admin |

`/me` and `/me/identities` need authentication only. Entitlements: user creation checks the
`users` limit (`10-entitlements-framework.md`); whether the user routes also take a
`Require<Feature>` is `10` Q1.

## Test cases

`#[sqlx::test]` for every database test; routes through the `09` permission suite. Data is
synthetic.

- **T1** (B1). `users::get` with org A's scope and an org B user id returns none.
- **T2** (B2, B3). Creating a user with an empty `display_name` answers 422 with
  `errors[].field = "/display_name"`; `locale: "fr"` answers 422 `user.locale_unsupported`;
  omitted `given_name` stores `''`.
- **T3** (B4). A user created through `POST /api/v1/users` has `source = "local"`; the
  `source` check constraint refuses `'ldap'` at the database.
- **T4** (B5). Two users in org A with `external_id = "E1"` are refused (409
  `user.external_id_taken`); the same `external_id` in org B is accepted.
- **T5** (B6). PATCH of `display_name` on an `idp` user answers 409 `user.externally_managed`
  and changes nothing; the same PATCH on a `local` user succeeds.
- **T6** (B7). Suspending a user makes their existing session's next `GET /api/v1/me` answer 401
  (with `08`'s session fixture); unsuspending lets a new sign-in work.
- **T7** (B7). Suspending yourself answers 409 `user.cannot_suspend_self`.
- **T8** (B8). `users.manager_id` pointing at an org B user is refused by the composite foreign
  key; pointing at itself is refused by the check.
- **T9** (B9). With a test plan whose `users` limit equals the current count, `POST
  /api/v1/users` answers the limit error from `10-entitlements-framework.md` and inserts nothing;
  with the unlimited plan it succeeds.
- **T10** (B10). Listing filters by `status` and `q` (`q=sa` matches "Sam" and "sam@…"), pages in
  `display_name, id` order.
- **T11** (B14). Linking `(vatsim, "1000001")` to a second user answers 409 `identity.taken`; the
  same subject under a different provider is accepted.
- **T12** (B14, Data). An identity row whose `org_id` differs from its user's or provider's
  organization is refused by the foreign keys.
- **T13** (B16). `identities::find_user` returns the linked user and status for a known
  `(provider, subject)` and none for an unknown one.
- **T14** (B17). With `email_matching` off (the default), an unknown identity whose email equals
  an existing user's is not linked by `find_user` or any other M0 function; setting
  `email_matching` to `true` answers 422 `settings.email_matching_unavailable` (until Q2).
- **T15** (B18). Pre-provisioning an identity and then calling `find_user` with it returns the
  user; an unknown `provider_key` answers 422 `identity.unknown_provider`; an org B provider key
  answers the same 422.
- **T16** (B19). Removing a user's only identity when they have no local credential answers 409
  `identity.last_sign_in_method`; with a second identity it succeeds. A user removing their own
  identity that came from a non-`local` user record answers 409 `identity.externally_managed`.
- **T17** (B21, B22, B24). `GET /api/v1/me`: anonymous 401; a session user gets the shape in B22
  (validated against the OpenAPI schema); an ownerless service-account key gets 403
  `auth.not_a_user`; an owned key gets the owner with the key's narrowed grants. Per B24, issue
  5 builds the anonymous and session cases, `08` issue 6 the owned-key case and `08` issue 8 an
  app token's; the ownerless case waits for a principal without a user, which M0 does not have.
- **T18** (B23). `/me/identities` never contains `attributes`.
- **T19** (every `/api/v1/users` route). Four cases: anonymous 401; a user with
  `AccountManage` on the root but no user permission gets 403; a user holding `UserView` at account
  scope only gets 403 on `GET /api/v1/users`, since the route's target is the organization;
  wrong scope (404) is not applicable to an organization target; an organization admin gets the
  2xx. Routes with `UserManage` also get a 403 for a user holding only `UserView`.
- **T20** (every route with an id). Cross-organization: org A's organization admin calling each
  users and identities route with org B's `user_id` or `identity_id` gets 404 and org B's rows
  are unchanged; `DELETE /api/v1/me/identities/{id}` with another user's identity id in the same
  organization answers 404 `identity.not_found`.

## Out of scope

- Local credentials (passkeys, password, TOTP, magic link): `04-local-accounts.md`.
- Provider configuration, the provider admin API and claim mapping: `05-oidc.md`,
  `06-oauth2-and-vatsim-preset.md`.
- Admission (rules, sync-only mode, the webhook, just-in-time creation): `07-admission.md`.
- Sessions, API keys, service accounts: `08-sessions-and-csrf.md`.
- Groups and group members, the sync API, SCIM, connectors, managers and the Manager role: M4.
- Admin invites by email and CSV import: they need email (M1); not specified here.
- Self-service profile editing (`PATCH /api/v1/me`): no M0 spec; a later spec adds it.
- Per-user export and deletion: M4.
- Custom profile fields (`profile` keys): a later spec (customization).

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Is `users.email` unique within an organization, and case-insensitively?** Local sign-in
  by email (`04-local-accounts.md`) needs a unique lookup; identity-provider users may share an
  address (a family account, a provider that sends none). Options: A, unique on `lower(email)`
  per organization for every user; B, unique only among users with a local credential; C, not
  unique, with local sign-in refusing an ambiguous address. Recommendation: A, with the rule
  stated in the admin UI, because it keeps one person per address and makes B17's matching (if
  ever on) unambiguous.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. What `email_matching` does when an organization turns it on.** Options: A, link an unknown
  identity to the single active user with the same address only when the provider marks the
  address verified (OIDC `email_verified`, or a JSON Pointer to a verified flag in an OAuth 2
  mapping) and only for providers the admin marks as trusted for matching; B, the same without
  the per-provider trust flag; C, no automatic link, but offer the signed-in user a "link to your
  existing account" step that proves control of the old account first. Recommendation: A, with
  C as a later addition. Either way: two matching users means no link.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Can an admin change the `status` of a user owned by an external source?** Suspending a
  synced user locally can fight the next `mode=full` sync run (M4) that would leave them active.
  Options: A, allow it, and have sync never unsuspend a user suspended by an admin (needs a
  "suspended by" marker the data model lacks); B, refuse it, so suspension happens at the source;
  C, allow it and let sync overwrite it. Recommendation: A, because an admin must be able to stop
  a compromised account at once; the marker can be a column added in M4.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. The default `tz` and how `tz` is validated.** Options for the default: A, `UTC`; B, an
  organization default timezone setting (an `OrgSettings` key, itself defaulting to `UTC`); C,
  the browser's zone sent at first sign-in. For validation, an IANA time zone database is needed:
  `jiff` (bundles or reads the system tzdb) or `chrono-tz`. Recommendation: B with `UTC` as its
  default, and `jiff` after `dependency-researcher`, since M1 due dates also need zone math.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Configuration names and file layout.** Only `STEPCLIMB_BIND`, `STEPCLIMB_DATABASE_URL`,
  `STEPCLIMB_DATABASE_URL_FILE` and `STEPCLIMB_LOG` exist (`crates/stepclimb-server/src/config.rs`,
  `main.rs`). M0 needs at least the public base URL (for redirect URIs), the secret-encryption key
  for provider client secrets (`*_FILE` variant required), and a decision on `[[providers]]` in
  `stepclimb.toml` (the plan's example). Options for providers: A, `stepclimb.toml` providers are
  upserted into `identity_providers` at startup and shown read-only in the admin UI; B, the file is
  the only source and the table is not used for them; C, the table is the only source and the TOML
  example is documentation of the JSON shape. For names: A, one variable per setting with the
  TOML path joined by `__` (`STEPCLIMB_SERVER__BASE_URL`); B, flat names (`STEPCLIMB_BASE_URL`).
  Recommendation: providers A (it supports the plan's TOML and the plan's admin API); names B,
  matching the existing variables; one ADR records the full list and the `stepclimb.toml` layout.
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **Users table and repository.** Deliver `migrations/<timestamp>_users.sql`, the shared
   `Source` enum and `stepclimb_core::users` (`get`, `list`, `create`, `update`, `set_status`,
   `count_for_limit`) with `#[sqlx::test]` tests. Covers B1 to B8, B12 (the unique `lower(email)`
   index, Q1), T1, T3, T4 (database part), T8. Depends on: `02-organizations-and-accounts.md`
   issue 2. The `tz` column has no default (Q4 makes the default an organization setting); that
   setting and the validation of `tz` come with issue 3.
2. **Identity providers and identities.** Deliver `migrations/<timestamp>_identities.sql` and
   `stepclimb_core::{identity_providers, identities}` (`find_user`, `link`, `unlink`,
   `list_for_user`, `update_attributes`, `get_by_key`, `list_enabled`). Covers B13 to B16, T11 to
   T13. Depends on: issue 1.
3. **User admin routes.** Deliver `GET`/`POST /api/v1/users`, `GET`/`PATCH
   /api/v1/users/{user_id}` with the external-source and self-suspension guards and the users
   limit check, and the `OrgSettings` time zone key (default `UTC`) with `tz` validation through
   `jiff` (Q4). Covers B3 (the `tz` default and validation), B6, B7, B9, B10, T2, T4, T5, T7, T9,
   T10, T19, T20 (users part). Depends on:
   issue 1; `09-permissions-and-roles.md` issue 4; `10-entitlements-framework.md` issue 2.
4. **Identity admin routes and `/me/identities`.** Deliver the three
   `/api/v1/users/{user_id}/identities` routes and `GET`/`DELETE /api/v1/me/identities`. Covers
   B18, B19, B23, T15, T16, T18, T20 (identities part). Depends on: issues 2 and 3;
   `04-local-accounts.md` for the local-credential check (until then the check treats "no local
   credential" as true, which only makes the guard stricter).
5. **`GET /api/v1/me`.** Deliver the route with `user` and `organization` for a browser
   session, and the response schema with `grants` and `features` declared. The owner-key,
   app-token and ownerless-key cases are not this issue's (B24): `08-sessions-and-csrf.md`
   issues 6 and 8 add the first two, and M0 has no ownerless key. `grants` is filled by
   `09-permissions-and-roles.md` issue 6 and `features` by `10-entitlements-framework.md` issue
   3; until then this issue returns both as empty arrays and the schema says so. Covers B21 (the
   session case), B22, B24, T6, T17 (the anonymous and session cases). Depends on: issue 1; the
   `08-sessions-and-csrf.md` issue that provides the authenticated principal.
6. **The `email_matching` setting.** Deliver the `OrgSettings` key (default `false`), the refusal
   to enable it until Q2 is decided, and T14 proving no M0 path links by email. After Q2, a
   follow-up issue in `07-admission.md` implements the decided matching. Covers B17, T14.
   Depends on: issue 2; `02-organizations-and-accounts.md` issue 4.
