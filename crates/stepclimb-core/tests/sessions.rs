// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sessions repository and the `sessions` table against a fresh database per test
//! (docs/specs/m0/08-sessions-and-csrf.md B6 to B11, Q4; T4, T6, T7, T9, T10 at the repository
//! level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, ScopeOrigin, Source,
    id::{Uuid, new_id},
    organizations::{self, Tenancy},
    sessions::{self, Authentication, Error, IDLE_TIMEOUT, MAX_LIFETIME},
    users::{self, NewUser, Status},
};

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded write shows.
fn sign_in_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the request-organization resolver"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

async fn insert_user(pool: &PgPool, org: Uuid) -> Uuid {
    let user = NewUser {
        display_name: "Sam Ortiz".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    users::create(pool, scope(org), &user, sign_in_time())
        .await
        .expect("create user")
        .id
}

/// A distinct 32-byte hash per call (the repository never sees a token).
fn hash(seed: u8) -> [u8; 32] {
    [seed; 32]
}

/// An organization with one user who signed in at [`sign_in_time`] with `hash(1)`.
async fn signed_in(pool: &PgPool) -> (Uuid, Uuid, sessions::Session) {
    let org = insert_org(pool, "org-a").await;
    let user = insert_user(pool, org).await;
    let session = sessions::create(pool, scope(org), user, &hash(1), sign_in_time())
        .await
        .expect("create session");
    (org, user, session)
}

async fn authenticate(pool: &PgPool, org: Uuid, at: SystemTime) -> Authentication {
    sessions::authenticate(pool, scope(org), &hash(1), at)
        .await
        .expect("authenticate")
}

/// The stored `last_seen_at` of `id` in microseconds since the epoch, or `None` when the row is
/// gone.
async fn last_seen_us(pool: &PgPool, id: Uuid) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM last_seen_at) * 1000000)::bigint FROM sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .expect("read session")
}

fn micros(time: SystemTime) -> i64 {
    i64::try_from(
        time.duration_since(UNIX_EPOCH)
            .expect("a time after the epoch")
            .as_micros(),
    )
    .expect("a time that fits in i64 microseconds")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_stores_a_browser_session_for_thirty_days(pool: PgPool) {
    let (_, user, session) = signed_in(&pool).await;

    assert_eq!(session.user_id, user);
    assert_eq!(session.created_at, sign_in_time());
    assert_eq!(session.last_seen_at, sign_in_time());
    assert_eq!(session.expires_at, sign_in_time() + MAX_LIFETIME, "T4");
    assert_eq!(session.id.get_version_num(), 7);
    let (kind, stored): (String, Vec<u8>) =
        sqlx::query_as("SELECT kind, token_hash FROM sessions WHERE id = $1")
            .bind(session.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "browser");
    assert_eq!(stored, hash(1));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_a_user_of_another_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user_b = insert_user(&pool, b).await;

    let result = sessions::create(&pool, scope(a), user_b, &hash(1), sign_in_time()).await;

    assert!(matches!(result, Err(Error::UserNotFound)), "{result:?}");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_anything_but_a_32_byte_hash(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    // A token's 43-character text where its hash belongs.
    let result = sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, kind, last_seen_at, expires_at)
         VALUES ($1, $2, convert_to('AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8', 'UTF8'),
                 'browser', now(), now())",
    )
    .bind(new_id())
    .bind(user)
    .execute(&pool)
    .await;
    let code = result
        .expect_err("refused")
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned());
    assert_eq!(code.as_deref(), Some("23514"), "check_violation");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_valid_session_yields_its_user_and_a_session_scope(pool: PgPool) {
    let (org, user, session) = signed_in(&pool).await;

    let Authentication::Valid(principal) = authenticate(&pool, org, sign_in_time()).await else {
        panic!("the session is valid");
    };

    assert_eq!(principal.user_id, user);
    assert_eq!(principal.session_id, session.id);
    assert_eq!(principal.scope.org_id(), org);
    assert_eq!(principal.scope.origin(), ScopeOrigin::Session);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_hash_is_unknown(pool: PgPool) {
    let (org, _, _) = signed_in(&pool).await;
    let result = sessions::authenticate(&pool, scope(org), &hash(2), sign_in_time())
        .await
        .unwrap();
    assert_eq!(result, Authentication::Unknown);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_a_session_presented_in_another_organization_is_unknown_and_untouched(pool: PgPool) {
    let (_, _, session) = signed_in(&pool).await;
    let b = insert_org(&pool, "org-b").await;

    // Late enough to be idle-expired and to need a touch: neither may happen through org B.
    let result = authenticate(&pool, b, sign_in_time() + 13 * HOUR).await;

    assert_eq!(result, Authentication::Unknown);
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        Some(micros(sign_in_time())),
        "org B's request neither deleted nor touched org A's session"
    );
    let deleted = sessions::delete(&pool, scope(b), session.id).await.unwrap();
    assert!(!deleted, "org B's scope cannot delete it either");
    assert!(last_seen_us(&pool, session.id).await.is_some());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_twelve_idle_hours_expire_and_delete_the_session(pool: PgPool) {
    let (org, _, session) = signed_in(&pool).await;

    let almost = authenticate(&pool, org, sign_in_time() + 12 * HOUR - MINUTE).await;
    assert!(matches!(almost, Authentication::Valid(_)), "at 11h59m");
    let last_seen = sign_in_time() + 12 * HOUR - MINUTE;
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        Some(micros(last_seen))
    );

    let idle = authenticate(&pool, org, last_seen + IDLE_TIMEOUT).await;
    assert_eq!(idle, Authentication::Expired, "at last_seen_at + 12h");
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        None,
        "the row is gone"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn last_seen_is_written_once_a_minute_at_most(pool: PgPool) {
    let (org, _, session) = signed_in(&pool).await;

    authenticate(&pool, org, sign_in_time() + MINUTE - Duration::from_secs(1)).await;
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        Some(micros(sign_in_time())),
        "59 s later: no write"
    );
    authenticate(&pool, org, sign_in_time() + MINUTE).await;
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        Some(micros(sign_in_time() + MINUTE)),
        "60 s later: written"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_hourly_use_keeps_a_session_until_thirty_days_and_no_longer(pool: PgPool) {
    let (org, _, session) = signed_in(&pool).await;
    let end = session.created_at + MAX_LIFETIME;

    let mut at = sign_in_time() + HOUR;
    while at < end {
        let result = authenticate(&pool, org, at).await;
        assert!(matches!(result, Authentication::Valid(_)), "at {at:?}");
        at += HOUR;
    }
    let last_moment = authenticate(&pool, org, end - Duration::from_micros(1)).await;
    assert!(matches!(last_moment, Authentication::Valid(_)));

    assert_eq!(authenticate(&pool, org, end).await, Authentication::Expired);
    assert_eq!(last_seen_us(&pool, session.id).await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_suspended_user_is_refused_and_restored_by_unsuspending(pool: PgPool) {
    let (org, user, session) = signed_in(&pool).await;
    users::set_status(&pool, scope(org), user, Status::Suspended, sign_in_time())
        .await
        .unwrap();

    let later = sign_in_time() + HOUR;
    assert_eq!(
        authenticate(&pool, org, later).await,
        Authentication::Suspended
    );
    assert_eq!(
        last_seen_us(&pool, session.id).await,
        Some(micros(sign_in_time())),
        "the row is kept and not refreshed"
    );

    users::set_status(&pool, scope(org), user, Status::Active, later)
        .await
        .unwrap();
    assert!(matches!(
        authenticate(&pool, org, later).await,
        Authentication::Valid(_)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_users_session_still_expires(pool: PgPool) {
    let (org, user, session) = signed_in(&pool).await;
    users::set_status(&pool, scope(org), user, Status::Suspended, sign_in_time())
        .await
        .unwrap();

    let idle = authenticate(&pool, org, sign_in_time() + IDLE_TIMEOUT).await;

    assert_eq!(idle, Authentication::Expired);
    assert_eq!(last_seen_us(&pool, session.id).await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn delete_removes_the_session_of_the_scope(pool: PgPool) {
    let (org, _, session) = signed_in(&pool).await;

    assert!(
        sessions::delete(&pool, scope(org), session.id)
            .await
            .unwrap()
    );
    assert!(
        !sessions::delete(&pool, scope(org), session.id)
            .await
            .unwrap()
    );
    assert_eq!(
        authenticate(&pool, org, sign_in_time()).await,
        Authentication::Unknown
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_the_user_deletes_their_sessions(pool: PgPool) {
    let (_, user, session) = signed_in(&pool).await;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(last_seen_us(&pool, session.id).await, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn tenancy_counts_up_to_two_organizations(pool: PgPool) {
    assert_eq!(organizations::tenancy(&pool).await.unwrap(), Tenancy::Empty);
    let a = insert_org(&pool, "org-a").await;
    assert_eq!(
        organizations::tenancy(&pool).await.unwrap(),
        Tenancy::Single(a)
    );
    insert_org(&pool, "org-b").await;
    assert_eq!(
        organizations::tenancy(&pool).await.unwrap(),
        Tenancy::Multiple
    );
}
