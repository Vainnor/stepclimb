# OpenID Connect sign-in

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/technical.md` -> Identity and access -> Sign-in providers (OpenID
  Connect: discovery, PKCE, ID token validation), Admission; Architecture and stack (auth
  libraries: `openidconnect`); API surface (`/auth/{provider}/start|callback`); Data model
  (`identity_providers`, `identities`); Security and privacy; Testing
- **Depends on:** `03-users-and-identities.md` (`identity_providers`, `identities`),
  `06-oauth2-and-vatsim-preset.md` (the mock IdP, I1; provider administration, I5),
  `07-admission.md` (I1), `08-sessions-and-csrf.md` (encryption, flow state, sessions, rate
  limits)
- **Review:** maintainer line-by-line

## Summary

An organization can let people sign in with any OpenID Connect provider (Google, Entra,
Authentik, Keycloak and others) by entering its issuer URL, a client id and a client secret.
Stepclimb reads the provider's discovery document, runs the authorization code flow with PKCE,
validates the ID token with the `openidconnect` crate, maps claims into an identity
`(provider, subject)` and its attributes, and hands the result to admission
(`07-admission.md`).

## Behavior

### Provider configuration

- **B1.** An OIDC provider is an `identity_providers` row with `kind = 'oidc'`, a `key` unique in
  the organization (used in `/auth/{key}/...`), a `label`, `enabled`, and a `config` holding:
  `issuer` (URL), `client_id`, the client secret (storage pending Q2), `scopes` (always
  including `openid`; default `["openid", "email", "profile"]`), and `claims`, the mapping of
  B13. Saving a config that lacks `issuer` or `client_id`, has a non-URL `issuer`, omits
  `openid` from `scopes`, or has an invalid JSON Pointer answers 422 with the field named
  (endpoint in `06-oauth2-and-vatsim-preset.md` B14).
- **B2.** The issuer URL and every URL the server fetches for the provider follow the outbound
  URL policy (pending Q4). Every outbound request has a timeout (pending Q4).
- Coordinator decision (2026-11-14), for I1 (`stepclimb_auth::oidc_config`): the config also
  holds `userinfo` (Q3, default `true`); any other member, `claims.subject` included (B12), is
  refused as `provider.field_unknown`, so a misspelled one is not silently dropped. The
  `client_secret` is required when the row has none stored, since both methods of Q6 need it.
  `issuer` is kept exactly as written (B4 compares it character for character) and must be an
  absolute `http(s)` URL with no query, fragment, whitespace or control character. A pointer
  must start with `/`; the empty pointer, which names the whole claims object, is refused.
  Each problem is a JSON Pointer and a code (`provider.field_required`, `provider.field_unknown`,
  `provider.field_type`, `provider.field_empty`, `provider.url_invalid`,
  `provider.url_refused`, `provider.scopes_missing_openid`, `provider.scope_invalid`,
  `provider.pointer_invalid`, `text.contains_nul`), the `errors` of `06` B14's 422. The stored
  secret is `client_secret: {"sealed": "<base64>"}`, sealed to
  `[org_id, "identity_providers", provider_id, "client_secret"]`. T1's 422 itself arrives with
  the admin API (`06` I5); I1 tests the validation it returns.
- **B3.** A provider that is disabled, belongs to another organization, or does not exist is
  "not found" for every `/auth/{key}/...` request, which redirects to the sign-in route with
  `error=auth.provider_not_found`.

### Start

- **B4.** `GET /auth/{key}/start?return_to=<path>` loads the provider's discovery document from
  `<issuer>/.well-known/openid-configuration` (cached per Q5). The document's `issuer` must
  equal the configured issuer exactly; otherwise, or if the fetch fails, the request redirects
  to the sign-in route with `error=auth.provider_unavailable`. Multi-tenant issuers such as a
  shared Entra endpoint: pending Q7.
- **B5.** The server creates a sign-in flow (`08-sessions-and-csrf.md` B17) holding a random
  `state`, a random `nonce`, a PKCE code verifier and `return_to`, and answers 303 to the
  provider's `authorization_endpoint` with `response_type=code`, `client_id`, `redirect_uri`,
  `scope`, `state`, `nonce`, `code_challenge` and `code_challenge_method=S256`. PKCE `plain` is
  never used.
- **B6.** The `redirect_uri` is the organization's origin followed by `/auth/{key}/callback`
  (origin pending `08-sessions-and-csrf.md` Q1). The admin page shows it so the admin can
  register it at the provider.

### Callback

- **B7.** `GET /auth/{key}/callback` requires the flow cookie and a `state` query parameter
  equal to the flow's `state` (compared in constant time). A missing cookie, an unknown or used
  flow, a flow for another provider, or a different `state` redirects with
  `error=auth.flow_invalid`; an expired flow with `error=auth.flow_expired`. This blocks a
  callback started in another browser (login CSRF).
- **B8.** A callback carrying `error=access_denied` redirects with `error=auth.provider_denied`;
  any other `error` value with `error=auth.provider_error`. The provider's `error_description`
  is logged (truncated to 200 characters) and never shown to the user.
- **B9.** The server exchanges `code` at the discovery document's `token_endpoint` with
  `redirect_uri`, the PKCE `code_verifier` and client authentication (method pending Q6). A
  failed exchange (non-2xx, timeout, malformed body, no `id_token`) redirects with
  `error=auth.provider_error`.
- **B10.** The ID token is accepted only when all of these hold; otherwise the callback
  redirects with `error=auth.id_token_invalid`:
  1. its signature verifies with a key from the provider's `jwks_uri` whose `kid` matches (an
     unknown `kid` triggers one JWKS refetch, then fails), using an accepted algorithm (pending
     Q8); `alg: none` is always refused;
  2. `iss` equals the configured issuer;
  3. `aud` contains the `client_id`, and when `aud` has more than one value, `azp` equals the
     `client_id`;
  4. `exp` is after now and `iat` is not after now, both within the clock skew of Q5, read from
     the controllable clock;
  5. `nonce` equals the flow's nonce;
  6. when the token carries `at_hash`, it matches the access token.
- **B11.** Whether the server also calls the `userinfo_endpoint`: pending Q3. When it does, the
  userinfo `sub` must equal the ID token's `sub`, or the callback redirects with
  `error=auth.provider_error`. Userinfo claims are merged over ID token claims, except `sub`,
  `iss` and `aud`, which always come from the ID token.
- **B12.** The identity's subject is the ID token's `sub`, a non-empty string, and is not
  configurable. Identities are `(provider_id, subject)`; the email address never identifies a
  person (`07-admission.md` B6).
- **B13.** The claims mapping reads values from the merged claims object by JSON Pointer (RFC
  6901), the same notation as `06-oauth2-and-vatsim-preset.md` B6. Defaults: `given_name =
  "/given_name"`, `family_name = "/family_name"`, `email = "/email"`, `email_verified =
  "/email_verified"`; `groups` has no default; `attributes.<name>` entries are added by the
  admin. A pointer that matches nothing leaves the field absent. A profile field whose value is
  not a string (or for `email_verified`, not a boolean) is treated as absent. An attribute keeps
  the JSON value it points to (string, number, boolean, array or object).
- **B14.** The callback passes the mapped sign-in to admission (`07-admission.md` B1). On
  admission it creates the session (`08-sessions-and-csrf.md` B7), deletes the flow in the same
  transaction, and answers 303 to the flow's `return_to`. On refusal it redirects as
  `07-admission.md` B2 says.
- **B15.** The provider's access token, refresh token and ID token are used during the callback
  and then dropped. None is stored or logged.
- **B16.** The start and callback routes are rate limited (`08-sessions-and-csrf.md` B22).

## API

Both routes have `#[utoipa::path]` (tag `auth`) and appear in `openapi/stepclimb.json`. They are
browser navigations, so failures are 303 redirects to the web app's sign-in route
(`11-app-shell.md`) with an `error` query parameter, not `problem+json` bodies; the OpenAPI
document lists the codes in each operation's description.

| Method and path | Request | Response | Error codes (in the redirect) | Permission |
| --- | --- | --- | --- | --- |
| `GET /auth/{provider}/start` | path `provider` = `identity_providers.key`; query `return_to?` | 303 to the provider's authorization endpoint; sets the flow cookie | `auth.provider_not_found`, `auth.provider_unavailable`, `auth.rate_limited` | none |
| `GET /auth/{provider}/callback` | query `code, state` or `error, error_description?, state` | 303 to `return_to`; sets `__Host-sc_session`; clears the flow cookie | `auth.provider_not_found`, `auth.flow_invalid`, `auth.flow_expired`, `auth.provider_denied`, `auth.provider_error`, `auth.id_token_invalid`, `auth.rate_limited`, and the admission refusals of `07-admission.md` | none |

The same two routes serve generic OAuth 2 providers (`06-oauth2-and-vatsim-preset.md`); the
handler picks the flow by the provider's `kind`. Provider administration
(`/api/v1/admin/providers`) is in `06-oauth2-and-vatsim-preset.md`, which validates `oidc`
configs with this spec's B1.

## Data

No new table. This spec defines the `config` shape for `identity_providers` rows with `kind =
'oidc'` (`03-users-and-identities.md` owns the table, which carries `org_id`):

```json
{
  "issuer": "https://idp.example.test/realms/acme",
  "client_id": "stepclimb",
  "client_secret": "<pending Q2: encrypted value or a file reference>",
  "scopes": ["openid", "email", "profile"],
  "claims": {
    "given_name": "/given_name",
    "family_name": "/family_name",
    "email": "/email",
    "email_verified": "/email_verified",
    "groups": "/groups",
    "attributes": { "department": "/department" }
  }
}
```

It writes `identities` (`subject` = `sub`, `attributes` = mapped attributes) through admission,
and `auth_flows` rows (`08-sessions-and-csrf.md`, pending its Q8). Discovery documents and JWKS
are cached in memory (Q5), not in the database.

## Permissions

Start and callback need no permission: they run before sign-in. Configuring a provider needs
`perm::ProviderManage` (`06-oauth2-and-vatsim-preset.md`). Entitlement gating
(`oidc_providers`): pending Q1.

## Test cases

Every test runs against the mock IdP of `06-oauth2-and-vatsim-preset.md` I1 on `127.0.0.1`,
never the network. Database tests use `#[sqlx::test]`. Expiry tests use the controllable clock.

- **T1.** (B1) Saving configs without `issuer`, with `issuer` = `not a url`, without `openid` in
  `scopes`, or with the pointer `given_name` (no leading `/`) answers 422 naming the field.
- **T2.** (B3) Start with an unknown key, a disabled provider, or organization B's provider key
  on organization A redirects with `error=auth.provider_not_found` and creates no flow
  (cross-organization case).
- **T3.** (B4) A discovery document whose `issuer` differs from the configured issuer by a
  trailing slash redirects with `error=auth.provider_unavailable`; so does a discovery endpoint
  answering 500.
- **T4.** (B5) The authorization redirect carries `code_challenge_method=S256`, a
  `code_challenge` equal to the S256 challenge of the stored verifier, a `state` and a `nonce`;
  the mock IdP refuses the token request if the verifier does not match, and the sign-in
  succeeds, which proves the verifier is sent.
- **T5.** (B7) A callback with no flow cookie, with another flow's cookie, or with a `state`
  differing by one character redirects with `error=auth.flow_invalid` and creates no session.
- **T6.** (B7) A callback after the flow lifetime (clock moved past it) redirects with
  `error=auth.flow_expired`; replaying a successful callback redirects with
  `error=auth.flow_invalid`.
- **T7.** (B8) `error=access_denied` gives `auth.provider_denied`; `error=server_error` gives
  `auth.provider_error`; the description text is absent from the redirect.
- **T8.** (B9) A token endpoint answering 500, timing out (paused time), or returning no
  `id_token` gives `auth.provider_error`.
- **T9.** (B10) Each mock-issued bad ID token gives `auth.id_token_invalid`: a signature from a
  key not in the JWKS, `alg: none`, `iss` of another issuer, `aud` without the client id, two
  audiences with `azp` of another client, `exp` in the past by more than the skew, `iat` in the
  future by more than the skew, a different `nonce`, a wrong `at_hash`.
- **T10.** (B10) After the mock rotates its signing key, the first sign-in with the new `kid`
  refetches the JWKS and succeeds; a `kid` absent after refetch fails.
- **T11.** (B11) With userinfo on (Q3), a userinfo `sub` different from the ID token's gives
  `auth.provider_error`; a userinfo `email` overrides the ID token's.
- **T12.** (B12, B13) The created identity has `subject` = `sub`; default mappings fill the
  user's names and email; a custom `attributes.department = "/department"` stores the claim; a
  pointer to a missing claim leaves the attribute absent; a numeric `given_name` is ignored.
- **T13.** (B14) A full sign-in ends with a `__Host-sc_session` cookie, a 303 to the
  `return_to` path, and the flow row deleted; `GET /api/v1/me` with the cookie returns the user.
- **T14.** (B14) An admission refusal (sign-up off, new person) redirects with
  `error=auth.signup_closed` and sets no session cookie.
- **T15.** (B15) After a sign-in, a search of every text and bytea column the test can read
  (sessions, identities, flows, users) finds none of the mock's access token, refresh token or
  ID token strings; a captured log contains none of them.
- **T16.** (B2) A provider whose issuer violates the outbound URL policy is refused on save
  (exact cases per Q4).
- **T17.** Contract: both routes appear in `openapi/stepclimb.json` with their error codes.

The start and callback routes are not protected routes (they run before sign-in), so the
four-case permission test does not apply to them; T2 is their cross-organization test.

## Out of scope

- Generic OAuth 2 providers, the provider admin API, the TOML provider file and the mock IdP:
  `06-oauth2-and-vatsim-preset.md`.
- Who may sign in, linking by email, just-in-time creation: `07-admission.md`.
- RP-initiated logout at the provider, front- or back-channel logout: the plan does not ask for
  them.
- Keeping provider tokens to call the provider's APIs later: the data model has no place for
  them.
- `private_key_jwt` client authentication: the plan does not ask for it.
- SAML 2.0: 1.x.

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Entitlement gating of sign-in methods.** The plan does not say whether a sign-in method
  kind is a feature; the M0 feature catalog is `10-entitlements-framework.md` Q1, whose option A
  names `oidc_providers` (this spec) and `oauth2_providers` (06). For this spec: (a) check
  `Require<Feature>` for `oidc_providers` on provider creation and change only, so a downgrade
  never locks existing members out of sign-in; (b) also at `/auth/{key}/start`; (c) no check.
  Recommendation: (a) if 10 Q1 picks A, (c) otherwise. Every feature is on for self-hosted.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Where provider client secrets come from.** (a) Entered through the admin API and stored
  in `identity_providers.config` encrypted with AES-256-GCM (`08-sessions-and-csrf.md` B3),
  write-only in the API; (b) a reference to a `_FILE` path or `STEPCLIMB_*` variable, never in
  the database; (c) both: (b) for providers defined in `stepclimb.toml`, (a) for providers
  created through the admin API. Recommendation: (c).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Userinfo.** (a) ID token claims only; (b) always call userinfo when discovery lists a
  `userinfo_endpoint`; (c) a per-provider `userinfo` flag, on by default when the endpoint
  exists. Some providers put groups or email only in userinfo. Recommendation: (c).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. Outbound URL policy and timeouts.** Admins enter URLs the server then fetches (issuer,
  discovery endpoints, the admission webhook), which in multi-organization or managed mode lets
  one organization's admin make the server request internal addresses. (a) `https` only, and
  in multi-organization mode refuse hosts that resolve to loopback, private, link-local or
  unique-local addresses, checked at connect time; (b) `https` only, no address checks; (c) an
  instance-level allowlist of hosts. Tests and the Compose e2e stack need `http://127.0.0.1`:
  allow `http` and loopback only when an instance setting (for example
  `STEPCLIMB_ALLOW_INSECURE_PROVIDERS`) is set, which the test setup sets. Timeouts: 5
  seconds per provider request (the webhook keeps its 3 seconds). Recommendation: (a) with the
  insecure switch and 5 seconds.
  **Decided:** the recommendation (owner, 13 Nov 2026).
  Coordinator decision (2026-11-14), for I1 (`stepclimb_auth::outbound`, ADR 0025): the switch is
  `STEPCLIMB_ALLOW_INSECURE_PROVIDERS` (`true` or `false`, default `false`). Loopback addresses
  need the switch on every instance; private and unique-local addresses (plus `100.64.0.0/10`,
  `198.18.0.0/15`, `192.0.0.0/24`, `fec0::/10`) are refused on a multi-organization instance;
  link-local (`169.254.0.0/16`, where cloud metadata services listen, and `fe80::/10`), the
  metadata addresses inside private ranges (`100.100.100.200`, `fd00:ec2::254`), Teredo
  (`2001::/32`), unspecified, multicast, reserved and documentation addresses are always refused.
  An IPv6 address carrying an IPv4 one (mapped, NAT64, 6to4) is judged by that IPv4 address. A
  host name is refused when any address it resolves to is refused, and the connection uses only
  the checked addresses. Saving checks the scheme, credentials in the URL (refused) and an
  address written as the host, without DNS. No outbound request follows a redirect or uses a
  proxy, providers included, as the `openidconnect` crate's documentation advises against
  following redirects. The 5 s covers the whole exchange, body included, and a provider response
  body over 1 MiB is refused.
- **Q5. Caching and clock skew.** The plan does not say how often discovery and JWKS are
  fetched. (a) In memory per provider for 1 hour, a JWKS refetch on an unknown `kid` at most
  once per minute, 60 seconds of clock skew; (b) fetch on every sign-in, no cache; (c) honor
  HTTP cache headers. Recommendation: (a); a cache per process keeps the server stateless.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. Client authentication at the token endpoint.** (a) `client_secret_basic` unless the
  discovery document lists only `client_secret_post`; (b) a per-provider setting; (c) always
  `client_secret_post`. Recommendation: (a), with (b) as an override if a provider misbehaves.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. Multi-tenant issuers.** A shared Entra endpoint publishes an issuer containing a tenant
  placeholder, so the exact match of B4 and B10 fails. (a) Support only tenant-specific
  issuers in M0 (one Entra tenant per provider row); (b) accept a configured issuer template
  and check the ID token's `tid` against an allowlist of tenants; (c) skip the issuer check for
  such providers. Recommendation: (a); (c) would let any tenant's users present tokens.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. Accepted ID token algorithms.** (a) The asymmetric algorithms the `openidconnect` crate
  supports that the discovery document lists (RS256 at least, as OIDC Core requires providers
  to support), refusing HS256; (b) also HS256 keyed by the client secret, as OIDC Core allows;
  (c) a per-provider list. Recommendation: (a).
  **Decided:** the recommendation (owner, 13 Nov 2026).

## Issues

1. **I1. OIDC provider config and outbound HTTP.** The `oidc` config type, its validation (B1),
   client secret handling (Q2), and the outbound HTTP client used by every provider and the
   admission webhook: URL policy, timeouts, no redirects followed for the webhook (Q4). Covers
   B1, B2; T1, T16. Depends on: `03-users-and-identities.md` (`identity_providers`),
   `08-sessions-and-csrf.md` I1 (encryption), Q2, Q4.
2. **I2. Discovery and the start route.** Adds the `openidconnect` dependency (exact pin, at
   least 14 days old, `just deny` clean); discovery with caching and the issuer check;
   `GET /auth/{provider}/start` creating the flow with state, nonce and PKCE; the
   `redirect_uri` rule. Covers B3 to B6, B16; T2 to T4. Depends on: I1,
   `08-sessions-and-csrf.md` I4 and I5, `06-oauth2-and-vatsim-preset.md` I1 (mock IdP), Q5,
   Q7. This issue also confirms how the crate takes a time source for B10's checks and records
   the answer in the PR.
3. **I3. Callback: code exchange and ID token validation.** State and flow checks, provider
   errors, the token exchange, every ID token check of B10 against the controllable clock, JWKS
   refetch, userinfo per Q3. Covers B7 to B11; T5 to T11. Depends on: I2, Q3, Q6, Q8.
4. **I4. Claim mapping and sign-in completion.** JSON Pointer mapping into the admission input,
   the hand-off to `07-admission.md`, session creation, the final redirect, dropping provider
   tokens. Covers B12 to B15; T12 to T15, T17. Depends on: I3, `07-admission.md` I1,
   `06-oauth2-and-vatsim-preset.md` I2 (shared JSON Pointer mapping).
