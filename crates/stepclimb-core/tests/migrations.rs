// SPDX-License-Identifier: AGPL-3.0-or-later
//! The embedded migrations against a fresh database per test (`#[sqlx::test]`, which needs
//! `DATABASE_URL` pointing at a PostgreSQL server: `just up` starts the Compose one).

use sqlx::PgPool;
use stepclimb_core::db::MIGRATOR;

#[sqlx::test(migrations = false)]
async fn migrations_create_the_organizations_table(pool: PgPool) {
    MIGRATOR
        .run(&pool)
        .await
        .expect("migrations apply to an empty database");

    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = 'organizations'
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("information_schema is readable");

    let expected = [
        ("id", "uuid", "NO"),
        ("slug", "text", "NO"),
        ("name", "text", "NO"),
        ("settings", "jsonb", "NO"),
        ("branding", "jsonb", "NO"),
        ("created_at", "timestamp with time zone", "NO"),
        ("updated_at", "timestamp with time zone", "NO"),
    ];
    let got: Vec<(&str, &str, &str)> = columns
        .iter()
        .map(|(n, t, null)| (n.as_str(), t.as_str(), null.as_str()))
        .collect();
    assert_eq!(got, expected);
}

#[sqlx::test(migrations = false)]
async fn organization_slugs_are_unique(pool: PgPool) {
    MIGRATOR.run(&pool).await.expect("migrations apply");
    let insert = "INSERT INTO organizations (id, slug, name) VALUES (gen_random_uuid(), $1, $2)";
    sqlx::query(insert)
        .bind("vzdc")
        .bind("vZDC")
        .execute(&pool)
        .await
        .expect("first insert");

    let second = sqlx::query(insert)
        .bind("vzdc")
        .bind("Another")
        .execute(&pool)
        .await;

    let error = second.expect_err("a second organization with the same slug is refused");
    let code = error
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned());
    assert_eq!(
        code.as_deref(),
        Some("23505"),
        "unique_violation, got {error}"
    );
}

#[sqlx::test(migrations = false)]
async fn migrations_are_idempotent_on_rerun(pool: PgPool) {
    MIGRATOR.run(&pool).await.expect("first run");
    MIGRATOR
        .run(&pool)
        .await
        .expect("a second run applies nothing and succeeds");
}
