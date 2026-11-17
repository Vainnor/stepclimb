// SPDX-License-Identifier: AGPL-3.0-or-later
//! The plans and `org_plans` repositories against a fresh database per test
//! (docs/specs/m0/10-entitlements-framework.md B3 to B7; T2, T3, T4).
//! Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    feature::{self, Feature, Limit, TestSeal},
    id::{Uuid, new_id},
    org_plans::{self, Error, PlanSource},
    organizations,
    plans::{self, UNLIMITED, UnknownKeys},
};

/// A feature defined only in this test crate, so not in `feature::ALL`.
struct TestOnly;

impl TestSeal for TestOnly {}

impl Feature for TestOnly {
    const KEY: &'static str = "test_only_feature";
}

/// 2025-11-13T02:13:20.123456Z.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn later_time() -> SystemTime {
    request_time() + Duration::from_secs(60)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers are not under test; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization row alone, as a database from before the plans migration holds it.
async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

/// A plan row written by SQL, as M0 tests do (no API changes a plan).
async fn insert_plan(pool: &PgPool, key: &str, features: &str, limits: &str) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3::jsonb, $4::jsonb)",
    )
    .bind(id)
    .bind(key)
    .bind(features)
    .bind(limits)
    .execute(pool)
    .await
    .expect("insert plan");
    id
}

/// Point an organization's `org_plans` row at another plan by SQL.
async fn move_to_plan(pool: &PgPool, org: Uuid, plan: Uuid) {
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("move organization to plan");
}

/// `(features, limits, updated_at in microseconds)` of the `unlimited` row.
async fn unlimited_row(pool: &PgPool) -> (serde_json::Value, serde_json::Value, i64) {
    let (features, limits, updated): (String, String, i64) = sqlx::query_as(
        "SELECT features::text, limits::text, (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM plans WHERE key = 'unlimited'",
    )
    .fetch_one(pool)
    .await
    .expect("read the unlimited plan");
    (
        serde_json::from_str(&features).expect("features JSON"),
        serde_json::from_str(&limits).expect("limits JSON"),
        updated,
    )
}

/// `(org_id, plan key, source)` of every `org_plans` row.
async fn org_plan_rows(pool: &PgPool) -> Vec<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT op.org_id, p.key, op.source FROM org_plans op JOIN plans p ON p.id = op.plan_id
         ORDER BY op.org_id",
    )
    .fetch_all(pool)
    .await
    .expect("read org_plans")
}

fn catalog_features() -> serde_json::Value {
    let map: serde_json::Map<_, _> = feature::ALL
        .iter()
        .map(|k| ((*k).to_owned(), serde_json::Value::Bool(true)))
        .collect();
    serde_json::Value::Object(map)
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn sync_writes_the_unlimited_plan_from_the_catalogs(pool: PgPool) {
    let id = plans::sync_unlimited(&pool, request_time()).await.unwrap();

    let (features, limits, updated) = unlimited_row(&pool).await;
    assert_eq!(features, catalog_features());
    assert_eq!(
        limits,
        serde_json::json!({
            "users": null,
            "storage": null,
            "video_storage": null,
            "transcode_minutes": null
        })
    );
    assert_eq!(updated, 1_763_000_000_123_456);
    assert_eq!(id.get_version_num(), 7);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_second_sync_keeps_the_id_and_touches_nothing_unchanged(pool: PgPool) {
    let first = plans::sync_unlimited(&pool, request_time()).await.unwrap();
    let second = plans::sync_unlimited(&pool, later_time()).await.unwrap();

    assert_eq!(first, second);
    assert_eq!(unlimited_row(&pool).await.2, 1_763_000_000_123_456);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sync_rewrites_an_edited_unlimited_row(pool: PgPool) {
    let id = plans::sync_unlimited(&pool, request_time()).await.unwrap();
    sqlx::query(
        "UPDATE plans SET features = '{\"custom_roles\": false, \"retired\": true}',
                          limits = '{\"users\": 3}'
         WHERE key = 'unlimited'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let again = plans::sync_unlimited(&pool, later_time()).await.unwrap();

    let (features, limits, updated) = unlimited_row(&pool).await;
    assert_eq!(again, id);
    assert_eq!(features, catalog_features());
    assert_eq!(
        limits,
        serde_json::json!({
            "users": null,
            "storage": null,
            "video_storage": null,
            "transcode_minutes": null
        })
    );
    assert_eq!(updated, 1_763_000_060_123_456);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_gives_the_organization_the_unlimited_plan_in_its_transaction(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();

    assert_eq!(
        org_plan_rows(&pool).await,
        [(org.id, UNLIMITED.to_owned(), "selfhost".to_owned())]
    );
    let plan = org_plans::get_with_plan(&pool, scope(org.id))
        .await
        .unwrap();
    assert_eq!(plan.plan, UNLIMITED);
    assert_eq!(plan.source, PlanSource::Selfhost);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_failed_create_leaves_no_plan_row(pool: PgPool) {
    organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();

    let again = organizations::create(&pool, "org-a", "Other", request_time()).await;

    assert!(again.is_err());
    assert_eq!(org_plan_rows(&pool).await.len(), 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_rolls_back_when_the_plan_row_cannot_be_written(pool: PgPool) {
    // Refuse every org_plans insert, so the last step of create fails.
    sqlx::query(
        "ALTER TABLE org_plans ADD CONSTRAINT test_refuse CHECK (source = 'none') NOT VALID",
    )
    .execute(&pool)
    .await
    .unwrap();

    let created = organizations::create(&pool, "org-a", "Org A", request_time()).await;

    assert!(
        matches!(created, Err(organizations::Error::OrgPlan(_))),
        "{created:?}"
    );
    assert_eq!(organizations::count(&pool).await.unwrap(), 0);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ensure_selfhost_gives_an_organization_without_a_plan_the_unlimited_one(pool: PgPool) {
    plans::sync_unlimited(&pool, request_time()).await.unwrap();
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;

    let inserted = org_plans::ensure_selfhost(&pool, scope(a), request_time())
        .await
        .unwrap();

    assert!(inserted);
    assert_eq!(
        org_plan_rows(&pool).await,
        [(a, UNLIMITED.to_owned(), "selfhost".to_owned())],
        "org B, outside the scope, is untouched"
    );
    let missing = org_plans::get_with_plan(&pool, scope(b)).await;
    assert!(matches!(missing, Err(Error::NotFound)), "{missing:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ensure_selfhost_leaves_an_existing_row_unchanged(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let test_plan = insert_plan(&pool, "test_plan", "{}", "{}").await;
    move_to_plan(&pool, org.id, test_plan).await;

    let inserted = org_plans::ensure_selfhost(&pool, scope(org.id), later_time())
        .await
        .unwrap();

    assert!(!inserted);
    assert_eq!(
        org_plan_rows(&pool).await,
        [(org.id, "test_plan".to_owned(), "grant".to_owned())]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ensure_selfhost_refuses_before_the_unlimited_plan_exists(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;

    let ensured = org_plans::ensure_selfhost(&pool, scope(org), request_time()).await;

    assert!(
        matches!(ensured, Err(Error::UnlimitedMissing)),
        "{ensured:?}"
    );
    assert!(org_plan_rows(&pool).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_unlimited_plan_enables_every_feature_whatever_its_row_says(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE plans SET features = '{\"custom_roles\": false}', limits = '{\"users\": 0}'
         WHERE key = 'unlimited'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let plan = org_plans::get_with_plan(&pool, scope(org.id))
        .await
        .unwrap()
        .entitlements;

    assert!(plan.has::<feature::CustomRoles>());
    assert!(plan.has::<TestOnly>());
    for key in feature::ALL {
        assert!(plan.has_key(key), "{key}");
    }
    assert_eq!(plan.limit(Limit::Users), None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_empty_test_plan_disables_every_feature_and_allows_no_user(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let test_plan = insert_plan(&pool, "test_plan", "{}", "{}").await;
    move_to_plan(&pool, org.id, test_plan).await;

    let plan = org_plans::get_with_plan(&pool, scope(org.id))
        .await
        .unwrap();

    assert_eq!(plan.plan, "test_plan");
    assert_eq!(plan.source, PlanSource::Grant);
    assert!(!plan.entitlements.has::<TestOnly>());
    for key in feature::ALL {
        assert!(!plan.entitlements.has_key(key), "{key}");
    }
    assert_eq!(plan.entitlements.limit(Limit::Users), Some(0));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_test_plan_reads_its_listed_keys_and_ignores_unknown_ones(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let test_plan = insert_plan(
        &pool,
        "test_plan",
        r#"{"custom_roles": true, "test_only_feature": true, "retired": true}"#,
        r#"{"users": 2, "retired_limit": 9}"#,
    )
    .await;
    move_to_plan(&pool, org.id, test_plan).await;

    let plan = org_plans::get_with_plan(&pool, scope(org.id))
        .await
        .unwrap()
        .entitlements;

    assert!(plan.has::<feature::CustomRoles>());
    assert!(plan.has::<TestOnly>());
    assert!(!plan.has::<feature::ApiKeys>());
    assert!(!plan.has_key("retired_limit"));
    assert_eq!(plan.limit(Limit::Users), Some(2));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_with_plan_reads_the_scopes_organization_only(pool: PgPool) {
    let a = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let b = organizations::create(&pool, "org-b", "Org B", request_time())
        .await
        .unwrap();
    let test_plan = insert_plan(&pool, "test_plan", "{}", "{}").await;
    move_to_plan(&pool, b.id, test_plan).await;

    let plan_a = org_plans::get_with_plan(&pool, scope(a.id)).await.unwrap();
    let plan_b = org_plans::get_with_plan(&pool, scope(b.id)).await.unwrap();

    assert_eq!(plan_a.plan, UNLIMITED);
    assert_eq!(plan_b.plan, "test_plan");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unknown_keys_lists_other_plans_keys_the_catalogs_lack(pool: PgPool) {
    plans::sync_unlimited(&pool, request_time()).await.unwrap();
    sqlx::query("UPDATE plans SET features = '{\"retired\": true}' WHERE key = 'unlimited'")
        .execute(&pool)
        .await
        .unwrap();
    insert_plan(
        &pool,
        "b_plan",
        r#"{"custom_roles": true, "zeta": false, "alpha": true}"#,
        r#"{"users": 1, "seats": 4}"#,
    )
    .await;
    insert_plan(
        &pool,
        "a_plan",
        r#"{"api_keys": true}"#,
        r#"{"users": null}"#,
    )
    .await;
    insert_plan(&pool, "c_plan", "{}", r#"{"api_rate": 10}"#).await;

    let unknown = plans::unknown_keys(&pool).await.unwrap();

    assert_eq!(
        unknown,
        [
            UnknownKeys {
                plan: "b_plan".to_owned(),
                features: vec!["alpha".to_owned(), "zeta".to_owned()],
                limits: vec!["seats".to_owned()],
            },
            UnknownKeys {
                plan: "c_plan".to_owned(),
                features: vec![],
                limits: vec!["api_rate".to_owned()],
            },
        ]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_database_refuses_a_second_plan_row_and_an_unknown_source(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let test_plan = insert_plan(&pool, "test_plan", "{}", "{}").await;

    let second =
        sqlx::query("INSERT INTO org_plans (org_id, plan_id, source) VALUES ($1, $2, 'grant')")
            .bind(org.id)
            .bind(test_plan)
            .execute(&pool)
            .await;
    let bad_source = sqlx::query("UPDATE org_plans SET source = 'trial' WHERE org_id = $1")
        .bind(org.id)
        .execute(&pool)
        .await;
    let not_object = sqlx::query("UPDATE plans SET features = '[]' WHERE key = 'test_plan'")
        .execute(&pool)
        .await;

    assert!(second.is_err(), "one row per organization");
    assert!(bad_source.is_err(), "source is checked");
    assert!(not_object.is_err(), "features must be an object");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_inserts_a_missing_unlimited_plan_but_never_rewrites_one(pool: PgPool) {
    organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let (features, limits, created) = unlimited_row(&pool).await;
    assert_eq!(features, catalog_features());
    assert_eq!(
        limits,
        serde_json::json!({
            "users": null,
            "storage": null,
            "video_storage": null,
            "transcode_minutes": null
        })
    );
    assert_eq!(created, 1_763_000_000_123_456);
    sqlx::query("UPDATE plans SET features = '{\"retired\": true}' WHERE key = 'unlimited'")
        .execute(&pool)
        .await
        .unwrap();

    organizations::create(&pool, "org-b", "Org B", later_time())
        .await
        .unwrap();

    let (features, _, updated) = unlimited_row(&pool).await;
    assert_eq!(features, serde_json::json!({ "retired": true }));
    assert_eq!(updated, 1_763_000_000_123_456);
}

/// A create whose transaction is still open does not hold the `unlimited` row: a second create
/// on another connection finishes without waiting for it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_open_create_does_not_block_another(pool: PgPool) {
    plans::sync_unlimited(&pool, request_time()).await.unwrap();
    let mut open = pool.begin().await.unwrap();
    organizations::create(&mut *open, "org-a", "Org A", request_time())
        .await
        .unwrap();

    let second = tokio::time::timeout(
        Duration::from_secs(5),
        organizations::create(&pool, "org-b", "Org B", request_time()),
    )
    .await;

    assert!(
        matches!(second, Ok(Ok(_))),
        "the second create waited on the first: {second:?}"
    );
    open.rollback().await.unwrap();
}
