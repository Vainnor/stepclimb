// SPDX-License-Identifier: AGPL-3.0-or-later
//! TOTP on local accounts: the sealed secret, its confirmation, the replay step and the recovery
//! codes (docs/specs/m0/04-local-accounts.md B15, B19 to B21, Data, Q3, Q11).
//!
//! The secret lives on the user's `local_logins` row, so only a user with a password has one.
//! This module never sees it in the clear: the server seals it with the key ring before
//! [`start`] and opens what [`lock`] returns. Likewise it stores only the SHA-256 of each
//! recovery code. Every query filters by `org_id = scope.org_id()`.
//!
//! Every function but [`lock`] changes the row; callers run them in a transaction after
//! `identities::lock_user` and [`lock`], so the state a code was checked against is the state
//! that is written. [`confirm`] and [`disable`] write the `totp.enable` and `totp.disable` audit
//! entries with the user (`09-permissions-and-roles.md` B22), never a secret or a code.

use std::time::SystemTime;

use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    id::new_id,
};

/// A user's TOTP as [`lock`] read it. Its `Debug` output leaves the secret out.
#[derive(Clone, PartialEq, Eq)]
pub struct Totp {
    /// Whether the user has a password, which TOTP needs.
    pub has_password: bool,
    /// The password's generation (`local_logins::Login::password_generation`).
    pub password_generation: i64,
    /// The sealed secret, confirmed or not; `None` when TOTP was never started or is disabled.
    pub sealed_secret: Option<Vec<u8>>,
    /// Whether a code confirmed the secret. Only then does sign-in ask for a code (B15).
    pub confirmed: bool,
    /// The step of the last code accepted for the user.
    pub last_step: Option<i64>,
}

impl std::fmt::Debug for Totp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Totp")
            .field("has_password", &self.has_password)
            .field("password_generation", &self.password_generation)
            .field("has_secret", &self.sealed_secret.is_some())
            .field("confirmed", &self.confirmed)
            .field("last_step", &self.last_step)
            .finish()
    }
}

/// Why a TOTP call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The user has no password, or no login at all.
    #[error("the user has no password")]
    NoPassword,
    /// TOTP is already confirmed: starting again would replace a working secret.
    #[error("TOTP is already confirmed for the user")]
    AlreadyConfirmed,
    /// There is no unconfirmed secret to confirm.
    #[error("no TOTP enrollment is waiting for confirmation")]
    NotStarted,
    /// There is no confirmed TOTP to disable.
    #[error("TOTP is not confirmed for the user")]
    NotConfirmed,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The TOTP of the user `user_id` in the scope's organization, its `local_logins` row locked
/// `FOR UPDATE` until the caller's transaction ends; `None` when the user has no login.
pub async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Option<Totp>, Error> {
    let row = sqlx::query!(
        r#"SELECT password_hash IS NOT NULL AS "has_password!", password_generation,
                  totp_secret, totp_confirmed_at IS NOT NULL AS "confirmed!", totp_last_step
           FROM local_logins WHERE org_id = $1 AND user_id = $2
           FOR UPDATE"#,
        scope.org_id(),
        user_id,
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| Totp {
        has_password: row.has_password,
        password_generation: row.password_generation,
        sealed_secret: row.totp_secret,
        confirmed: row.confirmed,
        last_step: row.totp_last_step,
    }))
}

/// Store `sealed_secret` as the unconfirmed secret of the user `user_id` (B19), replacing an
/// unconfirmed one. Refuses [`Error::NoPassword`] and [`Error::AlreadyConfirmed`]. Writes no
/// audit entry: an unconfirmed secret signs no one in.
pub async fn start(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    sealed_secret: &[u8],
    now: SystemTime,
) -> Result<(), Error> {
    let current = lock(&mut *conn, scope, user_id).await?;
    match current {
        Some(totp) if totp.confirmed => return Err(Error::AlreadyConfirmed),
        Some(totp) if totp.has_password => {}
        _ => return Err(Error::NoPassword),
    }
    sqlx::query!(
        "UPDATE local_logins
         SET totp_secret = $3, totp_confirmed_at = NULL, totp_last_step = NULL,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2",
        scope.org_id(),
        user_id,
        sealed_secret,
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Confirm the waiting secret of the user `user_id`, whose code the caller accepted and
/// recorded with [`record_step`], store `recovery_hashes` as the user's recovery codes in place
/// of any earlier ones, and write `totp.enable` by `actor`. Refuses [`Error::NotStarted`] when no
/// unconfirmed secret waits.
pub async fn confirm(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    recovery_hashes: &[[u8; 32]],
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let confirmed = sqlx::query!(
        "UPDATE local_logins
         SET totp_confirmed_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2
           AND totp_secret IS NOT NULL AND totp_confirmed_at IS NULL",
        scope.org_id(),
        user_id,
        now_us,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if confirmed == 0 {
        return Err(Error::NotStarted);
    }
    delete_recovery_codes(&mut *conn, scope, user_id).await?;
    for hash in recovery_hashes {
        sqlx::query!(
            "INSERT INTO totp_recovery_codes (id, org_id, user_id, code_hash, created_at,
                                              updated_at)
             VALUES ($1, $2, $3, $4,
                     timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                     timestamptz 'epoch' + $5::bigint * interval '1 microsecond')",
            new_id(),
            scope.org_id(),
            user_id,
            hash.as_slice(),
            now_us,
        )
        .execute(&mut *conn)
        .await?;
    }
    let data = json!({ "after": { "user_id": user_id } });
    audit::record(
        conn,
        scope,
        actor,
        Action::TotpEnable,
        user_id,
        &data,
        now_us,
    )
    .await?;
    Ok(())
}

/// Record `step` as the step of the last code accepted for the user `user_id` (B20), unless a
/// later one is already recorded. Returns whether it was recorded.
pub async fn record_step(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    step: i64,
    now: SystemTime,
) -> Result<bool, Error> {
    let recorded = sqlx::query!(
        "UPDATE local_logins
         SET totp_last_step = $3,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2
           AND (totp_last_step IS NULL OR totp_last_step < $3)",
        scope.org_id(),
        user_id,
        step,
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(recorded > 0)
}

/// Use the unused recovery code of the user `user_id` whose hash is `code_hash`: mark it used
/// at `now`. Returns whether there was one.
pub async fn use_recovery_code(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    code_hash: &[u8; 32],
    now: SystemTime,
) -> Result<bool, Error> {
    let used = sqlx::query!(
        "UPDATE totp_recovery_codes
         SET used_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2 AND code_hash = $3 AND used_at IS NULL",
        scope.org_id(),
        user_id,
        code_hash.as_slice(),
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(used > 0)
}

/// Remove the confirmed TOTP of the user `user_id`: the secret, the replay step and every
/// recovery code, with `totp.disable` by `actor` (B21). Refuses [`Error::NotConfirmed`].
pub async fn disable(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    let disabled = sqlx::query!(
        "UPDATE local_logins
         SET totp_secret = NULL, totp_confirmed_at = NULL, totp_last_step = NULL,
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND user_id = $2 AND totp_confirmed_at IS NOT NULL",
        scope.org_id(),
        user_id,
        now_us,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if disabled == 0 {
        return Err(Error::NotConfirmed);
    }
    delete_recovery_codes(&mut *conn, scope, user_id).await?;
    let data = json!({ "before": { "user_id": user_id } });
    audit::record(
        conn,
        scope,
        actor,
        Action::TotpDisable,
        user_id,
        &data,
        now_us,
    )
    .await?;
    Ok(())
}

async fn delete_recovery_codes(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<(), Error> {
    sqlx::query!(
        "DELETE FROM totp_recovery_codes WHERE org_id = $1 AND user_id = $2",
        scope.org_id(),
        user_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}
