// SPDX-License-Identifier: AGPL-3.0-or-later
//! The token request (`docs/specs/m0/05-oidc.md` B9, Q6; RFC 6749 4.1.3, RFC 7636 4.5).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use http::{HeaderValue, header};
use openidconnect::core::CoreClientAuthMethod;
use serde::Deserialize;
use url::form_urlencoded;

use super::ProviderMetadata;
use super::discovery::is_json;
use crate::oidc_config::ClientSecret;
use crate::outbound::{OutboundClient, OutboundError};

/// How the client authenticates at the token endpoint (RFC 6749 2.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientAuth {
    /// `client_secret_basic`: HTTP Basic with the form-encoded id and secret.
    Basic,
    /// `client_secret_post`: `client_id` and `client_secret` in the form body.
    Post,
}

/// Q6: `client_secret_basic`, unless the discovery document lists `client_secret_post` and not
/// `client_secret_basic`. A document that lists no methods means `client_secret_basic`
/// (OpenID Connect Discovery 1.0, 3).
#[must_use]
pub fn client_auth(metadata: &ProviderMetadata) -> ClientAuth {
    let Some(methods) = metadata.token_endpoint_auth_methods_supported() else {
        return ClientAuth::Basic;
    };
    let lists = |method: CoreClientAuthMethod| methods.contains(&method);
    if lists(CoreClientAuthMethod::ClientSecretPost)
        && !lists(CoreClientAuthMethod::ClientSecretBasic)
    {
        ClientAuth::Post
    } else {
        ClientAuth::Basic
    }
}

/// What one code redemption sends.
pub struct CodeExchange<'a> {
    pub client_id: &'a str,
    pub client_secret: &'a ClientSecret,
    /// The redirect URI the authorization request carried, byte for byte.
    pub redirect_uri: &'a str,
    /// The `code` the provider sent the browser back with. A credential: never log it.
    pub code: &'a str,
    /// The flow's PKCE code verifier.
    pub code_verifier: &'a str,
}

impl std::fmt::Debug for CodeExchange<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeExchange")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

/// The tokens a successful exchange returned. They are used during the callback and dropped
/// (B15); `Debug` shows neither. A refresh token, if the provider sent one, is not read.
pub struct Tokens {
    access_token: String,
    id_token: String,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tokens(<redacted>)")
    }
}

impl Tokens {
    /// The access token, for `at_hash` and the userinfo request.
    #[must_use]
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// The ID token in its compact serialization, not yet verified.
    #[must_use]
    pub fn id_token(&self) -> &str {
        &self.id_token
    }
}

/// Why the exchange failed (B9). None of these carries a URL, a body or a token.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    /// The discovery document names no `token_endpoint`.
    #[error("the provider lists no token endpoint")]
    NoTokenEndpoint,
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the token request failed: {0}")]
    Fetch(#[from] OutboundError),
    /// The endpoint answered a status outside 2xx.
    #[error("the token endpoint answered HTTP {0}")]
    Status(u16),
    /// The response is not `application/json`.
    #[error("the token response is not application/json")]
    ContentType,
    /// The body is not a token response. The message names the position, never the content.
    #[error("the token response does not parse ({0})")]
    Parse(String),
    /// The response's `token_type` is not `Bearer` (RFC 6750), so the access token could not be
    /// sent to the userinfo endpoint.
    #[error("the token response's token_type is not Bearer")]
    TokenType,
    /// The response has no `id_token`.
    #[error("the token response has no id_token")]
    NoIdToken,
}

/// The members of a token response the callback reads (RFC 6749 5.1, OIDC Core 3.1.3.3).
#[derive(Deserialize)]
struct Body {
    access_token: String,
    token_type: String,
    id_token: Option<String>,
}

/// Redeem the code at the discovery document's `token_endpoint` (B9) with the `redirect_uri`,
/// the PKCE `code_verifier` and the client authentication of [`client_auth`].
pub async fn exchange_code(
    client: &OutboundClient,
    metadata: &ProviderMetadata,
    exchange: &CodeExchange<'_>,
) -> Result<Tokens, ExchangeError> {
    let endpoint = metadata
        .token_endpoint()
        .ok_or(ExchangeError::NoTokenEndpoint)?;
    let request = token_request(endpoint.url().as_str(), client_auth(metadata), exchange)?;
    let response = client.send(request).await?;
    if !response.status().is_success() {
        return Err(ExchangeError::Status(response.status().as_u16()));
    }
    if !is_json(response.headers()) {
        return Err(ExchangeError::ContentType);
    }
    let body: Body = serde_json::from_slice(response.body()).map_err(|e| {
        ExchangeError::Parse(format!(
            "{:?} error at line {} column {}",
            e.classify(),
            e.line(),
            e.column()
        ))
    })?;
    if !body.token_type.eq_ignore_ascii_case("bearer") {
        return Err(ExchangeError::TokenType);
    }
    let id_token = body.id_token.ok_or(ExchangeError::NoIdToken)?;
    Ok(Tokens {
        access_token: body.access_token,
        id_token,
    })
}

/// The `POST` of the form, authenticated as `auth` says.
fn token_request(
    endpoint: &str,
    auth: ClientAuth,
    exchange: &CodeExchange<'_>,
) -> Result<http::Request<Vec<u8>>, OutboundError> {
    let mut form = form_urlencoded::Serializer::new(String::new());
    form.append_pair("grant_type", "authorization_code")
        .append_pair("code", exchange.code)
        .append_pair("redirect_uri", exchange.redirect_uri)
        .append_pair("code_verifier", exchange.code_verifier);
    let mut request = http::Request::post(endpoint)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    match auth {
        ClientAuth::Basic => {
            request = request.header(header::AUTHORIZATION, basic_credentials(exchange)?);
        }
        ClientAuth::Post => {
            form.append_pair("client_id", exchange.client_id)
                .append_pair("client_secret", exchange.client_secret.expose_secret());
        }
    }
    request
        .body(form.finish().into_bytes())
        .map_err(|_| OutboundError::Request)
}

/// `Basic base64(form(id) ":" form(secret))`: RFC 6749 2.3.1 form-encodes both before they are
/// joined, so a secret holding `+`, `/`, `=` or `:` arrives intact. Marked sensitive.
fn basic_credentials(exchange: &CodeExchange<'_>) -> Result<HeaderValue, OutboundError> {
    let encode =
        |value: &str| form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>();
    let pair = format!(
        "{}:{}",
        encode(exchange.client_id),
        encode(exchange.client_secret.expose_secret())
    );
    let mut value = HeaderValue::from_str(&format!("Basic {}", STANDARD.encode(pair)))
        .map_err(|_| OutboundError::Request)?;
    value.set_sensitive(true);
    Ok(value)
}
