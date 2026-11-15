// SPDX-License-Identifier: AGPL-3.0-or-later
//! The token request of a generic OAuth 2 provider (spec 06 B8; RFC 6749 4.1.3, RFC 7636 4.5),
//! on the `oauth2` crate's client.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};

use oauth2::basic::{BasicClient, BasicErrorResponse, BasicTokenType};
use oauth2::{
    AsyncHttpClient, AuthType, AuthorizationCode, ClientId, HttpRequest, HttpResponse,
    PkceCodeVerifier, RedirectUrl, RequestTokenError, TokenResponse, TokenUrl,
};

use crate::oauth2_config::ClientSecret;
use crate::outbound::{OutboundClient, OutboundError};

/// How much of a provider's `error` code an [`ExchangeError::Status`] keeps.
const ERROR_CHARS: usize = 200;

/// What one code redemption sends.
pub struct CodeExchange<'a> {
    /// The provider's `token` URL as stored.
    pub token_url: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a ClientSecret,
    /// The redirect URI the authorization request carried, byte for byte.
    pub redirect_uri: &'a str,
    /// The `code` the provider sent the browser back with. A credential: never log it.
    pub code: &'a str,
    /// The flow's PKCE code verifier, when the flow uses PKCE (Q3).
    pub code_verifier: Option<&'a str>,
}

impl std::fmt::Debug for CodeExchange<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeExchange")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("pkce", &self.code_verifier.is_some())
            .finish_non_exhaustive()
    }
}

/// The access token a successful exchange returned, for the userinfo request. `Debug` hides
/// it. Any refresh token in the response is never read and is dropped with it.
pub struct AccessToken(oauth2::AccessToken);

impl AccessToken {
    /// The token's text, for the `Authorization: Bearer` header.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.0.secret()
    }
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessToken(<redacted>)")
    }
}

/// Why the exchange failed (B8). None of these carries a URL, a body, a token or the code.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    /// The stored `token` URL or the redirect URI does not parse.
    #[error("the token or redirect URL is not a URL")]
    Url,
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the token request failed: {0}")]
    Fetch(OutboundError),
    /// The endpoint answered a status other than 200, with the RFC 6749 5.2 `error` code when
    /// the body carried one (cut to 200 characters; the description is never kept).
    #[error("the token endpoint answered HTTP {status} (error: {error:?})")]
    Status { status: u16, error: Option<String> },
    /// The body is not a token response: not JSON, or `access_token` or `token_type` missing or
    /// of the wrong type. Only the position is named.
    #[error("the token response does not parse (at {0})")]
    Parse(String),
    /// The response is not `application/json` or is empty.
    #[error("the token response is not a JSON body")]
    NotJson,
    /// The response's `token_type` is not `bearer` in any case (RFC 6750), so the access token
    /// could not be sent to the userinfo endpoint.
    #[error("the token response's token_type is not bearer")]
    TokenType,
}

/// Redeem the code at the `token` URL (B8) with the `redirect_uri`, the PKCE `code_verifier`
/// when there is one, and `client_secret_basic` authentication: HTTP Basic over the
/// form-encoded client id and secret (RFC 6749 2.3.1), which the `oauth2` crate builds.
/// The response must be 200 JSON with an `access_token` and a `token_type` of `bearer`.
pub async fn exchange_code(
    client: &OutboundClient,
    exchange: &CodeExchange<'_>,
) -> Result<AccessToken, ExchangeError> {
    let token_url = TokenUrl::new(exchange.token_url.to_owned()).map_err(|_| ExchangeError::Url)?;
    let redirect_uri =
        RedirectUrl::new(exchange.redirect_uri.to_owned()).map_err(|_| ExchangeError::Url)?;
    let oauth = BasicClient::new(ClientId::new(exchange.client_id.to_owned()))
        .set_client_secret(oauth2::ClientSecret::new(
            exchange.client_secret.expose_secret().to_owned(),
        ))
        .set_auth_type(AuthType::BasicAuth)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_uri);
    let mut request = oauth.exchange_code(AuthorizationCode::new(exchange.code.to_owned()));
    if let Some(verifier) = exchange.code_verifier {
        request = request.set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()));
    }
    let sender = Sender {
        client,
        status: AtomicU16::new(0),
    };
    let response = request
        .request_async(&sender)
        .await
        .map_err(|error| refused(error, sender.status.load(Ordering::Relaxed)))?;
    if *response.token_type() != BasicTokenType::Bearer {
        return Err(ExchangeError::TokenType);
    }
    Ok(AccessToken(response.access_token().clone()))
}

/// The `oauth2` crate's HTTP client: the request goes through the outbound client, and the
/// response's status is kept, since the crate's errors drop the status of a refused request.
struct Sender<'a> {
    client: &'a OutboundClient,
    status: AtomicU16,
}

impl<'c> AsyncHttpClient<'c> for Sender<'_> {
    type Error = OutboundError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, OutboundError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let response = self.client.send(request).await?;
            self.status
                .store(response.status().as_u16(), Ordering::Relaxed);
            Ok(response)
        })
    }
}

/// The [`ExchangeError`] of a failed request whose response, if any, had `status`. The crate's
/// parse errors carry the body, and its other errors provider text: neither is kept.
fn refused(
    error: RequestTokenError<OutboundError, BasicErrorResponse>,
    status: u16,
) -> ExchangeError {
    match error {
        RequestTokenError::Request(error) => ExchangeError::Fetch(error),
        RequestTokenError::ServerResponse(body) => ExchangeError::Status {
            status,
            error: Some(body.error().as_ref().chars().take(ERROR_CHARS).collect()),
        },
        _ if status != 200 => ExchangeError::Status {
            status,
            error: None,
        },
        RequestTokenError::Parse(error, _body) => ExchangeError::Parse(error.path().to_string()),
        RequestTokenError::Other(_) => ExchangeError::NotJson,
    }
}
