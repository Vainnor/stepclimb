# Sessions, CSRF, API keys and the OAuth 2.1 server

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access -> Sessions and tokens; Security and
  privacy; API surface (`/auth/...`, `POST /oauth/token|revoke`, `/.well-known/stepclimb`); Web
  app and native apps -> Tauri app; Data model (`sessions`, `oauth_refresh_tokens`,
  `service_accounts`); Hosting and operations (configuration, scaling); Testing; Decided ->
  Technical choices (own OAuth 2.1 server)
- **Depends on:** `02-organizations-and-accounts.md` (organizations, `OrgScope`),
  `03-users-and-identities.md` (users, `users.status`), `09-permissions-and-roles.md` (catalog,
  `Require<P>`, permission evaluation), `10-entitlements-framework.md` (`Require<Feature>`)
- **Review:** maintainer line-by-line

## Summary

This spec gives Stepclimb its authenticated request: a browser session in the `__Host-sc_session`
cookie, a bearer API key for machines, or a 15-minute app access token from Stepclimb's own
OAuth 2.1 server. It also adds CSRF protection for cookie requests, sign-out, the short-lived
sign-in flow state that the sign-in specs (04 to 07) share, rate limits on sign-in and token
endpoints, and `/.well-known/stepclimb`. The Tauri client side of app sign-in is M5.

## Behavior

### Shared foundations

- **B1.** Every expiry and lifetime rule in this spec reads a controllable clock carried in
  `AppState`, never the system time inside the logic (`.claude/rules/testing.md` -> Time and
  randomness). Tests replace it with a clock they move. If an earlier M0 issue already added the
  clock, this spec uses it.
- **B2.** Session tokens, API keys, refresh tokens, sign-in flow tokens and authorization codes
  are generated from the operating system's CSPRNG. The server stores only a hash of each and
  compares hashes in constant time. Token length, encoding and hash function: pending Q2.
- **B3.** Secrets the server must read back (provider client secrets, TOTP seeds, the admission
  webhook secret, sign-in flow payloads) are encrypted with AES-256-GCM under the server's
  encryption key, with a fresh nonce per value and a key identifier stored beside the
  ciphertext so the key can be rotated. Where the key comes from and how rotation works: pending
  Q3. Each value is also bound to where it lives: the caller passes context parts (for example
  table, organization id, row id and column) that are authenticated with it but not stored, and
  the same value opened with any other context fails decryption, so a ciphertext copied to another
  row, organization or column is useless. The server logs a warning at startup for each
  configured key that is the published development key.
- **B4.** No secret, token, cookie value, password, TOTP seed, API key or authorization code
  appears in a log line, an error body or a `problem+json` `detail`, at any log level. The
  configuration's debug output hides the database password and URL parameters and shows only
  encryption key ids.

### Organization of an unauthenticated request

- **B5.** In single-organization mode every request belongs to the one organization. How a
  multi-organization instance picks the organization for a request that carries no credential
  (sign-in pages, `/auth/...`, `/.well-known/stepclimb`, `/oauth/token`), and where the
  organization's public origin comes from: pending Q1. Every behavior below that says "the
  request's organization" or "the organization's origin" depends on Q1's answer.
- **B6.** A credential (session, API key, access token) whose user or service account belongs to
  a different organization than the request's organization authenticates nothing: the request
  is treated as anonymous.

### Browser sessions

- **B7.** A successful sign-in (from `04-local-accounts.md`, `05-oidc.md` or
  `06-oauth2-and-vatsim-preset.md`, after `07-admission.md`) creates a new `sessions` row with
  `kind = 'browser'`, `last_seen_at = now`, `expires_at = now + 30 days`, and sets the cookie
  `__Host-sc_session` with `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` and no `Domain`
  attribute. Whether the cookie also carries `Expires`: pending Q5. A sign-in never reuses an
  existing session token, so a token planted before sign-in is never promoted.
- **B8.** On each request carrying the cookie the server hashes the token and looks up the
  session through its user, within the request's organization. The session is valid when the
  row exists, `now < expires_at` (30 days at most) and `now < last_seen_at + 12 hours` (12 hours
  idle).
- **B9.** A valid session refreshes `last_seen_at`. How often the write happens: pending Q4.
- **B10.** An expired or idle session is deleted, the response clears the cookie
  (`Max-Age=0`), and a route that needs authentication answers 401 `auth.session_expired`. A
  cookie whose hash matches no row is cleared and answers 401 `auth.unauthenticated`.
- **B11.** The request authenticator reads the user's `status` on every request
  (`03-users-and-identities.md` B7). A session, API key or access token whose user is suspended
  authenticates nothing and the request answers 401 `auth.suspended`. The session row is kept,
  so unsuspending before it expires restores it; a refresh token of a suspended user is refused
  at `/oauth/token` with `invalid_grant`.
- **B12.** Safe methods (GET, HEAD, OPTIONS) change no state other than `last_seen_at` and the
  sign-in flow rows of B17, so a `SameSite=Lax` cookie sent on a cross-site top-level GET can
  do nothing.
- **B13.** `POST /auth/sign-out` deletes the request's session row and clears the cookie. It
  answers 204 whether or not a session existed.

### CSRF

- **B14.** Every request with an unsafe method (POST, PUT, PATCH, DELETE) that does not carry an
  `Authorization` header passes the CSRF check before any handler runs, whether or not it
  carries a session cookie (sign-in POSTs are covered too). The check passes when
  `Sec-Fetch-Site` is `same-origin`, or when `Sec-Fetch-Site` is absent and the `Origin` header
  is on the organization's Origin allowlist. `Sec-Fetch-Site` values `same-site`, `cross-site`
  and `none` fail. A failure answers 403 `auth.csrf_refused`.
- **B15.** What happens when both `Sec-Fetch-Site` and `Origin` are absent, and where the Origin
  allowlist comes from beyond the organization's own origin: pending Q6.
- **B16.** Requests authenticated by an `Authorization: Bearer` header (API keys, app access
  tokens, the `/oauth/token` and `/oauth/revoke` form posts) skip the CSRF check: a browser
  never attaches those credentials on its own. A request that carries both a bearer header and
  a session cookie is authenticated by the bearer only.

### Sign-in flow state

- **B17.** A sign-in that spans more than one request (an OIDC or OAuth 2 redirect, a WebAuthn
  ceremony, password then TOTP, an OAuth 2.1 authorization) keeps its state on the server in a
  sign-in flow, bound to the browser by a random flow token. Storage, cookie and lifetime:
  pending Q8 (recommended: an `auth_flows` table and a `__Host-sc_flow` cookie, 10 minutes).
- **B18.** A flow is single use: the request that completes it deletes it in the same
  transaction that creates the session (or authorization code). A second use answers
  `auth.flow_invalid`. An expired flow answers `auth.flow_expired`.
- **B19.** A flow records a `return_to` path. The server accepts only a path that starts with
  one `/` and is not `//` or `/\`, so a sign-in can never redirect to another host; anything
  else becomes `/`.
- **B20.** Flow payloads that hold secrets (PKCE verifier, OIDC nonce, WebAuthn ceremony state)
  are encrypted per B3.
- **B21.** Expired flows are deleted by the request that finds them and by a periodic cleanup;
  the cleanup's schedule rides on the jobs runner when it exists (M1), and until then on a
  `tokio` interval in the server. Pending Q8.

### Rate limits

- **B22.** The server rate-limits the sign-in endpoints (`/auth/...` start and finish routes,
  password and TOTP checks), `POST /oauth/token` and `POST /oauth/revoke`. A limited request
  answers 429 `auth.rate_limited` with a `Retry-After` header in seconds. Limits, keys (client
  IP, account identifier), storage and how the client IP is found behind a proxy: pending Q7.
- **B23.** Failed password and TOTP checks count against the account identifier as well as the
  client IP, so one address cannot try many passwords for one account and many addresses cannot
  share one account's budget (numbers pending Q7).

### Service accounts and API keys

- **B24.** An organization admin creates a service account with a name and a list of scopes. The
  server generates an API key `sc_<random>_<crc32>`, stores only its hash in
  `service_accounts.secret_hash`, and returns the key once in the creation response. It never
  returns it again. Whether every key has an owner, and one key per row: pending Q9.
- **B25.** The `<crc32>` suffix is the CRC-32 of the `sc_<random>` part, so a secret scanner (and
  the server, before any database lookup) can reject a mistyped or made-up key. A key whose
  checksum fails answers 401 `auth.api_key_invalid` without a query. Encodings pending Q2.
- **B26.** A request with `Authorization: Bearer sc_...` is authenticated as the service account
  whose `secret_hash` matches, within the request's organization. An unknown key answers 401
  `auth.api_key_invalid`.
- **B27.** A key's rights on each request are its scopes intersected with its owner's current
  permissions (`09-permissions-and-roles.md`), evaluated per request, never cached across
  requests. When the owner loses a permission or is suspended, the key loses it at once. A
  suspended owner makes the key answer 401 `auth.suspended`.
- **B28.** What a scope string names (one catalog permission, or a named bundle such as the
  plan's `reports:read`): pending Q9.
- **B29.** Deleting a service account makes its key fail on the next request.

### Stepclimb's OAuth 2.1 server (for Stepclimb's own apps)

- **B30.** The server accepts only public clients that belong to Stepclimb's own apps, only the
  authorization code grant with PKCE `S256`, and the refresh token grant. It refuses the
  implicit grant, the password grant, client credentials, `plain` PKCE and a missing
  `code_challenge`. The client registry and redirect URIs: pending Q10.
- **B31.** The app opens the authorization endpoint in the system browser with `client_id`,
  `redirect_uri`, `code_challenge`, `code_challenge_method=S256` and `state`. The server checks
  `client_id` and an exact `redirect_uri` match before anything else; on a mismatch it shows an
  error page and never redirects. If the browser has no valid session, the server stores the
  request in a sign-in flow (B17) and sends the browser through normal web sign-in, then back.
  With a session, it issues a single-use authorization code bound to the client, redirect URI,
  user and code challenge, and redirects to `redirect_uri` with `code` and `state`. Endpoint
  path, consent and code storage: pending Q10.
- **B32.** `POST /oauth/token` with `grant_type=authorization_code` exchanges a code once, when
  `client_id` and `redirect_uri` match the code and the S256 challenge computed from
  `code_verifier` (RFC 7636) equals the stored challenge. It returns an access token valid for
  15 minutes and a refresh token, and starts a new refresh family (`family_id` = the new row's
  id, `rotated_from` null). A second use of the same code fails and revokes the family the
  first use created.
- **B33.** `POST /oauth/token` with `grant_type=refresh_token` and a current refresh token
  returns a new access token and a new refresh token in the same family, with `rotated_from` set
  to the presented token's row. The presented token stops working. Refresh lifetime: pending
  Q11.
- **B34.** Presenting a refresh token that has already been rotated is reuse: the server deletes
  every refresh token in that family and answers `invalid_grant`. The user must sign in again
  on that device. Whether concurrent refreshes get a grace window: pending Q11.
- **B35.** `POST /oauth/revoke` with a refresh token deletes its whole family and answers 200
  whether or not the token was known (RFC 7009 behavior). Revoking an access token: pending Q12.
- **B36.** An access token authenticates `/api/v1` requests as its user, with the user's current
  permissions read on each request, and stops working at its 15-minute expiry. A suspended
  user's access token answers 401 `auth.suspended`. Token format, signing key and whether a
  revoked family also stops its live access tokens: pending Q12.
- **B37.** `/oauth/token` and `/oauth/revoke` take `application/x-www-form-urlencoded` bodies.
  Their error format: pending Q13.

### Server discovery

- **B38.** `GET /.well-known/stepclimb` answers without authentication with the request's
  organization name, its enabled sign-in methods (each provider's `key`, `kind` and `label` from
  `identity_providers`, plus the enabled local methods from `04-local-accounts.md`), and the
  OAuth 2.1 endpoints for the app. It never includes a client secret or any provider
  configuration beyond those three fields. Branding and `theme.css` fields are added by the spec
  that owns them (see Out of scope).

## API

Every endpoint has `#[utoipa::path]` and appears in `openapi/stepclimb.json`. Errors are RFC
9457 `application/problem+json` unless noted (B37, Q13). Every endpoint can also answer 429
`auth.rate_limited` where B22 applies, and 403 `auth.csrf_refused` for unsafe methods per B14.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /auth/sign-out` | none (cookie) | 204, cookie cleared | `auth.csrf_refused` | none |
| `GET /.well-known/stepclimb` | none | 200 `{name, sign_in: [{key, kind, label}], oauth: {authorization_endpoint, token_endpoint, revocation_endpoint}}` | none | none |
| `GET /oauth/authorize` (path pending Q10) | query `client_id, redirect_uri, code_challenge, code_challenge_method, state` | 302 to sign-in, or 302 to `redirect_uri?code&state`; HTML error page on client or redirect mismatch | `oauth.client_unknown`, `oauth.redirect_uri_mismatch`, `oauth.pkce_required` | browser session (or sign-in first) |
| `POST /oauth/token` | form: `grant_type=authorization_code, code, redirect_uri, client_id, code_verifier` or `grant_type=refresh_token, refresh_token, client_id` | 200 `{access_token, token_type: "Bearer", expires_in: 900, refresh_token}` | `invalid_request`, `invalid_client`, `invalid_grant`, `unsupported_grant_type` (format Q13), `auth.rate_limited` | none (client is public) |
| `POST /oauth/revoke` | form: `token, token_type_hint?, client_id` | 200 | `invalid_request`, `auth.rate_limited` | none |
| `GET /api/v1/admin/service-accounts` | cursor pagination | 200 list of `{id, name, kind, scopes, owner_user_id, created_at}`, never the key | `auth.unauthenticated`, `permission.denied` | `perm::ServiceAccountManage`, organization |
| `POST /api/v1/admin/service-accounts` | `{name, scopes: [string]}` | 201 `{id, ..., key}` (the only time `key` is returned) | `permission.denied`, `service_account.scope_unknown`, `service_account.scope_exceeds_owner` | `perm::ServiceAccountManage`, organization |
| `DELETE /api/v1/admin/service-accounts/{id}` | none | 204 | `permission.denied`, `service_account.not_found` | `perm::ServiceAccountManage`, organization |

Authentication errors any protected `/api/v1` route can answer, from this spec's extractor:
`auth.unauthenticated`, `auth.session_expired`, `auth.suspended`, `auth.api_key_invalid`,
`auth.access_token_invalid`, `auth.access_token_expired` (all 401). `Require<P>` from
`09-permissions-and-roles.md` adds 403 and 404.

The `name` column for service accounts is not in the plan's data model; see Q9.

## Data

Migrations are forward-only, one file per issue. Keys are UUID v7 generated by the server;
every table has `created_at` and `updated_at` (`timestamptz NOT NULL DEFAULT now()`).

**`sessions`** (plan columns): `id uuid PK`, `user_id uuid NOT NULL REFERENCES users(id) ON
DELETE CASCADE`, `token_hash bytea NOT NULL UNIQUE`, `kind text NOT NULL CHECK (kind IN
('browser'))`, `last_seen_at timestamptz NOT NULL`, `expires_at timestamptz NOT NULL`,
`created_at`, `updated_at`. Index on `user_id`. Organization: derived through `user_id` ->
`users.org_id`; every query joins `users` and filters on the `OrgScope`. The plan names no other
`kind` value; this spec writes only `browser`.

**`oauth_refresh_tokens`** (plan columns): `id uuid PK`, `client_id text NOT NULL`, `user_id uuid
NOT NULL REFERENCES users(id) ON DELETE CASCADE`, `token_hash bytea NOT NULL UNIQUE`, `family_id
uuid NOT NULL`, `rotated_from uuid NULL REFERENCES oauth_refresh_tokens(id) ON DELETE CASCADE`,
`expires_at timestamptz NOT NULL`, `created_at`, `updated_at`. Indexes on `family_id` and a
unique index on `rotated_from` (a token rotates at most once, which also makes two concurrent
rotations of one token conflict instead of both succeeding). A token is "already rotated" when a
row with `rotated_from = its id` exists. Organization: derived through `user_id`.

**`service_accounts`** (plan columns): `id uuid PK`, `org_id uuid NOT NULL REFERENCES
organizations(id)`, `kind text NOT NULL`, `owner_user_id uuid NULL REFERENCES users(id)`,
`secret_hash bytea NOT NULL UNIQUE`, `scopes text[] NOT NULL`, `created_at`, `updated_at`.
`kind` values, owner nullability in M0, and the proposed `name text NOT NULL` column: pending Q9.

**Proposed, pending Q8: `auth_flows`**: `id uuid PK`, `org_id uuid NOT NULL REFERENCES
organizations(id)`, `token_hash bytea NOT NULL UNIQUE` (hash of the flow cookie value), `kind
text NOT NULL` (`oidc`, `oauth2`, `passkey_register`, `passkey_sign_in`, `password_totp`,
`oauth_authorize`), `provider_id uuid NULL REFERENCES identity_providers(id) ON DELETE CASCADE`,
`user_id uuid NULL REFERENCES users(id) ON DELETE CASCADE`, `payload bytea NOT NULL`
(AES-256-GCM, B20), `return_to text NOT NULL`, `expires_at timestamptz NOT NULL`, `created_at`,
`updated_at`. Index on `expires_at` for cleanup. `org_id` is direct because a flow exists before
any user is known.

**Proposed, pending Q10: `oauth_authorization_codes`**: `id uuid PK`, `user_id uuid NOT NULL
REFERENCES users(id) ON DELETE CASCADE`, `client_id text NOT NULL`, `code_hash bytea NOT NULL
UNIQUE`, `redirect_uri text NOT NULL`, `code_challenge text NOT NULL`, `family_id uuid NULL`
(set on first exchange, for B32's replay revocation), `used_at timestamptz NULL`, `expires_at
timestamptz NOT NULL`, `created_at`, `updated_at`. Organization: derived through `user_id`.

## Permissions

| Entry (in the catalog of `09-permissions-and-roles.md` B2) | Scope | Built-in roles |
| --- | --- | --- |
| `perm::ServiceAccountManage` (`service_account.manage`): create, list and delete service accounts and API keys | organization | Organization admin |

Sign-out, discovery and the OAuth endpoints need no permission. The service-account routes check
`Require<Feature>` for `api_keys` if `10-entitlements-framework.md` Q1 adds it; per-organization
API rate quotas are M6 (Out of scope).

## Test cases

Database tests use `#[sqlx::test]`; time-dependent tests use the controllable clock (B1).

- **T1.** (B2, B4) Generated tokens differ across 10,000 draws, are the length Q2 fixes, and the
  database holds only hashes: a test reads every token column after a sign-in and finds no
  plaintext token.
- **T2.** (B3) A value encrypted under key A decrypts under a key ring holding A; tampering one
  ciphertext byte or the stored nonce fails decryption; two encryptions of one value differ.
- **T3.** (B4) A captured `tracing` subscriber records no session token, API key, refresh token
  or authorization code during a full sign-in, API-key request and token exchange.
- **T4.** (B7) A sign-in sets `__Host-sc_session` with `Secure`, `HttpOnly`, `SameSite=Lax`,
  `Path=/` and no `Domain`; the row has `expires_at = now + 30 days`.
- **T5.** (B7) A request carrying a cookie set before sign-in gets a different token after
  sign-in, and the old value authenticates nothing.
- **T6.** (B8, B10) With the clock at `last_seen_at + 11h59m` the session works; at
  `last_seen_at + 12h` it answers 401 `auth.session_expired`, the row is gone and the cookie is
  cleared.
- **T7.** (B8, B9) A session used every hour stays valid until `created_at + 30 days` and answers
  401 `auth.session_expired` at that instant.
- **T8.** (B10) A cookie with a well-formed but unknown token answers 401 `auth.unauthenticated`
  and is cleared.
- **T9.** (B11) After a user is suspended, their next request with the old cookie answers 401
  `auth.suspended`, and their refresh token gets `invalid_grant`; after unsuspending, the same
  cookie works again if the session has not expired.
- **T10.** (B6) A session of a user in organization A, presented with organization B as the
  request's organization (multi-organization test fixture), is anonymous: 401.
- **T11.** (B13) Sign-out deletes the row and clears the cookie; repeating it answers 204.
- **T12.** (B14) A cookie-authenticated POST with `Sec-Fetch-Site: same-origin` passes;
  `same-site`, `cross-site` and `none` answer 403 `auth.csrf_refused` and the handler does not
  run (a counter in the test handler stays 0).
- **T13.** (B14) Without `Sec-Fetch-Site`, an allowlisted `Origin` passes and any other `Origin`
  answers 403.
- **T14.** (B15) The case with neither header behaves as Q6 decides.
- **T15.** (B16) A POST with `Authorization: Bearer <valid API key>` and `Sec-Fetch-Site:
  cross-site` passes the CSRF layer; the same request with only a cookie fails.
- **T16.** (B16) A request with a valid bearer header and another user's session cookie is
  authenticated as the bearer's principal.
- **T17.** (B12) A router walk asserts no GET route under `/api/v1` is registered with a handler
  marked as mutating (the marker the issue adds), so B12 cannot regress silently.
- **T18.** (B18) Completing a flow twice answers `auth.flow_invalid` the second time; a flow used
  after its expiry (clock moved) answers `auth.flow_expired`.
- **T19.** (B19) `return_to` values `https://evil.example`, `//evil.example`, `/\evil.example`
  and `javascript:x` all end at `/`; `/c/s1/m/2` is kept.
- **T20.** (B22, B23) The request after the limit answers 429 `auth.rate_limited` with
  `Retry-After`; the counter resets after the window (clock moved); failures for one account
  from two IPs share the account budget.
- **T21.** (B24, B26) Creating a service account returns a key once; the list endpoint never
  shows it; the key authenticates `GET /api/v1/me`.
- **T22.** (B25) A key with a wrong CRC-32 suffix answers 401 `auth.api_key_invalid` and the
  test's query counter shows no database query.
- **T23.** (B27) A key scoped to permission X whose owner holds X and Y can use X and gets 403 on
  Y; after the owner's X is removed, the same key gets 403 on X on the next request with no
  restart.
- **T24.** (B27) A key whose owner is suspended answers 401 `auth.suspended`.
- **T25.** (B29) A deleted service account's key answers 401 on the next request.
- **T26.** Four-case and cross-organization tests for each `/api/v1/admin/service-accounts` route:
  anonymous 401; a user without `perm::ServiceAccountManage` 403; wrong scope 404 (09 makes the
  permission grantable at organization scope only, so this case is a role granted on an account
  that does not carry it); an organization
  admin 2xx; organization B's service account id used by organization A's admin, 404.
- **T27.** (B30) `/oauth/authorize` without `code_challenge`, with `code_challenge_method=plain`,
  or with `response_type=token` is refused; `grant_type=password` and `client_credentials`
  answer `unsupported_grant_type`.
- **T28.** (B31) An unknown `client_id` or a `redirect_uri` differing by one character shows the
  error page and never redirects (no `Location` header).
- **T29.** (B31) Without a session, authorize redirects to sign-in; after sign-in the browser
  returns to authorize and receives a code at the redirect URI with the original `state`.
- **T30.** (B32) A wrong `code_verifier` answers `invalid_grant`; the right one returns tokens
  with `expires_in = 900`; a second exchange of the same code answers `invalid_grant` and the
  refresh token from the first exchange stops working.
- **T31.** (B36) An access token works at 14m59s and answers 401 `auth.access_token_expired` at
  15m (clock moved).
- **T32.** (B33, B34) Refreshing with token R1 returns R2; refreshing with R1 again answers
  `invalid_grant` and R2 also stops working (family revoked).
- **T33.** (B34) Two concurrent refreshes with R1 produce one success at most, enforced by the
  unique `rotated_from` index (Q11 decides whether the loser also revokes the family).
- **T34.** (B35) Revoking R2 makes R2 and every other token in its family fail; revoking an
  unknown token answers 200.
- **T35.** (B38) `/.well-known/stepclimb` lists exactly the enabled providers with `key`, `kind`
  and `label`; a disabled provider is absent; a provider configured with a client secret leaks
  no other field (the test compares the JSON key set).
- **T36.** Contract: `just openapi-diff` passes with every endpoint above in
  `openapi/stepclimb.json`.

## Out of scope

- The Tauri client side of app sign-in (system browser, deep link, OS keychain): M5
  (`docs/specs/m5.md`).
- Sign-in methods themselves: `04-local-accounts.md`, `05-oidc.md`,
  `06-oauth2-and-vatsim-preset.md`; who may sign in: `07-admission.md`.
- `GET /api/v1/me`: `03-users-and-identities.md` (it uses this spec's extractor).
- The sign-in UI and the sign-out button: `11-app-shell.md`.
- Branding fields and `theme.css` in `/.well-known/stepclimb`: the spec that owns branding
  (`11-app-shell.md` or `12-tokens-and-theme-generator.md`); this spec serves the auth fields.
- Strict CSP and HSTS headers (plan, Security and privacy): not in this spec's issue list; they
  belong with the app shell or the deploy spec.
- Per-organization API rate quotas from plan entitlements: M6.
- A user-facing list of one's own sessions: the plan does not ask for it.

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Organization of an unauthenticated request, and the organization's public origin.** The
  plan gives each managed organization "a subdomain or its own custom domain" but no rule for
  resolving one, and no setting for a self-hosted instance's public URL. Sign-in redirect URIs,
  the WebAuthn relying party ID, the Origin allowlist and `/.well-known/stepclimb` all need it.
  (a) Host header -> organization through a new `organizations` domain column (and a
  `domains` table later for several per organization); (b) a path prefix such as `/o/{slug}`;
  (c) single-organization only in M0, with one public origin from a configuration variable
  (for example `STEPCLIMB_PUBLIC_URL`), and (a) when multi-organization mode is built.
  Recommendation: (c) now and (a) later; the code takes the organization from one resolver so
  (a) replaces only that function.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Token format parameters.** The plan fixes `sc_<random>_<crc32>` and "stored hashed" but
  not lengths, alphabet or hash. (a) 32 random bytes (256 bits) for every token, base62 for API
  keys and base64url elsewhere, CRC-32 as 6 base62 characters, hashed with SHA-256; (b) the
  same with 16 bytes; (c) HMAC-SHA-256 with a server key instead of plain SHA-256, so a
  database leak alone cannot test guesses. Recommendation: (a); tokens with 256 bits of entropy
  need no slow hash, and (c) adds a key to rotate.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Encryption and signing keys: source and rotation.** The plan says "encrypted
  (AES-256-GCM)" and secrets have `*_FILE` variants, but names no key. (a) Keys from
  `STEPCLIMB_*` variables or their `_FILE` variants, required at startup (missing key stops
  startup with a clear error), as a key ring where one key encrypts and older keys only decrypt;
  (b) a key generated on first start and stored in the database (a database dump then carries
  its own key); (c) an external KMS. Recommendation: (a), with a `stepclimb` CLI subcommand later
  to re-encrypt under the newest key.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. How often `last_seen_at` is written.** Every request writes a row. (a) Every request;
  (b) at most once per 60 seconds per session, so idle expiry is accurate to a minute; (c) at
  most once per 5 minutes. Recommendation: (b).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Cookie lifetime in the browser.** (a) A persistent cookie whose `Expires` is the
  session's `expires_at`, so a browser restart keeps the session; (b) a browser-session cookie
  with no `Expires`; (c) persistent only when the user ticks "keep me signed in".
  Recommendation: (a); the server still enforces both limits.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. CSRF fallback and the Origin allowlist.** When neither `Sec-Fetch-Site` nor `Origin` is
  present, (a) refuse; (b) allow (treat as a non-browser client); (c) allow only when no session
  cookie is present. For the allowlist: (a) the organization's own origin only; (b) its own
  origin plus a configured list (`STEPCLIMB_ALLOWED_ORIGINS`); (c) a per-organization setting.
  Recommendation: refuse, and own origin plus a configured list. Every browser that runs the
  web app sends at least one of the two headers on POST.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. Rate limits.** The plan asks for limits on sign-in and token endpoints but gives no
  numbers or store. Numbers: (a) per client IP 20 sign-in requests per minute and 60 token
  requests per minute, per account 10 failed password or TOTP checks per 15 minutes, as a
  throttle rather than a lockout (a lockout lets anyone lock a victim out); (b) half those; (c)
  configurable with (a) as defaults. Store: in-process per instance (limits multiply by
  replicas) or a Postgres table (one more write per attempt). Client IP: the socket address
  only, or `X-Forwarded-For` from a configured list of trusted proxies. Recommendation: (c)
  in-process, with trusted proxies configured, since the Compose stack puts Caddy in front.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. Sign-in flow state.** The data model has no table for it. (a) The `auth_flows` table in
  Data with a `__Host-sc_flow` cookie (`SameSite=Lax` so it survives the provider's redirect
  back), 10-minute lifetime; (b) an encrypted, self-contained cookie with no table; (c) reuse
  `sessions` with a pending kind. Recommendation: (a); it is single-use by construction, and a
  cookie (b) cannot be invalidated once used.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q9. Service accounts and API keys.** The plan's row has one `secret_hash`, an optional owner
  and `scopes`. Owner: (a) required in M0, so "scopes intersected with the owner's current
  permissions" always has an owner; (b) optional, with an ownerless account's rights equal to
  its scopes alone; (c) a service account is backed by its own `users` row holding role
  assignments. Keys per account: one key per row as modeled (rotation is a new account), or an
  `api_keys` child table. Scope strings: catalog permission keys, or named bundles such as
  `reports:read` mapped to permissions in the catalog. Also a `name` column, `kind` values, and
  `expires_at`/`last_used_at` columns, none in the plan. Recommendation: (a), one key per row,
  scopes as catalog permission keys, add `name` and `last_used_at`, no expiry in M0.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q10. OAuth clients, the authorization endpoint and codes.** The plan lists only `POST
  /oauth/token|revoke`, but PKCE in the system browser needs an authorization endpoint and a
  place to keep codes. Clients: (a) a fixed list in code (the Stepclimb app, its deep-link
  redirect URIs and loopback for desktop); (b) configuration; (c) a table. Endpoint: `GET
  /oauth/authorize` (proposed). Consent: none for Stepclimb's own clients, or a one-time
  "allow this app" page. Codes: the `oauth_authorization_codes` table in Data (60-second
  lifetime) or encrypted self-contained codes. Recommendation: (a), `GET /oauth/authorize`, a
  confirmation page naming the app and device, the table with a 60-second lifetime.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q11. Refresh token lifetime and concurrency.** The plan gives none. (a) 30 days from the
  last rotation, no family cap; (b) 30 days sliding with a 90-day family cap; (c) 90 days
  sliding. Concurrent use of one token by the same app: revoke the family (strict reuse
  detection) or allow a 10-second grace for the token that was just rotated. Recommendation:
  (b) and strict, since the app can serialize its refreshes.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q12. Access token format and revocation.** The plan builds the server on `jsonwebtoken`, so
  access tokens are JWTs. Signing: (a) HS256 with a server key from Q3 (only this server
  verifies them); (b) ES256 with a published key (lets other services verify); Revocation: (a)
  a token lives until its 15-minute expiry after its family is revoked; (b) each request also
  checks that the family still exists (one indexed lookup). Recommendation: HS256 and the family
  check, so sign-out on a device takes effect at once.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q13. Error format on `/oauth/*`.** OAuth clients expect RFC 6749 section 5.2 JSON (`error`,
  `error_description`); the plan says API errors are `problem+json`. (a) RFC 6749 format on
  `/oauth/*` only; (b) `problem+json` with the OAuth error as `code`; (c) both fields in one
  body. Recommendation: (a); the Tauri app's OAuth library reads the standard shape.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q14. Audit entries.** The plan's `audit_events` table records actor and action, but no M0
  spec owns it. Should M0 write audit entries for sign-in, sign-out, refused sign-ins,
  service-account creation and deletion, and refresh-token reuse? (a) Yes, and this spec's
  issues add the table if no earlier one has; (b) logs only in M0, audit entries in M4;
  Recommendation: (a) for service accounts and reuse detection, (b) for routine sign-ins.
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **I1. Auth foundations: clock, tokens, hashing, encryption.** Adds the controllable clock to
   `AppState` (unless present), token generation and hashing (Q2), and the AES-256-GCM key ring
   with keys from configuration (Q3). Covers B1 to B4; T1 (generation half), T2, T3 (the
   crate's own types and errors). Depends on: Q2, Q3 answered; `01-repo-and-ci.md` (CI gates).
2. **I2. Browser sessions.** Migration for `sessions`; create, look up, refresh and delete;
   the request extractor that authenticates by cookie; idle and absolute expiry; suspension
   hook. Check in this issue that Playwright's Chromium accepts a `__Host-` cookie on the
   Compose stack's origin; if it does not, the e2e stack serves HTTPS. Covers B5 to B11; T1
   (database half: no plaintext token in any token column after a sign-in), T4 to T10. Depends on: I1, `03-users-and-identities.md` (users table, status), Q1, Q4, Q5.
3. **I3. CSRF layer and sign-out.** The tower layer of B14 to B16, the GET-mutation marker and
   router walk, `POST /auth/sign-out`. Covers B12 to B16; T11 to T17. Depends on: I2, Q6.
4. **I4. Sign-in flow state.** Migration for `auth_flows` (pending Q8), the flow cookie,
   create/consume/expire, `return_to` validation, cleanup. Covers B17 to B21; T18, T19. Depends
   on: I1, I2, Q8.
5. **I5. Rate limiting.** The limiter for sign-in and token routes keyed by client IP and account
   identifier, trusted-proxy client IP, 429 responses. Covers B22, B23; T20. Depends on: I3, Q7.
6. **I6. Service accounts and API keys.** Migration for `service_accounts`, the admin endpoints,
   bearer API-key authentication with the CRC-32 precheck, scope intersection per request.
   Covers B24 to B29; T3 (API-key request part), T21 to T26. Depends on: I2, `09-permissions-and-roles.md` (catalog,
   `Require<P>`, effective permissions), Q9.
7. **I7. OAuth 2.1 authorization endpoint and codes.** Client registry, `GET /oauth/authorize`,
   the sign-in round trip through a flow, code issuance and storage. Covers B30, B31; T27 to
   T29. Depends on: I4, `07-admission.md` I1 (sign-in completion returns to the flow), Q10.
8. **I8. OAuth 2.1 token and revoke endpoints.** Migration for `oauth_refresh_tokens`; code
   exchange with PKCE, JWT access tokens, rotation, reuse detection, revocation, access-token
   authentication in the extractor. Covers B32 to B37; T3 (full sign-in, API-key request and
   token exchange), T30 to T34. Depends on: I5, I7, Q11 to
   Q13.
9. **I9. `/.well-known/stepclimb`.** The discovery document's auth fields. Covers B38; T35, T36.
   Depends on: I8, `03-users-and-identities.md` (`identity_providers`), `04-local-accounts.md`
   I1 (local method switches).
