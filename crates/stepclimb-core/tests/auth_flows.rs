// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sign-in flows repository and the `auth_flows` table against a fresh database per test
//! (docs/specs/m0/08-sessions-and-csrf.md B17 to B21, Q8; T18 and T19 at the repository level).
//! Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    auth_flows::{self, Consumed, Error, Kind, LIFETIME, NewFlow, ReturnTo},
    id::{Uuid, new_id},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded write shows.
fn start_time() -> SystemTime {
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
    users::create(pool, scope(org), &user, start_time())
        .await
        .expect("create user")
        .id
}

/// A distinct 32-byte hash per seed (the repository never sees a token).
fn hash(seed: u8) -> [u8; 32] {
    [seed; 32]
}

/// Store a flow of `kind` with `hash(seed)` in `org` at [`start_time`]; returns its id.
async fn create(pool: &PgPool, org: Uuid, seed: u8, kind: Kind, user: Option<Uuid>) -> Uuid {
    let id = new_id();
    let return_to = ReturnTo::sanitize(Some("/c/s1/m/2"));
    let flow = NewFlow {
        id,
        kind,
        user_id: user,
        provider_id: None,
        token_hash: &hash(seed),
        payload: b"sealed bytes",
        return_to: &return_to,
    };
    let expires_at = auth_flows::create(pool, scope(org), &flow, start_time())
        .await
        .expect("create flow");
    assert_eq!(expires_at, start_time() + LIFETIME);
    id
}

async fn consume(pool: &PgPool, org: Uuid, seed: u8, kind: Kind, at: SystemTime) -> Consumed {
    auth_flows::consume(pool, scope(org), &hash(seed), kind, at)
        .await
        .expect("consume")
}

async fn flow_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM auth_flows")
        .fetch_one(pool)
        .await
        .expect("count flows")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_is_returned_once_then_unknown(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    let id = create(&pool, org, 1, Kind::PasswordTotp, Some(user)).await;

    let first = consume(&pool, org, 1, Kind::PasswordTotp, start_time()).await;
    let Consumed::Valid(flow) = first else {
        panic!("expected a live flow, got {first:?}");
    };
    assert_eq!(flow.id, id);
    assert_eq!(flow.kind, Kind::PasswordTotp);
    assert_eq!(flow.user_id, Some(user));
    assert_eq!(flow.payload, b"sealed bytes");
    assert_eq!(flow.return_to.as_str(), "/c/s1/m/2");
    assert_eq!(flow.expires_at, start_time() + LIFETIME);
    assert_eq!(flow_count(&pool).await, 0);

    let second = consume(&pool, org, 1, Kind::PasswordTotp, start_time()).await;
    assert_eq!(second, Consumed::Unknown, "T18: a flow is single use");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_is_live_until_its_expiry_and_deleted_when_found_expired(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    create(&pool, org, 1, Kind::Oidc, None).await;
    create(&pool, org, 2, Kind::Oidc, None).await;
    let expiry = start_time() + LIFETIME;

    let just_before = consume(&pool, org, 1, Kind::Oidc, expiry - Duration::from_micros(1)).await;
    assert!(matches!(just_before, Consumed::Valid(_)), "{just_before:?}");

    let at_expiry = consume(&pool, org, 2, Kind::Oidc, expiry).await;
    assert_eq!(at_expiry, Consumed::Expired, "T18");
    assert_eq!(
        flow_count(&pool).await,
        0,
        "B21: the request that finds it deletes it"
    );
    assert_eq!(
        consume(&pool, org, 2, Kind::Oidc, expiry).await,
        Consumed::Unknown
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_of_another_kind_is_unknown_and_left_alone(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    create(&pool, org, 1, Kind::PasskeySignIn, None).await;

    for other in Kind::ALL.into_iter().filter(|k| *k != Kind::PasskeySignIn) {
        assert_eq!(
            consume(&pool, org, 1, other, start_time()).await,
            Consumed::Unknown,
            "{other:?}"
        );
    }
    assert_eq!(flow_count(&pool).await, 1);
    assert!(matches!(
        consume(&pool, org, 1, Kind::PasskeySignIn, start_time()).await,
        Consumed::Valid(_)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_flow_is_unknown_and_left_alone(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    create(&pool, b, 1, Kind::Oidc, None).await;

    assert_eq!(
        consume(&pool, a, 1, Kind::Oidc, start_time()).await,
        Consumed::Unknown
    );
    assert_eq!(flow_count(&pool).await, 1);
    assert!(matches!(
        consume(&pool, b, 1, Kind::Oidc, start_time()).await,
        Consumed::Valid(_)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_a_user_of_another_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user_b = insert_user(&pool, b).await;
    let return_to = ReturnTo::root();
    let flow = NewFlow {
        id: new_id(),
        kind: Kind::PasskeyRegister,
        user_id: Some(user_b),
        provider_id: None,
        token_hash: &hash(1),
        payload: b"x",
        return_to: &return_to,
    };

    let result = auth_flows::create(&pool, scope(a), &flow, start_time()).await;

    assert!(matches!(result, Err(Error::UserNotFound)), "{result:?}");
    assert_eq!(flow_count(&pool).await, 0);
}

async fn insert_provider(pool: &PgPool, org: Uuid, key: &str) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config) \
         VALUES ($1, $2, $3, 'oidc', '{}'::jsonb)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .execute(pool)
    .await
    .expect("insert provider");
    id
}

fn provider_flow(provider: Uuid, return_to: &ReturnTo) -> NewFlow<'_> {
    NewFlow {
        id: new_id(),
        kind: Kind::Oidc,
        user_id: None,
        provider_id: Some(provider),
        token_hash: &[7; 32],
        payload: b"x",
        return_to,
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_keeps_the_provider_it_was_started_for(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let provider = insert_provider(&pool, org, "idp").await;
    let return_to = ReturnTo::root();
    let flow = provider_flow(provider, &return_to);
    auth_flows::create(&pool, scope(org), &flow, start_time())
        .await
        .expect("create flow");

    let consumed = auth_flows::consume(&pool, scope(org), &[7; 32], Kind::Oidc, start_time())
        .await
        .expect("consume");

    let Consumed::Valid(found) = consumed else {
        panic!("expected a live flow, got {consumed:?}");
    };
    assert_eq!(found.provider_id, Some(provider));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_a_provider_of_another_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let provider_b = insert_provider(&pool, b, "idp").await;
    let return_to = ReturnTo::root();
    let flow = provider_flow(provider_b, &return_to);

    let result = auth_flows::create(&pool, scope(a), &flow, start_time()).await;

    assert!(matches!(result, Err(Error::ProviderNotFound)), "{result:?}");
    assert_eq!(flow_count(&pool).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_the_provider_deletes_its_flows(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let provider = insert_provider(&pool, org, "idp").await;
    let return_to = ReturnTo::root();
    auth_flows::create(
        &pool,
        scope(org),
        &provider_flow(provider, &return_to),
        start_time(),
    )
    .await
    .expect("create flow");

    sqlx::query("DELETE FROM identity_providers WHERE id = $1")
        .bind(provider)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(flow_count(&pool).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_the_user_deletes_their_flows(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let user = insert_user(&pool, org).await;
    create(&pool, org, 1, Kind::PasswordTotp, Some(user)).await;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(flow_count(&pool).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_deletes_only_the_expired_flows_of_the_scopes_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    create(&pool, a, 1, Kind::Oidc, None).await;
    create(&pool, b, 2, Kind::Oidc, None).await;
    let expiry = start_time() + LIFETIME;

    let early = auth_flows::delete_expired(&pool, scope(a), expiry - Duration::from_micros(1))
        .await
        .unwrap();
    assert_eq!(early, 0, "a live flow is kept");

    let deleted = auth_flows::delete_expired(&pool, scope(a), expiry)
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    let left: Vec<Uuid> = sqlx::query_scalar("SELECT org_id FROM auth_flows")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(
        left,
        [b],
        "another organization's expired flow is not this scope's"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_transactions_cannot_both_take_one_flow(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    create(&pool, org, 1, Kind::Oidc, None).await;

    let mut first = pool.begin().await.unwrap();
    let taken = auth_flows::consume(&mut *first, scope(org), &hash(1), Kind::Oidc, start_time())
        .await
        .unwrap();
    assert!(matches!(taken, Consumed::Valid(_)));

    // The second request's delete waits on the row the first holds, then finds it gone.
    let racing = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut second = pool.begin().await.unwrap();
            let result =
                auth_flows::consume(&mut *second, scope(org), &hash(1), Kind::Oidc, start_time())
                    .await
                    .unwrap();
            second.commit().await.unwrap();
            result
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !racing.is_finished(),
        "the second delete waits for the first transaction"
    );
    first.commit().await.unwrap();

    assert_eq!(racing.await.unwrap(), Consumed::Unknown);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rolled_back_completion_leaves_the_flow_usable(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    create(&pool, org, 1, Kind::PasswordTotp, None).await;

    let mut tx = pool.begin().await.unwrap();
    let taken = auth_flows::consume(
        &mut *tx,
        scope(org),
        &hash(1),
        Kind::PasswordTotp,
        start_time(),
    )
    .await
    .unwrap();
    assert!(matches!(taken, Consumed::Valid(_)));
    tx.rollback().await.unwrap();

    assert!(matches!(
        consume(&pool, org, 1, Kind::PasswordTotp, start_time()).await,
        Consumed::Valid(_)
    ));
}

/// Insert a row directly, bypassing the repository, to test the table's own checks.
async fn raw_insert(
    pool: &PgPool,
    org: Uuid,
    token_hash: &[u8],
    kind: &str,
    return_to: &str,
) -> bool {
    sqlx::query(
        "INSERT INTO auth_flows (id, org_id, token_hash, kind, payload, return_to, expires_at)
         VALUES ($1, $2, $3, $4, '\\x00', $5, now())",
    )
    .bind(new_id())
    .bind(org)
    .bind(token_hash)
    .bind(kind)
    .bind(return_to)
    .execute(pool)
    .await
    .is_ok()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_what_the_repository_never_writes(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    assert!(raw_insert(&pool, org, &hash(1), "oidc", "/c/s1").await);
    for kind in Kind::ALL {
        assert!(
            raw_insert(&pool, org, &[kind as u8 + 10; 32], kind.as_str(), "/").await,
            "{kind:?}"
        );
    }

    assert!(
        !raw_insert(&pool, org, &[2; 31], "oidc", "/").await,
        "hash length"
    );
    assert!(
        !raw_insert(&pool, org, &[3; 32], "session", "/").await,
        "kind"
    );
    for return_to in [
        "",
        "https://evil.example",
        "//evil.example",
        "/\\evil.example",
        "/\t/evil.example",
        "/caf\u{e9}",
    ] {
        assert!(
            !raw_insert(&pool, org, &[4; 32], "oidc", return_to).await,
            "{return_to:?}"
        );
    }
    let too_long = format!("/{}", "a".repeat(2048));
    assert!(!raw_insert(&pool, org, &[5; 32], "oidc", &too_long).await);
}
