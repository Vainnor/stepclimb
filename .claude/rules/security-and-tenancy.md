---
paths:
  - "crates/stepclimb-auth/**"
  - "crates/stepclimb-server/**"
  - "crates/stepclimb-core/**"
  - "migrations/**"
---
# Security and Tenancy Rules (not loaded at startup; read in full before writing or reviewing authentication, sessions, permissions, entitlements, or any query)

The maintainer reviews authentication, sessions, permissions, grading, feedback visibility,
billing and data deletion line by line (plan, Technical -> How it's built). Write them to be read.
The plan's sections are Technical -> Identity and access, Plans and billing, Security and privacy.

---

## Tenancy

- Every table but `organizations` has `org_id`; every repository function takes an `OrgScope`
  and every query filters by it. A test suite tries another organization's ids on every route and
  expects 404.
- Ids from the client are never trusted to imply an organization: the scope comes from the
  session or API key.

## Authorization: `Require<P>` and `Require<Feature>`

- Permissions are a fixed catalog in code (`stepclimb-core`). Handlers take a typed extractor,
  `Require<perm::CourseContentEdit>`, that checks the permission at the right scope before the
  body runs. A protected handler without one is a bug.
- Entitlements use the same shape, `Require<Feature>`, next to the permission. Self-hosted runs
  the unlimited plan, so every feature is on; the check still runs.
- Every protected route has the four-case test: anonymous 401, missing permission 403, wrong
  scope 404, allowed 2xx.
- An API key's rights are its scopes intersected with its owner's current permissions, evaluated
  on each request.

## Sessions and CSRF

- Browser sessions: the `__Host-sc_session` cookie (HttpOnly, Secure, SameSite=Lax) holds a random
  token; the database stores only its hash. 12 hours idle, 30 days at most.
- CSRF: unsafe methods are refused unless Fetch Metadata (`Sec-Fetch-Site`) says same-origin or
  the `Origin` is on the allowlist.
- App tokens (OAuth 2.1, PKCE only, public clients): 15-minute access tokens; refresh tokens rotate
  and a reused one revokes its family.

## Identities and admission

- Identities are `(provider, subject)`. Matching accounts by email is off by default; never link
  an identity by email without the organization's setting.
- Admission rules and the admission webhook (signed, 3 s timeout) fail closed: an error or a
  timeout refuses sign-in.

## Secrets

- Never log a secret, token, cookie, password, TOTP seed or API key, even at debug level.
- Passwords use argon2id. API keys are `sc_<random>_<crc32>`, stored hashed. Other secrets at
  rest are encrypted (AES-256-GCM).
- Compare secrets in constant time.

## Input and output

- Rich text is sanitized on the server (`ammonia`); the client never renders unsanitized HTML.
- Uploads are type-sniffed and served with `nosniff`, through short-lived signed URLs.
- Rate-limit sign-in, token and attempt endpoints.
