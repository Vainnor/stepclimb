// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission webhook repository and its table against a fresh database per test
//! (docs/specs/m0/07-admission.md B24, Data; spec 09 B22 for its audit entries). The sealed
//! secrets here are stand-in bytes: the repository never seals or opens one. Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    admission_webhooks::{self, Change, Error, Fresh, Saved},
    audit::{self, Actor},
    id::{Uuid, new_id},
    organizations,
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn later() -> SystemTime {
    now() + Duration::from_secs(60)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the admission webhook routes stand in here"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

const ACTOR: Actor = Actor::User {
    id: Uuid::from_u128(1),
    ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
};

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Example", now())
        .await
        .expect("create organization")
        .id
}

const URL: &str = "https://hooks.example.test/admission";

fn url(url: &str) -> Change<'_> {
    Change {
        url: Some(url),
        enabled: None,
    }
}

async fn save(
    pool: &PgPool,
    org: Uuid,
    change: Change<'_>,
    sealed: &[u8],
    at: SystemTime,
) -> Result<Saved, Error> {
    let fresh = Fresh {
        id: new_id(),
        sealed_secret: sealed,
    };
    admission_webhooks::save(pool, scope(org), change, &fresh, at, ACTOR).await
}

/// The entries written after the organization's own creation entry.
async fn webhook_entries(pool: &PgPool, org: Uuid) -> Vec<audit::Entry> {
    audit::list(pool, scope(org), 100)
        .await
        .expect("read audit")
        .into_iter()
        .filter(|e| e.target_type == "admission_webhook")
        .collect()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_first_save_creates_the_webhook_with_the_fresh_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    assert_eq!(
        admission_webhooks::get(&pool, scope(a)).await.unwrap(),
        None
    );
    let fresh = Fresh {
        id: new_id(),
        sealed_secret: b"sealed-one",
    };
    let saved = admission_webhooks::save(&pool, scope(a), url(URL), &fresh, now(), ACTOR)
        .await
        .unwrap();
    let Saved::Created(created) = saved else {
        panic!("not created: {saved:?}");
    };
    assert_eq!(created.id, fresh.id);
    assert_eq!(created.url, URL);
    assert!(!created.enabled, "a new webhook is off until enabled");
    assert_eq!(created.sealed_secret, b"sealed-one");
    assert_eq!((created.created_at, created.updated_at), (now(), now()));
    assert_eq!(
        admission_webhooks::get(&pool, scope(a)).await.unwrap(),
        Some(created.clone())
    );

    let entries = webhook_entries(&pool, a).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "admission_webhook.create");
    assert_eq!(entries[0].target_id, fresh.id);
    assert_eq!(entries[0].actor, ACTOR);
    assert_eq!(
        entries[0].data,
        json!({ "after": { "url": URL, "enabled": false } })
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn creating_needs_a_url_and_writes_nothing_without_one(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let off = Change {
        url: None,
        enabled: Some(false),
    };
    assert!(matches!(
        save(&pool, a, off, b"s", now()).await,
        Err(Error::UrlRequired)
    ));
    assert_eq!(
        admission_webhooks::get(&pool, scope(a)).await.unwrap(),
        None
    );
    assert!(webhook_entries(&pool, a).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_later_save_changes_only_what_it_names_and_keeps_the_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let Saved::Created(created) = save(&pool, a, url(URL), b"first", now()).await.unwrap() else {
        panic!("not created");
    };
    let on = Change {
        url: None,
        enabled: Some(true),
    };
    let Saved::Updated(updated) = save(&pool, a, on, b"unused", later()).await.unwrap() else {
        panic!("not updated");
    };
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.url, URL, "the URL the change left out stays");
    assert!(updated.enabled);
    assert_eq!(
        updated.sealed_secret, b"first",
        "an update keeps the secret"
    );
    assert_eq!((updated.created_at, updated.updated_at), (now(), later()));

    let moved = "https://hooks.example.test/v2";
    let Saved::Updated(updated) = save(&pool, a, url(moved), b"unused", later())
        .await
        .unwrap()
    else {
        panic!("not updated");
    };
    assert_eq!((updated.url.as_str(), updated.enabled), (moved, true));

    let entries = webhook_entries(&pool, a).await;
    let data: Vec<_> = entries.iter().map(|e| (&*e.action, &e.data)).collect();
    assert_eq!(
        data,
        [
            (
                "admission_webhook.create",
                &json!({ "after": { "url": URL, "enabled": false } })
            ),
            (
                "admission_webhook.update",
                &json!({ "before": { "enabled": false }, "after": { "enabled": true } })
            ),
            (
                "admission_webhook.update",
                &json!({ "before": { "url": URL }, "after": { "url": moved } })
            ),
        ]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_save_of_stored_values_writes_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    save(&pool, a, url(URL), b"first", now()).await.unwrap();
    let same = Change {
        url: Some(URL),
        enabled: Some(false),
    };
    for change in [same, Change::default()] {
        let Saved::Unchanged(stored) = save(&pool, a, change, b"x", later()).await.unwrap() else {
            panic!("changed");
        };
        assert_eq!(stored.updated_at, now(), "updated_at stays");
        assert_eq!(stored.sealed_secret, b"first");
    }
    assert_eq!(webhook_entries(&pool, a).await.len(), 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn rotating_replaces_the_secret_and_audits_no_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let Saved::Created(created) = save(&pool, a, url(URL), b"first", now()).await.unwrap() else {
        panic!("not created");
    };
    admission_webhooks::rotate_secret(
        &pool,
        scope(a),
        created.id,
        b"first",
        b"second",
        later(),
        ACTOR,
    )
    .await
    .unwrap();
    let stored = admission_webhooks::get(&pool, scope(a))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.sealed_secret, b"second");
    assert_eq!((stored.url.as_str(), stored.enabled), (URL, false));
    assert_eq!(stored.updated_at, later());

    let entries = webhook_entries(&pool, a).await;
    let last = entries.last().unwrap();
    assert_eq!(last.action, "admission_webhook.rotate_secret");
    assert_eq!(last.target_id, created.id);
    assert_eq!(last.data, json!({}));

    // A webhook id that is not the organization's current one is not found.
    assert!(matches!(
        admission_webhooks::rotate_secret(
            &pool,
            scope(a),
            new_id(),
            b"second",
            b"x",
            later(),
            ACTOR
        )
        .await,
        Err(Error::NotFound)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_removes_the_row_and_records_what_it_was(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    assert!(matches!(
        admission_webhooks::delete(&pool, scope(a), now(), ACTOR).await,
        Err(Error::NotFound)
    ));
    let Saved::Created(created) = save(&pool, a, url(URL), b"first", now()).await.unwrap() else {
        panic!("not created");
    };
    admission_webhooks::delete(&pool, scope(a), later(), ACTOR)
        .await
        .unwrap();
    assert_eq!(
        admission_webhooks::get(&pool, scope(a)).await.unwrap(),
        None
    );
    let entries = webhook_entries(&pool, a).await;
    let last = entries.last().unwrap();
    assert_eq!(last.action, "admission_webhook.delete");
    assert_eq!(last.target_id, created.id);
    assert_eq!(
        last.data,
        json!({ "before": { "url": URL, "enabled": false } })
    );

    // A save after a delete creates a new webhook with the new secret.
    let Saved::Created(again) = save(&pool, a, url(URL), b"new", later()).await.unwrap() else {
        panic!("not created");
    };
    assert_ne!(again.id, created.id);
    assert_eq!(again.sealed_secret, b"new");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_organization_sees_and_changes_only_its_own_webhook(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let Saved::Created(of_b) = save(&pool, b, url(URL), b"of-b", now()).await.unwrap() else {
        panic!("not created");
    };
    assert_eq!(
        admission_webhooks::get(&pool, scope(a)).await.unwrap(),
        None
    );
    assert!(matches!(
        admission_webhooks::rotate_secret(&pool, scope(a), of_b.id, b"of-b", b"x", later(), ACTOR)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        admission_webhooks::delete(&pool, scope(a), later(), ACTOR).await,
        Err(Error::NotFound)
    ));
    let Saved::Created(of_a) = save(&pool, a, url("https://a.example.test/"), b"of-a", now())
        .await
        .unwrap()
    else {
        panic!("org A's first save creates its own webhook");
    };
    assert_ne!(of_a.id, of_b.id);
    assert_eq!(
        admission_webhooks::get(&pool, scope(b)).await.unwrap(),
        Some(of_b)
    );
    assert!(
        webhook_entries(&pool, a)
            .await
            .iter()
            .all(|e| e.target_id == of_a.id)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_without_a_row_is_not_found(pool: PgPool) {
    assert!(matches!(
        save(&pool, new_id(), url(URL), b"s", now()).await,
        Err(Error::OrgNotFound)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_holds_one_webhook_per_organization_and_a_bounded_url(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let insert = |url: String| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO admission_webhooks (id, org_id, url, secret) VALUES ($1, $2, $3, 'x')",
            )
            .bind(new_id())
            .bind(a)
            .bind(url)
            .execute(&pool)
            .await
        }
    };
    assert!(insert(String::new()).await.is_err(), "an empty URL");
    assert!(
        insert(format!("https://h.example.test/{}", "a".repeat(2048)))
            .await
            .is_err(),
        "a URL over 2,048 characters"
    );
    insert(URL.to_owned()).await.unwrap();
    assert!(insert(URL.to_owned()).await.is_err(), "a second webhook");
    let empty_secret = sqlx::query(
        "INSERT INTO admission_webhooks (id, org_id, url, secret) VALUES ($1, $2, $3, '')",
    )
    .bind(new_id())
    .bind(org(&pool, "org-b").await)
    .bind(URL)
    .execute(&pool)
    .await;
    assert!(empty_secret.is_err(), "an empty secret");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn debug_output_leaves_the_sealed_secret_out(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let Saved::Created(created) = save(&pool, a, url(URL), b"SEALED-BYTES", now())
        .await
        .unwrap()
    else {
        panic!("not created");
    };
    let debug = format!("{created:?}");
    assert!(!debug.contains("SEALED"), "{debug}");
    assert!(!debug.contains("83, 69, 65, 76"), "{debug}");
    assert!(!debug.contains(URL), "{debug}");
}

/// Two rotations that read the same secret: the second finds it replaced and writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rotation_of_a_secret_already_replaced_is_refused(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let Saved::Created(created) = save(&pool, a, url(URL), b"first", now()).await.unwrap() else {
        panic!("not created");
    };
    let rotate = |read: &'static [u8], new: &'static [u8]| {
        admission_webhooks::rotate_secret(&pool, scope(a), created.id, read, new, later(), ACTOR)
    };
    rotate(b"first", b"second").await.unwrap();
    assert!(matches!(
        rotate(b"first", b"third").await,
        Err(Error::SecretChanged)
    ));
    let stored = admission_webhooks::get(&pool, scope(a))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.sealed_secret, b"second");
    let rotations = webhook_entries(&pool, a)
        .await
        .iter()
        .filter(|e| e.action == "admission_webhook.rotate_secret")
        .count();
    assert_eq!(rotations, 1);
}

/// The concurrent case: a rotation waits on the organization row's lock that another rotation
/// holds, and once that one commits it finds the secret it read replaced.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn concurrent_rotations_of_one_secret_let_one_win(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let Saved::Created(created) = save(&pool, a, url(URL), b"first", now()).await.unwrap() else {
        panic!("not created");
    };
    let id = created.id;
    let mut first = pool.begin().await.unwrap();
    admission_webhooks::rotate_secret(&mut *first, scope(a), id, b"first", b"one", later(), ACTOR)
        .await
        .unwrap();
    let racer = pool.clone();
    let second = tokio::spawn(async move {
        admission_webhooks::rotate_secret(&racer, scope(a), id, b"first", b"two", later(), ACTOR)
            .await
    });
    // The second rotation is blocked on the lock while the first is open.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !second.is_finished(),
        "the second rotation waits for the lock"
    );
    first.commit().await.unwrap();
    assert!(matches!(second.await.unwrap(), Err(Error::SecretChanged)));
    let stored = admission_webhooks::get(&pool, scope(a))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.sealed_secret, b"one");
}
