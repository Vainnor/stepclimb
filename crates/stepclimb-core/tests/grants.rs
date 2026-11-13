// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission evaluator against a fresh database per test
//! (docs/specs/m0/09-permissions-and-roles.md B9, B16 to B18; T2's database half, T7's evaluator
//! part, T12's evaluator part; docs/specs/m0/02-organizations-and-accounts.md `ancestors_path`).
//! Needs `DATABASE_URL` (`just up`).
//!
//! Each test builds two organizations, A and B, each with Tower and Enroute under the root and
//! Ground under Tower, and a user in each.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    grants::{Denied, Grants, RequireError, ScopeId, Target},
    id::{Uuid, new_id},
    organizations,
    perm::{AccountManage, AccountView, OrgManage, RoleView},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole, RolePatch},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20Z.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the evaluator is tested below the credential resolvers; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Facility {
    org: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    enroute: Uuid,
    user: Uuid,
}

impl Facility {
    fn scope(&self) -> OrgScope {
        scope(self.org)
    }
}

async fn facility(pool: &PgPool, slug: &str) -> Facility {
    roles::sync_builtin(pool, request_time())
        .await
        .expect("sync built-in roles");
    let org = organizations::create(pool, slug, slug, request_time())
        .await
        .expect("create organization")
        .id;
    let root =
        sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
            .bind(org)
            .fetch_one(pool)
            .await
            .expect("the root account");
    let account = |parent, name| async move {
        accounts::create(
            pool,
            scope(org),
            parent,
            name,
            request_time(),
            Actor::Instance,
        )
        .await
        .expect("create account")
        .id
    };
    let tower = account(root, "Tower").await;
    let ground = account(tower, "Ground").await;
    let enroute = account(root, "Enroute").await;
    let new_user = NewUser {
        display_name: "Pat Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let user = users::create(pool, scope(org), &new_user, request_time())
        .await
        .expect("create user")
        .id;
    Facility {
        org,
        root,
        tower,
        ground,
        enroute,
        user,
    }
}

async fn builtin_id(pool: &PgPool, key: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the built-in row")
}

async fn custom_role(pool: &PgPool, f: &Facility, key: &str, permissions: &[&str]) -> Uuid {
    let role = NewRole {
        key: key.into(),
        name: key.into(),
        permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
    };
    roles::create_custom(pool, f.scope(), &role, request_time(), Actor::Instance)
        .await
        .expect("create role")
        .id
}

async fn assign(pool: &PgPool, f: &Facility, role_id: Uuid, at: Scope) -> Uuid {
    let assignment = NewAssignment {
        user_id: f.user,
        role_id,
        scope: at,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(
        pool,
        f.scope(),
        &assignment,
        request_time(),
        Actor::Instance,
    )
    .await
    .expect("create assignment")
    .id
}

async fn load(pool: &PgPool, f: &Facility) -> Grants {
    Grants::load(pool, f.scope(), f.user)
        .await
        .expect("load grants")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ancestors_path_is_root_first_and_scoped_to_the_organization(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let path = |id| accounts::ancestors_path(&pool, a.scope(), id);
    assert_eq!(path(a.root).await.unwrap(), Some(vec![a.root]));
    assert_eq!(
        path(a.ground).await.unwrap(),
        Some(vec![a.root, a.tower, a.ground])
    );
    assert_eq!(path(b.tower).await.unwrap(), None);
    assert_eq!(path(new_id()).await.unwrap(), None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn loaded_grants_follow_the_tree(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    assign(
        &pool,
        &a,
        builtin_id(&pool, "account_admin").await,
        Scope::Account(a.tower),
    )
    .await;
    let grants = load(&pool, &a).await;
    let ground = Target::Account(vec![a.root, a.tower, a.ground]);
    let enroute = Target::Account(vec![a.root, a.enroute]);
    assert_eq!(grants.check::<AccountManage>(&ground), Ok(()));
    assert_eq!(
        grants.check::<AccountView>(&enroute),
        Err(Denied::AccountNotFound)
    );
    assert_eq!(
        grants.check::<OrgManage>(&Target::Organization),
        Err(Denied::Forbidden)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_built_in_role_grants_its_code_list_whatever_its_row_says(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    assign(
        &pool,
        &a,
        builtin_id(&pool, "student").await,
        Scope::Organization,
    )
    .await;
    sqlx::query("UPDATE roles SET permissions = '{organization.manage}' WHERE key = 'student'")
        .execute(&pool)
        .await
        .unwrap();
    let grants = load(&pool, &a).await;
    assert!(!grants.holds_anywhere::<OrgManage>());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_changed_custom_role_takes_effect_on_the_next_load(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let role = custom_role(&pool, &a, "mentor", &["account.view", "role.view"]).await;
    assign(&pool, &a, role, Scope::Account(a.tower)).await;
    let tower = Target::Account(vec![a.root, a.tower]);
    assert_eq!(load(&pool, &a).await.check::<AccountView>(&tower), Ok(()));

    let patch = RolePatch {
        name: None,
        permissions: Some(vec!["role.view".into()]),
    };
    roles::update_custom(
        &pool,
        a.scope(),
        role,
        &patch,
        request_time(),
        Actor::Instance,
    )
    .await
    .expect("update role");
    assert_eq!(
        load(&pool, &a).await.check::<AccountView>(&tower),
        Err(Denied::Forbidden)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_removed_catalog_key_is_ignored(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let role = custom_role(&pool, &a, "mentor", &["account.view"]).await;
    sqlx::query("UPDATE roles SET permissions = '{account.view,course.retired}' WHERE id = $1")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    assign(&pool, &a, role, Scope::Organization).await;
    let grants = load(&pool, &a).await;
    assert!(grants.holds::<AccountView>(&Target::Organization));
    assert_eq!(
        grants.ignored_keys().collect::<Vec<_>>(),
        ["course.retired"]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_scope_row_grants_nothing(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    // The repository refuses course scope, so the row is written by SQL.
    sqlx::query(
        "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
         VALUES ($1, $2, $3, $4, 'course', $5)",
    )
    .bind(new_id())
    .bind(a.org)
    .bind(a.user)
    .bind(builtin_id(&pool, "org_admin").await)
    .bind(new_id())
    .execute(&pool)
    .await
    .unwrap();
    let grants = load(&pool, &a).await;
    assert!(!grants.holds_anywhere::<AccountView>());
    assert!(!grants.is_visible(&Target::Account(vec![a.root])));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn grants_are_loaded_in_the_scope_organization_only(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    assign(
        &pool,
        &a,
        builtin_id(&pool, "org_admin").await,
        Scope::Organization,
    )
    .await;
    // Org A's user, read with org B's scope, holds nothing there.
    let grants = Grants::load(&pool, b.scope(), a.user).await.unwrap();
    assert!(!grants.holds_anywhere::<RoleView>());
    assert_eq!(grants.scope(), b.scope());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn require_at_resolves_then_checks(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    assign(
        &pool,
        &a,
        builtin_id(&pool, "account_admin").await,
        Scope::Account(a.tower),
    )
    .await;
    let grants = load(&pool, &a).await;
    let at = |id| grants.require_at::<AccountManage>(&pool, ScopeId::Account(id));

    assert_eq!(
        at(a.ground).await.unwrap(),
        Target::Account(vec![a.root, a.tower, a.ground])
    );
    for hidden in [a.enroute, a.root, b.tower, b.root, new_id()] {
        assert!(
            matches!(
                at(hidden).await,
                Err(RequireError::Denied(Denied::AccountNotFound))
            ),
            "{hidden}"
        );
    }
    assert!(matches!(
        grants
            .require_at::<OrgManage>(&pool, ScopeId::Organization(a.org))
            .await,
        Err(RequireError::Denied(Denied::Forbidden))
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_org_admin_of_a_cannot_reach_org_b_accounts(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    assign(
        &pool,
        &a,
        builtin_id(&pool, "org_admin").await,
        Scope::Organization,
    )
    .await;
    let grants = load(&pool, &a).await;
    for id in [b.root, b.tower, b.ground] {
        assert!(matches!(
            grants
                .require_at::<AccountView>(&pool, ScopeId::Account(id))
                .await,
            Err(RequireError::Denied(Denied::AccountNotFound))
        ));
    }
    assert_eq!(
        grants
            .require_at::<OrgManage>(&pool, ScopeId::Organization(a.org))
            .await
            .unwrap(),
        Target::Organization
    );
    // Naming org B (or an id that is no organization) as the organization target is refused
    // before any permission is looked at.
    for org in [b.org, new_id()] {
        assert!(matches!(
            grants
                .require_at::<OrgManage>(&pool, ScopeId::Organization(org))
                .await,
            Err(RequireError::Denied(Denied::OrganizationNotFound))
        ));
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_api_key_is_limited_to_its_scopes_and_its_owner_current_grants(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let assignment = assign(
        &pool,
        &a,
        builtin_id(&pool, "account_admin").await,
        Scope::Account(a.tower),
    )
    .await;
    let tower = ScopeId::Account(a.tower);

    // Scopes that do not cover `account.manage`: Tower stays visible, so 403.
    let mut narrow = load(&pool, &a).await;
    narrow.intersect_key_scopes(&["account.view"]);
    assert!(matches!(
        narrow.require_at::<AccountManage>(&pool, tower).await,
        Err(RequireError::Denied(Denied::Forbidden))
    ));

    // Scopes that cover it, after the owner loses the assignment: 404 on the next evaluation.
    let mut covering = load(&pool, &a).await;
    covering.intersect_key_scopes(&["account.manage"]);
    assert!(
        covering
            .require_at::<AccountManage>(&pool, tower)
            .await
            .is_ok()
    );
    role_assignments::delete(&pool, a.scope(), assignment, UNIX_EPOCH, Actor::Instance)
        .await
        .unwrap();
    let mut after = load(&pool, &a).await;
    after.intersect_key_scopes(&["account.manage"]);
    assert!(matches!(
        after.require_at::<AccountManage>(&pool, tower).await,
        Err(RequireError::Denied(Denied::AccountNotFound))
    ));
}
