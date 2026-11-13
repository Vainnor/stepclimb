// SPDX-License-Identifier: AGPL-3.0-or-later
//! The roles repository and the `roles` table against a fresh database per test
//! (docs/specs/m0/09-permissions-and-roles.md B4 to B10; T4 at the repository level). Needs
//! `DATABASE_URL` (`just up`).
//!
//! Every test that writes a custom role makes two organizations and checks that org A's scope
//! reads and changes org A's roles only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, BUILTIN, Error, NewRole, Role, RolePatch},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

/// One second after [`request_time`].
fn later_time() -> SystemTime {
    request_time() + Duration::from_secs(1)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, request_time())
        .await
        .expect("create organization")
        .id
}

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| (*k).to_owned()).collect()
}

fn new_role(key: &str, permissions: &[&str]) -> NewRole {
    NewRole {
        key: key.into(),
        name: format!("Role {key}"),
        permissions: keys(permissions),
    }
}

async fn create(pool: &PgPool, org: Uuid, key: &str, permissions: &[&str]) -> Role {
    roles::create_custom(
        pool,
        scope(org),
        &new_role(key, permissions),
        request_time(),
    )
    .await
    .expect("create role")
}

async fn builtin_id(pool: &PgPool, key: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the built-in row")
}

/// Every `roles` row as text, in id order: a snapshot that shows whether a call changed anything.
async fn table(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT to_jsonb(r)::text FROM roles r ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("read roles")
}

/// The variant name of a roles error.
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

fn code_list(key: &str) -> Vec<String> {
    let role = roles::builtin(key).expect("a built-in role");
    role.permissions().into_iter().map(str::to_owned).collect()
}

/// `key, name, builtin, permissions, org_id` of a `roles` row.
type RoleRow = (String, String, bool, Vec<String>, Option<Uuid>);

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn sync_writes_the_eight_builtin_rows_from_code(pool: PgPool) {
    roles::sync_builtin(&pool, request_time()).await.unwrap();

    let rows: Vec<RoleRow> =
        sqlx::query_as("SELECT key, name, builtin, permissions, org_id FROM roles ORDER BY key")
            .fetch_all(&pool)
            .await
            .unwrap();
    let mut expected: Vec<_> = BUILTIN
        .iter()
        .map(|r| {
            (
                r.key.to_owned(),
                r.name.to_owned(),
                true,
                code_list(r.key),
                None,
            )
        })
        .collect();
    expected.sort();
    // T4: eight rows, each with this binary's name and list.
    assert_eq!(rows.len(), 8);
    assert_eq!(rows, expected);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn reads_take_a_builtin_roles_permissions_from_code_and_a_resync_rewrites_the_row(
    pool: PgPool,
) {
    let a = org(&pool, "org-a").await;
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    let admin = builtin_id(&pool, "account_admin").await;
    sqlx::query(
        "UPDATE roles SET permissions = '{organization.manage}', name = 'Edited' WHERE id = $1",
    )
    .bind(admin)
    .execute(&pool)
    .await
    .unwrap();

    // T4: the edited row does not change what the server reads.
    let read = roles::get(&pool, scope(a), admin).await.unwrap().unwrap();
    assert_eq!(read.permissions, code_list("account_admin"));
    assert!(read.builtin);

    roles::sync_builtin(&pool, later_time()).await.unwrap();

    // T4: a restart's sync rewrites the row and stamps it.
    let (name, stored, updated): (String, Vec<String>, i64) = sqlx::query_as(
        "SELECT name, permissions, (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM roles WHERE id = $1",
    )
    .bind(admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(name, "Account admin");
    assert_eq!(stored, code_list("account_admin"));
    assert_eq!(updated, 1_763_000_001_123_456);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_resync_with_nothing_to_change_keeps_every_row(pool: PgPool) {
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    let before = table(&pool).await;

    roles::sync_builtin(&pool, later_time()).await.unwrap();

    assert_eq!(table(&pool).await, before, "same ids, same updated_at");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_builtin_row_this_binary_does_not_define_grants_nothing_and_is_kept(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let newer = new_id();
    sqlx::query(
        "INSERT INTO roles (id, key, name, builtin, permissions)
         VALUES ($1, 'from_a_newer_release', 'Newer', true, '{organization.manage}')",
    )
    .bind(newer)
    .execute(&pool)
    .await
    .unwrap();

    roles::sync_builtin(&pool, request_time()).await.unwrap();

    let read = roles::get(&pool, scope(a), newer).await.unwrap().unwrap();
    assert_eq!(read.permissions, Vec::<String>::new());
    let custom = roles::create_custom(
        &pool,
        scope(a),
        &new_role("from_a_newer_release", &[]),
        request_time(),
    )
    .await;
    assert_eq!(kind(&custom), "KeyTaken", "a built-in row's key is taken");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_custom_stores_the_role_in_the_scopes_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;

    let role = roles::create_custom(
        &pool,
        scope(a),
        &NewRole {
            key: "tower_mentor".into(),
            name: "  Tower mentor ".into(),
            permissions: keys(&["role.view", "account.view"]),
        },
        request_time(),
    )
    .await
    .unwrap();

    assert_eq!(role.key, "tower_mentor");
    assert_eq!(role.name, "Tower mentor", "the name is trimmed");
    assert!(!role.builtin);
    assert_eq!(role.permissions, ["role.view", "account.view"]);
    assert_eq!(role.created_at, request_time());
    assert_eq!(role.updated_at, request_time());
    assert_eq!(
        roles::get(&pool, scope(a), role.id).await.unwrap(),
        Some(role.clone())
    );
    assert_eq!(roles::get(&pool, scope(b), role.id).await.unwrap(), None);
    let org_id: Uuid = sqlx::query_scalar("SELECT org_id FROM roles WHERE id = $1")
        .bind(role.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(org_id, a);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_custom_refuses_bad_and_taken_keys_unknown_permissions_and_blank_names(
    pool: PgPool,
) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    create(&pool, a, "tower_mentor", &[]).await;
    create(&pool, b, "enroute_mentor", &[]).await;
    let before = table(&pool).await;

    let blank = NewRole {
        name: " ".into(),
        ..new_role("blank", &[])
    };
    let cases = [
        (new_role("org_admin", &[]), "KeyTaken"),
        (new_role("tower_mentor", &[]), "KeyTaken"),
        (new_role("Tower", &[]), "KeyInvalid"),
        (new_role("x", &["nonexistent.perm"]), "UnknownPermission"),
        (blank, "NameEmpty"),
    ];
    for (role, expected) in cases {
        let result = roles::create_custom(&pool, scope(a), &role, request_time()).await;
        assert_eq!(kind(&result), expected, "{role:?}");
    }
    let unknown = roles::create_custom(
        &pool,
        scope(a),
        &new_role("x", &["account.view", "nonexistent.perm"]),
        request_time(),
    )
    .await;
    assert!(matches!(
        unknown,
        Err(Error::UnknownPermission { index: 1 })
    ));
    assert_eq!(table(&pool).await, before);

    // Another organization's key is free in this one.
    create(&pool, a, "enroute_mentor", &[]).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_puts_builtin_roles_first_then_the_scopes_custom_roles_by_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    for key in ["zulu", "alpha", "mike"] {
        create(&pool, a, key, &[]).await;
    }
    create(&pool, b, "bravo", &[]).await;

    let mut pages = Vec::new();
    let mut after = None;
    loop {
        let page = roles::list(&pool, scope(a), after.as_ref(), 3)
            .await
            .unwrap();
        let Some(last) = page.last() else { break };
        after = Some((!last.builtin, last.key.clone()));
        pages.extend(page.into_iter().map(|r| (r.builtin, r.key)));
    }

    let mut builtin: Vec<_> = BUILTIN.iter().map(|r| (true, r.key.to_owned())).collect();
    builtin.sort();
    let expected: Vec<_> = builtin
        .into_iter()
        .chain(["alpha", "mike", "zulu"].map(|k| (false, k.to_owned())))
        .collect();
    assert_eq!(pages, expected);
    let nul = roles::list(&pool, scope(a), Some(&(true, "a\0".into())), 3).await;
    assert_eq!(kind(&nul), "ContainsNul");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_custom_changes_the_named_fields_of_the_scopes_role_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let role = create(&pool, a, "tower_mentor", &["account.view"]).await;
    let other = create(&pool, b, "tower_mentor", &["account.view"]).await;

    let renamed = roles::update_custom(
        &pool,
        scope(a),
        role.id,
        &RolePatch {
            name: Some(" Mentor ".into()),
            permissions: None,
        },
        later_time(),
    )
    .await
    .unwrap();
    let narrowed = roles::update_custom(
        &pool,
        scope(a),
        role.id,
        &RolePatch {
            name: None,
            permissions: Some(vec![]),
        },
        later_time(),
    )
    .await
    .unwrap();

    assert_eq!(renamed.name, "Mentor");
    assert_eq!(renamed.permissions, ["account.view"]);
    assert_eq!(renamed.updated_at, later_time());
    assert_eq!(narrowed.name, "Mentor");
    assert!(narrowed.permissions.is_empty());
    assert_eq!(narrowed.created_at, request_time());
    let untouched = roles::get(&pool, scope(b), other.id).await.unwrap();
    assert_eq!(untouched, Some(other));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn builtin_roles_and_other_organizations_roles_cannot_be_changed_or_deleted(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    let admin = builtin_id(&pool, "org_admin").await;
    let other = create(&pool, b, "tower_mentor", &[]).await;
    let before = table(&pool).await;
    let patch = RolePatch {
        name: Some("Renamed".into()),
        permissions: None,
    };
    let invalid = RolePatch {
        name: None,
        permissions: Some(keys(&["nonexistent.perm"])),
    };

    let results = [
        kind(&roles::update_custom(&pool, scope(a), admin, &patch, later_time()).await),
        kind(&roles::delete_custom(&pool, scope(a), admin).await),
        kind(&roles::update_custom(&pool, scope(a), other.id, &patch, later_time()).await),
        kind(&roles::delete_custom(&pool, scope(a), other.id).await),
        kind(&roles::delete_custom(&pool, scope(a), new_id()).await),
        kind(&roles::update_custom(&pool, scope(b), other.id, &invalid, later_time()).await),
    ];

    assert_eq!(
        results,
        [
            "BuiltinImmutable",
            "BuiltinImmutable",
            "NotFound",
            "NotFound",
            "NotFound",
            "UnknownPermission"
        ]
    );
    assert_eq!(table(&pool).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_custom_role_with_an_assignment_is_in_use_until_the_assignment_goes(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let role = create(&pool, a, "tower_mentor", &["account.view"]).await;
    let user = users::create(
        &pool,
        scope(a),
        &NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        },
        request_time(),
    )
    .await
    .unwrap();
    let assignment = role_assignments::create(
        &pool,
        scope(a),
        &NewAssignment {
            user_id: user.id,
            role_id: role.id,
            scope: Scope::Organization,
            source: Source::Local,
            external_id: None,
        },
        request_time(),
    )
    .await
    .unwrap();

    let in_use = roles::delete_custom(&pool, scope(a), role.id).await;
    role_assignments::delete(&pool, scope(a), assignment.id)
        .await
        .unwrap();
    let deleted = roles::delete_custom(&pool, scope(a), role.id).await;

    assert_eq!(kind(&in_use), "InUse");
    assert_eq!(kind(&deleted), "Ok(())");
    assert_eq!(roles::get(&pool, scope(a), role.id).await.unwrap(), None);
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
async fn the_table_ties_builtin_to_a_null_organization_and_keeps_keys_unique(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    roles::sync_builtin(&pool, request_time()).await.unwrap();
    let insert = "INSERT INTO roles (id, org_id, key, name, builtin) VALUES ($1, $2, $3, 'R', $4)";
    let run = |org: Option<Uuid>, key: &'static str, builtin: bool| {
        sqlx::query(insert)
            .bind(new_id())
            .bind(org)
            .bind(key)
            .bind(builtin)
            .execute(&pool)
    };

    let builtin_with_org = run(Some(a), "x", true).await;
    let custom_without_org = run(None, "y", false).await;
    let second_builtin_key = run(None, "org_admin", true).await;
    run(Some(a), "mentor", false).await.unwrap();
    let second_org_key = run(Some(a), "mentor", false).await;
    let blank_name = sqlx::query("UPDATE roles SET name = ' ' WHERE key = 'mentor'")
        .execute(&pool)
        .await;

    assert_eq!(
        refusal(builtin_with_org),
        ("23514".into(), Some("roles_check".into()))
    );
    assert_eq!(
        refusal(custom_without_org),
        ("23514".into(), Some("roles_check".into()))
    );
    assert_eq!(
        refusal(second_builtin_key),
        ("23505".into(), Some("roles_builtin_key".into()))
    );
    assert_eq!(
        refusal(second_org_key),
        ("23505".into(), Some("roles_org_key".into()))
    );
    assert_eq!(refusal(blank_name).0, "23514");
}
