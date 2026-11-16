// SPDX-License-Identifier: AGPL-3.0-or-later
//! The refresh tokens repository and the app access token resolver
//! (docs/specs/m0/08-sessions-and-csrf.md B32 to B36, Data, Q11, Q12, Q14).
//!
//! A refresh token belongs to a family: a code exchange starts one ([`start_family`]), each
//! refresh adds the next token to it ([`rotate`]), and a family ends whole ([`revoke_family`]),
//! or every family of a user at once with the user's unused codes ([`revoke_all_of_user`]).
//! A token lives [`LIFETIME`] from its issue, and no token outlives [`FAMILY_LIFETIME`] from the
//! family's start (Q11 (b)). A token is spent once a row rotated from it exists; the rotated row
//! stays, so presenting the spent token again is recognized as reuse, which ends the family and
//! writes a `refresh_token.reuse` audit entry ([`revoke_reused`], Q14).
//!
//! Every query filters by the scope's organization. The repository never sees a token, only its
//! SHA-256 hash. An app access token names its family, user and client; [`authenticate`] checks,
//! per request, that the family still exists for that user and client and reads the user's
//! status, so a revoked family's access tokens stop at once (Q12). It is the one place outside
//! tests that builds an [`OrgScope`] from an app access token.

use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use serde_json::json;
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    scope::Credential,
    users::Status,
};

/// How long a refresh token can be used after it is issued (Q11 (b): 30 days, sliding with
/// each refresh).
pub const LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How long a family lasts from its first token, however often it is refreshed (Q11 (b)).
pub const FAMILY_LIFETIME: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// Why a refresh token repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `user_id` names no user of the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// A stored user status this release does not know; the table's check keeps it from
    /// happening.
    #[error("the token's user has a status this release does not know")]
    UnknownStatus,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A refresh token to store.
#[derive(Clone, Copy, Debug)]
pub struct NewToken<'a> {
    /// The row's key (`id::new_id`); a family's first token's is the family's id.
    pub id: Uuid,
    /// The user the token acts as. Must be a user of the scope's organization.
    pub user_id: Uuid,
    pub client_id: &'a str,
    /// SHA-256 of the token the caller has just generated.
    pub token_hash: &'a [u8; 32],
}

/// Store `token` as the first of a new family, issued at `now`. Returns its expiry, [`LIFETIME`]
/// later. The user is taken from a `SELECT` filtered by the scope, so a user of another
/// organization stores nothing and is [`Error::UserNotFound`].
pub async fn start_family(
    conn: &mut PgConnection,
    scope: OrgScope,
    token: &NewToken<'_>,
    now: SystemTime,
) -> Result<SystemTime, Error> {
    let expires_at = now.checked_add(LIFETIME).ok_or(TimeOutOfRange)?;
    let inserted = sqlx::query_scalar!(
        r#"INSERT INTO oauth_refresh_tokens
               (id, org_id, client_id, user_id, token_hash, family_id, rotated_from, expires_at,
                created_at, updated_at)
           SELECT $1, u.org_id, $4, u.id, $5, $1, NULL,
                  timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.org_id = $2 AND u.id = $3
           RETURNING id"#,
        token.id,
        scope.org_id(),
        token.user_id,
        token.client_id,
        token.token_hash.as_slice(),
        epoch_micros(now)?,
        epoch_micros(expires_at)?,
    )
    .fetch_optional(conn)
    .await?;
    inserted.map(|_| expires_at).ok_or(Error::UserNotFound)
}

/// A presented refresh token as a refresh reads it, its row locked until the transaction ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedToken {
    pub id: Uuid,
    pub family_id: Uuid,
    pub user_id: Uuid,
    pub client_id: String,
    pub expires_at: SystemTime,
    /// When the family's first token was issued.
    pub family_started_at: SystemTime,
    /// The user's status now.
    pub user_status: Status,
}

struct LockedRow {
    id: Uuid,
    family_id: Uuid,
    user_id: Uuid,
    client_id: String,
    expires_at_us: Option<i64>,
    family_started_at_us: Option<i64>,
    status: String,
}

/// The refresh token of the scope's organization whose hash is `token_hash`, with its row locked
/// for the caller's transaction (`FOR UPDATE`), or `None`. Whether it was already rotated is a
/// separate statement, [`is_rotated`], run after the lock is held: under PostgreSQL's read
/// committed isolation a statement that waited for the lock still sees other rows as they were
/// when it started, so only a later statement sees the row a concurrent refresh just committed.
pub async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    token_hash: &[u8; 32],
) -> Result<Option<LockedToken>, Error> {
    let row = sqlx::query_as!(
        LockedRow,
        r#"SELECT r.id, r.family_id, r.user_id, r.client_id,
                  (extract(epoch FROM r.expires_at) * 1000000)::bigint AS expires_at_us,
                  (extract(epoch FROM f.created_at) * 1000000)::bigint AS family_started_at_us,
                  u.status
           FROM oauth_refresh_tokens r
           JOIN oauth_refresh_tokens f ON f.org_id = r.org_id AND f.id = r.family_id
           JOIN users u ON u.org_id = r.org_id AND u.id = r.user_id
           WHERE r.org_id = $1 AND r.token_hash = $2
           FOR UPDATE OF r"#,
        scope.org_id(),
        token_hash.as_slice(),
    )
    .fetch_optional(conn)
    .await?;
    row.map(LockedToken::try_from).transpose()
}

impl TryFrom<LockedRow> for LockedToken {
    type Error = Error;

    fn try_from(row: LockedRow) -> Result<Self, Error> {
        let micros = |value: Option<i64>| from_epoch_micros(value.ok_or(TimeOutOfRange)?);
        Ok(Self {
            id: row.id,
            family_id: row.family_id,
            user_id: row.user_id,
            client_id: row.client_id,
            expires_at: micros(row.expires_at_us)?,
            family_started_at: micros(row.family_started_at_us)?,
            user_status: Status::parse(&row.status).ok_or(Error::UnknownStatus)?,
        })
    }
}

/// Whether the token `id` of the scope's organization has been rotated: a row rotated from it
/// exists.
pub async fn is_rotated(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<bool, Error> {
    let rotated = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM oauth_refresh_tokens WHERE org_id = $1 AND rotated_from = $2
           ) AS "rotated!""#,
        scope.org_id(),
        id,
    )
    .fetch_one(conn)
    .await?;
    Ok(rotated)
}

/// What [`rotate`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    /// The new token is stored and expires at this time.
    Rotated(SystemTime),
    /// Another token already rotated from the presented one; nothing was stored.
    AlreadyRotated,
    /// The family reached [`FAMILY_LIFETIME`] at `now`; nothing was stored.
    FamilyExpired,
}

/// Store `next` as the token that replaces `presented`, in its family, issued at `now`. It
/// expires [`LIFETIME`] later or at the family's end, whichever comes first. The unique index on
/// `rotated_from` lets one token be rotated once: a second rotation stores nothing and answers
/// [`Rotation::AlreadyRotated`] without aborting the transaction.
pub async fn rotate(
    conn: &mut PgConnection,
    scope: OrgScope,
    presented: &LockedToken,
    next: &NewToken<'_>,
    now: SystemTime,
) -> Result<Rotation, Error> {
    let family_ends = presented
        .family_started_at
        .checked_add(FAMILY_LIFETIME)
        .ok_or(TimeOutOfRange)?;
    let expires_at = now
        .checked_add(LIFETIME)
        .ok_or(TimeOutOfRange)?
        .min(family_ends);
    if expires_at <= now {
        return Ok(Rotation::FamilyExpired);
    }
    let inserted = sqlx::query_scalar!(
        r#"INSERT INTO oauth_refresh_tokens
               (id, org_id, client_id, user_id, token_hash, family_id, rotated_from, expires_at,
                created_at, updated_at)
           SELECT $1, r.org_id, r.client_id, r.user_id, $3, r.family_id, r.id,
                  timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           FROM oauth_refresh_tokens r
           WHERE r.org_id = $2 AND r.id = $6
           ON CONFLICT (rotated_from) DO NOTHING
           RETURNING id"#,
        next.id,
        scope.org_id(),
        next.token_hash.as_slice(),
        epoch_micros(now)?,
        epoch_micros(expires_at)?,
        presented.id,
    )
    .fetch_optional(conn)
    .await?;
    Ok(match inserted {
        Some(_) => Rotation::Rotated(expires_at),
        None => Rotation::AlreadyRotated,
    })
}

/// Delete every token of the family `family_id` in the scope's organization. Returns how many
/// were deleted.
pub async fn revoke_family(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    family_id: Uuid,
) -> Result<u64, Error> {
    let result = sqlx::query!(
        "DELETE FROM oauth_refresh_tokens WHERE org_id = $1 AND family_id = $2",
        scope.org_id(),
        family_id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

/// Sign out every app signed in as the user `user_id` in the scope's organization: delete the
/// user's unused authorization codes, then every refresh token of the user, every family and
/// client alike, so no app keeps or gains a sign-in from before the call and the apps' access
/// tokens stop on their next request. A change that ends the user's other browser sessions calls
/// this in its transaction (a new password, the use of an enrollment link, spec 08 B46).
///
/// The codes go first: a code exchange in progress holds its code's row lock, so the delete
/// waits for it, skips the code it marked used, and the token delete, a later statement, sees the
/// family it started. Used codes stay, so presenting one again is still recognized as reuse.
/// Returns how many refresh tokens were deleted.
pub async fn revoke_all_of_user(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<u64, Error> {
    sqlx::query!(
        "DELETE FROM oauth_authorization_codes \
         WHERE org_id = $1 AND user_id = $2 AND used_at IS NULL",
        scope.org_id(),
        user_id,
    )
    .execute(&mut *conn)
    .await?;
    let result = sqlx::query!(
        "DELETE FROM oauth_refresh_tokens WHERE org_id = $1 AND user_id = $2",
        scope.org_id(),
        user_id,
    )
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

/// What was presented again when a family is revoked for reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reused {
    /// A refresh token that had already been rotated (B34).
    RefreshToken,
    /// An authorization code that had already been exchanged (B32).
    AuthorizationCode,
}

impl Reused {
    fn as_str(self) -> &'static str {
        match self {
            Self::RefreshToken => "refresh_token",
            Self::AuthorizationCode => "authorization_code",
        }
    }
}

/// A family revoked because a spent credential of it was presented again.
#[derive(Clone, Copy, Debug)]
pub struct Reuse<'a> {
    pub family_id: Uuid,
    pub user_id: Uuid,
    pub client_id: &'a str,
    pub presented: Reused,
    /// The client address of the request that presented it.
    pub client_address: IpAddr,
}

/// Revoke `reuse.family_id` ([`revoke_family`]) and write the `refresh_token.reuse` audit entry
/// on the family, in the caller's transaction, at `now` (Q14). The entry's actor is the
/// instance, which detected the reuse: whoever presented the credential is not known to be its
/// user. Its data names the user, the client, what was presented, the client address and how
/// many tokens were deleted; never a token or hash.
pub async fn revoke_reused(
    conn: &mut PgConnection,
    scope: OrgScope,
    reuse: &Reuse<'_>,
    now: SystemTime,
) -> Result<u64, Error> {
    let deleted = revoke_family(&mut *conn, scope, reuse.family_id).await?;
    let data = json!({
        "user_id": reuse.user_id,
        "client_id": reuse.client_id,
        "presented": reuse.presented.as_str(),
        "client_address": reuse.client_address.to_string(),
        "tokens_deleted": deleted,
    });
    audit::record(
        conn,
        scope,
        Actor::Instance,
        Action::RefreshTokenReuse,
        reuse.family_id,
        &data,
        epoch_micros(now)?,
    )
    .await?;
    Ok(deleted)
}

/// Delete the family of the refresh token whose hash is `token_hash`, when that token belongs
/// to the scope's organization and was issued to `client_id`, spent or not (RFC 7009). Returns
/// whether a family was deleted.
pub async fn revoke_by_token_hash(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    token_hash: &[u8; 32],
    client_id: &str,
) -> Result<bool, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM oauth_refresh_tokens
           WHERE org_id = $1
             AND family_id = (SELECT family_id FROM oauth_refresh_tokens
                              WHERE org_id = $1 AND token_hash = $2 AND client_id = $3)"#,
        scope.org_id(),
        token_hash.as_slice(),
        client_id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// What an app access token's family and user say on this request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTokenAuthentication {
    /// The family exists for the user and client, and the user is active.
    Valid(AppTokenPrincipal),
    /// The family exists, and its user is suspended (B36).
    Suspended,
    /// No such family for that user and client in the request's organization: revoked, reused,
    /// expired and cleaned up, or never this organization's.
    Unknown,
}

/// The principal of a request an app access token authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppTokenPrincipal {
    pub user_id: Uuid,
    /// The user's organization, for every repository call of the request.
    pub scope: OrgScope,
}

/// Check the family `family_id` of an app access token for `user_id` and `client_id` within the
/// request's organization, and read the user's status (B36, Q12 (b)). One indexed lookup.
pub async fn authenticate(
    db: impl PgExecutor<'_>,
    request_org: OrgScope,
    user_id: Uuid,
    family_id: Uuid,
    client_id: &str,
) -> Result<AppTokenAuthentication, Error> {
    let status = sqlx::query_scalar!(
        r#"SELECT u.status
           FROM oauth_refresh_tokens f
           JOIN users u ON u.org_id = f.org_id AND u.id = f.user_id
           WHERE f.org_id = $1 AND f.id = $2 AND f.family_id = $2
             AND f.user_id = $3 AND f.client_id = $4"#,
        request_org.org_id(),
        family_id,
        user_id,
        client_id,
    )
    .fetch_optional(db)
    .await?;
    let Some(status) = status else {
        return Ok(AppTokenAuthentication::Unknown);
    };
    Ok(match Status::parse(&status).ok_or(Error::UnknownStatus)? {
        Status::Active => AppTokenAuthentication::Valid(AppTokenPrincipal {
            user_id,
            scope: app_token_scope(request_org.org_id()),
        }),
        Status::Suspended => AppTokenAuthentication::Suspended,
    })
}

/// The scope of a request authenticated by an app access token of a family in `org_id`.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9: the app-token resolver builds the credential scope from the family's row, found within the request's organization"
)]
fn app_token_scope(org_id: Uuid) -> OrgScope {
    OrgScope::from_credential(org_id, Credential::AppToken)
}

/// Delete every family of the scope's organization none of whose tokens is still valid at
/// `now`, for the periodic cleanup. Returns how many tokens were deleted.
pub async fn delete_expired(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<u64, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM oauth_refresh_tokens r
           WHERE r.org_id = $1
             AND r.family_id IN (
                 SELECT f.family_id FROM oauth_refresh_tokens f
                 WHERE f.org_id = $1
                 GROUP BY f.family_id
                 HAVING max(f.expires_at)
                        <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
             )"#,
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
    fn a_token_lives_thirty_days_and_a_family_ninety() {
        assert_eq!(LIFETIME.as_secs(), 30 * 86_400);
        assert_eq!(FAMILY_LIFETIME.as_secs(), 90 * 86_400);
    }
}
