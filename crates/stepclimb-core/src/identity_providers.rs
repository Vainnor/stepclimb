// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity providers repository, read side (docs/specs/m0/03-users-and-identities.md B13).
//!
//! A provider is one `identity_providers` row of an organization, found by its `key` (unique in
//! the organization, the `{provider}` of `/auth/{provider}/...`). Every query here filters by
//! `org_id = scope.org_id()`.
//!
//! `config` is a JSON object this module passes through without reading: its shape, the
//! secrets sealed inside it and the functions that write providers belong to
//! docs/specs/m0/05-oidc.md and 06-oauth2-and-vatsim-preset.md.

use std::time::SystemTime;

use serde_json::{Map, Value};
use sqlx::PgExecutor;
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
    /// A stored `kind` this release does not know.
    #[error("the stored provider kind {0:?} is not one this release knows")]
    UnknownKind(String),
    /// A stored `config` that is not a JSON object. The table's check constraint keeps this from
    /// happening, and the read fails closed if it does.
    #[error("the stored provider config is not a JSON object")]
    StoredConfig,
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
