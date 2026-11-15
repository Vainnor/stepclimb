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
//! - [`redeem_code`] finishes the callback (B9 to B11): it exchanges the code at the token
//!   endpoint ([`exchange_code`], client authentication per Q6), checks the ID token
//!   ([`verify_id_token`]) against the provider's key set ([`JwksCache`], Q5), and merges the
//!   userinfo claims when the provider's `userinfo` flag is on (Q3). The result is a
//!   [`ProviderSignIn`]: the subject and the claims, the provider's tokens dropped.
//!
//! Nothing here logs. Errors name what went wrong but never carry a URL, a header, a body or a
//! secret, and [`FlowSecrets`] leaves its values out of `Debug`.
//!
//! The ID token checks of B10 read the time the caller passes, which the callback takes from
//! the server's controllable clock.

mod authorize;
mod callback;
mod discovery;
mod exchange;
mod id_token;
mod jwks;
mod userinfo;

pub use authorize::{AuthorizeError, FlowSecrets, PayloadError, authorization_url};
pub use callback::{CallbackError, CodeCallback, ProviderSignIn, redeem_code};
pub use discovery::{
    DISCOVERY_TTL, DiscoveryCache, DiscoveryError, ProviderMetadata, browser_may_go_to, discover,
    discovery_url,
};
pub use exchange::{ClientAuth, CodeExchange, ExchangeError, Tokens, client_auth, exchange_code};
pub use id_token::{
    CLOCK_SKEW, Expected, IdTokenError, VerifiedIdToken, accepted_algorithms, verify_id_token,
};
pub use jwks::{JWKS_REFETCH_INTERVAL, JWKS_TTL, JwksCache, JwksError, KeySet, fetch_jwks};
pub use userinfo::{UserinfoError, fetch_userinfo, merge_claims};

#[cfg(test)]
mod tests;
