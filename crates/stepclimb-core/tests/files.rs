// SPDX-License-Identifier: AGPL-3.0-or-later
//! The stored files repository and the `files` and `course_files` tables against a fresh
//! database per test (docs/specs/m1/06-files.md B2, B7, B22, Data, Q9): one row per distinct file
//! of an organization, found only in its own organization, and the `storage` limit's count.
//! Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    courses::{self, NewCourse},
    feature::Limit,
    files::{self, Error, Hashed, NewFile, Sha256Digest, Sha256Hasher},
    id::{Uuid, new_id},
    org_plans::{self, Room},
    organizations,
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id
}

/// What the server's hasher computes over `size` bytes of `byte`.
fn hashed(byte: u8, size: u64) -> Hashed {
    let mut hasher = Sha256Hasher::new();
    hasher.update(&vec![byte; usize::try_from(size).expect("a test size")]);
    hasher.finish()
}

fn new_file(byte: u8, size: u64) -> NewFile {
    NewFile {
        hashed: hashed(byte, size),
        mime: "application/pdf".into(),
    }
}

async fn insert(pool: &PgPool, org: Uuid, new: &NewFile) -> Result<files::File, Error> {
    files::insert(pool, scope(org), new, now()).await
}

async fn find(pool: &PgPool, org: Uuid, sha256: Sha256Digest) -> Option<files::File> {
    let mut conn = pool.acquire().await.expect("a connection");
    files::find_by_hash(&mut conn, scope(org), sha256)
        .await
        .expect("find_by_hash")
}

async fn row_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(pool)
        .await
        .expect("count files")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_inserted_file_is_found_by_its_hash_with_its_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let new = new_file(0xab, 1234);
    let file = insert(&pool, a, &new).await.unwrap();
    let sha256 = new.hashed.sha256();
    assert_eq!(file.sha256, sha256);
    assert_eq!(file.size, 1234);
    assert_eq!(file.mime, "application/pdf");
    assert_eq!(file.storage_key, format!("orgs/{a}/files/{}", sha256.hex()));
    assert_eq!(file.storage_key, files::storage_key(scope(a), sha256));
    assert_eq!((file.created_at, file.updated_at), (now(), now()));
    assert_eq!(find(&pool, a, sha256).await, Some(file));
    assert_eq!(find(&pool, a, hashed(0xcd, 1).sha256()).await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_same_bytes_are_stored_once_per_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let new = new_file(0x11, 900);
    let first = insert(&pool, a, &new).await.unwrap();
    let again = insert(&pool, a, &new).await;
    assert!(matches!(again, Err(Error::HashTaken)), "{again:?}");
    assert_eq!(row_count(&pool).await, 1);
    // Another organization's copy of the same bytes is its own row and key (Q9).
    let theirs = insert(&pool, b, &new).await.unwrap();
    assert_ne!(theirs.id, first.id);
    assert_ne!(theirs.storage_key, first.storage_key);
    assert_eq!(row_count(&pool).await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_hash_is_found_only_in_its_own_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let new = new_file(0x22, 10);
    insert(&pool, a, &new).await.unwrap();
    assert!(find(&pool, a, new.hashed.sha256()).await.is_some());
    assert_eq!(find(&pool, b, new.hashed.sha256()).await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_hash_taken_in_a_transaction_aborts_it_and_the_retry_finds_the_row(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let new = new_file(0x33, 10);
    let committed = insert(&pool, a, &new).await.unwrap();
    let mut tx = pool.begin().await.expect("begin");
    let lost = files::insert(&mut *tx, scope(a), &new, now()).await;
    assert!(matches!(lost, Err(Error::HashTaken)), "{lost:?}");
    tx.rollback().await.expect("roll back");
    let mut retry = pool.begin().await.expect("begin");
    let found = files::find_by_hash(&mut retry, scope(a), new.hashed.sha256())
        .await
        .unwrap();
    assert_eq!(found, Some(committed));
    retry.commit().await.expect("commit");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn find_by_hash_holds_the_row_against_a_delete_until_the_transaction_ends(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let new = new_file(0x44, 10);
    insert(&pool, a, &new).await.unwrap();
    let mut reusing = pool.begin().await.expect("begin");
    assert!(
        files::find_by_hash(&mut reusing, scope(a), new.hashed.sha256())
            .await
            .unwrap()
            .is_some()
    );
    let mut deleting = pool.begin().await.expect("begin");
    sqlx::query("SET LOCAL lock_timeout = '200ms'")
        .execute(&mut *deleting)
        .await
        .expect("lock timeout");
    let blocked = sqlx::query("DELETE FROM files WHERE org_id = $1")
        .bind(a)
        .execute(&mut *deleting)
        .await;
    let code = blocked
        .expect_err("the delete waits for the reusing transaction")
        .as_database_error()
        .and_then(|e| e.code().map(|c| c.into_owned()));
    assert_eq!(code.as_deref(), Some("55P03"), "lock_not_available");
    deleting.rollback().await.expect("roll back");
    reusing.commit().await.expect("commit");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn insert_refuses_a_mime_type_the_table_cannot_store(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let long = "x".repeat(101);
    for mime in ["ab", long.as_str(), "text/pl\0ain"] {
        let new = NewFile {
            mime: mime.to_owned(),
            ..new_file(0x55, 1)
        };
        let refused = insert(&pool, a, &new).await;
        assert!(matches!(refused, Err(Error::MimeInvalid)), "{mime:?}");
    }
    assert_eq!(row_count(&pool).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_storage_count_sums_the_scopes_files_once_each(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    assert_eq!(files::count_for_limit(&pool, scope(a)).await.unwrap(), 0);
    insert(&pool, a, &new_file(1, 900)).await.unwrap();
    insert(&pool, a, &new_file(2, 50)).await.unwrap();
    insert(&pool, b, &new_file(1, 900)).await.unwrap();
    assert_eq!(files::count_for_limit(&pool, scope(a)).await.unwrap(), 950);
    assert_eq!(files::count_for_limit(&pool, scope(b)).await.unwrap(), 900);
}

/// Put `org` on a test plan whose `storage` limit is `bytes`, by SQL (M1 has no API for it).
async fn limit_storage(pool: &PgPool, org: Uuid, bytes: u64) {
    let plan = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, limits)
         VALUES ($1, $2, jsonb_build_object('storage', $3::bigint))",
    )
    .bind(plan)
    .bind(format!("test_{}", plan.simple()))
    .bind(i64::try_from(bytes).expect("a test size"))
    .execute(pool)
    .await
    .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("move to the plan");
}

async fn storage_room(pool: &PgPool, org: Uuid, adding: u64) -> Room {
    let plan = org_plans::get_with_plan(pool, scope(org))
        .await
        .expect("read the plan")
        .entitlements;
    let mut tx = pool.begin().await.expect("begin");
    let room = org_plans::check_room(&mut tx, scope(org), &plan, Limit::Storage, adding, now())
        .await
        .expect("check_room");
    tx.rollback().await.expect("roll back");
    room
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_storage_limit_counts_bytes_already_stored(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    limit_storage(&pool, a, 1000).await;
    insert(&pool, a, &new_file(9, 900)).await.unwrap();
    // T3: 900 of 1,000 used, so 200 more do not fit, 100 do, and a reused file adds 0.
    assert_eq!(storage_room(&pool, a, 200).await, Room::Full { max: 1000 });
    assert_eq!(storage_room(&pool, a, 100).await, Room::Fits);
    assert_eq!(storage_room(&pool, a, 0).await, Room::Fits);
    // Another organization's bytes do not count against this one.
    limit_storage(&pool, b, 1000).await;
    assert_eq!(storage_room(&pool, b, 1000).await, Room::Fits);
    // The unlimited plan sets no storage limit.
    let c = org(&pool, "org-c").await;
    assert_eq!(storage_room(&pool, c, u64::MAX).await, Room::Fits);
}

/// A course of `org` under its root account.
async fn course(pool: &PgPool, org: Uuid, code: &str) -> Uuid {
    let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let new = NewCourse {
        account_id: root,
        title: format!("Course {code}"),
        code: code.into(),
    };
    courses::create(pool, scope(org), &new, now(), Actor::Instance)
        .await
        .expect("create course")
        .id
}

async fn add_course_file(
    pool: &PgPool,
    org: Uuid,
    course: Uuid,
    file: Uuid,
    folder: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO course_files (id, org_id, course_id, file_id, folder, name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(new_id())
    .bind(org)
    .bind(course)
    .bind(file)
    .bind(folder)
    .bind(name)
    .execute(pool)
    .await
    .map(|_| ())
}

fn constraint(error: sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|e| e.constraint().map(str::to_owned))
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_file_stays_in_its_organization_and_folder_rules(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let course_a = course(&pool, a, "A1").await;
    let course_b = course(&pool, b, "B1").await;
    let file_a = insert(&pool, a, &new_file(7, 10)).await.unwrap().id;
    add_course_file(&pool, a, course_a, file_a, "/", "notes.pdf")
        .await
        .unwrap();
    add_course_file(&pool, a, course_a, file_a, "/handouts/week-1", "notes.pdf")
        .await
        .unwrap();
    // The same file in two courses' folders is two course files and one `files` row (B7).
    let course_a2 = course(&pool, a, "A2").await;
    add_course_file(&pool, a, course_a2, file_a, "/", "notes.pdf")
        .await
        .unwrap();
    assert_eq!(row_count(&pool).await, 1);

    let taken = add_course_file(&pool, a, course_a, file_a, "/", "notes.pdf").await;
    assert_eq!(
        constraint(taken.unwrap_err()).as_deref(),
        Some("course_files_course_id_folder_name_key")
    );
    // Org B's row cannot name org A's file, nor org A's row org B's course.
    let foreign_file = add_course_file(&pool, b, course_b, file_a, "/", "x.pdf").await;
    assert_eq!(
        constraint(foreign_file.unwrap_err()).as_deref(),
        Some("course_files_org_id_file_id_fkey")
    );
    let foreign_course = add_course_file(&pool, a, course_b, file_a, "/", "x.pdf").await;
    assert_eq!(
        constraint(foreign_course.unwrap_err()).as_deref(),
        Some("course_files_org_id_course_id_fkey")
    );
    let long_segment = format!("/{}", "s".repeat(101));
    let long = "n".repeat(256);
    for (folder, name) in [
        ("/.", "x"),
        ("/..", "x"),
        ("/a/../b", "x"),
        ("/a/.", "x"),
        ("/a\\b", "x"),
        ("/a\tb", "x"),
        ("/a\u{7f}b", "x"),
        ("/a\u{85}b", "x"),
        (long_segment.as_str(), "x"),
        ("/", "."),
        ("/", ".."),
        ("/", "a\\b"),
        ("/", "a\nb"),
        ("/", "a\u{9f}b"),
        ("", "x"),
        ("handouts", "x"),
        ("/a/", "x"),
        ("/a//b", "x"),
        ("/1/2/3/4/5/6/7/8/9", "x"),
        ("/", ""),
        ("/", "a/b"),
        ("/", long.as_str()),
    ] {
        let refused = add_course_file(&pool, a, course_a, file_a, folder, name).await;
        assert!(refused.is_err(), "{folder:?} {name:?}");
    }
    // B9's edges that fit: 8 segments, a segment of 100 characters, dots inside a segment, a
    // name of 255 characters.
    let longest_segment = format!("/{}", "s".repeat(100));
    let name_255 = "n".repeat(255);
    for (folder, name) in [
        ("/1/2/3/4/5/6/7/8", "x"),
        (longest_segment.as_str(), "x"),
        ("/.hidden/v1.2/..x", "a.b"),
        ("/", name_255.as_str()),
        ("/notes", "..."),
    ] {
        add_course_file(&pool, a, course_a, file_a, folder, name)
            .await
            .unwrap_or_else(|e| panic!("{folder:?} {name:?}: {e}"));
    }
    // A referenced `files` row cannot be deleted (B21).
    let referenced = sqlx::query("DELETE FROM files WHERE id = $1")
        .bind(file_a)
        .execute(&pool)
        .await;
    assert_eq!(
        constraint(referenced.unwrap_err()).as_deref(),
        Some("course_files_org_id_file_id_fkey")
    );
}
