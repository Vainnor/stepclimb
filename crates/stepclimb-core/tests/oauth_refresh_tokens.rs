// SPDX-License-Identifier: AGPL-3.0-or-later
//! The refresh tokens repository, the code exchange's reads and writes on
//! `oauth_authorization_codes`, and the `oauth_refresh_tokens` table against a fresh database per
//! test (docs/specs/m0/08-sessions-and-csrf.md B32 to B36, Data, Q11, Q12, Q14). Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, ScopeOrigin, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    oauth_codes::{self, NewCode},
    oauth_refresh_tokens::{
        self as refresh, AppTokenAuthentication, FAMILY_LIFETIME, LIFETIME, NewToken, Reuse,
        Reused, Rotation,
    },
    users::{self, NewUser, Status},
};

const CLIENT: &str = "stepclimb-app";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7));

fn start_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
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
    users::create(pool, scope(org), &user, start_time())
        .await
        .expect("create user")
        .id
}

fn token(user_id: Uuid, hash: &[u8; 32]) -> NewToken<'_> {
    NewToken {
        id: new_id(),
        user_id,
        client_id: CLIENT,
        token_hash: hash,
    }
}

/// A family started for `user` at `at`, with its first token's hash `[first; 32]`.
async fn family(pool: &PgPool, org: Uuid, user: Uuid, first: u8, at: SystemTime) -> Uuid {
    let hash = [first; 32];
    let new = token(user, &hash);
    let mut conn = pool.acquire().await.expect("a connection");
    refresh::start_family(&mut conn, scope(org), &new, at)
        .await
        .expect("start a family");
    new.id
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_refresh_tokens")
        .fetch_one(pool)
        .await
        .expect("count tokens")
}

async fn lock(pool: &PgPool, org: Uuid, hash: u8) -> Option<refresh::LockedToken> {
    let mut conn = pool.acquire().await.expect("a connection");
    refresh::lock(&mut conn, scope(org), &[hash; 32])
        .await
        .expect("lock")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_family_starts_with_its_own_id_and_lives_thirty_days(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let id = family(&pool, org, user, 1, start_time()).await;
    let locked = lock(&pool, org, 1).await.expect("the token");
    assert_eq!(locked.id, id);
    assert_eq!(locked.family_id, id);
    assert_eq!(locked.user_id, user);
    assert_eq!(locked.client_id, CLIENT);
    assert_eq!(locked.expires_at, start_time() + LIFETIME);
    assert_eq!(locked.family_started_at, start_time());
    assert_eq!(locked.user_status, Status::Active);
    let rotated_from: Option<Uuid> =
        sqlx::query_scalar("SELECT rotated_from FROM oauth_refresh_tokens")
            .fetch_one(&pool)
            .await
            .expect("read");
    assert_eq!(rotated_from, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_user_or_token_is_not_found(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user_b = insert_user(&pool, b).await;
    let mut conn = pool.acquire().await.expect("a connection");
    let refused =
        refresh::start_family(&mut conn, scope(a), &token(user_b, &[1; 32]), start_time()).await;
    assert!(matches!(refused, Err(refresh::Error::UserNotFound)));
    drop(conn);
    let family_b = family(&pool, b, user_b, 2, start_time()).await;
    assert_eq!(
        refresh::revoke_family(&pool, scope(a), family_b)
            .await
            .expect("revoke"),
        0
    );
    assert!(lock(&pool, a, 2).await.is_none());
    assert!(lock(&pool, b, 2).await.is_some());
    assert!(
        !refresh::revoke_by_token_hash(&pool, scope(a), &[2; 32], CLIENT)
            .await
            .expect("revoke")
    );
    assert_eq!(count(&pool).await, 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_token_rotates_once_within_its_family(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let first = family(&pool, org, user, 1, start_time()).await;
    let presented = lock(&pool, org, 1).await.expect("R1");
    let later = start_time() + Duration::from_secs(3600);
    let mut conn = pool.acquire().await.expect("a connection");
    assert!(
        !refresh::is_rotated(&mut conn, scope(org), first)
            .await
            .expect("read")
    );
    let next = token(user, &[2; 32]);
    let rotated = refresh::rotate(&mut conn, scope(org), &presented, &next, later)
        .await
        .expect("rotate");
    assert_eq!(rotated, Rotation::Rotated(later + LIFETIME));
    assert!(
        refresh::is_rotated(&mut conn, scope(org), first)
            .await
            .expect("read")
    );
    let again = refresh::rotate(
        &mut conn,
        scope(org),
        &presented,
        &token(user, &[3; 32]),
        later,
    )
    .await
    .expect("rotate again");
    assert_eq!(again, Rotation::AlreadyRotated);
    drop(conn);
    let second = lock(&pool, org, 2).await.expect("R2");
    assert_eq!(second.family_id, first);
    assert_eq!(second.family_started_at, start_time());
    let from: Option<Uuid> =
        sqlx::query_scalar("SELECT rotated_from FROM oauth_refresh_tokens WHERE id = $1")
            .bind(next.id)
            .fetch_one(&pool)
            .await
            .expect("read");
    assert_eq!(from, Some(first));
    assert_eq!(count(&pool).await, 2);
}

/// Q11 (b): a token rotated late in the family's life expires with the family, and one rotated
/// at its end stores nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn no_token_outlives_the_family_cap(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    family(&pool, org, user, 1, start_time()).await;
    let presented = lock(&pool, org, 1).await.expect("R1");
    let cap = start_time() + FAMILY_LIFETIME;
    let late = cap - Duration::from_secs(86_400);
    let mut conn = pool.acquire().await.expect("a connection");
    let rotated = refresh::rotate(
        &mut conn,
        scope(org),
        &presented,
        &token(user, &[2; 32]),
        late,
    )
    .await
    .expect("rotate");
    assert_eq!(rotated, Rotation::Rotated(cap));
    drop(conn);
    let second = lock(&pool, org, 2).await.expect("R2");
    let mut conn = pool.acquire().await.expect("a connection");
    let at_cap = refresh::rotate(&mut conn, scope(org), &second, &token(user, &[3; 32]), cap)
        .await
        .expect("rotate at the cap");
    assert_eq!(at_cap, Rotation::FamilyExpired);
    assert_eq!(count(&pool).await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_a_second_rotation_and_a_broken_family(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let first = family(&pool, org, user, 1, start_time()).await;
    let insert = |hash: u8, family: Uuid, from: Option<Uuid>| {
        let pool = pool.clone();
        async move {
            let id = new_id();
            sqlx::query(
                "INSERT INTO oauth_refresh_tokens
                     (id, org_id, client_id, user_id, token_hash, family_id, rotated_from, expires_at)
                 VALUES ($1, $2, 'stepclimb-app', $3, $4, $5, $6, now())",
            )
            .bind(id)
            .bind(org)
            .bind(user)
            .bind(vec![hash; 32])
            .bind(family)
            .bind(from)
            .execute(&pool)
            .await
            .map(|_| id)
        }
    };
    insert(2, first, Some(first)).await.expect("one rotation");
    assert!(
        insert(3, first, Some(first)).await.is_err(),
        "a second rotation"
    );
    assert!(
        insert(4, first, None).await.is_err(),
        "a second first token"
    );
    assert!(
        insert(5, new_id(), None).await.is_err(),
        "a family of nothing"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn revoking_a_family_deletes_every_token_of_it_and_no_other(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let first = family(&pool, org, user, 1, start_time()).await;
    let presented = lock(&pool, org, 1).await.expect("R1");
    let mut conn = pool.acquire().await.expect("a connection");
    refresh::rotate(
        &mut conn,
        scope(org),
        &presented,
        &token(user, &[2; 32]),
        start_time(),
    )
    .await
    .expect("rotate");
    drop(conn);
    family(&pool, org, user, 9, start_time()).await;
    assert_eq!(
        refresh::revoke_family(&pool, scope(org), first)
            .await
            .expect("revoke"),
        2
    );
    assert!(lock(&pool, org, 1).await.is_none());
    assert!(lock(&pool, org, 2).await.is_none());
    assert!(lock(&pool, org, 9).await.is_some());
}

/// B35: revoking by any token of a family, spent or not, deletes the family, for its client
/// only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn revoking_by_a_spent_token_deletes_its_family(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    family(&pool, org, user, 1, start_time()).await;
    let presented = lock(&pool, org, 1).await.expect("R1");
    let mut conn = pool.acquire().await.expect("a connection");
    refresh::rotate(
        &mut conn,
        scope(org),
        &presented,
        &token(user, &[2; 32]),
        start_time(),
    )
    .await
    .expect("rotate");
    drop(conn);
    assert!(
        !refresh::revoke_by_token_hash(&pool, scope(org), &[1; 32], "other-app")
            .await
            .expect("revoke")
    );
    assert_eq!(count(&pool).await, 2);
    assert!(
        refresh::revoke_by_token_hash(&pool, scope(org), &[1; 32], CLIENT)
            .await
            .expect("revoke")
    );
    assert_eq!(count(&pool).await, 0);
    assert!(
        !refresh::revoke_by_token_hash(&pool, scope(org), &[1; 32], CLIENT)
            .await
            .expect("revoke an unknown token")
    );
}

/// Q14: reuse revokes the family and writes one entry naming it, never a token or hash.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn reuse_revokes_the_family_and_is_audited(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let first = family(&pool, org, user, 1, start_time()).await;
    let mut tx = pool.begin().await.expect("a transaction");
    let reuse = Reuse {
        family_id: first,
        user_id: user,
        client_id: CLIENT,
        presented: Reused::RefreshToken,
        client_address: ADDRESS,
    };
    let deleted = refresh::revoke_reused(&mut tx, scope(org), &reuse, start_time())
        .await
        .expect("revoke");
    tx.commit().await.expect("commit");
    assert_eq!(deleted, 1);
    assert_eq!(count(&pool).await, 0);
    let entries = audit::list(&pool, scope(org), 10).await.expect("entries");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.actor, Actor::Instance);
    assert_eq!(entry.action, "refresh_token.reuse");
    assert_eq!(entry.target_type, "refresh_token");
    assert_eq!(entry.target_id, first);
    assert_eq!(
        entry.data,
        serde_json::json!({
            "user_id": user,
            "client_id": CLIENT,
            "presented": "refresh_token",
            "client_address": "192.0.2.7",
            "tokens_deleted": 1,
        })
    );
}

/// Q12 (b): an access token's family is checked on each request, for its user and client, in
/// the request's organization, with the user's status.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_app_token_family_authenticates_while_it_exists(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = insert_user(&pool, a).await;
    let other = insert_user(&pool, a).await;
    let first = family(&pool, a, user, 1, start_time()).await;
    let check = |org: Uuid, user: Uuid, family: Uuid, client: &'static str| {
        let pool = pool.clone();
        async move {
            refresh::authenticate(&pool, scope(org), user, family, client)
                .await
                .expect("authenticate")
        }
    };
    let AppTokenAuthentication::Valid(principal) = check(a, user, first, CLIENT).await else {
        panic!("the family authenticates");
    };
    assert_eq!(principal.user_id, user);
    assert_eq!(principal.scope.org_id(), a);
    assert_eq!(principal.scope.origin(), ScopeOrigin::AppToken);
    for (org, who, fam, client) in [
        (b, user, first, CLIENT),
        (a, other, first, CLIENT),
        (a, user, new_id(), CLIENT),
        (a, user, first, "other-app"),
    ] {
        assert_eq!(
            check(org, who, fam, client).await,
            AppTokenAuthentication::Unknown
        );
    }
    sqlx::query("UPDATE users SET status = 'suspended' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("suspend");
    assert_eq!(
        check(a, user, first, CLIENT).await,
        AppTokenAuthentication::Suspended
    );
    refresh::revoke_family(&pool, scope(a), first)
        .await
        .expect("revoke");
    assert_eq!(
        check(a, user, first, CLIENT).await,
        AppTokenAuthentication::Unknown
    );
}

/// The cleanup deletes a family once its newest token expired, and keeps a family with a live
/// token, spent tokens and all.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_cleanup_deletes_families_with_no_live_token(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = insert_user(&pool, a).await;
    let user_b = insert_user(&pool, b).await;
    family(&pool, a, user, 1, start_time()).await;
    let rotated_at = start_time() + Duration::from_secs(86_400);
    family(&pool, a, user, 3, start_time()).await;
    let presented = lock(&pool, a, 3).await.expect("R1");
    let mut conn = pool.acquire().await.expect("a connection");
    refresh::rotate(
        &mut conn,
        scope(a),
        &presented,
        &token(user, &[4; 32]),
        rotated_at,
    )
    .await
    .expect("rotate");
    drop(conn);
    family(&pool, b, user_b, 5, start_time()).await;
    let at_first_expiry = start_time() + LIFETIME;
    assert_eq!(
        refresh::delete_expired(&pool, scope(a), at_first_expiry)
            .await
            .expect("clean up"),
        1
    );
    assert!(lock(&pool, a, 1).await.is_none());
    assert!(
        lock(&pool, a, 3).await.is_some(),
        "a spent token of a live family stays"
    );
    assert!(
        lock(&pool, b, 5).await.is_some(),
        "another organization's is not touched"
    );
    assert_eq!(
        refresh::delete_expired(&pool, scope(a), rotated_at + LIFETIME)
            .await
            .expect("clean up"),
        2
    );
}

/// B32: the exchange locks the code with its user's status, and records its use once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_code_is_locked_with_its_user_and_marked_used_once(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = insert_user(&pool, a).await;
    let code = NewCode {
        id: new_id(),
        user_id: user,
        client_id: CLIENT,
        code_hash: &[7; 32],
        redirect_uri: "http://127.0.0.1:43210/oauth/callback",
        code_challenge: CHALLENGE,
    };
    oauth_codes::create(&pool, scope(a), &code, start_time())
        .await
        .expect("store");
    let mut conn = pool.acquire().await.expect("a connection");
    assert!(
        oauth_codes::lock(&mut conn, scope(b), &[7; 32])
            .await
            .expect("lock")
            .is_none()
    );
    let locked = oauth_codes::lock(&mut conn, scope(a), &[7; 32])
        .await
        .expect("lock")
        .expect("the code");
    assert_eq!(locked.id, code.id);
    assert_eq!(locked.user_id, user);
    assert_eq!(locked.redirect_uri, code.redirect_uri);
    assert_eq!(locked.code_challenge, CHALLENGE);
    assert_eq!(locked.expires_at, start_time() + oauth_codes::LIFETIME);
    assert_eq!((locked.used_at, locked.family_id), (None, None));
    assert_eq!(locked.user_status, Status::Active);
    drop(conn);
    let first = family(&pool, a, user, 1, start_time()).await;
    let mut conn = pool.acquire().await.expect("a connection");
    assert!(
        !oauth_codes::mark_used(&mut conn, scope(b), code.id, first, start_time())
            .await
            .expect("mark in another organization")
    );
    assert!(
        oauth_codes::mark_used(&mut conn, scope(a), code.id, first, start_time())
            .await
            .expect("mark")
    );
    assert!(
        !oauth_codes::mark_used(&mut conn, scope(a), code.id, first, start_time())
            .await
            .expect("mark again")
    );
    let used = oauth_codes::lock(&mut conn, scope(a), &[7; 32])
        .await
        .expect("lock")
        .expect("the code");
    assert_eq!(used.used_at, Some(start_time()));
    assert_eq!(used.family_id, Some(first));
    drop(conn);
    refresh::revoke_family(&pool, scope(a), first)
        .await
        .expect("revoke");
    let mut conn = pool.acquire().await.expect("a connection");
    let after = oauth_codes::lock(&mut conn, scope(a), &[7; 32])
        .await
        .expect("lock")
        .expect("the code stays");
    assert_eq!((after.used_at, after.family_id), (Some(start_time()), None));
}

/// B34, Q11 (strict): two refreshes of one token in two transactions. A locks the token and
/// rotates it without committing; B's lock waits for A; once A commits, B holds the lock and
/// `is_rotated`, a statement after the lock, sees A's new token, so B treats the token as spent.
/// A rotation of the spent token stores nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_concurrent_refresh_waits_and_then_sees_the_token_spent(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let first = family(&pool, org, user, 1, start_time()).await;

    let mut a = pool.begin().await.expect("transaction A");
    let held = refresh::lock(&mut a, scope(org), &[1; 32])
        .await
        .expect("lock")
        .expect("R1");
    let rotated = refresh::rotate(
        &mut a,
        scope(org),
        &held,
        &token(user, &[2; 32]),
        start_time(),
    )
    .await
    .expect("rotate");
    assert!(matches!(rotated, Rotation::Rotated(_)));

    let b_pool = pool.clone();
    let mut b = tokio::spawn(async move {
        let mut b = b_pool.begin().await.expect("transaction B");
        let locked = refresh::lock(&mut b, scope(org), &[1; 32])
            .await
            .expect("lock")
            .expect("R1");
        let spent = refresh::is_rotated(&mut b, scope(org), locked.id)
            .await
            .expect("read");
        let again = refresh::rotate(
            &mut b,
            scope(org),
            &locked,
            &token(user, &[3; 32]),
            start_time(),
        )
        .await
        .expect("rotate");
        b.commit().await.expect("commit B");
        (spent, again)
    });
    let waited = tokio::time::timeout(Duration::from_millis(300), &mut b).await;
    assert!(waited.is_err(), "B's lock waits for A");
    a.commit().await.expect("commit A");
    let (spent, again) = b.await.expect("B finishes");
    assert!(spent, "B sees the token A rotated");
    assert_eq!(again, Rotation::AlreadyRotated);
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oauth_refresh_tokens WHERE family_id = $1")
            .bind(first)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(rows, 2);
}

/// Spec 08 B46: a change that ends the user's other sessions deletes every refresh token of the
/// user, in every family, and no other user's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_family_of_a_user_can_be_revoked_at_once(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = insert_user(&pool, a).await;
    let other = insert_user(&pool, a).await;
    let user_b = insert_user(&pool, b).await;
    family(&pool, a, user, 1, start_time()).await;
    family(&pool, a, user, 2, start_time()).await;
    family(&pool, a, other, 3, start_time()).await;
    family(&pool, b, user_b, 4, start_time()).await;
    assert_eq!(
        refresh::revoke_all_of_user(&pool, scope(b), user)
            .await
            .expect("revoke elsewhere"),
        0
    );
    assert_eq!(
        refresh::revoke_all_of_user(&pool, scope(a), user)
            .await
            .expect("revoke"),
        2
    );
    assert!(lock(&pool, a, 1).await.is_none());
    assert!(lock(&pool, a, 2).await.is_none());
    assert!(lock(&pool, a, 3).await.is_some());
    assert!(lock(&pool, b, 4).await.is_some());
}
