// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organizations repository against a fresh database per test (spec 02 B7, B13, B14).
//! Needs `DATABASE_URL` (`just up`).
//!
//! Every test makes two organizations and checks that a call with org A's scope reads or
//! changes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, OrgSettings, OrgSettingsPatch,
    id::{Uuid, new_id},
    organizations::{self, Error, MAX_NAME_CHARS, Organization},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
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

/// Inserts an organization directly; `organizations::create` belongs to spec 02 issue 3.
async fn insert_org(pool: &PgPool, slug: &str, name: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(slug)
        .bind(name)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

/// `(name, settings, updated_at in microseconds since the epoch)` straight from the table.
async fn stored(pool: &PgPool, id: Uuid) -> (String, String, i64) {
    sqlx::query_as(
        "SELECT name, settings::text,
                (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM organizations WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read organization")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_returns_the_scopes_organization_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let b = insert_org(&pool, "org-b", "Org B").await;

    let got_a = organizations::get(&pool, scope(a)).await.unwrap();
    let got_b = organizations::get(&pool, scope(b)).await.unwrap();

    let expected_a = Organization {
        id: a,
        slug: "org-a".into(),
        name: "Org A".into(),
    };
    assert_eq!(got_a, expected_a);
    assert_eq!(got_b.id, b);
    assert_eq!(got_b.slug, "org-b");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_call_with_a_scope_whose_organization_has_no_row_is_not_found(pool: PgPool) {
    insert_org(&pool, "org-a", "Org A").await;
    let gone = scope(new_id());
    let now = request_time();

    let get = organizations::get(&pool, gone).await;
    let rename = organizations::rename(&pool, gone, "New", now).await;
    let settings = organizations::settings(&pool, gone).await;
    let patch = OrgSettingsPatch::default();
    let update = organizations::update_settings(&pool, gone, &patch, now).await;

    assert!(matches!(get, Err(Error::NotFound)), "{get:?}");
    assert!(matches!(rename, Err(Error::NotFound)), "{rename:?}");
    assert!(matches!(settings, Err(Error::NotFound)), "{settings:?}");
    assert!(matches!(update, Err(Error::NotFound)), "{update:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn rename_trims_sets_updated_at_from_the_clock_and_touches_one_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let b = insert_org(&pool, "org-b", "Org B").await;
    let b_before = stored(&pool, b).await;

    let renamed = organizations::rename(&pool, scope(a), "  Tower Cab  ", request_time())
        .await
        .unwrap();

    assert_eq!(renamed.name, "Tower Cab");
    assert_eq!(renamed.slug, "org-a", "the slug never changes");
    let (name, _, updated_at) = stored(&pool, a).await;
    assert_eq!(name, "Tower Cab");
    assert_eq!(
        updated_at, 1_763_000_000_123_456,
        "updated_at is the clock's reading"
    );
    assert_eq!(stored(&pool, b).await, b_before, "org B is unchanged");
    let read_back = organizations::get(&pool, scope(a)).await.unwrap();
    assert_eq!(read_back, renamed);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn rename_refuses_a_blank_or_too_long_name_and_changes_nothing(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let before = stored(&pool, a).await;
    let now = request_time();

    let blank = organizations::rename(&pool, scope(a), " \t ", now).await;
    let long = "a".repeat(MAX_NAME_CHARS + 1);
    let too_long = organizations::rename(&pool, scope(a), &long, now).await;

    assert!(matches!(blank, Err(Error::NameEmpty)), "{blank:?}");
    assert!(matches!(too_long, Err(Error::NameTooLong)), "{too_long:?}");
    assert_eq!(stored(&pool, a).await, before);

    let at_limit = "a".repeat(MAX_NAME_CHARS);
    let renamed = organizations::rename(&pool, scope(a), &at_limit, now).await;
    assert_eq!(renamed.unwrap().name, at_limit);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_far_future_update_time_is_stored_to_the_microsecond(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    // 2200-01-01T00:00:00.123457Z.
    let micros = 7_258_118_400_123_457;
    let now = UNIX_EPOCH + Duration::from_micros(micros);

    organizations::rename(&pool, scope(a), "New", now)
        .await
        .unwrap();

    assert_eq!(stored(&pool, a).await.2, i64::try_from(micros).unwrap());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn rename_refuses_a_time_the_database_cannot_store(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let before_epoch = UNIX_EPOCH - Duration::from_secs(1);

    let result = organizations::rename(&pool, scope(a), "New", before_epoch).await;

    assert!(matches!(result, Err(Error::Time(_))), "{result:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_organization_reads_every_setting_as_its_default(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;

    let settings = organizations::settings(&pool, scope(a)).await.unwrap();

    assert_eq!(settings, OrgSettings::default());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_setting_the_code_does_not_know_is_an_error(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    sqlx::query("UPDATE organizations SET settings = '{\"no_such_key\": 1}' WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();

    let read = organizations::settings(&pool, scope(a)).await;

    assert!(matches!(read, Err(Error::StoredSettings(_))), "{read:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_settings_merges_into_the_stored_object_of_one_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let b = insert_org(&pool, "org-b", "Org B").await;
    let b_before = stored(&pool, b).await;

    let patch = OrgSettingsPatch::default();
    let settings = organizations::update_settings(&pool, scope(a), &patch, request_time())
        .await
        .unwrap();

    assert_eq!(settings, OrgSettings::default());
    let (_, stored_settings, updated_at) = stored(&pool, a).await;
    assert_eq!(stored_settings, "{}");
    assert_eq!(
        updated_at, 1_763_000_000_123_456,
        "updated_at is the clock's reading"
    );
    assert_eq!(stored(&pool, b).await, b_before, "org B is unchanged");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn count_is_instance_wide(pool: PgPool) {
    assert_eq!(organizations::count(&pool).await.unwrap(), 0);

    insert_org(&pool, "org-a", "Org A").await;
    insert_org(&pool, "org-b", "Org B").await;

    assert_eq!(organizations::count(&pool).await.unwrap(), 2);
}
