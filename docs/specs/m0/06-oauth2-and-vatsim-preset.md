# Generic OAuth 2 sign-in, provider configuration and the VATSIM preset

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access -> Sign-in providers (generic OAuth 2,
  the `[[providers]]` example, VATSIM Connect as a preset); Integration platform -> Connectors
  (VATSIM Connect as a sign-in preset); Architecture and stack (`oauth2` crate); API surface
  (`/auth/{provider}/start|callback`, `/api/v1/admin/providers`); Hosting and operations
  (configuration); Testing (mock-OIDC sign-in in Playwright); vZDC deployment (sign in with
  VATSIM); `docs/plan/vzdc-brief.md` -> What training staff need (VATSIM sign-in)
- **Depends on:** `03-users-and-identities.md` (`identity_providers`, `identities`),
  `05-oidc.md` (start and callback routing, outbound HTTP, secrets), `07-admission.md` (I1),
  `08-sessions-and-csrf.md` (flow state, sessions, rate limits), `09-permissions-and-roles.md`
  (`Require<P>`), `14-compose-deploy.md` (`stepclimb.toml`, the Compose stack),
  `11-app-shell.md` (sign-in page, for the e2e test)
- **Review:** maintainer line-by-line

## Summary

An organization can sign people in with any OAuth 2 provider that has authorize, token and
userinfo URLs, by mapping fields of the userinfo JSON to a subject, profile fields and
attributes with JSON Pointers. VATSIM Connect ships as a preset of that mechanism, so vZDC
configures only its client id and secret. This spec also defines how providers are configured
(the plan's `[[providers]]` TOML and `/api/v1/admin/providers`), and the mock OAuth 2 and OIDC
server every sign-in test and the Playwright stack use instead of a real provider.

## Behavior

### Mock identity provider

- **B1.** The repository has a mock IdP that serves, on a loopback port, an OIDC provider
  (discovery, JWKS, authorize, token, userinfo, ID tokens it signs with keys it generates) and a
  plain OAuth 2 provider (authorize, token, and a userinfo endpoint that returns any JSON the
  test sets). Its authorize endpoint approves at once as the user the test selects, with no
  login page. Where it lives and how it is packaged: pending Q1.
- **B2.** A test can make the mock misbehave in each way `05-oidc.md` T3 to T11 needs: wrong
  `iss`, `aud` or `azp`, expired or future tokens, `alg: none`, an unknown signing key, key
  rotation, a wrong `nonce` or `at_hash`, `error=access_denied` at authorize, 500 or delayed
  responses at any endpoint, and a userinfo `sub` mismatch. It records every request it
  receives so a test can assert what Stepclimb sent, and it refuses a token request whose PKCE
  verifier does not match the challenge.

  Coordinator decision (2026-11-14), for I1. The mock is the library crate
  `crates/stepclimb-mock-idp` (Q1, ADR 0027), taken as a dev-dependency only; I7 adds the
  binary for the Compose stack. Its OIDC issuer is `http://127.0.0.1:<port>/oidc` (a path, so a
  relying party that drops it when joining `/.well-known/openid-configuration` fails) and the
  OAuth 2 provider is under `/oauth2`. It registers one client with a random secret that
  contains `+`, `/` and `=`, so a client that skips form-encoding its Basic credentials (RFC
  6749 2.3.1) fails, and signs
  RS256 only, as `05-oidc.md` Q8 needs at least. PKCE is S256 only and required by default; a
  switch makes it optional for a provider with `pkce = false` (Q3), and a verifier sent without
  a challenge is refused. The token endpoints accept `client_secret_basic` and
  `client_secret_post` unless a test narrows the list, which the discovery document then
  advertises (`05-oidc.md` Q6). Authorize accepts any absolute `http` or `https` redirect URI
  unless a test sets an allowlist, matched as exact strings (400 on a miss), and the token
  request must repeat the authorize request's `redirect_uri` as the same string. Once the client
  has authenticated, presenting a code spends it, whether the rest of the token request passes
  or not. Dropping the mock closes its port and cuts its open connections. Token responses carry a refresh token, but the refresh grant answers
  `unsupported_grant_type`: nothing in M0 refreshes (`05-oidc.md` B15). Every fault switch lasts
  until the test sets it back.
- **B3.** The Compose Playwright stack (`just e2e`) runs the mock as a service, and the stack
  configures an OIDC provider pointing at it. A Playwright test signs in through it from the
  sign-in page and lands signed in on the app shell (the plan's "mock-OIDC sign-in").

### Generic OAuth 2 provider

- **B4.** A generic OAuth 2 provider is an `identity_providers` row with `kind = 'oauth2'`, a
  `key`, a `label`, `enabled`, and a `config` holding `authorize`, `token` and `userinfo` URLs,
  `client_id`, the client secret (storage per `05-oidc.md` Q2), `scopes`, and `claims` with a
  required `subject` pointer and optional `given_name`, `family_name`, `email`,
  `email_verified`, `groups` and `attributes.<name>` pointers. A config missing a URL,
  `client_id` or `claims.subject`, or with an invalid pointer, is refused (B13, B14). URLs
  follow the outbound URL policy (`05-oidc.md` Q4).
- **B5.** `GET /auth/{key}/start` for an `oauth2` provider creates a sign-in flow
  (`08-sessions-and-csrf.md` B17) with a random `state` and `return_to`, and answers 303 to the
  `authorize` URL with `response_type=code`, `client_id`, `redirect_uri` (as `05-oidc.md` B6),
  `scope` (the scopes joined by spaces) and `state`. Whether PKCE is sent: pending Q3.
- **B6.** Claim mapping reads values from a JSON document by JSON Pointer (RFC 6901): for
  `oauth2` the userinfo response body, for `oidc` the merged claims (`05-oidc.md` B13). One
  module implements it for both kinds. The `subject` value must be a non-empty string or an
  integer (an integer becomes its decimal string, so `1000001` and `"1000001"` are the same
  subject); anything else, or a missing subject, fails the sign-in with
  `error=auth.subject_missing`. Other fields follow `05-oidc.md` B13.
- **B7.** `GET /auth/{key}/callback` checks the flow and `state` as `05-oidc.md` B7 says, and
  handles provider errors as `05-oidc.md` B8 says.
- **B8.** The server exchanges `code` at the `token` URL with `redirect_uri`, client
  authentication (as `05-oidc.md` Q6) and the PKCE verifier if Q3 sends one. The response must
  be JSON with an `access_token` and a `token_type` of `bearer` (any case). Anything else, a
  non-2xx status or a timeout redirects with `error=auth.provider_error`.
- **B9.** The server calls the `userinfo` URL with `Authorization: Bearer <access_token>` and
  `Accept: application/json`. A non-2xx status, a timeout or a body that is not a JSON object
  redirects with `error=auth.provider_error`.
- **B10.** The mapped sign-in goes to admission (`07-admission.md` B1); the session, final
  redirect and refusal handling are as `05-oidc.md` B14. The provider's tokens are dropped as
  `05-oidc.md` B15 says.

### Provider configuration

- **B11.** Providers can be declared in `stepclimb.toml` as `[[providers]]` tables whose keys are
  the config fields of B4 (and of `05-oidc.md` B1 for `kind = "oidc"`), plus `kind`, `key`,
  `label` and `enabled` (default true). The plan's example writes several pairs on one line,
  which TOML does not allow; the canonical form puts one pair per line:

  ```toml
  [[providers]]
  kind = "oauth2"
  key = "vatsim"
  label = "Sign in with VATSIM"
  authorize = "https://auth.vatsim.net/oauth/authorize"
  token = "https://auth.vatsim.net/oauth/token"
  userinfo = "https://auth.vatsim.net/api/user"
  scopes = ["full_name", "vatsim_details", "email"]
  client_id = "..."
  client_secret_file = "/run/secrets/vatsim_client_secret"   # pending 05-oidc.md Q2

  [providers.claims]
  subject = "/data/cid"
  given_name = "/data/personal/name_first"
  family_name = "/data/personal/name_last"
  email = "/data/personal/email"
  attributes.rating = "/data/vatsim/rating/short"
  attributes.subdivision = "/data/vatsim/subdivision/id"
  ```

- **B12.** How TOML-declared providers relate to rows created through the admin API (which is
  the source of truth, whether TOML rows can be edited in the API, what happens when a provider
  is removed from the file, and which organization a TOML provider belongs to in
  multi-organization mode): pending Q2.
- **B13.** An invalid `[[providers]]` entry stops startup with an error naming the provider
  `key` (or its position when the key is missing) and the field, as the plan requires for bad
  configuration. `stepclimb check-config`, when it exists, reports the same errors.
- **B14.** `/api/v1/admin/providers` lists, creates, reads, updates and deletes the
  organization's `oidc` and `oauth2` providers. Validation is B4 for `oauth2` and `05-oidc.md`
  B1 for `oidc`, answering 422 `provider.config_invalid` with a JSON Pointer to the bad field.
  Client secrets are write-only: no response contains them, only `client_secret_set: true`.
  Each response includes the provider's `redirect_uri` for registration at the provider.
- **B15.** A `key` already used in the organization answers 409 `provider.key_taken`. Key
  format: pending Q5.
- **B16.** Deleting a provider that has identities: pending Q5. Disabling (`enabled: false`)
  always works and makes its `/auth/{key}/...` routes "not found" (`05-oidc.md` B3) while
  keeping identities.
- **B17.** This API does not create or edit the `local` provider row; local sign-in methods are
  switched through `04-local-accounts.md`. Creating `kind = "local"` answers 422
  `provider.kind_unsupported`, as does `saml`.

### The VATSIM Connect preset

- **B18.** A provider with `preset = "vatsim"` (in TOML or the admin API) takes `kind`,
  `authorize`, `token`, `userinfo`, `scopes` and `claims` from the preset, exactly the plan's
  example values shown in B11. The organization supplies `client_id` and the secret; `key`
  defaults to `vatsim` and `label` to "Sign in with VATSIM". Whether other preset fields can be
  overridden: pending Q4.
- **B19.** Nothing VATSIM-specific lives in the core sign-in code (`AGENTS.md` non-negotiable
  10): the preset is data, loaded from where Q4 decides. A server built without the preset
  refuses `preset = "vatsim"` with `provider.preset_unknown` (at startup for TOML, 422 in the
  API).
- **B20.** The admin API and `/.well-known/stepclimb` report a preset provider as `kind:
  "oauth2"`; the API also returns `preset: "vatsim"`.

## API

Every endpoint has `#[utoipa::path]` and appears in `openapi/stepclimb.json`. `/auth/...` routes
are those of `05-oidc.md` (303 redirects with `error` codes). Admin routes return RFC 9457
`application/problem+json` and also the authentication errors of `08-sessions-and-csrf.md` and
403 `auth.csrf_refused` on unsafe methods.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /auth/{provider}/start`, `GET /auth/{provider}/callback` (`oauth2` kind) | as `05-oidc.md` | as `05-oidc.md` | as `05-oidc.md`, plus `auth.subject_missing`; no `auth.id_token_invalid` | none |
| `GET /api/v1/admin/providers` | cursor pagination | 200 list of `{id, key, kind, preset?, label, enabled, source, redirect_uri, config}` (secrets removed) | `auth.unauthenticated`, `permission.denied` | `perm::ProviderManage`, organization |
| `POST /api/v1/admin/providers` | `{key, kind, preset?, label, enabled, config}` | 201 the provider | `provider.config_invalid`, `provider.key_taken`, `provider.kind_unsupported`, `provider.preset_unknown`, `permission.denied` | `perm::ProviderManage`, organization |
| `GET /api/v1/admin/providers/{id}` | none | 200 the provider | `provider.not_found`, `permission.denied` | `perm::ProviderManage`, organization |
| `PATCH /api/v1/admin/providers/{id}` | any of `label, enabled, config` (a `config` patch replaces named fields; an absent secret keeps the stored one) | 200 the provider | `provider.not_found`, `provider.config_invalid`, `provider.config_owned` (pending Q2), `permission.denied` | `perm::ProviderManage`, organization |
| `DELETE /api/v1/admin/providers/{id}` | none | 204 | `provider.not_found`, `provider.in_use` (pending Q5), `provider.config_owned` (pending Q2), `permission.denied` | `perm::ProviderManage`, organization |

`source` in responses is `api` or `config` (pending Q2).

## Data

No new table. This spec defines the `config` shape for `kind = 'oauth2'` rows of
`identity_providers` (owned by `03-users-and-identities.md`, which carries `org_id`):

```json
{
  "authorize": "https://idp.example.test/oauth/authorize",
  "token": "https://idp.example.test/oauth/token",
  "userinfo": "https://idp.example.test/api/user",
  "client_id": "stepclimb",
  "client_secret": "<per 05-oidc.md Q2>",
  "scopes": ["profile", "email"],
  "pkce": true,
  "claims": {
    "subject": "/data/id",
    "given_name": "/data/first_name",
    "email": "/data/email",
    "attributes": { "team": "/data/team" }
  }
}
```

`pkce` is pending Q3. Pending Q2, the row also records whether it came from `stepclimb.toml`
(proposed: `config.source = "config"`, or a new `source text` column if the owner prefers a
column; the plan's `identity_providers` has none). A preset row stores `preset = "vatsim"` and
only the organization's fields; the preset's values are applied when the row is read, so a
preset fix in a release reaches every organization.

## Permissions

| Entry (in the catalog of `09-permissions-and-roles.md` B2) | Scope | Built-in roles |
| --- | --- | --- |
| `perm::ProviderManage` (`provider.manage`): list, create, change and delete identity providers | organization | Organization admin (the plan: "Everything, including providers") |

Entitlement gating (`oauth2_providers` in `10-entitlements-framework.md` Q1, and whether the
VATSIM preset counts as a connector): handled as `05-oidc.md` Q1 recommends for OIDC.

## Test cases

Every sign-in test runs against the mock IdP (B1) on `127.0.0.1`. Database tests use
`#[sqlx::test]`. Test data is synthetic: CIDs such as `1000001`, names such as "Test Student".

- **T1.** (B1, B2) The mock's own tests: discovery and JWKS documents parse with the
  `openidconnect` crate; each fault switch changes exactly the field it names; a wrong PKCE
  verifier gets `invalid_grant`; recorded requests are readable by the test.
- **T2.** (B4) Configs missing `authorize`, `token`, `userinfo`, `client_id` or
  `claims.subject`, or with `claims.email = "data/email"`, are refused naming the field.
- **T3.** (B5) The authorize redirect carries `response_type=code`, the space-joined scopes, the
  `redirect_uri` and a `state`; PKCE parameters are present or absent as Q3 decides.
- **T4.** (B6) Mapping over a userinfo body returns the subject for `"/data/cid"` when `cid` is
  the string `"1000001"` and when it is the number `1000001`, with the same result; a boolean,
  an empty string or a missing subject gives `auth.subject_missing`; JSON Pointer escapes `~0`
  and `~1` resolve per RFC 6901.
- **T5.** (B7) Flow and state failures behave as `05-oidc.md` T5 and T6 for an `oauth2` provider.
- **T6.** (B8) A token response without `access_token`, with `token_type` `mac`, with status
  400, or delayed past the timeout gives `auth.provider_error`; `token_type` `Bearer` succeeds.
- **T7.** (B9) The mock records `Authorization: Bearer <the issued token>` on the userinfo call;
  a userinfo answering 401, or with a JSON array body, gives `auth.provider_error`.
- **T8.** (B10) A full `oauth2` sign-in creates the identity, sets the session cookie, redirects
  to `return_to`, and stores no provider token (the search of `05-oidc.md` T15).
- **T9.** (B11, B13) A `stepclimb.toml` with the canonical example loads; one with `kind =
  "oauth2"` but no `token` stops startup with an error naming the key `vatsim` and the field
  `token`.
- **T10.** (B12) TOML and API interaction behaves as Q2 decides (for the recommended option: a
  TOML provider appears with `source: "config"`, `PATCH` answers 409 `provider.config_owned`,
  removing it from the file and restarting disables it).
- **T11.** (B14) Create, read, update and delete a provider; no response body contains the
  secret string; a `PATCH` without `client_secret` keeps the stored secret (a later sign-in
  against the mock still authenticates).
- **T12.** (B15, B16) A duplicate key answers 409 `provider.key_taken`; deleting a provider with
  identities behaves as Q5 decides; a disabled provider's start route gives
  `auth.provider_not_found`.
- **T13.** (B17) Creating `kind: "local"` or `kind: "saml"` answers 422
  `provider.kind_unsupported`.
- **T14.** (B18) A `preset = "vatsim"` provider resolves to exactly the plan's URLs, scopes and
  claims (a test compares every field to the values in B11).
- **T15.** (B18) Signing in through a preset provider whose URLs a test-only override points at
  the mock, with a VATSIM-shaped synthetic userinfo body, yields subject `1000001`, the mapped
  names and email, and attributes `rating = "S1"` and `subdivision = "USA"`. How the test
  redirects preset URLs to the mock depends on Q4.
- **T16.** (B19) With the preset compiled out, `preset = "vatsim"` gives
  `provider.preset_unknown`; `bin/check-layering.sh` passes.
- **T17.** Four-case test for each `/api/v1/admin/providers` route: anonymous 401; a user
  without `perm::ProviderManage` 403; wrong scope 404 (the permission held only below
  organization scope, if `09-permissions-and-roles.md` allows that grant); an organization
  admin 2xx.
- **T18.** Cross-organization: organization A's admin reading, patching or deleting organization
  B's provider id gets 404, and B's row is unchanged.
- **T19.** (B3) Playwright, against the Compose stack: open the app, choose the mock provider on
  the sign-in page, arrive signed in with the mock user's display name visible. Zero axe
  violations on the sign-in page.
- **T20.** Contract: every route above is in `openapi/stepclimb.json`.

## Out of scope

- OIDC specifics (discovery, ID token validation): `05-oidc.md`.
- The VATUSA connector, roster sync and sync-only mode for vZDC: M4 (`docs/specs/m4.md`) and
  `07-admission.md`.
- Registering vZDC's VATSIM Connect client: an operational step for vZDC staff, not code.
- SAML 2.0 providers: 1.x.
- The sign-in page layout and provider buttons: `11-app-shell.md` (this spec's T19 only
  exercises them).

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Where the mock IdP lives.** (a) A new workspace crate (for example
  `stepclimb-mock-idp`, a library for in-process tests plus a small binary for the Compose
  stack), which needs an ADR and a layering rule; (b) a test-support module inside
  `stepclimb-auth` behind a cargo feature, with the e2e stack using an existing open-source
  mock OIDC server image; (c) an existing third-party server (such as Keycloak) for both, which
  is slow to start and cannot inject the faults of B2. Recommendation: (a); one implementation
  serves both test layers, and it never ships in the release binary.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. TOML providers and database providers** (also `03-users-and-identities.md` Q5). (a) The TOML
  file is the source for the providers it declares: at startup the server upserts them by `key`,
  marks them `source: "config"`, the admin API shows them read-only (409 `provider.config_owned` on
  change), and a provider removed from the file is disabled, not deleted (identities stay); (b) the
  file seeds the database once and the database is the source afterwards; (c) TOML only, no admin
  API for providers. In multi-organization mode a TOML provider needs an organization: (a) an `org =
  "<slug>"` field, required when more than one organization exists; (b) TOML providers only in
  single-organization mode. Recommendation: (a) and (a), which matches the plan's "records owned by
  an external source are read-only in the UI".
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. PKCE for generic OAuth 2.** OAuth 2.1 requires PKCE, and RFC 6749 says an authorization
  server ignores parameters it does not recognize, but the plan names PKCE only for OIDC. (a)
  Always send S256 PKCE; (b) a per-provider `pkce` flag, default on; (c) never for `oauth2`.
  Whether VATSIM Connect accepts or requires PKCE must be checked against VATSIM's
  documentation before the preset's value is set. Recommendation: (b), with the preset's value
  set from that check.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. Where the VATSIM preset lives and what can be overridden.** The plan calls VATSIM
  Connect both "a preset" and a connector. (a) A TOML data file in `stepclimb-connectors`
  exposed as a constant, compiled into the server behind a `vatsim` cargo feature (the server
  may depend on connectors behind features); (b) a data file under `deploy/presets/` read at
  startup; (c) documentation only: an example `[[providers]]` block that admins copy.
  Overrides: (a) any field can be overridden per organization (which also lets a test or a
  developer point it at VATSIM's development environment or the mock); (b) only `key`, `label`
  and the client credentials. Whether to ship a second preset for VATSIM's development
  environment, once its URLs are checked against VATSIM's documentation, is part of this
  question. Recommendation: (a) and (a).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Provider keys and deleting providers in use.** Keys appear in URLs. (a) Lowercase
  letters, digits and hyphens, 1 to 32 characters, starting with a letter or digit, immutable
  after creation (changing it breaks the redirect URI registered at the provider); (b) any
  URL-safe string, editable. Deleting a provider with identities: (a) refuse with 409
  `provider.in_use` and point the admin to disabling; (b) delete the identities with it, so its
  users lose that sign-in method; (c) delete and keep orphaned identities. Recommendation: (a)
  and (a).
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **I1. Mock IdP.** The mock of B1 and B2 (OIDC and OAuth 2, fault switches, request
   recording, PKCE checking), usable in-process from server tests; the ADR and layering rule if
   Q1 makes it a crate. Covers B1, B2; T1. Depends on: Q1, `01-repo-and-ci.md` (CI gates).
2. **I2. Claim mapping and the `oauth2` config type.** The shared JSON Pointer mapping module
   used by both kinds, subject normalization, the `oauth2` config type and its validation.
   Covers B4, B6; T2, T4. Depends on: `05-oidc.md` I1 (config plumbing, URL policy).
3. **I3. Generic OAuth 2 sign-in flow.** Adds the `oauth2` crate (exact pin, at least 14 days
   old, `just deny` clean); start and callback for `kind = 'oauth2'`, token exchange, userinfo
   call, admission hand-off. Covers B5, B7 to B10; T3, T5 to T8. Depends on: I1, I2,
   `05-oidc.md` I2 (shared routes and flow handling), `07-admission.md` I1, Q3.
4. **I4. Providers from `stepclimb.toml`.** Parsing `[[providers]]`, startup validation errors,
   the TOML-to-database relationship of Q2, secrets from `_FILE` paths. Covers B11 to B13; T9,
   T10. Depends on: I2, `14-compose-deploy.md` (the `stepclimb.toml` reader), `05-oidc.md` I1,
   Q2, `05-oidc.md` Q2.
5. **I5. Provider administration API.** `/api/v1/admin/providers` CRUD for both kinds, write-only
   secrets, key rules, deletion rule. Covers B14 to B17; T11 to T13, T17, T18. Depends on: I2, I4
   (for `provider.config_owned`), `09-permissions-and-roles.md` (catalog, `Require<P>`), Q5.
6. **I6. VATSIM Connect preset.** The preset data where Q4 puts it, behind its cargo feature,
   preset resolution in TOML and the API, the preset sign-in test against the mock. Covers B18
   to B20; T14 to T16. Depends on: I3, I4, I5, Q3, Q4.
7. **I7. Mock-OIDC sign-in in the Playwright stack.** The mock as a Compose service for
   `just e2e`, a provider row (or `[[providers]]` entry) pointing at it, the Playwright test.
   Covers B3; T19, T20. Depends on: I1, `05-oidc.md` I4, `14-compose-deploy.md` (Compose
   stack), `11-app-shell.md` (sign-in page).
