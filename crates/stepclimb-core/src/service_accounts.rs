// SPDX-License-Identifier: AGPL-3.0-or-later
//! The service accounts repository and the API-key credential resolver
//! (docs/specs/m0/08-sessions-and-csrf.md B24 to B29, Data, Q9).
//!
//! A service account is one row in `service_accounts` and holds one API key: rotating a key is
//! creating a new account and deleting the old one. Every account has an owner, a user of its
//! organization (Q9 (a)), and a list of scopes, each a catalog permission key. The key's rights
//! on a request are its scopes intersected with the owner's grants of that request
//! (`grants::Grants::intersect_key_scopes`); this module stores and finds the account, and the
//! server intersects.
//!
//! The repository never sees an API key, only its SHA-256 hash
//! (`stepclimb_auth::token::TokenHash::as_bytes`), and the caller generates the key, so this
//! module cannot store or log one. [`authenticate`] finds the account by that hash through the
//! unique index, within the request's organization; it is the one place outside tests that
//! builds an [`OrgScope`] from an API key.
//!
//! [`create`] and [`delete`] write the `service_account.create` and `service_account.delete`
//! audit entries (docs/specs/m0/09-permissions-and-roles.md B22, spec 08 Q14) in their
//! transaction, with the name, kind, owner and scopes, never the key or its hash.

use std::time::{Duration, SystemTime};

use serde_json::json;
use sqlx::{Acquire, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name::{self, Invalid},
    perm,
    scope::Credential,
    users::Status,
};

/// The least time between two writes of `last_used_at`, as for a session's `last_seen_at`, so a
/// burst of requests with one key writes its row once.
pub const TOUCH_INTERVAL: Duration = Duration::from_secs(60);

/// The longest service account name, in characters, as for other names.
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// What a service account is. M0 has one kind (Q9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An account that holds one API key.
    ApiKey,
}

impl Kind {
    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
        }
    }

    fn parse(stored: &str) -> Option<Self> {
        (stored == "api_key").then_some(Self::ApiKey)
    }
}

/// A service account to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewServiceAccount {
    /// Trimmed before it is stored; 1 to [`MAX_NAME_CHARS`] characters.
    pub name: String,
    /// A user of the scope's organization.
    pub owner_user_id: Uuid,
    /// Catalog permission keys, each at most once.
    pub scopes: Vec<String>,
}

/// A stored service account, without its key's hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceAccount {
    pub id: Uuid,
    pub kind: Kind,
    pub name: String,
    pub owner_user_id: Uuid,
    pub scopes: Vec<String>,
    pub created_at: SystemTime,
    /// The last request its key authenticated, to the minute ([`TOUCH_INTERVAL`]); `None`
    /// before the first.
    pub last_used_at: Option<SystemTime>,
}

/// What is wrong with one entry of a new account's scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeProblem {
    /// Not a key of the permission catalog.
    Unknown,
    /// The same key as an earlier entry.
    Duplicate,
}

/// Why a service accounts repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the owner does not exist")]
    OwnerNotFound,
    /// No service account with that id in the scope's organization.
    #[error("the service account does not exist")]
    NotFound,
    /// The name is empty or blank.
    #[error("the service account name is empty")]
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`].
    #[error("the service account name is longer than {MAX_NAME_CHARS} characters")]
    NameTooLong,
    /// The name contains NUL, which PostgreSQL `text` cannot store.
    #[error("the service account name contains NUL")]
    NameContainsNul,
    /// The scope at `index` of the list is refused.
    #[error("scope {index} is refused: {problem:?}")]
    Scope { index: usize, problem: ScopeProblem },
    /// A stored `kind` or owner `status` this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    Stored { column: &'static str, value: String },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The first refused entry of `scopes`: a key outside the permission catalog, or a key listed
/// twice.
pub fn check_scopes<S: AsRef<str>>(scopes: &[S]) -> Result<(), Error> {
    for (index, scope) in scopes.iter().enumerate() {
        let scope = scope.as_ref();
        let problem = if !perm::ALL.iter().any(|p| p.key() == scope) {
            ScopeProblem::Unknown
        } else if scopes[..index].iter().any(|s| s.as_ref() == scope) {
            ScopeProblem::Duplicate
        } else {
            continue;
        };
        return Err(Error::Scope { index, problem });
    }
    Ok(())
}

fn valid_name(name: &str) -> Result<&str, Error> {
    name::valid(name).map_err(|invalid| match invalid {
        Invalid::Empty => Error::NameEmpty,
        Invalid::TooLong => Error::NameTooLong,
        Invalid::ContainsNul => Error::NameContainsNul,
    })
}

/// Store `account` in the scope's organization at `now`, holding the key whose SHA-256 is
/// `secret_hash`, and write its `service_account.create` audit entry by `actor`. Runs in one
/// transaction (a savepoint when `db` is already one).
///
/// `secret_hash` is the hash of a key the caller has just generated for this account; never pass
/// a key the request brought. The name is trimmed; the scopes are checked with
/// [`check_scopes`]. An owner the organization does not have is [`Error::OwnerNotFound`]: the
/// insert takes the owner from a `SELECT` filtered by the scope, so the refusal does not abort
/// the caller's transaction.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    account: &NewServiceAccount,
    secret_hash: &[u8; 32],
    now: SystemTime,
    actor: Actor,
) -> Result<ServiceAccount, Error> {
    let name = valid_name(&account.name)?;
    check_scopes(&account.scopes)?;
    let now_us = epoch_micros(now)?;
    let id = new_id();
    let kind = Kind::ApiKey;
    let mut tx = db.begin().await?;
    let created_at_us = sqlx::query_scalar!(
        r#"INSERT INTO service_accounts (id, org_id, kind, name, owner_user_id, secret_hash,
                                         scopes, created_at, updated_at)
           SELECT $1, u.org_id, $3, $4, u.id, $6, $7,
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.org_id = $2 AND u.id = $5
           RETURNING (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!""#,
        id,
        scope.org_id(),
        kind.as_str(),
        name,
        account.owner_user_id,
        secret_hash.as_slice(),
        &account.scopes,
        now_us,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::OwnerNotFound)?;
    let data = json!({ "after": {
        "name": name,
        "kind": kind.as_str(),
        "owner_user_id": account.owner_user_id,
        "scopes": account.scopes,
    } });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::ServiceAccountCreate,
        id,
        &data,
        now_us,
    )
    .await?;
    tx.commit().await?;
    Ok(ServiceAccount {
        id,
        kind,
        name: name.to_owned(),
        owner_user_id: account.owner_user_id,
        scopes: account.scopes.clone(),
        created_at: from_epoch_micros(created_at_us)?,
        last_used_at: None,
    })
}

/// The sort key of [`list`]: the account's id, which is UUID v7 and so in creation order.
pub type ListKey = Uuid;

/// The scope's service accounts by id, after `after`, at most `limit`.
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<ServiceAccount>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, kind, name, owner_user_id, scopes,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM last_used_at) * 1000000)::bigint AS last_used_at_us
           FROM service_accounts
           WHERE org_id = $1 AND ($2::uuid IS NULL OR id > $2)
           ORDER BY id
           LIMIT $3"#,
        scope.org_id(),
        after.copied(),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(ServiceAccount::try_from).collect()
}

/// A `service_accounts` row as [`list`] selects it.
struct Row {
    id: Uuid,
    kind: String,
    name: String,
    owner_user_id: Uuid,
    scopes: Vec<String>,
    created_at_us: i64,
    last_used_at_us: Option<i64>,
}

impl TryFrom<Row> for ServiceAccount {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let kind = Kind::parse(&row.kind).ok_or(Error::Stored {
            column: "kind",
            value: row.kind,
        })?;
        Ok(Self {
            id: row.id,
            kind,
            name: row.name,
            owner_user_id: row.owner_user_id,
            scopes: row.scopes,
            created_at: from_epoch_micros(row.created_at_us)?,
            last_used_at: row.last_used_at_us.map(from_epoch_micros).transpose()?,
        })
    }
}

/// Delete the service account `id` of the scope's organization, so its key fails on the next
/// request (B29), and write its `service_account.delete` audit entry by `actor`. Another
/// organization's id, like an id that does not exist, is [`Error::NotFound`].
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let deleted = sqlx::query!(
        "DELETE FROM service_accounts WHERE org_id = $1 AND id = $2
         RETURNING kind, name, owner_user_id, scopes",
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": {
        "name": deleted.name,
        "kind": deleted.kind,
        "owner_user_id": deleted.owner_user_id,
        "scopes": deleted.scopes,
    } });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::ServiceAccountDelete,
        id,
        &data,
        now_us,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The principal a valid API key authenticates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPrincipal {
    pub service_account_id: Uuid,
    /// The owner, whose current grants the scopes narrow.
    pub user_id: Uuid,
    /// The key's scopes as stored.
    pub scopes: Vec<String>,
    /// The organization, with origin `ApiKey`.
    pub scope: OrgScope,
}

/// What a presented API key turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyAuthentication {
    /// The key is valid; `last_used_at` was refreshed if [`TOUCH_INTERVAL`] had passed.
    Valid(KeyPrincipal),
    /// The key's owner is suspended (B27); the account is kept.
    Suspended,
    /// No service account of the request's organization has this hash (B6, B26, B29).
    Unknown,
}

/// A service account joined with its owner, as [`authenticate`] reads it.
struct Found {
    id: Uuid,
    owner_user_id: Uuid,
    org_id: Uuid,
    scopes: Vec<String>,
    status: String,
    last_used_at_us: Option<i64>,
}

/// Authenticate a presented API key within the request's organization (B6, B26, B27).
///
/// `request_org` is the organization the request belongs to, from the request-organization
/// resolver; an account of another organization is [`KeyAuthentication::Unknown`] and is left
/// untouched. The owner's status is read on every call, so a suspension takes effect on the
/// next request. A valid key has its `last_used_at` set to `now` when [`TOUCH_INTERVAL`] has
/// passed since the last write.
pub async fn authenticate<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    request_org: OrgScope,
    secret_hash: &[u8; 32],
    now: SystemTime,
) -> Result<KeyAuthentication, Error> {
    let mut conn = db.acquire().await?;
    let found = sqlx::query_as!(
        Found,
        r#"SELECT s.id, s.owner_user_id, s.org_id, s.scopes, u.status,
                  (extract(epoch FROM s.last_used_at) * 1000000)::bigint AS last_used_at_us
           FROM service_accounts s
           JOIN users u ON u.org_id = s.org_id AND u.id = s.owner_user_id
           WHERE s.org_id = $1 AND s.secret_hash = $2"#,
        request_org.org_id(),
        secret_hash.as_slice(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(found) = found else {
        return Ok(KeyAuthentication::Unknown);
    };
    match Status::parse(&found.status) {
        Some(Status::Active) => {}
        Some(Status::Suspended) => return Ok(KeyAuthentication::Suspended),
        None => {
            return Err(Error::Stored {
                column: "status",
                value: found.status,
            });
        }
    }
    let last_used_at = found.last_used_at_us.map(from_epoch_micros).transpose()?;
    if needs_touch(now, last_used_at) {
        touch(&mut *conn, request_org, found.id, now).await?;
    }
    Ok(KeyAuthentication::Valid(KeyPrincipal {
        service_account_id: found.id,
        user_id: found.owner_user_id,
        scopes: found.scopes,
        scope: key_scope(found.org_id),
    }))
}

/// The scope of a request authenticated by an API key of a service account in `org_id`.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9: the API-key resolver builds the credential scope from the service account row"
)]
fn key_scope(org_id: Uuid) -> OrgScope {
    OrgScope::from_credential(org_id, Credential::ApiKey)
}

/// Whether `last_used_at` is old enough at `now` to be written again.
fn needs_touch(now: SystemTime, last_used_at: Option<SystemTime>) -> bool {
    last_used_at.is_none_or(|last| {
        now.duration_since(last)
            .is_ok_and(|since| since >= TOUCH_INTERVAL)
    })
}

/// Set `last_used_at` and `updated_at` of the service account `id` of the scope's organization
/// to `now`. A write that would move `last_used_at` back changes nothing.
async fn touch(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE service_accounts
           SET last_used_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
               updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2
             AND (last_used_at IS NULL
                  OR last_used_at < timestamptz 'epoch' + $3::bigint * interval '1 microsecond')"#,
        scope.org_id(),
        id,
        epoch_micros(now)?,
    )
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_763_000_000 + secs)
    }

    #[test]
    fn scopes_must_be_catalog_keys_listed_once() {
        assert!(check_scopes::<&str>(&[]).is_ok());
        assert!(check_scopes(&["account.view", "user.view"]).is_ok());
        assert!(matches!(
            check_scopes(&["account.view", "reports:read"]),
            Err(Error::Scope {
                index: 1,
                problem: ScopeProblem::Unknown
            })
        ));
        assert!(matches!(
            check_scopes(&["user.view", "account.view", "user.view"]),
            Err(Error::Scope {
                index: 2,
                problem: ScopeProblem::Duplicate
            })
        ));
    }

    #[test]
    fn last_used_is_written_at_most_once_a_minute() {
        assert!(needs_touch(at(0), None));
        assert!(!needs_touch(at(59), Some(at(0))));
        assert!(needs_touch(at(60), Some(at(0))));
        assert!(!needs_touch(at(0), Some(at(60))), "never back in time");
    }

    #[test]
    fn the_kind_round_trips_through_its_stored_spelling() {
        assert_eq!(Kind::parse(Kind::ApiKey.as_str()), Some(Kind::ApiKey));
        assert_eq!(Kind::parse("connector"), None);
    }
}
