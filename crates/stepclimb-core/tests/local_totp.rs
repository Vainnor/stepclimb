// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TOTP repository and its columns against a fresh database per test
//! (docs/specs/m0/04-local-accounts.md B19 to B21, Data, Q11). The secrets here are opaque
//! bytes: sealing them is the server's. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    local_logins,
    local_totp::{self, Error},
    organizations,
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the TOTP routes stand in for the credential resolver here"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

const HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g";
const SEALED: &[u8] = b"sealed-secret-one";
const OTHER_SEALED: &[u8] = b"sealed-secret-two";

fn actor(user_id: Uuid) -> Actor {
    Actor::User {
        id: user_id,
        ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
    }
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Example", now())
        .await
        .expect("create organization")
        .id
}

async fn user(pool: &PgPool, org: Uuid) -> Uuid {
    let user = NewUser {
        display_name: "Sam Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: Some(format!("{}@example.test", new_id())),
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    users::create(pool, scope(org), &user, now())
        .await
        .expect("create user")
        .id
}

async fn with_password(pool: &PgPool, org: Uuid) -> Uuid {
    let id = user(pool, org).await;
    local_logins::set_password(pool, scope(org), id, HASH, now(), Actor::Instance)
        .await
        .expect("set the password");
    id
}

async fn start(pool: &PgPool, org: Uuid, user_id: Uuid, sealed: &[u8]) -> Result<(), Error> {
    let mut conn = pool.acquire().await.expect("a connection");
    local_totp::start(&mut conn, scope(org), user_id, sealed, now()).await
}

async fn read(pool: &PgPool, org: Uuid, user_id: Uuid) -> Option<local_totp::Totp> {
    let mut tx = pool.begin().await.expect("a transaction");
    local_totp::lock(&mut tx, scope(org), user_id)
        .await
        .expect("read the TOTP")
}

async fn confirm(
    pool: &PgPool,
    org: Uuid,
    user_id: Uuid,
    hashes: &[[u8; 32]],
) -> Result<(), Error> {
    let mut conn = pool.acquire().await.expect("a connection");
    local_totp::confirm(
        &mut conn,
        scope(org),
        user_id,
        hashes,
        now(),
        actor(user_id),
    )
    .await
}

/// B19: only a user with a password starts TOTP; starting again before confirming replaces
/// the secret; once confirmed, starting is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn starting_needs_a_password_and_replaces_only_an_unconfirmed_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let bare = user(&pool, a).await;
    assert!(matches!(
        start(&pool, a, bare, SEALED).await,
        Err(Error::NoPassword)
    ));
    assert!(read(&pool, a, bare).await.is_none());

    let sam = with_password(&pool, a).await;
    start(&pool, a, sam, SEALED).await.unwrap();
    start(&pool, a, sam, OTHER_SEALED).await.unwrap();
    let totp = read(&pool, a, sam).await.unwrap();
    assert_eq!(totp.sealed_secret.as_deref(), Some(OTHER_SEALED));
    assert!(!totp.confirmed);

    confirm(&pool, a, sam, &[[1; 32]]).await.unwrap();
    assert!(matches!(
        start(&pool, a, sam, SEALED).await,
        Err(Error::AlreadyConfirmed)
    ));
    assert_eq!(
        read(&pool, a, sam).await.unwrap().sealed_secret.as_deref(),
        Some(OTHER_SEALED)
    );
}

/// The table refuses a secret on a login without a password, and a confirmation without a
/// secret, whatever writes them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_columns_refuse_totp_without_a_password_or_a_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = with_password(&pool, a).await;
    for sql in [
        "UPDATE local_logins SET totp_secret = '\\x01', password_hash = NULL, password_changed_at = NULL WHERE user_id = $1",
        "UPDATE local_logins SET totp_confirmed_at = now() WHERE user_id = $1",
    ] {
        let refused = sqlx::query(sql).bind(sam).execute(&pool).await.unwrap_err();
        let constraint = refused.as_database_error().and_then(|e| e.constraint());
        assert!(
            matches!(
                constraint,
                Some("local_logins_totp_needs_password" | "local_logins_totp_confirmed_has_secret")
            ),
            "{sql}: {refused}"
        );
    }
}

/// Q11: confirming stores the recovery codes in place of earlier ones and writes `totp.enable`;
/// there is nothing to confirm without a waiting secret.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn confirming_stores_the_recovery_codes_and_audits(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = with_password(&pool, a).await;
    assert!(matches!(
        confirm(&pool, a, sam, &[[1; 32]]).await,
        Err(Error::NotStarted)
    ));
    start(&pool, a, sam, SEALED).await.unwrap();
    let hashes: Vec<[u8; 32]> = (0..10u8).map(|n| [n; 32]).collect();
    confirm(&pool, a, sam, &hashes).await.unwrap();
    assert!(read(&pool, a, sam).await.unwrap().confirmed);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM totp_recovery_codes WHERE user_id = $1")
            .bind(sam)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 10);
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let entry = entries.iter().find(|e| e.action == "totp.enable").unwrap();
    assert_eq!(
        (entry.target_type.as_str(), entry.target_id, entry.actor),
        ("totp", sam, actor(sam))
    );
    assert_eq!(
        entry.data,
        serde_json::json!({ "after": { "user_id": sam } })
    );
}

/// B20: a step is recorded only when it is later than the last one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_step_is_recorded_only_forwards(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = with_password(&pool, a).await;
    start(&pool, a, sam, SEALED).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let s = scope(a);
    assert!(
        local_totp::record_step(&mut conn, s, sam, 100, now())
            .await
            .unwrap()
    );
    assert!(
        !local_totp::record_step(&mut conn, s, sam, 100, now())
            .await
            .unwrap()
    );
    assert!(
        !local_totp::record_step(&mut conn, s, sam, 99, now())
            .await
            .unwrap()
    );
    assert!(
        local_totp::record_step(&mut conn, s, sam, 101, now())
            .await
            .unwrap()
    );
    drop(conn);
    assert_eq!(read(&pool, a, sam).await.unwrap().last_step, Some(101));
}

/// Q11: a recovery code is used once, and only by its own user in its own organization.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_recovery_code_is_used_once_by_its_user(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = with_password(&pool, a).await;
    let ada = with_password(&pool, a).await;
    start(&pool, a, sam, SEALED).await.unwrap();
    confirm(&pool, a, sam, &[[7; 32]]).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let code = [7; 32];
    assert!(
        !local_totp::use_recovery_code(&mut conn, scope(a), ada, &code, now())
            .await
            .unwrap()
    );
    assert!(
        !local_totp::use_recovery_code(&mut conn, scope(b), sam, &code, now())
            .await
            .unwrap()
    );
    assert!(
        local_totp::use_recovery_code(&mut conn, scope(a), sam, &code, now())
            .await
            .unwrap()
    );
    assert!(
        !local_totp::use_recovery_code(&mut conn, scope(a), sam, &code, now())
            .await
            .unwrap()
    );
}

/// B21: disabling removes the secret, the step and the codes and writes `totp.disable`; there
/// is nothing to disable before a confirmation; another organization's scope reaches nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn disabling_removes_everything_and_audits(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = with_password(&pool, a).await;
    start(&pool, a, sam, SEALED).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(matches!(
        local_totp::disable(&mut conn, scope(a), sam, now(), actor(sam)).await,
        Err(Error::NotConfirmed)
    ));
    drop(conn);
    confirm(&pool, a, sam, &[[3; 32]]).await.unwrap();
    assert!(
        read(&pool, b, sam).await.is_none(),
        "another organization's scope"
    );
    let mut conn = pool.acquire().await.unwrap();
    assert!(matches!(
        local_totp::disable(&mut conn, scope(b), sam, now(), actor(sam)).await,
        Err(Error::NotConfirmed)
    ));
    local_totp::disable(&mut conn, scope(a), sam, now(), actor(sam))
        .await
        .unwrap();
    drop(conn);
    let totp = read(&pool, a, sam).await.unwrap();
    assert_eq!(
        (totp.sealed_secret, totp.confirmed, totp.last_step),
        (None, false, None)
    );
    let codes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM totp_recovery_codes WHERE user_id = $1")
            .bind(sam)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(codes, 0);
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let entry = entries.iter().find(|e| e.action == "totp.disable").unwrap();
    assert_eq!(
        entry.data,
        serde_json::json!({ "before": { "user_id": sam } })
    );
}
