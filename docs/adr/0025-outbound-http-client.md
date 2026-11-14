# 0025. Outbound HTTP: reqwest with rustls, ring and the system trust store

- **Status:** Accepted
- **Date:** 2026-11-14
- **Decider:** the coordinator, on the owner's delegation of M0 decisions (2026-11-13); the
  owner to confirm
- **Related:** `docs/specs/m0/05-oidc.md` I1, B2, Q4; `docs/specs/m0/07-admission.md` B14 to
  B17; `docs/plan/technical.md` -> Architecture and stack (`openidconnect`, `oauth2`);
  `crates/stepclimb-auth/src/outbound/`

## Context

From spec 05 I2 on, the server makes HTTP requests to URLs an organization admin enters: OIDC
discovery, JWKS, token and userinfo endpoints, generic OAuth 2 endpoints (spec 06) and the
admission webhook (spec 07). Until this change the workspace had no HTTP client and no TLS
library; sqlx is built without TLS.

The client has to enforce spec 05 Q4's URL policy at connect time (refuse a host name that
resolves to a loopback or, on a multi-organization instance, a private address), never follow
redirects for the webhook, time out after 5 s (3 s for the webhook), and fit the HTTP client
trait of the `openidconnect` and `oauth2` crates the plan names, which take an
`http::Request<Vec<u8>>` and return an `http::Response<Vec<u8>>`.

A self-hosted instance often signs in through an identity provider on the operator's own
network (Keycloak, Authentik) behind a certificate from an internal CA, so certificate checks
have to accept the CAs the operator installed on the host.

The release image is cross-compiled from amd64 to arm64 (`deploy/Dockerfile`) with Debian's
`gcc-aarch64-linux-gnu`; nothing in the server linked C code before.

## Options

### Option A: reqwest 0.13 with rustls, ring and rustls-platform-verifier

reqwest is the most used async HTTP client for Rust and the one `oauth2` and `openidconnect`
ship an adapter for. Its `dns_resolver` hook takes a resolver whose answers the connector uses
as they are, so the policy check and the connection use the same addresses. Redirects, proxies
and decompression are off or not compiled in. With the `rustls-no-provider` feature the client
is handed a rustls config: ring as the crypto (C code that `cc` builds with the cross compiler
the Dockerfile already installs) and rustls-platform-verifier, which reads the operating
system's trust store. Cost: 49 new crates in `Cargo.lock` (most of them Windows, macOS and
Android target crates that a Linux build does not compile), and ring's C code in the build.

### Option B: reqwest's default TLS (rustls with aws-lc-rs)

The same client with its default features. aws-lc-rs needs cmake and, for some targets,
bindgen at build time, which the Dockerfile and the Nix shell would both have to add, and the
default features also bring HTTP/2 and system proxy support that the policy would have to turn
off.

### Option C: hyper-util's client and hyper-rustls directly

Fewer crates, but the timeout, body limit, resolver wiring and request conversion that reqwest
provides would be written and reviewed here, and the `openidconnect` adapter written by hand
either way.

### Option D: webpki-roots instead of the system trust store

Mozilla's roots compiled into the binary: the same answer on every host, but a provider behind
an operator's internal CA could not be used without a further setting for extra roots.

## Recommendation

Option A: the least code to review here, no new build tool, and internal CAs work through the
host's trust store.

## Decision

Option A. `stepclimb_auth::outbound::OutboundClient` wraps reqwest 0.13.5 (`rustls-no-provider`
only), rustls 0.23.46 with ring, and rustls-platform-verifier 0.7.1, HTTP/1.1 only.

## Consequences

- The distroless runtime image must keep its CA bundle (`gcr.io/distroless/cc-debian12` ships
  one); an operator with an internal CA mounts or installs it on the host or in the image.
- The Dockerfile names the cross C compiler for arm64 builds (`CC_aarch64_unknown_linux_gnu`),
  and `just image-check` builds both architectures.
- Spec 05 I2 implements the `openidconnect` HTTP client trait on `OutboundClient` instead of
  enabling the crate's own reqwest feature, so every provider request goes through the policy.
- Revisit if reqwest drops the preconfigured rustls hook or a pure-Rust rustls provider matures.
