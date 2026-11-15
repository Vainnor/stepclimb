// SPDX-License-Identifier: AGPL-3.0-or-later
//! The provider's signing keys (`docs/specs/m0/05-oidc.md` B10 1, Q5).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use http::{StatusCode, header};
use openidconnect::core::CoreJsonWebKeySet;
use url::Url;
use uuid::Uuid;

use crate::outbound::{OutboundClient, OutboundError, UrlPolicy};

/// A provider's JSON Web Key Set, as `openidconnect` reads it. Keys of a type or algorithm the
/// crate does not know are skipped, so one odd key does not make the whole set unusable.
pub type KeySet = CoreJsonWebKeySet;

/// How long a fetched key set is used before it is fetched again (Q5).
pub const JWKS_TTL: Duration = Duration::from_secs(60 * 60);

/// The shortest time between two fetches of one provider's key set: an ID token whose `kid` the
/// kept set does not hold refetches it at most this often (Q5), so a stream of forged tokens
/// cannot make the server fetch on every request.
pub const JWKS_REFETCH_INTERVAL: Duration = Duration::from_secs(60);

/// Why a provider's key set could not be read. None of these carries a URL or a body.
#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the key set request failed: {0}")]
    Fetch(#[from] OutboundError),
    /// The endpoint answered a status other than 200, a redirect included.
    #[error("the key set endpoint answered HTTP {0}")]
    Status(u16),
    /// The response is neither `application/json` nor `application/jwk-set+json`.
    #[error("the key set is not application/json")]
    ContentType,
    /// The body is not a key set. The message names the position, never the content.
    #[error("the key set does not parse ({0})")]
    Parse(String),
}

/// Fetch the key set at `url` through `client`: 200, `application/json` or
/// `application/jwk-set+json` (RFC 7517 8.5), and a body that parses.
pub async fn fetch_jwks(client: &OutboundClient, url: &Url) -> Result<KeySet, JwksError> {
    let request = http::Request::get(url.as_str())
        .header(header::ACCEPT, "application/json")
        .body(Vec::new())
        .map_err(|_| OutboundError::Request)?;
    let response = client.send(request).await?;
    if response.status() != StatusCode::OK {
        return Err(JwksError::Status(response.status().as_u16()));
    }
    if !is_key_set_type(response.headers()) {
        return Err(JwksError::ContentType);
    }
    serde_json::from_slice(response.body()).map_err(|e| {
        JwksError::Parse(format!(
            "{:?} error at line {} column {}",
            e.classify(),
            e.line(),
            e.column()
        ))
    })
}

fn is_key_set_type(headers: &http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/json")
                || essence.eq_ignore_ascii_case("application/jwk-set+json")
        })
}

/// Fetched key sets, per provider, in this process only (Q5). Cloning is cheap; clones share
/// the sets.
///
/// A set is used for its provider only while the provider's `jwks_uri` and the client's URL
/// policy are the ones it was fetched under, and for at most [`JWKS_TTL`]. A failed fetch is not
/// kept. Each insert drops every set older than [`JWKS_TTL`].
#[derive(Clone, Default)]
pub struct JwksCache(Arc<Mutex<HashMap<Uuid, Entry>>>);

struct Entry {
    jwks_uri: Url,
    policy: UrlPolicy,
    fetched_at: SystemTime,
    keys: Arc<KeySet>,
}

impl std::fmt::Debug for JwksCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksCache").finish_non_exhaustive()
    }
}

impl JwksCache {
    /// The key set of `provider_id` at `jwks_uri`: the kept one when it was fetched from this
    /// URL under `client`'s policy less than [`JWKS_TTL`] before `now`, otherwise a new one.
    pub async fn get(
        &self,
        client: &OutboundClient,
        provider_id: Uuid,
        jwks_uri: &Url,
        now: SystemTime,
    ) -> Result<Arc<KeySet>, JwksError> {
        match self.kept(provider_id, jwks_uri, client.policy(), now) {
            Some((keys, _)) => Ok(keys),
            None => self.fetch(client, provider_id, jwks_uri, now).await,
        }
    }

    /// Fetch the key set again because an ID token names a key the kept one lacks (B10 1): a new
    /// set, or `None` when the kept set was fetched less than [`JWKS_REFETCH_INTERVAL`] before
    /// `now`, in which case nothing is fetched.
    pub async fn refetch(
        &self,
        client: &OutboundClient,
        provider_id: Uuid,
        jwks_uri: &Url,
        now: SystemTime,
    ) -> Result<Option<Arc<KeySet>>, JwksError> {
        if let Some((_, age)) = self.kept(provider_id, jwks_uri, client.policy(), now)
            && age < JWKS_REFETCH_INTERVAL
        {
            return Ok(None);
        }
        self.fetch(client, provider_id, jwks_uri, now)
            .await
            .map(Some)
    }

    async fn fetch(
        &self,
        client: &OutboundClient,
        provider_id: Uuid,
        jwks_uri: &Url,
        now: SystemTime,
    ) -> Result<Arc<KeySet>, JwksError> {
        let keys = Arc::new(fetch_jwks(client, jwks_uri).await?);
        let entry = Entry {
            jwks_uri: jwks_uri.clone(),
            policy: client.policy(),
            fetched_at: now,
            keys: Arc::clone(&keys),
        };
        let mut entries = self.lock();
        entries.retain(|_, kept| age(kept, now).is_some_and(|age| age < JWKS_TTL));
        entries.insert(provider_id, entry);
        Ok(keys)
    }

    /// The kept set and its age, when it is for `jwks_uri` under `policy` and younger than
    /// [`JWKS_TTL`] at `now`.
    fn kept(
        &self,
        provider_id: Uuid,
        jwks_uri: &Url,
        policy: UrlPolicy,
        now: SystemTime,
    ) -> Option<(Arc<KeySet>, Duration)> {
        let entries = self.lock();
        let entry = entries.get(&provider_id)?;
        if entry.jwks_uri != *jwks_uri || entry.policy != policy {
            return None;
        }
        let age = age(entry, now).filter(|age| *age < JWKS_TTL)?;
        Some((Arc::clone(&entry.keys), age))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Entry>> {
        // Nothing panics while the lock is held; a poisoned map still holds whole entries.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How long ago `entry` was fetched; `None` when it was fetched after `now` (the clock went
/// back), which makes it stale.
fn age(entry: &Entry, now: SystemTime) -> Option<Duration> {
    now.duration_since(entry.fetched_at).ok()
}
