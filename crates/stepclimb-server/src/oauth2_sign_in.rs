// SPDX-License-Identifier: AGPL-3.0-or-later
//! The generic OAuth 2 steps of provider sign-in (docs/specs/m0/06-oauth2-and-vatsim-preset.md
//! B5, B7 to B10, Q3), called by the start and callback routes for a provider of kind
//! `oauth2`.
//!
//! [`authorize`] is the start: it reads the provider's stored config, checks its `authorize`
//! URL against the outbound URL policy the instance is under now (nothing fetches that URL, so
//! this is its only check before the browser goes there), and builds the authorization URL
//! with a new `state` and, when the provider's `pkce` is on, an S256 challenge.
//!
//! [`finish`] is the callback after `provider_callback` took the flow and checked its provider,
//! the `state` (in constant time) and the provider's `error`, in that order for both kinds. It:
//!
//! 1. redeems the `code` at the `token` URL with `client_secret_basic` and the PKCE verifier,
//!    calls the `userinfo` URL with the access token, and maps the body (B6, B8, B9). A failed
//!    token or userinfo request is `auth.provider_error`; a body without a usable subject is
//!    `auth.subject_missing`. When the config maps `email_verified` and the body's value is not
//!    `true`, the email is dropped (B10);
//! 2. hands the mapped sign-in to the completion both kinds share
//!    ([`crate::provider_callback::complete_sign_in`]): admission admits or refuses it, and an
//!    admitted user gets a session and a 303 to the flow's `return_to`.
//!
//! Refusals are logged at warn with the provider's key and the reason, never with a URL, the
//! `code`, a token or the client secret.

use axum::response::Response;
use stepclimb_auth::{
    oauth2_config::{MappedSignIn, Oauth2Config, ProviderRow},
    oauth2_flow::{CallbackError, CodeCallback, FlowSecrets, authorization_url, redeem_code},
};
use stepclimb_core::{
    OrgScope,
    auth_flows::{Kind as FlowKind, ReturnTo},
    identity_providers::IdentityProvider,
    organizations,
};

use crate::{
    AppState,
    error::ApiError,
    provider_callback::{Request, code, complete_sign_in},
    provider_sign_in::{Authorization, redirect_uri, to_sign_in},
};

/// The start's authorization URL and flow secrets for the `oauth2` `provider`, or `None` when
/// its stored config does not read or its `authorize` URL is refused, logged with its key.
pub(crate) async fn authorize(
    state: &AppState,
    provider: &IdentityProvider,
    key: &str,
) -> Result<Option<Authorization>, ApiError> {
    let Some(config) = stored_config(provider, key) else {
        return Ok(None);
    };
    let tenancy = organizations::tenancy(&state.db)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let policy = state.providers.client(tenancy).policy();
    let secrets =
        FlowSecrets::generate(config.settings.pkce).map_err(|e| ApiError::internal(&e))?;
    let redirect_uri = redirect_uri(&state.public_url, key);
    match authorization_url(&config.settings, &redirect_uri, &secrets, policy) {
        Ok(url) => Ok(Some(Authorization {
            kind: FlowKind::Oauth2,
            url,
            payload: secrets.to_payload(),
        })),
        Err(error) => {
            tracing::warn!(provider = key, %error, "the provider's authorize URL was not accepted");
            Ok(None)
        }
    }
}

/// The provider's stored config, or `None`, logged, when it does not read.
fn stored_config(provider: &IdentityProvider, key: &str) -> Option<Oauth2Config> {
    Oauth2Config::from_stored(&provider.config)
        .inspect_err(|error| {
            tracing::warn!(provider = key, %error, "the provider's stored config does not read");
        })
        .ok()
}

/// The callback's `oauth2` steps, once the route has checked the flow's provider, the `state`
/// and the provider's `error` (`provider_callback`): redeem `code` and pass the mapped sign-in
/// to admission.
pub(crate) async fn finish(
    state: &AppState,
    request: &Request<'_>,
    provider: &IdentityProvider,
    key: &str,
    secrets: &FlowSecrets,
    code: &str,
    return_to: &ReturnTo,
) -> Result<Response, ApiError> {
    match redeem(state, request.scope, provider, key, secrets, code).await? {
        Ok(sign_in) => complete_sign_in(state, request, provider, return_to, sign_in).await,
        Err(refusal) => Ok(to_sign_in(refusal)),
    }
}

/// Redeem `code` for `provider` (B8, B9, B6): the mapped sign-in, or the code to refuse with.
async fn redeem(
    state: &AppState,
    scope: OrgScope,
    provider: &IdentityProvider,
    key: &str,
    secrets: &FlowSecrets,
    code: &str,
) -> Result<Result<MappedSignIn, &'static str>, ApiError> {
    let Some(config) = stored_config(provider, key) else {
        return Ok(Err(code::AUTH_PROVIDER_ERROR));
    };
    let row = ProviderRow {
        org_id: scope.org_id(),
        provider_id: provider.id,
    };
    let client_secret = config
        .client_secret(&state.keys, row)
        .map_err(|e| ApiError::internal(&e))?;
    let tenancy = organizations::tenancy(&state.db)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let redirect_uri = redirect_uri(&state.public_url, key);
    let callback = CodeCallback {
        client: state.providers.client(tenancy),
        settings: &config.settings,
        client_secret: &client_secret,
        redirect_uri: &redirect_uri,
        secrets,
        code,
    };
    Ok(redeem_code(&callback).await.map_err(|error| {
        tracing::warn!(provider = key, %error, "the provider sign-in was refused");
        refusal_code(&error)
    }))
}

/// The redirect code for a callback that could not be redeemed.
fn refusal_code(error: &CallbackError) -> &'static str {
    match error {
        CallbackError::Subject(_) => code::AUTH_SUBJECT_MISSING,
        CallbackError::Exchange(_) | CallbackError::Userinfo(_) => code::AUTH_PROVIDER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_auth::{claims::SubjectMissing, oauth2_flow::ExchangeError};

    use super::*;

    #[test]
    fn a_missing_subject_is_subject_missing_and_a_failed_request_provider_error() {
        assert_eq!(
            refusal_code(&CallbackError::Subject(SubjectMissing)),
            "auth.subject_missing"
        );
        assert_eq!(
            refusal_code(&CallbackError::Exchange(ExchangeError::TokenType)),
            "auth.provider_error"
        );
        assert_eq!(
            refusal_code(&CallbackError::Userinfo(
                stepclimb_auth::oauth2_flow::UserinfoError::NotObject
            )),
            "auth.provider_error"
        );
    }
}
