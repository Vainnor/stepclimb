// SPDX-License-Identifier: AGPL-3.0-or-later
//! The browser sessions repository and its credential resolver
//! (docs/specs/m0/08-sessions-and-csrf.md B6 to B11, Q4).
//!
//! A session is one row in `sessions`. It has no `org_id` of its own: its organization is its
//! user's, so every query here joins `users` and filters on `users.org_id = scope.org_id()`.
//!
//! The repository never sees a session token, only its SHA-256 hash
//! (`stepclimb_auth::token::TokenHash::as_bytes`), and the caller generates the token, so this
//! module cannot store or log one. The lookup finds the row by that hash through the unique
//! index; a hash of a 256-bit random token reveals nothing a timing difference could narrow.
//!
//! A session is valid while `now < expires_at` (30 days after sign-in at most) and
//! `now < last_seen_at + 12 hours` (B8). [`authenticate`] is the credential resolver: it is the
//! one place outside tests that builds an [`OrgScope`] from a session.

use std::time::{Duration, SystemTime};

use sqlx::{Acquire, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    scope::Credential,
    users::Status,
};

/// How long a session lasts after sign-in, however often it is used (B7).
pub const MAX_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How long a session lasts without a request (B8).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

/// The least time between two writes of `last_seen_at` (Q4), so idle expiry is accurate to a
/// minute and a burst of requests writes the row once.
pub const TOUCH_INTERVAL: Duration = Duration::from_secs(60);

/// The only kind of session this release creates (spec 08 Data).
const KIND_BROWSER: &str = "browser";

/// A session row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub last_seen_at: SystemTime,
    pub expires_at: SystemTime,
    pub created_at: SystemTime,
}

/// The principal a valid session authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPrincipal {
    pub session_id: Uuid,
    pub user_id: Uuid,
    /// The user's organization, with origin `Session`.
    pub scope: OrgScope,
}

/// What a presented session token turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authentication {
    /// The session is valid; `last_seen_at` was refreshed if [`TOUCH_INTERVAL`] had passed.
    Valid(SessionPrincipal),
    /// The session had passed its idle or absolute expiry and has been deleted (B10).
    Expired,
    /// The session is within its lifetime but its user is suspended. The row is kept, so
    /// unsuspending before it expires restores it (B11).
    Suspended,
    /// No session of the request's organization has this hash (B6, B10).
    Unknown,
}

/// Why a sessions repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// A stored user `status` this release does not know.
    #[error("the stored user status {0:?} is not one this release knows")]
    UnknownStatus(String),
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Whether a session last seen at `last_seen_at` and expiring at `expires_at` is valid at `now`.
fn is_live(now: SystemTime, last_seen_at: SystemTime, expires_at: SystemTime) -> bool {
    // `checked_add` fails only for a time no clock produces; such a session is treated as idle
    // past its limit rather than as valid.
    let idle_until = last_seen_at.checked_add(IDLE_TIMEOUT);
    now < expires_at && idle_until.is_some_and(|until| now < until)
}

/// Whether `last_seen_at` is old enough at `now` to be written again (Q4).
fn needs_touch(now: SystemTime, last_seen_at: SystemTime) -> bool {
    now.duration_since(last_seen_at)
        .is_ok_and(|since| since >= TOUCH_INTERVAL)
}

/// Create a browser session for the user `user_id` of the scope's organization, last seen at
/// `now` and expiring [`MAX_LIFETIME`] after it (B7). `token_hash` is the SHA-256 of a token the
/// caller has just generated for this session; never pass a token the request brought.
pub async fn create(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    token_hash: &[u8; 32],
    now: SystemTime,
) -> Result<Session, Error> {
    let expires_at = now.checked_add(MAX_LIFETIME).ok_or(TimeOutOfRange)?;
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO sessions (id, user_id, token_hash, kind, last_seen_at, expires_at,
                                 created_at, updated_at)
           SELECT $1, u.id, $4, $5,
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.org_id = $2 AND u.id = $3
           RETURNING id, user_id,
                     (extract(epoch FROM last_seen_at) * 1000000)::bigint AS "last_seen_at_us!",
                     (extract(epoch FROM expires_at) * 1000000)::bigint AS "expires_at_us!",
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!""#,
        new_id(),
        scope.org_id(),
        user_id,
        token_hash.as_slice(),
        KIND_BROWSER,
        epoch_micros(now)?,
        epoch_micros(expires_at)?,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::UserNotFound)?;
    Ok(Session {
        id: row.id,
        user_id: row.user_id,
        last_seen_at: from_epoch_micros(row.last_seen_at_us)?,
        expires_at: from_epoch_micros(row.expires_at_us)?,
        created_at: from_epoch_micros(row.created_at_us)?,
    })
}

/// A `sessions` row as [`create`] returns it.
struct Row {
    id: Uuid,
    user_id: Uuid,
    last_seen_at_us: i64,
    expires_at_us: i64,
    created_at_us: i64,
}

/// A session joined with its user, as [`authenticate`] reads it.
struct Found {
    id: Uuid,
    user_id: Uuid,
    org_id: Uuid,
    status: String,
    last_seen_at_us: i64,
    expires_at_us: i64,
}

/// Authenticate a presented session token within the request's organization (B6, B8 to B11).
///
/// `request_org` is the organization the request belongs to, from the request-organization
/// resolver; a session whose user belongs to another organization is [`Authentication::Unknown`]
/// and is left untouched. An expired or idle session is deleted. A valid one has its
/// `last_seen_at` set to `now` when [`TOUCH_INTERVAL`] has passed since the last write.
pub async fn authenticate<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    request_org: OrgScope,
    token_hash: &[u8; 32],
    now: SystemTime,
) -> Result<Authentication, Error> {
    let mut conn = db.acquire().await?;
    let found = sqlx::query_as!(
        Found,
        r#"SELECT s.id, s.user_id, u.org_id, u.status,
                  (extract(epoch FROM s.last_seen_at) * 1000000)::bigint AS "last_seen_at_us!",
                  (extract(epoch FROM s.expires_at) * 1000000)::bigint AS "expires_at_us!"
           FROM sessions s
           JOIN users u ON u.id = s.user_id
           WHERE u.org_id = $1 AND s.token_hash = $2"#,
        request_org.org_id(),
        token_hash.as_slice(),
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(found) = found else {
        return Ok(Authentication::Unknown);
    };
    let last_seen_at = from_epoch_micros(found.last_seen_at_us)?;
    let expires_at = from_epoch_micros(found.expires_at_us)?;
    if !is_live(now, last_seen_at, expires_at) {
        delete(&mut *conn, request_org, found.id).await?;
        return Ok(Authentication::Expired);
    }
    match Status::parse(&found.status) {
        Some(Status::Active) => {}
        Some(Status::Suspended) => return Ok(Authentication::Suspended),
        None => return Err(Error::UnknownStatus(found.status)),
    }
    if needs_touch(now, last_seen_at) {
        touch(&mut *conn, request_org, found.id, now).await?;
    }
    Ok(Authentication::Valid(SessionPrincipal {
        session_id: found.id,
        user_id: found.user_id,
        scope: session_scope(found.org_id),
    }))
}

/// The scope of a request authenticated by a session whose user is in `org_id`.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9: the browser-session resolver builds the credential scope from the session's user row"
)]
fn session_scope(org_id: Uuid) -> OrgScope {
    OrgScope::from_credential(org_id, Credential::Session)
}

/// Set `last_seen_at` and `updated_at` of the session `id` of the scope's organization to `now`.
/// A write that would move `last_seen_at` back (a concurrent request with an earlier reading)
/// changes nothing.
async fn touch(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
) -> Result<(), Error> {
    sqlx::query!(
        r#"UPDATE sessions s
           SET last_seen_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
               updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
           FROM users u
           WHERE u.id = s.user_id AND u.org_id = $1 AND s.id = $2
             AND s.last_seen_at < timestamptz 'epoch' + $3::bigint * interval '1 microsecond'"#,
        scope.org_id(),
        id,
        epoch_micros(now)?,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Delete the session `id` of the scope's organization. Returns whether a row was deleted.
pub async fn delete(db: impl PgExecutor<'_>, scope: OrgScope, id: Uuid) -> Result<bool, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM sessions s
           USING users u
           WHERE u.id = s.user_id AND u.org_id = $1 AND s.id = $2"#,
        scope.org_id(),
        id,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete the session of the scope's organization whose token hashes to `token_hash`, live or
/// expired, for a sign-in that replaces the session cookie the browser presented or a sign-out.
/// Returns whether a row was deleted; a hash of another organization's session deletes nothing.
pub async fn delete_by_token_hash(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    token_hash: &[u8; 32],
) -> Result<bool, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM sessions s
           USING users u
           WHERE u.id = s.user_id AND u.org_id = $1 AND s.token_hash = $2"#,
        scope.org_id(),
        token_hash.as_slice(),
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The session [`delete_others_of_user`] keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The session with this id, such as the one a sign-in has just created.
    Session(Uuid),
    /// The session whose token hashes to this, such as the one a request presents.
    TokenHash([u8; 32]),
    /// None: every session of the user ends, as when an administrator removes their TOTP
    /// (docs/specs/m0/04-local-accounts.md B21).
    Nothing,
}

/// Delete every session of the user `user_id` in the scope's organization but the one `keep`
/// names (all of them with [`Keep::Nothing`]), for a new password that ends the user's other
/// sessions (a change or a reset through an enrollment link, docs/specs/m0/04-local-accounts.md
/// B9, B17, Q8). Returns how many were deleted.
pub async fn delete_others_of_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    keep: Keep,
) -> Result<u64, Error> {
    let (id, hash) = match keep {
        Keep::Session(id) => (Some(id), None),
        Keep::TokenHash(hash) => (None, Some(hash.to_vec())),
        Keep::Nothing => (None, None),
    };
    let result = sqlx::query!(
        r#"DELETE FROM sessions s
           USING users u
           WHERE u.id = s.user_id AND u.org_id = $1 AND s.user_id = $2
             AND ($3::uuid IS NULL OR s.id <> $3)
             AND ($4::bytea IS NULL OR s.token_hash <> $4)"#,
        scope.org_id(),
        user_id,
        id,
        hash,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_763_000_000 + secs)
    }

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;

    #[test]
    fn a_session_is_live_until_twelve_idle_hours() {
        let expires = at(30 * 24 * HOUR);
        assert!(is_live(at(12 * HOUR - 1), at(0), expires));
        assert!(!is_live(at(12 * HOUR), at(0), expires));
        assert!(!is_live(at(12 * HOUR + 1), at(0), expires));
    }

    #[test]
    fn a_session_is_dead_at_its_absolute_expiry_however_recently_used() {
        let expires = at(30 * 24 * HOUR);
        let just_seen = at(30 * 24 * HOUR - MINUTE);
        assert!(is_live(at(30 * 24 * HOUR - 1), just_seen, expires));
        assert!(!is_live(expires, just_seen, expires));
    }

    #[test]
    fn last_seen_is_written_at_most_once_a_minute() {
        assert!(!needs_touch(at(59), at(0)));
        assert!(needs_touch(at(MINUTE), at(0)));
        assert!(
            !needs_touch(at(0), at(MINUTE)),
            "a clock reading before last_seen_at never writes"
        );
    }

    #[test]
    fn the_lifetimes_are_the_specs() {
        assert_eq!(MAX_LIFETIME.as_secs(), 30 * 24 * HOUR);
        assert_eq!(IDLE_TIMEOUT.as_secs(), 12 * HOUR);
        assert_eq!(TOUCH_INTERVAL.as_secs(), MINUTE);
    }
}
