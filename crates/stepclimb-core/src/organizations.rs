// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organizations repository (spec 02 B7, B13, B14).
//!
//! An organization is one row in `organizations`, the tenant every other row is scoped to. The
//! table is the scope itself, so its queries filter by `id = scope.org_id()` where every other
//! table's filter by `org_id`. `branding` belongs to spec 12 and is neither read nor written
//! here.

use std::time::SystemTime;

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    settings::{OrgSettings, OrgSettingsPatch},
};

/// The longest organization name, in characters (Unicode scalar values; spec 02 Q6).
pub const MAX_NAME_CHARS: usize = 200;

/// An organization as its own principals see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Organization {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
}

/// Why an organizations repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    NotFound,
    /// The name is empty once trimmed.
    #[error("an organization name cannot be empty")]
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`] once trimmed.
    #[error("an organization name is longer than {MAX_NAME_CHARS} characters")]
    NameTooLong,
    /// The stored settings object does not parse as [`OrgSettings`].
    #[error("the stored organization settings do not parse")]
    StoredSettings(#[source] serde_json::Error),
    /// The update time cannot be stored.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The scope's organization.
pub async fn get(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<Organization, Error> {
    sqlx::query_as!(
        Organization,
        "SELECT id, slug, name FROM organizations WHERE id = $1",
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)
}

/// Rename the scope's organization to `name`, trimmed, and set `updated_at` to `now`, the
/// request's clock reading. The slug never changes (spec 02 Q3).
pub async fn rename(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    name: &str,
    now: SystemTime,
) -> Result<Organization, Error> {
    let name = valid_name(name)?;
    let now = epoch_micros(now)?;
    sqlx::query_as!(
        Organization,
        "UPDATE organizations
         SET name = $2, updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE id = $1
         RETURNING id, slug, name",
        scope.org_id(),
        name,
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)
}

/// The scope's organization settings, every key with its effective value.
pub async fn settings(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<OrgSettings, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT settings::text AS "settings!" FROM organizations WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    parse_settings(&stored)
}

/// Merge `patch` into the scope's organization settings and set `updated_at` to `now`. The
/// merge (`settings || patch`) runs inside the `UPDATE` against the row it locks, never against
/// a copy read earlier, so a patch cannot write back a stale value for a key it does not name.
/// Returns the settings after the merge.
pub async fn update_settings(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    patch: &OrgSettingsPatch,
    now: SystemTime,
) -> Result<OrgSettings, Error> {
    // Invariant: the patch is a struct of plain, string-keyed fields, which serde_json always
    // serializes; it fails only on a map with non-string keys or a failing `Serialize` impl.
    let patch = serde_json::to_string(patch).expect("a settings patch serializes to JSON");
    let now = epoch_micros(now)?;
    let stored = sqlx::query_scalar!(
        r#"UPDATE organizations
           SET settings = settings || $2::text::jsonb,
               updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
           WHERE id = $1
           RETURNING settings::text AS "settings!""#,
        scope.org_id(),
        patch,
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    parse_settings(&stored)
}

/// How many organizations the instance has, for the request-organization resolver of
/// unauthenticated routes (spec 02 B11).
pub async fn count(db: impl PgExecutor<'_>) -> Result<i64, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: the resolver needs the count to
    // decide which organization, if any, a request without a credential belongs to. It returns
    // a number, never another organization's row.
    let count = sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM organizations"#)
        .fetch_one(db)
        .await?;
    Ok(count)
}

fn valid_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::NameEmpty);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::NameTooLong);
    }
    Ok(name)
}

fn parse_settings(stored: &str) -> Result<OrgSettings, Error> {
    serde_json::from_str(stored).map_err(Error::StoredSettings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed() {
        assert_eq!(valid_name("  Tower Cab \n").unwrap(), "Tower Cab");
    }

    #[test]
    fn an_empty_or_blank_name_is_refused() {
        for name in ["", "   ", "\t\n"] {
            assert!(
                matches!(valid_name(name), Err(Error::NameEmpty)),
                "{name:?}"
            );
        }
    }

    #[test]
    fn the_length_limit_counts_characters_after_trimming() {
        let at_limit = "é".repeat(MAX_NAME_CHARS);
        assert_eq!(valid_name(&format!(" {at_limit} ")).unwrap(), at_limit);
        let over = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(valid_name(&over), Err(Error::NameTooLong)));
    }
}
