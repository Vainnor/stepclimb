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
    appearance::Theme,
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
    let saved = branding::save(
        &pool,
        scope(a),
        &rendered,
        Theme::Dark,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let expected = Branding::from_rendered(&rendered, Theme::Dark);
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
            "default_appearance": "dark",
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
        Theme::System,
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
        Theme::System,
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
        Theme::System,
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
        Theme::System,
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
    let first = branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::System,
        request_time(),
        user,
    )
    .await
    .unwrap();
    branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::System,
        request_time(),
        user,
    )
    .await
    .unwrap();
    let second = branding::save(
        &pool,
        scope(a),
        &rendered("#0a1030"),
        Theme::System,
        request_time(),
        user,
    )
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
    let side = |b: &Branding| {
        let mut inputs = serde_json::to_value(&b.brand).unwrap();
        inputs["default_appearance"] = "system".into();
        serde_json::json!({ "inputs": inputs, "css_hash": b.css_hash })
    };
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
        Theme::System,
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

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_default_appearance_alone_is_a_change(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let user = Actor::User {
        id: new_id(),
        ip: "192.0.2.10".parse().unwrap(),
    };
    let first = branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::System,
        request_time(),
        user,
    )
    .await
    .unwrap();
    let dark = branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::Dark,
        request_time(),
        user,
    )
    .await
    .unwrap();
    assert_eq!(dark.default_appearance, Theme::Dark);
    assert_eq!(dark.css_hash, first.css_hash, "the stylesheet is the same");
    assert_eq!(branding::get(&pool, scope(a)).await.unwrap(), Some(dark));
    let entries = audit::list(&pool, scope(a), 10).await.unwrap();
    assert_eq!(entries.len(), 2, "{entries:#?}");
    assert_eq!(
        entries[1].data["before"]["inputs"]["default_appearance"],
        "system"
    );
    assert_eq!(
        entries[1].data["after"]["inputs"]["default_appearance"],
        "dark"
    );
}

/// Rewrites the stored record as another generator version wrote it: `generator` and a
/// stylesheet this generator does not make.
async fn age_record(pool: &PgPool, org: Uuid, generator: u32, primary: Option<&str>) {
    let (mut record, _) = stored(pool, org).await;
    record["generator"] = generator.into();
    record["css"] = ":root { --sc-brand: red; }".into();
    record["css_hash"] = branding::css_hash(":root { --sc-brand: red; }").into();
    if let Some(primary) = primary {
        record["inputs"]["primary"] = primary.into();
    }
    sqlx::query("UPDATE organizations SET branding = $2::text::jsonb WHERE id = $1")
        .bind(org)
        .bind(record.to_string())
        .execute(pool)
        .await
        .expect("age the record");
}

async fn audit_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(pool)
        .await
        .expect("count the audit entries")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn refresh_rerenders_another_generators_record_for_the_scope_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let saved = branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::Dark,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    branding::save(
        &pool,
        scope(b),
        &rendered("#0a1030"),
        Theme::System,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    age_record(&pool, a, GENERATOR_VERSION - 1, None).await;
    age_record(&pool, b, GENERATOR_VERSION - 1, None).await;
    let (_, a_time) = stored(&pool, a).await;
    let b_before = stored(&pool, b).await;
    let entries = audit_count(&pool).await;

    let refreshed = branding::refresh(&pool, scope(a)).await.unwrap();
    let Some(branding::Rerendered::Passed(rerendered)) = refreshed else {
        panic!("the record was not re-rendered: {refreshed:?}");
    };
    let key = |b: &Branding| {
        (
            b.brand.clone(),
            b.default_appearance,
            b.generator,
            b.css.clone(),
            b.css_hash.clone(),
            b.fallback.clone(),
        )
    };
    assert_eq!(key(&rerendered), key(&saved));
    assert_eq!(branding::get(&pool, scope(a)).await.unwrap(), Some(saved));
    let (record, updated_at) = stored(&pool, a).await;
    assert_eq!(record["generator"], GENERATOR_VERSION);
    assert_eq!(updated_at, a_time, "nobody changed the branding");
    assert_eq!(audit_count(&pool).await, entries, "no audit entry");
    assert_eq!(stored(&pool, b).await, b_before);

    // This generator's record is left alone.
    let again = branding::refresh(&pool, scope(a)).await.unwrap();
    assert!(again.is_none(), "{again:?}");
    // So is a newer generator's: that release re-renders its own records.
    age_record(&pool, a, GENERATOR_VERSION + 1, None).await;
    let newer = stored(&pool, a).await;
    let left = branding::refresh(&pool, scope(a)).await.unwrap();
    assert!(left.is_none(), "{left:?}");
    assert_eq!(stored(&pool, a).await, newer);
    let missing = branding::refresh(&pool, scope(new_id())).await;
    assert!(matches!(missing, Err(Error::NotFound)), "{missing:?}");
    let none = insert_org(&pool, "org-c").await;
    assert!(
        branding::refresh(&pool, scope(none))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(stored(&pool, none).await.0, serde_json::json!({}));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn refresh_stores_the_default_stylesheet_for_inputs_this_generator_refuses(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    branding::save(
        &pool,
        scope(a),
        &rendered("#500E0E"),
        Theme::HighContrast,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    // A chroma above the running generator's cap, as a record from before the cap.
    age_record(&pool, a, 0, Some("oklch(0.5 0.9 30)")).await;
    let entries = audit_count(&pool).await;
    let refreshed = branding::refresh(&pool, scope(a)).await.unwrap();
    let Some(branding::Rerendered::FellBack { branding, .. }) = refreshed else {
        panic!("the refused inputs were not replaced: {refreshed:?}");
    };
    let default = theme::render(&Brand::stepclimb_default()).unwrap();
    assert_eq!(branding.css_hash, branding::css_hash(default.css()));
    assert!(branding.fallback.is_some());
    let read = branding::get(&pool, scope(a)).await.unwrap().unwrap();
    assert_eq!(read.css, default.css());
    assert_eq!(
        read.brand.primary, "oklch(0.5 0.9 30)",
        "the inputs are kept"
    );
    assert_eq!(read.default_appearance, Theme::HighContrast);
    assert_eq!(read.generator, GENERATOR_VERSION);
    assert!(read.fallback.is_some());
    assert_eq!(audit_count(&pool).await, entries);
}
