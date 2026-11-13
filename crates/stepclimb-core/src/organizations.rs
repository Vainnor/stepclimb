// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organizations repository (spec 02 B7, B13, B14).
//!
//! An organization is one row in `organizations`, the tenant every other row is scoped to. The
//! table is the scope itself, so its queries filter by `id = scope.org_id()` where every other
//! table's filter by `org_id`. [`create`] is the one function that makes an organization, with
//! its root account (B26) and its `org_plans` row (spec 10 B5). `branding` belongs to spec 12
//! and is neither read nor written here.

use std::time::SystemTime;

use serde_json::{Map, Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, accounts,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    id::new_id,
    name, org_plans, plans,
    settings::{OrgSettings, OrgSettingsPatch},
};

/// The longest organization name, in characters (Unicode scalar values; spec 02 Q6).
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// The longest slug, in characters (spec 02 Q3: a DNS label of at most 32).
pub const MAX_SLUG_CHARS: usize = 32;

/// Slugs no organization may take, because a subdomain of that name serves the instance itself
/// (spec 02 Q3).
pub const RESERVED_SLUGS: [&str; 12] = [
    "www",
    "api",
    "admin",
    "auth",
    "oauth",
    "docs",
    "app",
    "static",
    "assets",
    "status",
    "mail",
    "stepclimb",
];

/// The unique constraint the initial migration put on `organizations.slug`.
const SLUG_CONSTRAINT: &str = "organizations_slug_key";

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
    /// The name holds NUL, which PostgreSQL `text` cannot store.
    #[error("an organization name cannot contain the NUL character")]
    NameContainsNul,
    /// The slug is not 1 to [`MAX_SLUG_CHARS`] of lowercase ASCII letters, digits and hyphens,
    /// starting and ending with a letter or digit.
    #[error(
        "an organization slug must be 1 to {MAX_SLUG_CHARS} lowercase letters, digits and inner hyphens"
    )]
    SlugInvalid,
    /// The slug is one of [`RESERVED_SLUGS`].
    #[error("the organization slug is reserved")]
    SlugReserved,
    /// Another organization of the instance has this slug.
    #[error("another organization has this slug")]
    SlugTaken,
    /// The stored settings object does not parse as [`OrgSettings`].
    #[error("the stored organization settings do not parse")]
    StoredSettings(#[source] serde_json::Error),
    /// Writing the `unlimited` plan failed.
    #[error("could not write the unlimited plan")]
    Plan(#[from] plans::Error),
    /// Giving the organization its plan failed.
    #[error("could not give the organization its plan")]
    OrgPlan(#[from] org_plans::Error),
    /// The update time cannot be stored.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(SLUG_CONSTRAINT) => Self::SlugTaken,
            _ => Self::Database(error),
        }
    }
}

/// Create an organization named `name`, trimmed, with the slug `slug`, its root account, named
/// after it (B16, B26), and its `org_plans` row, plan `unlimited` with source `selfhost` (spec
/// 10 B5), in one transaction (a savepoint when `db` is already one). It inserts the
/// `unlimited` plan first when it does not exist yet, so it works before `stepclimb serve` has
/// ever started; it never rewrites an existing one, which is startup's job. `created_at` and
/// `updated_at` of the new rows are `now`. It writes the `organization.create` audit entry, by
/// the instance, the only actor that creates organizations.
///
/// Only instance contexts call this: the `stepclimb admin` CLI in M0, the operator console in
/// M6 (B9, B26). It takes no `OrgScope` because the organization it acts in does not exist
/// until it runs. The slug is fixed after creation (Q3).
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    slug: &str,
    name: &str,
    now: SystemTime,
) -> Result<Organization, Error> {
    valid_slug(slug)?;
    let name = valid_name(name)?;
    let now_micros = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    // Instance-wide on purpose: this inserts the scope row itself, so there is no `org_id` to
    // filter by. The id is new, so the row is the only one it can touch.
    let organization = sqlx::query_as!(
        Organization,
        "INSERT INTO organizations (id, slug, name, created_at, updated_at)
         VALUES ($1, $2, $3,
                 timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $4::bigint * interval '1 microsecond')
         RETURNING id, slug, name",
        new_id(),
        slug,
        name,
        now_micros,
    )
    .fetch_one(&mut *tx)
    .await?;
    let scope = new_org_scope(organization.id);
    accounts::create_root(&mut tx, scope, name, now_micros).await?;
    plans::insert_unlimited_if_missing(&mut tx, now_micros).await?;
    org_plans::ensure_selfhost(&mut *tx, scope, now).await?;
    let data = json!({ "after": { "slug": organization.slug, "name": organization.name } });
    let (instance, create) = (Actor::Instance, Action::OrganizationCreate);
    let id = organization.id;
    audit::record(&mut tx, scope, instance, create, id, &data, now_micros).await?;
    tx.commit().await?;
    Ok(organization)
}

/// The scope of the organization [`create`] has just inserted, for writing its first rows.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope for creating an organization (B26), which no credential can belong to yet"
)]
fn new_org_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
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

/// Rename the scope's organization to `name`, trimmed, set `updated_at` to `now`, the request's
/// clock reading, and write the `organization.rename` audit entry by `actor` with the name
/// before and after. The slug never changes (spec 02 Q3). A name equal to the current one
/// writes nothing and returns the organization as it is (spec 09 B22).
///
/// Runs in one transaction (a savepoint when `db` is already one).
pub async fn rename<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    name: &str,
    now: SystemTime,
    actor: Actor,
) -> Result<Organization, Error> {
    let name = valid_name(name)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let before = sqlx::query_scalar!(
        "SELECT name FROM organizations WHERE id = $1 FOR NO KEY UPDATE",
        scope.org_id(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if before == name {
        return get(&mut *tx, scope).await;
    }
    let organization = sqlx::query_as!(
        Organization,
        "UPDATE organizations
         SET name = $2, updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE id = $1
         RETURNING id, slug, name",
        scope.org_id(),
        name,
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": { "name": before }, "after": { "name": organization.name } });
    let rename = Action::OrganizationRename;
    audit::record(&mut tx, scope, actor, rename, scope.org_id(), &data, now).await?;
    tx.commit().await?;
    Ok(organization)
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
/// a copy read earlier, so a patch cannot write back a stale value for a key it does not name,
/// and stored keys the patch does not name (including ones this release does not know) stay.
/// Returns the settings after the merge.
///
/// It writes the `organization.update_settings` audit entry by `actor` with the effective
/// value of each key the patch names, before and after. A patch that names no key, or only keys
/// whose effective value already equals the patched one, writes nothing and returns the
/// settings as they are (spec 09 B22).
///
/// The reads, the `UPDATE`, the parse of its result and the entry run in one transaction, a
/// savepoint when `db` is already one: if the merged object does not parse, the call returns
/// [`Error::StoredSettings`] and nothing it wrote is kept.
pub async fn update_settings<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    patch: &OrgSettingsPatch,
    now: SystemTime,
    actor: Actor,
) -> Result<OrgSettings, Error> {
    // Invariant: the patch is a struct of plain, string-keyed fields, which serde_json always
    // serializes; it fails only on a map with non-string keys or a failing `Serialize` impl.
    let patch = serde_json::to_value(patch).expect("a settings patch serializes to JSON");
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let before = sqlx::query_scalar!(
        r#"SELECT settings::text AS "settings!" FROM organizations WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let before = parse_settings(&before)?;
    if !settings_change(&patch, &before) {
        return Ok(before);
    }
    let stored = sqlx::query_scalar!(
        r#"UPDATE organizations
           SET settings = settings || $2::text::jsonb,
               updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
           WHERE id = $1
           RETURNING settings::text AS "settings!""#,
        scope.org_id(),
        patch.to_string(),
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    // An early return drops `tx`, which rolls the update back.
    let settings = parse_settings(&stored)?;
    let data = settings_data(&patch, &before, &settings);
    let update = Action::OrganizationUpdateSettings;
    audit::record(&mut tx, scope, actor, update, scope.org_id(), &data, now).await?;
    tx.commit().await?;
    Ok(settings)
}

/// Whether `patch` names a key whose value differs from its effective value in `before`.
fn settings_change(patch: &Value, before: &OrgSettings) -> bool {
    // Invariant: `OrgSettings` is a struct of plain, string-keyed fields (see above).
    let current = serde_json::to_value(before).expect("settings serialize to JSON");
    let mut named = patch.as_object().into_iter().flatten();
    named.any(|(key, value)| current.get(key) != Some(value))
}

/// The `organization.update_settings` entry's data: the effective value of each key `patch`
/// names, before and after. A setting that holds a secret must be left out here when one is
/// added: entries never hold secrets.
fn settings_data(patch: &Value, before: &OrgSettings, after: &OrgSettings) -> Value {
    // Invariant: `OrgSettings` is a struct of plain, string-keyed fields (see above).
    let pick = |settings: &OrgSettings| {
        let all = serde_json::to_value(settings).expect("settings serialize to JSON");
        let keys = patch.as_object().into_iter().flat_map(Map::keys);
        keys.map(|key| (key.clone(), all.get(key).cloned().unwrap_or(Value::Null)))
            .collect::<Map<_, _>>()
    };
    json!({ "before": pick(before), "after": pick(after) })
}

/// How many organizations the instance has.
pub async fn count(db: impl PgExecutor<'_>) -> Result<i64, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: the resolver needs the count to
    // decide which organization, if any, a request without a credential belongs to. It returns
    // a number, never another organization's row.
    let count = sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM organizations"#)
        .fetch_one(db)
        .await?;
    Ok(count)
}

/// Lock the `organizations` table against inserts until the end of the caller's transaction,
/// then count its rows, so the count holds until that transaction ends: `stepclimb admin create`
/// refuses on a non-zero count and creates the first organization in the same transaction
/// (spec 02 B27). `SHARE ROW EXCLUSIVE` conflicts with itself and with the lock an `INSERT`
/// takes, so a second caller waits here until the first commits, then counts its organization;
/// reads of the table are not blocked.
pub async fn lock_and_count(conn: &mut PgConnection) -> Result<i64, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: whether any organization exists is a
    // fact about the instance. It returns a number, never another organization's row.
    sqlx::query!("LOCK TABLE organizations IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *conn)
        .await?;
    count(&mut *conn).await
}

/// The id of every organization, for instance-wide jobs that then act in each organization
/// with an instance scope (the sign-in flow cleanup, spec 08 B21).
pub async fn ids(db: impl PgExecutor<'_>) -> Result<Vec<Uuid>, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: a job has no request to take an
    // organization from and must visit each one. It returns ids only, never a row's contents.
    let ids = sqlx::query_scalar!("SELECT id FROM organizations ORDER BY id")
        .fetch_all(db)
        .await?;
    Ok(ids)
}

/// How many organizations the instance has, as the request-organization resolver needs to
/// know it (spec 02 B11, spec 08 B5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tenancy {
    /// No organization yet (a fresh install).
    Empty,
    /// Exactly one organization, which every request belongs to.
    Single(Uuid),
    /// Two or more.
    Multiple,
}

/// Whether the instance has no organization, exactly one (and which), or several.
pub async fn tenancy(db: impl PgExecutor<'_>) -> Result<Tenancy, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: this decides which organization a
    // request belongs to before any scope exists. It returns an id only when the instance has
    // exactly one organization, which is then every request's organization.
    let ids = sqlx::query_scalar!("SELECT id FROM organizations ORDER BY id LIMIT 2")
        .fetch_all(db)
        .await?;
    Ok(match ids.as_slice() {
        [] => Tenancy::Empty,
        [only] => Tenancy::Single(*only),
        _ => Tenancy::Multiple,
    })
}

fn valid_name(name: &str) -> Result<&str, Error> {
    name::valid(name).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::NameEmpty,
        name::Invalid::TooLong => Error::NameTooLong,
        name::Invalid::ContainsNul => Error::NameContainsNul,
    })
}

/// Spec 02 Q3: `^[a-z0-9](?:[a-z0-9-]{0,30}[a-z0-9])?$`, and not reserved.
fn valid_slug(slug: &str) -> Result<(), Error> {
    let bytes = slug.as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let well_formed = (1..=MAX_SLUG_CHARS).contains(&bytes.len())
        && bytes.iter().all(|b| alnum(b) || *b == b'-')
        && bytes.first().is_some_and(alnum)
        && bytes.last().is_some_and(alnum);
    if !well_formed {
        return Err(Error::SlugInvalid);
    }
    if RESERVED_SLUGS.contains(&slug) {
        return Err(Error::SlugReserved);
    }
    Ok(())
}

fn parse_settings(stored: &str) -> Result<OrgSettings, Error> {
    serde_json::from_str(stored).map_err(Error::StoredSettings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_problems_map_to_organization_errors() {
        assert_eq!(valid_name("  Tower Cab ").unwrap(), "Tower Cab");
        assert!(matches!(valid_name(" "), Err(Error::NameEmpty)));
        assert!(matches!(valid_name("a\0"), Err(Error::NameContainsNul)));
        let over = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(valid_name(&over), Err(Error::NameTooLong)));
    }

    #[test]
    fn slugs_of_the_q3_format_are_accepted() {
        let longest = "a".repeat(MAX_SLUG_CHARS);
        for slug in [
            "a",
            "7",
            "vzdc",
            "org-a",
            "a-b-c",
            "a--b",
            "x1-2y",
            longest.as_str(),
        ] {
            assert!(valid_slug(slug).is_ok(), "{slug:?}");
        }
    }

    #[test]
    fn slugs_outside_the_q3_format_are_refused() {
        let too_long = "a".repeat(MAX_SLUG_CHARS + 1);
        for slug in [
            "",
            "-a",
            "a-",
            "-",
            "Org",
            "org_a",
            "org.a",
            "org a",
            " org",
            "orgé",
            "a\0",
            too_long.as_str(),
        ] {
            assert!(
                matches!(valid_slug(slug), Err(Error::SlugInvalid)),
                "{slug:?}"
            );
        }
    }

    #[test]
    fn reserved_slugs_are_refused() {
        for slug in RESERVED_SLUGS {
            assert!(
                matches!(valid_slug(slug), Err(Error::SlugReserved)),
                "{slug:?}"
            );
        }
        assert!(
            valid_slug("stepclimb-ops").is_ok(),
            "only the exact word is reserved"
        );
    }
}
