// SPDX-License-Identifier: AGPL-3.0-or-later
//! Writing the `[[providers]]` of `stepclimb.toml` to their organizations when `stepclimb serve`
//! starts (docs/specs/m0/06-oauth2-and-vatsim-preset.md B12, Q2 (a) and (a); 05-oidc.md Q2 (c)).
//!
//! The file is the source of the providers it declares:
//!
//! - Each entry belongs to the organization its `org` slug names, or, without one, to the
//!   instance's only organization; on an instance with several, `org` is required. An entry
//!   that fits no organization stops startup.
//! - Each entry is checked again under the URL policy of the instance as it is now (private
//!   addresses only with one organization).
//! - In one transaction, which also reads the organizations, each entry is written to its
//!   organization's row with the same key (`identity_providers::upsert_from_config`) and marked
//!   `source: "config"`; every other row of the file that is still enabled is disabled, never
//!   deleted, so its identities stay.
//! - While a row has identities, the file may not take it over from the admin API or change
//!   what the provider is: its kind, issuer, endpoint origins or subject pointer
//!   ([`changed_anchors`]). Either stops startup; with no identity it goes ahead with a warning.
//! - A preset entry's row stores only what differs from its preset (`crate::presets`); the
//!   anchors and the fingerprint below are those of the resolved config, which sign-in reads.
//! - The client secrets stay in memory ([`ConfigSecrets`], by row), never in the database, each
//!   with the fingerprint of the config written, so a row another server rewrote from another
//!   file gets no secret here. Every server of an instance must read the same file.
//!
//! An instance with no organization yet has nowhere to put them: startup logs a warning and
//! goes on, and the next start after `stepclimb admin create` writes them.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use serde_json::{Map, Value};
use sqlx::{PgConnection, PgPool};
use stepclimb_auth::outbound::UrlPolicy;
use stepclimb_auth::provider_config::{ConfigSecrets, Fingerprint, ProviderRow};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    identity_providers::{self, ConfigRow, Kind},
    organizations::{self, Tenancy},
};

use crate::config::ConfigProvider;
use crate::config::providers::EntryError;
use crate::presets;

/// Why the file's providers could not be written.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// An entry that does not fit the instance: the configuration is refused.
    #[error("stepclimb.toml {0}")]
    Entry(EntryError),
    #[error("could not read the organizations: {0}")]
    Organizations(#[from] organizations::Error),
    #[error("could not write the providers of stepclimb.toml: {0}")]
    Providers(#[from] identity_providers::Error),
    #[error("could not write the providers of stepclimb.toml: {0}")]
    Database(#[from] sqlx::Error),
}

fn entry_error(provider: &ConfigProvider, problems: Vec<String>) -> ApplyError {
    ApplyError::Entry(EntryError {
        entry: provider.entry,
        key: Some(provider.key.clone()),
        problems,
    })
}

/// Write `providers` as the module docs say, with the insecure switch `allow_insecure`,
/// stamping changed rows `now`. Returns the secrets of the rows written. Every read and write
/// is on one transaction.
pub async fn apply(
    pool: &PgPool,
    providers: &[ConfigProvider],
    allow_insecure: bool,
    now: SystemTime,
) -> Result<ConfigSecrets, ApplyError> {
    let mut tx = pool.begin().await?;
    let tenancy = organizations::tenancy(&mut *tx).await?;
    if tenancy == Tenancy::Empty {
        if !providers.is_empty() {
            tracing::warn!(
                providers = providers.len(),
                "stepclimb.toml declares providers, but the instance has no organization yet; \
                 restart the server after `stepclimb admin create` to add them"
            );
        }
        return Ok(ConfigSecrets::default());
    }
    let policy = UrlPolicy::new(allow_insecure, matches!(tenancy, Tenancy::Single(_)));
    let mut by_org: BTreeMap<Uuid, Vec<Declared<'_>>> = BTreeMap::new();
    for provider in providers {
        let org_id = organization(&mut tx, provider, tenancy).await?;
        let stored = provider
            .stored(policy)
            .map_err(|problems| entry_error(provider, problems))?;
        by_org.entry(org_id).or_default().push((provider, stored));
    }
    check_unique_per_org(&by_org)?;
    let ids = organizations::ids(&mut *tx).await?;
    if let Some(missing) = unknown_org(by_org.keys().copied(), &ids)
        && let Some((provider, _)) = by_org.get(&missing).and_then(|d| d.first())
    {
        return Err(entry_error(
            provider,
            vec![
                "`org` names an organization that was removed while the server started".to_owned(),
            ],
        ));
    }
    let mut secrets = ConfigSecrets::default();
    for org_id in ids {
        let declared = by_org.remove(&org_id).unwrap_or_default();
        write_org(&mut tx, org_id, &declared, now, &mut secrets).await?;
    }
    tx.commit().await?;
    if !secrets.is_empty() {
        tracing::info!(
            providers = secrets.len(),
            "the providers of stepclimb.toml are up to date"
        );
    }
    Ok(secrets)
}

/// An entry and the config it stores.
type Declared<'a> = (&'a ConfigProvider, Map<String, Value>);

/// The first of `declared` that is not one of the instance's organizations `ids`.
fn unknown_org(mut declared: impl Iterator<Item = Uuid>, ids: &[Uuid]) -> Option<Uuid> {
    declared.find(|org_id| !ids.contains(org_id))
}

/// The organization `provider` belongs to.
async fn organization(
    tx: &mut PgConnection,
    provider: &ConfigProvider,
    tenancy: Tenancy,
) -> Result<Uuid, ApplyError> {
    match (&provider.org, tenancy) {
        (Some(slug), _) => organizations::id_by_slug(&mut *tx, slug)
            .await?
            .ok_or_else(|| {
                entry_error(
                    provider,
                    vec![format!(
                        "`org` names no organization of this instance ({slug:?})"
                    )],
                )
            }),
        (None, Tenancy::Single(org_id)) => Ok(org_id),
        (None, Tenancy::Multiple | Tenancy::Empty) => Err(entry_error(
            provider,
            vec!["`org` is required on an instance with more than one organization".to_owned()],
        )),
    }
}

/// Two entries that end up in one organization with the same key are refused, such as one
/// naming the only organization's slug and one naming none.
fn check_unique_per_org(by_org: &BTreeMap<Uuid, Vec<Declared<'_>>>) -> Result<(), ApplyError> {
    for declared in by_org.values() {
        let mut seen = BTreeMap::new();
        for (provider, _) in declared {
            if let Some(first) = seen.insert(provider.key.as_str(), provider.entry) {
                return Err(entry_error(
                    provider,
                    vec![format!(
                        "`key` is also the key of entry {first} for the same organization"
                    )],
                ));
            }
        }
    }
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a startup task; spec 06 Q2 writes the file's providers before serving"
)]
fn startup_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Write one organization's declared providers and disable the file's other rows there.
async fn write_org(
    tx: &mut PgConnection,
    org_id: Uuid,
    declared: &[Declared<'_>],
    now: SystemTime,
    secrets: &mut ConfigSecrets,
) -> Result<(), ApplyError> {
    let scope = startup_scope(org_id);
    let mut keys = BTreeSet::new();
    for (provider, config) in declared {
        check_trust_anchors(&mut *tx, scope, provider, config).await?;
        let row = ConfigRow {
            key: &provider.key,
            kind: provider.kind,
            config,
            enabled: provider.enabled,
        };
        let provider_id =
            identity_providers::upsert_from_config(&mut *tx, scope, &row, now).await?;
        secrets.insert(
            ProviderRow {
                org_id,
                provider_id,
            },
            provider.secret().clone(),
            Fingerprint::of(&resolved(config)),
        );
        keys.insert(provider.key.clone());
    }
    let keep: Vec<String> = keys.into_iter().collect();
    let disabled =
        identity_providers::disable_config_rows_except(&mut *tx, scope, &keep, now).await?;
    for key in disabled {
        tracing::warn!(
            %org_id,
            provider = %key,
            "provider {key:?} is no longer in stepclimb.toml; it is disabled and its identities are kept"
        );
    }
    Ok(())
}

/// What makes a provider the provider its identities were linked at: its kind, and for that
/// kind the issuer, or the origins of its three endpoints and the subject pointer. The members
/// of `stored` (the row) that `config` (the file's) changes, by name; `kind` alone when the kind
/// changes. A member missing or unreadable in either counts as changed.
pub(crate) fn changed_anchors(
    stored_kind: Kind,
    stored: &Map<String, Value>,
    kind: Kind,
    config: &Map<String, Value>,
) -> Vec<&'static str> {
    if stored_kind != kind {
        return vec!["kind"];
    }
    let text = |map: &Map<String, Value>, name: &str| {
        map.get(name).and_then(Value::as_str).map(str::to_owned)
    };
    let origin = |map: &Map<String, Value>, name: &str| {
        text(map, name)
            .and_then(|url| url::Url::parse(&url).ok())
            .map(|url| url.origin().ascii_serialization())
    };
    let subject = |map: &Map<String, Value>| {
        map.get("claims")
            .and_then(|claims| claims.get("subject"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let differs = |a: Option<String>, b: Option<String>| a.is_none() || a != b;
    let mut changed = Vec::new();
    match kind {
        Kind::Oidc | Kind::Local => {
            if differs(text(stored, "issuer"), text(config, "issuer")) {
                changed.push("issuer");
            }
        }
        Kind::Oauth2 => {
            for name in ["authorize", "token", "userinfo"] {
                if differs(origin(stored, name), origin(config, name)) {
                    changed.push(name);
                }
            }
            if differs(subject(stored), subject(config)) {
                changed.push("claims.subject");
            }
        }
    }
    changed
}

/// `stored` as the sign-in reads it ([`presets::resolve`]), the anchors and the fingerprint
/// being those of the config sign-in uses. A preset this server lacks leaves `stored` as it
/// is: it then has no endpoints, which [`changed_anchors`] counts as changed.
fn resolved(stored: &Map<String, Value>) -> Map<String, Value> {
    presets::resolve(stored).map_or_else(|_| stored.clone(), Cow::into_owned)
}

/// Refuse, while the row has identities, an entry that would take over a row the admin API
/// wrote or change what the provider is ([`changed_anchors`]): the users linked to it would
/// then sign in as whoever the new provider says (spec 06 B13). With no identity it goes ahead
/// with a warning.
async fn check_trust_anchors(
    tx: &mut PgConnection,
    scope: OrgScope,
    provider: &ConfigProvider,
    config: &Map<String, Value>,
) -> Result<(), ApplyError> {
    let Some(existing) = identity_providers::get_by_key(&mut *tx, scope, &provider.key).await?
    else {
        return Ok(());
    };
    let takeover = !existing.is_from_config_file();
    let changed = changed_anchors(
        existing.kind,
        &resolved(&existing.config),
        provider.kind,
        &resolved(config),
    );
    if !takeover && changed.is_empty() {
        return Ok(());
    }
    let identities = identity_providers::identity_count(&mut *tx, scope, existing.id).await?;
    if identities > 0 {
        let slug = organizations::get(&mut *tx, scope).await?.slug;
        let mut problems = Vec::new();
        if takeover {
            problems.push(format!(
                "would take over provider {:?} of organization {slug:?}, created through the \
                 admin API, which {identities} identities sign in with",
                provider.key
            ));
        }
        for member in &changed {
            problems.push(format!(
                "`{member}` differs from provider {:?} of organization {slug:?}, which \
                 {identities} identities sign in with; keep the stored value or remove those \
                 identities first",
                provider.key
            ));
        }
        return Err(entry_error(provider, problems));
    }
    if takeover {
        tracing::warn!(
            org_id = %scope.org_id(),
            provider = %provider.key,
            "provider {:?} was created through the admin API; stepclimb.toml now owns it",
            provider.key
        );
    }
    if !changed.is_empty() {
        tracing::warn!(
            org_id = %scope.org_id(),
            provider = %provider.key,
            from = existing.kind.as_str(),
            to = provider.kind.as_str(),
            "provider {:?} changes {}; no identity signs in with it yet",
            provider.key,
            changed.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn an_organization_missing_from_the_instance_is_found() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        assert_eq!(unknown_org([a, b].into_iter(), &[a, b]), None);
        assert_eq!(unknown_org([a, b].into_iter(), &[a]), Some(b));
    }

    #[test]
    fn the_anchors_are_the_kind_issuer_endpoint_origins_and_subject() {
        let oidc = map(json!({ "issuer": "https://idp.example.test/a", "label": "A" }));
        assert!(changed_anchors(Kind::Oidc, &oidc, Kind::Oidc, &oidc).is_empty());
        let relabeled = map(json!({ "issuer": "https://idp.example.test/a", "label": "B" }));
        assert!(changed_anchors(Kind::Oidc, &oidc, Kind::Oidc, &relabeled).is_empty());
        let moved = map(json!({ "issuer": "https://idp.example.test/b" }));
        assert_eq!(
            changed_anchors(Kind::Oidc, &oidc, Kind::Oidc, &moved),
            ["issuer"]
        );
        assert_eq!(
            changed_anchors(Kind::Oidc, &oidc, Kind::Oauth2, &oidc),
            ["kind"]
        );
        assert_eq!(
            changed_anchors(Kind::Oidc, &Map::new(), Kind::Oidc, &oidc),
            ["issuer"]
        );

        let oauth2 = map(json!({
            "authorize": "https://idp.example.test/oauth/authorize",
            "token": "https://idp.example.test/oauth/token",
            "userinfo": "https://idp.example.test/api/user",
            "claims": { "subject": "/data/cid" },
        }));
        let paths = map(json!({
            "authorize": "https://idp.example.test/v2/authorize?prompt=login",
            "token": "https://idp.example.test/v2/token",
            "userinfo": "https://idp.example.test/v2/user",
            "claims": { "subject": "/data/cid", "email": "/data/email" },
        }));
        assert!(
            changed_anchors(Kind::Oauth2, &oauth2, Kind::Oauth2, &paths).is_empty(),
            "a new path on the same origin is the same provider"
        );
        let elsewhere = map(json!({
            "authorize": "https://other.example.test/oauth/authorize",
            "token": "https://idp.example.test:8443/oauth/token",
            "userinfo": "http://idp.example.test/api/user",
            "claims": { "subject": "/data/id" },
        }));
        assert_eq!(
            changed_anchors(Kind::Oauth2, &oauth2, Kind::Oauth2, &elsewhere),
            ["authorize", "token", "userinfo", "claims.subject"]
        );
    }
}
