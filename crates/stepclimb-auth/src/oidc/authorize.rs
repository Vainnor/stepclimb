// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorization request (`docs/specs/m0/05-oidc.md` B5).

use openidconnect::core::{CoreAuthenticationFlow, CoreClient};
use openidconnect::{
    ClientId, CsrfToken, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use url::Url;

use super::ProviderMetadata;
use crate::oidc_config::OidcSettings;
use crate::token::{RandomError, Token};

/// The scope `openidconnect` always adds to an authorization request, so it is not added twice.
const OPENID: &str = "openid";

/// The random values one OIDC sign-in flow keeps on the server (B5): the `state` the callback
/// must echo, the `nonce` the ID token must carry, and the PKCE code verifier the token request
/// proves the flow with. Each is a [`Token`]: 32 bytes from the operating system's CSPRNG in
/// unpadded base64url, 43 characters, which RFC 7636 allows as a verifier (43 to 128 unreserved
/// characters).
///
/// The flow stores them sealed ([`FlowSecrets::to_payload`]); `Debug` shows none of them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowSecrets {
    state: String,
    nonce: String,
    code_verifier: String,
}

impl std::fmt::Debug for FlowSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FlowSecrets(<redacted>)")
    }
}

/// A stored flow payload that is not a [`FlowSecrets`]: a flow of another kind's, or corruption.
#[derive(Debug, thiserror::Error)]
#[error("the sign-in flow payload is not an OIDC flow's")]
pub struct PayloadError;

/// Why the authorization URL could not be built.
#[derive(Debug, thiserror::Error)]
pub enum AuthorizeError {
    /// The redirect URI is not an absolute URL.
    #[error("the redirect URI is not a URL")]
    RedirectUri,
}

impl FlowSecrets {
    /// Three new values from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self {
            state: Token::generate()?.expose_secret().to_owned(),
            nonce: Token::generate()?.expose_secret().to_owned(),
            code_verifier: Token::generate()?.expose_secret().to_owned(),
        })
    }

    /// The `state` the callback must echo. Compare it in constant time.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Whether `presented`, the callback's `state`, is this flow's (B7), compared in constant
    /// time so the comparison does not reveal how much of it matched.
    #[must_use]
    pub fn state_matches(&self, presented: &str) -> bool {
        self.state.as_bytes().ct_eq(presented.as_bytes()).into()
    }

    /// The `nonce` the ID token must carry.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// The PKCE code verifier for the token request.
    #[must_use]
    pub fn code_verifier(&self) -> &str {
        &self.code_verifier
    }

    /// The S256 code challenge of the verifier: unpadded base64url of its SHA-256 (RFC 7636 4.2).
    #[must_use]
    pub fn code_challenge(&self) -> String {
        self.challenge().as_str().to_owned()
    }

    fn challenge(&self) -> PkceCodeChallenge {
        // The verifier is a 43-character token, inside the 43 to 128 characters this asserts.
        PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
            self.code_verifier.clone(),
        ))
    }

    /// The bytes the sign-in flow stores, sealed, as its payload.
    #[must_use]
    pub fn to_payload(&self) -> Vec<u8> {
        // Serializing a struct of three strings to JSON cannot fail.
        serde_json::to_vec(self).expect("three strings serialize to JSON")
    }

    /// The values a flow's opened payload holds.
    pub fn from_payload(payload: &[u8]) -> Result<Self, PayloadError> {
        serde_json::from_slice(payload).map_err(|_| PayloadError)
    }
}

/// The provider's authorization endpoint with `response_type=code`, `client_id`,
/// `redirect_uri`, `scope` (the configured scopes, `openid` once), `state`, `nonce`,
/// `code_challenge` and `code_challenge_method=S256` (B5). Query parameters the endpoint already
/// carries are kept.
pub fn authorization_url(
    metadata: &ProviderMetadata,
    settings: &OidcSettings,
    redirect_uri: &str,
    secrets: &FlowSecrets,
) -> Result<Url, AuthorizeError> {
    let redirect_uri =
        RedirectUrl::new(redirect_uri.to_owned()).map_err(|_| AuthorizeError::RedirectUri)?;
    let client = CoreClient::from_provider_metadata(
        metadata.clone(),
        ClientId::new(settings.client_id.clone()),
        None,
    )
    .set_redirect_uri(redirect_uri);
    let state = CsrfToken::new(secrets.state.clone());
    let nonce = Nonce::new(secrets.nonce.clone());
    let scopes = settings
        .scopes
        .iter()
        .filter(|scope| scope.as_str() != OPENID)
        .map(|scope| Scope::new(scope.clone()));
    let (url, _, _) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            move || state,
            move || nonce,
        )
        .add_scopes(scopes)
        .set_pkce_challenge(secrets.challenge())
        .url();
    Ok(url)
}
