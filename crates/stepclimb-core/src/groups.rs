// SPDX-License-Identifier: AGPL-3.0-or-later
//! Groups and group memberships (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7).
//!
//! In M0 memberships come from sign-in only. [`replace_memberships`] makes the memberships one
//! [`Giver`] gave a user exactly the given group keys: it creates a group the organization does
//! not have yet (named by its key, recording the source that created it), adds the memberships
//! that are missing and removes that giver's memberships the keys no longer name. A giver is
//! the admission webhook, or one identity provider's claim (source `idp`, recorded with the
//! provider), so a membership another giver made, another provider's included, is never
//! touched: a group two givers return is kept until both stop returning it.
//!
//! An organization has at most [`MAX_GROUPS`] groups. A key naming no group once it is full
//! makes no group and no membership, and is counted in [`Replaced::dropped`].
//!
//! Keys are matched exactly. A key must be [usable](usable_key); the caller decides what to do
//! with one that is not, since the sources differ (a provider's claim, the webhook's reply).
//! Every query filters by the scope's organization.
//!
//! Memberships derived at sign-in write no audit entry, as the identity's attributes do not
//! (B7): they are a copy of what the provider or webhook said, replaced on every sign-in.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    id::new_id,
    users::is_format_char,
};

/// The longest group key, in characters.
pub const MAX_KEY_CHARS: usize = 255;

/// The most group keys one source may give one sign-in.
pub const MAX_KEYS: usize = 1000;

/// The most groups one organization may have.
pub const MAX_GROUPS: usize = 10_000;

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
/// character (NUL included), a format character (general category Cf, such as U+200B or a
/// bidirectional control) or the line or paragraph separator (U+2028, U+2029), as the `groups`
/// table's check says. Each would let two keys that look the same name two groups.
#[must_use]
pub fn usable_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().count() <= MAX_KEY_CHARS
        && !key
            .chars()
            .any(|c| c.is_control() || is_format_char(c) || c == '\u{2028}' || c == '\u{2029}')
}

/// Who gives a user memberships at sign-in: a provider's group claim (source `idp`, recorded
/// with the provider), or the admission webhook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Giver {
    /// The claim of the identity provider with this id.
    Provider(Uuid),
    /// The admission webhook's reply.
    AdmissionWebhook,
}

impl Giver {
    /// The source its memberships, and the groups it creates, record.
    #[must_use]
    pub fn source(self) -> GroupSource {
        match self {
            Self::Provider(_) => GroupSource::Idp,
            Self::AdmissionWebhook => GroupSource::AdmissionWebhook,
        }
    }

    fn provider_id(self) -> Option<Uuid> {
        match self {
            Self::Provider(id) => Some(id),
            Self::AdmissionWebhook => None,
        }
    }
}

/// What [`replace_memberships`] changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    /// Memberships added.
    pub added: u64,
    /// Memberships removed.
    pub removed: u64,
    /// Keys of groups the organization lacks that were not created, because it has
    /// [`MAX_GROUPS`]; no membership was made for them.
    pub dropped: u64,
}

/// Why a groups repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A key is not [usable](usable_key); nothing was written.
    #[error("a group key is empty, too long or holds a control, format or separator character")]
    UnusableKey,
    /// More than [`MAX_KEYS`] distinct keys; nothing was written.
    #[error("more than {MAX_KEYS} group keys")]
    TooManyKeys,
    /// The scope's organization does not exist.
    #[error("the organization does not exist")]
    OrgNotFound,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Make `user_id`'s memberships from `giver` in the scope's organization exactly `keys`, at
/// `now` (module documentation). Duplicate keys count once. Runs in one transaction (a savepoint
/// when `db` is already one). The user and the provider must be the organization's: the
/// foreign keys refuse another organization's.
pub async fn replace_memberships<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    giver: Giver,
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
           AND m.provider_id IS NOT DISTINCT FROM $4
           AND g.org_id = m.org_id AND g.id = m.group_id AND g.key <> ALL($5::text[])",
        org_id,
        user_id,
        giver.source().as_str(),
        giver.provider_id(),
        &keys,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let (added, dropped) = if keys.is_empty() {
        (0, 0)
    } else {
        let dropped = create_missing(&mut tx, org_id, giver.source(), &keys, now).await?;
        let added = add_missing(&mut tx, org_id, user_id, giver, &keys, now).await?;
        (added, dropped)
    };
    tx.commit().await?;
    Ok(Replaced {
        added,
        removed,
        dropped,
    })
}

/// The keys of `keys`, sorted, that name no group of the organization.
async fn missing(
    conn: &mut PgConnection,
    org_id: Uuid,
    keys: &[String],
) -> Result<Vec<String>, Error> {
    let missing = sqlx::query_scalar!(
        r#"SELECT k AS "key!" FROM unnest($2::text[]) AS k
           WHERE NOT EXISTS (SELECT 1 FROM groups g WHERE g.org_id = $1 AND g.key = k)
           ORDER BY k"#,
        org_id,
        keys,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(missing)
}

/// Create the groups of `keys` the organization does not have, named by their keys, up to
/// [`MAX_GROUPS`] groups in the organization; how many were not created. The count and the
/// inserts run under the organization row's lock (`FOR NO KEY UPDATE`), so two sign-ins cannot
/// both take the last places. Keys past the cap are left out in key order.
async fn create_missing(
    conn: &mut PgConnection,
    org_id: Uuid,
    source: GroupSource,
    keys: &[String],
    now: i64,
) -> Result<u64, Error> {
    if missing(&mut *conn, org_id, keys).await?.is_empty() {
        return Ok(0);
    }
    sqlx::query_scalar!(
        "SELECT id FROM organizations WHERE id = $1 FOR NO KEY UPDATE",
        org_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::OrgNotFound)?;
    let mut missing = missing(&mut *conn, org_id, keys).await?;
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM groups WHERE org_id = $1"#,
        org_id,
    )
    .fetch_one(&mut *conn)
    .await?;
    let room = usize::try_from(count).map_or(0, |count| MAX_GROUPS.saturating_sub(count));
    let dropped = missing.len().saturating_sub(room);
    missing.truncate(room);
    let ids: Vec<Uuid> = missing.iter().map(|_| new_id()).collect();
    sqlx::query!(
        "INSERT INTO groups (id, org_id, key, name, source, created_at, updated_at)
         SELECT new.id, $1, new.key, new.key, $4,
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         FROM unnest($2::uuid[], $3::text[]) AS new (id, key)",
        org_id,
        &ids,
        &missing,
        source.as_str(),
        now,
    )
    .execute(&mut *conn)
    .await?;
    Ok(dropped as u64)
}

/// Add the memberships of `keys` from `giver` that `user_id` does not have, for the keys that
/// name a group; how many.
async fn add_missing(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
    giver: Giver,
    keys: &[String],
    now: i64,
) -> Result<u64, Error> {
    let added = sqlx::query!(
        "INSERT INTO group_members (org_id, group_id, user_id, source, provider_id, created_at,
                                    updated_at)
         SELECT g.org_id, g.id, $2, $3, $4,
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         FROM groups g
         WHERE g.org_id = $1 AND g.key = ANY($5::text[])
         ON CONFLICT DO NOTHING",
        org_id,
        user_id,
        giver.source().as_str(),
        giver.provider_id(),
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
    fn a_usable_key_is_short_and_has_no_control_format_or_separator_character() {
        for key in [
            "staff",
            "home",
            "a b",
            "Gruppe-ä",
            "a\u{2027}b",
            "a\u{202f}b",
            &"k".repeat(MAX_KEY_CHARS),
        ] {
            assert!(usable_key(key), "{key:?}");
        }
        let long = "k".repeat(MAX_KEY_CHARS + 1);
        for key in [
            "",
            "a\u{0}b",
            "a\nb",
            "\t",
            "a\u{7f}",
            "a\u{85}",
            "st\u{200b}aff",
            "staff\u{200d}",
            "\u{202e}ffats",
            "\u{feff}staff",
            "staff\u{ad}",
            "a\u{e0041}",
            "a\u{2028}b",
            "a\u{2029}b",
            long.as_str(),
        ] {
            assert!(!usable_key(key), "{key:?}");
        }
    }
}
