# 0029. ID token checks on plain JSON claims, signatures by openidconnect's keys

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm
- **Related:** `docs/specs/m0/05-oidc.md` B10, B12, B13, Q5, Q8, I3; ADR 0028 (discovery and the
  authorization request); `crates/stepclimb-auth/src/oidc/id_token.rs`

## Context

Spec 05's summary has the server validate the ID token with the `openidconnect` crate the plan
names (Technical -> Architecture and stack), and ADR 0028's consequences expected spec 05 I3 to
call the crate's `IdTokenVerifier` with a time function fed from the controllable clock.

The crate's verifier only works on its typed claims, `IdTokenClaims`. Those parse the standard
profile claims with fixed types, so a token whose `given_name` is a number or whose
`email_verified` is the string `"true"` fails to parse, and the whole sign-in with it. A test
against `openidconnect` 4.1.0 (2026-11-15) showed exactly that: `{"given_name": 7}` and
`{"email_verified": "true"}` fail with `invalid type`, while an unknown claim parses. Spec 05
B13 requires the opposite: a profile field of the wrong type is treated as absent and the
sign-in goes on (T12).

The crate's verifier also differs from B10 in three places: it ignores `azp`, has no clock skew
on `exp`, and refuses any audience besides the client id, so B10 3's "two audiences with `azp`
equal to the client id" could never pass. Its checks run inside the crate, so no test of ours
can break one to prove it is tested.

## Options

### Option A: the crate's verifier, claims parsed twice

Keep `IdTokenVerifier`, add skew through the time function and `iat` through
`set_issue_time_verifier_fn`, check `azp` afterwards, and decode the payload a second time as
JSON for the claims mapping. A token with a profile claim of an unexpected type still fails to
parse before any check runs, so B13 cannot be met.

### Option B: our checks over JSON claims, the crate's keys for the signature

`stepclimb_auth::oidc::verify_id_token` splits the compact JWS itself, reads the header and
payload as JSON objects, picks the key from the crate's `CoreJsonWebKeySet` by `kid`, type, use
and algorithm, and verifies the signature with `CoreJsonWebKey::verify_signature`. It then checks
`iss`, `aud`, `azp`, `exp`, `iat`, `nbf`, `nonce`, `at_hash` (the hash from
`CoreJsonWebKey::hash_bytes`) and `sub` on the JSON. About 380 lines here with their
documentation, every check reachable by a test.

### Option C: a JOSE crate (`jsonwebtoken`, `josekit`)

Another dependency for what the crate already provides, the RSA, EC and EdDSA verification, and
a second key format beside the discovery code's.

## Recommendation

Option B: B13 needs claims read as plain JSON, and the signature code stays the crate's.

## Decision

Option B. The cryptography (signature verification and the `at_hash` digest) is
`openidconnect` 4.1.0's; the JWS framing, key choice and claim checks are Stepclimb's, listed in
the module documentation of `stepclimb_auth::oidc::id_token`.

## Consequences

- No `chrono` dependency: the checks read `SystemTime` from the caller, which the callback takes
  from the controllable clock. ADR 0028's note on `IdTokenVerifier::set_time_fn` no longer
  describes the code.
- The header checks are ours: `alg: none`, an algorithm outside Q8's list, `crit`, `cty`, `enc`
  and a `typ` other than `JWT` or `application/jwt` in any case (RFC 7515 4.1.9) are refused
  before the signature is checked.
- A crate upgrade that changes `CoreJsonWebKey`'s verification is caught by the tests that sign
  with the mock's keys and alter tokens after signing (`crates/stepclimb-auth/tests/oidc_callback.rs`).
