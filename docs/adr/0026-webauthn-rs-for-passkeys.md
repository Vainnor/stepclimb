# 0026. webauthn-rs 0.6 for passkeys

- **Status:** Accepted
- **Date:** 2026-11-14
- **Decider:** the coordinator, on the owner's delegation
- **Related:** `docs/plan/technical.md` -> Architecture and stack (`webauthn-rs`);
  `docs/specs/m0/04-local-accounts.md` B8, Q12, issue I3; `docs/specs/m0/08-sessions-and-csrf.md`
  Q1, B17, B20; `deny.toml`

## Context

The plan names `webauthn-rs` for local passkeys. Spec 04 I3 adds it with an exact pin at least
14 days old that passes `just deny`. On 2026-11-14 crates.io lists two lines:

- 0.5.5 (2026-04-30). Its `webauthn-rs-core` verifies signatures with OpenSSL (`openssl-sys`),
  which `deny.toml` refuses because the images carry no system OpenSSL.
- 0.6.2 (2026-10-31T03:07Z, 14 days old). It replaced OpenSSL with RustCrypto through
  `crypto-glue` 0.2.2, so it builds with no system library. Both lines are MPL-2.0, which
  `deny.toml` allows.

0.6.2 brings `rsa` 0.10.0-rc.19 (2026-10-06). cargo-deny reports RUSTSEC-2023-0071 (the Marvin
attack, a timing leak in RSA private-key operations) against it, because a prerelease sorts
before the fixed 0.10.0. The 0.10 prereleases already compute on `crypto-bigint`, the change the
advisory names as the fix. `webauthn-rs-core` 0.6.2 uses `rsa` only for `RS256VerifyingKey`, to
verify a passkey signed with RS256. Verifying uses the public key alone, so there is no private
key for a timing leak to reveal.

`rsa` 0.10.0 itself (2026-11-08) requires `crypto-bigint` 0.7.11 or later, also published on
2026-11-08, so neither can enter the lockfile before 2026-11-22. The rule applies to every crate
in the lockfile, not only the ones named in `Cargo.toml`. The resolver's first choice for this
graph took `crypto-bigint` 0.7.11 and `pkcs1` 0.8.0 (2026-11-02), and both are under 14 days
old. The lockfile pins them back to `crypto-bigint` 0.7.10 (2026-10-26) and `pkcs1` 0.8.0-rc.5
(2026-10-05), which `rsa` 0.10.0-rc.19 accepts. On 2026-11-14 every crate the dependency adds
to `Cargo.lock` (113 of them) is at least 14 days old by its crates.io publish date.

Tests need an authenticator without hardware. The same project publishes
`webauthn-authenticator-rs` 0.6.2 (MPL-2.0, same date), whose `softpasskey` feature is a software
passkey.

## Options

### Option A: webauthn-rs 0.6.2, with the advisory ignored and a removal condition
Builds without OpenSSL, follows the plan, and the advisory does not reach a code path that holds
a secret. Costs one `ignore` entry in `deny.toml` that someone must remove.

### Option B: webauthn-rs 0.5.5
Needs `openssl-sys`, so `deny.toml`'s ban and the distroless image would both have to change.

### Option C: wait until `rsa` 0.10.0 and `crypto-bigint` 0.7.11 are 14 days old (2026-11-22)
No ignore entry, but it blocks spec 04 I3 to I5 for 8 days.

## Recommendation

Option A.

## Decision

Option A. `webauthn-rs` 0.6.2 with default features off (`attestation` vets devices, which Q12
does not do) and `danger-allow-state-serialisation`, so a ceremony's state can be sealed into the
sign-in flow between its two requests; `webauthn-authenticator-rs` 0.6.2 with `softpasskey` as a
dev-dependency of `stepclimb-auth`. The lockfile holds `crypto-bigint` at 0.7.10 and `pkcs1` at
0.8.0-rc.5, so every crate in it is at least 14 days old. `deny.toml` ignores RUSTSEC-2023-0071
with the reason above, for as long as `crypto-glue` (through webauthn-rs) is the only crate that
depends on `rsa`.
The `webauthn-rs` types stay inside `stepclimb_auth::passkey`: the server and the repositories
pass JSON and bytes.

## Consequences

- The `rsa` ignore comes out of `deny.toml` on or after 2026-11-22, when `rsa` 0.10.0 and
  `crypto-bigint` 0.7.11 are 14 days old: `cargo update -p rsa` to 0.10.0 or later, then drop
  the entry. Issue #164 tracks it. A second crate that comes to depend on `rsa` before then is
  judged on its own; the ignore does not cover it.
- A `webauthn-rs` bump changes what the stored `passkeys.credential` JSON looks like only if its
  serialization changes; the bump's PR checks that existing rows still load.
- The relying party ID is the public URL's host, and passkeys are bound to it: changing
  `STEPCLIMB_PUBLIC_URL` to another host strands every registered passkey.
