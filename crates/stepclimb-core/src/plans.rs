// SPDX-License-Identifier: AGPL-3.0-or-later
//! Plans: the `plans` repository and how a plan row reads as entitlements
//! (docs/specs/m0/10-entitlements-framework.md B3, B4, B7).
//!
//! A plan is an instance-wide row, not organization data, so the queries here take no
//! `OrgScope`; each says so beside it. An organization's plan is read through
//! [`crate::org_plans`], which does take one.
//!
//! The `unlimited` plan is decided in code: [`PlanEntitlements`] for it enables every feature,
//! also one only a test crate defines, and sets no limit, whatever its row holds (B4).
//! [`sync_unlimited`] writes the row from the catalogs so SQL readers see them. For any other
//! plan the row decides, and a key it lacks fails closed (B7).

use std::time::SystemTime;

use serde_json::{Map, Value};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    db::{TimeOutOfRange, epoch_micros},
    feature::{self, Feature, Limit},
    id::new_id,
};

/// The key of the plan every self-hosted organization runs on.
pub const UNLIMITED: &str = "unlimited";

/// Why a plans repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A stored `features` or `limits` value is not a JSON object.
    #[error("the stored plan {plan:?} does not parse")]
    StoredPlan {
        plan: String,
        #[source]
        source: serde_json::Error,
    },
    /// A time that cannot be stored.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// What a plan allows: which features are on and what each limit is.
///
/// Built only by this crate's repository functions, from a plan row.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanEntitlements(Rules);

#[derive(Clone, Debug, PartialEq)]
enum Rules {
    /// The `unlimited` plan: every feature on, no limit (B4).
    Unlimited,
    /// Any other plan, as its row stores it.
    Listed {
        features: Map<String, Value>,
        limits: Map<String, Value>,
    },
}

impl PlanEntitlements {
    /// The entitlements of the plan `key`, whose row stores `features` and `limits` as JSON
    /// text. The `unlimited` plan's stored values are not read.
    pub(crate) fn from_row(key: &str, features: &str, limits: &str) -> Result<Self, Error> {
        if key == UNLIMITED {
            return Ok(Self(Rules::Unlimited));
        }
        let parse = |text: &str| {
            serde_json::from_str(text).map_err(|source| Error::StoredPlan {
                plan: key.to_owned(),
                source,
            })
        };
        Ok(Self(Rules::Listed {
            features: parse(features)?,
            limits: parse(limits)?,
        }))
    }

    /// Whether the plan includes the feature `F`.
    #[must_use]
    pub fn has<F: Feature>(&self) -> bool {
        self.has_key(F::KEY)
    }

    /// Whether the plan includes the feature with this key. On a plan other than `unlimited`
    /// only a stored `true` enables it: a missing key, `false` or any other value disables it
    /// (B7).
    #[must_use]
    pub fn has_key(&self, key: &str) -> bool {
        match &self.0 {
            Rules::Unlimited => true,
            Rules::Listed { features, .. } => features.get(key) == Some(&Value::Bool(true)),
        }
    }

    /// The plan's value for `limit`: `None` for no limit, else the most units allowed. On a
    /// plan other than `unlimited` a stored `null` is no limit and a non-negative integer is
    /// itself; a missing key or any other value is `Some(0)` (B7).
    #[must_use]
    pub fn limit(&self, limit: Limit) -> Option<u64> {
        match &self.0 {
            Rules::Unlimited => None,
            Rules::Listed { limits, .. } => match limits.get(limit.key()) {
                Some(Value::Null) => None,
                Some(value) => Some(value.as_u64().unwrap_or(0)),
                None => Some(0),
            },
        }
    }
}

/// Write the `unlimited` plan's row by key: every key of [`feature::ALL`] `true` and every
/// [`Limit`] `null`, inserting it on first run and rewriting `features` and `limits` where they
/// differ. `updated_at` moves to `now` only when it changes the row. Returns the plan's id,
/// which the first insert generates and later runs keep. Runs in one transaction (a savepoint
/// when `db` is already one).
///
/// The startup task of `stepclimb serve` calls this before serving (B4). Only startup rewrites
/// the row: `ON CONFLICT DO UPDATE` locks it until the transaction ends, even when nothing
/// differs, so [`crate::organizations::create`] uses [`insert_unlimited_if_missing`] instead.
pub async fn sync_unlimited<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    now: SystemTime,
) -> Result<Uuid, Error> {
    let now = epoch_micros(now)?;
    let (features, limits) = unlimited_values();
    let mut tx = db.begin().await?;
    // Instance-wide on purpose, so it takes no `OrgScope`: a plan belongs to no organization,
    // and the conflict target is the plan's unique key.
    sqlx::query!(
        "INSERT INTO plans (id, key, features, limits, created_at, updated_at)
         VALUES ($1, $2, $3::text::jsonb, $4::text::jsonb,
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
         ON CONFLICT (key) DO UPDATE
         SET features = EXCLUDED.features, limits = EXCLUDED.limits,
             updated_at = EXCLUDED.updated_at
         WHERE (plans.features, plans.limits)
               IS DISTINCT FROM (EXCLUDED.features, EXCLUDED.limits)",
        new_id(),
        UNLIMITED,
        features,
        limits,
        now,
    )
    .execute(&mut *tx)
    .await?;
    // Instance-wide on purpose, as above: reads the id of the row just written.
    let id = sqlx::query_scalar!("SELECT id FROM plans WHERE key = $1", UNLIMITED)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// Insert the `unlimited` row as [`sync_unlimited`] writes it when there is none, and return its
/// id. An existing row is neither changed nor locked (`ON CONFLICT DO NOTHING`), so organizations
/// created in concurrent transactions do not wait on each other, and an older binary creating an
/// organization never rewrites what a newer one wrote. For [`crate::organizations::create`],
/// so an organization created before `stepclimb serve` ever ran still gets its plan (B5).
pub(crate) async fn insert_unlimited_if_missing(
    conn: &mut PgConnection,
    now_micros: i64,
) -> Result<Uuid, Error> {
    let (features, limits) = unlimited_values();
    // Instance-wide on purpose, so it takes no `OrgScope`: a plan belongs to no organization.
    sqlx::query!(
        "INSERT INTO plans (id, key, features, limits, created_at, updated_at)
         VALUES ($1, $2, $3::text::jsonb, $4::text::jsonb,
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
         ON CONFLICT (key) DO NOTHING",
        new_id(),
        UNLIMITED,
        features,
        limits,
        now_micros,
    )
    .execute(&mut *conn)
    .await?;
    // Instance-wide on purpose, as above: reads the id of the row that now exists.
    let id = sqlx::query_scalar!("SELECT id FROM plans WHERE key = $1", UNLIMITED)
        .fetch_one(&mut *conn)
        .await?;
    Ok(id)
}

/// The `features` and `limits` objects of the `unlimited` row, as JSON text.
fn unlimited_values() -> (String, String) {
    let features: Map<String, Value> = feature::ALL
        .iter()
        .map(|key| ((*key).to_owned(), Value::Bool(true)))
        .collect();
    let limits: Map<String, Value> = Limit::ALL
        .iter()
        .map(|limit| (limit.key().to_owned(), Value::Null))
        .collect();
    (
        Value::Object(features).to_string(),
        Value::Object(limits).to_string(),
    )
}

/// The keys of one plan row that this binary does not use (B7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownKeys {
    /// The plan's key.
    pub plan: String,
    /// Keys of `features` that are not in [`feature::ALL`].
    pub features: Vec<String>,
    /// Keys of `limits` that are not a [`Limit`] key.
    pub limits: Vec<String>,
}

/// Every plan other than `unlimited` whose row holds a feature or limit key this binary's
/// catalogs lack, with those keys sorted, by plan key. Reading ignores such keys; the startup
/// task logs them once (B7).
pub async fn unknown_keys(db: impl PgExecutor<'_>) -> Result<Vec<UnknownKeys>, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: plans belong to no organization, and
    // the startup task checks every one. It returns key names, never an organization's data.
    let rows = sqlx::query!(
        r#"SELECT key,
                  ARRAY(SELECT jsonb_object_keys(features) ORDER BY 1) AS "features!",
                  ARRAY(SELECT jsonb_object_keys(limits) ORDER BY 1) AS "limits!"
           FROM plans WHERE key <> $1 ORDER BY key"#,
        UNLIMITED,
    )
    .fetch_all(db)
    .await?;
    let limit_keys: Vec<&str> = Limit::ALL.iter().map(|l| l.key()).collect();
    Ok(rows
        .into_iter()
        .map(|row| UnknownKeys {
            plan: row.key,
            features: unknown(row.features, feature::ALL),
            limits: unknown(row.limits, &limit_keys),
        })
        .filter(|row| !row.features.is_empty() || !row.limits.is_empty())
        .collect())
}

fn unknown(keys: Vec<String>, known: &[&str]) -> Vec<String> {
    keys.into_iter()
        .filter(|key| !known.contains(&key.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A feature outside the catalog, as a test crate defines one.
    struct Outside;

    impl feature::TestSeal for Outside {}

    impl Feature for Outside {
        const KEY: &'static str = "outside_feature";
    }

    fn listed(features: &str, limits: &str) -> PlanEntitlements {
        PlanEntitlements::from_row("test_plan", features, limits).unwrap()
    }

    #[test]
    fn the_unlimited_plan_enables_every_feature_and_sets_no_limit_whatever_its_row_says() {
        let plan =
            PlanEntitlements::from_row(UNLIMITED, r#"{"custom_roles": false}"#, r#"{"users": 0}"#)
                .unwrap();
        assert!(plan.has::<feature::CustomRoles>());
        assert!(plan.has::<Outside>());
        for key in feature::ALL {
            assert!(plan.has_key(key), "{key}");
        }
        assert_eq!(plan.limit(Limit::Users), None);
    }

    #[test]
    fn the_unlimited_rows_values_are_not_parsed() {
        assert!(PlanEntitlements::from_row(UNLIMITED, "not json", "[]").is_ok());
    }

    #[test]
    fn an_empty_plan_disables_every_feature_and_allows_no_unit() {
        let plan = listed("{}", "{}");
        assert!(!plan.has::<Outside>());
        for key in feature::ALL {
            assert!(!plan.has_key(key), "{key}");
        }
        assert_eq!(plan.limit(Limit::Users), Some(0));
    }

    #[test]
    fn only_a_stored_true_enables_a_feature() {
        let plan = listed(
            r#"{"custom_roles": true, "api_keys": false, "oidc_providers": "true",
                "custom_branding": 1, "admission_webhook": null, "outside_feature": true}"#,
            "{}",
        );
        assert!(plan.has::<feature::CustomRoles>());
        assert!(plan.has::<Outside>());
        assert!(!plan.has::<feature::ApiKeys>());
        assert!(!plan.has::<feature::OidcProviders>());
        assert!(!plan.has::<feature::CustomBranding>());
        assert!(!plan.has::<feature::AdmissionWebhook>());
        assert!(!plan.has::<feature::Oauth2Providers>());
    }

    #[test]
    fn a_limit_is_null_for_none_an_integer_for_itself_and_anything_else_zero() {
        let cases = [
            (r#"{"users": null}"#, None),
            (r#"{"users": 2}"#, Some(2)),
            (r#"{"users": 0}"#, Some(0)),
            (r#"{"users": -1}"#, Some(0)),
            (r#"{"users": 2.5}"#, Some(0)),
            (r#"{"users": "2"}"#, Some(0)),
            (r#"{"seats": null}"#, Some(0)),
        ];
        for (limits, expected) in cases {
            assert_eq!(
                listed("{}", limits).limit(Limit::Users),
                expected,
                "{limits}"
            );
        }
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let plan = listed(
            r#"{"retired_feature": true, "custom_roles": true}"#,
            r#"{"retired_limit": null, "users": 5}"#,
        );
        assert!(plan.has::<feature::CustomRoles>());
        assert!(!plan.has::<feature::ApiKeys>());
        assert_eq!(plan.limit(Limit::Users), Some(5));
    }

    #[test]
    fn a_stored_value_that_is_not_an_object_is_an_error_naming_the_plan() {
        for (features, limits) in [("[]", "{}"), ("{}", "null")] {
            let error = PlanEntitlements::from_row("test_plan", features, limits).unwrap_err();
            assert!(
                matches!(&error, Error::StoredPlan { plan, .. } if plan == "test_plan"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn the_unlimited_values_list_every_catalog_key() {
        let (features, limits) = unlimited_values();
        let features: Map<String, Value> = serde_json::from_str(&features).unwrap();
        let limits: Map<String, Value> = serde_json::from_str(&limits).unwrap();
        assert_eq!(features.len(), feature::ALL.len());
        for key in feature::ALL {
            assert_eq!(features.get(*key), Some(&Value::Bool(true)), "{key}");
        }
        assert_eq!(limits.len(), Limit::ALL.len());
        assert_eq!(limits.get("users"), Some(&Value::Null));
    }
}
