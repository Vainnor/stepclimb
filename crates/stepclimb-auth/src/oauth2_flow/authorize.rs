// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorization request of a generic OAuth 2 provider (spec 06 B5, Q3).

use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use url::Url;
use url::form_urlencoded;

use crate::oauth2_config::{Oauth2Settings, RESERVED_PARAMETERS};
use crate::outbound::{Refused, UrlPolicy};
use crate::token::{RandomError, Token};

/// The random values one OAuth 2 sign-in flow keeps on the server: the `state` the callback
/// must echo and, when the provider's `pkce` is on (Q3), the PKCE code verifier the token
/// request proves the flow with. Each is a [`Token`]: 32 bytes from the operating system's
/// CSPRNG in unpadded base64url, 43 characters, which RFC 7636 allows as a verifier.
///
/// The flow stores them sealed ([`FlowSecrets::to_payload`]); `Debug` shows neither.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowSecrets {
    state: String,
    code_verifier: Option<String>,
}

impl std::fmt::Debug for FlowSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FlowSecrets(<redacted>)")
    }
}

/// A stored flow payload that is not a [`FlowSecrets`]: a flow of another kind's, or corruption.
#[derive(Debug, thiserror::Error)]
#[error("the sign-in flow payload is not an OAuth 2 flow's")]
pub struct PayloadError;

/// Why the authorization URL could not be built. Neither carries the URL.
#[derive(Debug, thiserror::Error)]
pub enum AuthorizeError {
    /// The stored `authorize` URL does not parse.
    #[error("the authorize URL is not a URL")]
    Url,
    /// The `authorize` URL breaks the outbound URL policy the instance is under now. Nothing
    /// fetches it, so this is the only check before a browser is sent there.
    #[error("the authorize URL is refused: {0}")]
    Refused(Refused),
}

impl FlowSecrets {
    /// A new `state`, and a new code verifier when `pkce` is on.
    pub fn generate(pkce: bool) -> Result<Self, RandomError> {
        let code_verifier = if pkce {
            Some(Token::generate()?.expose_secret().to_owned())
        } else {
            None
        };
        Ok(Self {
            state: Token::generate()?.expose_secret().to_owned(),
            code_verifier,
        })
    }

    /// The `state` the callback must echo.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Whether `presented`, the callback's `state`, is this flow's (`05-oidc.md` B7), compared
    /// in constant time so the comparison does not reveal how much of it matched.
    #[must_use]
    pub fn state_matches(&self, presented: &str) -> bool {
        self.state.as_bytes().ct_eq(presented.as_bytes()).into()
    }

    /// The PKCE code verifier for the token request, when the flow uses PKCE.
    #[must_use]
    pub fn code_verifier(&self) -> Option<&str> {
        self.code_verifier.as_deref()
    }

    /// The S256 code challenge of the verifier (RFC 7636 4.2), when the flow uses PKCE.
    #[must_use]
    pub fn code_challenge(&self) -> Option<String> {
        self.code_verifier.as_ref().map(|verifier| {
            // The verifier is a 43-character token, inside the 43 to 128 characters this
            // asserts.
            PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(verifier.clone()))
                .as_str()
                .to_owned()
        })
    }

    /// The bytes the sign-in flow stores, sealed, as its payload.
    #[must_use]
    pub fn to_payload(&self) -> Vec<u8> {
        // Serializing a struct of a string and an optional string to JSON cannot fail.
        serde_json::to_vec(self).expect("two strings serialize to JSON")
    }

    /// The values a flow's opened payload holds.
    pub fn from_payload(payload: &[u8]) -> Result<Self, PayloadError> {
        serde_json::from_slice(payload).map_err(|_| PayloadError)
    }
}

/// The provider's `authorize` URL with `response_type=code`, `client_id`, `redirect_uri`,
/// `scope` (the scopes joined by spaces, left out when there are none), `state`, and with PKCE
/// `code_challenge` and `code_challenge_method=S256` (B5).
///
/// The URL must pass `policy`, the outbound URL policy of the instance now, since the stored
/// config was checked under the policy of the day it was saved. Its own query parameters are
/// kept as written, except one named like a parameter this request sets (compared after
/// percent-decoding, as the config's validation does), which is removed so the provider reads
/// only the server's value.
pub fn authorization_url(
    settings: &Oauth2Settings,
    redirect_uri: &str,
    secrets: &FlowSecrets,
    policy: UrlPolicy,
) -> Result<Url, AuthorizeError> {
    let mut url = Url::parse(&settings.authorize).map_err(|_| AuthorizeError::Url)?;
    policy.check_url(&url).map_err(AuthorizeError::Refused)?;
    let kept = url.query().map(without_reserved).filter(|q| !q.is_empty());
    url.set_query(kept.as_deref());
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &settings.client_id)
            .append_pair("redirect_uri", redirect_uri);
        if !settings.scopes.is_empty() {
            query.append_pair("scope", &settings.scopes.join(" "));
        }
        query.append_pair("state", &secrets.state);
        if let Some(challenge) = secrets.code_challenge() {
            query
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256");
        }
    }
    Ok(url)
}

/// `query` without the `&`-separated pairs whose decoded name is a reserved parameter; the
/// others are kept byte for byte.
fn without_reserved(query: &str) -> String {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| {
            !form_urlencoded::parse(pair.as_bytes())
                .next()
                .is_some_and(|(name, _)| RESERVED_PARAMETERS.contains(&name.as_ref()))
        })
        .collect::<Vec<_>>()
        .join("&")
}
