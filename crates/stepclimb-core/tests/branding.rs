// SPDX-License-Identifier: AGPL-3.0-or-later
//! The branding repository against a fresh database per test (spec 12, B21, B24, B25 and Data).
//! Needs `DATABASE_URL` (`just up`).
//!
//! Every test makes two organizations and checks that a call with org A's scope reads or
//! changes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    branding::{self, Branding, Error, RECORD_VERSION},
    id::{Uuid, new_id},
    theme::{self, Brand, BrandInputs, GENERATOR_VERSION},
};

fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(slug)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

/// `(branding, updated_at in microseconds)` straight from the table.
async fn stored(pool: &PgPool, id: Uuid) -> (serde_json::Value, i64) {
    let (text, at): (String, i64) = sqlx::query_as(
        "SELECT branding::text, (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM organizations WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read organization");
    (serde_json::from_str(&text).expect("jsonb is JSON"), at)
}

fn rendered(primary: &str) -> theme::Rendered {
    let inputs = BrandInputs {
        primary: primary.to_owned(),
        ui_font: Some("Roboto".to_owned()),
        corner_style: Some("round".to_owned()),
        ..BrandInputs::default()
    };
    let brand = Brand::parse(&inputs).expect("valid inputs");
    theme::render(&brand).expect("the brand passes its contrast report")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_without_saved_branding_has_none(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    assert_eq!(branding::get(&pool, scope(a)).await.unwrap(), None);
    let missing = branding::get(&pool, scope(new_id())).await;
    assert!(matches!(missing, Err(Error::NotFound)), "{missing:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn save_stores_the_versioned_record_for_the_scope_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let (b_before, b_time) = stored(&pool, b).await;
    let rendered = rendered("#500E0E");
    let saved = branding::save(&pool, scope(a), &rendered, request_time())
        .await
        .unwrap();
    assert_eq!(saved, Branding::from_rendered(&rendered));
    assert_eq!(saved.css_hash, branding::css_hash(rendered.css()));
    assert_eq!(
        branding::get(&pool, scope(a)).await.unwrap(),
        Some(saved.clone())
    );

    let (record, updated_at) = stored(&pool, a).await;
    assert_eq!(record["version"], RECORD_VERSION);
    assert_eq!(record["generator"], GENERATOR_VERSION);
    assert_eq!(record["css_hash"], saved.css_hash.as_str());
    assert_eq!(
        record["inputs"],
        serde_json::json!({
            "primary": "#500E0E",
            "ui_font": "Roboto",
            "reading_font": "serif",
            "corner_style": "round",
        })
    );
    assert_eq!(updated_at, 1_763_000_000_123_456);
    assert_eq!(stored(&pool, b).await, (b_before, b_time));
    assert_eq!(branding::get(&pool, scope(b)).await.unwrap(), None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn saving_the_same_branding_again_writes_nothing(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    branding::save(&pool, scope(a), &rendered("#500E0E"), request_time())
        .await
        .unwrap();
    let first = stored(&pool, a).await;
    let later = request_time() + Duration::from_secs(60);
    branding::save(&pool, scope(a), &rendered("#500E0E"), later)
        .await
        .unwrap();
    assert_eq!(stored(&pool, a).await, first);
    let changed = branding::save(&pool, scope(a), &rendered("#0a1030"), later)
        .await
        .unwrap();
    assert_eq!(stored(&pool, a).await.1, 1_763_000_060_123_456);
    assert_eq!(branding::get(&pool, scope(a)).await.unwrap(), Some(changed));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn saving_for_a_missing_organization_is_not_found(pool: PgPool) {
    let result = branding::save(&pool, scope(new_id()), &rendered("#500E0E"), request_time()).await;
    assert!(matches!(result, Err(Error::NotFound)), "{result:?}");
}
