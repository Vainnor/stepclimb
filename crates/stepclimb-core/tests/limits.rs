// SPDX-License-Identifier: AGPL-3.0-or-later
//! The limit check, `org_plans::check_room`, against a fresh database per test
//! (docs/specs/m0/10-entitlements-framework.md B10, B11; T7's repository part): a limit with no
//! value runs no query, a limit with a value counts the scope's organization's users only, and
//! two creations at `max - 1` in one organization take turns, so only one fits. Needs
//! `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{PgPool, Postgres, Transaction};
use stepclimb_core::{
    OrgScope, Source,
    feature::Limit,
    id::{Uuid, new_id},
    org_plans::{self, Error, Room},
    organizations,
    plans::{self, PlanEntitlements},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers are not under test; tests stand in for one"
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

/// Put `org` on a test plan whose `users` limit is `users`, by SQL (M0 has no API for it).
async fn limit_users(pool: &PgPool, org: Uuid, users: &str) {
    let plan = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, limits) VALUES ($1, $2, jsonb_build_object('users', $3::jsonb))",
    )
    .bind(plan)
    .bind(format!("test_{}", plan.simple()))
    .bind(users)
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

async fn plan_of(pool: &PgPool, org: Uuid) -> PlanEntitlements {
    org_plans::get_with_plan(pool, scope(org))
        .await
        .expect("read the plan")
        .entitlements
}

fn new_user(name: &str) -> NewUser {
    NewUser {
        display_name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    }
}

async fn add_user(db: &mut Transaction<'_, Postgres>, org: Uuid, name: &str) {
    users::create(&mut **db, scope(org), &new_user(name), now())
        .await
        .expect("create user");
}

async fn add_users(pool: &PgPool, org: Uuid, count: usize) {
    let mut tx = pool.begin().await.expect("begin");
    for n in 0..count {
        add_user(&mut tx, org, &format!("User {n}")).await;
    }
    tx.commit().await.expect("commit");
}

async fn room_for(pool: &PgPool, org: Uuid, adding: u64) -> Result<Room, Error> {
    let plan = plan_of(pool, org).await;
    let mut tx = pool.begin().await.expect("begin");
    let room = org_plans::check_room(&mut tx, scope(org), &plan, Limit::Users, adding, now()).await;
    tx.rollback().await.expect("roll back");
    room
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_limit_without_a_value_fits_without_a_query(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let unlimited = plan_of(&pool, a).await;
    assert_eq!(unlimited.limit(Limit::Users), None);
    // With no `org_plans` row a query would answer `NotFound`; `Fits` shows none ran.
    sqlx::query("DELETE FROM org_plans WHERE org_id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let room =
        org_plans::check_room(&mut tx, scope(a), &unlimited, Limit::Users, u64::MAX, now()).await;
    assert_eq!(room.unwrap(), Room::Fits);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_limit_counts_the_scopes_users_up_to_its_value(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    limit_users(&pool, a, "2").await;
    // Org B's users never count against org A.
    add_users(&pool, b, 3).await;

    assert_eq!(room_for(&pool, a, 1).await.unwrap(), Room::Fits);
    assert_eq!(room_for(&pool, a, 2).await.unwrap(), Room::Fits);
    assert_eq!(room_for(&pool, a, 3).await.unwrap(), Room::Full { max: 2 });
    add_users(&pool, a, 1).await;
    assert_eq!(room_for(&pool, a, 1).await.unwrap(), Room::Fits);
    assert_eq!(room_for(&pool, a, 2).await.unwrap(), Room::Full { max: 2 });
    add_users(&pool, a, 1).await;
    assert_eq!(room_for(&pool, a, 1).await.unwrap(), Room::Full { max: 2 });
    // A suspended user still counts (spec 03's `count_for_limit`).
    sqlx::query("UPDATE users SET status = 'suspended' WHERE org_id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(room_for(&pool, a, 1).await.unwrap(), Room::Full { max: 2 });
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_limit_of_zero_refuses_the_first_user(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    limit_users(&pool, a, "0").await;
    assert_eq!(room_for(&pool, a, 1).await.unwrap(), Room::Full { max: 0 });
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_without_a_plan_row_has_no_room(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    limit_users(&pool, a, "5").await;
    let plan = plan_of(&pool, a).await;
    sqlx::query("DELETE FROM org_plans WHERE org_id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let room = org_plans::check_room(&mut tx, scope(a), &plan, Limit::Users, 1, now()).await;
    assert!(matches!(room, Err(Error::NotFound)), "{room:?}");
}

/// Two creations at `max - 1`: the second waits for the first to commit, then counts its user
/// and is refused. Without the lock both would count one user and both insert.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn concurrent_creations_take_turns(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    limit_users(&pool, a, "2").await;
    add_users(&pool, a, 1).await;
    let plan = plan_of(&pool, a).await;

    let mut first = pool.begin().await.unwrap();
    let room = org_plans::check_room(&mut first, scope(a), &plan, Limit::Users, 1, now()).await;
    assert_eq!(room.unwrap(), Room::Fits);
    add_user(&mut first, a, "First").await;

    let mut second = tokio::spawn({
        let (pool, plan) = (pool.clone(), plan.clone());
        async move {
            let mut tx = pool.begin().await.unwrap();
            let room =
                org_plans::check_room(&mut tx, scope(a), &plan, Limit::Users, 1, now()).await;
            if matches!(room, Ok(Room::Fits)) {
                add_user(&mut tx, a, "Second").await;
                tx.commit().await.unwrap();
            }
            room.unwrap()
        }
    });
    let waited = tokio::time::timeout(Duration::from_millis(500), &mut second).await;
    assert!(waited.is_err(), "the second check did not wait: {waited:?}");

    first.commit().await.unwrap();
    let second = tokio::time::timeout(Duration::from_secs(10), second)
        .await
        .expect("the second check finishes once the first commits")
        .expect("the task does not panic");
    assert_eq!(second, Room::Full { max: 2 });
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 2);
}

/// The lock is per organization: an open creation in org A does not hold up org B's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_open_creation_does_not_block_another_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    limit_users(&pool, a, "2").await;
    limit_users(&pool, b, "2").await;
    let plan_a = plan_of(&pool, a).await;

    let mut open = pool.begin().await.unwrap();
    let room = org_plans::check_room(&mut open, scope(a), &plan_a, Limit::Users, 1, now()).await;
    assert_eq!(room.unwrap(), Room::Fits);

    let other = tokio::time::timeout(Duration::from_secs(5), room_for(&pool, b, 1)).await;
    assert!(
        matches!(other, Ok(Ok(Room::Fits))),
        "org B waited on org A: {other:?}"
    );
    open.rollback().await.unwrap();
}

/// Above `READ COMMITTED` the snapshot predates the lock wait, so the count could miss a
/// concurrent creation: the check refuses to count at all.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_transaction_above_read_committed_is_refused(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    limit_users(&pool, a, "5").await;
    let plan = plan_of(&pool, a).await;
    for (level, set_level) in [
        (
            "repeatable read",
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        ),
        (
            "serializable",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
        ),
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql(set_level).execute(&mut *tx).await.unwrap();
        let checked = org_plans::check_room(&mut tx, scope(a), &plan, Limit::Users, 1, now()).await;
        assert!(
            matches!(&checked, Err(Error::IsolationLevel { found }) if found == level),
            "{level}: {checked:?}"
        );
        tx.rollback().await.unwrap();
    }
    // A limit with no value runs no query, so it has nothing to protect and passes at any level.
    let unlimited = plan_of(&pool, org(&pool, "org-b").await).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    let checked =
        org_plans::check_room(&mut tx, scope(a), &unlimited, Limit::Users, 1, now()).await;
    assert_eq!(checked.unwrap(), Room::Fits);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_unlimited_plan_has_no_users_limit_after_a_sync(pool: PgPool) {
    plans::sync_unlimited(&pool, now()).await.unwrap();
    let a = org(&pool, "org-a").await;
    add_users(&pool, a, 3).await;
    assert_eq!(room_for(&pool, a, 1_000_000).await.unwrap(), Room::Fits);
}
