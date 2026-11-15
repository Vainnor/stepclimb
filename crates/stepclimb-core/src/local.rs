// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local accounts' credentials (docs/specs/m0/04-local-accounts.md B3, B13, Q10).
//!
//! [`has_credential`] says whether a user can sign in with a local credential: a passkey or a
//! password. `identities::unlink_unless_last` counts it as a sign-in method
//! besides the user's identities at enabled providers (docs/specs/m0/03-users-and-identities.md
//! B19).
//!
//! [`delete_passkey_unless_last`] is the same rule seen from the other side: a user removes one
//! of their passkeys only while another way to sign in stays, which is another passkey, a
//! password, or an identity at an enabled provider that is not local (Q10, 409
//! `local.last_method`).

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, Postgres};
use uuid::Uuid;

use crate::{OrgScope, audit::Actor, identities, local_logins, passkeys};

/// Why reading a user's local credentials failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Passkeys(#[from] passkeys::Error),
    #[error(transparent)]
    LocalLogins(#[from] local_logins::Error),
}

/// Whether the user `user_id` of the scope's organization has a passkey
/// ([`passkeys::exists_for_user`]) or a password ([`local_logins::has_password`]).
///
/// A credential counts whether or not its method is switched on: a switched-off method's
/// credentials work again once it is switched back on (B2), so the user still has them.
pub async fn has_credential(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<bool, Error> {
    if passkeys::exists_for_user(&mut *conn, scope, user_id).await? {
        return Ok(true);
    }
    Ok(local_logins::has_password(&mut *conn, scope, user_id).await?)
}

/// Why [`delete_passkey_unless_last`] refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    /// The user has no passkey with that id in the scope's organization.
    #[error("the passkey does not exist")]
    NotFound,
    /// The passkey is the user's last way to sign in.
    #[error("the passkey is the user's last way to sign in")]
    LastSignInMethod,
    #[error(transparent)]
    Identities(#[from] identities::Error),
    #[error(transparent)]
    Passkeys(#[from] passkeys::Error),
    #[error(transparent)]
    LocalLogins(#[from] local_logins::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Delete the passkey `id` of the user `user_id` in the scope's organization, as
/// [`passkeys::delete`] does (audit entry included), unless the user would be left with no way
/// to sign in ([`DeleteError::LastSignInMethod`]): no other passkey, no password, and no
/// identity at an enabled provider that is not local. A passkey of another user or organization
/// is [`DeleteError::NotFound`], checked first.
///
/// Takes `identities::lock_user` first, the lock `identities::unlink_unless_last` takes, so two
/// removals for one user (of passkeys, identities or both) take turns and the second counts what
/// the first left. Runs in one transaction (a savepoint when `db` is already one); the lock is
/// held until the outermost transaction ends.
pub async fn delete_passkey_unless_last<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), DeleteError> {
    let mut tx = db.begin().await?;
    identities::lock_user(&mut tx, scope, user_id).await?;
    let mine = passkeys::list_for_user(&mut *tx, scope, user_id).await?;
    if !mine.iter().any(|passkey| passkey.id == id) {
        return Err(DeleteError::NotFound);
    }
    let other_passkey = mine.len() > 1;
    if !other_passkey
        && !local_logins::has_password(&mut *tx, scope, user_id).await?
        && !identities::has_enabled_identity(&mut *tx, scope, user_id).await?
    {
        return Err(DeleteError::LastSignInMethod);
    }
    passkeys::delete(&mut *tx, scope, user_id, id, now, actor).await?;
    tx.commit().await?;
    Ok(())
}
