// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb_server::admin::create_organization_with_admin`, the transaction behind
//! `stepclimb admin create` (docs/specs/m0/02-organizations-and-accounts.md B27, T20). The
//! binary's own cases are in `tests/cli.rs`. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_auth::token::TokenHash;
use stepclimb_core::local_tokens;
use stepclimb_core::{
    OrgScope,
    audit::{self, Actor},
    organizations, perm,
    role_assignments::{self, GrantScope},
    roles, users,
};
use stepclimb_server::{
    admin::{CreateError, NewOrganizationAdmin, create_organization_with_admin},
    web_origin::WebOrigin,
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded write shows.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn public_url() -> WebOrigin {
    WebOrigin::parse("https://lms.example.org").expect("a valid origin")
}

fn input(slug: &str) -> NewOrganizationAdmin {
    NewOrganizationAdmin {
        org_slug: slug.to_owned(),
        org_name: "Example Training".to_owned(),
        email: "sam@example.test".to_owned(),
        name: "Sam Admin".to_owned(),
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test reads the new organization's rows as the CLI does"
)]
fn scope(org_id: stepclimb_core::id::Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .expect("count rows")
}

async fn assert_nothing_created(pool: &PgPool) {
    for table in [
        "organizations",
        "accounts",
        "org_plans",
        "users",
        "role_assignments",
        "local_tokens",
        "audit_events",
    ] {
        assert_eq!(count(pool, table).await, 0, "{table}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_first_user_holds_every_permission_at_organization_scope(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    let created = create_organization_with_admin(&pool, &input("example"), &public_url(), now())
        .await
        .unwrap();
    let scope = scope(created.organization.id);

    assert_eq!(
        organizations::get(&pool, scope).await.unwrap(),
        created.organization
    );
    let user = users::get(&pool, scope, created.user.id).await.unwrap();
    assert_eq!(user.as_ref(), Some(&created.user));
    assert_eq!(created.user.created_at, now());

    let grants = role_assignments::for_user_with_paths(&pool, scope, created.user.id)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].assignment_id, created.assignment_id);
    assert_eq!(grants[0].role_key, "org_admin");
    assert_eq!(grants[0].scope, GrantScope::Organization);
    let all: Vec<String> = perm::ALL.iter().map(|p| p.key().to_owned()).collect();
    assert_eq!(grants[0].permissions, all);
}

/// A failure after the organization and the user are inserted leaves neither: without the
/// built-in roles, the role lookup fails last.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_failure_after_the_user_is_inserted_leaves_nothing_behind(pool: PgPool) {
    let result =
        create_organization_with_admin(&pool, &input("example"), &public_url(), now()).await;
    assert!(
        matches!(result, Err(CreateError::RoleMissing)),
        "{result:?}"
    );
    assert_eq!(
        result
            .as_ref()
            .err()
            .and_then(CreateError::refused_argument),
        None
    );
    assert_nothing_created(&pool).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_value_names_its_flag_and_leaves_nothing_behind(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    let cases = [
        (
            NewOrganizationAdmin {
                org_slug: "-bad".to_owned(),
                ..input("unused")
            },
            "--org-slug",
        ),
        (
            NewOrganizationAdmin {
                org_name: "   ".to_owned(),
                ..input("example")
            },
            "--org-name",
        ),
        (
            NewOrganizationAdmin {
                org_name: "x".repeat(organizations::MAX_NAME_CHARS + 1),
                ..input("example")
            },
            "--org-name",
        ),
        (
            NewOrganizationAdmin {
                email: "\u{7}sam@example.test".to_owned(),
                ..input("example")
            },
            "--email",
        ),
        (
            NewOrganizationAdmin {
                name: String::new(),
                ..input("example")
            },
            "--name",
        ),
    ];
    for (input, flag) in cases {
        let error = create_organization_with_admin(&pool, &input, &public_url(), now())
            .await
            .expect_err("refused");
        assert_eq!(error.refused_argument(), Some(flag), "{error:?}");
        assert_nothing_created(&pool).await;
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_existing_organization_is_refused_and_nothing_is_written(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    organizations::create(&pool, "first", "First", now())
        .await
        .unwrap();
    let result =
        create_organization_with_admin(&pool, &input("second"), &public_url(), now()).await;
    let error = result.expect_err("refused");
    assert!(
        matches!(error, CreateError::OrganizationExists),
        "{error:?}"
    );
    assert!(error.is_refusal());
    assert_eq!(count(&pool, "organizations").await, 1);
    assert_eq!(count(&pool, "users").await, 0);
    assert_eq!(count(&pool, "role_assignments").await, 0);
}

/// The count holds until the commit: a create that starts while another transaction is creating
/// the first organization waits for it, then refuses, so two concurrent runs never make two.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_concurrent_create_waits_for_the_first_and_is_refused(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    let mut first = pool.begin().await.unwrap();
    assert_eq!(organizations::lock_and_count(&mut first).await.unwrap(), 0);
    organizations::create(&mut *first, "first", "First", now())
        .await
        .unwrap();

    let input = input("second");
    let public_url = public_url();
    let second = create_organization_with_admin(&pool, &input, &public_url, now());
    tokio::pin!(second);
    let early = tokio::time::timeout(Duration::from_millis(300), &mut second).await;
    assert!(early.is_err(), "the second create did not wait: {early:?}");
    first.commit().await.unwrap();

    let error = second.await.expect_err("refused");
    assert!(
        matches!(error, CreateError::OrganizationExists),
        "{error:?}"
    );
    assert_eq!(count(&pool, "organizations").await, 1);
    assert_eq!(count(&pool, "users").await, 0);
}

/// The command writes three audit entries by the instance: the organization (with its root
/// account and plan), the first user's Organization admin assignment and their enrollment link.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_command_audits_the_organization_and_the_first_admin_grant(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    let created = create_organization_with_admin(&pool, &input("example"), &public_url(), now())
        .await
        .unwrap();

    let entries = audit::list(&pool, scope(created.organization.id), 10)
        .await
        .unwrap();
    let got: Vec<_> = entries
        .iter()
        .map(|e| (e.actor, e.action.as_str(), e.target_id))
        .collect();
    assert_eq!(
        got,
        [
            (
                Actor::Instance,
                "organization.create",
                created.organization.id
            ),
            (
                Actor::Instance,
                "role_assignment.grant",
                created.assignment_id
            ),
            (
                Actor::Instance,
                "enrollment_link.create",
                created.setup_link.id
            ),
        ]
    );
    assert_eq!(
        entries[1].data["after"]["user_id"],
        created.user.id.to_string()
    );
}

/// Spec 04 B10, Q7: the report's link opens the web app's enrollment page with a token that the
/// enrollment start route accepts for the new admin, and the database holds only its hash.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_setup_link_enrolls_the_first_admin(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.unwrap();
    let created = create_organization_with_admin(&pool, &input("example"), &public_url(), now())
        .await
        .unwrap();
    let url = created.setup_link.expose_url();
    let token = url
        .strip_prefix("https://lms.example.org/enroll#token=")
        .unwrap_or_else(|| panic!("not an enrollment link: {url}"));
    assert_eq!(token.len(), 43, "{url}");
    let scope = scope(created.organization.id);
    let hash = TokenHash::of(token);
    let enrollment = local_tokens::check_enrollment(&pool, scope, hash.as_bytes(), now())
        .await
        .unwrap();
    assert_eq!(enrollment.user_id, created.user.id);
    assert_eq!(enrollment.email, "sam@example.test");
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM local_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, hash.as_bytes());
    let created_by: Option<stepclimb_core::id::Uuid> =
        sqlx::query_scalar("SELECT created_by FROM local_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(created_by, None);
}
