// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization's outside providers as the admin API reads and writes them
//! (docs/specs/m0/06-oauth2-and-vatsim-preset.md B14 to B17, I5).
//!
//! Only `oidc` and `oauth2` rows: the `local` row is invisible here, read and written by
//! [`crate::local_sign_in`] (B17). Every query filters by `org_id = scope.org_id()` and leaves
//! out `kind = 'local'`.
//!
//! The caller passes configs already validated and sealed by their kind's config type
//! (`stepclimb_auth::oidc_config`, `stepclimb_auth::oauth2_config`), with the label under
//! [`LABEL`]; this module checks only what the table needs: the key (Q5 (a), also a check
//! constraint), the label, the kind, and that no config claims to be `stepclimb.toml`'s. Every
//! config write goes through [`super::replace_config`], so a row keeps its admission rules.
//!
//! Each write takes the [`Actor`] and writes its audit entry on the same transaction (spec 09
//! B22): `identity_provider.create` with the row `after`, `identity_provider.update` with
//! `before` and `after` of each member that changed, and `identity_provider.delete` with the
//! row `before`. A config in an entry is [`public_config`]: never the sealed secret. A changed
//! secret shows only as `client_secret_replaced: true`.

use std::time::SystemTime;

use serde_json::{Map, Value, json};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use super::{
    CLIENT_SECRET, IdentityProvider, Kind, LABEL, Row, SOURCE, public_config, valid_key,
    valid_label,
};
use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    local_sign_in,
};

/// The unique constraint on `(org_id, key)`.
const ORG_KEY_UNIQUE: &str = "identity_providers_org_id_key_key";

/// The foreign key from `identities` to `identity_providers`, whose default action refuses
/// deleting a provider that identities still name.
const IDENTITY_PROVIDER_FKEY: &str = "identities_org_id_provider_id_fkey";

/// Why an admin write was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no outside provider with that id.
    #[error("the provider does not exist")]
    NotFound,
    /// The key is not 1 to 32 lowercase letters, digits and hyphens starting with a letter or
    /// digit, or it is `local` (Q5 (a)).
    #[error("a provider key is 1 to 32 lowercase letters, digits and hyphens, and not local")]
    KeyInvalid,
    /// Another provider of the organization has the key (B15).
    #[error("another provider of the organization has this key")]
    KeyTaken,
    /// The config has no label, or one that breaks [`valid_label`].
    #[error("a provider label is 1 to 200 characters without control characters")]
    LabelInvalid,
    /// The kind is `local`, which the admin API does not create (B17).
    #[error("the admin API creates only oidc and oauth2 providers")]
    KindUnsupported,
    /// The config names an owner ([`SOURCE`]): only startup writes `stepclimb.toml`'s rows.
    #[error("a provider written through the admin API cannot name an owner")]
    SourceMember,
    /// Identities still link users to the provider (Q5 (a)).
    #[error("identities still link users to the provider")]
    InUse,
    #[error(transparent)]
    Provider(#[from] super::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(ORG_KEY_UNIQUE) => Self::KeyTaken,
            Some(IDENTITY_PROVIDER_FKEY) => Self::InUse,
            _ => Self::Database(error),
        }
    }
}

/// A provider to create. `id` is new (the secret in `config` is sealed to it).
pub struct NewProvider<'a> {
    pub id: Uuid,
    pub key: &'a str,
    /// [`Kind::Oidc`] or [`Kind::Oauth2`].
    pub kind: Kind,
    /// The whole stored config: the kind's config and [`LABEL`].
    pub config: &'a Map<String, Value>,
    pub enabled: bool,
}

/// The scope's outside provider `id`, or `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<IdentityProvider>, Error> {
    let row = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND id = $2 AND kind <> $3"#,
        scope.org_id(),
        id,
        Kind::Local.as_str(),
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(IdentityProvider::try_from).transpose()?)
}

/// [`get`], locking the row `FOR UPDATE` until the caller's transaction ends, so the checks a
/// write makes on it, and on its identities, hold until the write commits: linking an identity
/// to the row waits for the lock.
pub async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<IdentityProvider>, Error> {
    let row = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND id = $2 AND kind <> $3
           FOR UPDATE"#,
        scope.org_id(),
        id,
        Kind::Local.as_str(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(IdentityProvider::try_from).transpose()?)
}

/// At most `limit` of the scope's outside providers, enabled or not, sorted by key, after the
/// key `after` when there is one. `after` comes from a cursor: it is only a bound.
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<IdentityProvider>, Error> {
    if after.is_some_and(|key| key.contains('\0')) {
        return Err(super::Error::ContainsNul { field: "cursor" }.into());
    }
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND kind <> $2 AND ($3::text IS NULL OR key > $3)
           ORDER BY key
           LIMIT $4"#,
        scope.org_id(),
        Kind::Local.as_str(),
        after,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| IdentityProvider::try_from(row).map_err(Error::from))
        .collect()
}

/// Refuse a config the admin API may not store: one without a valid label, or naming an owner.
fn check_config(config: &Map<String, Value>) -> Result<(), Error> {
    if config.contains_key(SOURCE) {
        return Err(Error::SourceMember);
    }
    match config.get(LABEL).and_then(Value::as_str) {
        Some(label) if valid_label(label) => Ok(()),
        _ => Err(Error::LabelInvalid),
    }
}

/// Create `new` in the scope's organization, by `actor` at `now`, with its
/// `identity_provider.create` entry. The row starts with an empty config, which
/// [`super::replace_config`] then fills: the config has one writer.
pub async fn create(
    conn: &mut PgConnection,
    scope: OrgScope,
    new: &NewProvider<'_>,
    now: SystemTime,
    actor: Actor,
) -> Result<IdentityProvider, Error> {
    if new.kind == Kind::Local {
        return Err(Error::KindUnsupported);
    }
    if !valid_key(new.key) || new.key == local_sign_in::PROVIDER_KEY {
        return Err(Error::KeyInvalid);
    }
    check_config(new.config)?;
    let micros = epoch_micros(now)?;
    sqlx::query!(
        r#"INSERT INTO identity_providers
               (id, org_id, key, kind, config, enabled, created_at, updated_at)
           VALUES ($1, $2, $3, $4, '{}'::jsonb, $5,
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond')"#,
        new.id,
        scope.org_id(),
        new.key,
        new.kind.as_str(),
        new.enabled,
        micros,
    )
    .execute(&mut *conn)
    .await?;
    super::replace_config(&mut *conn, scope, new.id, new.config, now).await?;
    let created = lock(&mut *conn, scope, new.id)
        .await?
        .ok_or(Error::NotFound)?;
    let data = json!({ "after": snapshot(&created) });
    let action = Action::IdentityProviderCreate;
    audit::record(conn, scope, actor, action, created.id, &data, micros).await?;
    Ok(created)
}

/// What an update sets: the whole new config (the kind's config and [`LABEL`]) and `enabled`.
pub struct Change<'a> {
    pub config: &'a Map<String, Value>,
    pub enabled: bool,
}

/// Write `change` to `current`, a row [`lock`] read on this transaction, by `actor` at `now`.
/// A change equal to the row writes nothing and returns the row as it is. Otherwise it writes
/// what differs and the `identity_provider.update` entry, and returns the row as written.
pub async fn update(
    conn: &mut PgConnection,
    scope: OrgScope,
    current: &IdentityProvider,
    change: &Change<'_>,
    now: SystemTime,
    actor: Actor,
) -> Result<IdentityProvider, Error> {
    check_config(change.config)?;
    let mut stored = current.config.clone();
    stored.remove(crate::admission_rules::PROVIDER_MEMBER);
    let config_changed = stored != *change.config;
    let enabled_changed = current.enabled != change.enabled;
    if !config_changed && !enabled_changed {
        return Ok(current.clone());
    }
    let micros = epoch_micros(now)?;
    if enabled_changed {
        sqlx::query!(
            "UPDATE identity_providers
             SET enabled = $3,
                 updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
             WHERE org_id = $1 AND id = $2",
            scope.org_id(),
            current.id,
            change.enabled,
            micros,
        )
        .execute(&mut *conn)
        .await?;
    }
    if config_changed {
        super::replace_config(&mut *conn, scope, current.id, change.config, now).await?;
    }
    let updated = lock(&mut *conn, scope, current.id)
        .await?
        .ok_or(Error::NotFound)?;
    let data = changes(current, &updated);
    let action = Action::IdentityProviderUpdate;
    audit::record(conn, scope, actor, action, current.id, &data, micros).await?;
    Ok(updated)
}

/// Delete `current`, a row [`lock`] read on this transaction, by `actor` at `now`, with its
/// `identity_provider.delete` entry. [`Error::InUse`] while identities name it (Q5 (a)); its
/// open sign-in flows go with it (their foreign key cascades).
pub async fn delete(
    conn: &mut PgConnection,
    scope: OrgScope,
    current: &IdentityProvider,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    if super::identity_count(&mut *conn, scope, current.id).await? > 0 {
        return Err(Error::InUse);
    }
    let micros = epoch_micros(now)?;
    let deleted = sqlx::query!(
        "DELETE FROM identity_providers WHERE org_id = $1 AND id = $2 AND kind <> $3",
        scope.org_id(),
        current.id,
        Kind::Local.as_str(),
    )
    .execute(&mut *conn)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    let data = json!({ "before": snapshot(current) });
    let action = Action::IdentityProviderDelete;
    audit::record(conn, scope, actor, action, current.id, &data, micros).await?;
    Ok(())
}

/// A row as an audit entry shows it: never its secret.
fn snapshot(provider: &IdentityProvider) -> Value {
    json!({
        "key": provider.key,
        "kind": provider.kind.as_str(),
        "label": provider.label(),
        "enabled": provider.enabled,
        "config": public_config(&provider.config),
    })
}

/// `before` and `after` of each member of `from` that `to` changes (label, enabled, the public
/// config), and `client_secret_replaced: true` when the sealed secret differs.
fn changes(from: &IdentityProvider, to: &IdentityProvider) -> Value {
    let mut before = Map::new();
    let mut after = Map::new();
    let mut member = |name: &str, old: Value, new: Value| {
        if old != new {
            before.insert(name.to_owned(), old);
            after.insert(name.to_owned(), new);
        }
    };
    member("label", json!(from.label()), json!(to.label()));
    member("enabled", json!(from.enabled), json!(to.enabled));
    let config = |p: &IdentityProvider| Value::Object(public_config(&p.config));
    member("config", config(from), config(to));
    let mut data = json!({ "before": before, "after": after });
    if from.config.get(CLIENT_SECRET) != to.config.get(CLIENT_SECRET) {
        data["client_secret_replaced"] = Value::Bool(true);
    }
    data
}
