// SPDX-License-Identifier: AGPL-3.0-or-later
//! The startup pass over plans (docs/specs/m0/10-entitlements-framework.md B4, B5; T2, T3):
//! `startup::prepare`, which `stepclimb serve` runs before it listens, writes the `unlimited`
//! row from the catalogs, rewrites it on a restart, and gives every organization without a plan
//! the `unlimited` one while leaving an existing row alone; it warns about plan keys this
//! release does not know (B7) and stops when it cannot give an organization its plan. Needs
//! `DATABASE_URL` (`just up`).

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    feature::{self, Feature, Limit, TestSeal},
    id::{Uuid, new_id},
    org_plans,
};
use stepclimb_server::startup;

/// A feature defined only in this test crate, so not in `feature::ALL`.
struct TestOnly;

impl TestSeal for TestOnly {}

impl Feature for TestOnly {
    const KEY: &'static str = "test_only_feature";
}

fn clock() -> ManualClock {
    ManualClock::new(UNIX_EPOCH + Duration::from_secs(1_763_000_000))
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers are not under test; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn unlimited_row(pool: &PgPool) -> (Uuid, serde_json::Value, serde_json::Value) {
    let (id, features, limits): (Uuid, String, String) = sqlx::query_as(
        "SELECT id, features::text, limits::text FROM plans WHERE key = 'unlimited'",
    )
    .fetch_one(pool)
    .await
    .expect("read the unlimited plan");
    (
        id,
        serde_json::from_str(&features).expect("features JSON"),
        serde_json::from_str(&limits).expect("limits JSON"),
    )
}

async fn org_plan_rows(pool: &PgPool) -> Vec<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT op.org_id, p.key, op.source FROM org_plans op JOIN plans p ON p.id = op.plan_id
         ORDER BY op.org_id",
    )
    .fetch_all(pool)
    .await
    .expect("read org_plans")
}

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

#[sqlx::test(migrations = false)]
async fn startup_writes_the_unlimited_plan_and_a_restart_rewrites_an_edited_row(pool: PgPool) {
    let clock = clock();
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    let (id, features, limits) = unlimited_row(&pool).await;
    let expected: serde_json::Map<_, _> = feature::ALL
        .iter()
        .map(|k| ((*k).to_owned(), serde_json::Value::Bool(true)))
        .collect();
    assert_eq!(features, serde_json::Value::Object(expected.clone()));
    assert_eq!(limits, serde_json::json!({ "users": null }));

    let org = insert_org(&pool, "org-a").await;
    clock.advance(Duration::from_secs(60));
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("second start");
    sqlx::query(
        "UPDATE plans SET features = '{\"custom_roles\": false}', limits = '{\"users\": 1}'
         WHERE key = 'unlimited'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let plan = org_plans::get_with_plan(&pool, scope(org))
        .await
        .expect("the organization has a plan")
        .entitlements;
    assert!(
        plan.has::<feature::CustomRoles>(),
        "the edited row does not disable it"
    );
    assert!(plan.has::<TestOnly>());
    assert_eq!(plan.limit(Limit::Users), None);

    clock.advance(Duration::from_secs(60));
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("restart");
    let (id_after, features_after, limits_after) = unlimited_row(&pool).await;
    assert_eq!(id_after, id, "the plan's id stays fixed");
    assert_eq!(features_after, serde_json::Value::Object(expected));
    assert_eq!(limits_after, serde_json::json!({ "users": null }));
}

#[sqlx::test(migrations = false)]
async fn startup_gives_organizations_without_a_plan_the_unlimited_one_and_keeps_others(
    pool: PgPool,
) {
    let clock = clock();
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let test_plan = new_id();
    sqlx::query("INSERT INTO plans (id, key) VALUES ($1, 'test_plan')")
        .bind(test_plan)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO org_plans (org_id, plan_id, source) VALUES ($1, $2, 'grant')")
        .bind(b)
        .bind(test_plan)
        .execute(&pool)
        .await
        .unwrap();

    startup::prepare(&pool, &clock.clock())
        .await
        .expect("restart");

    let mut expected = vec![
        (a, "unlimited".to_owned(), "selfhost".to_owned()),
        (b, "test_plan".to_owned(), "grant".to_owned()),
    ];
    expected.sort();
    assert_eq!(org_plan_rows(&pool).await, expected);
}

#[sqlx::test(migrations = false)]
async fn startup_stops_when_it_cannot_give_an_organization_its_plan(pool: PgPool) {
    let clock = clock();
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    let org = insert_org(&pool, "org-a").await;
    sqlx::query(
        "ALTER TABLE org_plans ADD CONSTRAINT test_refuse CHECK (source = 'none') NOT VALID",
    )
    .execute(&pool)
    .await
    .unwrap();

    let started = startup::prepare(&pool, &clock.clock()).await;

    let error = started.expect_err("startup refuses");
    assert!(
        matches!(error, startup::StartupError::OrgPlan { org_id, .. } if org_id == org),
        "{error:?}"
    );
    assert!(error.to_string().contains(&org.to_string()), "{error}");
    assert!(org_plan_rows(&pool).await.is_empty());
}

/// Log lines written while the guard returned by [`capture`] lives, on this thread.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("log buffer").clone()).expect("UTF-8 logs")
    }
}

fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

#[sqlx::test(migrations = false)]
async fn startup_warns_about_plan_keys_this_release_does_not_know(pool: PgPool) {
    let clock = clock();
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    sqlx::query(
        "INSERT INTO plans (id, key, features, limits)
         VALUES ($1, 'test_plan', '{\"custom_roles\": true, \"retired_feature\": true}',
                 '{\"users\": 2, \"retired_limit\": 1}')",
    )
    .bind(new_id())
    .execute(&pool)
    .await
    .unwrap();

    let (captured, guard) = capture();
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("restart");
    drop(guard);

    let output = captured.text();
    let warning = output
        .lines()
        .find(|line| line.contains("WARN"))
        .unwrap_or_else(|| panic!("no warning in {output}"));
    assert!(
        warning.contains(r#"plan "test_plan" holds keys this release does not know"#),
        "{warning}"
    );
    assert!(warning.contains("retired_feature"), "{warning}");
    assert!(warning.contains("retired_limit"), "{warning}");
    assert!(!warning.contains("custom_roles"), "{warning}");
    assert_eq!(output.matches("WARN").count(), 1, "{output}");
}
