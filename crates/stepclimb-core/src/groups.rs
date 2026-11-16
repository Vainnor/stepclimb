// SPDX-License-Identifier: AGPL-3.0-or-later
//! Groups and group memberships (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7).
//!
//! In M0 memberships come from sign-in only. [`replace_memberships`] makes a user's memberships
//! from one [`GroupSource`] exactly the given group keys: it creates a group the organization
//! does not have yet (named by its key, recording the source that created it), adds the
//! memberships that are missing and removes that source's memberships the keys no longer name.
//! A membership from another source is never touched, so a group the claims and the webhook
//! both return is kept until both stop returning it.
//!
//! Keys are matched exactly. A key must be [usable](usable_key); the caller decides what to do
//! with one that is not, since the sources differ (a provider's claim, the webhook's reply).
//! Every query filters by the scope's organization.
//!
//! Memberships derived at sign-in write no audit entry, as the identity's attributes do not
//! (B7): they are a copy of what the provider or webhook said, replaced on every sign-in.

use std::time::SystemTime;

use sqlx::{Acquire, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    id::new_id,
};

/// The longest group key, in characters.
pub const MAX_KEY_CHARS: usize = 255;

/// The most group keys one source may give one sign-in.
pub const MAX_KEYS: usize = 1000;

/// Where a group or a membership came from: the five sources users have (spec 03 B4) and the
/// admission webhook (Q8 (a)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupSource {
    /// Made in Stepclimb, by an admin (M4).
    Local,
    /// From a sign-in's group claim.
    Idp,
    /// Written by the sync API (M4).
    Sync,
    /// Written by SCIM (M4).
    Scim,
    /// Written by a connector (M4).
    Connector,
    /// From the admission webhook's reply.
    AdmissionWebhook,
}

impl GroupSource {
    /// Every source, in the order the check constraints list them.
    pub const ALL: [Self; 6] = [
        Self::Local,
        Self::Idp,
        Self::Sync,
        Self::Scim,
        Self::Connector,
        Self::AdmissionWebhook,
    ];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Idp => "idp",
            Self::Sync => "sync",
            Self::Scim => "scim",
            Self::Connector => "connector",
            Self::AdmissionWebhook => "admission_webhook",
        }
    }

    /// The source a stored spelling names, or `None` for any other string.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == stored)
    }
}

/// Whether `key` can be a group key: 1 to [`MAX_KEY_CHARS`] characters, none of them a control
/// character (NUL included), as the `groups` table's check says.
#[must_use]
pub fn usable_key(key: &str) -> bool {
    !key.is_empty() && key.chars().count() <= MAX_KEY_CHARS && !key.chars().any(char::is_control)
}

/// What [`replace_memberships`] changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    /// Memberships added.
    pub added: u64,
    /// Memberships removed.
    pub removed: u64,
}

/// Why a groups repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A key is not [usable](usable_key); nothing was written.
    #[error("a group key is empty, too long or holds a control character")]
    UnusableKey,
    /// More than [`MAX_KEYS`] distinct keys; nothing was written.
    #[error("more than {MAX_KEYS} group keys")]
    TooManyKeys,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Make `user_id`'s memberships from `source` in the scope's organization exactly `keys`, at
/// `now` (module documentation). Duplicate keys count once. Runs in one transaction (a savepoint
/// when `db` is already one). The user must be the organization's: the foreign key refuses
/// another organization's user.
pub async fn replace_memberships<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    source: GroupSource,
    keys: &[String],
    now: SystemTime,
) -> Result<Replaced, Error> {
    let mut keys: Vec<String> = keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    if !keys.iter().all(|key| usable_key(key)) {
        return Err(Error::UnusableKey);
    }
    if keys.len() > MAX_KEYS {
        return Err(Error::TooManyKeys);
    }
    let now = epoch_micros(now)?;
    let org_id = scope.org_id();
    let mut tx = db.begin().await?;
    let removed = sqlx::query!(
        "DELETE FROM group_members m
         USING groups g
         WHERE m.org_id = $1 AND m.user_id = $2 AND m.source = $3
           AND g.org_id = m.org_id AND g.id = m.group_id AND g.key <> ALL($4::text[])",
        org_id,
        user_id,
        source.as_str(),
        &keys,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let added = if keys.is_empty() {
        0
    } else {
        create_missing(&mut tx, org_id, source, &keys, now).await?;
        add_missing(&mut tx, org_id, user_id, source, &keys, now).await?
    };
    tx.commit().await?;
    Ok(Replaced { added, removed })
}

/// Create the groups of `keys` the organization does not have, named by their keys.
async fn create_missing(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    source: GroupSource,
    keys: &[String],
    now: i64,
) -> Result<(), Error> {
    let ids: Vec<Uuid> = keys.iter().map(|_| new_id()).collect();
    sqlx::query!(
        "INSERT INTO groups (id, org_id, key, name, source, created_at, updated_at)
         SELECT new.id, $1, new.key, new.key, $4,
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         FROM unnest($2::uuid[], $3::text[]) AS new (id, key)
         ON CONFLICT (org_id, key) DO NOTHING",
        org_id,
        &ids,
        keys,
        source.as_str(),
        now,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Add the memberships of `keys` from `source` that `user_id` does not have; how many.
async fn add_missing(
    conn: &mut sqlx::PgConnection,
    org_id: Uuid,
    user_id: Uuid,
    source: GroupSource,
    keys: &[String],
    now: i64,
) -> Result<u64, Error> {
    let added = sqlx::query!(
        "INSERT INTO group_members (org_id, group_id, user_id, source, created_at, updated_at)
         SELECT g.org_id, g.id, $2, $3,
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         FROM groups g
         WHERE g.org_id = $1 AND g.key = ANY($4::text[])
         ON CONFLICT (group_id, user_id, source) DO NOTHING",
        org_id,
        user_id,
        source.as_str(),
        keys,
        now,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_round_trips_through_its_stored_spelling() {
        for source in GroupSource::ALL {
            assert_eq!(GroupSource::parse(source.as_str()), Some(source));
        }
        let spellings: Vec<&str> = GroupSource::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            spellings,
            [
                "local",
                "idp",
                "sync",
                "scim",
                "connector",
                "admission_webhook"
            ]
        );
        assert_eq!(GroupSource::parse("Idp"), None);
    }

    #[test]
    fn a_usable_key_is_short_and_has_no_control_character() {
        for key in [
            "staff",
            "home",
            "a b",
            "Gruppe-ä",
            &"k".repeat(MAX_KEY_CHARS),
        ] {
            assert!(usable_key(key), "{key:?}");
        }
        let long = "k".repeat(MAX_KEY_CHARS + 1);
        for key in ["", "a\u{0}b", "a\nb", "\t", "a\u{7f}", long.as_str()] {
            assert!(!usable_key(key), "{key:?}");
        }
    }
}
