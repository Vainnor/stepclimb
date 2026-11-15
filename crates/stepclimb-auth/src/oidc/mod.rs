// SPDX-License-Identifier: AGPL-3.0-or-later
//! The relying-party side of OpenID Connect sign-in (`docs/specs/m0/05-oidc.md`), on the
//! `openidconnect` crate's types (ADR 0028).
//!
//! - [`discover`] reads a provider's discovery document through an
//!   [`OutboundClient`](crate::outbound::OutboundClient) and accepts it only when its `issuer`
//!   is the configured issuer, character for character (B4). [`DiscoveryCache`] keeps an
//!   accepted document for [`DISCOVERY_TTL`] per provider (Q5), read against the caller's
//!   clock.
//! - [`FlowSecrets`] are the random `state`, `nonce` and PKCE code verifier a sign-in flow keeps
//!   (B5), and [`authorization_url`] is the provider's authorization endpoint with them, the
//!   client id, the redirect URI and the scopes. The challenge is always S256; PKCE `plain` is
//!   never sent.
//!
//! Nothing here logs. Errors name what went wrong but never carry a URL, a header, a body or a
//! secret, and [`FlowSecrets`] leaves its values out of `Debug`.
//!
//! The ID token checks of B10 read the time through `openidconnect`'s
//! `IdTokenVerifier::set_time_fn`, which takes a closure returning the current time: the
//! callback passes one that reads the server's controllable clock.

mod authorize;
mod discovery;

pub use authorize::{AuthorizeError, FlowSecrets, PayloadError, authorization_url};
pub use discovery::{
    DISCOVERY_TTL, DiscoveryCache, DiscoveryError, ProviderMetadata, discover, discovery_url,
};

#[cfg(test)]
mod tests;
