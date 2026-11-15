// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity providers repository, read side (docs/specs/m0/03-users-and-identities.md B13).
//!
//! A provider is one `identity_providers` row of an organization, found by its `key` (unique in
//! the organization, the `{provider}` of `/auth/{provider}/...`). Every query here filters by
//! `org_id = scope.org_id()`.
//!
//! `config` is a JSON object this module passes through without reading, but for its `source`
//! member: its shape and the secrets sealed inside it belong to docs/specs/m0/05-oidc.md and
//! 06-oauth2-and-vatsim-preset.md. A row whose config has `source: "config"` belongs to
//! `stepclimb.toml` (spec 06 Q2): [`upsert_from_config`] writes it at startup and
//! [`disable_config_rows_except`] disables it once the file no longer declares it.

use std::time::SystemTime;

use serde_json::{Map, Value};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, from_epoch_micros},
};

/// The protocol a provider speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// OpenID Connect (spec 05).
    Oidc,
    /// Generic OAuth 2 with a JSON user endpoint (spec 06).
    Oauth2,
    /// The organization's own accounts: the one row with key `local` that holds the local
    /// sign-in methods (spec 04 B1, [`crate::local_sign_in`]).
    Local,
}

impl Kind {
    /// Every kind, in the order the `identity_providers.kind` check constraint lists them.
    pub const ALL: [Self; 3] = [Self::Oidc, Self::Oauth2, Self::Local];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Oauth2 => "oauth2",
            Self::Local => "local",
        }
    }

    /// The kind a stored spelling names, or `None` for any other string.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == stored)
    }
}

/// The config member naming who owns the row, and its value for a row `stepclimb.toml`
/// declares. `stepclimb_auth::provider_config::{SOURCE, SOURCE_CONFIG}` read and write the same
/// member; a server test keeps the two in step.
pub const SOURCE: &str = "source";
/// [`SOURCE`]'s value for a provider `stepclimb.toml` declares.
pub const SOURCE_CONFIG: &str = "config";

/// The longest provider key (spec 06 Q5 (a)).
pub const KEY_MAX_LEN: usize = 32;

/// Whether `key` is a provider key of spec 06 Q5 (a): 1 to 32 lowercase ASCII letters, digits
/// and hyphens, starting with a letter or digit. It appears in `/auth/{key}/...` URLs and in the
/// redirect URI registered at the provider.
#[must_use]
pub fn valid_key(key: &str) -> bool {
    let first_ok = key
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first_ok
        && key.len() <= KEY_MAX_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A provider as the repository reads it.
#[derive(Clone, PartialEq, Eq)]
pub struct IdentityProvider {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    /// The provider's configuration, as stored; secrets inside it are sealed by their writer.
    pub config: Map<String, Value>,
    pub enabled: bool,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

impl IdentityProvider {
    /// Whether `stepclimb.toml` owns the row (its config has `source: "config"`).
    #[must_use]
    pub fn is_from_config_file(&self) -> bool {
        self.config.get(SOURCE).and_then(Value::as_str) == Some(SOURCE_CONFIG)
    }
}

/// Leaves `config` out: it holds the provider's sealed client secret.
impl std::fmt::Debug for IdentityProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityProvider")
            .field("id", &self.id)
            .field("key", &self.key)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// Why an identity providers repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// The scope's organization has no provider with that id.
    #[error("the provider does not exist")]
    NotFound,
    /// A stored `kind` this release does not know.
    #[error("the stored provider kind {0:?} is not one this release knows")]
    UnknownKind(String),
    /// A stored `config` that is not a JSON object. The table's check constraint keeps this from
    /// happening, and the read fails closed if it does.
    #[error("the stored provider config is not a JSON object")]
    StoredConfig,
    /// [`upsert_from_config`] was given a config without `source: "config"`.
    #[error("a provider written from stepclimb.toml must have source \"config\"")]
    NotFromConfig,
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// An `identity_providers` row as the queries select it, `config` as JSON text.
struct Row {
    id: Uuid,
    key: String,
    kind: String,
    config: String,
    enabled: bool,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for IdentityProvider {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let kind = Kind::parse(&row.kind).ok_or(Error::UnknownKind(row.kind))?;
        let config = serde_json::from_str(&row.config).map_err(|_| Error::StoredConfig)?;
        Ok(Self {
            id: row.id,
            key: row.key,
            kind,
            config,
            enabled: row.enabled,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The provider with this `key` in the scope's organization, enabled or not, or `None` when the
/// organization has none. A key holding NUL is refused with [`Error::ContainsNul`].
pub async fn get_by_key(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    key: &str,
) -> Result<Option<IdentityProvider>, Error> {
    if key.contains('\0') {
        return Err(Error::ContainsNul { field: "key" });
    }
    sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND key = $2"#,
        scope.org_id(),
        key,
    )
    .fetch_optional(db)
    .await?
    .map(IdentityProvider::try_from)
    .transpose()
}

/// The scope's enabled providers but the local one, sorted by key: the outside providers a
/// person can sign in with. The local row's methods are [`crate::local_sign_in`]'s.
pub async fn list_enabled(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Vec<IdentityProvider>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND enabled AND kind <> $2
           ORDER BY key"#,
        scope.org_id(),
        Kind::Local.as_str(),
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(IdentityProvider::try_from).collect()
}

/// The scope's providers, enabled or not and the local one included, at which the user
/// `user_id` has an identity, sorted by key, each once. Empty for a user the organization does
/// not have.
pub async fn list_for_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Vec<IdentityProvider>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT p.id, p.key, p.kind, p.config::text AS "config!", p.enabled,
                  (extract(epoch FROM p.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM p.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers p
           WHERE p.org_id = $1
             AND EXISTS (
                 SELECT 1 FROM identities i
                 WHERE i.org_id = p.org_id AND i.provider_id = p.id AND i.user_id = $2
             )
           ORDER BY p.key"#,
        scope.org_id(),
        user_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(IdentityProvider::try_from).collect()
}

/// The scope's providers but the local one, enabled or not, sorted by key: every provider an
/// admission rule list can belong to (docs/specs/m0/07-admission.md Q2 (a)).
pub async fn list_outside(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Vec<IdentityProvider>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND kind <> $2
           ORDER BY key"#,
        scope.org_id(),
        Kind::Local.as_str(),
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(IdentityProvider::try_from).collect()
}

/// [`list_outside`], locking each row `FOR NO KEY UPDATE` until the caller's transaction ends,
/// in key order, so a write of their admission rules replaces the lists read here.
pub async fn lock_outside(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Vec<IdentityProvider>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, key, kind, config::text AS "config!", enabled,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identity_providers
           WHERE org_id = $1 AND kind <> $2
           ORDER BY key
           FOR NO KEY UPDATE"#,
        scope.org_id(),
        Kind::Local.as_str(),
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(IdentityProvider::try_from).collect()
}

/// Replace the config of the scope's provider `id` with `config`, keeping the stored
/// admission rules (`crate::admission_rules::PROVIDER_MEMBER`, docs/specs/m0/07-admission.md
/// I3): the member is taken from the stored row in SQL, and one in `config` is dropped, so no
/// config write can remove, or set, a provider's rules. Every writer of a provider's config
/// but `admission_rules::update` goes through this. Sets `updated_at` to `now` when the config
/// changes. [`Error::NotFound`] when the organization has no such provider.
pub async fn replace_config(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    config: &Map<String, Value>,
    now: SystemTime,
) -> Result<(), Error> {
    let config = serde_json::to_string(config).map_err(|_| Error::StoredConfig)?;
    let now = crate::db::epoch_micros(now)?;
    let updated = sqlx::query!(
        r#"UPDATE identity_providers p
           SET config = n.config,
               updated_at = CASE WHEN p.config IS DISTINCT FROM n.config
                   THEN timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
                   ELSE p.updated_at
               END
           FROM (
               SELECT id,
                      CASE WHEN config ? 'admission_rules'
                          THEN ($3::text::jsonb - 'admission_rules')
                               || jsonb_build_object('admission_rules', config -> 'admission_rules')
                          ELSE $3::text::jsonb - 'admission_rules'
                      END AS config
               FROM identity_providers
               WHERE org_id = $1 AND id = $2
           ) n
           WHERE p.org_id = $1 AND p.id = n.id"#,
        scope.org_id(),
        id,
        config,
        now,
    )
    .execute(db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())

/// A provider `stepclimb.toml` declares, as [`upsert_from_config`] writes it.
pub struct ConfigRow<'a> {
    pub key: &'a str,
    /// [`Kind::Oidc`] or [`Kind::Oauth2`].
    pub kind: Kind,
    /// The whole stored config, with `source: "config"`.
    pub config: &'a Map<String, Value>,
    pub enabled: bool,
}

/// Write `row` into the scope's organization by its key (spec 06 Q2 (a)): a new row with a new
/// id, or the existing row with that key, whoever wrote it, taking the file's kind, config
/// and `enabled`. The config goes through [`replace_config`], so the row keeps the admission
/// rules it has (docs/specs/m0/07-admission.md I3). `updated_at` moves to `now` only when
/// something changed. Returns the row's id. A config without `source: "config"` is refused with
/// [`Error::NotFromConfig`], so the rows this writes are always the file's.
pub async fn upsert_from_config(
    conn: &mut PgConnection,
    scope: OrgScope,
    row: &ConfigRow<'_>,
    now: SystemTime,
) -> Result<Uuid, Error> {
    if row.key.contains('\0') {
        return Err(Error::ContainsNul { field: "key" });
    }
    if row.config.get(SOURCE).and_then(Value::as_str) != Some(SOURCE_CONFIG) {
        return Err(Error::NotFromConfig);
    }
    let micros = crate::db::epoch_micros(now)?;
    // A new row starts with an empty config, which `replace_config` then fills: the config has
    // one writer.
    let id = sqlx::query_scalar!(
        r#"INSERT INTO identity_providers
               (id, org_id, key, kind, config, enabled, created_at, updated_at)
           VALUES ($1, $2, $3, $4, '{}'::jsonb, $5,
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond')
           ON CONFLICT (org_id, key) DO UPDATE SET
               kind = EXCLUDED.kind,
               enabled = EXCLUDED.enabled,
               updated_at = CASE
                   WHEN (identity_providers.kind, identity_providers.enabled)
                        IS DISTINCT FROM (EXCLUDED.kind, EXCLUDED.enabled)
                   THEN EXCLUDED.updated_at
                   ELSE identity_providers.updated_at
               END
           RETURNING id"#,
        crate::id::new_id(),
        scope.org_id(),
        row.key,
        row.kind.as_str(),
        row.enabled,
        micros,
    )
    .fetch_one(&mut *conn)
    .await?;
    replace_config(&mut *conn, scope, id, row.config, now).await?;
    Ok(id)
}

/// Disable every enabled row of the scope's organization that `stepclimb.toml` owns and whose
/// key is not in `keep` (spec 06 Q2 (a): a provider removed from the file is disabled, not
/// deleted, so its identities stay). Returns the keys it disabled, sorted.
pub async fn disable_config_rows_except(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    keep: &[String],
    now: SystemTime,
) -> Result<Vec<String>, Error> {
    let now = crate::db::epoch_micros(now)?;
    let mut keys = sqlx::query_scalar!(
        r#"UPDATE identity_providers
           SET enabled = false, updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND enabled AND config ->> $2 = $3 AND NOT (key = ANY($5))
           RETURNING key"#,
        scope.org_id(),
        SOURCE,
        SOURCE_CONFIG,
        now,
        keep,
    )
    .fetch_all(db)
    .await?;
    keys.sort();
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_stored_spelling() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("saml"), None);
        assert_eq!(Kind::parse("Local"), None);
    }

    #[test]
    fn provider_keys_follow_q5() {
        for key in ["vatsim", "a", "0", "corp-sso", "a1-b2", &"k".repeat(32)] {
            assert!(valid_key(key), "{key:?}");
        }
        for key in [
            "",
            "-sso",
            "Vatsim",
            "corp_sso",
            "corp sso",
            "corp.sso",
            "é",
            &"k".repeat(33),
        ] {
            assert!(!valid_key(key), "{key:?}");
        }
    }

    #[test]
    fn a_row_is_from_the_config_file_only_with_source_config() {
        let mut provider = IdentityProvider {
            id: Uuid::nil(),
            key: "example".into(),
            kind: Kind::Oidc,
            config: Map::new(),
            enabled: true,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        };
        assert!(!provider.is_from_config_file());
        provider.config.insert(SOURCE.into(), Value::from("api"));
        assert!(!provider.is_from_config_file());
        provider.config.insert(SOURCE.into(), Value::from(true));
        assert!(!provider.is_from_config_file());
        provider
            .config
            .insert(SOURCE.into(), Value::from(SOURCE_CONFIG));
        assert!(provider.is_from_config_file());
    }

    #[test]
    fn debug_leaves_the_config_out() {
        let mut config = Map::new();
        config.insert("client_secret".into(), Value::from("sealed-secret-bytes"));
        let provider = IdentityProvider {
            id: Uuid::nil(),
            key: "example".into(),
            kind: Kind::Oidc,
            config,
            enabled: true,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        };
        let debug = format!("{provider:?}");
        assert!(debug.contains("example"), "{debug}");
        assert!(!debug.contains("sealed-secret-bytes"), "{debug}");
    }
}
