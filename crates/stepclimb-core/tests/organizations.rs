// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organizations repository against a fresh database per test (spec 02 B7, B13, B14, B16,
//! B26; T10).
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

/// Inserts an organization row alone, without the root account `organizations::create` adds.
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

/// Writes `settings` straight into the row, standing in for a newer release or a bad write.
async fn store_settings(pool: &PgPool, id: Uuid, settings: &str) {
    sqlx::query("UPDATE organizations SET settings = $2::jsonb WHERE id = $1")
        .bind(id)
        .bind(settings)
        .execute(pool)
        .await
        .expect("store settings");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_setting_the_code_does_not_know_is_ignored_on_read(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    store_settings(&pool, a, r#"{"newer_key": 1}"#).await;

    let read = organizations::settings(&pool, scope(a)).await.unwrap();

    assert_eq!(read, OrgSettings::default());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_settings_merges_into_the_stored_object_of_one_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    let b = insert_org(&pool, "org-b", "Org B").await;
    store_settings(&pool, a, r#"{"newer_key": {"kept": true}}"#).await;
    let b_before = stored(&pool, b).await;

    let patch = OrgSettingsPatch::default();
    let settings = organizations::update_settings(&pool, scope(a), &patch, request_time())
        .await
        .unwrap();

    assert_eq!(settings, OrgSettings::default());
    let (_, stored_settings, updated_at) = stored(&pool, a).await;
    assert_eq!(
        stored_settings, r#"{"newer_key": {"kept": true}}"#,
        "a stored key the patch does not name survives the merge"
    );
    assert_eq!(
        updated_at, 1_763_000_000_123_456,
        "updated_at is the clock's reading"
    );
    assert_eq!(stored(&pool, b).await, b_before, "org B is unchanged");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_settings_keeps_nothing_when_the_merged_object_does_not_parse(pool: PgPool) {
    let a = insert_org(&pool, "org-a", "Org A").await;
    // Not an object: `"oops" || {}` is the array `["oops", {}]`, which is no `OrgSettings`.
    store_settings(&pool, a, r#""oops""#).await;
    let before = stored(&pool, a).await;

    let patch = OrgSettingsPatch::default();
    let result = organizations::update_settings(&pool, scope(a), &patch, request_time()).await;

    assert!(
        matches!(result, Err(Error::StoredSettings(_))),
        "{result:?}"
    );
    assert_eq!(
        stored(&pool, a).await,
        before,
        "the failed update was rolled back"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn count_is_instance_wide(pool: PgPool) {
    assert_eq!(organizations::count(&pool).await.unwrap(), 0);

    insert_org(&pool, "org-a", "Org A").await;
    insert_org(&pool, "org-b", "Org B").await;

    assert_eq!(organizations::count(&pool).await.unwrap(), 2);
}

/// `(accounts of the organization, of which roots)` straight from the table.
async fn account_counts(pool: &PgPool, org: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE parent_id IS NULL)
         FROM accounts WHERE org_id = $1",
    )
    .bind(org)
    .fetch_one(pool)
    .await
    .expect("count accounts")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_inserts_the_organization_and_its_root_account(pool: PgPool) {
    let created = organizations::create(&pool, "org-a", "  Org A  ", request_time())
        .await
        .unwrap();

    assert_eq!(created.slug, "org-a");
    assert_eq!(created.name, "Org A", "the name is trimmed");
    assert_eq!(created.id.get_version_num(), 7);
    assert_eq!(
        organizations::get(&pool, scope(created.id)).await.unwrap(),
        created
    );
    let (_, settings, updated_at) = stored(&pool, created.id).await;
    assert_eq!(settings, "{}");
    assert_eq!(updated_at, 1_763_000_000_123_456);

    // T10: exactly one root, depth 0, named after the organization, with the clock's times.
    let root: (Uuid, String, i32, String, i64, i64) = sqlx::query_as(
        "SELECT id, name, nlevel(path) - 1, path::text,
                (extract(epoch FROM created_at) * 1000000)::bigint,
                (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM accounts WHERE org_id = $1 AND parent_id IS NULL",
    )
    .bind(created.id)
    .fetch_one(&pool)
    .await
    .expect("the root account");
    let (root_id, root_name, depth, path, created_at, root_updated_at) = root;
    assert_eq!(root_name, "Org A");
    assert_eq!(depth, 0);
    assert_eq!(path, root_id.simple().to_string());
    assert_eq!(root_id.get_version_num(), 7);
    assert_eq!((created_at, root_updated_at), (updated_at, updated_at));
    assert_eq!(account_counts(&pool, created.id).await, (1, 1));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_second_root_for_an_organization_is_refused_by_the_database(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();
    let id = new_id();

    let second = sqlx::query(
        "INSERT INTO accounts (id, org_id, name, path) VALUES ($1, $2, 'Second', $3::text::ltree)",
    )
    .bind(id)
    .bind(org.id)
    .bind(id.simple().to_string())
    .execute(&pool)
    .await;

    let error = second.expect_err("a second root is refused");
    let constraint = error.as_database_error().and_then(|e| e.constraint());
    assert_eq!(constraint, Some("accounts_one_root"), "{error}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_a_taken_slug_and_inserts_nothing(pool: PgPool) {
    let first = organizations::create(&pool, "org-a", "Org A", request_time())
        .await
        .unwrap();

    let again = organizations::create(&pool, "org-a", "Another", request_time()).await;

    assert!(matches!(again, Err(Error::SlugTaken)), "{again:?}");
    assert_eq!(organizations::count(&pool).await.unwrap(), 1);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 1, "no root account was left behind");
    assert_eq!(account_counts(&pool, first.id).await, (1, 1));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_an_invalid_slug_or_name_and_inserts_nothing(pool: PgPool) {
    let now = request_time();
    let long = "a".repeat(MAX_NAME_CHARS + 1);
    let cases: [(&str, &str, &str); 6] = [
        ("Org-A", "Org A", "SlugInvalid"),
        ("-org", "Org A", "SlugInvalid"),
        ("api", "Org A", "SlugReserved"),
        ("org-a", " ", "NameEmpty"),
        ("org-a", &long, "NameTooLong"),
        ("org-a", "Org\0A", "NameContainsNul"),
    ];

    for (slug, name, expected) in cases {
        let result = organizations::create(&pool, slug, name, now).await;
        let matched = match &result {
            Err(Error::SlugInvalid) => "SlugInvalid",
            Err(Error::SlugReserved) => "SlugReserved",
            Err(Error::NameEmpty) => "NameEmpty",
            Err(Error::NameTooLong) => "NameTooLong",
            Err(Error::NameContainsNul) => "NameContainsNul",
            _ => "other",
        };
        assert_eq!(matched, expected, "{slug:?} {name:?}: {result:?}");
    }
    assert_eq!(organizations::count(&pool).await.unwrap(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_inside_a_transaction_is_undone_with_it(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    organizations::create(&mut *tx, "org-a", "Org A", request_time())
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(organizations::count(&pool).await.unwrap(), 0);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_keeps_no_organization_when_its_root_account_fails(pool: PgPool) {
    // A trigger in this test's own database makes the root insert fail after the organization
    // row is written.
    sqlx::raw_sql(
        "CREATE FUNCTION refuse_accounts() RETURNS trigger LANGUAGE plpgsql
             AS $$ BEGIN RAISE EXCEPTION 'refused by the test'; END $$;
         CREATE TRIGGER refuse_accounts BEFORE INSERT ON accounts
             FOR EACH ROW EXECUTE FUNCTION refuse_accounts();",
    )
    .execute(&pool)
    .await
    .expect("install the trigger");

    let result = organizations::create(&pool, "org-a", "Org A", request_time()).await;

    assert!(matches!(result, Err(Error::Database(_))), "{result:?}");
    assert_eq!(
        organizations::count(&pool).await.unwrap(),
        0,
        "the organization row was rolled back with the failed root"
    );
}
