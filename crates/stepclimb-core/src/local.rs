// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local accounts' credentials (docs/specs/m0/04-local-accounts.md B3).
//!
//! [`has_credential`] says whether a user can sign in with a local credential: a passkey (issue
//! I3) or a password (issue I6). `identities::unlink_unless_last` counts it as a sign-in method
//! besides the user's identities at enabled providers (docs/specs/m0/03-users-and-identities.md
//! B19).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::{OrgScope, passkeys};

/// Why reading a user's local credentials failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Passkeys(#[from] passkeys::Error),
}

/// Whether the user `user_id` of the scope's organization has a passkey or a password.
///
/// Today that is a passkey ([`passkeys::exists_for_user`]); the password column arrives with
/// issue I6, which adds its check here. A credential counts whether or not its method is
/// switched on: a switched-off method's credentials work again once it is switched back on
/// (B2), so the user still has them.
pub async fn has_credential(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<bool, Error> {
    Ok(passkeys::exists_for_user(&mut *conn, scope, user_id).await?)
}
