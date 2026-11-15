# 0027. The mock identity provider is a test-only workspace crate

- **Status:** Accepted
- **Date:** 2026-11-14
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm. The crate boundary is spec 06 Q1's recommendation, which the owner accepted
  on 2026-11-13
- **Related:** `docs/specs/m0/06-oauth2-and-vatsim-preset.md` B1 to B3, I1, I7;
  `docs/specs/m0/05-oidc.md` T3 to T15, Q8; ADR 0025; `bin/check-layering.sh`;
  `crates/stepclimb-mock-idp/`

## Context

Every sign-in test of specs 05 and 06 runs against a mock identity provider on 127.0.0.1, never
a real one. The mock has to serve an OpenID Connect provider (discovery, a JWKS, authorize, token,
userinfo, signed ID tokens) and a plain OAuth 2 provider, and misbehave on request: wrong claims,
`alg: none`, an unknown key, key rotation, provider errors, error statuses and delays. Spec 06 I7
later runs the same mock as a service of the Playwright Compose stack.

Spec 06 Q1 offered three homes: (a) a new workspace crate, a library for in-process tests plus a
small binary for Compose; (b) a module of `stepclimb-auth` behind a cargo feature, with a
third-party mock image for Compose; (c) a third-party server such as Keycloak for both. The owner
accepted (a) on 2026-11-13. A new crate needs an ADR and a layering rule
(`.claude/rules/architecture.md`).

Signing ID tokens needs an RSA key pair. Q8 of spec 05 has the server accept RS256 at least, and
OpenID Connect Discovery 1.0, section 3, requires a provider to list RS256. ring, already in the
tree through rustls (ADR 0025), signs RS256 but cannot generate RSA keys; committing a private
key as a fixture would trip `just secrets-scan`. The `rsa` crate generates and signs, and
`openidconnect` 4.1, the relying-party crate the plan names, already depends on `rsa` 0.9 to
verify.

## Options

### Option A: `stepclimb-mock-idp`, a library used as a dev-dependency only

The crate has no normal or build dependency on a workspace crate. Tests start it in their own tokio runtime with
`MockIdp::start()`, which binds `127.0.0.1:0`. The layering script refuses it as a normal or
build dependency of any crate, the server included, so it cannot reach the release binary or
image. Spec 06 I7 adds the Compose binary to this crate.

### Option B: a `test-support` module in `stepclimb-auth`

No new crate, but the module would compile into `stepclimb-auth` whenever the feature is on, and
the e2e stack would need a second mock that cannot inject spec 05's faults.

### Option C: Keycloak or another third-party server

Real behavior, but slow to start and without the fault switches spec 05 T9 needs.

## Recommendation

Option A, as spec 06 Q1 recommends: one implementation serves both test layers and it never
ships.

## Decision

Option A. The mock signs RS256 with 2048-bit keys from `rsa` 0.9.10, generated at start and on
each rotation and never written anywhere. Its tests read what it serves with `openidconnect`
4.1.0 (spec 06 T1), taken as a dev-dependency with default features off.

## Consequences

- `rsa` 0.9 carries RUSTSEC-2023-0071 (Marvin), a timing leak in private-key operations, fixed
  only in `rsa` 0.10.0 (published 2026-11-08, under 14 days old, and not accepted by
  `openidconnect` 4.1). ADR 0026 already ignores the advisory for webauthn-rs's `rsa`
  0.10.0-rc.19; `deny.toml` keeps one entry naming both consumers: the shipped code only
  verifies, a public-key operation, and the only private keys are the mock's throwaway keys on
  loopback. The entry goes when `cargo tree --target all -e all -i rsa` shows only `rsa` 0.10.0
  or later (issue #164).
- `openidconnect` adds 83 third-party crates to a `Cargo.lock` without webauthn-rs (most of them
  for `oauth2`, `chrono`, `serde_with` and the elliptic-curve algorithms it supports); on top of
  webauthn-rs (ADR 0026), which already brings many of them, it adds 24. Spec 05 I2 needs the
  same crate in `stepclimb-auth`, so the cost is paid once.
- RSA key generation unoptimized takes about 1 to 4 s per key; the root `Cargo.toml` builds `rsa`
  and `num-bigint-dig` at `opt-level = 3` in the dev profile, which brings a key to about 0.1 s.
- `bin/check-layering.sh` now also checks `stepclimb-server`, for rule 5, and reads every
  crate's tree with all features on, so an optional dependency on the mock is caught too.
