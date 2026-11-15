// SPDX-License-Identifier: AGPL-3.0-or-later
//! The passkeys repository (docs/specs/m0/04-local-accounts.md B8, Data, Q3).
//!
//! A passkey is one WebAuthn credential of a local account. The credential itself is opaque
//! here: `stepclimb_auth::passkey` verifies and serializes it, and this module stores the JSON
//! with the credential id beside it. Every query filters by `org_id = scope.org_id()`; the
//! composite foreign key keeps a passkey's user in its organization, and `(org_id,
//! credential_id)` is unique, so an authenticator credential is registered at most once in an
//! organization ([`Error::CredentialTaken`]).
//!
//! [`create`] writes the `passkey.create` audit entry (docs/specs/m0/09-permissions-and-roles.md
//! B22) with the user and the name, never the credential.

use std::time::SystemTime;

use serde_json::{Value, json};
use sqlx::{Acquire, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name::{self, Invalid},
};

/// The longest passkey name, in characters, as for other names.
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// A passkey to store.
#[derive(Clone, Copy)]
pub struct NewPasskey<'a> {
    /// A user of the scope's organization.
    pub user_id: Uuid,
    /// The id the authenticator chose for the credential.
    pub credential_id: &'a [u8],
    /// The credential as `stepclimb_auth::passkey` serialized it.
    pub credential: &'a Value,
    /// Trimmed before it is stored; see [`create`].
    pub name: &'a str,
}

/// Leaves the credential out: it is the user's public key and counters, which logs do not need.
impl std::fmt::Debug for NewPasskey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewPasskey")
            .field("user_id", &self.user_id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A stored passkey, without its credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passkey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub created_at: SystemTime,
}

/// Why a passkeys repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// A passkey of the organization already has this credential id.
    #[error("this passkey is already registered")]
    CredentialTaken,
    /// The name is empty or blank.
    #[error("the passkey name is empty")]
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`].
    #[error("the passkey name is longer than {MAX_NAME_CHARS} characters")]
    NameTooLong,
    /// The name contains NUL, which PostgreSQL `text` cannot store.
    #[error("the passkey name contains NUL")]
    NameContainsNul,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// `name` trimmed, or why it is refused: not empty, at most [`MAX_NAME_CHARS`] characters, no
/// NUL.
pub fn valid_name(name: &str) -> Result<&str, Error> {
    name::valid(name).map_err(|invalid| match invalid {
        Invalid::Empty => Error::NameEmpty,
        Invalid::TooLong => Error::NameTooLong,
        Invalid::ContainsNul => Error::NameContainsNul,
    })
}

/// Store `passkey` for its user in the scope's organization at `now`, with its name trimmed
/// ([`valid_name`]), and write its `passkey.create` audit entry by `actor`. Runs in one
/// transaction (a savepoint when `db` is already one).
///
/// Refuses a user the organization does not have ([`Error::UserNotFound`]) and a credential id
/// another passkey of the organization has ([`Error::CredentialTaken`]). The insert takes the
/// user from a `SELECT` filtered by the scope and skips a taken credential id, so neither
/// refusal aborts the caller's transaction.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    passkey: NewPasskey<'_>,
    now: SystemTime,
    actor: Actor,
) -> Result<Passkey, Error> {
    let name = valid_name(passkey.name)?;
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let user = sqlx::query_scalar!(
        "SELECT id FROM users WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        passkey.user_id,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if user.is_none() {
        return Err(Error::UserNotFound);
    }
    let id = new_id();
    let created_at_us = sqlx::query_scalar!(
        r#"INSERT INTO passkeys (id, org_id, user_id, credential_id, credential, name,
                                 created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5::text::jsonb, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond')
           ON CONFLICT (org_id, credential_id) DO NOTHING
           RETURNING (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!""#,
        id,
        scope.org_id(),
        passkey.user_id,
        passkey.credential_id,
        passkey.credential.to_string(),
        name,
        now_us,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::CredentialTaken)?;
    let data = json!({ "after": { "user_id": passkey.user_id, "name": name } });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::PasskeyCreate,
        id,
        &data,
        now_us,
    )
    .await?;
    tx.commit().await?;
    Ok(Passkey {
        id,
        user_id: passkey.user_id,
        name: name.to_owned(),
        created_at: from_epoch_micros(created_at_us)?,
    })
}

/// The credential ids of the passkeys of the user `user_id` in the scope's organization, oldest
/// first: what a new registration excludes, so one authenticator is not registered twice.
pub async fn credential_ids(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Vec<Vec<u8>>, Error> {
    let ids = sqlx::query_scalar!(
        "SELECT credential_id FROM passkeys
         WHERE org_id = $1 AND user_id = $2
         ORDER BY created_at, id",
        scope.org_id(),
        user_id,
    )
    .fetch_all(db)
    .await?;
    Ok(ids)
}

/// Whether the user `user_id` of the scope's organization has a passkey.
pub async fn exists_for_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<bool, Error> {
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM passkeys WHERE org_id = $1 AND user_id = $2
           ) AS "found!""#,
        scope.org_id(),
        user_id,
    )
    .fetch_one(db)
    .await?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_name_rule() {
        assert_eq!(valid_name("  Work laptop ").unwrap(), "Work laptop");
        assert!(matches!(valid_name(" "), Err(Error::NameEmpty)));
        assert!(matches!(valid_name("a\0b"), Err(Error::NameContainsNul)));
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(valid_name(&long), Err(Error::NameTooLong)));
    }

    #[test]
    fn debug_output_leaves_the_credential_out() {
        let credential = json!({ "cred": { "counter": 0, "secret-looking": "zq" } });
        let new = NewPasskey {
            user_id: Uuid::nil(),
            credential_id: &[1, 2, 3],
            credential: &credential,
            name: "Phone",
        };
        let printed = format!("{new:?}");
        assert!(!printed.contains("zq"), "{printed}");
        assert!(printed.contains("Phone"), "{printed}");
    }
}
