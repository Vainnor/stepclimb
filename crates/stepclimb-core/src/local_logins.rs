// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local accounts' passwords and the email they sign in with (docs/specs/m0/04-local-accounts.md
//! B4, B9, B14, B16, B17, Data, Q3, Q5).
//!
//! A `local_logins` row holds a user's argon2id hash (a PHC string `stepclimb_auth::password`
//! computed; this module never sees a password) and `email_normalized`, the user's email as
//! `lower()` folds it, which the migration keeps equal to `lower(users.email)`. Every query
//! filters by `org_id = scope.org_id()`.
//!
//! [`LoginEmail`] is the one normalized form of an address password sign-in matches accounts
//! by, and the account identifier the per-account rate limit counts failures against
//! (docs/specs/m0/08-sessions-and-csrf.md B41). Only [`find_for_sign_in`] and [`get`] build one,
//! both with the database's `lower()`, the function the stored column and the `users_email`
//! index use, so `Ada@example.org` and `ada@example.org` are one account and one budget however
//! the database's locale folds letters outside ASCII.
//!
//! [`set_password`] stores a new hash (an enrollment link's password, B9, or a change, B17) and
//! writes the `password.set` audit entry with the user, never the hash. [`replace_hash`] swaps a
//! hash for one at newer parameters after a successful sign-in (B16), without an entry: the
//! password is the same.

use std::time::SystemTime;

use serde_json::json;
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
};

/// An email address as password sign-in matches accounts by it: trimmed, then lowercased by the
/// database's `lower()`. Its `Debug` output leaves the address out.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginEmail(String);

impl LoginEmail {
    /// The normalized address, for the rate limiter's account key.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `identifier` taken as already normalized, for a test that exercises the rate limiter
    /// without a database. Only a `test-support` build has it.
    #[cfg(feature = "test-support")]
    pub fn for_test(identifier: &str) -> Self {
        Self(identifier.to_owned())
    }
}

impl std::fmt::Debug for LoginEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoginEmail(..)")
    }
}

/// A user's local login as sign-in reads it. Its `Debug` output leaves the hash out.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub user_id: Uuid,
    /// The argon2id PHC string; `None` for a login without a password.
    pub password_hash: Option<String>,
    /// The address the login signs in with; `None` while the user has no email.
    pub email: Option<LoginEmail>,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("user_id", &self.user_id)
            .field("has_password", &self.password_hash.is_some())
            .finish_non_exhaustive()
    }
}

/// Why a local login call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// `email` as [`LoginEmail`] normalizes it, and the scope's login it matches, if any (B4, B14).
///
/// One query whatever the address: it returns the normalized form whether or not an account
/// matches. An address holding NUL, which no stored email can hold, is normalized by trimming
/// only, without a query, and matches nothing.
pub async fn find_for_sign_in(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    email: &str,
) -> Result<(LoginEmail, Option<Login>), Error> {
    let email = email.trim();
    if email.contains('\0') {
        return Ok((LoginEmail(email.to_owned()), None));
    }
    let row = sqlx::query!(
        r#"SELECT lower($2::text) AS "normalized!", l.user_id AS "user_id?",
                  l.password_hash AS "password_hash?"
           FROM (VALUES (1)) AS one (n)
           LEFT JOIN local_logins l ON l.org_id = $1 AND l.email_normalized = lower($2::text)"#,
        scope.org_id(),
        email,
    )
    .fetch_one(db)
    .await?;
    let normalized = LoginEmail(row.normalized);
    let login = row.user_id.map(|user_id| Login {
        user_id,
        password_hash: row.password_hash,
        email: Some(normalized.clone()),
    });
    Ok((normalized, login))
}

/// The login of the user `user_id` in the scope's organization, or `None` when they have none.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Option<Login>, Error> {
    let row = sqlx::query!(
        "SELECT user_id, password_hash, email_normalized
         FROM local_logins WHERE org_id = $1 AND user_id = $2",
        scope.org_id(),
        user_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| Login {
        user_id: row.user_id,
        password_hash: row.password_hash,
        email: row.email_normalized.map(LoginEmail),
    }))
}

/// Whether the user `user_id` of the scope's organization has a password.
pub async fn has_password(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<bool, Error> {
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM local_logins
               WHERE org_id = $1 AND user_id = $2 AND password_hash IS NOT NULL
           ) AS "found!""#,
        scope.org_id(),
        user_id,
    )
    .fetch_one(db)
    .await?;
    Ok(found)
}

/// Store `password_hash` (an argon2id PHC string) as the password of the user `user_id` in the
/// scope's organization at `now`, creating the login with the user's email if they have none,
/// and write the `password.set` audit entry by `actor` with the user. Runs in one transaction (a
/// savepoint when `db` is already one). A user the organization does not have is
/// [`Error::UserNotFound`].
pub async fn set_password<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    password_hash: &str,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let stored = sqlx::query_scalar!(
        r#"INSERT INTO local_logins (user_id, org_id, email_normalized, password_hash,
                                     password_changed_at, created_at, updated_at)
           SELECT u.id, u.org_id, lower(u.email), $3,
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.org_id = $1 AND u.id = $2
           ON CONFLICT (user_id) DO UPDATE
           SET password_hash = EXCLUDED.password_hash,
               password_changed_at = EXCLUDED.password_changed_at,
               updated_at = EXCLUDED.updated_at
           RETURNING user_id"#,
        scope.org_id(),
        user_id,
        password_hash,
        now_us,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if stored.is_none() {
        return Err(Error::UserNotFound);
    }
    let data = json!({ "after": { "user_id": user_id } });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::PasswordSet,
        user_id,
        &data,
        now_us,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Replace the password hash of the user `user_id` in the scope's organization with
/// `new_hash`, computed from the same password at newer parameters (B16), unless it is no
/// longer `old_hash` (a change committed since the sign-in read it). Returns whether it was
/// replaced. Writes no audit entry and keeps `password_changed_at`.
pub async fn replace_hash(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    old_hash: &str,
    new_hash: &str,
    now: SystemTime,
) -> Result<bool, Error> {
    let result = sqlx::query!(
        "UPDATE local_logins
         SET password_hash = $4,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2 AND password_hash = $3",
        scope.org_id(),
        user_id,
        old_hash,
        new_hash,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}
