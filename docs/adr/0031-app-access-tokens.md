# 0031. App access tokens: HS256 JWTs signed with a key derived from the encryption key ring

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm
- **Related:** `docs/specs/m0/08-sessions-and-csrf.md` B36, B46, Q3, Q12, I8; ADR 0012 (technical
  choices: our own OAuth 2.1 server "built on `oauth2`/`jsonwebtoken`"); ADR 0029 (ID token
  checks); `crates/stepclimb-auth/src/oauth_server/access_token.rs`,
  `crates/stepclimb-auth/src/encryption.rs`

## Context

Spec 08 Q12, which the owner accepted on 13 Nov 2026, makes the app's access tokens JWTs signed
HS256 "with a server key from Q3" and checks on every request that the token's refresh family
still exists. Q3 gives the server one secret, the AES-256-GCM key ring in
`STEPCLIMB_ENCRYPTION_KEYS`, whose first key encrypts and whose other keys only decrypt. Q12 names
no separate signing key and no setting for one.

The plan builds the OAuth 2.1 server "on `oauth2`/`jsonwebtoken`" (ADR 0012). The workspace has
neither crate as a server-side JOSE library: `oauth2` is a client, and ADR 0029 kept JOSE parsing
for ID tokens in our own code over `openidconnect`'s keys. The server only ever checks tokens it
signed itself, with one algorithm, one header shape and one claim set.

Because Q12 (b) looks the family up on every request anyway, the token's signature saves no
database query; what it does give is a refusal of a forged or altered token before any query, and
a payload the server can read without a lookup table.

## Options

### Option A: `jsonwebtoken` with a new `STEPCLIMB_TOKEN_SIGNING_KEY` setting

The plan's crate, and a second secret for operators to generate, store, rotate and back up, with
its own `_FILE` variant, Compose secret and documentation. `jsonwebtoken` brings algorithm
negotiation, key types and validation options the server does not use; its validation would still
need our own checks of the header and claim set (the reasoning of ADR 0029).

### Option B: our own HS256 over `hmac` and `sha2`, keyed from the encryption key ring

Each key of the ring derives an HMAC-SHA-256 key: HMAC-SHA-256 keyed with the AES key over the
fixed label `stepclimb oauth access token signing key v1`. The current key signs and its id is
the header's `kid`; any key of the ring verifies, so a key rotation keeps the tokens signed before
it valid for their remaining 15 minutes, and removing a key ends them. The AES key never signs and
the derived key never encrypts. About 200 lines with tests, on crates the workspace already pins
(`hmac` 0.13.0, `sha2` 0.11.1, `subtle` 2.6.1); no new dependency, no new setting.

### Option C: opaque random access tokens stored hashed

A `Token` (Q2) per access token in a new table, looked up by its hash on each request. It departs
from Q12's decided JWT, adds a table the plan's data model does not have, and adds a write per
refresh.

## Recommendation

Option B: it is what Q12 decided, with no second secret and no dependency, and every check is ours
and reachable by a test.

## Decision

Option B, built in `stepclimb_auth::oauth_server::access_token` and `KeyRing::sign_access_token` /
`KeyRing::verify_access_token`. The header is exactly `alg: "HS256"`, `typ: "JWT"` and `kid`; the
payload is exactly `iss`, `sub`, `org`, `client_id`, `fid`, `iat` and `exp`. Any other header or
claim member, algorithm or type is refused before the signature is checked or after it, as spec
08 B46 lists.

## Consequences

- No `jsonwebtoken` dependency; ADR 0012's "built on `oauth2`/`jsonwebtoken`" no longer describes
  the token side of the server. If Stepclimb ever publishes keys for other services to verify
  its tokens (Q12's option (b), ES256), that is a new decision and likely a JOSE crate.
- Anyone who holds the encryption key ring can mint access tokens. That holder can already read
  every sealed secret, so the ring's protection (`_FILE`, a `0700` directory) covers both.
- Rotating the encryption key also rotates the signing key. Removing an old key from the ring
  ends the access tokens it signed at once, as it ends the values sealed under it.
- The label versions the derivation: a future change of token format takes a new label rather
  than reusing the key.
