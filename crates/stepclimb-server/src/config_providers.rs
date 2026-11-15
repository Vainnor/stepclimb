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
//! - In one transaction, each entry is written to its organization's row with the same key
//!   (`identity_providers::upsert_from_config`), whoever wrote that row, and marked `source:
//!   "config"`; every other row of the file that is still enabled is disabled, never deleted,
//!   so its identities stay.
//! - The client secrets stay in memory ([`ConfigSecrets`], by row), never in the database.
//!
//! An instance with no organization yet has nowhere to put them: startup logs a warning and
//! goes on, and the next start after `stepclimb admin create` writes them.

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use sqlx::PgPool;
use stepclimb_auth::outbound::UrlPolicy;
use stepclimb_auth::provider_config::{ConfigSecrets, ProviderRow};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    identity_providers::{self, ConfigRow},
    organizations::{self, Tenancy},
};

use crate::config::ConfigProvider;
use crate::config::providers::EntryError;

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
/// stamping changed rows `now`. Returns the secrets of the rows written.
pub async fn apply(
    pool: &PgPool,
    providers: &[ConfigProvider],
    allow_insecure: bool,
    now: SystemTime,
) -> Result<ConfigSecrets, ApplyError> {
    let tenancy = organizations::tenancy(pool).await?;
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
    let mut by_org: BTreeMap<Uuid, Vec<(&ConfigProvider, serde_json::Map<_, _>)>> = BTreeMap::new();
    for provider in providers {
        let org_id = organization(pool, provider, tenancy).await?;
        let stored = provider
            .stored(policy)
            .map_err(|problems| entry_error(provider, problems))?;
        by_org.entry(org_id).or_default().push((provider, stored));
    }
    check_unique_per_org(&by_org)?;
    let mut tx = pool.begin().await?;
    let mut secrets = ConfigSecrets::default();
    for org_id in organizations::ids(&mut *tx).await? {
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

/// The organization `provider` belongs to.
async fn organization(
    pool: &PgPool,
    provider: &ConfigProvider,
    tenancy: Tenancy,
) -> Result<Uuid, ApplyError> {
    match (&provider.org, tenancy) {
        (Some(slug), _) => organizations::id_by_slug(pool, slug).await?.ok_or_else(|| {
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
fn check_unique_per_org<T>(
    by_org: &BTreeMap<Uuid, Vec<(&ConfigProvider, T)>>,
) -> Result<(), ApplyError> {
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
    tx: &mut sqlx::PgConnection,
    org_id: Uuid,
    declared: &[(&ConfigProvider, serde_json::Map<String, serde_json::Value>)],
    now: SystemTime,
    secrets: &mut ConfigSecrets,
) -> Result<(), ApplyError> {
    let scope = startup_scope(org_id);
    let mut keys = BTreeSet::new();
    for (provider, config) in declared {
        warn_about_takeover(&mut *tx, scope, provider).await?;
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

/// Log a warning when the file takes over a row the admin API wrote, or changes a row's kind:
/// existing identities then sign in through what the file now says.
async fn warn_about_takeover(
    tx: &mut sqlx::PgConnection,
    scope: OrgScope,
    provider: &ConfigProvider,
) -> Result<(), ApplyError> {
    let Some(existing) = identity_providers::get_by_key(&mut *tx, scope, &provider.key).await?
    else {
        return Ok(());
    };
    if !existing.is_from_config_file() {
        tracing::warn!(
            org_id = %scope.org_id(),
            provider = %provider.key,
            "provider {:?} was created through the admin API; stepclimb.toml now owns it",
            provider.key
        );
    }
    if existing.kind != provider.kind {
        tracing::warn!(
            org_id = %scope.org_id(),
            provider = %provider.key,
            from = existing.kind.as_str(),
            to = provider.kind.as_str(),
            "provider {:?} changes kind; its existing identities keep their subjects",
            provider.key
        );
    }
    Ok(())
}
