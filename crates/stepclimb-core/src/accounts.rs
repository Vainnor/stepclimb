// SPDX-License-Identifier: AGPL-3.0-or-later
//! The accounts repository (docs/specs/m0/02-organizations-and-accounts.md B16 to B22).
//!
//! Inside an organization, accounts form a tree with exactly one root, made with the
//! organization (`organizations::create`). `accounts.path` is an `ltree` of the ids of the root,
//! every ancestor and the account itself, each as 32 lowercase hex digits (B17), so "A is in B's
//! subtree" is `a.path <@ b.path` and a rename touches only `name`. Every query filters by
//! `org_id = scope.org_id()`.
//!
//! That a path equals its parent's path plus the account's id is kept here, not by a constraint
//! (SQL cannot express it across rows): [`create`], [`move_to`] and [`delete`] take a
//! transaction-scoped advisory lock on the organization's tree before reading it (B22), so two
//! concurrent moves cannot build a cycle and a create cannot attach to an account being
//! deleted. [`rename`] does not take it. The lock is only correct at PostgreSQL's default
//! `READ COMMITTED` isolation, where each statement after the lock sees what the previous holder
//! committed: a caller passing in its own transaction must not raise the isolation level.
//!
//! Who may do what to which account (B23) is the routes' to decide; this module checks only the
//! rules of the tree itself.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name,
};

/// The longest account name, in characters (Unicode scalar values; spec 02 Q6).
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// The deepest an account may sit, counted as [`Account::depth`] counts (0 for the root), so a
/// tree has at most 17 levels (spec 02 Q6).
pub const MAX_DEPTH: i32 = 16;

/// The first key of the advisory lock on an organization's account tree. PostgreSQL keeps
/// two-`int4` advisory keys apart from single-`bigint` ones (such as sqlx's migration lock);
/// this constant keeps the tree lock apart from any other two-key lock. The ASCII of `acct`.
const TREE_LOCK_CLASS: i32 = 0x6163_6374;

/// An account as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: Uuid,
    /// `None` for the root only.
    pub parent_id: Option<Uuid>,
    pub name: String,
    /// 0 for the root, 1 for its children, and so on.
    pub depth: i32,
    pub has_children: bool,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// The sort key of an account in [`list_children`]: `(name, id)`.
pub type ListKey = (String, Uuid);

/// Why an accounts repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No account with that id in the scope's organization.
    #[error("the account does not exist")]
    NotFound,
    /// No account with the given parent id in the scope's organization.
    #[error("the parent account does not exist")]
    ParentNotFound,
    /// The name is empty once trimmed.
    #[error("an account name cannot be empty")]
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`] once trimmed.
    #[error("an account name is longer than {MAX_NAME_CHARS} characters")]
    NameTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// The root account cannot be moved or deleted (B20, B21).
    #[error("the root account cannot be moved or deleted")]
    RootImmutable,
    /// The new parent is the account itself or one of its descendants (B20).
    #[error("an account cannot move into its own subtree")]
    MoveIntoDescendant,
    /// The account still has child accounts (B21).
    #[error("an account with child accounts cannot be deleted")]
    HasChildren,
    /// The create or move would put an account deeper than [`MAX_DEPTH`].
    #[error("accounts cannot be nested deeper than {MAX_DEPTH} levels below the root")]
    TooDeep,
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// An `accounts` row as the queries select it.
struct Row {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    depth: i32,
    has_children: bool,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Account {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.id,
            parent_id: row.parent_id,
            name: row.name,
            depth: row.depth,
            has_children: row.has_children,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The account `id` of the scope's organization, or `None` when the organization has no such
/// account.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Account>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT a.id, a.parent_id, a.name, nlevel(a.path) - 1 AS "depth!",
                  EXISTS (SELECT 1 FROM accounts c WHERE c.org_id = $1 AND c.parent_id = a.id)
                      AS "has_children!",
                  (extract(epoch FROM a.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM a.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM accounts a
           WHERE a.org_id = $1 AND a.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Account::try_from)
    .transpose()
}

/// The children of the account `parent_id` in the scope's organization, sorted by name then id,
/// starting after `after` (the sort key of the previous page's last account) and at most `limit`
/// of them. Empty when the organization has no such account.
///
/// `after` comes from a client's cursor: it only bounds a query already filtered by the scope.
/// A NUL in its name is refused with [`Error::ContainsNul`].
pub async fn list_children(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    parent_id: Uuid,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Account>, Error> {
    let after_name = after.map(|(name, _)| no_nul("cursor", name)).transpose()?;
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT a.id, a.parent_id, a.name, nlevel(a.path) - 1 AS "depth!",
                  EXISTS (SELECT 1 FROM accounts c WHERE c.org_id = $1 AND c.parent_id = a.id)
                      AS "has_children!",
                  (extract(epoch FROM a.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM a.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM accounts a
           WHERE a.org_id = $1 AND a.parent_id = $2
             AND ($3::text IS NULL OR (a.name, a.id) > ($3, $4::uuid))
           ORDER BY a.name, a.id
           LIMIT $5"#,
        scope.org_id(),
        parent_id,
        after_name,
        after.map(|(_, id)| *id),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Account::try_from).collect()
}

/// Create an account named `name`, trimmed, under the account `parent_id` of the scope's
/// organization, with `created_at` and `updated_at` set to `now` (B18). The repository computes
/// the path from the parent's.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    parent_id: Uuid,
    name: &str,
    now: SystemTime,
) -> Result<Account, Error> {
    let name = valid_name(name)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    lock_tree(&mut tx, scope).await?;
    let parent = sqlx::query!(
        r#"SELECT path::text AS "path!", nlevel(path) - 1 AS "depth!"
           FROM accounts WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        parent_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::ParentNotFound)?;
    if parent.depth + 1 > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    let id = new_id();
    let path = format!("{}.{}", parent.path, label(id));
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO accounts (id, org_id, parent_id, name, path, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5::text::ltree,
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $6::bigint * interval '1 microsecond')
           RETURNING id, parent_id, name, nlevel(path) - 1 AS "depth!",
                     false AS "has_children!",
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        id,
        scope.org_id(),
        parent_id,
        name,
        path,
        now,
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Account::try_from(row)
}

/// Insert the root account of the organization `organizations::create` is inserting in the same
/// transaction, named `name` (already validated as an organization name, the same rule). Takes
/// no tree lock: the organization is not committed, so no other transaction can see its tree.
pub(crate) async fn create_root(
    conn: &mut PgConnection,
    scope: OrgScope,
    name: &str,
    now_micros: i64,
) -> Result<(), sqlx::Error> {
    let id = new_id();
    sqlx::query!(
        "INSERT INTO accounts (id, org_id, name, path, created_at, updated_at)
         VALUES ($1, $2, $3, $4::text::ltree,
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        name,
        label(id),
        now_micros,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Rename the account `id` of the scope's organization to `name`, trimmed, and set `updated_at`
/// to `now` (B19). Changes nothing else, so it takes no tree lock.
pub async fn rename(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    name: &str,
    now: SystemTime,
) -> Result<Account, Error> {
    let name = valid_name(name)?;
    let now = epoch_micros(now)?;
    sqlx::query_as!(
        Row,
        r#"UPDATE accounts a
           SET name = $3, updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           WHERE a.org_id = $1 AND a.id = $2
           RETURNING a.id, a.parent_id, a.name, nlevel(a.path) - 1 AS "depth!",
                     EXISTS (SELECT 1 FROM accounts c WHERE c.org_id = $1 AND c.parent_id = a.id)
                         AS "has_children!",
                     (extract(epoch FROM a.created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM a.updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        name,
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?
    .try_into()
}

/// Move the account `id` of the scope's organization under the account `parent_id` (B20).
///
/// Refuses the root ([`Error::RootImmutable`]), a new parent that is the account or one of its
/// descendants ([`Error::MoveIntoDescendant`]), and a move that would put any account of the
/// subtree deeper than [`MAX_DEPTH`]. One `UPDATE` rewrites the path of the account and every
/// descendant and sets their `updated_at` to `now`, since each one's depth may change; only the
/// account's own `parent_id` changes.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock.
pub async fn move_to<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    parent_id: Uuid,
    now: SystemTime,
) -> Result<Account, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    lock_tree(&mut tx, scope).await?;
    let account = sqlx::query!(
        r#"SELECT a.parent_id, a.path::text AS "path!", nlevel(a.path) - 1 AS "depth!",
                  (SELECT max(nlevel(d.path)) - 1 FROM accounts d
                   WHERE d.org_id = $1 AND d.path <@ a.path) AS "deepest!"
           FROM accounts a WHERE a.org_id = $1 AND a.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if account.parent_id.is_none() {
        return Err(Error::RootImmutable);
    }
    let parent = sqlx::query!(
        r#"SELECT path::text AS "path!", nlevel(path) - 1 AS "depth!",
                  path <@ $3::text::ltree AS "inside!"
           FROM accounts WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        parent_id,
        account.path,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::ParentNotFound)?;
    if parent.inside {
        return Err(Error::MoveIntoDescendant);
    }
    // The subtree's deepest account keeps its distance below the moved account, which lands at
    // the new parent's depth plus one.
    if parent.depth + 1 + (account.deepest - account.depth) > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    sqlx::query!(
        "UPDATE accounts
         SET path = $4::text::ltree || subpath(path, $5),
             parent_id = CASE WHEN id = $2 THEN $3 ELSE parent_id END,
             updated_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND path <@ $7::text::ltree",
        scope.org_id(),
        id,
        parent_id,
        parent.path,
        account.depth,
        now,
        account.path,
    )
    .execute(&mut *tx)
    .await?;
    let moved = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(moved)
}

/// Delete the account `id` of the scope's organization (B21). Refuses the root
/// ([`Error::RootImmutable`]) and an account with child accounts ([`Error::HasChildren`]).
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock. It
/// deletes the account row only: no other table refers to an account yet.
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    lock_tree(&mut tx, scope).await?;
    let account = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    if account.parent_id.is_none() {
        return Err(Error::RootImmutable);
    }
    if account.has_children {
        return Err(Error::HasChildren);
    }
    sqlx::query!(
        "DELETE FROM accounts WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Take the transaction-scoped advisory lock on the scope's account tree (B22). It is released
/// when the outermost transaction ends.
async fn lock_tree(conn: &mut PgConnection, scope: OrgScope) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"SELECT true AS "locked!" FROM pg_advisory_xact_lock($1, $2)"#,
        TREE_LOCK_CLASS,
        tree_lock_key(scope.org_id()),
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

/// The second key of an organization's tree lock: the organization id's four 32-bit words
/// XORed together. Two organizations sharing a key only wait on each other; they never see each
/// other's rows.
fn tree_lock_key(org_id: Uuid) -> i32 {
    let (words, _) = org_id.as_bytes().as_chunks::<4>();
    words
        .iter()
        .map(|word| i32::from_be_bytes(*word))
        .fold(0, |key, word| key ^ word)
}

/// The `ltree` label of an account: its id as 32 lowercase hex digits, without hyphens (B17).
fn label(id: Uuid) -> String {
    id.simple().to_string()
}

fn valid_name(name: &str) -> Result<&str, Error> {
    name::valid(name).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::NameEmpty,
        name::Invalid::TooLong => Error::NameTooLong,
        name::Invalid::ContainsNul => Error::ContainsNul { field: "name" },
    })
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
    fn labels_are_32_lowercase_hex_digits() {
        let id: Uuid = "0192F3A4-5B6C-7D8E-9FA0-B1C2D3E4F5A6".parse().unwrap();
        assert_eq!(label(id), "0192f3a45b6c7d8e9fa0b1c2d3e4f5a6");
        let fresh = label(new_id());
        assert_eq!(fresh.len(), 32);
        assert!(
            fresh
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn the_lock_key_folds_every_word_of_the_id() {
        let id: Uuid = "00000001-0000-0002-0000-000400000008".parse().unwrap();
        assert_eq!(tree_lock_key(id), 1 ^ 2 ^ 4 ^ 8);
        assert_eq!(tree_lock_key(Uuid::nil()), 0);
    }

    #[test]
    fn name_problems_map_to_account_errors() {
        assert_eq!(valid_name(" Tower ").unwrap(), "Tower");
        assert!(matches!(valid_name(" "), Err(Error::NameEmpty)));
        assert!(matches!(
            valid_name("T\0"),
            Err(Error::ContainsNul { field: "name" })
        ));
        let over = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(valid_name(&over), Err(Error::NameTooLong)));
    }
}
