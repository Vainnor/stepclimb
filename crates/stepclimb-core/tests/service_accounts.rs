// SPDX-License-Identifier: AGPL-3.0-or-later
//! The service accounts repository, the API-key resolver and their table against a fresh
//! database per test (docs/specs/m0/08-sessions-and-csrf.md B24 to B29, Data, Q9). Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, ScopeOrigin, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    organizations,
    service_accounts::{
        self, Error, KeyAuthentication, Kind, NewServiceAccount, ScopeProblem, TOUCH_INTERVAL,
    },
    users::{self, NewUser, Status},
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the service account routes and the request resolver stand in here"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

const ACTOR_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

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
        email: None,
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

/// A stand-in for an API key's SHA-256: the repository never sees a key.
fn hash() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(new_id().as_bytes());
    bytes[16..].copy_from_slice(new_id().as_bytes());
    bytes
}

fn new(owner: Uuid, scopes: &[&str]) -> NewServiceAccount {
    NewServiceAccount {
        name: "  Nightly export ".into(),
        owner_user_id: owner,
        scopes: scopes.iter().map(|&s| s.to_owned()).collect(),
    }
}

fn actor(id: Uuid) -> Actor {
    Actor::User { id, ip: ACTOR_IP }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_account_is_stored_listed_and_audited_without_its_hash(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a).await;
    let secret = hash();
    let created = service_accounts::create(
        &pool,
        scope(a),
        &new(sam, &["user.view"]),
        &secret,
        now(),
        actor(sam),
    )
    .await
    .unwrap();
    assert_eq!(created.name, "Nightly export");
    assert_eq!(created.kind, Kind::ApiKey);
    assert_eq!(created.scopes, ["user.view"]);
    assert_eq!(created.owner_user_id, sam);
    assert_eq!(created.created_at, now());
    assert_eq!(created.last_used_at, None);

    let listed = service_accounts::list(&pool, scope(a), None, 10)
        .await
        .unwrap();
    assert_eq!(listed, std::slice::from_ref(&created));
    let after = service_accounts::list(&pool, scope(a), Some(&created.id), 10)
        .await
        .unwrap();
    assert!(after.is_empty());

    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let entry = entries.last().unwrap();
    assert_eq!(entry.action, "service_account.create");
    assert_eq!(entry.target_id, created.id);
    assert_eq!(entry.actor, actor(sam));
    assert_eq!(
        entry.data,
        json!({ "after": {
            "name": "Nightly export", "kind": "api_key",
            "owner_user_id": sam, "scopes": ["user.view"],
        } })
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn bad_names_scopes_and_owners_are_refused_and_write_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = user(&pool, a).await;
    let robin = user(&pool, b).await;
    let create = |account: NewServiceAccount| {
        let pool = pool.clone();
        async move {
            service_accounts::create(&pool, scope(a), &account, &hash(), now(), actor(sam)).await
        }
    };
    let named = |name: &str| NewServiceAccount {
        name: name.into(),
        ..new(sam, &[])
    };
    assert!(matches!(create(named(" ")).await, Err(Error::NameEmpty)));
    assert!(matches!(
        create(named(&"x".repeat(201))).await,
        Err(Error::NameTooLong)
    ));
    assert!(matches!(
        create(named("a\0b")).await,
        Err(Error::NameContainsNul)
    ));
    assert!(matches!(
        create(new(sam, &["user.view", "reports:read"])).await,
        Err(Error::Scope {
            index: 1,
            problem: ScopeProblem::Unknown
        })
    ));
    assert!(matches!(
        create(new(sam, &["user.view", "user.view"])).await,
        Err(Error::Scope {
            index: 1,
            problem: ScopeProblem::Duplicate
        })
    ));
    // Another organization's user cannot own an account of this one.
    assert!(matches!(
        create(new(robin, &[])).await,
        Err(Error::OwnerNotFound)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM service_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    assert!(entries.iter().all(|e| e.action != "service_account.create"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_key_authenticates_its_owner_in_its_own_organization_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = user(&pool, a).await;
    let secret = hash();
    let created = service_accounts::create(
        &pool,
        scope(a),
        &new(sam, &["account.view"]),
        &secret,
        now(),
        actor(sam),
    )
    .await
    .unwrap();

    let KeyAuthentication::Valid(principal) =
        service_accounts::authenticate(&pool, scope(a), &secret, now())
            .await
            .unwrap()
    else {
        panic!("the key is valid");
    };
    assert_eq!(principal.service_account_id, created.id);
    assert_eq!(principal.user_id, sam);
    assert_eq!(principal.scopes, ["account.view"]);
    assert_eq!(principal.scope.org_id(), a);
    assert_eq!(principal.scope.origin(), ScopeOrigin::ApiKey);

    // B6: presented to another organization, the key is unknown.
    assert_eq!(
        service_accounts::authenticate(&pool, scope(b), &secret, now())
            .await
            .unwrap(),
        KeyAuthentication::Unknown
    );
    assert_eq!(
        service_accounts::authenticate(&pool, scope(a), &hash(), now())
            .await
            .unwrap(),
        KeyAuthentication::Unknown
    );
}

async fn last_used(pool: &PgPool, org: Uuid, id: Uuid) -> Option<SystemTime> {
    service_accounts::list(pool, scope(org), None, 200)
        .await
        .expect("list")
        .into_iter()
        .find(|account| account.id == id)
        .expect("the account")
        .last_used_at
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn last_used_is_written_at_most_once_a_minute(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a).await;
    let secret = hash();
    let id = service_accounts::create(&pool, scope(a), &new(sam, &[]), &secret, now(), actor(sam))
        .await
        .unwrap()
        .id;
    let use_at = |at: SystemTime| {
        let pool = pool.clone();
        async move {
            service_accounts::authenticate(&pool, scope(a), &secret, at)
                .await
                .unwrap()
        }
    };
    use_at(now()).await;
    assert_eq!(last_used(&pool, a, id).await, Some(now()));
    use_at(now() + TOUCH_INTERVAL - Duration::from_secs(1)).await;
    assert_eq!(last_used(&pool, a, id).await, Some(now()));
    use_at(now() + TOUCH_INTERVAL).await;
    assert_eq!(last_used(&pool, a, id).await, Some(now() + TOUCH_INTERVAL));
}

/// B27, B29: a suspended owner's key answers suspended and is kept; a deleted account's key is
/// unknown, and the deletion is audited.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn suspension_and_deletion_stop_the_key_on_the_next_lookup(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = user(&pool, a).await;
    let secret = hash();
    let id = service_accounts::create(
        &pool,
        scope(a),
        &new(sam, &["user.view"]),
        &secret,
        now(),
        actor(sam),
    )
    .await
    .unwrap()
    .id;
    let lookup = || service_accounts::authenticate(&pool, scope(a), &secret, now());

    users::set_status(&pool, scope(a), sam, Status::Suspended, now(), actor(sam))
        .await
        .unwrap();
    assert_eq!(lookup().await.unwrap(), KeyAuthentication::Suspended);
    users::set_status(&pool, scope(a), sam, Status::Active, now(), actor(sam))
        .await
        .unwrap();
    assert!(matches!(
        lookup().await.unwrap(),
        KeyAuthentication::Valid(_)
    ));

    // Another organization's scope deletes nothing.
    assert!(matches!(
        service_accounts::delete(&pool, scope(b), id, now(), actor(sam)).await,
        Err(Error::NotFound)
    ));
    service_accounts::delete(&pool, scope(a), id, now(), actor(sam))
        .await
        .unwrap();
    assert_eq!(lookup().await.unwrap(), KeyAuthentication::Unknown);
    assert!(matches!(
        service_accounts::delete(&pool, scope(a), id, now(), actor(sam)).await,
        Err(Error::NotFound)
    ));
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let entry = entries.last().unwrap();
    assert_eq!(entry.action, "service_account.delete");
    assert_eq!(entry.target_id, id);
    assert_eq!(
        entry.data,
        json!({ "before": {
            "name": "Nightly export", "kind": "api_key",
            "owner_user_id": sam, "scopes": ["user.view"],
        } })
    );
}

/// T1's database half for keys: the table holds only the 32-byte hash, never key text.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_anything_but_a_32_byte_hash_and_a_foreign_owner(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let robin = user(&pool, b).await;
    let sam = user(&pool, a).await;
    let insert = |owner: Uuid, secret: Vec<u8>| {
        sqlx::query(
            "INSERT INTO service_accounts (id, org_id, kind, name, owner_user_id, secret_hash,
                                           scopes)
             VALUES (gen_random_uuid(), $1, 'api_key', 'x', $2, $3, '{}')",
        )
        .bind(a)
        .bind(owner)
        .bind(secret)
        .execute(&pool)
    };
    let key_text = b"sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ".to_vec();
    let refused = insert(sam, key_text).await.expect_err("refused");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("23514"),
        "check_violation"
    );
    let refused = insert(robin, hash().to_vec()).await.expect_err("refused");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("23503"),
        "foreign_key_violation"
    );
    assert!(insert(sam, hash().to_vec()).await.is_ok());
}
