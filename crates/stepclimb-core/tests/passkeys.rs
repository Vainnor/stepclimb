// SPDX-License-Identifier: AGPL-3.0-or-later
//! The passkeys repository and its table against a fresh database per test
//! (docs/specs/m0/04-local-accounts.md B3, B8, Data, Q3). Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    id::Uuid,
    local, organizations,
    passkeys::{self, Error, NewPasskey},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded write shows.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the passkey routes stand in for the credential resolver here"
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
        email: Some(format!(
            "{}@example.test",
            stepclimb_core::id::new_id().simple()
        )),
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

fn credential() -> Value {
    json!({ "cred": { "counter": 0 } })
}

async fn store(
    pool: &PgPool,
    org: Uuid,
    user_id: Uuid,
    credential_id: &[u8],
) -> Result<passkeys::Passkey, Error> {
    let credential = credential();
    let new = NewPasskey {
        user_id,
        credential_id,
        credential: &credential,
        name: " Phone ",
    };
    let actor = Actor::User {
        id: user_id,
        ip: ACTOR_IP,
    };
    passkeys::create(pool, scope(org), new, now(), actor).await
}

/// A stored passkey keeps its trimmed name, its credential and its time, is listed for its user
/// only, counts as a local credential, and is audited without the credential.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_passkey_is_stored_listed_and_audited(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a).await;
    let pat = user(&pool, a).await;
    let mut conn = pool.acquire().await.expect("a connection");
    assert!(
        !local::has_credential(&mut conn, scope(a), sam)
            .await
            .unwrap()
    );

    let stored = store(&pool, a, sam, &[1; 16]).await.expect("store");
    assert_eq!((stored.user_id, stored.name.as_str()), (sam, "Phone"));
    assert_eq!(stored.created_at, now());
    let row: Value = sqlx::query_scalar("SELECT to_jsonb(p) FROM passkeys p")
        .fetch_one(&pool)
        .await
        .expect("the row");
    assert_eq!(row["credential"], credential());
    assert_eq!(row["last_used_at"], Value::Null);

    assert_eq!(
        passkeys::credential_ids(&pool, scope(a), sam)
            .await
            .unwrap(),
        [vec![1; 16]]
    );
    assert!(
        passkeys::credential_ids(&pool, scope(a), pat)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        local::has_credential(&mut conn, scope(a), sam)
            .await
            .unwrap()
    );
    assert!(
        !local::has_credential(&mut conn, scope(a), pat)
            .await
            .unwrap()
    );

    let entries = audit::list(&pool, scope(a), 100).await.expect("entries");
    let entry = entries
        .iter()
        .find(|e| e.action == "passkey.create")
        .expect("the entry");
    assert_eq!(
        (entry.target_type.as_str(), entry.target_id),
        ("passkey", stored.id)
    );
    assert_eq!(
        entry.data,
        json!({ "after": { "user_id": sam, "name": "Phone" } })
    );
    assert_eq!(
        entry.actor,
        Actor::User {
            id: sam,
            ip: ACTOR_IP
        }
    );
}

/// One credential id is registered once per organization, for any user of it, and the refused
/// insert writes nothing; another organization may hold the same id.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_credential_id_is_unique_per_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let sam = user(&pool, a).await;
    let pat = user(&pool, a).await;
    let kim = user(&pool, b).await;
    store(&pool, a, sam, &[7; 16]).await.expect("store");
    for again in [sam, pat] {
        let taken = store(&pool, a, again, &[7; 16]).await;
        assert!(matches!(taken, Err(Error::CredentialTaken)), "{taken:?}");
    }
    store(&pool, b, kim, &[7; 16])
        .await
        .expect("org B may hold it");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM passkeys")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 2);
    let entries: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'passkey.create'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(entries, 2);
}

/// Another organization's user is refused as unknown, and its passkeys are not read through
/// this organization's scope.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_user_is_not_found(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let kim = user(&pool, b).await;
    let refused = store(&pool, a, kim, &[2; 16]).await;
    assert!(matches!(refused, Err(Error::UserNotFound)), "{refused:?}");
    store(&pool, b, kim, &[2; 16]).await.expect("store in B");
    assert!(
        passkeys::credential_ids(&pool, scope(a), kim)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !passkeys::exists_for_user(&pool, scope(a), kim)
            .await
            .unwrap()
    );
}

/// A bad name is refused before anything is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_blank_name_is_refused(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a).await;
    let credential = credential();
    let new = NewPasskey {
        user_id: sam,
        credential_id: &[3; 16],
        credential: &credential,
        name: "  ",
    };
    let refused = passkeys::create(&pool, scope(a), new, now(), Actor::Instance).await;
    assert!(matches!(refused, Err(Error::NameEmpty)), "{refused:?}");
    assert!(
        !passkeys::exists_for_user(&pool, scope(a), sam)
            .await
            .unwrap()
    );
}

/// Deleting the user deletes their passkeys.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_users_passkeys_go_with_the_user(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let sam = user(&pool, a).await;
    store(&pool, a, sam, &[4; 16]).await.expect("store");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(sam)
        .execute(&pool)
        .await
        .expect("delete the user");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM passkeys")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 0);
}
