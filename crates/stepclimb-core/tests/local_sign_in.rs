// SPDX-License-Identifier: AGPL-3.0-or-later
//! The local provider row and the method switches against a fresh database per test
//! (docs/specs/m0/04-local-accounts.md B1 to B3, T1's switch part). Needs `DATABASE_URL`
//! (`just up`).

use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    audit::{self, Actor},
    id::{Uuid, new_id},
    identity_providers::{self, Kind},
    local_sign_in::{self, Error, LocalMethods, LocalSignIn, Method},
    organizations,
};

fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn later() -> SystemTime {
    request_time() + Duration::from_secs(60)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn admin() -> Actor {
    Actor::User {
        id: new_id(),
        ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)),
    }
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, request_time())
        .await
        .expect("create organization")
        .id
}

/// The organization's local row as `(id, kind, enabled, config)`.
async fn local_row(pool: &PgPool, org: Uuid) -> Option<(Uuid, String, bool, String)> {
    sqlx::query_as(
        "SELECT id, kind, enabled, config::text FROM identity_providers
         WHERE org_id = $1 AND key = 'local'",
    )
    .bind(org)
    .fetch_optional(pool)
    .await
    .expect("read the local row")
}

async fn insert_provider(pool: &PgPool, org: Uuid, key: &str, kind: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config) VALUES ($1, $2, $3, $4, '{}')",
    )
    .bind(new_id())
    .bind(org)
    .bind(key)
    .bind(kind)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Every row of the organization that the switches touch, as text.
async fn state(pool: &PgPool, org: Uuid) -> (String, Option<String>, i64) {
    sqlx::query_as(
        "SELECT o.settings::text || o.updated_at::text,
                (SELECT to_jsonb(p)::text FROM identity_providers p
                 WHERE p.org_id = o.id AND p.key = 'local'),
                (SELECT count(*) FROM audit_events a WHERE a.org_id = o.id)
         FROM organizations o WHERE o.id = $1",
    )
    .bind(org)
    .fetch_one(pool)
    .await
    .expect("read state")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_organization_has_an_enabled_local_row_and_the_default_switches(pool: PgPool) {
    let a = org(&pool, "org-a").await;

    let (id, kind, enabled, config) = local_row(&pool, a).await.expect("a local row");
    assert_eq!(
        (kind.as_str(), enabled, config.as_str()),
        ("local", true, "{}")
    );
    let provider = identity_providers::get_by_key(&pool, scope(a), "local")
        .await
        .unwrap()
        .expect("readable through the repository");
    assert_eq!((provider.id, provider.kind), (id, Kind::Local));

    let read = local_sign_in::get(&pool, scope(a)).await.unwrap();
    assert_eq!(
        read,
        LocalSignIn {
            enabled: true,
            methods: LocalMethods::default(),
        }
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ensure_row_fills_in_a_missing_row_and_leaves_an_existing_one(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let before = local_row(&pool, a).await;
    assert!(
        !local_sign_in::ensure_row(&pool, scope(a), later())
            .await
            .unwrap()
    );
    assert_eq!(local_row(&pool, a).await, before);

    let bare = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'bare', 'Bare')")
        .bind(bare)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        local_sign_in::get(&pool, scope(bare)).await,
        Err(Error::RowMissing)
    ));
    assert!(
        local_sign_in::ensure_row(&pool, scope(bare), later())
            .await
            .unwrap()
    );
    let (id, _, enabled, _) = local_row(&pool, bare).await.expect("inserted");
    assert!(enabled);
    assert_eq!(id.get_version_num(), 7, "a UUID v7 key");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_kind_and_the_key_local_go_together(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    assert!(insert_provider(&pool, a, "local", "oidc").await.is_err());
    assert!(insert_provider(&pool, a, "staff", "local").await.is_err());
    assert!(
        insert_provider(&pool, a, "local", "local").await.is_err(),
        "one per organization"
    );
    assert!(insert_provider(&pool, a, "staff", "oidc").await.is_ok());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_of_an_unknown_organization_is_not_found(pool: PgPool) {
    assert!(matches!(
        local_sign_in::get(&pool, scope(new_id())).await,
        Err(Error::NotFound)
    ));
}

/// T1, switch part: a switched-off method is not offered and comes back when switched on; org B
/// is untouched; each half writes its own audit entry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_switches_methods_and_the_row_in_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let b_before = state(&pool, b).await;
    let off = LocalSignIn {
        enabled: true,
        methods: LocalMethods {
            passkey: false,
            ..LocalMethods::default()
        },
    };

    let after = local_sign_in::update(&pool, scope(a), off, later(), admin())
        .await
        .unwrap();

    assert_eq!(after, off);
    let read = local_sign_in::get(&pool, scope(a)).await.unwrap();
    assert_eq!(read, off);
    assert!(!read.offers(Method::Passkey));
    assert!(read.offers(Method::Password));
    assert_eq!(state(&pool, b).await, b_before, "org B is unchanged");

    let on = LocalSignIn {
        enabled: true,
        methods: LocalMethods::default(),
    };
    local_sign_in::update(&pool, scope(a), on, later(), admin())
        .await
        .unwrap();
    assert!(
        local_sign_in::get(&pool, scope(a))
            .await
            .unwrap()
            .offers(Method::Passkey)
    );

    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        [
            "organization.create",
            "organization.update_settings",
            "organization.update_settings"
        ]
    );
    let first = &entries[1];
    assert_eq!(
        first.data,
        json!({
            "before": { "local_sign_in": { "passkey": true, "password": true, "totp": true, "magic_link": false } },
            "after": { "local_sign_in": { "passkey": false, "password": true, "totp": true, "magic_link": false } },
        })
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn disabling_the_row_offers_no_method_and_is_audited(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    // An outside provider to sign in with, so disabling local sign-in locks no one out.
    insert_provider(&pool, a, "example-oidc", "oidc")
        .await
        .unwrap();
    let (row_id, ..) = local_row(&pool, a).await.unwrap();
    let disabled = LocalSignIn {
        enabled: false,
        methods: LocalMethods::default(),
    };

    local_sign_in::update(&pool, scope(a), disabled, later(), admin())
        .await
        .unwrap();

    let read = local_sign_in::get(&pool, scope(a)).await.unwrap();
    assert_eq!(read, disabled);
    assert!(read.offered().is_empty());
    let entries = audit::list(&pool, scope(a), 100).await.unwrap();
    let last = entries.last().unwrap();
    assert_eq!(
        (
            last.action.as_str(),
            last.target_type.as_str(),
            last.target_id
        ),
        ("identity_provider.update", "identity_provider", row_id)
    );
    assert_eq!(
        last.data,
        json!({ "before": { "enabled": true }, "after": { "enabled": false } })
    );
    assert_eq!(entries.len(), 2, "the unchanged switches write no entry");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_update_that_changes_nothing_writes_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let before = state(&pool, a).await;
    let same = local_sign_in::get(&pool, scope(a)).await.unwrap();

    local_sign_in::update(&pool, scope(a), same, later(), admin())
        .await
        .unwrap();

    assert_eq!(state(&pool, a).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_of_an_organization_without_a_row_changes_nothing(pool: PgPool) {
    let bare = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'bare', 'Bare')")
        .bind(bare)
        .execute(&pool)
        .await
        .unwrap();
    let before = state(&pool, bare).await;
    let off = LocalSignIn {
        enabled: false,
        methods: LocalMethods {
            password: false,
            ..LocalMethods::default()
        },
    };

    let result = local_sign_in::update(&pool, scope(bare), off, later(), admin()).await;

    assert!(matches!(result, Err(Error::RowMissing)), "{result:?}");
    assert_eq!(
        state(&pool, bare).await,
        before,
        "the settings write rolled back"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_refuses_the_magic_link_and_a_lockout_writing_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let before = state(&pool, a).await;
    let magic = LocalSignIn {
        enabled: true,
        methods: LocalMethods {
            magic_link: true,
            ..LocalMethods::default()
        },
    };
    let result = local_sign_in::update(&pool, scope(a), magic, later(), admin()).await;
    assert!(
        matches!(result, Err(Error::MagicLinkNeedsEmail)),
        "{result:?}"
    );

    let totp_only = LocalSignIn {
        enabled: true,
        methods: LocalMethods {
            passkey: false,
            password: false,
            totp: true,
            magic_link: false,
        },
    };
    let row_off = LocalSignIn {
        enabled: false,
        methods: LocalMethods::default(),
    };
    for new in [totp_only, row_off] {
        let result = local_sign_in::update(&pool, scope(a), new, later(), admin()).await;
        assert!(
            matches!(result, Err(Error::LastMethod)),
            "{new:?}: {result:?}"
        );
    }
    assert_eq!(state(&pool, a).await, before);

    // Another organization's enabled provider does not count.
    let b = org(&pool, "org-b").await;
    insert_provider(&pool, b, "example-oidc", "oidc")
        .await
        .unwrap();
    let result = local_sign_in::update(&pool, scope(a), row_off, later(), admin()).await;
    assert!(matches!(result, Err(Error::LastMethod)), "{result:?}");
    insert_provider(&pool, a, "example-oidc", "oidc")
        .await
        .unwrap();
    let result = local_sign_in::update(&pool, scope(a), row_off, later(), admin()).await;
    assert_eq!(result.unwrap(), row_off);
}
