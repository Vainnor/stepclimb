// SPDX-License-Identifier: AGPL-3.0-or-later
//! The startup sync of built-in roles (docs/specs/m0/09-permissions-and-roles.md B6, T4):
//! `startup::prepare`, which `stepclimb serve` runs before it listens, leaves eight built-in rows
//! holding this binary's lists, and running it again rewrites a row edited by SQL. Needs
//! `DATABASE_URL` (`just up`).

use std::time::{Duration, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{clock::ManualClock, roles::BUILTIN};
use stepclimb_server::startup;

async fn builtin_rows(pool: &PgPool) -> Vec<(String, String, Vec<String>)> {
    sqlx::query_as(
        "SELECT key, name, permissions FROM roles WHERE org_id IS NULL AND builtin ORDER BY key",
    )
    .fetch_all(pool)
    .await
    .expect("read roles")
}

fn code_rows() -> Vec<(String, String, Vec<String>)> {
    let mut rows: Vec<_> = BUILTIN
        .iter()
        .map(|r| {
            let permissions = r.permissions().into_iter().map(str::to_owned).collect();
            (r.key.to_owned(), r.name.to_owned(), permissions)
        })
        .collect();
    rows.sort();
    rows
}

#[sqlx::test(migrations = false)]
async fn startup_writes_the_builtin_roles_and_a_restart_rewrites_an_edited_row(pool: PgPool) {
    let clock = ManualClock::new(UNIX_EPOCH + Duration::from_secs(1_763_000_000));

    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    let first = builtin_rows(&pool).await;
    sqlx::query(
        "UPDATE roles SET name = 'Edited', permissions = '{branding.manage}'
         WHERE key = 'org_admin'",
    )
    .execute(&pool)
    .await
    .unwrap();
    clock.advance(Duration::from_secs(60));
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("restart");

    assert_eq!(first.len(), 8);
    assert_eq!(first, code_rows());
    assert_eq!(builtin_rows(&pool).await, code_rows());
}
