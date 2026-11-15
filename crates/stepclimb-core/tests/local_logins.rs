// SPDX-License-Identifier: AGPL-3.0-or-later
//! The local logins repository and its table against a fresh database per test
//! (docs/specs/m0/04-local-accounts.md B3, B4, B9, B16, B17, Q5, Q10;
//! docs/specs/m0/08-sessions-and-csrf.md B41). Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    local::{self, DeleteError},
    local_logins, organizations,
    passkeys::{self, NewPasskey},
    sessions,
    users::{self, NewUser, UserPatch},
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the password routes stand in for the credential resolver here"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

const ACTOR_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

/// A PHC string the column's check accepts. The repository never parses it.
const HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g";
const NEWER: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$bmV3c2FsdG5ld3NhbHQ$bmV3aGFzaG5ld2hhc2huZXdoYXNobmV3aGFzaG5ldw";

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Example", now())
        .await
        .expect("create organization")
        .id
}

async fn user(pool: &PgPool, org: Uuid, email: Option<&str>) -> Uuid {
    let user = NewUser {
        display_name: "Sam Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: email.map(str::to_owned),
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

fn actor(user_id: Uuid) -> Actor {
    Actor::User {
        id: user_id,
        ip: ACTOR_IP,
    }
}

async fn set(pool: &PgPool, org: Uuid, user_id: Uuid, hash: &str) {
    local_logins::set_password(pool, scope(org), user_id, hash, now(), actor(user_id))
        .await
        .expect("set the password");
}

async fn set_email(pool: &PgPool, org: Uuid, user_id: Uuid, email: Option<&str>) {
    let patch = UserPatch {
        email: Some(email.map(str::to_owned)),
        ..UserPatch::default()
    };
    users::update(pool, scope(org), user_id, &patch, now())
        .await
        .expect("update the email");
}

async fn found(pool: &PgPool, org: Uuid, email: &str) -> Option<Uuid> {
    let (_, login) = local_logins::find_for_sign_in(pool, scope(org), email)
        .await
        .expect("look up");
    login.map(|login| login.user_id)
}

/// B9: a password is stored with the user's email folded, counts as a local credential, and is
/// audited with the user and never the hash.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_is_stored_found_by_email_and_audited(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("Sam.Example@Example.test")).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        !local::has_credential(&mut conn, scope(a), sam)
            .await
            .unwrap()
    );

    set(&pool, a, sam, HASH).await;
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(l) FROM local_logins l")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row["email_normalized"], "sam.example@example.test");
    assert_eq!(row["password_hash"], HASH);
    assert!(
        local::has_credential(&mut conn, scope(a), sam)
            .await
            .unwrap()
    );
    assert!(
        local_logins::has_password(&pool, scope(a), sam)
            .await
            .unwrap()
    );

    for typed in ["sam.example@example.test", "  SAM.EXAMPLE@EXAMPLE.TEST "] {
        let (email, login) = local_logins::find_for_sign_in(&pool, scope(a), typed)
            .await
            .unwrap();
        assert_eq!(email.as_str(), "sam.example@example.test", "{typed:?}");
        let login = login.expect("found");
        assert_eq!(login.user_id, sam);
        assert_eq!(login.password_hash.as_deref(), Some(HASH));
    }

    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let entry = entries
        .iter()
        .find(|e| e.action == "password.set")
        .expect("a password.set entry");
    assert_eq!(
        (entry.target_type.as_str(), entry.target_id),
        ("password", sam)
    );
    assert_eq!(entry.data["after"]["user_id"], sam.to_string());
    assert!(!entry.data.to_string().contains("argon2"), "{}", entry.data);
}

/// B17: setting again replaces the hash and writes another entry; the row stays one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn setting_a_password_again_replaces_it(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    set(&pool, a, sam, HASH).await;
    set(&pool, a, sam, NEWER).await;
    let login = local_logins::get(&pool, scope(a), sam)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(login.password_hash.as_deref(), Some(NEWER));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_logins")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.action == "password.set")
            .count(),
        2
    );
}

/// Q5: the stored address follows the user's email, so sign-in always matches the email the
/// user has now; without an email the login matches nothing until one is set again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_login_follows_the_users_email(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    set(&pool, a, sam, HASH).await;

    set_email(&pool, a, sam, Some("Samuel@Example.test")).await;
    assert_eq!(found(&pool, a, "samuel@example.test").await, Some(sam));
    assert_eq!(found(&pool, a, "sam@example.test").await, None);

    set_email(&pool, a, sam, None).await;
    assert_eq!(found(&pool, a, "samuel@example.test").await, None);
    let login = local_logins::get(&pool, scope(a), sam)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(login.email, None);
    assert!(
        local_logins::has_password(&pool, scope(a), sam)
            .await
            .unwrap()
    );

    set_email(&pool, a, sam, Some("sam@example.test")).await;
    assert_eq!(found(&pool, a, "SAM@example.test").await, Some(sam));
}

/// B41: the address the rate limit counts against is the one the account is matched by, folded
/// by the database's `lower()`, whatever that folds outside ASCII. An address the users table
/// holds as one account (`users_email` compares `lower()`) is one identifier, and two it holds
/// apart are two.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_non_ascii_address_folds_once_for_the_match_and_the_budget(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let elodie = user(&pool, a, Some("élodie@example.test")).await;
    set(&pool, a, elodie, HASH).await;
    let folds: bool = sqlx::query_scalar("SELECT lower('ÉLODIE') = lower('élodie')")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (upper, upper_login) =
        local_logins::find_for_sign_in(&pool, scope(a), "ÉLODIE@Example.test")
            .await
            .unwrap();
    let (lower, lower_login) =
        local_logins::find_for_sign_in(&pool, scope(a), "élodie@example.test")
            .await
            .unwrap();
    assert_eq!(lower_login.map(|l| l.user_id), Some(elodie));
    assert_eq!(
        upper == lower,
        folds,
        "one identifier exactly when lower() folds É"
    );
    assert_eq!(
        upper_login.is_some(),
        folds,
        "one account exactly when lower() folds É"
    );
    // The users table agrees: it finds the same account for the same address.
    let by_users = users::find_by_email(&pool, scope(a), "ÉLODIE@Example.test")
        .await
        .unwrap();
    assert_eq!(by_users.map(|u| u.id), upper_login.map(|l| l.user_id));
}

/// An address holding NUL matches nothing and needs no query.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_address_with_nul_matches_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let (email, login) = local_logins::find_for_sign_in(&pool, scope(a), " sam\0@example.test ")
        .await
        .unwrap();
    assert_eq!(email.as_str(), "sam\0@example.test");
    assert!(login.is_none());
}

/// T19: another organization's address is no account here.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_login_is_not_found(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let pat = user(&pool, b, Some("pat@example.test")).await;
    set(&pool, b, pat, HASH).await;
    assert_eq!(found(&pool, a, "pat@example.test").await, None);
    assert_eq!(found(&pool, b, "pat@example.test").await, Some(pat));
    assert!(
        local_logins::get(&pool, scope(a), pat)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !local_logins::has_password(&pool, scope(a), pat)
            .await
            .unwrap()
    );
    let refused = local_logins::set_password(&pool, scope(a), pat, HASH, now(), actor(pat)).await;
    assert!(matches!(refused, Err(local_logins::Error::UserNotFound)));
}

/// B16: a rehash replaces the hash it read, and nothing once the hash has changed since.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rehash_replaces_only_the_hash_it_read(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    set(&pool, a, sam, HASH).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        local_logins::replace_hash(&mut conn, scope(a), sam, HASH, NEWER, now())
            .await
            .unwrap()
    );
    assert!(
        !local_logins::replace_hash(&mut conn, scope(a), sam, HASH, HASH, now())
            .await
            .unwrap()
    );
    let login = local_logins::get(&pool, scope(a), sam)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(login.password_hash.as_deref(), Some(NEWER));
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.action == "password.set")
            .count(),
        1
    );
}

/// The column refuses anything but an argon2id PHC string.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_hash_column_takes_argon2id_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    let refused =
        local_logins::set_password(&pool, scope(a), sam, "$2b$12$bcrypt", now(), actor(sam)).await;
    assert!(refused.is_err());
}

/// A user's login goes with the user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_users_login_goes_with_the_user(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    set(&pool, a, sam, HASH).await;
    sqlx::query("DELETE FROM audit_events")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(sam)
        .execute(&pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_logins")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

async fn provider(pool: &PgPool, org: Uuid) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'sso', 'oidc', '{\"label\": \"SSO\"}'::jsonb, true)",
    )
    .bind(id)
    .bind(org)
    .execute(pool)
    .await
    .expect("insert provider");
    id
}

/// spec 03 B19, spec 04 B3: a password is a way to sign in, so it lets the user's last outside
/// identity go; without it the removal is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_lets_the_last_outside_identity_go(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sso = provider(&pool, a).await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    let identity = NewIdentity {
        user_id: sam,
        provider_id: sso,
        subject: "sam-subject".into(),
        attributes: serde_json::Map::new(),
    };
    let linked = identities::link(&pool, scope(a), &identity, now(), Actor::Instance)
        .await
        .unwrap();
    let refused =
        identities::unlink_unless_last(&pool, scope(a), sam, linked.id, now(), Actor::Instance)
            .await;
    assert!(matches!(refused, Err(identities::Error::LastSignInMethod)));

    set(&pool, a, sam, HASH).await;
    identities::unlink_unless_last(&pool, scope(a), sam, linked.id, now(), Actor::Instance)
        .await
        .expect("the password keeps a way in");
}

/// Q10: a password is another way to sign in, so the user's last passkey may go.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_lets_the_last_passkey_go(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    let credential = serde_json::json!({ "cred": { "counter": 0 } });
    let new = NewPasskey {
        user_id: sam,
        credential_id: &[7; 16],
        credential: &credential,
        name: "Phone",
    };
    let passkey = passkeys::create(&pool, scope(a), new, now(), actor(sam))
        .await
        .unwrap();
    let refused =
        local::delete_passkey_unless_last(&pool, scope(a), sam, passkey.id, now(), actor(sam))
            .await;
    assert!(matches!(refused, Err(DeleteError::LastSignInMethod)));

    set(&pool, a, sam, HASH).await;
    local::delete_passkey_unless_last(&pool, scope(a), sam, passkey.id, now(), actor(sam))
        .await
        .expect("the password keeps a way in");
}

/// Q8: a password change ends the user's other sessions and keeps the one that asked, and no
/// other user's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ending_other_sessions_keeps_the_callers(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a, Some("sam@example.test")).await;
    let pat = user(&pool, a, Some("pat@example.test")).await;
    for (user_id, hash) in [
        (sam, [1u8; 32]),
        (sam, [2; 32]),
        (sam, [3; 32]),
        (pat, [4; 32]),
    ] {
        sessions::create(&pool, scope(a), user_id, &hash, now())
            .await
            .unwrap();
    }
    let ended =
        sessions::delete_others_of_user(&pool, scope(a), sam, sessions::Keep::TokenHash([2; 32]))
            .await
            .unwrap();
    assert_eq!(ended, 2);
    let left: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT token_hash FROM sessions ORDER BY token_hash")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(left, [vec![2; 32], vec![4; 32]]);
    // By id: the kept session is the one named.
    let kept = sessions::create(&pool, scope(a), sam, &[5; 32], now())
        .await
        .unwrap();
    let ended =
        sessions::delete_others_of_user(&pool, scope(a), sam, sessions::Keep::Session(kept.id))
            .await
            .unwrap();
    assert_eq!(ended, 1);
    // Another organization's scope ends nothing.
    let b = org(&pool, "org-b").await;
    let none =
        sessions::delete_others_of_user(&pool, scope(b), pat, sessions::Keep::TokenHash([0; 32]))
            .await
            .unwrap();
    assert_eq!(none, 0);
}
