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
    audit::{self, Actor},
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
    let saved = branding::save(&pool, scope(a), &rendered, request_time(), Actor::Instance)
        .await
        .unwrap();
    let expected = Branding::from_rendered(&rendered);
    assert_eq!(
        (&saved.brand, saved.generator, &saved.css, &saved.css_hash),
        (
            &expected.brand,
            expected.generator,
            &expected.css,
            &expected.css_hash
        )
    );
    assert_eq!(saved.css_hash, branding::css_hash(rendered.css()));
    assert_eq!(saved.report.len(), 3);
    assert_eq!(record_report_passes(&saved), [true, true, true]);
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
    branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let first = stored(&pool, a).await;
    let later = request_time() + Duration::from_secs(60);
    branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        later,
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(stored(&pool, a).await, first);
    let changed = branding::save(
        &pool,
        scope(a),
        &rendered("#0a1030"),
        later,
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(stored(&pool, a).await.1, 1_763_000_060_123_456);
    assert_eq!(branding::get(&pool, scope(a)).await.unwrap(), Some(changed));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn saving_for_a_missing_organization_is_not_found(pool: PgPool) {
    let result = branding::save(
        &pool,
        scope(new_id()),
        &rendered("#500E0E"),
        request_time(),
        Actor::Instance,
    )
    .await;
    assert!(matches!(result, Err(Error::NotFound)), "{result:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_change_writes_one_audit_entry_and_an_unchanged_save_none(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = Actor::User {
        id: new_id(),
        ip: "192.0.2.10".parse().unwrap(),
    };
    let first = branding::save(&pool, scope(a), &rendered("#500E0E"), request_time(), user)
        .await
        .unwrap();
    branding::save(&pool, scope(a), &rendered("#500E0E"), request_time(), user)
        .await
        .unwrap();
    let second = branding::save(&pool, scope(a), &rendered("#0a1030"), request_time(), user)
        .await
        .unwrap();

    let entries = audit::list(&pool, scope(a), 10).await.unwrap();
    assert_eq!(entries.len(), 2, "{entries:#?}");
    for entry in &entries {
        assert_eq!(entry.action, "organization.update_branding");
        assert_eq!(entry.target_type, "organization");
        assert_eq!(entry.target_id, a);
        assert_eq!(entry.actor, user);
        assert!(!entry.data.to_string().contains("--sc-"), "{}", entry.data);
    }
    let side = |b: &Branding| serde_json::json!({ "inputs": b.brand, "css_hash": b.css_hash });
    assert_eq!(
        entries[0].data,
        serde_json::json!({ "before": null, "after": side(&first) })
    );
    assert_eq!(
        entries[1].data,
        serde_json::json!({ "before": side(&first), "after": side(&second) })
    );
    assert!(audit::list(&pool, scope(b), 10).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_failed_save_writes_no_entry(pool: PgPool) {
    let missing = new_id();
    let result = branding::save(
        &pool,
        scope(missing),
        &rendered("#500E0E"),
        request_time(),
        Actor::Instance,
    )
    .await;
    assert!(matches!(result, Err(Error::NotFound)), "{result:?}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

fn record_report_passes(branding: &Branding) -> Vec<bool> {
    branding.report.iter().map(|r| r.pass).collect()
}
