# 0028. OIDC discovery and the authorization request on openidconnect's types

- **Status:** Accepted
- **Date:** 2026-11-15
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm
- **Related:** `docs/specs/m0/05-oidc.md` B4, B5, Q5, Q7, I2; ADR 0025 (outbound HTTP client);
  ADR 0027 (the mock identity provider); `crates/stepclimb-auth/src/oidc/`

## Context

Spec 05 I2 adds the `openidconnect` crate the plan names (Technical -> Architecture and stack)
to `stepclimb-auth`. ADR 0027 pinned it at 4.1.0 (published 2026-10-31) for the mock's tests,
default features off. ADR 0025 expected I2 to implement the crate's HTTP client trait on
`OutboundClient` so every provider request goes through the URL policy.

The crate's `ProviderMetadata::discover_async` joins the discovery path, fetches the document,
compares its `issuer` with the configured one as strings and then fetches the JWKS in the same
call. Its errors carry the discovery URL and the response body in their messages. Spec 05 B4
needs the exact issuer check and a refusal that logs no URL; Q5 caches the discovery document
for an hour and refetches the JWKS on an unknown `kid` at most once a minute, which is a
different rule from the document's and belongs to the callback (I3).

Making the crate a normal dependency brings 44 crates into the release binary's graph that only
the tests had (among them `oauth2` 5.0.0, `chrono`, `rsa` 0.9.10, `p256`, `p384`,
`ed25519-dalek`). Every one was already in `Cargo.lock`; the newest besides `openidconnect` is
`lazy_static` 1.5.1 (2026-10-01). `just deny` passes.

## Options

### Option A: the crate's `discover_async` through a trait adapter

Least code here. The issuer check is the crate's, so no test of ours can break it to prove it
is tested; the JWKS is fetched on every discovery; error text needs mapping so no URL or body is
logged.

### Option B: our own fetch, the crate's types

`stepclimb_auth::oidc::discover` sends one GET through `OutboundClient::send`, requires 200 and
`application/json`, parses the body as `CoreProviderMetadata`, and compares `issuer` byte for
byte itself. The authorization URL is built with `CoreClient::authorize_url`, with `state`,
`nonce` and the PKCE verifier from `stepclimb_auth::token::Token` (the OS CSPRNG) rather than
the crate's `rand`-based `new_random`. About 100 lines more than option A, all of them read in
review.

## Recommendation

Option B: the issuer check, the error text and the caching stay in code we test and review.

## Decision

Option B. `openidconnect` 4.1.0 is a normal dependency of `stepclimb-auth`, default features
off. The HTTP client trait adapter of ADR 0025's consequences is written by the first issue that
calls a crate function needing it (the token exchange of spec 05 I3, if it uses the crate's).

## Consequences

- The ID token checks of spec 05 B10 take the time through
  `IdTokenVerifier::set_time_fn(Fn() -> DateTime<Utc>)`, which the callback feeds from the
  controllable clock. The crate refuses a token when that time is at or past `exp`, with no
  skew of its own, and does not check `iat` unless `set_issue_time_verifier_fn` is given, so I3
  applies Q5's 60 seconds of skew itself on both.
- `rsa` 0.9.10 now reaches the release binary. It only verifies there, a public-key operation;
  the `deny.toml` entry for RUSTSEC-2023-0071 (ADR 0026, ADR 0027) covers it unchanged.
- The authorization endpoint from a discovery document must be `https`, or `http` with the
  insecure-providers switch, before a browser is sent to it.
