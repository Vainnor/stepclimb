// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorization codes repository and the `oauth_authorization_codes` table against a fresh
//! database per test (docs/specs/m0/08-sessions-and-csrf.md B31, Data, Q10). Needs
//! `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    id::{Uuid, new_id},
    oauth_codes::{self, Error, LIFETIME, NewCode},
    users::{self, NewUser},
};

const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

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

fn code<'a>(user_id: Uuid, hash: &'a [u8; 32], challenge: &'a str) -> NewCode<'a> {
    NewCode {
        id: new_id(),
        user_id,
        client_id: "stepclimb-app",
        code_hash: hash,
        redirect_uri: "http://127.0.0.1:43210/oauth/callback",
        code_challenge: challenge,
    }
}

async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_authorization_codes")
        .fetch_one(pool)
        .await
        .expect("count codes")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_code_is_stored_for_sixty_seconds_in_its_organization(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let expires = oauth_codes::create(
        &pool,
        scope(org),
        &code(user, &[1; 32], CHALLENGE),
        start_time(),
    )
    .await
    .expect("store");
    assert_eq!(expires, start_time() + LIFETIME);
    let row: (Uuid, Uuid, i64) = sqlx::query_as(
        "SELECT org_id, user_id, (extract(epoch FROM expires_at))::bigint FROM oauth_authorization_codes",
    )
    .fetch_one(&pool)
    .await
    .expect("the row");
    assert_eq!(row, (org, user, 1_763_000_060));
}

/// Tenancy: a user of another organization stores nothing, and the caller's transaction is not
/// aborted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_user_stores_nothing(pool: PgPool) {
    let org_a = insert_org(&pool, "org-a").await;
    let org_b = insert_org(&pool, "org-b").await;
    let kim = insert_user(&pool, org_b).await;
    let mut tx = pool.begin().await.expect("a transaction");
    let refused = oauth_codes::create(
        &mut *tx,
        scope(org_a),
        &code(kim, &[1; 32], CHALLENGE),
        start_time(),
    )
    .await;
    assert!(matches!(refused, Err(Error::UserNotFound)), "{refused:?}");
    sqlx::query("SELECT 1")
        .execute(&mut *tx)
        .await
        .expect("the transaction lives");
    tx.commit().await.expect("commit");
    assert_eq!(count(&pool).await, 0);
}

/// The table refuses what the server never writes: a challenge that is not 43 base64url
/// characters, a hash that is not 32 bytes, and a second row with the same hash.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_holds_only_s256_challenges_and_unique_hashes(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    for challenge in [
        "plain-verifier",
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM",
        "",
    ] {
        let refused = oauth_codes::create(
            &pool,
            scope(org),
            &code(user, &[2; 32], challenge),
            start_time(),
        )
        .await;
        assert!(
            matches!(refused, Err(Error::Database(_))),
            "{challenge}: {refused:?}"
        );
    }
    oauth_codes::create(
        &pool,
        scope(org),
        &code(user, &[3; 32], CHALLENGE),
        start_time(),
    )
    .await
    .expect("store");
    let again = oauth_codes::create(
        &pool,
        scope(org),
        &code(user, &[3; 32], CHALLENGE),
        start_time(),
    )
    .await;
    assert!(matches!(again, Err(Error::Database(_))), "{again:?}");
    let short = sqlx::query(
        "INSERT INTO oauth_authorization_codes (id, org_id, user_id, client_id, code_hash,
                                                redirect_uri, code_challenge, expires_at)
         VALUES ($1, $2, $3, 'stepclimb-app', '\\x01', 'x', $4, now())",
    )
    .bind(new_id())
    .bind(org)
    .bind(user)
    .bind(CHALLENGE)
    .execute(&pool)
    .await;
    assert!(short.is_err());
    assert_eq!(count(&pool).await, 1);
}

/// The cleanup deletes the scope's expired codes only: a live one and another organization's
/// stay.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn delete_expired_removes_only_the_scopes_expired_codes(pool: PgPool) {
    let org_a = insert_org(&pool, "org-a").await;
    let org_b = insert_org(&pool, "org-b").await;
    let sam = insert_user(&pool, org_a).await;
    let kim = insert_user(&pool, org_b).await;
    let later = start_time() + Duration::from_secs(30);
    oauth_codes::create(
        &pool,
        scope(org_a),
        &code(sam, &[1; 32], CHALLENGE),
        start_time(),
    )
    .await
    .expect("store");
    oauth_codes::create(&pool, scope(org_a), &code(sam, &[2; 32], CHALLENGE), later)
        .await
        .expect("store");
    oauth_codes::create(
        &pool,
        scope(org_b),
        &code(kim, &[3; 32], CHALLENGE),
        start_time(),
    )
    .await
    .expect("store");
    let at = start_time() + LIFETIME;
    assert_eq!(
        oauth_codes::delete_expired(&pool, scope(org_a), at)
            .await
            .expect("delete"),
        1
    );
    assert_eq!(count(&pool).await, 2);
    assert_eq!(
        oauth_codes::delete_expired(&pool, scope(org_b), at)
            .await
            .expect("delete"),
        1
    );
    assert_eq!(count(&pool).await, 1);
}
