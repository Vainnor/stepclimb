// SPDX-License-Identifier: AGPL-3.0-or-later
//! The callback's work after the flow and `state` checks: code, access token, userinfo, mapped
//! sign-in (spec 06 B8 to B10).

use serde_json::Value;

use super::{
    CodeExchange, ExchangeError, FlowSecrets, UserinfoError, exchange_code, fetch_userinfo,
};
use crate::claims::SubjectMissing;
use crate::oauth2_config::{ClientSecret, MappedSignIn, Oauth2Settings};
use crate::outbound::OutboundClient;

/// What one callback redeems.
pub struct CodeCallback<'a> {
    /// The provider client for the instance's organization count.
    pub client: &'a OutboundClient,
    pub settings: &'a Oauth2Settings,
    pub client_secret: &'a ClientSecret,
    /// The redirect URI the authorization request carried.
    pub redirect_uri: &'a str,
    /// The flow's secrets, for the PKCE verifier.
    pub secrets: &'a FlowSecrets,
    /// The `code` the provider sent the browser back with. A credential: never log it.
    pub code: &'a str,
}

/// Why the callback could not produce a sign-in.
#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    /// The token request failed (B8): `auth.provider_error`.
    #[error(transparent)]
    Exchange(#[from] ExchangeError),
    /// The userinfo request failed (B9): `auth.provider_error`.
    #[error(transparent)]
    Userinfo(#[from] UserinfoError),
    /// The userinfo body has no usable subject (B6): `auth.subject_missing`.
    #[error(transparent)]
    Subject(#[from] SubjectMissing),
}

/// Exchange the code, call userinfo with the access token, and map the body with the
/// provider's claims. The access token is dropped before this returns, whatever the outcome
/// (`05-oidc.md` B15).
pub async fn redeem_code(callback: &CodeCallback<'_>) -> Result<MappedSignIn, CallbackError> {
    let settings = callback.settings;
    let exchange = CodeExchange {
        token_url: &settings.token,
        client_id: &settings.client_id,
        client_secret: callback.client_secret,
        redirect_uri: callback.redirect_uri,
        code: callback.code,
        code_verifier: callback.secrets.code_verifier(),
    };
    let access_token = exchange_code(callback.client, &exchange).await?;
    let body = fetch_userinfo(
        callback.client,
        &settings.userinfo,
        access_token.expose_secret(),
    )
    .await;
    drop(access_token);
    Ok(settings.claims.map(&Value::Object(body?))?)
}
