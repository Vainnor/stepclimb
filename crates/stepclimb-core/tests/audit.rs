// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audit entries against a fresh database per test (docs/specs/m0/09-permissions-and-roles.md
//! Q5 and "Audit entries"). Each audited repository change writes exactly one entry with its
//! actor, action, target and data; a refused or rolled-back change writes none; an organization's
//! scope reads its own entries only. Needs `DATABASE_URL` (`just up`).

use std::{
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, OrgSettingsPatch, Source, accounts,
    audit::{self, Action, Actor, Entry},
    id::Uuid,
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole, RolePatch},
    users::{self, NewUser, Status},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded `at` shows.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Org {
    id: Uuid,
    root: Uuid,
    /// An active user holding Organization admin at organization scope, the actor of the
    /// changes.
    admin: Uuid,
    admin_grant: Uuid,
}

impl Org {
    fn actor(&self) -> Actor {
        Actor::User {
            id: self.admin,
            ip: "2001:db8::7".parse().expect("an address"),
        }
    }
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
    users::create(pool, scope(org), &user, now())
        .await
        .expect("create user")
        .id
}

async fn org(pool: &PgPool, slug: &str) -> Org {
    roles::sync_builtin(pool, now()).await.expect("sync roles");
    let id = organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the root");
    let admin = user(pool, id, "Admin").await;
    let org_admin = roles::builtin_id(pool, "org_admin")
        .await
        .expect("read the role")
        .expect("synced");
    let admin_grant = grant(pool, id, admin, org_admin, Actor::Instance).await;
    Org {
        id,
        root,
        admin,
        admin_grant,
    }
}

async fn grant(pool: &PgPool, org: Uuid, user: Uuid, role: Uuid, actor: Actor) -> Uuid {
    let assignment = NewAssignment {
        user_id: user,
        role_id: role,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(pool, scope(org), &assignment, now(), actor)
        .await
        .expect("create assignment")
        .id
}

async fn entries(pool: &PgPool, org: Uuid) -> Vec<Entry> {
    audit::list(pool, scope(org), 1000).await.expect("list")
}

/// The one entry written since `before` entries existed, checked against what the change was.
async fn only_new_entry(
    pool: &PgPool,
    org: &Org,
    before: usize,
    action: Action,
    target: Uuid,
    data: Value,
) {
    let all = entries(pool, org.id).await;
    let new = &all[before..];
    assert_eq!(new.len(), 1, "{action:?}: {new:#?}");
    let entry = &new[0];
    assert_eq!(entry.actor, org.actor(), "{action:?}");
    assert_eq!(entry.action, action.as_str());
    assert_eq!(entry.target_type, action.target_type());
    assert_eq!(entry.target_id, target, "{action:?}");
    assert_eq!(entry.data, data, "{action:?}");
    assert_eq!(entry.at, now());
}

async fn count(pool: &PgPool, org: Uuid) -> usize {
    entries(pool, org).await.len()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_role_and_assignment_change_writes_one_entry(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);

    let n = count(&pool, a.id).await;
    let new = NewRole {
        key: "mentor".into(),
        name: "Mentor".into(),
        permissions: vec!["account.view".into()],
    };
    let role = roles::create_custom(&pool, s, &new, now(), a.actor())
        .await
        .unwrap();
    let after = json!({ "key": "mentor", "name": "Mentor", "permissions": ["account.view"] });
    only_new_entry(
        &pool,
        &a,
        n,
        Action::RoleCreate,
        role.id,
        json!({ "after": after }),
    )
    .await;

    let n = count(&pool, a.id).await;
    let patch = RolePatch {
        name: None,
        permissions: Some(vec!["account.view".into(), "user.view".into()]),
    };
    roles::update_custom(&pool, s, role.id, &patch, now(), a.actor())
        .await
        .unwrap();
    let data = json!({
        "before": { "permissions": ["account.view"] },
        "after": { "permissions": ["account.view", "user.view"] },
    });
    only_new_entry(&pool, &a, n, Action::RoleUpdate, role.id, data).await;

    let n = count(&pool, a.id).await;
    let member = user(&pool, a.id, "Member").await;
    let assignment = grant(&pool, a.id, member, role.id, a.actor()).await;
    let held = json!({
        "user_id": member, "role_id": role.id, "scope_type": "organization",
        "scope_id": a.id, "source": "local",
    });
    let grant_data = json!({ "after": held });
    only_new_entry(
        &pool,
        &a,
        n,
        Action::RoleAssignmentGrant,
        assignment,
        grant_data,
    )
    .await;

    let n = count(&pool, a.id).await;
    role_assignments::delete(&pool, s, assignment, now(), a.actor())
        .await
        .unwrap();
    let removed = json!({ "before": held });
    only_new_entry(
        &pool,
        &a,
        n,
        Action::RoleAssignmentRemove,
        assignment,
        removed,
    )
    .await;

    let n = count(&pool, a.id).await;
    roles::delete_custom(&pool, s, role.id, now(), a.actor())
        .await
        .unwrap();
    let before = json!({
        "key": "mentor", "name": "Mentor", "permissions": ["account.view", "user.view"],
    });
    only_new_entry(
        &pool,
        &a,
        n,
        Action::RoleDelete,
        role.id,
        json!({ "before": before }),
    )
    .await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_account_change_writes_one_entry(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);
    let tower = accounts::create(&pool, s, a.root, "Tower", now(), Actor::Instance)
        .await
        .unwrap()
        .id;

    let n = count(&pool, a.id).await;
    let ground = accounts::create(&pool, s, a.root, " Ground ", now(), a.actor())
        .await
        .unwrap()
        .id;
    let data = json!({ "after": { "parent_id": a.root, "name": "Ground" } });
    only_new_entry(&pool, &a, n, Action::AccountCreate, ground, data).await;

    let n = count(&pool, a.id).await;
    accounts::rename(&pool, s, ground, "Ground Control", now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "name": "Ground" }, "after": { "name": "Ground Control" } });
    only_new_entry(&pool, &a, n, Action::AccountRename, ground, data).await;

    let n = count(&pool, a.id).await;
    accounts::move_to(&pool, s, ground, tower, now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "parent_id": a.root }, "after": { "parent_id": tower } });
    only_new_entry(&pool, &a, n, Action::AccountMove, ground, data).await;

    let n = count(&pool, a.id).await;
    accounts::delete(&pool, s, ground, now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "parent_id": tower, "name": "Ground Control" } });
    only_new_entry(&pool, &a, n, Action::AccountDelete, ground, data).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_organization_user_status_and_manager_change_writes_one_entry(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);

    let n = count(&pool, a.id).await;
    organizations::rename(&pool, s, "Renamed", now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "name": "org-a" }, "after": { "name": "Renamed" } });
    only_new_entry(&pool, &a, n, Action::OrganizationRename, a.id, data).await;

    let member = user(&pool, a.id, "Member").await;
    let n = count(&pool, a.id).await;
    users::set_status(&pool, s, member, Status::Suspended, now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "status": "active" }, "after": { "status": "suspended" } });
    only_new_entry(&pool, &a, n, Action::UserSetStatus, member, data).await;

    // docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B15.
    let manager = user(&pool, a.id, "Manager").await;
    let n = count(&pool, a.id).await;
    users::set_manager(&pool, s, member, Some(manager), now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "manager_id": null }, "after": { "manager_id": manager } });
    only_new_entry(&pool, &a, n, Action::UserSetManager, member, data).await;
    let n = count(&pool, a.id).await;
    users::set_manager(&pool, s, member, None, now(), a.actor())
        .await
        .unwrap();
    let data = json!({ "before": { "manager_id": manager }, "after": { "manager_id": null } });
    only_new_entry(&pool, &a, n, Action::UserSetManager, member, data).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn creating_an_organization_writes_its_entry_by_the_instance(pool: PgPool) {
    let created = organizations::create(&pool, "org-a", "Org A", now())
        .await
        .unwrap();

    let all = entries(&pool, created.id).await;
    assert_eq!(all.len(), 1, "{all:#?}");
    assert_eq!(all[0].actor, Actor::Instance);
    assert_eq!(all[0].action, "organization.create");
    assert_eq!(all[0].target_type, "organization");
    assert_eq!(all[0].target_id, created.id);
    assert_eq!(
        all[0].data,
        json!({ "after": { "slug": "org-a", "name": "Org A" } })
    );
}

/// Every refusal below happens after the repository opened its transaction or as the
/// statement that would have changed the row, so a write of the entry before the check would
/// show here.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_change_writes_no_entry(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);
    let actor = a.actor();
    let tower = accounts::create(&pool, s, a.root, "Tower", now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    let ground = accounts::create(&pool, s, tower, "Ground", now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    let new = |key: &str| NewRole {
        key: key.into(),
        name: "Mentor".into(),
        permissions: vec![],
    };
    let used = roles::create_custom(&pool, s, &new("used"), now(), Actor::Instance)
        .await
        .unwrap();
    grant(&pool, a.id, a.admin, used.id, Actor::Instance).await;
    let org_admin = roles::builtin_id(&pool, "org_admin")
        .await
        .unwrap()
        .unwrap();
    let rename = RolePatch {
        name: Some("Other".into()),
        permissions: None,
    };
    let before = count(&pool, a.id).await;

    let refusals = [
        format!(
            "{:?}",
            roles::create_custom(&pool, s, &new("used"), now(), actor).await
        ),
        format!(
            "{:?}",
            roles::create_custom(&pool, s, &new("org_admin"), now(), actor).await
        ),
        format!(
            "{:?}",
            roles::update_custom(&pool, s, org_admin, &rename, now(), actor).await
        ),
        format!(
            "{:?}",
            roles::delete_custom(&pool, s, used.id, now(), actor).await
        ),
        format!(
            "{:?}",
            roles::delete_custom(&pool, s, org_admin, now(), actor).await
        ),
        format!(
            "{:?}",
            role_assignments::delete(&pool, s, a.admin_grant, now(), actor).await
        ),
        format!(
            "{:?}",
            role_assignments::create(
                &pool,
                s,
                &NewAssignment {
                    user_id: a.admin,
                    role_id: used.id,
                    scope: Scope::Organization,
                    source: Source::Local,
                    external_id: None,
                },
                now(),
                actor,
            )
            .await
        ),
        format!(
            "{:?}",
            accounts::delete(&pool, s, tower, now(), actor).await
        ),
        format!(
            "{:?}",
            accounts::delete(&pool, s, a.root, now(), actor).await
        ),
        format!(
            "{:?}",
            accounts::move_to(&pool, s, tower, ground, now(), actor).await
        ),
        format!(
            "{:?}",
            accounts::rename(&pool, s, tower, "  ", now(), actor).await
        ),
        format!(
            "{:?}",
            organizations::rename(&pool, s, "", now(), actor).await
        ),
        format!(
            "{:?}",
            users::set_status(&pool, s, a.admin, Status::Suspended, now(), actor).await
        ),
    ];

    for refusal in &refusals {
        assert!(refusal.starts_with("Err("), "{refusal}");
    }
    assert_eq!(count(&pool, a.id).await, before, "{refusals:#?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_to_the_current_parent_changes_nothing_and_writes_no_entry(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);
    let tower = accounts::create(&pool, s, a.root, "Tower", now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    let before = count(&pool, a.id).await;

    accounts::move_to(&pool, s, tower, a.root, now(), a.actor())
        .await
        .unwrap();

    assert_eq!(count(&pool, a.id).await, before);
}

/// The entry is written on the caller's transaction: rolling the caller back removes the change
/// and its entry together.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_entry_rolls_back_with_its_change(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let before = count(&pool, a.id).await;
    let mut tx = pool.begin().await.unwrap();
    let new = NewRole {
        key: "mentor".into(),
        name: "Mentor".into(),
        permissions: vec![],
    };
    let role = roles::create_custom(&mut *tx, scope(a.id), &new, now(), a.actor())
        .await
        .unwrap();
    let inside = audit::list(&mut *tx, scope(a.id), 1000).await.unwrap();
    assert_eq!(inside.len(), before + 1);

    tx.rollback().await.unwrap();

    assert_eq!(count(&pool, a.id).await, before);
    let kept = roles::get(&pool, scope(a.id), role.id).await.unwrap();
    assert_eq!(kept, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organizations_scope_reads_only_its_own_entries(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    organizations::rename(&pool, scope(b.id), "B renamed", now(), b.actor())
        .await
        .unwrap();

    let of_a = entries(&pool, a.id).await;
    let of_b = entries(&pool, b.id).await;

    assert!(!of_a.is_empty() && !of_b.is_empty());
    let b_ids: Vec<Uuid> = of_b.iter().map(|e| e.id).collect();
    assert!(of_a.iter().all(|e| !b_ids.contains(&e.id)));
    let a_targets = [a.id, a.admin_grant];
    assert!(
        of_a.iter().all(|e| a_targets.contains(&e.target_id)),
        "{of_a:#?}"
    );
    assert!(of_b.iter().any(|e| e.action == "organization.rename"));
    assert!(of_a.iter().all(|e| e.action != "organization.rename"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_is_bounded_and_in_write_order(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let all = entries(&pool, a.id).await;
    assert_eq!(
        all.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        ["organization.create", "role_assignment.grant"]
    );
    let first = audit::list(&pool, scope(a.id), 1).await.unwrap();
    assert_eq!(first, all[..1]);
}

/// The table's checks: a user or API-key actor has an id and an address, the instance has
/// neither.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_refuses_an_actor_without_its_id_or_address(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let insert = |actor_type: &'static str, actor_id: Option<Uuid>, ip: Option<&'static str>| {
        sqlx::query(
            "INSERT INTO audit_events (id, org_id, actor_type, actor_id, action, target_type,
                                       target_id, at, ip)
             VALUES (gen_random_uuid(), $1, $2, $3, 'role.create', 'role', $1, now(), $4::inet)",
        )
        .bind(a.id)
        .bind(actor_type)
        .bind(actor_id)
        .bind(ip)
        .execute(&pool)
    };
    let ip = Some("192.0.2.1");
    assert!(insert("user", Some(a.admin), ip).await.is_ok());
    assert!(insert("instance", None, None).await.is_ok());
    assert!(insert("api_key", Some(a.id), ip).await.is_ok());
    for refused in [
        insert("api_key", None, ip).await,
        insert("api_key", Some(a.id), None).await,
        insert("user", None, ip).await,
        insert("user", Some(a.admin), None).await,
        insert("instance", Some(a.admin), None).await,
        insert("instance", None, ip).await,
        insert("system", None, None).await,
    ] {
        let code = refused.expect_err("refused");
        let code = code.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("23514"), "check_violation");
    }
    let ipv4: IpAddr = "192.0.2.1".parse().unwrap();
    let stored = entries(&pool, a.id).await;
    assert!(stored.iter().any(|e| e.actor
        == Actor::User {
            id: a.admin,
            ip: ipv4
        }));
    assert!(
        stored
            .iter()
            .any(|e| e.actor == Actor::ApiKey { id: a.id, ip: ipv4 })
    );
}

/// 2025-11-13T02:13:21.123456Z, one second after [`now`]: a no-op that wrote would move
/// `updated_at` here.
fn later() -> SystemTime {
    now() + Duration::from_secs(1)
}

async fn updated_at(pool: &PgPool, table: &str, id: Uuid) -> SystemTime {
    let sql = format!(
        "SELECT (extract(epoch FROM updated_at) * 1000000)::bigint FROM {table} WHERE id = $1"
    );
    let micros: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read updated_at");
    UNIX_EPOCH + Duration::from_micros(u64::try_from(micros).expect("after the epoch"))
}

/// A patch that names nothing, or only the values the row already has, writes neither the row
/// (`updated_at` stays) nor an entry, and answers the row as it is.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_to_the_current_values_writes_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let s = scope(a.id);
    let new = NewRole {
        key: "mentor".into(),
        name: "Mentor".into(),
        permissions: vec!["account.view".into(), "user.view".into()],
    };
    let role = roles::create_custom(&pool, s, &new, now(), Actor::Instance)
        .await
        .unwrap();
    let tower = accounts::create(&pool, s, a.root, "Tower", now(), Actor::Instance)
        .await
        .unwrap();
    let member = user(&pool, a.id, "Member").await;
    let before = count(&pool, a.id).await;
    let (actor, at) = (a.actor(), later());

    let patches = [
        RolePatch::default(),
        RolePatch {
            name: Some(" Mentor ".into()),
            permissions: Some(vec!["user.view".into(), "account.view".into()]),
        },
    ];
    for patch in &patches {
        let same = roles::update_custom(&pool, s, role.id, patch, at, actor).await;
        assert_eq!(same.unwrap(), role, "{patch:?}");
    }
    let same = accounts::rename(&pool, s, tower.id, "  Tower ", at, actor).await;
    assert_eq!(same.unwrap(), tower);
    let same = organizations::rename(&pool, s, "org-a", at, actor).await;
    assert_eq!(same.unwrap().name, "org-a");
    let patch = OrgSettingsPatch::default();
    let same = organizations::update_settings(&pool, s, &patch, at, actor).await;
    assert_eq!(
        same.unwrap(),
        organizations::settings(&pool, s).await.unwrap()
    );
    let same = users::set_status(&pool, s, member, Status::Active, at, actor).await;
    assert_eq!(same.unwrap().status, Status::Active);

    assert_eq!(count(&pool, a.id).await, before);
    for (table, id) in [
        ("roles", role.id),
        ("accounts", tower.id),
        ("organizations", a.id),
        ("users", member),
    ] {
        assert_eq!(updated_at(&pool, table, id).await, now(), "{table}");
    }
}

/// Entries are append-only: UPDATE and TRUNCATE are refused, DELETE is not.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_entry_cannot_be_updated_or_truncated(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let before = entries(&pool, a.id).await;

    let update = sqlx::query("UPDATE audit_events SET action = 'role.delete' WHERE org_id = $1")
        .bind(a.id)
        .execute(&pool)
        .await;
    let truncate = sqlx::query("TRUNCATE audit_events").execute(&pool).await;

    for refused in [update, truncate] {
        let error = refused.expect_err("refused");
        let code = error.as_database_error().and_then(|e| e.code());
        assert_eq!(
            code.as_deref(),
            Some("23001"),
            "restrict_violation: {error}"
        );
    }
    assert_eq!(entries(&pool, a.id).await, before);
    sqlx::query("DELETE FROM audit_events WHERE org_id = $1")
        .bind(a.id)
        .execute(&pool)
        .await
        .expect("DELETE stays allowed");
    assert!(entries(&pool, a.id).await.is_empty());
}
