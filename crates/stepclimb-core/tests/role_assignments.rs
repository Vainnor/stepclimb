// SPDX-License-Identifier: AGPL-3.0-or-later
//! The role assignments repository and the `role_assignments` table against a fresh database per
//! test (docs/specs/m0/09-permissions-and-roles.md B11, B15, B16; T4's evaluator read, T19;
//! docs/specs/m0/02-organizations-and-accounts.md B21, T14). Needs `DATABASE_URL` (`just up`).
//!
//! Each test builds two organizations, A and B, each with Tower, Ground (Tower's child), Ramp
//! (Ground's child) and Enroute under the root, a user and a custom role, and checks that org
//! A's scope reads and changes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{PgPool, Postgres};
use stepclimb_core::{
    OrgScope, Source, accounts,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, Error, GrantScope, ListFilter, NewAssignment, RoleAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser, Status},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Facility {
    org: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    ramp: Uuid,
    enroute: Uuid,
    user: Uuid,
    /// A custom role of this organization holding `account.view`.
    mentor: Uuid,
}

async fn user(pool: &PgPool, org: Uuid, name: &str) -> Uuid {
    let user = NewUser {
        display_name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    users::create(pool, scope(org), &user, request_time())
        .await
        .expect("create user")
        .id
}

async fn account(pool: &PgPool, org: Uuid, parent: Uuid, name: &str) -> Uuid {
    accounts::create(pool, scope(org), parent, name, request_time())
        .await
        .expect("create account")
        .id
}

/// A facility in a fresh organization. Syncs the built-in roles, which is idempotent.
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
    let tower = account(pool, org, root, "Tower").await;
    let ground = account(pool, org, tower, "Ground").await;
    let ramp = account(pool, org, ground, "Ramp").await;
    let enroute = account(pool, org, root, "Enroute").await;
    let mentor = roles::create_custom(
        pool,
        scope(org),
        &NewRole {
            key: "mentor".into(),
            name: "Mentor".into(),
            permissions: vec!["account.view".into()],
        },
        request_time(),
    )
    .await
    .expect("create role")
    .id;
    Facility {
        org,
        root,
        tower,
        ground,
        ramp,
        enroute,
        user: user(pool, org, "Pat Example").await,
        mentor,
    }
}

async fn builtin_id(pool: &PgPool, key: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the built-in row")
}

fn new(user_id: Uuid, role_id: Uuid, scope: Scope) -> NewAssignment {
    NewAssignment {
        user_id,
        role_id,
        scope,
        source: Source::Local,
        external_id: None,
    }
}

async fn assign(pool: &PgPool, org: Uuid, assignment: NewAssignment) -> RoleAssignment {
    role_assignments::create(pool, scope(org), &assignment, request_time())
        .await
        .expect("create assignment")
}

/// Every `role_assignments` row as text, in id order.
async fn table(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT to_jsonb(r)::text FROM role_assignments r ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("read role_assignments")
}

/// The variant name of a role assignments error.
fn kind<T: std::fmt::Debug>(result: &Result<T, Error>) -> String {
    match result {
        Ok(value) => format!("Ok({value:?})"),
        Err(error) => format!("{error:?}")
            .split([' ', '(', '{'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_stores_the_assignment_and_reads_back_in_its_scope_only(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;

    let created = role_assignments::create(
        &pool,
        scope(a.org),
        &NewAssignment {
            external_id: Some("ext-1".into()),
            source: Source::Sync,
            ..new(a.user, a.mentor, Scope::Account(a.tower))
        },
        request_time(),
    )
    .await
    .unwrap();

    assert_eq!(created.user_id, a.user);
    assert_eq!(created.role_id, a.mentor);
    assert_eq!(created.scope, Scope::Account(a.tower));
    assert_eq!(created.source, Source::Sync);
    assert_eq!(created.external_id.as_deref(), Some("ext-1"));
    assert_eq!(created.created_at, request_time());
    assert_eq!(created.updated_at, request_time());
    let read = role_assignments::get(&pool, scope(a.org), created.id).await;
    assert_eq!(read.unwrap(), Some(created.clone()));
    let across = role_assignments::get(&pool, scope(b.org), created.id).await;
    assert_eq!(across.unwrap(), None);
    let org_id: Uuid = sqlx::query_scalar("SELECT org_id FROM role_assignments WHERE id = $1")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(org_id, a.org);

    let at_org = assign(&pool, a.org, new(a.user, a.mentor, Scope::Organization)).await;
    let scope_id: Uuid = sqlx::query_scalar("SELECT scope_id FROM role_assignments WHERE id = $1")
        .bind(at_org.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scope_id, a.org, "an organization scope stores the org id");
    assert_eq!(at_org.source, Source::Local);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_what_is_outside_the_scope_and_inserts_nothing(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    assign(&pool, a.org, new(a.user, a.mentor, Scope::Account(a.tower))).await;
    let before = table(&pool).await;
    let with_nul = NewAssignment {
        external_id: Some("e\0".into()),
        ..new(a.user, a.mentor, Scope::Organization)
    };

    let cases = [
        (new(b.user, a.mentor, Scope::Organization), "UserNotFound"),
        (new(new_id(), a.mentor, Scope::Organization), "UserNotFound"),
        (new(a.user, b.mentor, Scope::Organization), "RoleNotFound"),
        (new(a.user, new_id(), Scope::Organization), "RoleNotFound"),
        (
            new(a.user, a.mentor, Scope::Account(b.tower)),
            "AccountNotFound",
        ),
        (
            new(a.user, a.mentor, Scope::Account(new_id())),
            "AccountNotFound",
        ),
        (
            new(a.user, a.mentor, Scope::Course(new_id())),
            "ScopeUnavailable",
        ),
        (new(a.user, a.mentor, Scope::Account(a.tower)), "Exists"),
        (with_nul, "ContainsNul"),
    ];
    for (assignment, expected) in cases {
        let result =
            role_assignments::create(&pool, scope(a.org), &assignment, request_time()).await;
        assert_eq!(kind(&result), expected, "{assignment:?}");
    }

    assert_eq!(table(&pool).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_builtin_role_can_be_assigned_in_every_organization(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let admin = builtin_id(&pool, "org_admin").await;

    assign(&pool, a.org, new(a.user, admin, Scope::Organization)).await;
    assign(&pool, b.org, new(b.user, admin, Scope::Organization)).await;

    assert_eq!(table(&pool).await.len(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_filters_by_user_and_scope_and_pages_by_id(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let other = user(&pool, a.org, "Sam Example").await;
    let admin = builtin_id(&pool, "account_admin").await;
    let ids = [
        assign(&pool, a.org, new(a.user, a.mentor, Scope::Account(a.tower)))
            .await
            .id,
        assign(&pool, a.org, new(other, a.mentor, Scope::Account(a.tower)))
            .await
            .id,
        assign(&pool, a.org, new(a.user, admin, Scope::Account(a.enroute)))
            .await
            .id,
        assign(&pool, a.org, new(a.user, a.mentor, Scope::Organization))
            .await
            .id,
    ];
    assign(&pool, b.org, new(b.user, b.mentor, Scope::Account(b.tower))).await;
    let list = |filter: ListFilter, after: Option<Uuid>, limit: i64| {
        let pool = pool.clone();
        async move {
            role_assignments::list(&pool, scope(a.org), filter, after, limit)
                .await
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        }
    };

    let everything = list(ListFilter::default(), None, 10).await;
    let first_page = list(ListFilter::default(), None, 2).await;
    let second_page = list(ListFilter::default(), Some(ids[1]), 2).await;
    let by_user = ListFilter {
        user_id: Some(a.user),
        scope: None,
    };
    let at_tower = ListFilter {
        user_id: None,
        scope: Some(Scope::Account(a.tower)),
    };
    let at_org = ListFilter {
        user_id: None,
        scope: Some(Scope::Organization),
    };
    let other_org_user = ListFilter {
        user_id: Some(b.user),
        scope: None,
    };

    assert_eq!(everything, ids);
    assert_eq!(first_page, ids[..2]);
    assert_eq!(second_page, ids[2..]);
    assert_eq!(list(by_user, None, 10).await, [ids[0], ids[2], ids[3]]);
    assert_eq!(list(at_tower, None, 10).await, ids[..2]);
    assert_eq!(list(at_org, None, 10).await, [ids[3]]);
    assert_eq!(list(other_org_user, None, 10).await, Vec::<Uuid>::new());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn delete_removes_an_assignment_of_the_scope_only(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let mine = assign(&pool, a.org, new(a.user, a.mentor, Scope::Organization)).await;
    let theirs = assign(&pool, b.org, new(b.user, b.mentor, Scope::Organization)).await;

    let across = role_assignments::delete(&pool, scope(a.org), theirs.id).await;
    role_assignments::delete(&pool, scope(a.org), mine.id)
        .await
        .unwrap();
    let again = role_assignments::delete(&pool, scope(a.org), mine.id).await;

    assert_eq!(kind(&across), "NotFound");
    assert_eq!(kind(&again), "NotFound");
    let left = role_assignments::get(&pool, scope(b.org), theirs.id).await;
    assert_eq!(left.unwrap(), Some(theirs));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn grants_carry_the_scope_path_and_builtin_permissions_from_code(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let admin = builtin_id(&pool, "account_admin").await;
    let at_ground = assign(&pool, a.org, new(a.user, admin, Scope::Account(a.ground))).await;
    let at_org = assign(&pool, a.org, new(a.user, a.mentor, Scope::Organization)).await;
    assign(&pool, b.org, new(b.user, b.mentor, Scope::Organization)).await;
    // T4: the server reads a built-in role's list from code, whatever its row says.
    sqlx::query("UPDATE roles SET permissions = '{organization.manage}' WHERE id = $1")
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();

    let grants = role_assignments::for_user_with_paths(&pool, scope(a.org), a.user)
        .await
        .unwrap();
    let across = role_assignments::for_user_with_paths(&pool, scope(a.org), b.user)
        .await
        .unwrap();

    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0].assignment_id, at_ground.id);
    assert_eq!(grants[0].role_key, "account_admin");
    assert_eq!(
        grants[0].permissions,
        [
            "account.view",
            "account.manage",
            "user.view",
            "role.view",
            "role.assign"
        ]
    );
    assert_eq!(
        grants[0].scope,
        GrantScope::Account {
            id: a.ground,
            path: vec![a.root, a.tower, a.ground],
        }
    );
    assert_eq!(grants[1].assignment_id, at_org.id);
    assert_eq!(grants[1].role_id, a.mentor);
    assert_eq!(grants[1].permissions, ["account.view"]);
    assert_eq!(grants[1].scope, GrantScope::Organization);
    assert!(
        across.is_empty(),
        "another organization's user has no grants here"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn active_org_admins_count_organization_scope_admins_who_are_active(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let admin = builtin_id(&pool, "org_admin").await;
    let suspended = user(&pool, a.org, "Sam Example").await;
    let second = user(&pool, a.org, "Kim Example").await;
    assign(&pool, a.org, new(a.user, admin, Scope::Organization)).await;
    assign(&pool, a.org, new(suspended, admin, Scope::Organization)).await;
    assign(&pool, a.org, new(second, admin, Scope::Account(a.tower))).await;
    assign(&pool, a.org, new(second, a.mentor, Scope::Organization)).await;
    assign(&pool, b.org, new(b.user, admin, Scope::Organization)).await;
    users::set_status(
        &pool,
        scope(a.org),
        suspended,
        Status::Suspended,
        request_time(),
    )
    .await
    .unwrap();

    let count = role_assignments::count_active_org_admins(&pool, scope(a.org)).await;

    assert_eq!(count.unwrap(), 1);
}

/// The SQLSTATE and constraint of a refused statement.
fn refusal(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> (String, Option<String>) {
    let error = result.expect_err("the statement is refused");
    let db = error.as_database_error().expect("a database error");
    (
        db.code().map(|c| c.into_owned()).unwrap_or_default(),
        db.constraint().map(str::to_owned),
    )
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_another_organizations_role_and_organization(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let insert = "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
                  VALUES ($1, $2, $3, $4, $5, $6)";
    let run = |role: Uuid, scope_type: &'static str, scope_id: Uuid| {
        sqlx::query(insert)
            .bind(new_id())
            .bind(a.org)
            .bind(a.user)
            .bind(role)
            .bind(scope_type)
            .bind(scope_id)
            .execute(&pool)
    };

    // T19.
    let other_orgs_role = run(b.mentor, "organization", a.org).await;
    let other_org_scope = run(a.mentor, "organization", b.org).await;
    let other_orgs_user = sqlx::query(insert)
        .bind(new_id())
        .bind(a.org)
        .bind(b.user)
        .bind(a.mentor)
        .bind("organization")
        .bind(a.org)
        .execute(&pool)
        .await;
    run(a.mentor, "account", a.tower).await.unwrap();
    let switched_to_other_orgs_role =
        sqlx::query("UPDATE role_assignments SET role_id = $1 WHERE org_id = $2")
            .bind(b.mentor)
            .bind(a.org)
            .execute(&pool)
            .await;

    assert_eq!(
        refusal(other_orgs_role),
        ("23514".into(), Some("role_assignments_role_in_org".into()))
    );
    assert_eq!(
        refusal(other_org_scope),
        ("23514".into(), Some("role_assignments_check".into()))
    );
    assert_eq!(
        refusal(other_orgs_user),
        (
            "23503".into(),
            Some("role_assignments_org_id_user_id_fkey".into())
        )
    );
    assert_eq!(
        refusal(switched_to_other_orgs_role),
        ("23514".into(), Some("role_assignments_role_in_org".into()))
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_an_account_deletes_the_assignments_scoped_to_it_only(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let at_ramp = assign(&pool, a.org, new(a.user, a.mentor, Scope::Account(a.ramp))).await;
    let at_ground = assign(
        &pool,
        a.org,
        new(a.user, a.mentor, Scope::Account(a.ground)),
    )
    .await;
    let at_org = assign(&pool, a.org, new(a.user, a.mentor, Scope::Organization)).await;
    let in_b = assign(&pool, b.org, new(b.user, b.mentor, Scope::Account(b.ramp))).await;

    // Spec 02 T14: deleting a leaf removes it and the assignments scoped to it.
    accounts::delete(&pool, scope(a.org), a.ramp).await.unwrap();

    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM role_assignments ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, [at_ground.id, at_org.id, in_b.id]);
    assert!(
        !left.contains(&at_ramp.id),
        "the assignment at Ramp went with it"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_account_delete_keeps_its_assignments(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    assign(
        &pool,
        a.org,
        new(a.user, a.mentor, Scope::Account(a.ground)),
    )
    .await;
    let before = table(&pool).await;

    let refused = accounts::delete(&pool, scope(a.org), a.ground).await;

    assert!(matches!(refused, Err(accounts::Error::HasChildren)));
    assert_eq!(table(&pool).await, before);
}

async fn begin(pool: &PgPool) -> sqlx::Transaction<'static, Postgres> {
    pool.begin().await.expect("begin")
}

/// Waits until a session of this test's database waits on a row or transaction lock, so the
/// test knows the second call is blocked behind the first.
async fn until_a_call_waits_on_a_lock(pool: &PgPool) {
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no session waited on a lock within 5 s");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_create_waiting_on_a_delete_of_its_account_is_refused(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let mut deleting = begin(&pool).await;
    accounts::delete(&mut *deleting, scope(a.org), a.ramp)
        .await
        .unwrap();
    let creating = tokio::spawn({
        let pool = pool.clone();
        let assignment = new(a.user, a.mentor, Scope::Account(a.ramp));
        async move { role_assignments::create(&pool, scope(a.org), &assignment, request_time()).await }
    });
    until_a_call_waits_on_a_lock(&pool).await;
    deleting.commit().await.unwrap();

    let created = creating.await.expect("the create ran");

    assert_eq!(kind(&created), "AccountNotFound");
    assert!(table(&pool).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_delete_waiting_on_a_create_at_its_account_deletes_the_new_assignment(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let mut creating = begin(&pool).await;
    let assignment = new(a.user, a.mentor, Scope::Account(a.ramp));
    role_assignments::create(&mut *creating, scope(a.org), &assignment, request_time())
        .await
        .unwrap();
    let deleting = tokio::spawn({
        let pool = pool.clone();
        async move { accounts::delete(&pool, scope(a.org), a.ramp).await }
    });
    until_a_call_waits_on_a_lock(&pool).await;
    creating.commit().await.unwrap();

    deleting
        .await
        .expect("the delete ran")
        .expect("the delete succeeds");

    assert!(
        table(&pool).await.is_empty(),
        "no assignment points at the deleted account"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn grants_fail_closed_on_an_account_scope_in_another_organization(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    let id = new_id();
    // The repository's `create` refuses this; SQL stands in for a row that got past it.
    sqlx::query(
        "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
         VALUES ($1, $2, $3, $4, 'account', $5)",
    )
    .bind(id)
    .bind(a.org)
    .bind(a.user)
    .bind(a.mentor)
    .bind(b.tower)
    .execute(&pool)
    .await
    .unwrap();

    let grants = role_assignments::for_user_with_paths(&pool, scope(a.org), a.user).await;

    assert!(
        matches!(grants, Err(Error::MissingAccount { assignment_id }) if assignment_id == id),
        "{grants:?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn grants_ignore_another_organizations_role_even_without_the_trigger(pool: PgPool) {
    let a = facility(&pool, "org-a").await;
    let b = facility(&pool, "org-b").await;
    // The trigger refuses this row; disabling it stands in for one stored some other way.
    sqlx::query("ALTER TABLE role_assignments DISABLE TRIGGER role_assignments_role_in_org")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
         VALUES ($1, $2, $3, $4, 'organization', $2)",
    )
    .bind(new_id())
    .bind(a.org)
    .bind(a.user)
    .bind(b.mentor)
    .execute(&pool)
    .await
    .unwrap();

    let grants = role_assignments::for_user_with_paths(&pool, scope(a.org), a.user)
        .await
        .unwrap();

    assert!(grants.is_empty(), "{grants:?}");
}
