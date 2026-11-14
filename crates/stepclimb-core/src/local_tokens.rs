// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-time local sign-in tokens: enrollment links (docs/specs/m0/04-local-accounts.md B6, B7,
//! Q6, Q7).
//!
//! An enrollment link lets one user of the organization set up a local credential (a passkey,
//! issue I3, or a password, issue I6) without signing in first. The caller generates the token
//! and passes only its SHA-256 (`stepclimb_auth::token::TokenHash::as_bytes`); this module never
//! sees the token. [`create_enrollment`] stores the hash with an expiry [`ENROLLMENT_LIFETIME`]
//! after `now`, removes the user's earlier unused enrollment links, and writes the
//! `enrollment_link.create` audit entry. [`check_enrollment`] reads a presented token without
//! using it; [`consume_enrollment`] marks it used, once, in the caller's transaction.
//!
//! Both read the token by hash within the scope's organization only, so another organization's
//! token is [`Error::Invalid`], as is an unknown token, a used one, and one whose user has no
//! email (local sign-in finds a user by email, B4). A token past its expiry is
//! [`Error::Expired`] (B7). Every query filters by `org_id = scope.org_id()`.
//!
//! A link an admin made works only while that admin could make it again, so both re-check its
//! creator at use ([`Origin`]): the creator is still a user of the organization, active, holding
//! `user.manage` at organization scope, and holding at organization scope every permission the
//! link's user now holds at any scope (the escalation rule of the user routes). A failed check
//! is [`Error::Invalid`], the same answer as an unknown token. A link the instance made (`stepclimb
//! admin create`, `stepclimb admin enrollment-link`) has no creator to check; one whose creator
//! was deleted has `created_by` NULL without being the instance's, and is invalid.

use std::time::{Duration, SystemTime};

use serde_json::json;
use sqlx::{Acquire, PgConnection, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    grants::{Grants, Target},
    id::new_id,
    perm::UserManage,
    role_assignments,
    users::{self, Status},
};

/// How long an enrollment link works (Q6: 7 days).
pub const ENROLLMENT_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The stored `kind` of an enrollment link.
const KIND_ENROLLMENT: &str = "enrollment";

/// Who made an enrollment link, which decides what its use re-checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// An admin through the API: the link works only while this user could create it again.
    Admin(Uuid),
    /// `stepclimb admin create`, for the organization's first admin (Q7).
    Bootstrap,
    /// `stepclimb admin enrollment-link`, the host's way back in; its audit entry says so.
    Recovery,
}

impl Origin {
    fn created_by(self) -> Option<Uuid> {
        match self {
            Self::Admin(id) => Some(id),
            Self::Bootstrap | Self::Recovery => None,
        }
    }
}

/// An enrollment link as [`create_enrollment`] stored it. It holds no token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentLink {
    pub id: Uuid,
    pub user_id: Uuid,
    pub expires_at: SystemTime,
}

/// A live enrollment link and the user it enrolls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
    pub id: Uuid,
    pub user_id: Uuid,
    /// The user's email, which local sign-in finds them by.
    pub email: String,
}

/// Why a local token call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// The user has no email, which local sign-in finds a user by (B4).
    #[error("the user has no email address")]
    EmailMissing,
    /// No unused enrollment link of the organization has this token, its user has no email, or
    /// its creator could no longer create it.
    #[error("the enrollment link is unknown, used or no longer backed by its creator")]
    Invalid,
    /// The enrollment link passed its expiry.
    #[error("the enrollment link has expired")]
    Expired,
    /// Reading a user's grants failed.
    #[error(transparent)]
    Grants(#[from] role_assignments::Error),
    /// Reading a user failed.
    #[error(transparent)]
    User(#[from] users::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Store an enrollment link for the user `user_id` of the scope's organization, made by
/// `origin` and audited as `actor`, at `now`, expiring [`ENROLLMENT_LIFETIME`] later.
/// `token_hash` is the SHA-256 of a token the caller has just generated.
///
/// Locks the user's row `FOR NO KEY UPDATE` first, so two creations for one user take turns and
/// the second removes the first's link. Refuses a user the organization does not have
/// ([`Error::UserNotFound`]) and one without an email ([`Error::EmailMissing`]). Deletes the
/// user's earlier unused enrollment links (B6), inserts the new one and writes its
/// `enrollment_link.create` entry with the user and the expiry (and `recovery: true` for
/// [`Origin::Recovery`]), never the token or its hash. Runs in one transaction (a savepoint when
/// `db` is already one).
pub async fn create_enrollment<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    token_hash: &[u8; 32],
    origin: Origin,
    now: SystemTime,
    actor: Actor,
) -> Result<EnrollmentLink, Error> {
    let expires_at = now.checked_add(ENROLLMENT_LIFETIME).ok_or(TimeOutOfRange)?;
    let (now_us, expires_us) = (epoch_micros(now)?, epoch_micros(expires_at)?);
    let mut tx = db.begin().await?;
    let email = sqlx::query_scalar!(
        "SELECT email FROM users WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::UserNotFound)?;
    if email.is_none() {
        return Err(Error::EmailMissing);
    }
    sqlx::query!(
        "DELETE FROM local_tokens
         WHERE org_id = $1 AND user_id = $2 AND kind = $3 AND used_at IS NULL",
        scope.org_id(),
        user_id,
        KIND_ENROLLMENT,
    )
    .execute(&mut *tx)
    .await?;
    let id = new_id();
    sqlx::query!(
        "INSERT INTO local_tokens (id, org_id, user_id, kind, token_hash, created_by,
                                   created_by_instance, expires_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7,
                 timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $8::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        user_id,
        KIND_ENROLLMENT,
        token_hash.as_slice(),
        origin.created_by(),
        origin.created_by().is_none(),
        now_us,
        expires_us,
    )
    .execute(&mut *tx)
    .await?;
    let expires = jiff::Timestamp::try_from(expires_at).map_err(|_| TimeOutOfRange)?;
    let mut after = json!({ "user_id": user_id, "expires_at": expires.to_string() });
    if origin == Origin::Recovery {
        after["recovery"] = true.into();
    }
    let data = json!({ "after": after });
    let action = Action::EnrollmentLinkCreate;
    audit::record(&mut tx, scope, actor, action, id, &data, now_us).await?;
    tx.commit().await?;
    Ok(EnrollmentLink {
        id,
        user_id,
        expires_at,
    })
}

/// A `local_tokens` row joined with its user's email, as the reads below select it.
struct Found {
    id: Uuid,
    user_id: Uuid,
    email: Option<String>,
    used: bool,
    expires_at_us: i64,
    created_by: Option<Uuid>,
    created_by_instance: bool,
}

impl Found {
    /// [`Error::Invalid`] for a used link or a user without an email, [`Error::Expired`] for a
    /// link at or past its expiry at `now`, else the enrollment. The creator is checked
    /// separately ([`refuse_unbacked`]).
    fn live(&self, now: SystemTime) -> Result<Enrollment, Error> {
        let Some(email) = self.email.clone().filter(|_| !self.used) else {
            return Err(Error::Invalid);
        };
        if now >= from_epoch_micros(self.expires_at_us)? {
            return Err(Error::Expired);
        }
        Ok(Enrollment {
            id: self.id,
            user_id: self.user_id,
            email,
        })
    }
}

/// [`Error::Invalid`] unless the link's creator could create it again now (see the module
/// documentation): the instance always could; an admin must be an active user of the
/// organization holding `user.manage` at organization scope and every permission the link's
/// user holds; a link with neither lost its creator.
async fn refuse_unbacked(
    conn: &mut PgConnection,
    scope: OrgScope,
    found: &Found,
) -> Result<(), Error> {
    if found.created_by_instance {
        return Ok(());
    }
    let Some(creator) = found.created_by else {
        return Err(Error::Invalid);
    };
    let active = users::get(&mut *conn, scope, creator)
        .await?
        .is_some_and(|user| user.status == Status::Active);
    if !active {
        return Err(Error::Invalid);
    }
    let creator = Grants::load(&mut *conn, scope, creator).await?;
    if !creator.holds::<UserManage>(&Target::Organization) {
        return Err(Error::Invalid);
    }
    let target = Grants::load(&mut *conn, scope, found.user_id).await?;
    if creator.lacks_any_held_by(&target) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// The live enrollment link of the scope's organization whose token hashes to `token_hash` at
/// `now`, without using it (B7). See the module documentation for the refusals.
pub async fn check_enrollment(
    conn: &mut PgConnection,
    scope: OrgScope,
    token_hash: &[u8; 32],
    now: SystemTime,
) -> Result<Enrollment, Error> {
    let found = sqlx::query_as!(
        Found,
        r#"SELECT t.id, t.user_id, u.email, t.used_at IS NOT NULL AS "used!",
                  (extract(epoch FROM t.expires_at) * 1000000)::bigint AS "expires_at_us!",
                  t.created_by, t.created_by_instance
           FROM local_tokens t
           JOIN users u ON u.org_id = t.org_id AND u.id = t.user_id
           WHERE t.org_id = $1 AND t.token_hash = $2 AND t.kind = $3"#,
        scope.org_id(),
        token_hash.as_slice(),
        KIND_ENROLLMENT,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::Invalid)?;
    let enrollment = found.live(now)?;
    refuse_unbacked(conn, scope, &found).await?;
    Ok(enrollment)
}

/// Use the live enrollment link of the scope's organization whose token hashes to
/// `token_hash`: lock its row, refuse it as [`check_enrollment`] does, and set `used_at` to
/// `now`. The completion that sets up the credential calls this in its own transaction, at
/// `READ COMMITTED`, so the link is used exactly when the credential is stored; a second call,
/// here or concurrently once the first commits, is [`Error::Invalid`].
pub async fn consume_enrollment(
    conn: &mut PgConnection,
    scope: OrgScope,
    token_hash: &[u8; 32],
    now: SystemTime,
) -> Result<Enrollment, Error> {
    let found = sqlx::query_as!(
        Found,
        r#"SELECT t.id, t.user_id, u.email, t.used_at IS NOT NULL AS "used!",
                  (extract(epoch FROM t.expires_at) * 1000000)::bigint AS "expires_at_us!",
                  t.created_by, t.created_by_instance
           FROM local_tokens t
           JOIN users u ON u.org_id = t.org_id AND u.id = t.user_id
           WHERE t.org_id = $1 AND t.token_hash = $2 AND t.kind = $3
           FOR NO KEY UPDATE OF t"#,
        scope.org_id(),
        token_hash.as_slice(),
        KIND_ENROLLMENT,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::Invalid)?;
    let enrollment = found.live(now)?;
    refuse_unbacked(conn, scope, &found).await?;
    let now_us = epoch_micros(now)?;
    sqlx::query!(
        "UPDATE local_tokens
         SET used_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        enrollment.id,
        now_us,
    )
    .execute(&mut *conn)
    .await?;
    Ok(enrollment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(email: Option<&str>, used: bool, expires_at_us: i64) -> Found {
        Found {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            email: email.map(str::to_owned),
            used,
            expires_at_us,
            created_by: None,
            created_by_instance: true,
        }
    }

    fn at_us(micros: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_micros(micros)
    }

    #[test]
    fn the_lifetime_is_seven_days() {
        assert_eq!(ENROLLMENT_LIFETIME.as_secs(), 604_800);
    }

    #[test]
    fn a_link_works_until_its_expiry_and_not_at_it() {
        let live = found(Some("sam@example.test"), false, 1_000);
        assert_eq!(live.live(at_us(999)).unwrap().email, "sam@example.test");
        assert!(matches!(live.live(at_us(1_000)), Err(Error::Expired)));
    }

    #[test]
    fn a_used_link_or_a_user_without_email_is_invalid_even_when_expired() {
        let used = found(Some("sam@example.test"), true, 1_000);
        assert!(matches!(used.live(at_us(2_000)), Err(Error::Invalid)));
        let no_email = found(None, false, 1_000);
        assert!(matches!(no_email.live(at_us(0)), Err(Error::Invalid)));
    }

    #[test]
    fn only_an_admins_link_names_its_creator() {
        let admin = Uuid::from_u128(7);
        assert_eq!(Origin::Admin(admin).created_by(), Some(admin));
        assert_eq!(Origin::Bootstrap.created_by(), None);
        assert_eq!(Origin::Recovery.created_by(), None);
    }
}
