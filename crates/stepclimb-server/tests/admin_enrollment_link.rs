// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb_server::recovery::create_recovery_link`, the transaction behind `stepclimb admin
//! enrollment-link` (docs/specs/m0/04-local-accounts.md B10, coordinator decision 2026-11-14).
//! The binary's own cases are in `tests/cli.rs`. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_auth::token::TokenHash;
use stepclimb_core::{
    OrgScope, Source, audit,
    audit::Actor,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    local_tokens, roles,
    users::{self, NewUser},
};
use stepclimb_server::{
    admin::{Created, NewOrganizationAdmin, create_organization_with_admin},
    recovery::{RecoveryError, RecoveryRequest, UserRef, create_recovery_link},
    web_origin::WebOrigin,
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn public_url() -> WebOrigin {
    WebOrigin::parse("https://lms.example.org").expect("a valid origin")
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test reads the organization's rows as the CLI does"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An instance made by `admin create`: organization `example`, admin `sam@example.test`.
async fn instance(pool: &PgPool) -> Created {
    roles::sync_builtin(pool, now()).await.expect("sync roles");
    let input = NewOrganizationAdmin {
        org_slug: "example".into(),
        org_name: "Example Training".into(),
        email: "sam@example.test".into(),
        name: "Sam Admin".into(),
    };
    create_organization_with_admin(pool, &input, &public_url(), now())
        .await
        .expect("admin create")
}

fn request(user: UserRef) -> RecoveryRequest {
    RecoveryRequest {
        org_slug: "example".into(),
        user,
        admin_required_override: false,
        force: false,
    }
}

async fn recover(pool: &PgPool, request: &RecoveryRequest) -> Result<String, RecoveryError> {
    create_recovery_link(pool, request, &public_url(), now())
        .await
        .map(|recovered| recovered.link.expose_url().to_owned())
}

async fn check(pool: &PgPool, org: Uuid, url: &str) -> Result<Uuid, local_tokens::Error> {
    let token = url.split_once("#token=").expect("a token").1;
    let mut conn = pool.acquire().await.expect("a connection");
    let hash = TokenHash::of(token);
    local_tokens::check_enrollment(&mut conn, scope(org), hash.as_bytes(), now())
        .await
        .map(|enrollment| enrollment.user_id)
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .expect("count rows")
}

/// The admin gets a new link by email in any letter case or by id; it replaces the bootstrap
/// link, works without a creator, and is audited by the instance as a recovery.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_admin_gets_a_new_link_that_replaces_the_old_one(pool: PgPool) {
    let created = instance(&pool).await;
    let org = created.organization.id;
    let first = created.setup_link.expose_url().to_owned();

    let by_email = recover(&pool, &request(UserRef::Email("SAM@example.test".into())))
        .await
        .unwrap();
    assert!(by_email.starts_with("https://lms.example.org/enroll#token="));
    assert!(matches!(
        check(&pool, org, &first).await,
        Err(local_tokens::Error::Invalid)
    ));
    assert_eq!(check(&pool, org, &by_email).await.unwrap(), created.user.id);

    let by_id = recover(&pool, &request(UserRef::Id(created.user.id)))
        .await
        .unwrap();
    assert!(check(&pool, org, &by_email).await.is_err());
    assert_eq!(check(&pool, org, &by_id).await.unwrap(), created.user.id);
    assert_eq!(count(&pool, "SELECT count(*) FROM local_tokens").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM local_tokens WHERE created_by_instance AND created_by IS NULL"
        )
        .await,
        1
    );

    let entries = audit::list(&pool, scope(org), 20).await.unwrap();
    let last = entries.last().unwrap();
    assert_eq!(
        (last.actor, last.action.as_str()),
        (Actor::Instance, "enrollment_link.create")
    );
    assert_eq!(last.data["after"]["recovery"], json!(true));
    assert_eq!(last.data["after"]["user_id"], json!(created.user.id));
    let bootstrap = &entries[entries.len() - 3];
    assert_eq!(bootstrap.action, "enrollment_link.create");
    assert_eq!(bootstrap.data["after"].get("recovery"), None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_organization_or_user_is_refused(pool: PgPool) {
    let created = instance(&pool).await;
    let unknown_org = RecoveryRequest {
        org_slug: "other".into(),
        ..request(UserRef::Email("sam@example.test".into()))
    };
    let error = recover(&pool, &unknown_org).await.unwrap_err();
    assert!(
        matches!(error, RecoveryError::OrganizationNotFound),
        "{error:?}"
    );
    assert!(error.is_refusal());
    for user in [
        UserRef::Email("kim@example.test".into()),
        UserRef::Id(new_id()),
    ] {
        let error = recover(&pool, &request(user.clone())).await.unwrap_err();
        assert!(
            matches!(error, RecoveryError::UserNotFound),
            "{user:?}: {error:?}"
        );
    }
    // The bootstrap link still works: nothing was written.
    assert!(
        check(
            &pool,
            created.organization.id,
            created.setup_link.expose_url()
        )
        .await
        .is_ok()
    );
}

/// A user who is not an Organization admin at organization scope is refused unless the override
/// flag is given.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_who_is_not_an_org_admin_needs_the_override(pool: PgPool) {
    let created = instance(&pool).await;
    let org = created.organization.id;
    let new_user = NewUser {
        display_name: "Kim Learner".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: Some("kim@example.test".into()),
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let kim = users::create(&pool, scope(org), &new_user, now())
        .await
        .unwrap();
    let plain = request(UserRef::Id(kim.id));
    let error = recover(&pool, &plain).await.unwrap_err();
    assert!(matches!(error, RecoveryError::NotOrgAdmin), "{error:?}");
    assert!(error.is_refusal());
    assert_eq!(count(&pool, "SELECT count(*) FROM local_tokens").await, 1);

    let overridden = RecoveryRequest {
        admin_required_override: true,
        ..plain
    };
    let url = recover(&pool, &overridden).await.unwrap();
    assert_eq!(check(&pool, org, &url).await.unwrap(), kim.id);
}

/// A user with an identity at an enabled provider can sign in already and is refused unless
/// `--force`; one at a disabled provider is no way in and does not count.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_who_can_sign_in_needs_force(pool: PgPool) {
    let created = instance(&pool).await;
    let org = created.organization.id;
    let provider = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'example-oidc', 'oidc', '{}', false)",
    )
    .bind(provider)
    .bind(org)
    .execute(&pool)
    .await
    .unwrap();
    let identity = NewIdentity {
        user_id: created.user.id,
        provider_id: provider,
        subject: "1000001".into(),
        attributes: serde_json::Map::new(),
    };
    identities::link(&pool, scope(org), &identity, now(), Actor::Instance)
        .await
        .unwrap();
    let sam = request(UserRef::Email("sam@example.test".into()));
    assert!(
        recover(&pool, &sam).await.is_ok(),
        "an identity at a disabled provider does not count"
    );

    sqlx::query("UPDATE identity_providers SET enabled = true WHERE id = $1")
        .bind(provider)
        .execute(&pool)
        .await
        .unwrap();
    let error = recover(&pool, &sam).await.unwrap_err();
    assert!(matches!(error, RecoveryError::HasSignInMethod), "{error:?}");
    assert!(error.is_refusal());
    let forced = RecoveryRequest { force: true, ..sam };
    assert!(recover(&pool, &forced).await.is_ok());
}

/// A user with a passkey can sign in already (`local::has_credential`) and is refused unless
/// `--force`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_with_a_passkey_needs_force(pool: PgPool) {
    let created = instance(&pool).await;
    let org = created.organization.id;
    let credential = json!({ "cred": { "counter": 0 } });
    let passkey = stepclimb_core::passkeys::NewPasskey {
        user_id: created.user.id,
        credential_id: &[9; 16],
        credential: &credential,
        name: "Phone",
    };
    stepclimb_core::passkeys::create(&pool, scope(org), passkey, now(), Actor::Instance)
        .await
        .expect("store a passkey");
    let sam = request(UserRef::Email("sam@example.test".into()));
    let error = recover(&pool, &sam).await.unwrap_err();
    assert!(matches!(error, RecoveryError::HasSignInMethod), "{error:?}");
    let forced = RecoveryRequest { force: true, ..sam };
    assert!(recover(&pool, &forced).await.is_ok());
}
