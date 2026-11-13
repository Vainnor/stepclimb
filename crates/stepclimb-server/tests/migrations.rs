// SPDX-License-Identifier: AGPL-3.0-or-later
//! Migrations under concurrency and failure (docs/specs/m0/14-compose-deploy.md B15, B16; T6,
//! T7): server processes starting together apply each migration once, and a failing migration
//! stops the run, names its version and leaves the earlier ones applied. Needs `DATABASE_URL`
//! (`just up`).

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use stepclimb_core::db::MIGRATOR;
use stepclimb_server::startup::{self, StartupError};

/// Four migrations: the third creates a table, then fails on a missing one.
static FAILING: Migrator = sqlx::migrate!("tests/fixtures/failing-migrations");

/// How many servers start at once in the concurrency test.
const RUNNERS: usize = 4;

async fn migration_rows(pool: &PgPool) -> Vec<(i64, bool)> {
    sqlx::query("SELECT version, success FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .expect("the migrations table exists")
        .into_iter()
        .map(|row| (row.get("version"), row.get("success")))
        .collect()
}

async fn table_exists(pool: &PgPool, name: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("to_regclass runs")
}

/// T6: several servers migrate one empty database at once, each through its own pool as separate
/// processes would; every run succeeds and each version is recorded once.
#[sqlx::test(migrations = false)]
async fn concurrent_migration_runs_apply_each_migration_once(pool: PgPool) {
    let options = pool.connect_options().as_ref().clone();
    let mut pools = Vec::with_capacity(RUNNERS);
    for _ in 0..RUNNERS {
        pools.push(
            PgPoolOptions::new()
                .max_connections(1)
                .connect_with(options.clone())
                .await
                .expect("connect"),
        );
    }

    let mut runs = tokio::task::JoinSet::new();
    for (runner, pool) in pools.into_iter().enumerate() {
        runs.spawn(async move { (runner, startup::migrate(&pool).await) });
    }
    while let Some(joined) = runs.join_next().await {
        let (runner, result) = joined.expect("the run does not panic");
        assert!(result.is_ok(), "runner {runner}: {result:?}");
    }

    let embedded: Vec<(i64, bool)> = MIGRATOR.iter().map(|m| (m.version, true)).collect();
    assert!(!embedded.is_empty());
    assert_eq!(migration_rows(&pool).await, embedded);
}

/// T7: the run stops at the failing migration, the error names its version and the database's
/// error, the migrations before it stay applied, and nothing of it or after it remains.
#[sqlx::test(migrations = false)]
async fn a_failing_migration_names_its_version_and_keeps_the_earlier_ones(pool: PgPool) {
    let error = startup::run_migrations(&FAILING, &pool)
        .await
        .expect_err("migration 3 fails");
    assert!(matches!(error, StartupError::Migrate(_)), "{error:?}");
    let message = error.to_string();
    assert!(
        message.contains("while executing migration 3: "),
        "{message}"
    );
    assert!(
        message.contains(r#"relation "fixture_missing" does not exist"#),
        "{message}"
    );

    assert_eq!(migration_rows(&pool).await, vec![(1, true), (2, true)]);
    assert!(table_exists(&pool, "fixture_first").await);
    assert!(table_exists(&pool, "fixture_second").await);
    assert!(!table_exists(&pool, "fixture_broken").await);
    assert!(!table_exists(&pool, "fixture_after").await);
}
