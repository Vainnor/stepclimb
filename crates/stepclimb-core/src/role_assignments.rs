// SPDX-License-Identifier: AGPL-3.0-or-later
//! The role assignments repository (docs/specs/m0/09-permissions-and-roles.md B11, B15, B16).
//!
//! An assignment grants one role to one user at one scope: the organization, an account (and
//! its subtree) or, from M1, a course. Every query filters by `org_id = scope.org_id()`. The
//! table keeps the user in the row's organization (a composite foreign key), the role built-in
//! or the organization's own (the `role_assignments_role_in_org` trigger), and an organization
//! scope pointing at the row's organization (a check). `scope_id` has no foreign key, so
//! [`create`] checks that an account scope exists, and `accounts::delete` refuses an account
//! that assignments are still scoped to (B15), which it learns through [`any_at_account`].
//!
//! Account deletion and [`create`] at an account cannot cross: [`create`] reads the account
//! `FOR KEY SHARE`, which waits for a delete of that account in progress and then finds it gone,
//! and `accounts::delete` locks the account row `FOR UPDATE` before looking for assignments, so
//! it waits for a create in progress and then sees the new assignment.
//!
//! [`delete`] keeps the organization's last organization-scope Organization admin held by an
//! active user (B14). Removals in one organization take turns: each locks the organization's
//! row before it reads anything, so two removals of the last two admins cannot both count two.
//! The lock makes the count current only at `READ COMMITTED`, PostgreSQL's default, where each
//! statement after the lock sees what the previous holder committed; at `REPEATABLE READ` or
//! `SERIALIZABLE` the snapshot predates the lock wait, so [`delete`] refuses any other level with
//! [`Error::IsolationLevel`] before it reads.
//!
//! Who may assign which role where (B12 steps 4 and 5, B13, B14's permission half) is the
//! routes' to decide, as is refusing changes to an assignment an outside source owns; the sync
//! API (M4) writes those through these same functions.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, Source, accounts,
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
pub struct ListFilter<'a> {
    pub user_id: Option<Uuid>,
    pub scope: Option<Scope>,
    /// Only assignments at an account inside one of these accounts' subtrees (each account
    /// included), for a caller whose permission is held at these accounts only. `Some(&[])`
    /// matches nothing.
    pub within: Option<&'a [Uuid]>,
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
    /// The assignment is the organization's last organization-scope Organization admin held by
    /// an active user (B14).
    #[error("the last organization admin assignment of an active user cannot be removed")]
    LastOrgAdmin,
    /// The transaction runs at an isolation level other than `READ COMMITTED`, where the
    /// last-admin count could miss a concurrent removal.
    #[error("role assignments can be removed only at READ COMMITTED isolation, not {found}")]
    IsolationLevel { found: String },
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// A stored value this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    UnknownStoredValue { column: &'static str, value: String },
    /// An account-scoped assignment whose account has no row in the scope's organization: B15
    /// and [`create`] keep this from happening, and the read fails closed if it does.
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
    filter: ListFilter<'_>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<RoleAssignment>, Error> {
    let (scope_type, scope_id) = filter.scope.map(|s| s.as_parts(scope.org_id())).unzip();
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT ra.id, ra.user_id, ra.role_id, ra.scope_type, ra.scope_id, ra.source,
                  ra.external_id,
                  (extract(epoch FROM ra.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM ra.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM role_assignments ra
           WHERE ra.org_id = $1
             AND ($2::uuid IS NULL OR ra.user_id = $2)
             AND ($3::text IS NULL OR (ra.scope_type = $3 AND ra.scope_id = $4))
             AND ($5::uuid IS NULL OR ra.id > $5)
             AND ($7::uuid[] IS NULL OR (ra.scope_type = 'account' AND EXISTS (
                     SELECT 1
                     FROM accounts a
                     JOIN accounts w ON w.org_id = a.org_id AND a.path <@ w.path
                     WHERE a.org_id = $1 AND a.id = ra.scope_id AND w.id = ANY($7))))
           ORDER BY ra.id
           LIMIT $6"#,
        scope.org_id(),
        filter.user_id,
        scope_type,
        scope_id,
        after,
        limit,
        filter.within,
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

/// Delete the assignment `id` of the scope's organization, unless it is the organization's last
/// organization-scope Organization admin assignment held by an active user
/// ([`Error::LastOrgAdmin`], B14).
///
/// Takes the organization's removal lock first ([`lock_removals`]), so concurrent removals in
/// one organization take turns and each counts what the previous one left. Runs in one
/// transaction (a savepoint when `db` is already one); the lock is held until the outermost
/// transaction ends.
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    lock_removals(&mut tx, scope).await?;
    let counted = sqlx::query_scalar!(
        r#"SELECT (r.org_id IS NULL AND r.key = 'org_admin'
                   AND ra.scope_type = 'organization' AND u.status = $3) AS "counted!"
           FROM role_assignments ra
           JOIN roles r ON r.id = ra.role_id
           JOIN users u ON u.org_id = ra.org_id AND u.id = ra.user_id
           WHERE ra.org_id = $1 AND ra.id = $2"#,
        scope.org_id(),
        id,
        Status::Active.as_str(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if counted && count_active_org_admins(&mut *tx, scope).await? <= 1 {
        return Err(Error::LastOrgAdmin);
    }
    sqlx::query!(
        "DELETE FROM role_assignments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Lock the scope's organization row (`FOR NO KEY UPDATE`, which foreign key checks on
/// `organizations` do not wait for) until the transaction ends, so removals in the organization
/// take turns, and refuse a transaction whose isolation level is not `READ COMMITTED`.
async fn lock_removals(conn: &mut PgConnection, scope: OrgScope) -> Result<(), Error> {
    let isolation = sqlx::query_scalar!(
        r#"SELECT current_setting('transaction_isolation') AS "isolation!"
           FROM organizations WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::NotFound)?;
    if isolation != "read committed" {
        return Err(Error::IsolationLevel { found: isolation });
    }
    Ok(())
}

/// Whether any assignment is scoped to the account `account_id` of the scope's organization,
/// for `accounts::delete` in its transaction (B15, spec 02 B21).
pub(crate) async fn any_at_account(
    conn: &mut PgConnection,
    scope: OrgScope,
    account_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM role_assignments
               WHERE org_id = $1 AND scope_type = 'account' AND scope_id = $2) AS "any!""#,
        scope.org_id(),
        account_id,
    )
    .fetch_one(conn)
    .await
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
                    path: row.path.as_deref().and_then(accounts::parse_path).ok_or(
                        Error::MissingAccount {
                            assignment_id: row.id,
                        },
                    )?,
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
}
