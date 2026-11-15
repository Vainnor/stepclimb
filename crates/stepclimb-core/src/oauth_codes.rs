// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorization codes repository (docs/specs/m0/08-sessions-and-csrf.md B31, Data, Q10).
//!
//! The OAuth 2.1 authorization endpoint stores one row per code it issues, in the organization
//! of the signed-in user, which every query here filters by. A row binds the code to the user,
//! the client, the exact redirect URI and the PKCE S256 challenge, and lives [`LIFETIME`]. The
//! row holds only the SHA-256 of the code, so this module never sees a code. Expired rows are
//! deleted by [`delete_expired`], which the periodic sign-in cleanup runs.
//!
//! The token endpoint exchanges a code in one transaction (B32): [`lock`] reads the code with its
//! user's status and locks its row, so a second exchange of the same code waits for the first
//! and then reads it used; [`mark_used`] records the exchange and the refresh family it started,
//! once.

use std::time::{Duration, SystemTime};

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    users::Status,
};

/// How long a code can be exchanged after it is issued (Q10).
pub const LIFETIME: Duration = Duration::from_secs(60);

/// A code to store.
#[derive(Clone, Copy, Debug)]
pub struct NewCode<'a> {
    /// The row's key (`id::new_id`).
    pub id: Uuid,
    /// The signed-in user the code is for. Must be a user of the scope's organization.
    pub user_id: Uuid,
    pub client_id: &'a str,
    /// SHA-256 of the code the caller has just generated.
    pub code_hash: &'a [u8; 32],
    /// The redirect URI the request named, which the exchange must name again.
    pub redirect_uri: &'a str,
    /// The request's S256 challenge.
    pub code_challenge: &'a str,
}

/// Why a codes repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `user_id` names no user of the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// A stored user status this release does not know; the table's check keeps it from
    /// happening.
    #[error("the code's user has a status this release does not know")]
    UnknownStatus,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A stored code as an exchange reads it, its row locked until the transaction ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub expires_at: SystemTime,
    /// When the code was exchanged, if it was.
    pub used_at: Option<SystemTime>,
    /// The refresh family its exchange started, while that family exists.
    pub family_id: Option<Uuid>,
    /// The user's status now.
    pub user_status: Status,
}

struct LockedRow {
    id: Uuid,
    user_id: Uuid,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    expires_at_us: Option<i64>,
    used_at_us: Option<i64>,
    family_id: Option<Uuid>,
    status: String,
}

/// The code of the scope's organization whose hash is `code_hash`, with its row locked for the
/// caller's transaction (`FOR UPDATE`), or `None`. A concurrent exchange of the same code waits
/// here until the first one commits, then reads the code as it left it.
pub async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    code_hash: &[u8; 32],
) -> Result<Option<LockedCode>, Error> {
    let row = sqlx::query_as!(
        LockedRow,
        r#"SELECT c.id, c.user_id, c.client_id, c.redirect_uri, c.code_challenge,
                  (extract(epoch FROM c.expires_at) * 1000000)::bigint AS expires_at_us,
                  (extract(epoch FROM c.used_at) * 1000000)::bigint AS used_at_us,
                  c.family_id, u.status
           FROM oauth_authorization_codes c
           JOIN users u ON u.org_id = c.org_id AND u.id = c.user_id
           WHERE c.org_id = $1 AND c.code_hash = $2
           FOR UPDATE OF c"#,
        scope.org_id(),
        code_hash.as_slice(),
    )
    .fetch_optional(conn)
    .await?;
    row.map(LockedCode::try_from).transpose()
}

impl TryFrom<LockedRow> for LockedCode {
    type Error = Error;

    fn try_from(row: LockedRow) -> Result<Self, Error> {
        let expires_at = row.expires_at_us.ok_or(TimeOutOfRange)?;
        Ok(Self {
            id: row.id,
            user_id: row.user_id,
            client_id: row.client_id,
            redirect_uri: row.redirect_uri,
            code_challenge: row.code_challenge,
            expires_at: from_epoch_micros(expires_at)?,
            used_at: row.used_at_us.map(from_epoch_micros).transpose()?,
            family_id: row.family_id,
            user_status: Status::parse(&row.status).ok_or(Error::UnknownStatus)?,
        })
    }
}

/// Record that the code `id` of the scope's organization was exchanged at `now`, starting the
/// refresh family `family_id`. Returns `false`, changing nothing, when the code does not exist
/// there or was already used.
pub async fn mark_used(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    family_id: Uuid,
    now: SystemTime,
) -> Result<bool, Error> {
    let result = sqlx::query!(
        r#"UPDATE oauth_authorization_codes
           SET used_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
               updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
               family_id = $3
           WHERE org_id = $1 AND id = $2 AND used_at IS NULL"#,
        scope.org_id(),
        id,
        family_id,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Store a code in the scope's organization, issued at `now` and expiring [`LIFETIME`] after it.
/// Returns the expiry. The insert takes the user from a `SELECT` filtered by the scope, so a
/// user of another organization stores nothing and is [`Error::UserNotFound`] without aborting
/// the caller's transaction.
pub async fn create(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    code: &NewCode<'_>,
    now: SystemTime,
) -> Result<SystemTime, Error> {
    let expires_at = now.checked_add(LIFETIME).ok_or(TimeOutOfRange)?;
    let inserted = sqlx::query_scalar!(
        r#"INSERT INTO oauth_authorization_codes
               (id, org_id, user_id, client_id, code_hash, redirect_uri, code_challenge,
                expires_at, created_at, updated_at)
           SELECT $1, u.org_id, u.id, $4, $5, $6, $7,
                  timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.org_id = $2 AND u.id = $3
           RETURNING id"#,
        code.id,
        scope.org_id(),
        code.user_id,
        code.client_id,
        code.code_hash.as_slice(),
        code.redirect_uri,
        code.code_challenge,
        epoch_micros(now)?,
        epoch_micros(expires_at)?,
    )
    .fetch_optional(db)
    .await?;
    inserted.map(|_| expires_at).ok_or(Error::UserNotFound)
}

/// Delete every code of the scope's organization that has expired at `now`, for the periodic
/// cleanup. Returns how many were deleted.
pub async fn delete_expired(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<u64, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM oauth_authorization_codes
           WHERE org_id = $1
             AND expires_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'"#,
        scope.org_id(),
        epoch_micros(now)?,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_lives_sixty_seconds() {
        assert_eq!(LIFETIME.as_secs(), 60);
    }
}
