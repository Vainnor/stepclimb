// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local accounts' credentials (docs/specs/m0/04-local-accounts.md B3).
//!
//! [`has_credential`] says whether a user can sign in with a local credential: a passkey (issue
//! I3) or a password (issue I6). `identities::unlink_unless_last` counts it as a sign-in method
//! besides the user's identities at enabled providers (docs/specs/m0/03-users-and-identities.md
//! B19).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::OrgScope;

/// Why reading a user's local credentials failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Whether the user `user_id` of the scope's organization has a passkey or a password.
///
/// Neither credential can be stored yet: the `passkeys` table arrives with issue I3 and the
/// password column with issue I6, and each adds its check here with a test that the credential
/// keeps `identities::unlink_unless_last` from refusing. Until then no user has one, and this
/// answers `false` without a query.
pub async fn has_credential(
    _conn: &mut PgConnection,
    _scope: OrgScope,
    _user_id: Uuid,
) -> Result<bool, Error> {
    Ok(false)
}
