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
  looks a token up by the SHA-256 of the presented value; in-process comparisons of hashes are
  constant time (coordinator decision, 13 Nov 2026). Token length, encoding and hash function:
  pending Q2.
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
  existing session token, so a token planted before sign-in is never promoted. Before it creates
  the new session, a sign-in deletes the session of the request's organization that each
  `__Host-sc_session` cookie the browser presented names, live or expired, so the old cookie's
  session does not stay alive (coordinator decision, 13 Nov 2026).
- **B8.** On each request carrying the cookie the server hashes the token and looks up the
  session through its user, within the request's organization. The session is valid when the
  row exists, `now < expires_at` (30 days at most) and `now < last_seen_at + 12 hours` (12 hours
  idle).
- **B9.** A valid session refreshes `last_seen_at`. How often the write happens: pending Q4.
- **B10.** An expired or idle session is deleted, the response clears the cookie
  (`Max-Age=0`), and a route that needs authentication answers 401 `auth.session_expired`. A
  cookie whose hash matches no row is cleared and answers 401 `auth.unauthenticated`. A request
  carrying two or more `__Host-sc_session` cookies, or one whose value is not ASCII, is treated
  the same way without a lookup: 401 `auth.unauthenticated` and the cookie cleared. Other
  cookies in the `Cookie` header, ASCII or not, never hide the session cookie (coordinator
  decision, 13 Nov 2026).
- **B11.** The request authenticator reads the user's `status` on every request
  (`03-users-and-identities.md` B7). A session, API key or access token whose user is suspended
  authenticates nothing and the request answers 401 `auth.suspended`. The session row is kept,
  so unsuspending before it expires restores it; a refresh token of a suspended user is refused
  at `/oauth/token` with `invalid_grant`. Expiry is checked before suspension: an expired or idle
  session of a suspended user is deleted and answers `auth.session_expired` per B10, and a
  suspended user's session is not refreshed, so it still idles out after 12 hours (coordinator
  decision, 13 Nov 2026).
- **B12.** Safe methods (GET, HEAD, OPTIONS) change no state other than `last_seen_at` and the
  sign-in flow rows of B17, so a `SameSite=Lax` cookie sent on a cross-site top-level GET can
  do nothing.
- **B13.** `POST /auth/sign-out` deletes the request's session row and clears the cookie. It
  answers 204 whether or not a session existed. When it carries a session cookie and the
  request's organization cannot be resolved (B5), it answers that error instead, 404
  `org.not_found` or 503 `org.none`, deletes nothing and leaves the cookie, since it cannot tell
  whose session to end; without a session cookie it answers 204 without resolving (coordinator
  decision, 13 Nov 2026).

### CSRF

- **B14.** Every request with an unsafe method (POST, PUT, PATCH, DELETE) that does not carry an
  `Authorization` header passes the CSRF check before any handler runs, whether or not it
  carries a session cookie (sign-in POSTs are covered too). The check passes when
  `Sec-Fetch-Site` is `same-origin`, or when `Sec-Fetch-Site` is absent and the `Origin` header
  is on the organization's Origin allowlist. `Sec-Fetch-Site` values `same-site`, `cross-site`
  and `none` fail. An unsafe request that carries an `Origin` header must also have that origin
  on the allowlist, even when `Sec-Fetch-Site` is `same-origin`: a page on a name an attacker
  re-points at the server (DNS rebinding) is same-origin with itself, but its `Origin` is the
  attacker's name (coordinator decision, 13 Nov 2026). A failure answers 403
  `auth.csrf_refused`.
- **B15.** What happens when both `Sec-Fetch-Site` and `Origin` are absent, and where the Origin
  allowlist comes from beyond the organization's own origin: pending Q6.
- **B16.** Requests authenticated by an `Authorization: Bearer` header (API keys, app access
  tokens, the `/oauth/token` and `/oauth/revoke` form posts) skip the CSRF check: a browser
  never attaches those credentials on its own. A request that carries both a bearer header and
  a session cookie is authenticated by the bearer only. Only the `Bearer` scheme (in any case)
  skips the check in B14; another scheme such as `Basic`, which a browser resends on its own to
  a site behind a proxy that asks for it, does not, and does not hide the session cookie. Sign-out
  with a bearer header ends no session (I3's reading of B14 and B16, 13 Nov 2026; the owner can
  overrule it).

### Transport security

- **B39.** When the configured public URL (Q1, `STEPCLIMB_PUBLIC_URL`) is `https`, every
  response carries `Strict-Transport-Security: max-age=31536000` (one year, without
  `includeSubDomains`: subdomains of the public host may be other services of the operator's).
  The server never terminates TLS itself and trusts no proxy header until I5, so the
  configuration, not the request, says the instance is served over HTTPS. With an `http`
  public URL the header is not sent (plan, Security and privacy: HSTS; coordinator decision,
  13 Nov 2026). The public URL is required configuration, since a default would silently turn
  HSTS off: the server refuses to start without it. `STEPCLIMB_ALLOWED_ORIGINS` (Q6) is an
  optional comma-separated list of the further origins an `Origin` header may name, which is
  what browsers that send no `Sec-Fetch-Site` are accepted by (I3's choices, 13 Nov 2026).
  Development (`just serve`, `.env.example`) lists the Vite dev server's origin,
  `http://localhost:5173`, so `just dev` works; nothing lists it by default. A configuration
  error about either setting names the setting and the problem but never repeats the value,
  which can carry a password (coordinator decision, 13 Nov 2026).
- **B43.** Coordinator decision (2026-11-13): every response to a request under `/api/v1`
  carries `Cache-Control: no-store`, answers, errors and refusals alike (the CSRF refusal, an
  unknown path, a wrong method included), because a GET under `/api/v1` can answer personal data
  (`GET /api/v1/me`) that neither a shared cache nor the browser's may keep. One layer around the
  whole router sets it by the request's path, so a new route cannot leave it out. The probes, the
  API document (`/api/openapi.json`, `/docs`) and the SPA keep their own caching. Delivered with
  `03-users-and-identities.md` issue 5.

### Sign-in flow state

- **B17.** A sign-in that spans more than one request (an OIDC or OAuth 2 redirect, a WebAuthn
  ceremony, password then TOTP, an OAuth 2.1 authorization) keeps its state on the server in a
  sign-in flow, bound to the browser by a random flow token. Storage, cookie and lifetime:
  pending Q8 (recommended: an `auth_flows` table and a `__Host-sc_flow` cookie, 10 minutes).
- **B18.** A flow is single use: the request that completes it deletes it in the same
  transaction that creates the session (or authorization code). A second use answers
  `auth.flow_invalid`. An expired flow answers `auth.flow_expired`.
  An OIDC callback takes its flow before it contacts the provider, in a statement of its own
  (`05-oidc.md` I3's coordinator decision, 2026-11-15).
- **B19.** A flow records a `return_to` path. The server accepts only a path that starts with
  one `/` and is not `//` or `/\`, so a sign-in can never redirect to another host; anything
  else becomes `/`. A path is also kept only when every byte is printable ASCII and it is at
  most 2,048 bytes: browsers drop tab and newline from a URL, so `/<tab>/evil.example` would
  reach them as `//evil.example` (I4's reading of B19, 13 Nov 2026; the owner can overrule it).
  Coordinator decision (2026-11-14), failing closed: a path with a `.` or `..` segment in its
  path part (before `?` or `#`), written literally or with `%2e` in either case, and with `/` or
  `\` as the separator, also becomes `/`. Browsers resolve those segments, so `/a/..//evil.example`
  would reach them as `//evil.example`. The web app applies the same rule to the return target it
  carries to `/sign-in` (11 B15).
- **B20.** Flow payloads that hold secrets (PKCE verifier, OIDC nonce, WebAuthn ceremony state)
  are encrypted per B3.
- **B21.** Expired flows are deleted by the request that finds them and by a periodic cleanup;
  the cleanup's schedule rides on the jobs runner when it exists (M1), and until then on a
  `tokio` interval in the server. Pending Q8. The server's interval is 10 minutes; a failure in
  one organization is logged and the pass continues with the next, and the server logs an error
  if the cleanup task ever stops. The flow cookie's `Max-Age` is twice the flow's lifetime, so a
  user who comes back late still presents it and hears `auth.flow_expired` rather than
  `auth.flow_invalid`; the server alone decides expiry (coordinator decision, 13 Nov 2026).

### Rate limits

- **B22.** The server rate-limits the sign-in endpoints (`/auth/...` start and finish routes,
  password and TOTP checks), `POST /oauth/token` and `POST /oauth/revoke`. A limited request
  answers 429 `auth.rate_limited` with a `Retry-After` header in seconds. Limits, keys (client
  IP, account identifier), storage and how the client IP is found behind a proxy: pending Q7.
  Coordinator decision (2026-11-15; the owner to confirm): a route the browser opens as a
  top-level navigation (`GET /auth/{provider}/start`, `05-oidc.md` B16) answers its limit with
  a 303 to the web app's sign-in route with `error=auth.rate_limited` and the same
  `Retry-After`, since a navigation would otherwise show a raw problem body.
- **B23.** Failed password and TOTP checks count against the account identifier as well as the
  client IP, so one address cannot try many passwords for one account and many addresses cannot
  share one account's budget (numbers pending Q7).
- **B40.** How I5 builds Q7 (I5's choices, 13 Nov 2026; the owner can overrule them). Each limit
  is a fixed window that opens at the first request it counts: 20 sign-in requests a minute and
  60 token or revoke requests a minute per client address, and 10 failed password or TOTP checks
  per account identifier in an organization per 15 minutes, set by `STEPCLIMB_SIGN_IN_RATE_LIMIT`,
  `STEPCLIMB_TOKEN_RATE_LIMIT` and `STEPCLIMB_FAILED_SIGN_IN_LIMIT`. A check counts as a failure
  from the moment it starts until it succeeds, so parallel guesses cannot all pass before the
  first failure is recorded, and an account over its budget is refused without running the
  check. An IPv6 client is counted by its /64. The client address is the TCP peer, unless the
  peer is listed in `STEPCLIMB_TRUSTED_PROXIES` (addresses and CIDR ranges no wider than /8 for
  IPv4 or /32 for IPv6, none by default; a trusted range must hold proxies only, never clients):
  then it is the right-most `X-Forwarded-For` entry that is not a trusted proxy, or the nearest
  proxy when that entry is not an address. No other forwarding header is read. The production
  Compose example trusts Caddy's fixed address. An account is stored as the SHA-256 of its
  organization id and identifier, so a long identifier costs the limiter 32 bytes, and closed
  windows are swept out within about a minute.
- **B41.** Every route that adds a sign-in step (the `/auth/...` start and finish routes, password
  and TOTP checks, in `04-local-accounts.md` to `07-admission.md`) takes `RateLimited<SignIn>`,
  and `POST /oauth/token` and `POST /oauth/revoke` take `RateLimited<Token>`, as their first
  extractor. The issue that adds the first such route also adds a router walk, like T17's, that
  fails when a route under `/auth/` or `/oauth/` lacks it, so a route cannot skip its limit
  silently. `04-local-accounts.md` I2 added the first, `POST /auth/local/enrollment/start`, and
  the walk (`tests/enrollment.rs`): with a budget of one request, every such route but
  `/auth/sign-out`, which adds no sign-in step, must answer 429 to its second POST.
  `04-local-accounts.md` I6 widened the walk to every operation that documents a 429, which
  takes in `PUT /api/v1/me/password`. A password or TOTP check passes `account_check` its account identifier through one
  normalizing type shared by the sign-in methods (for example a newtype built only by the
  function that matches accounts), so `Ada@example.org` and `ada@example.org` cannot get two
  budgets (security review of I5, 13 Nov 2026).
- **B42.** Coordinator decision (2026-11-13), for M0, to stand as B40 describes:
  - The per-account budget refuses the right password too while it is spent, so anyone who
    knows an identifier can keep that account signed out for up to 15 minutes at a time. An
    exemption for a browser that signed in to the account before (a device cookie) is deferred
    to the owner. Decided by the owner (13 Nov 2026, issue 26) and built by
    `04-local-accounts.md` I6: a successful password sign-in or password enrollment sets
    `__Host-sc_device` (`Path=/`, `Secure`, `HttpOnly`, `SameSite=Strict`, `Max-Age` 30 days),
    the user's id and the issue time sealed with the key ring and bound to the organization. A
    password check for that user from a browser presenting a valid one, younger than 30 days, is
    neither counted against nor refused by the account's budget; the per-address budget still
    applies. A cookie that does not open, is older, or names another user exempts nothing.
    Coordinator decisions (2026-11-15): the 30-day lifetime (a session's longest), and only the
    password routes set it, since only a password check has a per-account budget.
    Coordinator decisions (2026-11-15), from the security review, following OWASP's device-cookie
    scheme and failing closed, for the owner to confirm: each cookie carries a random device id
    and has a failed-check budget of its own (the account's numbers), which replaces the
    account's budget for that browser, so a stolen cookie gives an attacker no more guesses than
    the account had; and each cookie seals the password's generation
    (`local_logins.password_generation`, one more for each password set), so a change of
    password or a reset through an enrollment link revokes every older cookie.
  - Clients behind one address (a shared NAT, a host-run reverse proxy without the `tls`
    profile, rootless Docker without a port driver that keeps source addresses) share one
    per-address budget. The self-hosting guide documents this when it is written (M4); until
    then the production Compose file's header does.

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
- **B44.** How I6 builds Q9. Coordinator decision (2026-11-15), for the owner's review; the
  owner can overrule any of them:
  - Only a browser session creates a service account: `POST /api/v1/admin/service-accounts`
    answers any bearer request 403 `auth.session_required` before anything else, so a key
    cannot mint a key. The creator owns the account. Each scope is a catalog permission key,
    listed once (422 `request.invalid` with `service_account.scope_unknown` or
    `service_account.scope_duplicate` at `/scopes/<i>`), that the creator holds at some scope
    (403 `service_account.scope_exceeds_owner`). Holding a scope only at an account is enough:
    the intersection of B27 keeps the key's right at that account. An empty list is allowed;
    such a key can call `GET /api/v1/me`, `GET /api/v1/organization`, `PUT
    /api/v1/me/appearance` and `GET /api/v1/me/identities` and nothing else under `/api/v1`, which
    a router walk with such a key checks (`tests/service_accounts.rs`). A key with
    `service_account.manage` may list and delete service accounts, its own included.
  - `kind` has one value, `api_key`. `name` is trimmed, 1 to 200 characters, no NUL, as other
    names. `last_used_at` is written at most once a minute, like a session's `last_seen_at`
    (Q4), and the list answers it; there is no expiry in M0.
  - The bearer header must be the request's only `Authorization` header, `Bearer` in any case,
    one space, then the credential. A credential that starts with `sc_` is an API key and is
    checked by its shape and CRC-32 before the request's organization is resolved, so a bad
    checksum costs no query. Any other bearer credential answers 401 `auth.unauthenticated`
    until I8 adds app access tokens. A cookie beside a bearer header is never read or cleared.
    A page of the web app (`spa`) never authenticates a bearer header, so a page load does not
    look a key up or record its use.
  - A change an API key makes is audited with actor type `api_key` and the service account's
    id, with the client address as for a user. `audit_events` gains that type: an `api_key`
    actor has an id and an address, like a user. Deleting an owner deletes their service
    accounts by the foreign key's cascade, with no `service_account.delete` entry; accepted for
    M0, since M0 deletes no users.
  - A key cannot change how its owner signs in. `DELETE /api/v1/me/identities/{identity_id}`
    takes `SessionOnly`, as the passkey routes and `PUT /api/v1/me/password` do: any bearer
    request answers 403 `auth.session_required`. Every step of using an enrollment link
    (`POST /auth/local/enrollment/start`, `.../password` and `.../passkey`) refuses a bearer
    request the same way. On the admin routes (`PATCH /api/v1/users/{user_id}` with `email` or
    `status`, `POST` and `DELETE /api/v1/users/{user_id}/identities...`), a key whose target is
    its own owner answers 403 `auth.session_required`, whatever its scopes; the escalation rule
    (`03-users-and-identities.md` B6) still applies to other users. `PUT /api/v1/me/appearance`
    stays open to keys (a display choice only).
  - Only a browser session creates an enrollment link: `POST
    /api/v1/admin/users/{user_id}/enrollment-links` answers any bearer request 403
    `auth.session_required`, for any target user, since a link is a new sign-in credential.

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
  path, consent and code storage: pending Q10. Without a session, I7 carries the authorization
  request in the `return_to` of the sign-in flow the browser then starts (a path back to the
  authorization endpoint with its query), so flows never nest under the one `__Host-sc_flow`
  cookie name (coordinator decision, 13 Nov 2026).
- **B45.** How I7 builds B30, B31 and Q10. Coordinator decisions (2026-11-15), for the owner's
  review; the owner can overrule any of them:
  - **Clients.** One client in code, `stepclimb-app`, public (no secret), named "the Stepclimb
    app" on the confirmation page (`stepclimb_auth::oauth_server::clients`). Its redirect URIs
    are `dev.stepclimb.shell:/oauth/callback`, the private-use scheme of `apps/app`'s bundle
    identifier (RFC 8252 section 7.1), and the loopback URIs `http://127.0.0.1:<port>/oauth/callback`
    and `http://[::1]:<port>/oauth/callback`. A presented `redirect_uri` must equal a registered
    one character for character; the only exception is a loopback URI's port, which may be any
    port from 1 to 65535, as RFC 8252 section 7.3 and OAuth 2.1 ask for a desktop app's ephemeral
    listener. No other normalization: `localhost`, a trailing `/` or a query is a mismatch.
  - **Order of checks.** `GET /oauth/authorize` counts the sign-in budget (B41, a 303 to
    `/sign-in?error=auth.rate_limited` like B22's navigation routes), refuses any bearer request
    with 403 `auth.session_required` (an API key or app token never authorizes an app), then
    checks `client_id` and `redirect_uri`, each given exactly once: either wrong shows the HTML
    error page (400 `oauth.client_unknown` or `oauth.redirect_uri_mismatch`) and never
    redirects. After those, every problem is a 303 to the redirect URI with the RFC 6749 section
    4.1.2.1 `error`, a fixed `error_description` and the request's `state` (Q13 (a)): a missing
    `response_type` or a repeated parameter is `invalid_request`; any `response_type` but `code`
    (the implicit grant's `token` included) is `unsupported_response_type`; a missing or `plain`
    `code_challenge_method`, or a `code_challenge` that is not 43 base64url characters holding
    32 bytes, is `invalid_request`. So `oauth.pkce_required` is not a code: the RFC's
    `invalid_request` carries it. Other parameters, `scope` among them, are ignored (RFC 6749
    section 3.1).
  - **`state`.** Optional, as OAuth 2.1 allows with PKCE. When given it is returned exactly, and
    must be at most 256 bytes of RFC 6749's VSCHAR (`0x20` to `0x7E`), so the request always fits
    B19's 2,048-byte `return_to` after percent-encoding; otherwise `invalid_request` without a
    `state`.
  - **Without a session** the browser gets a 303 to `/sign-in?return_to=<the request>`, the path
    back to the endpoint with only the parameters it reads, in a fixed order. No flow is started
    there. The web app loads a return target under `/oauth/` as a new page after sign-in, since it
    has no route for it (`11-app-shell.md` B15). An expired or unknown session cookie is also
    cleared (B10); a suspended user's session counts as none.
  - **Consent (Q10).** With a session, the endpoint stores the checked request in an
    `oauth_authorize` sign-in flow for the user (B17, its payload sealed per B20) and answers 200
    with a confirmation page naming the app, the user (display name and email) and the
    organization, with "Allow" and "Cancel". The page names no device: the request carries no
    trustworthy device name, and a name from the query would be text an attacker chose on a page
    that asks for trust. The page is plain HTML the server writes, in English, with
    `Cache-Control: no-store`, `X-Frame-Options: DENY` and a `Content-Security-Policy` that loads
    nothing and forbids framing. Its policy has no `form-action`, which browsers apply to the
    redirect after the form is sent to the app's scheme. Its `Referrer-Policy` is `same-origin`:
    under `no-referrer` a browser sends `Origin: null` with the form post, which B14 refuses, so
    no one could answer (security review of I7, coordinator decision 2026-11-15). The error pages
    and every redirect of the endpoint are `no-referrer`. A page drawn with the web app's
    components through the API is left for later.
  - **The answer** is `POST /oauth/authorize` with one `decision`, `allow` or `deny`, and one
    `request`, the flow id the page carries in a hidden field, form encoded. It passes the CSRF check (B14) and the same budget and bearer refusal, needs the
    browser session (else a 401 page `auth.unauthenticated`), and in one transaction takes the
    flow (B18: a second answer or a late one shows `auth.flow_invalid` or `auth.flow_expired`),
    checks the flow is this user's and is the one the answered page was shown for (else
    `auth.flow_invalid`: opening the request again replaces the flow cookie, and the older page's
    answer then counts for nothing), and checks its client and redirect URI against the registry
    again. `deny` is a 303 to the redirect URI with
    `error=access_denied` and `state`; `allow` stores the code and is a 303 with `code` and
    `state`. Both clear the flow cookie.
  - **Issuer (RFC 9207).** Every answer the endpoint sends to a redirect URI, a code or an error,
    carries `iss`, the server's issuer identifier: `STEPCLIMB_PUBLIC_URL` as the discovery
    document's `oauth.issuer` writes it. The app must compare `iss` with the issuer it expects
    for the server it started the sign-in with, and refuse the answer when they differ or `iss`
    is missing; and when it links a server it must check that the document's `oauth.issuer`
    equals the URL it fetched the document from. That stops a mix-up between two servers the app
    is linked to (M5, `docs/specs/m5.md`). Security review of I7, coordinator decision
    2026-11-15.
  - **Codes** are `Token`s (Q2), stored as their SHA-256 in `oauth_authorization_codes` with the
    organization, user, client, redirect URI and challenge, and expire 60 seconds after issue
    (Q10). The periodic flow cleanup (B21) also deletes expired codes. Single use at the token
    endpoint is I8's.
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

  Coordinator decisions (2026-11-15), for issue I9, on the owner's delegation:

  - **The local entry.** `sign_in` lists local sign-in first, as one entry `{key: "local", kind:
    "local", label: null, methods: [...]}` whose `methods` are `LocalSignIn::offered()`
    (`04-local-accounts.md` B2) in the order `passkey`, `password`, `totp`, `magic_link`. It is
    absent when that list is empty: the local row disabled, or no primary method on (TOTP alone
    offers nothing). `totp` is listed only when `password` is listed too, since it is the second
    step after a password and signs no one in by itself: with the passkey and TOTP switched on and
    the password off, `methods` is `["passkey"]` (coordinator decision, 2026-11-15, after the
    security review of I9). `require_method` for TOTP does not yet require the password; issue
    #27 (`04-local-accounts.md` I7) owns that check. The outside providers follow, sorted by key, each exactly `{key, kind,
    label}`; provider entries never carry `methods`. An organization that offers nothing
    answers `sign_in: []`.
  - **`label`** is the string `label` member of the provider's config, and `null` when it has
    none or it is not a string, as `03-users-and-identities.md` B23 reads it. Local sign-in's
    labels are the web app's strings, so its `label` is always `null`.
  - **`oauth`** lists the OAuth 2.1 server's `issuer` (the public URL, B45) and the endpoints
    that exist, as absolute URLs on `STEPCLIMB_PUBLIC_URL`: I7 added `issuer` and
    `authorization_endpoint` (`<public URL>/oauth/authorize`);
    `token_endpoint` and `revocation_endpoint` join it with I8. Listing a URL that answers 404
    would point the M5 app at nothing.
  - **`theme`** (`11-app-shell.md` Q4) is `{stylesheet}`, the absolute URL
    `<STEPCLIMB_PUBLIC_URL>/theme.css?v=<SHA-256>` of the stylesheet `/theme.css` serves the
    organization, the same hash the served page links; a stored hash that is not 64 lowercase
    hex digits leaves the `?v=` out, as the page does.
  - **Caching.** The document answers `Cache-Control: no-cache`: a switched-off method or a saved
    branding change must show on the next read, and the document holds nothing personal.
  - **Organization.** `RequestOrg` picks it, so no organization answers 503 `org.none` and two or
    more 404 `org.not_found`, as every unauthenticated route does. Nothing in the document is an
    internal id.

## API

Every endpoint has `#[utoipa::path]` and appears in `openapi/stepclimb.json`. Errors are RFC
9457 `application/problem+json` unless noted (B37, Q13). Every endpoint can also answer 429
`auth.rate_limited` where B22 applies, and 403 `auth.csrf_refused` for unsafe methods per B14.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `POST /auth/sign-out` | none (cookie) | 204, cookie cleared | `auth.csrf_refused` | none |
| `GET /.well-known/stepclimb` | none | 200 `{name, sign_in: [{key, kind, label, methods?}], theme: {stylesheet}, oauth: {issuer, authorization_endpoint}}`; `token_endpoint` and `revocation_endpoint` join `oauth` with I8 (B38) | `org.not_found`, `org.none` | none |
| `GET /oauth/authorize` | query `response_type=code, client_id, redirect_uri, code_challenge, code_challenge_method=S256, state?` | 200 HTML confirmation page with the flow cookie; 303 to `/sign-in?return_to=...` without a session; 303 to `redirect_uri?error&error_description&state&iss` (B45) | HTML page 400 `oauth.client_unknown`, `oauth.redirect_uri_mismatch`; 403 `auth.session_required` | browser session (or sign-in first) |
| `POST /oauth/authorize` | form `decision=allow\|deny, request` (the confirmation page) | 303 to `redirect_uri?code&state&iss` or `?error=access_denied&state&iss` | HTML page 400 `request.malformed`, `auth.flow_invalid`, `auth.flow_expired`; 401 `auth.unauthenticated`; 403 `auth.session_required`, `auth.csrf_refused` | browser session |
| `POST /oauth/token` | form: `grant_type=authorization_code, code, redirect_uri, client_id, code_verifier` or `grant_type=refresh_token, refresh_token, client_id` | 200 `{access_token, token_type: "Bearer", expires_in: 900, refresh_token}` | `invalid_request`, `invalid_client`, `invalid_grant`, `unsupported_grant_type` (format Q13), `auth.rate_limited` | none (client is public) |
| `POST /oauth/revoke` | form: `token, token_type_hint?, client_id` | 200 | `invalid_request`, `auth.rate_limited` | none |
| `GET /api/v1/admin/service-accounts` | cursor pagination, by id | 200 page of `{id, name, kind, scopes, owner_user_id, created_at, last_used_at}`, never the key | `auth.unauthenticated`, `entitlement.feature_unavailable`, `permission.denied` | `Require<ApiKeys>`, `perm::ServiceAccountManage`, organization |
| `POST /api/v1/admin/service-accounts` | `{name, scopes: [string]}` | 201 `{id, ..., key}` (the only time `key` is returned) | `entitlement.feature_unavailable`, `permission.denied`, `service_account.scope_exceeds_owner` (403), `service_account.scope_unknown`, `service_account.scope_duplicate`, `name.empty`, `name.too_long` (422) | `Require<ApiKeys>`, `perm::ServiceAccountManage`, organization |
| `DELETE /api/v1/admin/service-accounts/{service_account_id}` | none | 204 | `entitlement.feature_unavailable`, `permission.denied`, `service_account.not_found` | `Require<ApiKeys>`, `perm::ServiceAccountManage`, organization |

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
As built by I6 (migration `20261115220000_service_accounts.sql`, B44): `kind text NOT NULL CHECK
(kind IN ('api_key'))`, `name text NOT NULL`, `owner_user_id uuid NOT NULL` with `(org_id,
owner_user_id) REFERENCES users(org_id, id) ON DELETE CASCADE` so the owner is in the account's
organization, `secret_hash` checked to 32 bytes, `last_used_at timestamptz NULL`, and an index on
`(org_id, owner_user_id)`. The same migration adds `api_key` to `audit_events.actor_type`.

**`auth_flows`** (Q8): `id uuid PK`, `org_id uuid NOT NULL REFERENCES organizations(id)`,
`token_hash bytea NOT NULL UNIQUE` (SHA-256 of the flow cookie value, 32 bytes), `kind text NOT
NULL` (`oidc`, `oauth2`, `passkey_register`, `passkey_sign_in`, `password_totp`,
`oauth_authorize`), `user_id uuid NULL`, with `(org_id, user_id) REFERENCES users(org_id, id) ON
DELETE CASCADE` so a flow's user is in the flow's organization, `payload bytea NOT NULL`
(AES-256-GCM, B20, sealed for `[org_id, "auth_flows", id, "payload"]`), `return_to text NOT NULL`
(checked against B19's rule, printable ASCII, at most 2,048 bytes), `expires_at timestamptz NOT
NULL`, `created_at`, `updated_at`. Index on `(org_id, expires_at)` for the per-organization
cleanup. `org_id` is direct because a flow exists before any user is known. `provider_id uuid
NULL`, with `(org_id, provider_id) REFERENCES identity_providers(org_id, id) ON DELETE CASCADE`,
was added by the first issue whose flow names a provider (`05-oidc.md` I2, migration
`20261115090000_auth_flow_provider.sql`), since `identity_providers`
(`03-users-and-identities.md` I2) did not exist when I4 landed.

**`oauth_authorization_codes`** (Q10, as proposed): `id uuid PK`, `user_id uuid NOT NULL
REFERENCES users(id) ON DELETE CASCADE`, `client_id text NOT NULL`, `code_hash bytea NOT NULL
UNIQUE`, `redirect_uri text NOT NULL`, `code_challenge text NOT NULL`, `family_id uuid NULL`
(set on first exchange, for B32's replay revocation), `used_at timestamptz NULL`, `expires_at
timestamptz NOT NULL`, `created_at`, `updated_at`. Organization: derived through `user_id`.
As built by I7 (migration `20261116010000_oauth_authorization_codes.sql`, B45; coordinator
decision 2026-11-15): `org_id uuid NOT NULL REFERENCES organizations(id)` is direct, with `(org_id,
user_id) REFERENCES users(org_id, id) ON DELETE CASCADE`, as `auth_flows` and `service_accounts`
have it, so every query filters on the `OrgScope` without a join; `code_hash` is checked to 32
bytes, `code_challenge` to 43 base64url characters, and an index on `(org_id, expires_at)` serves
the cleanup. `family_id` and `used_at` are left to I8, which writes them when it exchanges a code
and can give `family_id` its foreign key to `oauth_refresh_tokens`.

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
  marked as mutating (the marker the issue adds), so B12 cannot regress silently. The marker is
  the `x-stepclimb-mutates` extension on the handler's `#[utoipa::path]`; the walk reads the
  OpenAPI document (every path, not only `/api/v1`), and a second test sends each method to each
  documented path so the router cannot mount a handler with another method than the document
  says. The router mounts every route from one list (`app::routes()`), a test checks each of
  its paths is in the document, and another that `app.rs` mounts nothing else but the document
  and its UI (coordinator decision, 13 Nov 2026).
- **T18.** (B18) Completing a flow twice answers `auth.flow_invalid` the second time; a flow used
  after its expiry (clock moved) answers `auth.flow_expired`.
- **T19.** (B19) `return_to` values `https://evil.example`, `//evil.example`, `/\evil.example`,
  `javascript:x`, `/..//evil.example`, `/.//evil.example`, `/a/..//evil` and `/%2e%2e//evil.example`
  all end at `/`; `/c/s1/m/2` and `/.well-known/x` are kept.
- **T20.** (B22, B23) The request after the limit answers 429 `auth.rate_limited` with
  `Retry-After` (a navigation route: a 303 to `/sign-in?error=auth.rate_limited` with
  `Retry-After`); the counter resets after the window (clock moved); failures for one account
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
- **T37.** (B39) With an `https` public URL, a GET, a refused POST and an unknown `/api/` path
  each answer with `Strict-Transport-Security: max-age=31536000`; with an `http` one, none does.
- **T38.** (B43) `GET /api/v1/me` answers `Cache-Control: no-store` signed in, anonymous and
  suspended; so do an unknown `/api/v1` path, a wrong method and a CSRF refusal under `/api/v1`;
  `/healthz`, `/api/openapi.json` and `/api/v1x` do not.

## Out of scope

- The Tauri client side of app sign-in (system browser, deep link, OS keychain): M5
  (`docs/specs/m5.md`).
- Sign-in methods themselves: `04-local-accounts.md`, `05-oidc.md`,
  `06-oauth2-and-vatsim-preset.md`; who may sign in: `07-admission.md`.
- `GET /api/v1/me`: `03-users-and-identities.md` (it uses this spec's extractor).
- The sign-in UI and the sign-out button: `11-app-shell.md`.
- Branding fields and `theme.css` in `/.well-known/stepclimb`: the spec that owns branding
  (`11-app-shell.md` or `12-tokens-and-theme-generator.md`); this spec serves the auth fields.
- A strict CSP (plan, Security and privacy): not in this spec's issue list; it belongs with the
  app shell or the deploy spec. HSTS is B39.
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
   router walk, `POST /auth/sign-out`, the `STEPCLIMB_PUBLIC_URL` and
   `STEPCLIMB_ALLOWED_ORIGINS` settings, and HSTS. Covers B12 to B16, B39; T11 to T17, T37. T15
   and T16 use a bearer header the server does not accept yet; their valid-API-key halves come
   with I6. Depends on: I2, Q6.
4. **I4. Sign-in flow state.** Migration for `auth_flows` (pending Q8), the flow cookie,
   create/consume/expire, `return_to` validation, cleanup. Covers B17 to B21; T18, T19. Depends
   on: I1, I2, Q8.
5. **I5. Rate limiting.** The limiter for sign-in and token routes keyed by client IP and account
   identifier, trusted-proxy client IP (the configuration key, and setting it in
   `deploy/compose/compose.production.yaml` to Caddy's fixed address on the `proxy` network,
   14-compose-deploy B7), 429 responses. Real client addresses reach Caddy only on rootful
   Docker or on rootless Docker with a RootlessKit port driver that propagates source
   addresses; otherwise every client arrives from the `proxy` network's gateway, and the
   self-hosting guide says so. Covers B22, B23, B40, B42; T20. Depends on: I3, Q7.
6. **I6. Service accounts and API keys.** Migration for `service_accounts`, the admin endpoints,
   bearer API-key authentication with the CRC-32 precheck, scope intersection per request.
   Covers B24 to B29; T3 (API-key request part), T21 to T26. Depends on: I2, `09-permissions-and-roles.md` (catalog,
   `Require<P>`, effective permissions), Q9. It also makes `GET /api/v1/me` answer an owned key for its owner and
   adds that case of `03-users-and-identities.md` T17 (03 B24). It writes Q14's audit entries for
   service-account creation and deletion into `audit_events` (`09-permissions-and-roles.md` B22
   to B24), in the transaction of the change, with the API key as the actor when a key makes it.
7. **I7. OAuth 2.1 authorization endpoint and codes.** Client registry, `GET /oauth/authorize`,
   the sign-in round trip through a flow, code issuance and storage. Covers B30, B31, B41 (the
   authorization endpoint's sign-in steps), B45; T27 to T29. Depends on: I4, `07-admission.md` I1
   (sign-in completion returns to the flow), Q10.
8. **I8. OAuth 2.1 token and revoke endpoints.** Migration for `oauth_refresh_tokens`; code
   exchange with PKCE, JWT access tokens, rotation, reuse detection, revocation, access-token
   authentication in the extractor, `RateLimited<Token>` on both endpoints and the router walk of
   B41 for `/oauth/`. Covers B32 to B37, B41; T3 (full sign-in, API-key request and token
   exchange), T30 to T34. Depends on: I5, I7, Q11 to Q13. Its access-token authentication also
   makes `GET /api/v1/me` answer an app token for its user and adds that case of
   `03-users-and-identities.md` T17 (03 B24). It writes Q14's audit entry for refresh-token reuse
   detection into `audit_events` (`09-permissions-and-roles.md` B22 to B24), in the transaction
   that revokes the family.
9. **I9. `/.well-known/stepclimb`.** The discovery document's auth fields. Covers B38; T35, T36.
   Depends on: I8, `03-users-and-identities.md` (`identity_providers`), `04-local-accounts.md`
   I1 (local method switches).
