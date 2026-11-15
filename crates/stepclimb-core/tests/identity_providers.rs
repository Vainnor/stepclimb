// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity providers repository's writes for `stepclimb.toml` against a fresh database per
//! test (docs/specs/m0/06-oauth2-and-vatsim-preset.md B12, Q2). Needs `DATABASE_URL` (`just up`).
//!
//! Each test builds two organizations, A and B, and checks that org A's scope writes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    id::{Uuid, new_id},
    identity_providers::{self, ConfigRow, Error, Kind},
    organizations,
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn restart() -> SystemTime {
    start() + Duration::from_secs(3_600)
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a startup task, which these writes serve"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, start())
        .await
        .expect("create organization")
        .id
}

fn config(label: &str) -> Map<String, Value> {
    match json!({ "issuer": "https://idp.example.test", "label": label, "source": "config" }) {
        Value::Object(map) => map,
        _ => unreachable!("a JSON object literal"),
    }
}

fn row<'a>(key: &'a str, config: &'a Map<String, Value>, enabled: bool) -> ConfigRow<'a> {
    ConfigRow {
        key,
        kind: Kind::Oidc,
        config,
        enabled,
    }
}

/// `(kind, config, enabled, updated_at in microseconds)` of the org's row with `key`.
async fn stored(pool: &PgPool, org: Uuid, key: &str) -> Option<(String, Value, bool, i64)> {
    sqlx::query_as(
        "SELECT kind, config, enabled,
                (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM identity_providers WHERE org_id = $1 AND key = $2",
    )
    .bind(org)
    .bind(key)
    .fetch_optional(pool)
    .await
    .expect("select")
}

/// Insert a row as the admin API would, with a sealed secret and no `source`.
async fn api_row(pool: &PgPool, org: Uuid, key: &str) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, $3, 'oauth2', $4::jsonb, true)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .bind(r#"{"label": "From the API", "client_secret": {"sealed": "AAAA"}}"#)
    .execute(pool)
    .await
    .expect("insert provider");
    id
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire().await.expect("a connection")
}

fn micros(time: SystemTime) -> i64 {
    let since = time.duration_since(UNIX_EPOCH).expect("after the epoch");
    i64::try_from(since.as_micros()).expect("fits")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn upsert_inserts_then_updates_by_key_in_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let first = config("First");
    let id = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &first, true),
        start(),
    )
    .await
    .expect("insert");
    let (kind, value, enabled, updated) = stored(&pool, a, "corp").await.expect("row");
    assert_eq!((kind.as_str(), enabled), ("oidc", true));
    assert_eq!(value, Value::Object(first.clone()));
    assert_eq!(updated, micros(start()));
    assert_eq!(stored(&pool, b, "corp").await, None);

    // The same row again leaves `updated_at` alone.
    let again = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &first, true),
        restart(),
    )
    .await
    .expect("same");
    assert_eq!(again, id);
    assert_eq!(
        stored(&pool, a, "corp").await.expect("row").3,
        micros(start())
    );

    // A change moves it, and keeps the id.
    let second = config("Second");
    let changed = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &second, false),
        restart(),
    )
    .await
    .expect("update");
    assert_eq!(changed, id);
    let (_, value, enabled, updated) = stored(&pool, a, "corp").await.expect("row");
    assert_eq!(value["label"], json!("Second"));
    assert!(!enabled);
    assert_eq!(updated, micros(restart()));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn upsert_takes_over_a_row_the_api_wrote_with_the_same_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let api = api_row(&pool, a, "corp").await;
    let file = config("From the file");
    let id = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &file, true),
        start(),
    )
    .await
    .expect("upsert");
    assert_eq!(id, api, "the identities stay linked to the same row");
    let (kind, value, _, _) = stored(&pool, a, "corp").await.expect("row");
    assert_eq!(kind, "oidc");
    assert_eq!(value, Value::Object(file));
    let provider = identity_providers::get_by_key(&pool, scope(a), "corp")
        .await
        .expect("read")
        .expect("found");
    assert!(provider.is_from_config_file());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn upsert_refuses_a_config_without_the_config_source(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let mut not_ours = config("Not ours");
    not_ours.remove("source");
    let result = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &not_ours, true),
        start(),
    )
    .await;
    assert!(matches!(result, Err(Error::NotFromConfig)), "{result:?}");
    not_ours.insert("source".into(), json!("api"));
    let result = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("corp", &not_ours, true),
        start(),
    )
    .await;
    assert!(matches!(result, Err(Error::NotFromConfig)), "{result:?}");
    assert_eq!(stored(&pool, a, "corp").await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn upsert_cannot_turn_the_local_row_into_an_outside_provider(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let file = config("Local");
    let result = identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(a),
        &row("local", &file, true),
        start(),
    )
    .await;
    assert!(matches!(result, Err(Error::Database(_))), "{result:?}");
    let (kind, _, _, _) = stored(&pool, a, "local").await.expect("the local row");
    assert_eq!(kind, "local");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn disable_turns_off_only_the_files_rows_it_no_longer_declares_in_the_scope(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let file = config("File");
    for key in ["kept", "removed", "already-off"] {
        identity_providers::upsert_from_config(
            &mut conn(&pool).await,
            scope(a),
            &row(key, &file, key != "already-off"),
            start(),
        )
        .await
        .expect("upsert");
    }
    identity_providers::upsert_from_config(
        &mut conn(&pool).await,
        scope(b),
        &row("removed", &file, true),
        start(),
    )
    .await
    .expect("upsert in B");
    api_row(&pool, a, "from-api").await;

    let disabled = identity_providers::disable_config_rows_except(
        &pool,
        scope(a),
        &["kept".to_owned()],
        restart(),
    )
    .await
    .expect("disable");
    assert_eq!(disabled, vec!["removed".to_owned()]);
    let (_, _, enabled, updated) = stored(&pool, a, "removed")
        .await
        .expect("kept, not deleted");
    assert!(!enabled);
    assert_eq!(updated, micros(restart()));
    assert!(stored(&pool, a, "kept").await.expect("row").2);
    assert!(
        stored(&pool, a, "from-api").await.expect("row").2,
        "the API's row is not the file's"
    );
    assert!(
        stored(&pool, a, "local").await.expect("row").2,
        "the local row stays"
    );
    let (_, _, enabled, updated) = stored(&pool, a, "already-off").await.expect("row");
    assert!(!enabled);
    assert_eq!(
        updated,
        micros(start()),
        "an already disabled row is not touched"
    );
    assert!(
        stored(&pool, b, "removed").await.expect("B's row").2,
        "B is another scope"
    );
}
