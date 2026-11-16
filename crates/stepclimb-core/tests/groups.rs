// SPDX-License-Identifier: AGPL-3.0-or-later
//! The groups repository and its tables against a fresh database per test
//! (docs/specs/m0/07-admission.md B18, B22, Q8 (a), I7). All data is synthetic. Needs
//! `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    groups::{self, Error, Giver, MAX_GROUPS, MAX_KEYS, Replaced},
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

async fn org_named(pool: &PgPool, slug: &str) -> Uuid {
    org(pool, slug).await
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

/// The giver of `org`'s provider `sso`, created on first use.
async fn idp(pool: &PgPool, org: Uuid) -> Giver {
    Giver::Provider(provider(pool, org, "sso").await)
}

/// The id of `org`'s `oidc` provider `key`, created on first use.
async fn provider(pool: &PgPool, org: Uuid, key: &str) -> Uuid {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, $3, 'oidc', '{}'::jsonb) ON CONFLICT (org_id, key) DO NOTHING",
    )
    .bind(new_id())
    .bind(org)
    .bind(key)
    .execute(pool)
    .await
    .expect("insert provider");
    sqlx::query_scalar("SELECT id FROM identity_providers WHERE org_id = $1 AND key = $2")
        .bind(org)
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the provider")
}

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|&key| key.to_owned()).collect()
}

async fn replace(
    pool: &PgPool,
    org: Uuid,
    user: Uuid,
    giver: Giver,
    given: &[&str],
) -> Result<Replaced, Error> {
    groups::replace_memberships(pool, scope(org), user, giver, &keys(given), now()).await
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
        idp(&pool, org).await,
        &["staff", "home", "staff"],
    )
    .await;
    assert_eq!(
        first.unwrap(),
        Replaced {
            added: 2,
            removed: 0,
            dropped: 0,
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

    let second = replace(&pool, org, pat, idp(&pool, org).await, &["home", "mentor"]).await;
    assert_eq!(
        second.unwrap(),
        Replaced {
            added: 1,
            removed: 1,
            dropped: 0,
        }
    );
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp"), ("mentor", "idp")])
    );
    assert_eq!(groups_of(&pool, org).await.len(), 3, "groups are kept");

    let same = replace(&pool, org, pat, idp(&pool, org).await, &["mentor", "home"]).await;
    assert_eq!(same.unwrap(), Replaced::default());
    let none = replace(&pool, org, pat, idp(&pool, org).await, &[]).await;
    assert_eq!(
        none.unwrap(),
        Replaced {
            added: 0,
            removed: 2,
            dropped: 0,
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
    let webhook = Giver::AdmissionWebhook;
    replace(&pool, org, pat, idp(&pool, org).await, &["home", "staff"])
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

    replace(&pool, org, pat, idp(&pool, org).await, &[])
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
    replace(&pool, org, sam, idp(&pool, org).await, &["staff"])
        .await
        .unwrap();
    replace(&pool, org, pat, idp(&pool, org).await, &["Staff"])
        .await
        .unwrap();
    replace(&pool, org, pat, idp(&pool, org).await, &[])
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
    replace(&pool, a, pat, idp(&pool, a).await, &["staff"])
        .await
        .unwrap();
    replace(&pool, b, robin, idp(&pool, b).await, &["staff"])
        .await
        .unwrap();
    replace(&pool, a, pat, idp(&pool, a).await, &[])
        .await
        .unwrap();

    assert_eq!(
        memberships(&pool, b, robin).await,
        pairs(&[("staff", "idp")])
    );
    assert_eq!(groups_of(&pool, a).await.len(), 1);
    assert_eq!(groups_of(&pool, b).await.len(), 1);

    // B's user, emptied under A's scope by each giver: B's rows are not A's to remove.
    let webhook = Giver::AdmissionWebhook;
    replace(&pool, b, robin, webhook, &["mentor"])
        .await
        .unwrap();
    let robins = pairs(&[("mentor", "admission_webhook"), ("staff", "idp")]);
    for giver in [idp(&pool, a).await, webhook] {
        let emptied = replace(&pool, a, robin, giver, &[]).await;
        assert_eq!(emptied.unwrap(), Replaced::default(), "{giver:?}");
        assert_eq!(memberships(&pool, b, robin).await, robins, "{giver:?}");
    }
    let crossed = replace(&pool, a, robin, idp(&pool, a).await, &["staff"]).await;
    assert!(matches!(crossed, Err(Error::Database(_))), "{crossed:?}");
    assert!(memberships(&pool, a, robin).await.is_empty());
    assert_eq!(memberships(&pool, b, robin).await, robins);
}

/// An unusable key or too many keys refuse the whole call, before anything is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unusable_or_too_many_keys_write_nothing(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;
    replace(&pool, org, pat, idp(&pool, org).await, &["home"])
        .await
        .unwrap();

    let long = "k".repeat(groups::MAX_KEY_CHARS + 1);
    for bad in ["", "a\nb", "a\u{85}b", long.as_str()] {
        let refused = replace(&pool, org, pat, idp(&pool, org).await, &["staff", bad]).await;
        assert!(matches!(refused, Err(Error::UnusableKey)), "{bad:?}");
    }
    let many: Vec<String> = (0..=MAX_KEYS).map(|n| format!("g{n}")).collect();
    let refused =
        groups::replace_memberships(&pool, scope(org), pat, idp(&pool, org).await, &many, now())
            .await;
    assert!(matches!(refused, Err(Error::TooManyKeys)));
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp")])
    );
    assert_eq!(groups_of(&pool, org).await.len(), 1);

    let most: Vec<String> = (0..MAX_KEYS).map(|n| format!("g{n}")).collect();
    let stored =
        groups::replace_memberships(&pool, scope(org), pat, idp(&pool, org).await, &most, now())
            .await;
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
    insert(b, "staff", "idp").await.expect("a group of b");
    insert(a, "home", "idp").await.expect("a group of a");
    let group_of = |org: Uuid| {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM groups WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
    };
    let (a_home, b_staff) = (group_of(a).await.unwrap(), group_of(b).await.unwrap());
    let (sso_a, sso_b) = (
        provider(&pool, a, "sso").await,
        provider(&pool, b, "sso").await,
    );
    let member = |group: Uuid, source: &str, provider: Option<Uuid>| {
        sqlx::query(
            "INSERT INTO group_members (org_id, group_id, user_id, source, provider_id)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(a)
        .bind(group)
        .bind(pat)
        .bind(source.to_owned())
        .bind(provider)
        .execute(&pool)
    };
    assert!(
        member(b_staff, "admission_webhook", None).await.is_err(),
        "B's group"
    );
    assert!(
        member(a_home, "idp", Some(sso_b)).await.is_err(),
        "B's provider"
    );
    assert!(
        member(a_home, "idp", None).await.is_err(),
        "idp names its provider"
    );
    assert!(
        member(a_home, "admission_webhook", Some(sso_a))
            .await
            .is_err(),
        "only idp"
    );
    member(a_home, "idp", Some(sso_a))
        .await
        .expect("A's provider");
    member(a_home, "admission_webhook", None)
        .await
        .expect("the webhook's");
    assert!(
        member(a_home, "admission_webhook", None).await.is_err(),
        "once per giver"
    );
}

/// The key check refuses exactly what `usable_key` refuses: the first and last character of
/// every control and format range and the two separators, and accepts their neighbours that are
/// neither.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_key_check_matches_usable_key(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let ranges: [(u32, u32); 25] = [
        (0x01, 0x1F),
        (0x7F, 0x9F),
        (0x00AD, 0x00AD),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x2028, 0x2029),
        (0x202A, 0x202E),
        (0x2060, 0x2064),
        (0x2066, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0001, 0xE0001),
        (0xE0020, 0xE007F),
        (0x2065, 0x2065),
    ];
    let mut codes: Vec<u32> = ranges
        .iter()
        .flat_map(|&(first, last)| [first - 1, first, last, last + 1])
        .collect();
    codes.sort_unstable();
    codes.dedup();
    let mut checked = 0;
    for code in codes {
        let Some(c) = char::from_u32(code).filter(|&c| c != '\0') else {
            continue;
        };
        let key = format!("k{c}k");
        let stored = sqlx::query(
            "INSERT INTO groups (id, org_id, key, name, source) VALUES ($1, $2, $3, 'n', 'idp')",
        )
        .bind(new_id())
        .bind(org)
        .bind(&key)
        .execute(&pool)
        .await;
        assert_eq!(
            stored.is_ok(),
            groups::usable_key(&key),
            "U+{code:04X}: the table and usable_key disagree"
        );
        checked += 1;
    }
    assert_eq!(checked, 82, "every distinct code point but NUL");
}

/// Each provider replaces only its own claim memberships: a group two providers give stays
/// until both stop giving it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_provider_replaces_only_its_own_memberships(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;
    let sso = Giver::Provider(provider(&pool, org, "sso").await);
    let partner = Giver::Provider(provider(&pool, org, "partner").await);
    replace(&pool, org, pat, sso, &["staff", "home"])
        .await
        .unwrap();
    replace(&pool, org, pat, partner, &["staff"]).await.unwrap();
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("home", "idp"), ("staff", "idp"), ("staff", "idp")])
    );

    replace(&pool, org, pat, sso, &[]).await.unwrap();
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("staff", "idp")])
    );
    let from: Option<String> = sqlx::query_scalar(
        "SELECT p.key FROM group_members m JOIN identity_providers p ON p.id = m.provider_id
         WHERE m.user_id = $1",
    )
    .bind(pat)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(from.as_deref(), Some("partner"));

    sqlx::query("DELETE FROM identity_providers WHERE org_id = $1 AND key = 'partner'")
        .bind(org)
        .execute(&pool)
        .await
        .expect("a provider with memberships can be deleted");
    assert!(
        memberships(&pool, org, pat).await.is_empty(),
        "its memberships go with it"
    );
}

/// An organization has at most `MAX_GROUPS` groups: once full, a key naming no group makes no
/// group and no membership and is counted as dropped, while keys naming groups still count.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_full_organization_makes_no_new_group(pool: PgPool) {
    let org = org(&pool, "org-a").await;
    let pat = user(&pool, org).await;
    let fill = |count: usize| {
        sqlx::query(
            "INSERT INTO groups (id, org_id, key, name, source)
             SELECT gen_random_uuid(), $1, 'filler-' || n, 'filler-' || n, 'sync'
             FROM generate_series(1, $2) AS n",
        )
        .bind(org)
        .bind(i32::try_from(count).unwrap())
        .execute(&pool)
    };
    fill(MAX_GROUPS - 2).await.expect("fill");
    let sso = idp(&pool, org).await;

    let near = replace(
        &pool,
        org,
        pat,
        sso,
        &["new-b", "new-a", "new-c", "filler-1"],
    )
    .await;
    assert_eq!(
        near.unwrap(),
        Replaced {
            added: 3,
            removed: 0,
            dropped: 1,
        },
        "two places left: new-a and new-b, in key order"
    );
    assert_eq!(
        memberships(&pool, org, pat).await,
        pairs(&[("filler-1", "idp"), ("new-a", "idp"), ("new-b", "idp")])
    );

    let webhook = Giver::AdmissionWebhook;
    let full = replace(&pool, org, pat, webhook, &["new-c", "new-a"]).await;
    assert_eq!(
        full.unwrap(),
        Replaced {
            added: 1,
            removed: 0,
            dropped: 1,
        }
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM groups WHERE org_id = $1")
        .bind(org)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, MAX_GROUPS as i64);

    let other = org_named(&pool, "org-b").await;
    let robin = user(&pool, other).await;
    let fresh = replace(&pool, other, robin, idp(&pool, other).await, &["new-c"]).await;
    assert_eq!(fresh.unwrap().dropped, 0, "the cap is per organization");
}
