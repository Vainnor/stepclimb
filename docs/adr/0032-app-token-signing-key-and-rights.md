# 0032. App access tokens: a signing key derived from the encryption key ring, and the user's rights

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm
- **Related:** `docs/specs/m0/08-sessions-and-csrf.md` B36, B44, B46, Q3, Q12, I8; plan,
  Technical -> Decided (the OAuth 2.1 server "built on `oauth2`/`jsonwebtoken`");
  `crates/stepclimb-auth/src/oauth_server/access_token.rs`,
  `crates/stepclimb-auth/src/encryption.rs`

## Context

Spec 08 Q12, which the owner accepted on 13 Nov 2026, makes the app's access tokens JWTs signed
HS256 "with a server key from Q3" and checks on every request that the token's refresh family
still exists. The plan builds the server on `jsonwebtoken`, which I8 uses. Two points stay open:

1. **Which key signs.** Q3 gives the server one secret, the AES-256-GCM key ring in
   `STEPCLIMB_ENCRYPTION_KEYS`, whose first key encrypts and whose other keys only decrypt. Q12
   names no separate signing key and no setting for one.
2. **What an app token may do.** The plan names no scopes for Stepclimb's own app, and spec 08
   B44 (from #189) refuses API keys on the routes that change how their owner signs in.

## Options

### Signing key

- **A: a new `STEPCLIMB_TOKEN_SIGNING_KEY` setting.** A second secret for operators to
  generate, store, rotate and back up, with its own `_FILE` variant, Compose secret and
  documentation.
- **B: a key derived from each key of the encryption key ring.** HMAC-SHA-256 keyed with the AES
  key over the fixed label `stepclimb oauth access token signing key v1`. The current key signs
  and its id is the header's `kid`; any key of the ring verifies. The AES key never signs and the
  derived key never encrypts. No new setting.
- **C: use the AES key bytes as the HMAC key directly.** One key for two algorithms, against
  ordinary key-separation practice.

### App token rights

- **A: the user's current grants, refused wherever a session is required.** Like a browser
  session for permissions; like an API key for `NoBearer`, `SessionOnly` and the admin user routes
  acting on the credential's own user.
- **B: the user's grants with no extra refusals.** The app could change its user's email, status
  or sign-in methods with a token that lives on a device.
- **C: scopes.** No scope list exists, and the plan names none.

## Recommendation

Signing key B and rights A: no second secret, separated keys, and a stolen device token cannot
take over or lock out its user.

## Decision

Signing key B: `KeyRing` derives the key at parse time and hands it only to
`stepclimb_auth::oauth_server::access_token`, which signs with `jsonwebtoken` through its own
HS256-only `CryptoProvider` over the workspace's `hmac` and `sha2`. Rights A: an app token acts
with all of its user's current grants and is audited as the user, and every route that refuses an
API key for being a bearer credential refuses it the same way (spec 08 B46).

## Consequences

- Anyone who holds the encryption key ring can mint access tokens. That holder can already read
  every sealed secret, so the ring's protection (`_FILE`, a `0700` directory) covers both.
- Rotating the encryption key rotates the signing key: tokens signed under the old key stay valid
  for their 15 minutes while the old key stays in the ring, and end as soon as it is removed.
- The label versions the derivation: a future change of token format takes a new label rather
  than reusing the key.
- If Stepclimb ever publishes keys for other services to verify its tokens (Q12's option (b),
  ES256), that is a new decision with a key of its own.
- App scopes, if the app ever needs fewer rights than its user, are a new decision.
