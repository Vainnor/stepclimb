// SPDX-License-Identifier: AGPL-3.0-or-later
//! The relying-party side of generic OAuth 2 sign-in
//! (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B5, B7 to B10, Q3).
//!
//! - [`FlowSecrets`] are the random `state` and, when the provider's `pkce` is on (Q3), the PKCE
//!   code verifier a sign-in flow keeps; [`authorization_url`] is the provider's `authorize` URL
//!   with them, the client id, the redirect URI and the scopes (B5). The challenge is always
//!   S256.
//! - [`redeem_code`] finishes the callback: it exchanges the code at the `token` URL
//!   ([`exchange_code`], B8), calls the `userinfo` URL with the access token
//!   ([`fetch_userinfo`], B9), and maps the userinfo body with the provider's claims
//!   ([`crate::claims`], B6). The access token is dropped before it returns (`05-oidc.md` B15).
//!
//! Every request goes through an [`OutboundClient`](crate::outbound::OutboundClient). Nothing
//! here logs. Errors name what went wrong but never carry a URL, a header, a body, a token or
//! the code, and [`FlowSecrets`] leaves its values out of `Debug`.

mod authorize;
mod callback;
mod exchange;
mod userinfo;

pub use authorize::{AuthorizeError, FlowSecrets, PayloadError, authorization_url};
pub use callback::{CallbackError, CodeCallback, redeem_code};
pub use exchange::{AccessToken, CodeExchange, ExchangeError, exchange_code};
pub use userinfo::{UserinfoError, fetch_userinfo};

#[cfg(test)]
mod tests;
