// SPDX-License-Identifier: AGPL-3.0-or-later
//! Discovery documents (`docs/specs/m0/05-oidc.md` B4, Q5, Q7).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use http::{StatusCode, header};
use openidconnect::IssuerUrl;
use openidconnect::core::CoreProviderMetadata;
use url::Url;
use uuid::Uuid;

use crate::outbound::{OutboundClient, OutboundError};

/// A provider's discovery document, as `openidconnect` reads it.
pub type ProviderMetadata = CoreProviderMetadata;

/// How long an accepted discovery document is used before it is fetched again (Q5).
pub const DISCOVERY_TTL: Duration = Duration::from_secs(60 * 60);

/// What the discovery path is joined to the issuer with (OpenID Connect Discovery 1.0, 4).
const CONFIG_PATH: &str = ".well-known/openid-configuration";

/// Why a provider's discovery document was not accepted. None of these carries a URL or a body.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// The configured issuer is not a URL the discovery path can be joined to.
    #[error("the issuer is not a URL")]
    IssuerUrl,
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the discovery request failed: {0}")]
    Fetch(#[from] OutboundError),
    /// The endpoint answered a status other than 200, a redirect included.
    #[error("the discovery endpoint answered HTTP {0}")]
    Status(u16),
    /// The response is not `application/json`.
    #[error("the discovery document is not application/json")]
    ContentType,
    /// The body is not a discovery document: not JSON, or a required member missing or of the
    /// wrong type. The message names the position, never the content.
    #[error("the discovery document does not parse ({0})")]
    Parse(String),
    /// The document's `issuer` is not the configured issuer exactly (B4, Q7).
    #[error("the discovery document names another issuer")]
    IssuerMismatch,
    /// The `authorization_endpoint` is not `https` (or `http` where the client's policy allows
    /// insecure providers), so the browser would not be sent to it.
    #[error("the authorization endpoint's scheme is not one the server sends browsers to")]
    AuthorizationEndpoint,
}

/// `<issuer>/.well-known/openid-configuration`, with one `/` between them whether or not the
/// issuer ends in one, so an issuer's path is kept.
pub fn discovery_url(issuer: &str) -> Result<Url, DiscoveryError> {
    IssuerUrl::new(issuer.to_owned())
        .and_then(|issuer| issuer.join(CONFIG_PATH))
        .map_err(|_| DiscoveryError::IssuerUrl)
}

/// Fetch and check the discovery document of `issuer` through `client` (B4). The document is
/// accepted only when the endpoint answers 200 with `application/json`, the body parses, its
/// `issuer` equals `issuer` byte for byte (a trailing `/` is a difference), and its
/// `authorization_endpoint` has a scheme the client's policy allows.
pub async fn discover(
    client: &OutboundClient,
    issuer: &str,
) -> Result<ProviderMetadata, DiscoveryError> {
    let url = discovery_url(issuer)?;
    let request = http::Request::get(url.as_str())
        .header(header::ACCEPT, "application/json")
        .body(Vec::new())
        .map_err(|_| DiscoveryError::IssuerUrl)?;
    let response = client.send(request).await?;
    if response.status() != StatusCode::OK {
        return Err(DiscoveryError::Status(response.status().as_u16()));
    }
    if !is_json(response.headers()) {
        return Err(DiscoveryError::ContentType);
    }
    let metadata: ProviderMetadata = serde_json::from_slice(response.body()).map_err(|e| {
        DiscoveryError::Parse(format!(
            "{:?} error at line {} column {}",
            e.classify(),
            e.line(),
            e.column()
        ))
    })?;
    if metadata.issuer().as_str() != issuer {
        return Err(DiscoveryError::IssuerMismatch);
    }
    match metadata.authorization_endpoint().url().scheme() {
        "https" => {}
        "http" if client.policy().allows_insecure() => {}
        _ => return Err(DiscoveryError::AuthorizationEndpoint),
    }
    Ok(metadata)
}

/// `Content-Type: application/json`, parameters such as `charset` allowed.
fn is_json(headers: &http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// Accepted discovery documents, per provider, for [`DISCOVERY_TTL`] (Q5). In this process
/// only, so the server keeps no state between replicas or restarts. Cloning is cheap; clones
/// share the documents.
///
/// A document is used for its provider only while the provider's issuer is the one it was
/// fetched for, so changing the issuer fetches again. A failed fetch is not kept: the next
/// sign-in tries again, at the rate the sign-in limit allows.
#[derive(Clone, Default)]
pub struct DiscoveryCache(Arc<Mutex<HashMap<Uuid, Entry>>>);

struct Entry {
    issuer: String,
    fetched_at: SystemTime,
    metadata: Arc<ProviderMetadata>,
}

impl std::fmt::Debug for DiscoveryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryCache").finish_non_exhaustive()
    }
}

impl DiscoveryCache {
    /// The discovery document of `provider_id`, whose configured issuer is `issuer`: the kept one
    /// when it was fetched for this issuer less than [`DISCOVERY_TTL`] before `now`, otherwise a
    /// new one from [`discover`], kept from `now` on.
    pub async fn get(
        &self,
        client: &OutboundClient,
        provider_id: Uuid,
        issuer: &str,
        now: SystemTime,
    ) -> Result<Arc<ProviderMetadata>, DiscoveryError> {
        if let Some(metadata) = self.fresh(provider_id, issuer, now) {
            return Ok(metadata);
        }
        let metadata = Arc::new(discover(client, issuer).await?);
        let entry = Entry {
            issuer: issuer.to_owned(),
            fetched_at: now,
            metadata: Arc::clone(&metadata),
        };
        self.lock().insert(provider_id, entry);
        Ok(metadata)
    }

    /// The kept document, when it is for `issuer` and younger than [`DISCOVERY_TTL`] at `now`. A
    /// document fetched after `now` (the clock went back) is not fresh.
    fn fresh(
        &self,
        provider_id: Uuid,
        issuer: &str,
        now: SystemTime,
    ) -> Option<Arc<ProviderMetadata>> {
        let entries = self.lock();
        let entry = entries.get(&provider_id)?;
        let young = now
            .duration_since(entry.fetched_at)
            .is_ok_and(|age| age < DISCOVERY_TTL);
        (entry.issuer == issuer && young).then(|| Arc::clone(&entry.metadata))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Entry>> {
        // Nothing panics while the lock is held; a poisoned map still holds whole entries.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
