// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bearer authentication: API keys here, app access tokens in [`crate::app_token`]
//! (docs/specs/m0/08-sessions-and-csrf.md B16, B25 to B27, B29, B36).
//!
//! `session::authenticate` hands every request that carries an `Authorization: Bearer` header
//! here, whether or not it also carries a session cookie, which is never read or cleared. The
//! header must be the request's only `Authorization` header and hold `Bearer <credential>`, one
//! space between them (the scheme in any case). A credential that starts with `sc_` is an API
//! key: its shape and CRC-32 are checked before anything else, so a mistyped or made-up key
//! answers 401 `auth.api_key_invalid` without a database query (B25). A well-formed key is looked
//! up by its SHA-256 within the request's organization
//! (`stepclimb_core::service_accounts::authenticate`): an unknown key, another organization's
//! and a deleted account's answer the same 401, and a suspended owner answers 401
//! `auth.suspended`. Any other credential after `Bearer ` is an app access token and goes to
//! [`crate::app_token`]. A header that is not one `Bearer <credential>` answers 401
//! `auth.unauthenticated`.
//!
//! The key's text never reaches a log line or an error body: `ApiKey` has no `Display` and a
//! redacted `Debug`, and the answers here name no part of it.

use axum::http::{HeaderMap, header, request::Parts};
use stepclimb_auth::token::{API_KEY_PREFIX, ApiKey};
use stepclimb_core::service_accounts::{self, KeyAuthentication};

use crate::{
    AppState, app_token,
    error::ApiError,
    session::{
        Authenticated, Credentialed, Rejection, request_organization, suspended, unauthenticated,
        unauthorized,
    },
};

/// The codes this module produces (401).
pub mod code {
    /// The bearer credential is not a well-formed API key, or no service account of the
    /// organization has it.
    pub const AUTH_API_KEY_INVALID: &str = "auth.api_key_invalid";
}

fn invalid() -> ApiError {
    unauthorized(
        code::AUTH_API_KEY_INVALID,
        "The API key is not valid. Check that it was copied whole, and that its service account still exists.",
    )
}

/// What the request's `Authorization` header presents.
#[derive(Debug)]
enum Presented<'h> {
    /// A well-formed API key with a matching checksum.
    Key(ApiKey),
    /// A credential that starts with `sc_` but is not a well-formed key.
    MalformedKey,
    /// Any other credential, non-empty and without spaces: an app access token, checked by
    /// [`crate::app_token`].
    AccessToken(&'h str),
    /// Several headers, or a value that is not `Bearer <credential>`.
    Other,
}

fn presented(headers: &HeaderMap) -> Presented<'_> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Presented::Other;
    };
    let Some((scheme, credential)) = value.to_str().ok().and_then(|v| v.split_once(' ')) else {
        return Presented::Other;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Presented::Other;
    }
    if credential.starts_with(API_KEY_PREFIX) {
        return match ApiKey::parse(credential) {
            Ok(key) => Presented::Key(key),
            Err(_) => Presented::MalformedKey,
        };
    }
    if credential.is_empty() || credential.contains(|c: char| c.is_ascii_whitespace()) {
        return Presented::Other;
    }
    Presented::AccessToken(credential)
}

/// The principal of a bearer request: an API key's, with its scopes, or an app access token's.
pub(crate) async fn authenticate(
    parts: &Parts,
    state: &AppState,
) -> Result<Credentialed, Rejection> {
    let key = match presented(&parts.headers) {
        Presented::Key(key) => key,
        Presented::MalformedKey => return Err(Rejection::keep(invalid())),
        Presented::AccessToken(token) => return app_token::authenticate(token, parts, state).await,
        Presented::Other => return Err(Rejection::keep(unauthenticated())),
    };
    let request_org = request_organization(state, &parts.headers)
        .await
        .map_err(Rejection::keep)?;
    let now = state.clock.now();
    let found = service_accounts::authenticate(&state.db, request_org, key.hash().as_bytes(), now)
        .await
        .map_err(|error| Rejection::keep(ApiError::internal(&error)))?;
    match found {
        KeyAuthentication::Valid(principal) => Ok(Credentialed {
            principal: Authenticated::by_api_key(
                principal.user_id,
                principal.scope,
                principal.service_account_id,
            ),
            key_scopes: Some(principal.scopes),
        }),
        KeyAuthentication::Suspended => Err(Rejection::keep(suspended())),
        KeyAuthentication::Unknown => Err(Rejection::keep(invalid())),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const GOOD: &str = "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ";

    fn with(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn a_well_formed_key_is_presented_in_any_case_of_the_scheme() {
        for value in [format!("Bearer {GOOD}"), format!("bEaReR {GOOD}")] {
            let headers = with(&[&value]);
            assert!(matches!(presented(&headers), Presented::Key(_)), "{value}");
        }
    }

    #[test]
    fn a_key_with_a_wrong_checksum_is_malformed() {
        let wrong = format!("Bearer {}Y", &GOOD[..GOOD.len() - 1]);
        assert!(matches!(
            presented(&with(&[&wrong])),
            Presented::MalformedKey
        ));
        let spaced = format!("Bearer  {GOOD}");
        assert!(matches!(presented(&with(&[&spaced])), Presented::Other));
        let trailing = format!("Bearer {GOOD} ");
        assert!(matches!(
            presented(&with(&[&trailing])),
            Presented::MalformedKey
        ));
    }

    #[test]
    fn any_other_bearer_credential_is_an_access_token() {
        for value in ["Bearer eyJhbGciOiJIUzI1NiJ9.e30.x", "bearer x", "Bearer s"] {
            assert!(
                matches!(presented(&with(&[value])), Presented::AccessToken(_)),
                "{value}"
            );
        }
    }

    #[test]
    fn repeated_headers_and_other_schemes_are_no_credential() {
        let good = format!("Bearer {GOOD}");
        for headers in [
            with(&["Bearer"]),
            with(&["Bearer "]),
            with(&["Bearer  eyJ.e30.x"]),
            with(&["Bearer eyJ.e30.x x"]),
            with(&[&format!("Basic {GOOD}")]),
            with(&[&good, &good]),
            with(&[&good, "Bearer eyJ.e30.x"]),
            HeaderMap::new(),
        ] {
            assert!(
                matches!(presented(&headers), Presented::Other),
                "{headers:?}"
            );
        }
    }
}
