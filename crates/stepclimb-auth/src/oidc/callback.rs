// SPDX-License-Identifier: AGPL-3.0-or-later
//! From the `code` the provider sent back to a verified sign-in (`docs/specs/m0/05-oidc.md` B9
//! to B11).

use std::time::SystemTime;

use serde_json::{Map, Value};
use uuid::Uuid;

use super::exchange::{CodeExchange, ExchangeError, exchange_code};
use super::id_token::{
    Expected, IdTokenError, VerifiedIdToken, accepted_algorithms, verify_id_token,
};
use super::jwks::{JwksCache, JwksError};
use super::userinfo::{UserinfoError, fetch_userinfo, merge_claims};
use super::{FlowSecrets, ProviderMetadata};
use crate::claims::{self, MappedSignIn};
use crate::oidc_config::{ClaimsMapping, ClientSecret, OidcSettings};
use crate::outbound::OutboundClient;

/// What redeeming one callback's code needs.
pub struct CodeCallback<'a> {
    /// The client for the instance's organization count (Q4).
    pub client: &'a OutboundClient,
    pub keys: &'a JwksCache,
    pub provider_id: Uuid,
    /// The provider's accepted discovery document (B4).
    pub metadata: &'a ProviderMetadata,
    pub settings: &'a OidcSettings,
    pub client_secret: &'a ClientSecret,
    /// The redirect URI the authorization request carried.
    pub redirect_uri: &'a str,
    /// The flow's `nonce` and PKCE verifier.
    pub secrets: &'a FlowSecrets,
    /// The callback's `code`. A credential: never log it.
    pub code: &'a str,
    pub now: SystemTime,
}

impl std::fmt::Debug for CodeCallback<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeCallback")
            .field("provider_id", &self.provider_id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

/// A sign-in the provider vouched for: the identity's subject and the claims the mapping reads
/// (B12, B13). The provider's tokens are already dropped (B15).
#[derive(Clone, PartialEq)]
pub struct ProviderSignIn {
    /// The ID token's `sub`.
    pub subject: String,
    /// The ID token's claims, with userinfo's laid over them when userinfo was called (B11).
    pub claims: Map<String, Value>,
    /// The ID token's claims alone, before userinfo's were laid over them.
    pub id_token_claims: Map<String, Value>,
    /// The userinfo claims, when userinfo was called.
    pub userinfo_claims: Option<Map<String, Value>>,
}

impl std::fmt::Debug for ProviderSignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSignIn")
            .field("claims", &self.claims.keys().collect::<Vec<_>>())
            .field("userinfo", &self.userinfo_claims.is_some())
            .finish_non_exhaustive()
    }
}

impl ProviderSignIn {
    /// The subject and the profile `mapping` reads from the merged claims (B12, B13), with the
    /// email kept only when the provider vouched for it:
    ///
    /// 1. the merged claims' `email_verified` is exactly `true`, and
    /// 2. each claims set the server read (the ID token's, and userinfo's when it was called)
    ///    that gives the same email under the mapping says `email_verified` is exactly `true`
    ///    there too.
    ///
    /// Rule 2 keeps one source from vouching for another's address: an ID token saying its
    /// email is verified does not make a different, or an unconfirmed, userinfo email look
    /// verified, and a source that calls the address unverified is never overruled by one that
    /// does not.
    #[must_use]
    pub fn map(self, mapping: &ClaimsMapping) -> MappedSignIn {
        let pointers = mapping.profile();
        let mut profile = claims::profile(&Value::Object(self.claims), &pointers);
        let sources = std::iter::once(self.id_token_claims).chain(self.userinfo_claims);
        let vouched = profile.email_verified == Some(true)
            && sources
                .map(|source| claims::profile(&Value::Object(source), &pointers))
                .filter(|own| own.email == profile.email)
                .all(|own| own.email_verified == Some(true));
        if !vouched {
            profile.email = None;
        }
        MappedSignIn {
            subject: self.subject,
            profile,
        }
    }
}

/// Why the callback could not verify a sign-in. None of these carries a URL, a body or a token.
#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    /// The token request failed (B9).
    #[error(transparent)]
    Exchange(#[from] ExchangeError),
    /// The provider's key set could not be read.
    #[error(transparent)]
    Keys(#[from] JwksError),
    /// The ID token was refused (B10).
    #[error(transparent)]
    IdToken(#[from] IdTokenError),
    /// The userinfo answer was not used (B11).
    #[error(transparent)]
    Userinfo(#[from] UserinfoError),
}

/// Redeem the code (B9), verify the ID token against the provider's keys (B10), and, when the
/// provider's `userinfo` flag is on and the discovery document lists a `userinfo_endpoint`
/// (Q3), merge the userinfo claims (B11).
pub async fn redeem_code(callback: &CodeCallback<'_>) -> Result<ProviderSignIn, CallbackError> {
    let exchange = CodeExchange {
        client_id: &callback.settings.client_id,
        client_secret: callback.client_secret,
        redirect_uri: callback.redirect_uri,
        code: callback.code,
        code_verifier: callback.secrets.code_verifier(),
    };
    let tokens = exchange_code(callback.client, callback.metadata, &exchange).await?;
    let verified = verify(callback, tokens.id_token(), tokens.access_token()).await?;
    let userinfo = callback
        .metadata
        .userinfo_endpoint()
        .filter(|_| callback.settings.userinfo);
    let userinfo_claims = match userinfo {
        Some(endpoint) => Some(
            fetch_userinfo(
                callback.client,
                endpoint.url(),
                tokens.access_token(),
                &verified.subject,
            )
            .await?,
        ),
        None => None,
    };
    let claims = match &userinfo_claims {
        Some(userinfo) => merge_claims(verified.claims.clone(), userinfo.clone()),
        None => verified.claims.clone(),
    };
    Ok(ProviderSignIn {
        subject: verified.subject,
        claims,
        id_token_claims: verified.claims,
        userinfo_claims,
    })
}

/// [`verify_id_token`] with the kept key set; when the token names a key the set lacks, once
/// more with a refetched set, unless the set was fetched within the refetch interval (B10 1,
/// Q5).
async fn verify(
    callback: &CodeCallback<'_>,
    id_token: &str,
    access_token: &str,
) -> Result<VerifiedIdToken, CallbackError> {
    let algorithms = accepted_algorithms(callback.metadata);
    let expected = Expected {
        issuer: &callback.settings.issuer,
        client_id: &callback.settings.client_id,
        nonce: callback.secrets.nonce(),
        access_token,
        algorithms: &algorithms,
    };
    let jwks_uri = callback.metadata.jwks_uri().url();
    let (client, id, now) = (callback.client, callback.provider_id, callback.now);
    let keys = callback.keys.get(client, id, jwks_uri, now).await?;
    match verify_id_token(id_token, &keys, &expected, now) {
        Err(IdTokenError::UnknownKey) => {
            let Some(keys) = callback.keys.refetch(client, id, jwks_uri, now).await? else {
                return Err(IdTokenError::UnknownKey.into());
            };
            Ok(verify_id_token(id_token, &keys, &expected, now)?)
        }
        result => Ok(result?),
    }
}
