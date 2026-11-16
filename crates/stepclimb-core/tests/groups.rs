// SPDX-License-Identifier: AGPL-3.0-or-later
//! The groups repository and its tables against a fresh database per test
//! (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7). All data is synthetic. Needs
//! `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    groups::{self, Error, GroupSource, MAX_KEYS, Replaced},
    id::{Uuid, new_id},
    organizations,
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "admission, which resolves the request's organization, stands in here"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Example", now())
        .await
        .expect("create organization")
        .id
}

async fn user(pool: &PgPool, org: Uuid) -> Uuid {
    let new = NewUser {
        display_name: "Pat Example".into(),
        given_name: "Pat".into(),
        family_name: "Example".into(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Idp,
        external_id: None,
    };
    users::create(pool, scope(org), &new, now())
        .await
        .expect("create user")
        .id
}

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|&key| key.to_owned()).collect()
}

async fn replace(
    pool: &PgPool,
    org: Uuid,
    user: Uuid,
    source: GroupSource,
    given: &[&str],
) -> Result<Replaced, Error> {
    groups::replace_memberships(pool, scope(org), user, source, &keys(given), now()).await
}

/// `user`'s memberships in `org`: `(group key, membership source)`, sorted.
async fn memberships(pool: &PgPool, org: Uuid, user: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT g.key, m.source FROM group_members m
         JOIN groups g ON g.org_id = m.org_id AND g.id = m.group_id
         WHERE m.org_id = $1 AND m.user_id = $2
         ORDER BY g.key, m.source",
    )
    .bind(org)
    .bind(user)
    .fetch_all(pool)
    .await
    .expect("read memberships")
}

/// `org`'s groups: `(key, name, source)`, sorted.
async fn groups_of(pool: &PgPool, org: Uuid) -> Vec<(String, String, String)> {
    sqlx::query_as("SELECT key, name, source FROM groups WHERE org_id = $1 ORDER BY key")
        .bind(org)
        .fetch_all(pool)
        .await
        .expect("read groups")
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

/// Q8 (a): unknown keys become groups named by their keys, recording the source that created
/// them; a second call adds what is new and removes what that source no longer names.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_source_replaces_its_memberships_and_creates_groups_on_first_use(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;

    let first = replace(
        &pool,
        org,
        pat,
        GroupSource::Idp,
        &["staff", "home", "staff"],
    )
    .await;
    assert_eq!(
        first.unwrap(),
        Replaced {
            added: 2,
            removed: 0
        }
    );
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp"), ("staff", "idp")])
    );
    let created = groups_of(&pool, org).await;
    assert_eq!(created.len(), 2);
    assert!(
        created
            .iter()
            .all(|(key, name, source)| key == name && source == "idp")
    );

    let second = replace(&pool, org, pat, GroupSource::Idp, &["home", "mentor"]).await;
    assert_eq!(
        second.unwrap(),
        Replaced {
            added: 1,
            removed: 1
        }
    );
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp"), ("mentor", "idp")])
    );
    assert_eq!(groups_of(&pool, org).await.len(), 3, "groups are kept");

    let same = replace(&pool, org, pat, GroupSource::Idp, &["mentor", "home"]).await;
    assert_eq!(same.unwrap(), Replaced::default());
    let none = replace(&pool, org, pat, GroupSource::Idp, &[]).await;
    assert_eq!(
        none.unwrap(),
        Replaced {
            added: 0,
            removed: 2
        }
    );
    assert!(memberships(&pool, org, pat).await.is_empty());
}

/// Q8: each source removes only its own memberships, so a group two sources give stays until
/// both stop giving it. A group keeps the source that created it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_source_never_touches_another_sources_memberships(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;
    let webhook = GroupSource::AdmissionWebhook;
    replace(&pool, org, pat, GroupSource::Idp, &["home", "staff"])
        .await
        .unwrap();
    replace(&pool, org, pat, webhook, &["staff", "mentor"])
        .await
        .unwrap();
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[
            ("home", "idp"),
            ("mentor", "admission_webhook"),
            ("staff", "admission_webhook"),
            ("staff", "idp"),
        ])
    );

    replace(&pool, org, pat, GroupSource::Idp, &[])
        .await
        .unwrap();
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[
            ("mentor", "admission_webhook"),
            ("staff", "admission_webhook")
        ])
    );
    let sources: Vec<(String, String)> = groups_of(&pool, org)
        .await
        .into_iter()
        .map(|(key, _, source)| (key, source))
        .collect();
    assert_eq!(
        sources,
        pairs(&[
            ("home", "idp"),
            ("mentor", "admission_webhook"),
            ("staff", "idp")
        ])
    );
}

/// Another user's memberships from the same source are not touched, and keys are matched
/// exactly: `Staff` and `staff` are two groups.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn only_the_users_memberships_change_and_keys_are_exact(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let (pat, sam) = (user(&pool, org).await, user(&pool, org).await);
    replace(&pool, org, sam, GroupSource::Idp, &["staff"])
        .await
        .unwrap();
    replace(&pool, org, pat, GroupSource::Idp, &["Staff"])
        .await
        .unwrap();
    replace(&pool, org, pat, GroupSource::Idp, &[])
        .await
        .unwrap();

    assert_eq!(
        memberships(&pool, org, sam).await,
        pairs(&[("staff", "idp")])
    );
    assert_eq!(groups_of(&pool, org).await.len(), 2);
}

/// Tenancy: the same key in two organizations is two groups, a change in one leaves the other
/// as it was, and another organization's user cannot be given a membership.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn organizations_never_share_groups_or_memberships(pool: PgPool) {
    let (a, b) = (org(&pool, "org-a").await, org(&pool, "org-b").await);
    let (pat, robin) = (user(&pool, a).await, user(&pool, b).await);
    replace(&pool, a, pat, GroupSource::Idp, &["staff"])
        .await
        .unwrap();
    replace(&pool, b, robin, GroupSource::Idp, &["staff"])
        .await
        .unwrap();
    replace(&pool, a, pat, GroupSource::Idp, &[]).await.unwrap();

    assert_eq!(
        memberships(&pool, b, robin).await,
        pairs(&[("staff", "idp")])
    );
    assert_eq!(groups_of(&pool, a).await.len(), 1);
    assert_eq!(groups_of(&pool, b).await.len(), 1);

    let crossed = replace(&pool, a, robin, GroupSource::Idp, &["staff"]).await;
    assert!(matches!(crossed, Err(Error::Database(_))), "{crossed:?}");
    assert!(memberships(&pool, a, robin).await.is_empty());
    assert_eq!(
        memberships(&pool, b, robin).await,
        pairs(&[("staff", "idp")])
    );
}

/// An unusable key or too many keys refuse the whole call, before anything is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unusable_or_too_many_keys_write_nothing(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;
    replace(&pool, org, pat, GroupSource::Idp, &["home"])
        .await
        .unwrap();

    let long = "k".repeat(groups::MAX_KEY_CHARS + 1);
    for bad in ["", "a\nb", "a\u{85}b", long.as_str()] {
        let refused = replace(&pool, org, pat, GroupSource::Idp, &["staff", bad]).await;
        assert!(matches!(refused, Err(Error::UnusableKey)), "{bad:?}");
    }
    let many: Vec<String> = (0..=MAX_KEYS).map(|n| format!("g{n}")).collect();
    let refused =
        groups::replace_memberships(&pool, scope(org), pat, GroupSource::Idp, &many, now()).await;
    assert!(matches!(refused, Err(Error::TooManyKeys)));
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp")])
    );
    assert_eq!(groups_of(&pool, org).await.len(), 1);

    let most: Vec<String> = (0..MAX_KEYS).map(|n| format!("g{n}")).collect();
    let stored =
        groups::replace_memberships(&pool, scope(org), pat, GroupSource::Idp, &most, now()).await;
    assert_eq!(stored.unwrap().added, MAX_KEYS as u64);
}

/// The tables' checks hold the rules the repository checks: a key with a control character,
/// an unknown source and a membership of another organization's group are refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_tables_refuse_what_the_repository_refuses(pool: PgPool) {
    let (a, b) = (org(&pool, "org-a").await, org(&pool, "org-b").await);
    let pat = user(&pool, a).await;
    let insert = |org: Uuid, key: &str, source: &str| {
        sqlx::query(
            "INSERT INTO groups (id, org_id, key, name, source) VALUES ($1, $2, $3, $3, $4)",
        )
        .bind(new_id())
        .bind(org)
        .bind(key.to_owned())
        .bind(source.to_owned())
        .execute(&pool)
    };
    assert!(insert(a, "a\u{1}b", "idp").await.is_err());
    assert!(insert(a, "a\u{9f}b", "idp").await.is_err());
    assert!(insert(a, "", "idp").await.is_err());
    assert!(insert(a, "staff", "ldap").await.is_err());
    assert!(
        insert(a, "a\u{2028}b", "idp").await.is_ok(),
        "only control characters"
    );
    insert(b, "staff", "idp").await.expect("a group of b");
    let b_staff: Uuid = sqlx::query_scalar("SELECT id FROM groups WHERE org_id = $1")
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
    let crossed = sqlx::query(
        "INSERT INTO group_members (org_id, group_id, user_id, source) VALUES ($1, $2, $3, 'idp')",
    )
    .bind(a)
    .bind(b_staff)
    .bind(pat)
    .execute(&pool)
    .await;
    assert!(crossed.is_err());
}
