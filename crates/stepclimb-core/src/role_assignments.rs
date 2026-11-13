// SPDX-License-Identifier: AGPL-3.0-or-later
//! The role assignments repository (docs/specs/m0/09-permissions-and-roles.md B11, B15, B16).
//!
//! An assignment grants one role to one user at one scope: the organization, an account (and
//! its subtree) or, from M1, a course. Every query filters by `org_id = scope.org_id()`. The
//! table keeps the user in the row's organization (a composite foreign key), the role built-in
//! or the organization's own (the `role_assignments_role_in_org` trigger), and an organization
//! scope pointing at the row's organization (a check). `scope_id` has no foreign key, so
//! [`create`] checks that an account scope exists, and `accounts::delete` deletes the
//! assignments of the account it deletes (B15) through [`delete_for_account`].
//!
//! Account deletion and [`create`] at an account cannot cross: [`create`] reads the account
//! `FOR KEY SHARE`, which waits for a delete of that account in progress and then finds it gone,
//! and `accounts::delete` deletes the account row before its assignments, so it waits for a
//! create in progress and then deletes the new assignment too.
//!
//! Who may assign which role where (B12 steps 4 and 5, B13, B14) is the routes' to decide, as is
//! refusing changes to an assignment an outside source owns; the sync API (M4) writes those
//! through these same functions.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, Source,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    roles,
    users::Status,
};

/// The composite foreign key that keeps the user in the row's organization.
const USER_FKEY: &str = "role_assignments_org_id_user_id_fkey";
/// The foreign key to `roles`.
const ROLE_FKEY: &str = "role_assignments_role_id_fkey";
/// The constraint trigger that refuses another organization's custom role.
const ROLE_IN_ORG_TRIGGER: &str = "role_assignments_role_in_org";
/// The unique constraint on `(user_id, role_id, scope_type, scope_id)`.
const UNIQUE_CONSTRAINT: &str = "role_assignments_user_id_role_id_scope_type_scope_id_key";

/// Where an assignment takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The scope's organization (stored with `scope_id = org_id`).
    Organization,
    /// One account and its whole subtree.
    Account(Uuid),
    /// One course. [`create`] refuses it until the M1 courses spec enables it.
    Course(Uuid),
}

impl Scope {
    fn as_parts(self, org_id: Uuid) -> (&'static str, Uuid) {
        match self {
            Self::Organization => ("organization", org_id),
            Self::Account(id) => ("account", id),
            Self::Course(id) => ("course", id),
        }
    }

    fn from_parts(scope_type: &str, scope_id: Uuid) -> Result<Self, Error> {
        match scope_type {
            "organization" => Ok(Self::Organization),
            "account" => Ok(Self::Account(scope_id)),
            "course" => Ok(Self::Course(scope_id)),
            other => Err(Error::UnknownStoredValue {
                column: "scope_type",
                value: other.to_owned(),
            }),
        }
    }
}

/// An assignment as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleAssignment {
    pub id: Uuid,
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub scope: Scope,
    pub source: Source,
    pub external_id: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// An assignment to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAssignment {
    pub user_id: Uuid,
    /// A built-in role or a custom role of the scope's organization.
    pub role_id: Uuid,
    pub scope: Scope,
    /// `Local` for an admin's assignment; the M4 permission modes write the others (B11).
    pub source: Source,
    pub external_id: Option<String>,
}

/// The filters of [`list`]; `None` filters nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub user_id: Option<Uuid>,
    pub scope: Option<Scope>,
}

/// Where an assignment of [`for_user_with_paths`] takes effect, with what the evaluator needs to
/// decide what it contains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantScope {
    Organization,
    /// The account and the ids on its path: the root first, the account itself last.
    Account {
        id: Uuid,
        path: Vec<Uuid>,
    },
    Course(Uuid),
}

/// One assignment of a user with its role's permissions, as the evaluator reads it (B16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserGrant {
    pub assignment_id: Uuid,
    pub role_id: Uuid,
    pub role_key: String,
    /// As [`roles::Role::permissions`]: a built-in role's list from code (B6), a custom role's
    /// as stored, including keys a later release removed from the catalog (B9).
    pub permissions: Vec<String>,
    pub scope: GrantScope,
}

/// Why a role assignments repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No assignment with that id in the scope's organization.
    #[error("the role assignment does not exist")]
    NotFound,
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// The role is neither built-in nor a custom role of the scope's organization.
    #[error("the role does not exist")]
    RoleNotFound,
    /// No account with the scope's id in the scope's organization.
    #[error("the account does not exist")]
    AccountNotFound,
    /// Assignments at this scope type are not available yet (course scope until M1).
    #[error("role assignments at this scope are not available")]
    ScopeUnavailable,
    /// The user already holds the role at that scope.
    #[error("the user already holds this role at this scope")]
    Exists,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// A stored value this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    UnknownStoredValue { column: &'static str, value: String },
    /// An account-scoped assignment whose account has no row: B15 keeps this from happening.
    #[error("role assignment {assignment_id} points at a missing account")]
    MissingAccount { assignment_id: Uuid },
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(USER_FKEY) => Self::UserNotFound,
            Some(ROLE_FKEY | ROLE_IN_ORG_TRIGGER) => Self::RoleNotFound,
            Some(UNIQUE_CONSTRAINT) => Self::Exists,
            _ => Self::Database(error),
        }
    }
}

/// A `role_assignments` row as the queries select it.
struct Row {
    id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    scope_type: String,
    scope_id: Uuid,
    source: String,
    external_id: Option<String>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for RoleAssignment {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let source = Source::parse(&row.source).ok_or(Error::UnknownStoredValue {
            column: "source",
            value: row.source,
        })?;
        Ok(Self {
            id: row.id,
            user_id: row.user_id,
            role_id: row.role_id,
            scope: Scope::from_parts(&row.scope_type, row.scope_id)?,
            source,
            external_id: row.external_id,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The assignment `id` of the scope's organization, or `None` when it has no such assignment.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<RoleAssignment>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, user_id, role_id, scope_type, scope_id, source, external_id,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM role_assignments
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(RoleAssignment::try_from)
    .transpose()
}

/// The scope's assignments matching `filter`, sorted by id (creation order), starting after the
/// id `after` and at most `limit` of them.
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    filter: ListFilter,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<RoleAssignment>, Error> {
    let (scope_type, scope_id) = filter.scope.map(|s| s.as_parts(scope.org_id())).unzip();
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, user_id, role_id, scope_type, scope_id, source, external_id,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM role_assignments
           WHERE org_id = $1
             AND ($2::uuid IS NULL OR user_id = $2)
             AND ($3::text IS NULL OR (scope_type = $3 AND scope_id = $4))
             AND ($5::uuid IS NULL OR id > $5)
           ORDER BY id
           LIMIT $6"#,
        scope.org_id(),
        filter.user_id,
        scope_type,
        scope_id,
        after,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(RoleAssignment::try_from).collect()
}

/// Create an assignment in the scope's organization, with `created_at` and `updated_at` set to
/// `now` (B11).
///
/// Refuses a course scope ([`Error::ScopeUnavailable`]) and an account scope naming no account
/// of the organization ([`Error::AccountNotFound`]); the table refuses a user or role outside
/// the organization and a repeat of the same user, role and scope. Runs in one transaction (a
/// savepoint when `db` is already one).
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    assignment: &NewAssignment,
    now: SystemTime,
) -> Result<RoleAssignment, Error> {
    let external_id = assignment
        .external_id
        .as_deref()
        .map(|id| no_nul("external_id", id))
        .transpose()?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    match assignment.scope {
        Scope::Organization => {}
        Scope::Account(id) => lock_account(&mut tx, scope, id).await?,
        Scope::Course(_) => return Err(Error::ScopeUnavailable),
    }
    let (scope_type, scope_id) = assignment.scope.as_parts(scope.org_id());
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id,
                                         source, external_id, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                   timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $9::bigint * interval '1 microsecond')
           RETURNING id, user_id, role_id, scope_type, scope_id, source, external_id,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        new_id(),
        scope.org_id(),
        assignment.user_id,
        assignment.role_id,
        scope_type,
        scope_id,
        assignment.source.as_str(),
        external_id,
        now,
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    RoleAssignment::try_from(row)
}

/// Read the account `id` of the scope's organization `FOR KEY SHARE`, so a concurrent
/// `accounts::delete` of it either finished first (the account is gone) or waits for this
/// transaction to end.
async fn lock_account(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    sqlx::query_scalar!(
        "SELECT id FROM accounts WHERE org_id = $1 AND id = $2 FOR KEY SHARE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::AccountNotFound)?;
    Ok(())
}

/// Delete the assignment `id` of the scope's organization.
pub async fn delete(db: impl PgExecutor<'_>, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    let deleted = sqlx::query!(
        "DELETE FROM role_assignments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// Delete every assignment scoped to the account `account_id` of the scope's organization, for
/// `accounts::delete` in its transaction (B15, spec 02 B21).
pub(crate) async fn delete_for_account(
    conn: &mut PgConnection,
    scope: OrgScope,
    account_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM role_assignments
         WHERE org_id = $1 AND scope_type = 'account' AND scope_id = $2",
        scope.org_id(),
        account_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Every assignment of the user `user_id` in the scope's organization with its role's key and
/// permissions and, for an account scope, the account's path, in one query (B16). Empty for a
/// user the organization does not have.
pub async fn for_user_with_paths(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Vec<UserGrant>, Error> {
    // The role filter repeats the trigger's rule, so a role row of another organization could
    // not grant anything here even if one were stored.
    let rows = sqlx::query!(
        r#"SELECT ra.id, ra.role_id, r.key, r.builtin, r.permissions, ra.scope_type, ra.scope_id,
                  a.path::text AS path
           FROM role_assignments ra
           JOIN roles r ON r.id = ra.role_id AND (r.org_id IS NULL OR r.org_id = $1)
           LEFT JOIN accounts a
                  ON ra.scope_type = 'account' AND a.org_id = $1 AND a.id = ra.scope_id
           WHERE ra.org_id = $1 AND ra.user_id = $2
           ORDER BY ra.id"#,
        scope.org_id(),
        user_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            let scope = match Scope::from_parts(&row.scope_type, row.scope_id)? {
                Scope::Organization => GrantScope::Organization,
                Scope::Course(id) => GrantScope::Course(id),
                Scope::Account(id) => GrantScope::Account {
                    id,
                    path: parse_path(row.path.as_deref()).ok_or(Error::MissingAccount {
                        assignment_id: row.id,
                    })?,
                },
            };
            Ok(UserGrant {
                assignment_id: row.id,
                role_id: row.role_id,
                permissions: roles::effective_permissions(&row.key, row.builtin, row.permissions),
                role_key: row.key,
                scope,
            })
        })
        .collect()
}

/// The account ids of an `ltree` path of 32-hex-digit labels, or `None` when there is no path
/// or a label is not an id.
fn parse_path(path: Option<&str>) -> Option<Vec<Uuid>> {
    path?
        .split('.')
        .map(|label| Uuid::try_parse(label).ok())
        .collect()
}

/// How many organization-scope assignments of the built-in Organization admin role in the
/// scope's organization are held by `active` users (B14).
pub async fn count_active_org_admins(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<i64, Error> {
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!"
           FROM role_assignments ra
           JOIN roles r ON r.id = ra.role_id AND r.org_id IS NULL AND r.key = 'org_admin'
           JOIN users u ON u.org_id = ra.org_id AND u.id = ra.user_id
           WHERE ra.org_id = $1 AND ra.scope_type = 'organization' AND u.status = $2"#,
        scope.org_id(),
        Status::Active.as_str(),
    )
    .fetch_one(db)
    .await?;
    Ok(count)
}

/// `value` unchanged, or [`Error::ContainsNul`] naming `field`.
fn no_nul<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    if value.contains('\0') {
        return Err(Error::ContainsNul { field });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_round_trips_through_its_stored_parts() {
        let (org, id) = (new_id(), new_id());
        for scope in [Scope::Organization, Scope::Account(id), Scope::Course(id)] {
            let (scope_type, scope_id) = scope.as_parts(org);
            assert_eq!(Scope::from_parts(scope_type, scope_id).unwrap(), scope);
        }
        assert_eq!(Scope::Organization.as_parts(org), ("organization", org));
        assert!(matches!(
            Scope::from_parts("group", id),
            Err(Error::UnknownStoredValue {
                column: "scope_type",
                ..
            })
        ));
    }

    #[test]
    fn a_path_parses_to_its_ids_in_order() {
        let (root, tower) = (new_id(), new_id());
        let path = format!("{}.{}", root.simple(), tower.simple());
        assert_eq!(parse_path(Some(&path)), Some(vec![root, tower]));
        assert_eq!(parse_path(None), None);
        assert_eq!(parse_path(Some("not_an_id")), None);
    }
}
