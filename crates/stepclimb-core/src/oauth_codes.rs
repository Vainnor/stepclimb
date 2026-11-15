// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorization codes repository (docs/specs/m0/08-sessions-and-csrf.md B31, Data, Q10).
//!
//! The OAuth 2.1 authorization endpoint stores one row per code it issues, in the organization
//! of the signed-in user, which every query here filters by. A row binds the code to the user,
//! the client, the exact redirect URI and the PKCE S256 challenge, and lives [`LIFETIME`]. The
//! row holds only the SHA-256 of the code, so this module never sees a code. Expired rows are
//! deleted by [`delete_expired`], which the periodic sign-in cleanup runs.

use std::time::{Duration, SystemTime};

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
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
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
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
