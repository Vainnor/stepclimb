// SPDX-License-Identifier: AGPL-3.0-or-later
//! App access token authentication of bearer requests (docs/specs/m0/08-sessions-and-csrf.md
//! B6, B11, B16, B36, Q12).
//!
//! `api_key` hands this module every `Authorization: Bearer` credential that is not an API key.
//! The token is checked first without the database (`stepclimb_auth::oauth_server::access_token
//! ::verify`): its signature under the key ring, its issuer (the public URL) and its expiry. An
//! expired token answers 401 `auth.access_token_expired`; any other failure 401
//! `auth.access_token_invalid`, without a query. Then the request's organization must be the
//! token's (B6), its client must still be in the registry, and its refresh family must still
//! exist for its user and client (`stepclimb_core::oauth_refresh_tokens::authenticate`), so a
//! revoked or reused family stops its access tokens at once; otherwise the same 401
//! `auth.access_token_invalid`. A suspended user answers 401 `auth.suspended` (B11).
//!
//! An app token acts as its user with all of the user's current grants, like a browser session;
//! it is still a bearer credential, so every route that takes `require::NoBearer` or
//! `require::SessionOnly` refuses it. The token's text never reaches a log line or an error
//! body.

use axum::http::request::Parts;
use stepclimb_auth::oauth_server::{access_token, clients};
use stepclimb_core::oauth_refresh_tokens::{self, AppTokenAuthentication};

use crate::{
    AppState,
    error::ApiError,
    oauth_authorize::issuer,
    session::{
        Authenticated, Credentialed, Rejection, request_organization, suspended, unauthorized,
    },
};

/// The codes this module produces (401).
pub mod code {
    /// The bearer credential is not an access token this server issued, or its family was
    /// revoked.
    pub const AUTH_ACCESS_TOKEN_INVALID: &str = "auth.access_token_invalid";
    /// The access token is past its 15-minute lifetime.
    pub const AUTH_ACCESS_TOKEN_EXPIRED: &str = "auth.access_token_expired";
}

fn invalid() -> Rejection {
    Rejection::keep(unauthorized(
        code::AUTH_ACCESS_TOKEN_INVALID,
        "The access token is not valid. Sign in to the app again.",
    ))
}

fn expired() -> Rejection {
    Rejection::keep(unauthorized(
        code::AUTH_ACCESS_TOKEN_EXPIRED,
        "The access token has expired. Refresh it at the token endpoint.",
    ))
}

/// The principal of a bearer request whose credential `presented` is not an API key.
pub(crate) async fn authenticate(
    presented: &str,
    parts: &Parts,
    state: &AppState,
) -> Result<Credentialed, Rejection> {
    let now = state.clock.now();
    let claims = match access_token::verify(&state.keys, presented, &issuer(state), now) {
        Ok(claims) => claims,
        Err(access_token::Refusal::Expired) => return Err(expired()),
        Err(access_token::Refusal::Invalid) => return Err(invalid()),
    };
    let request_org = request_organization(state, &parts.headers)
        .await
        .map_err(Rejection::keep)?;
    if claims.org != request_org.org_id() || clients::find(&claims.client_id).is_none() {
        return Err(invalid());
    }
    let found = oauth_refresh_tokens::authenticate(
        &state.db,
        request_org,
        claims.sub,
        claims.fid,
        &claims.client_id,
    )
    .await
    .map_err(|error| Rejection::keep(ApiError::internal(&error)))?;
    match found {
        AppTokenAuthentication::Valid(principal) => Ok(Credentialed {
            principal: Authenticated::by_app_token(principal.user_id, principal.scope, claims.fid),
            key_scopes: None,
        }),
        AppTokenAuthentication::Suspended => Err(Rejection::keep(suspended())),
        AppTokenAuthentication::Unknown => Err(invalid()),
    }
}
