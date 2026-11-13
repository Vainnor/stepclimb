// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission suite (docs/specs/m0/09-permissions-and-roles.md B19 to B21, T15, T16).
//!
//! Every operation of the OpenAPI document is either in [`PUBLIC`] or has an [`Entry`] in
//! [`TABLE`]; the walker test fails, naming each, on an operation in neither. From each entry the
//! runner generates the cases of B20 against the whole router:
//!
//! - **401**: no credential;
//! - **403**: a custom role with every catalog permission but `P`, at the target's scope;
//! - **404**: a custom role with every catalog permission at a sibling account of the target
//!   (not applicable to `OrgTarget`, `AnyScope` lists and organization-only permissions);
//! - **2xx**: a custom role with exactly `P` (and the entry's extra permissions) at the target's
//!   scope;
//! - **cross-organization**: an Organization admin of org A calls with the target built in org
//!   B: 404 for a route with a target, 2xx acting on org A otherwise; org B's rows are unchanged
//!   and none of their ids is in the response.
//!
//! An `AuthOnly` entry (a route that needs a principal and no permission) gets the 401, 2xx and
//! cross-organization cases.
//!
//! Each case builds its own organizations in the test's database, so cases cannot disturb each
//! other. The request names its organization with the `test-support` header of `request_org`
//! (the production resolver refuses every request while two organizations exist); the e2e test
//! `web/e2e/smoke.spec.ts` shows the release image ignores that header. Needs `DATABASE_URL`
//! (`just up`).

use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::{get, patch, post},
};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    clock::ManualClock,
    grants::ScopeId,
    id::{Uuid, new_id},
    organizations,
    perm::{
        self, AccountManage, AccountView, OrgManage, Permission, RoleAssign, RoleManage, RoleView,
        ScopeType, UserManage, UserView,
    },
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    error::{ApiError, Problem},
    extract::{ApiJson, ApiPath},
    openapi,
    request_org::TEST_ORG_HEADER,
    require::{AccountParentTarget, AccountTarget, AnyScope, Require},
    router,
    session::{self, Authenticated},
    web_origin::WebOrigin,
};
use tower::ServiceExt;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

// The table.

/// The operations that need no credential: the probes and the unauthenticated routes specs 04
/// to 08 declare (B21).
const PUBLIC: &[&str] = &["healthz", "readyz", "signOut"];

/// One entry per operation that needs a principal (B20). The M0 specs' routes add theirs here
/// as they land.
const TABLE: &[Entry] = &[
    Entry {
        operation_id: "getMe",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "getOrganization",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "updateOrganization",
        access: permission(OrgManage::KEY, Target::Org, rename_organization),
    },
    Entry {
        operation_id: "getOrganizationSettings",
        access: permission(OrgManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "updateOrganizationSettings",
        access: permission(OrgManage::KEY, Target::Org, empty_settings_patch),
    },
    Entry {
        operation_id: "listAccounts",
        access: permission(AccountView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getAccount",
        access: permission(AccountView::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
    },
    Entry {
        operation_id: "createAccount",
        access: permission(
            AccountManage::KEY,
            ACCOUNT_NOT_FOUND,
            new_account_under_tower,
        ),
    },
    Entry {
        operation_id: "updateAccount",
        access: permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, rename_tower),
    },
    Entry {
        operation_id: "moveAccount",
        access: permission(
            AccountManage::KEY,
            ACCOUNT_NOT_FOUND,
            move_ground_within_tower,
        ),
    },
    Entry {
        operation_id: "deleteAccount",
        access: permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, ground_under_tower),
    },
    Entry {
        operation_id: "listPermissions",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "listRoles",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getRole",
        access: permission(RoleView::KEY, ROLE_NOT_FOUND, custom_role_in_path),
    },
    Entry {
        operation_id: "createRole",
        access: permission(RoleManage::KEY, Target::Org, new_role_in_body),
    },
    Entry {
        operation_id: "updateRole",
        access: permission(RoleManage::KEY, ROLE_NOT_FOUND, rename_custom_role),
    },
    Entry {
        operation_id: "deleteRole",
        access: permission(RoleManage::KEY, ROLE_NOT_FOUND, custom_role_in_path),
    },
    Entry {
        operation_id: "listRoleAssignments",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "createRoleAssignment",
        // The user is checked first, so another organization's request ends at its user.
        access: permission(
            RoleAssign::KEY,
            Target::Scoped {
                not_found: "account.not_found",
                cross_org: "user.not_found",
            },
            assignment_at_tower_in_body,
        ),
    },
    Entry {
        operation_id: "deleteRoleAssignment",
        access: permission(
            RoleAssign::KEY,
            Target::Scoped {
                not_found: "role_assignment.not_found",
                cross_org: "role_assignment.not_found",
            },
            assignment_at_tower_in_path,
        ),
    },
    Entry {
        operation_id: "listUsers",
        access: permission(UserView::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "getUser",
        access: permission(UserView::KEY, USER_NOT_FOUND, user_in_path),
    },
    Entry {
        operation_id: "createUser",
        access: permission(UserManage::KEY, Target::Org, new_user_in_body),
    },
    Entry {
        operation_id: "updateUser",
        access: permission(UserManage::KEY, USER_NOT_FOUND, rename_user_in_path),
    },
];

/// A user that is not the organization's: 404 `user.not_found`. The user routes take the
/// organization as their target (spec 03 API), so they have no wrong-account 404 case.
const USER_NOT_FOUND: Target = Target::Scoped {
    not_found: "user.not_found",
    cross_org: "user.not_found",
};

/// A new user of `org`, named in the path; the caller acts at organization scope.
fn user_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        Call {
            params: vec![("user_id", user)],
            body: None,
            at: At::Org,
        }
    })
}

/// [`user_in_path`] with a new display name in the body.
fn rename_user_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "display_name": "Renamed user" })),
            ..user_in_path(org).await
        }
    })
}

/// A new user named in the body.
fn new_user_in_body(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "display_name": "Casey Example" })),
            at: At::Org,
        }
    })
}

/// A new name for the caller's organization.
fn rename_organization(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "name": "Renamed organization" })),
            at: At::Org,
        }
    })
}

/// A settings patch naming no key, the only valid one while no setting exists.
fn empty_settings_patch(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({})),
            at: At::Org,
        }
    })
}

/// A new user and an unassigned custom role with no permissions, to assign at Tower: any
/// `role.assign` holder at Tower may grant it.
fn assignment_at_tower_in_body(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        let role = org.custom_role().await;
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "user_id": user,
                "role_id": role,
                "scope": { "type": "account", "id": org.tower },
            })),
            at: At::Account(org.tower),
        }
    })
}

/// A new user's assignment of a custom role with no permissions at Tower, named in the path.
fn assignment_at_tower_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        let role = org.custom_role().await;
        let assignment = org.assign(user, role, At::Account(org.tower)).await;
        Call {
            params: vec![("assignment_id", assignment)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A new account under Tower, named in the body; the caller acts at Tower.
fn new_account_under_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "parent_id": org.tower, "name": "Approach" })),
            at: At::Account(org.tower),
        }
    })
}

/// [`tower_in_path`] with a new name in the body.
fn rename_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "name": "Renamed account" })),
            ..tower_in_path(org).await
        }
    })
}

/// Ground, checked at its parent Tower, moved under a new account beside it: the destination
/// is inside Tower's subtree, so `account.manage` at Tower covers both ends of the move.
fn move_ground_within_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let destination = org.account(org.tower, "Destination").await;
        Call {
            body: Some(serde_json::json!({ "parent_id": destination })),
            ..ground_under_tower(org).await
        }
    })
}

/// A role that is not the organization's: 404 `role.not_found`. Roles are organization-wide,
/// so a role route's scope is the organization and it has no wrong-account 404 case.
const ROLE_NOT_FOUND: Target = Target::Scoped {
    not_found: "role.not_found",
    cross_org: "role.not_found",
};

/// A new unassigned custom role of `org`, named in the path; the caller acts at organization
/// scope.
fn custom_role_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("role_id", org.custom_role().await)],
            body: None,
            at: At::Org,
        }
    })
}

/// [`custom_role_in_path`] with a new name in the body.
fn rename_custom_role(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "name": "Renamed role" })),
            ..custom_role_in_path(org).await
        }
    })
}

/// A role with no permissions, which any `role.manage` holder may define.
fn new_role_in_body(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "key": "suite_new_role",
                "name": "Suite new role",
                "permissions": [],
            })),
            at: At::Org,
        }
    })
}

/// A protected operation and how to call it.
struct Entry {
    /// The OpenAPI `operationId`; the method and path come from the document.
    operation_id: &'static str,
    access: Access,
}

/// What an operation needs besides a principal.
enum Access {
    /// A principal and no permission (`GET /api/v1/me`): the 401, 2xx and cross-organization
    /// cases.
    AuthOnly(Setup),
    /// `Require<P, T>`: every case.
    Permission {
        /// `P::KEY`.
        key: &'static str,
        target: Target,
        /// Permissions the 2xx case adds to `P`, which the route documents (a move's
        /// destination).
        extra: &'static [&'static str],
        setup: Setup,
    },
}

/// How the route finds its target, as far as the cases need to know.
#[derive(Clone, Copy)]
enum Target {
    /// `OrgTarget`: the caller's own organization. No 404 case; the cross-organization case acts
    /// on org A and answers 2xx.
    Org,
    /// `AnyScope` without a target (a list). No 404 case; the cross-organization case answers
    /// 2xx.
    AnyScope,
    /// A target the setup builds, in the path (`AccountTarget`, `AccountParentTarget`) or the
    /// body (`AnyScope` with `require_at`). Hidden, it answers 404 with `not_found`; built in
    /// another organization, 404 with `cross_org`, which differs when the route checks another
    /// id of the request first (a role assignment's user, B12).
    Scoped {
        not_found: &'static str,
        cross_org: &'static str,
    },
}

/// Builds what one case calls in a fresh organization.
type Setup = for<'a> fn(&'a Org) -> Pin<Box<dyn Future<Output = Call> + Send + 'a>>;

/// One call of an operation.
struct Call {
    /// Values of the path's `{name}` segments.
    params: Vec<(&'static str, Uuid)>,
    body: Option<Value>,
    /// The target's scope, where the 403 and 2xx cases assign their role: the organization for
    /// `OrgTarget`, the parent account for `AccountParentTarget`, the body's scope for a body
    /// target.
    at: At,
}

#[derive(Clone, Copy, Debug)]
enum At {
    Org,
    Account(Uuid),
}

// The walker (B21, T16).

const METHODS: [&str; 7] = ["get", "head", "options", "post", "put", "patch", "delete"];

/// An operation of the document.
#[derive(Clone, Debug)]
struct Operation {
    id: String,
    method: Method,
    /// The path template, `{name}` for a parameter.
    path: String,
}

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document serializes")
}

/// Every operation of `doc`, or the `method path` of those without an `operationId`.
fn operations(doc: &Value) -> Result<Vec<Operation>, Vec<String>> {
    let mut found = Vec::new();
    let mut unnamed = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            if !METHODS.contains(&method.as_str()) {
                continue;
            }
            match operation["operationId"].as_str() {
                Some(id) => found.push(Operation {
                    id: id.to_owned(),
                    method: Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method"),
                    path: path.clone(),
                }),
                None => unnamed.push(format!("{method} {path}")),
            }
        }
    }
    if unnamed.is_empty() {
        Ok(found)
    } else {
        Err(unnamed)
    }
}

/// What is wrong between `doc` and the suite's lists: operations in neither list, list names
/// that are no operation, and names listed twice.
fn walk(doc: &Value, table: &[&str], public: &[&str]) -> Vec<String> {
    let operations = match operations(doc) {
        Ok(operations) => operations,
        Err(unnamed) => return vec![format!("operations without an operationId: {unnamed:?}")],
    };
    let mut problems = Vec::new();
    for op in &operations {
        if !table.contains(&op.id.as_str()) && !public.contains(&op.id.as_str()) {
            problems.push(format!(
                "{} ({} {}) has no permission suite entry and is not public: add it to TABLE \
                 in tests/permissions.rs",
                op.id, op.method, op.path
            ));
        }
    }
    let listed: Vec<&str> = table.iter().chain(public).copied().collect();
    for (i, name) in listed.iter().enumerate() {
        if !operations.iter().any(|op| op.id == *name) {
            problems.push(format!(
                "{name} is listed but is no operation of the document"
            ));
        }
        if listed[..i].contains(name) {
            problems.push(format!("{name} is listed twice"));
        }
    }
    problems
}

fn table_ids() -> Vec<&'static str> {
    TABLE.iter().map(|entry| entry.operation_id).collect()
}

#[test]
fn b21_every_operation_is_in_the_table_or_the_public_list() {
    let problems = walk(&document(), &table_ids(), PUBLIC);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn t16_an_operation_without_an_entry_fails_the_walk_by_name() {
    let mut doc = document();
    doc["paths"]["/api/v1/widgets"]["post"] = serde_json::json!({ "operationId": "createWidget" });
    let problems = walk(&doc, &table_ids(), PUBLIC);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("createWidget (POST /api/v1/widgets)"),
        "{problems:#?}"
    );
}

#[test]
fn the_walk_refuses_stale_duplicate_and_unnamed_entries() {
    let doc = document();
    let problems = walk(&doc, &["healthz", "removedRoute"], PUBLIC);
    assert!(
        problems.contains(&"removedRoute is listed but is no operation of the document".into()),
        "{problems:#?}"
    );
    assert!(
        problems.contains(&"healthz is listed twice".into()),
        "{problems:#?}"
    );
    let mut unnamed = doc;
    unnamed["paths"]["/api/v1/widgets"]["get"] = serde_json::json!({});
    assert_eq!(
        walk(&unnamed, &table_ids(), PUBLIC),
        ["operations without an operationId: [\"get /api/v1/widgets\"]"]
    );
}

/// A protected route put in the public list would skip its cases; this catches it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn public_operations_answer_without_a_credential(pool: PgPool) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let org = world.org().await;
    let operations = operations(&document()).expect("named operations");
    for id in PUBLIC {
        let op = operations
            .iter()
            .find(|op| op.id == *id)
            .expect("listed operations exist");
        let answer = send(&app, &op.method, &op.path, org.id, None, None).await;
        assert!(answer.status.is_success(), "{id}: {answer:?}");
    }
}

// The suite over the real router.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b20_every_table_entry_passes_its_cases(pool: PgPool) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let operations = operations(&document()).expect("named operations");
    let mut failures = Vec::new();
    for entry in TABLE {
        let op = operations
            .iter()
            .find(|op| op.id == entry.operation_id)
            .unwrap_or_else(|| panic!("{} is not in the document", entry.operation_id));
        failures.extend(run(&world, &app, op, &entry.access).await.failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// The runner.

/// What the runner did with one entry.
#[derive(Debug, Default)]
struct Report {
    /// The cases that ran, in order.
    ran: Vec<&'static str>,
    failures: Vec<String>,
}

impl Report {
    fn record(&mut self, op: &Operation, case: &'static str, result: Result<(), String>) {
        self.ran.push(case);
        if let Err(why) = result {
            self.failures.push(format!(
                "{} ({} {}) {case}: {why}",
                op.id, op.method, op.path
            ));
        }
    }
}

async fn run(world: &World, app: &Router, op: &Operation, access: &Access) -> Report {
    let mut report = Report::default();
    match access {
        Access::AuthOnly(setup) => {
            report.record(op, "401", anonymous(world, app, op, *setup).await);
            report.record(op, "2xx", allowed(world, app, op, &[], *setup).await);
            let result = cross_organization(world, app, op, None, *setup).await;
            report.record(op, "cross-organization", result);
        }
        Access::Permission {
            key,
            target,
            extra,
            setup,
        } => {
            let Some(info) = perm::ALL.iter().find(|p| p.key() == *key) else {
                report.record(op, "catalog", Err(format!("{key} is not in perm::ALL")));
                return report;
            };
            report.record(op, "401", anonymous(world, app, op, *setup).await);
            report.record(op, "403", denied(world, app, op, key, *setup).await);
            if let Target::Scoped { not_found, .. } = target
                && info.grantable().contains(&ScopeType::Account)
                && let Some(result) = hidden(world, app, op, not_found, *setup).await
            {
                report.record(op, "404", result);
            }
            let keys: Vec<&str> = [*key].iter().chain(*extra).copied().collect();
            report.record(op, "2xx", allowed(world, app, op, &keys, *setup).await);
            let not_found = match target {
                Target::Scoped { cross_org, .. } => Some(*cross_org),
                Target::Org | Target::AnyScope => None,
            };
            let result = cross_organization(world, app, op, not_found, *setup).await;
            report.record(op, "cross-organization", result);
        }
    }
    report
}

async fn anonymous(
    world: &World,
    app: &Router,
    op: &Operation,
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let answer = call_op(app, op, &call, org.id, None).await?;
    answer.expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
}

async fn denied(
    world: &World,
    app: &Router,
    op: &Operation,
    key: &str,
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let others: Vec<&str> = all_keys().into_iter().filter(|k| *k != key).collect();
    let cookie = org.holder(&others, call.at).await;
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    answer.expect(StatusCode::FORBIDDEN, "permission.denied")
}

/// `None` when the target's scope is the organization, which has no sibling.
async fn hidden(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: &str,
    setup: Setup,
) -> Option<Result<(), String>> {
    let org = world.org().await;
    let call = setup(&org).await;
    let At::Account(account) = call.at else {
        return None;
    };
    let sibling = org.sibling(account).await;
    let cookie = org.holder(&all_keys(), At::Account(sibling)).await;
    Some(match call_op(app, op, &call, org.id, Some(&cookie)).await {
        Ok(answer) => answer.expect(StatusCode::NOT_FOUND, not_found),
        Err(why) => Err(why),
    })
}

/// With `keys` empty, a user with no role at all.
async fn allowed(
    world: &World,
    app: &Router,
    op: &Operation,
    keys: &[&str],
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let cookie = if keys.is_empty() {
        org.user().await.0
    } else {
        org.holder(keys, call.at).await
    };
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    answer.expect_success()
}

/// An Organization admin of org A calls with the target built in org B, which also holds a
/// user, a session, a custom role and an assignment: `not_found` (or 2xx when the route has no
/// target), org B's rows unchanged, and none of their identifying values answered. Every one of
/// the three that fails is reported.
async fn cross_organization(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: Option<&str>,
    setup: Setup,
) -> Result<(), String> {
    let a = world.org().await;
    let admin = a.holder_of_builtin("org_admin", At::Org).await;
    let b = world.org().await;
    b.seed().await;
    let call = setup(&b).await;
    let before = snapshot(&world.pool, b.id).await?;
    let answer = call_op(app, op, &call, a.id, Some(&admin)).await?;
    let mut wrong = Vec::new();
    let status = match not_found {
        Some(code) => answer.expect(StatusCode::NOT_FOUND, code),
        None => answer.expect_success(),
    };
    wrong.extend(status.err());
    if snapshot(&world.pool, b.id).await? != before {
        wrong.push("org B's rows changed".to_owned());
    }
    let mut identifying = Vec::new();
    identifiers(&before, &b.slug, &mut identifying);
    if let Some(value) = identifying
        .iter()
        .find(|v| answer.body.contains(v.as_str()))
    {
        wrong.push(format!("the response holds org B's value {value:?}"));
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

/// Tables whose rows belong to an organization without an `org_id` column, with the condition
/// that picks organization `$1`'s rows of `t`.
const SCOPED_OTHERWISE: &[(&str, &str)] = &[
    ("organizations", "t.id = $1"),
    (
        "sessions",
        "t.user_id IN (SELECT id FROM users WHERE org_id = $1)",
    ),
];

/// Tables that hold no organization's rows. `plans` holds instance-wide plan definitions (spec
/// 10 Data); an organization's plan is its `org_plans` row, which has `org_id`.
const INSTANCE_ONLY: &[&str] = &["_sqlx_migrations", "plans"];

/// Every row of organization `org`, by table: each table with `org_id`, and each table of
/// [`SCOPED_OTHERWISE`]. A table of the schema in neither, nor in [`INSTANCE_ONLY`], is an error,
/// so a new table scoped through another one cannot be left out of the comparison.
async fn snapshot(pool: &PgPool, org: Uuid) -> Result<Value, String> {
    let tables: Vec<(String, bool)> = sqlx::query_as(
        "SELECT t.table_name::text, EXISTS (
                SELECT 1 FROM information_schema.columns c
                WHERE c.table_schema = t.table_schema AND c.table_name = t.table_name
                  AND c.column_name = 'org_id')
         FROM information_schema.tables t
         WHERE t.table_schema = 'public' AND t.table_type = 'BASE TABLE'
         ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .expect("list tables");
    let mut rows = serde_json::Map::new();
    let mut unscoped = Vec::new();
    for (table, has_org_id) in tables {
        let other = SCOPED_OTHERWISE.iter().find(|(name, _)| *name == table);
        let condition = match (has_org_id, other) {
            (_, Some((_, condition))) => *condition,
            (true, None) => "t.org_id = $1",
            (false, None) if INSTANCE_ONLY.contains(&table.as_str()) => continue,
            (false, None) => {
                unscoped.push(table);
                continue;
            }
        };
        let sql = format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text), '[]')::text
             FROM \"{table}\" t WHERE {condition}"
        );
        let json: String = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(org)
            .fetch_one(pool)
            .await
            .expect("read rows");
        rows.insert(table, serde_json::from_str(&json).expect("JSON rows"));
    }
    if unscoped.is_empty() {
        Ok(Value::Object(rows))
    } else {
        Err(format!(
            "tables with no org_id and no entry in SCOPED_OTHERWISE or INSTANCE_ONLY in \
             tests/permissions.rs: {unscoped:?}"
        ))
    }
}

/// Every string in `value` that is a UUID or contains the organization's `slug` (its name, and
/// the seeded names, emails and role key).
fn identifiers(value: &Value, slug: &str, out: &mut Vec<String>) {
    match value {
        Value::String(s) if Uuid::try_parse(s).is_ok() || s.contains(slug) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| identifiers(v, slug, out)),
        Value::Object(map) => map.values().for_each(|v| identifiers(v, slug, out)),
        _ => {}
    }
}

fn all_keys() -> Vec<&'static str> {
    perm::ALL.iter().map(|p| p.key()).collect()
}

// Requests.

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: String,
}

impl Answer {
    fn code(&self) -> Option<String> {
        serde_json::from_str::<Problem>(&self.body)
            .ok()
            .map(|p| p.code)
    }

    fn expect(&self, status: StatusCode, code: &str) -> Result<(), String> {
        if self.status == status && self.code().as_deref() == Some(code) {
            Ok(())
        } else {
            Err(format!(
                "expected {status} {code}, got {} {}",
                self.status, self.body
            ))
        }
    }

    fn expect_success(&self) -> Result<(), String> {
        if self.status.is_success() {
            Ok(())
        } else {
            Err(format!("expected 2xx, got {} {}", self.status, self.body))
        }
    }
}

/// `call` against `op`, in the organization `org`.
async fn call_op(
    app: &Router,
    op: &Operation,
    call: &Call,
    org: Uuid,
    cookie: Option<&str>,
) -> Result<Answer, String> {
    let mut uri = op.path.clone();
    for (name, value) in &call.params {
        uri = uri.replace(&format!("{{{name}}}"), &value.to_string());
    }
    if uri.contains('{') {
        return Err(format!("the setup leaves a path parameter unset: {uri}"));
    }
    Ok(send(app, &op.method, &uri, org, cookie, call.body.as_ref()).await)
}

/// One request, same-origin as the browser app sends it, in the organization `org`.
async fn send(
    app: &Router,
    method: &Method,
    uri: &str,
    org: Uuid,
    cookie: Option<&str>,
    body: Option<&Value>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(TEST_ORG_HEADER, org.to_string())
        .header("sec-fetch-site", "same-origin");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let body = match body {
        Some(json) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("a valid request"))
        .await
        .expect("the router never fails");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    Answer {
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

// Fixtures.

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// The test's database, with the built-in roles synced.
struct World {
    pool: PgPool,
}

impl World {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        Self { pool }
    }

    fn state(&self) -> AppState {
        AppState {
            db: self.pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
        }
    }

    /// A new organization with Tower under the root and Ground under Tower.
    async fn org(&self) -> Org {
        let hex = new_id().simple().to_string();
        let slug = format!("org-{}", &hex[hex.len() - 16..]);
        let id = organizations::create(&self.pool, &slug, &slug, now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("the root");
        let mut org = Org {
            pool: self.pool.clone(),
            id,
            slug,
            tower: root,
            ground: root,
        };
        org.tower = org.account(root, "Tower").await;
        org.ground = org.account(org.tower, "Ground").await;
        org
    }
}

/// One organization of the test's database and its accounts.
struct Org {
    pool: PgPool,
    id: Uuid,
    /// Also its name.
    slug: String,
    tower: Uuid,
    ground: Uuid,
}

impl Org {
    async fn account(&self, parent: Uuid, name: &str) -> Uuid {
        accounts::create(
            &self.pool,
            scope(self.id),
            parent,
            name,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create account")
        .id
    }

    /// A new account beside `account`, under its parent; under the root itself when `account`
    /// is the root, which still does not contain it.
    async fn sibling(&self, account: Uuid) -> Uuid {
        let parent: Option<Uuid> =
            sqlx::query_scalar("SELECT parent_id FROM accounts WHERE id = $1")
                .bind(account)
                .fetch_one(&self.pool)
                .await
                .expect("the account");
        self.account(parent.unwrap_or(account), "Sibling").await
    }

    /// A user, signed in, with a custom role at the organization, every value naming the slug:
    /// the rows of org B a cross-organization call must neither change nor answer.
    async fn seed(&self) {
        let slug = &self.slug;
        let (_, user) = self
            .named_user(
                format!("Robin {slug}"),
                Some(format!("robin@{slug}.example")),
            )
            .await;
        let role = NewRole {
            key: format!("r_{}", slug.replace('-', "_")),
            name: format!("Role of {slug}"),
            permissions: vec![RoleView::KEY.to_owned()],
        };
        let role = roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, At::Org).await;
    }

    /// A new user, signed in, with no role: their `Cookie` header value and id.
    async fn user(&self) -> (String, Uuid) {
        self.named_user("Pat Example".into(), None).await
    }

    async fn named_user(&self, display_name: String, email: Option<String>) -> (String, Uuid) {
        let new_user = NewUser {
            display_name,
            given_name: String::new(),
            family_name: String::new(),
            email,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let id = users::create(&self.pool, scope(self.id), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(self.id), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    async fn assign(&self, user: Uuid, role: Uuid, at: At) -> Uuid {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: match at {
                At::Org => Scope::Organization,
                At::Account(id) => Scope::Account(id),
            },
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.id),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment")
        .id
    }

    /// A signed-in user holding a new custom role with exactly `permissions` at `at`.
    async fn holder(&self, permissions: &[&str], at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Suite role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, at).await;
        cookie
    }

    /// A new custom role with no permissions and no assignment.
    async fn custom_role(&self) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Target role".into(),
            permissions: Vec::new(),
        };
        roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    async fn holder_of_builtin(&self, key: &str, at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(user, role, at).await;
        cookie
    }
}

// The runner on test-only routes, one per target kind, and two broken ones (T15).

async fn org_route(_: Require<OrgManage>) -> &'static str {
    "ok"
}

async fn rename_account(
    require: Require<AccountManage, AccountTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let scope = require.principal().scope();
    accounts::rename(
        &state.db,
        scope,
        require.target().id,
        "Renamed",
        now(),
        Actor::Instance,
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    Ok(StatusCode::OK)
}

async fn delete_account(
    require: Require<AccountManage, AccountParentTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let scope = require.principal().scope();
    accounts::delete(
        &state.db,
        scope,
        require.target().id,
        UNIX_EPOCH,
        Actor::Instance,
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_roles(require: Require<RoleView, AnyScope>) -> String {
    format!("{:?}", require.grants().scopes_with::<RoleView>())
}

#[derive(Deserialize)]
struct AssignBody {
    account_id: Uuid,
}

async fn assign_role(
    require: Require<RoleAssign, AnyScope>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AssignBody>,
) -> Result<StatusCode, ApiError> {
    let at = ScopeId::Account(body.account_id);
    require
        .grants()
        .require_at::<RoleAssign>(&state.db, at)
        .await?;
    Ok(StatusCode::CREATED)
}

async fn me(principal: Authenticated) -> String {
    principal.user_id.to_string()
}

/// No `Require`, and an update that ignores the organization.
async fn open_rename(State(state): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> StatusCode {
    sqlx::query("UPDATE accounts SET name = 'Renamed' WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .expect("update");
    StatusCode::OK
}

/// Signs every other user out, in every organization.
async fn end_other_sessions(principal: Authenticated, State(state): State<AppState>) -> StatusCode {
    sqlx::query("DELETE FROM sessions WHERE user_id <> $1")
        .bind(principal.user_id)
        .execute(&state.db)
        .await
        .expect("delete");
    StatusCode::NO_CONTENT
}

/// Every user's name, in every organization.
async fn every_name(_: Authenticated, State(state): State<AppState>) -> String {
    let names: Vec<String> = sqlx::query_scalar("SELECT display_name FROM users")
        .fetch_all(&state.db)
        .await
        .expect("select");
    names.join(",")
}

/// Checks `account.view` where its entry says `account.manage`.
async fn wrong_permission(_: Require<AccountView, AccountTarget>) -> StatusCode {
    StatusCode::OK
}

fn fixture_app(world: &World) -> Router {
    Router::new()
        .route("/t/org", get(org_route))
        .route(
            "/t/accounts/{account_id}",
            patch(rename_account).delete(delete_account),
        )
        .route("/t/roles", get(list_roles))
        .route("/t/assignments", post(assign_role))
        .route("/t/me", get(me))
        .route("/t/open/{account_id}", patch(open_rename))
        .route("/t/wrong/{account_id}", patch(wrong_permission))
        .route("/t/sessions", axum::routing::delete(end_other_sessions))
        .route("/t/names", get(every_name))
        .with_state(world.state())
}

fn fixture_op(method: Method, path: &str) -> Operation {
    Operation {
        id: format!("{method} {path}"),
        method,
        path: path.to_owned(),
    }
}

fn at_org(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        }
    })
}

fn tower_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("account_id", org.tower)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// Ground, checked at its parent Tower.
fn ground_under_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("account_id", org.ground)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

fn tower_in_body(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "account_id": org.tower })),
            at: At::Account(org.tower),
        }
    })
}

const ACCOUNT_NOT_FOUND: Target = Target::Scoped {
    not_found: "account.not_found",
    cross_org: "account.not_found",
};

const fn permission(key: &'static str, target: Target, setup: Setup) -> Access {
    Access::Permission {
        key,
        target,
        extra: &[],
        setup,
    }
}

const EVERY_CASE: [&str; 5] = ["401", "403", "404", "2xx", "cross-organization"];
const NO_404: [&str; 4] = ["401", "403", "2xx", "cross-organization"];

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_runner_passes_a_correct_route_of_each_target_kind(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let cases: [(Method, &str, Access, &[&str]); 6] = [
        (
            Method::GET,
            "/t/org",
            permission(OrgManage::KEY, Target::Org, at_org),
            &NO_404,
        ),
        (
            Method::PATCH,
            "/t/accounts/{account_id}",
            permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
            &EVERY_CASE,
        ),
        (
            Method::DELETE,
            "/t/accounts/{account_id}",
            permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, ground_under_tower),
            &EVERY_CASE,
        ),
        (
            Method::GET,
            "/t/roles",
            permission(RoleView::KEY, Target::AnyScope, at_org),
            &NO_404,
        ),
        (
            Method::POST,
            "/t/assignments",
            permission(RoleAssign::KEY, ACCOUNT_NOT_FOUND, tower_in_body),
            &EVERY_CASE,
        ),
        (
            Method::GET,
            "/t/me",
            Access::AuthOnly(at_org),
            &["401", "2xx", "cross-organization"],
        ),
    ];
    for (method, path, access, expected) in cases {
        let op = fixture_op(method, path);
        let report = run(&world, &app, &op, &access).await;
        assert!(report.failures.is_empty(), "{:#?}", report.failures);
        assert_eq!(report.ran, expected, "{}", op.id);
    }
}

/// Organization-only permissions have no 404 case: no account grant can hold them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_only_permission_has_no_404_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/org");
    let access = permission(OrgManage::KEY, ACCOUNT_NOT_FOUND, at_org);
    let report = run(&world, &app, &op, &access).await;
    assert_eq!(report.ran, NO_404);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_route_without_require_fails_its_401_and_cross_organization_cases(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/open/{account_id}");
    let access = permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path);
    let report = run(&world, &app, &op, &access).await;
    let failed = failed_cases(&report);
    assert_eq!(
        failed,
        ["401", "403", "404", "cross-organization"],
        "{report:#?}"
    );
    assert!(
        report.failures[3].contains("org B's rows changed"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_route_checking_another_permission_fails_its_2xx_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/wrong/{account_id}");
    let access = permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path);
    let report = run(&world, &app, &op, &access).await;
    let failed = failed_cases(&report);
    assert_eq!(failed, ["403", "2xx"], "{report:#?}");
}

/// The case names in `report`'s failures, in order.
fn failed_cases(report: &Report) -> Vec<&str> {
    report
        .failures
        .iter()
        .map(|f| {
            f.split(": ")
                .next()
                .expect("a case")
                .rsplit(' ')
                .next()
                .expect("a case")
        })
        .collect()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_auth_only_route_ending_org_bs_sessions_fails_the_cross_organization_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::DELETE, "/t/sessions");
    let report = run(&world, &app, &op, &Access::AuthOnly(at_org)).await;
    assert_eq!(failed_cases(&report), ["cross-organization"], "{report:#?}");
    assert!(
        report.failures[0].ends_with("org B's rows changed"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_auth_only_route_answering_org_bs_names_fails_the_cross_organization_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/names");
    let report = run(&world, &app, &op, &Access::AuthOnly(at_org)).await;
    assert_eq!(failed_cases(&report), ["cross-organization"], "{report:#?}");
    assert!(
        report.failures[0].contains("the response holds org B's value \"org-"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_table_scoped_by_neither_rule_fails_the_snapshot(pool: PgPool) {
    let world = World::new(pool).await;
    let org = world.org().await;
    assert!(snapshot(&world.pool, org.id).await.is_ok());
    sqlx::query("CREATE TABLE stray_notes (id uuid PRIMARY KEY, user_id uuid)")
        .execute(&world.pool)
        .await
        .expect("create table");
    let error = snapshot(&world.pool, org.id)
        .await
        .expect_err("an unscoped table");
    assert!(error.ends_with(r#"["stray_notes"]"#), "{error}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_permission_key_fails_the_entry(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/org");
    let report = run(
        &world,
        &app,
        &op,
        &permission("org.everything", Target::Org, at_org),
    )
    .await;
    assert_eq!(report.ran, ["catalog"]);
    assert_eq!(report.failures.len(), 1);
}

// The test-support organization header.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_test_header_picks_one_of_several_organizations(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let a = world.org().await;
    let b = world.org().await;
    let (cookie, user) = b.user().await;
    let me = |org: Option<Uuid>| {
        let mut request = Request::builder()
            .uri("/t/me")
            .header(header::COOKIE, &cookie);
        if let Some(org) = org {
            request = request.header(TEST_ORG_HEADER, org.to_string());
        }
        let app = app.clone();
        async move {
            let response = app
                .oneshot(request.body(Body::empty()).expect("a request"))
                .await
                .expect("the router never fails");
            let status = response.status();
            let body = response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            Answer {
                status,
                body: String::from_utf8_lossy(&body).into_owned(),
            }
        }
    };
    let answer = me(Some(b.id)).await;
    assert_eq!(
        (answer.status, answer.body),
        (StatusCode::OK, user.to_string())
    );
    // Org B's session is not one of org A's (spec 08 B6).
    me(Some(a.id))
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
        .unwrap();
    me(Some(new_id()))
        .await
        .expect(StatusCode::NOT_FOUND, "org.not_found")
        .unwrap();
    // Without the header the production rule applies: two organizations, none is picked.
    me(None)
        .await
        .expect(StatusCode::NOT_FOUND, "org.not_found")
        .unwrap();
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_malformed_test_header_names_no_organization(pool: PgPool) {
    let world = World::new(pool).await;
    let org = world.org().await;
    let (cookie, _) = org.user().await;
    let request = Request::builder()
        .uri("/t/me")
        .header(header::COOKIE, cookie)
        .header(TEST_ORG_HEADER, "not-a-uuid")
        .body(Body::empty())
        .expect("a request");
    let response = fixture_app(&world)
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    Answer {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
    .expect(StatusCode::NOT_FOUND, "org.not_found")
    .unwrap();
}
