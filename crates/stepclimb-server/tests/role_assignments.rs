// SPDX-License-Identifier: AGPL-3.0-or-later
//! The role assignment routes over the whole router (docs/specs/m0/09-permissions-and-roles.md
//! B11 to B14, T9 to T11, T14's assignments half). The four permission cases and the
//! cross-organization case of each route are in `tests/permissions.rs`.
//!
//! Org A holds Tower and Enroute under the root and Ground under Tower; org B exists only to
//! hold ids of another organization, so requests name org A with the `test-support` header.
//! `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;

const ASSIGNMENTS: &str = "/api/v1/admin/role-assignments";

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

#[derive(Clone, Copy)]
enum At {
    Org,
    Account(Uuid),
}

impl At {
    fn json(self, org: Uuid) -> Value {
        match self {
            Self::Org => json!({ "type": "organization", "id": org }),
            Self::Account(id) => json!({ "type": "account", "id": id }),
        }
    }
}

/// One organization's ids.
struct Org {
    id: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    enroute: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    a: Org,
    b: Org,
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    /// The `(field, code)` pairs of a 422's `errors`.
    fn errors(&self) -> Vec<(String, String)> {
        self.body["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .map(|e| {
                        (
                            e["field"].as_str().expect("a field").into(),
                            e["code"].as_str().expect("a code").into(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `id`s of a page's items.
    fn ids(&self) -> Vec<Uuid> {
        self.body["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|item| item["id"].as_str().expect("an id").parse().expect("a UUID"))
            .collect()
    }
}

async fn org(pool: &PgPool, slug: &str) -> Org {
    let id = organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the root");
    let account = |parent, name| async move {
        accounts::create(pool, scope(id), parent, name, now())
            .await
            .expect("create account")
            .id
    };
    let tower = account(root, "Tower").await;
    let ground = account(tower, "Ground").await;
    let enroute = account(root, "Enroute").await;
    Org {
        id,
        root,
        tower,
        ground,
        enroute,
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let a = org(&pool, "org-a").await;
        let b = org(&pool, "org-b").await;
        let app = router(AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
        });
        Self { pool, app, a, b }
    }

    /// A new user of `org` with no role, and a session: their `Cookie` value and id.
    async fn user_in(&self, org: Uuid) -> (String, Uuid) {
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
        let id = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    async fn user(&self) -> (String, Uuid) {
        self.user_in(self.a.id).await
    }

    async fn custom_role_in(&self, org: Uuid, permissions: &[&str]) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(org), &role, now())
            .await
            .expect("create role")
            .id
    }

    async fn custom_role(&self, permissions: &[&str]) -> Uuid {
        self.custom_role_in(self.a.id, permissions).await
    }

    async fn builtin(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    /// Assign `role` to `user` of org A at `at`, as an admin would; the assignment's id.
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
        role_assignments::create(&self.pool, scope(self.a.id), &assignment, now())
            .await
            .expect("create assignment")
            .id
    }

    /// A signed-in user of org A holding the built-in role `key` at `at`.
    async fn holder_of(&self, key: &str, at: At) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        self.assign(user, self.builtin(key).await, at).await;
        (cookie, user)
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, self.a.id.to_string())
            .header(header::COOKIE, cookie)
            .header("sec-fetch-site", "same-origin");
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a valid request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("a JSON body")
        };
        Answer {
            status,
            headers,
            body,
        }
    }

    async fn create(&self, cookie: &str, user: Uuid, role: Uuid, at: Value) -> Answer {
        let body = json!({ "user_id": user, "role_id": role, "scope": at });
        self.send(Method::POST, ASSIGNMENTS, cookie, Some(body))
            .await
    }

    async fn delete(&self, cookie: &str, assignment: Uuid) -> Answer {
        self.send(
            Method::DELETE,
            &format!("{ASSIGNMENTS}/{assignment}"),
            cookie,
            None,
        )
        .await
    }

    async fn list(&self, cookie: &str, query: &str) -> Answer {
        self.send(Method::GET, &format!("{ASSIGNMENTS}{query}"), cookie, None)
            .await
    }

    async fn exists(&self, assignment: Uuid) -> bool {
        role_assignments::get(&self.pool, scope(self.a.id), assignment)
            .await
            .expect("read assignment")
            .is_some()
    }
}

// Creating (B11, B12, B13).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_each_check_answers_in_order(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, tower_admin_id) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (_, pat) = f.user().await;
    let (_, b_user) = f.user_in(f.b.id).await;
    let account_admin = f.builtin("account_admin").await;
    let org_admin = f.builtin("org_admin").await;
    let b_role = f.custom_role_in(f.b.id, &[]).await;
    let ground = At::Account(f.a.ground).json(f.a.id);
    let course = json!({ "type": "course", "id": new_id() });

    let steps = [
        // 1: the user, even with a scope the caller cannot see.
        (b_user, account_admin, At::Account(f.a.enroute).json(f.a.id)),
        // 2: the role.
        (pat, b_role, ground.clone()),
        // 3: the scope.
        (pat, account_admin, course),
        (pat, account_admin, At::Account(f.b.tower).json(f.a.id)),
        // 4: Enroute is outside Tower's subtree.
        (pat, account_admin, At::Account(f.a.enroute).json(f.a.id)),
        // 5: Organization admin holds organization-wide permissions Tower's admin lacks.
        (pat, org_admin, At::Account(f.a.tower).json(f.a.id)),
    ];
    let mut answers = Vec::new();
    for (user, role, at) in steps {
        let answer = f.create(&tower_admin, user, role, at).await;
        answers.push((answer.status.as_u16(), answer.code().to_owned()));
    }
    let created = f
        .create(&tower_admin, pat, account_admin, ground.clone())
        .await;
    let repeated = f.create(&tower_admin, pat, account_admin, ground).await;

    assert_eq!(
        answers,
        [
            (404, "user.not_found".into()),
            (404, "role.not_found".into()),
            (422, "request.invalid".into()),
            (404, "account.not_found".into()),
            (404, "account.not_found".into()),
            (403, "permission.escalation".into()),
        ]
    );
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(repeated.status, StatusCode::CONFLICT, "{repeated:?}");
    assert_eq!(repeated.code(), "role_assignment.exists");
    // Step 3's org B account, and every other refusal, wrote nothing in either organization.
    let rows: Vec<(Uuid, Uuid)> =
        sqlx::query_as("SELECT org_id, scope_id FROM role_assignments WHERE user_id <> $1")
            .bind(tower_admin_id)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(rows, [(f.a.id, f.a.ground)]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_scope_is_unavailable_at_its_field(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&[]).await;
    let course = json!({ "type": "course", "id": new_id() });

    let answer = f.create(&admin, pat, role, course).await;

    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{answer:?}"
    );
    assert_eq!(answer.code(), "request.invalid");
    assert_eq!(
        answer.errors(),
        [(
            "/scope/type".to_owned(),
            "role_assignment.scope_unavailable".to_owned()
        )]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_manager_and_observer_cannot_be_assigned(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    for key in ["manager", "observer"] {
        for at in [At::Org, At::Account(f.a.tower)] {
            let answer = f
                .create(&admin, pat, f.builtin(key).await, at.json(f.a.id))
                .await;
            assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{key}");
            assert_eq!(
                answer.errors(),
                [(
                    "/role_id".to_owned(),
                    "role_assignment.scope_unavailable".to_owned()
                )],
                "{key}"
            );
        }
    }
    // A custom role keyed like them is not affected.
    let custom = NewRole {
        key: "observer_custom".into(),
        name: "Observer custom".into(),
        permissions: Vec::new(),
    };
    let custom = roles::create_custom(&f.pool, scope(f.a.id), &custom, now())
        .await
        .unwrap()
        .id;
    let answer = f.create(&admin, pat, custom, At::Org.json(f.a.id)).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_created_assignment_is_local_and_located(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&["account.view"]).await;

    let answer = f.create(&admin, pat, role, At::Org.json(f.a.id)).await;

    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    let id: Uuid = answer.body["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        answer.body,
        json!({
            "id": id,
            "user_id": pat,
            "role_id": role,
            "scope": { "type": "organization", "id": f.a.id },
            "source": "local",
            "external_id": null,
            "created_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    assert_eq!(
        answer.headers[header::LOCATION],
        format!("{ASSIGNMENTS}/{id}").as_str()
    );
    assert!(f.exists(id).await);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_scope_must_name_this_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&[]).await;

    let answer = f.create(&admin, pat, role, At::Org.json(f.b.id)).await;

    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{answer:?}");
    assert_eq!(answer.code(), "org.not_found");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_body_member_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&[]).await;
    let body = json!({
        "user_id": pat, "role_id": role,
        "scope": { "type": "organization", "id": f.a.id },
        "source": "sync",
    });

    let answer = f.send(Method::POST, ASSIGNMENTS, &admin, Some(body)).await;

    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{answer:?}");
    assert_eq!(answer.code(), "request.malformed");
}

/// The escalation rule checks every catalog permission of the role, also those that would take
/// no effect at the scope, and an organization scope is visible, so missing `role.assign` there
/// is a 403.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn escalation_covers_every_permission_of_the_role(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (_, pat) = f.user().await;
    let own = f.custom_role(&["account.view", "role.assign"]).await;
    let mixed = f
        .custom_role(&["account.view", "organization.manage"])
        .await;
    let unknown_key = f.custom_role(&[]).await;
    sqlx::query("UPDATE roles SET permissions = '{retired.key}' WHERE id = $1")
        .bind(unknown_key)
        .execute(&f.pool)
        .await
        .unwrap();
    let at_tower = || At::Account(f.a.tower).json(f.a.id);

    let held = f.create(&tower_admin, pat, own, at_tower()).await;
    let org_only = f.create(&tower_admin, pat, mixed, at_tower()).await;
    let retired = f.create(&tower_admin, pat, unknown_key, at_tower()).await;
    let at_org = f.create(&tower_admin, pat, own, At::Org.json(f.a.id)).await;

    assert_eq!(held.status, StatusCode::CREATED, "{held:?}");
    assert_eq!(org_only.status, StatusCode::FORBIDDEN, "{org_only:?}");
    assert_eq!(org_only.code(), "permission.escalation");
    // A key the catalog does not define grants nothing and is not checked.
    assert_eq!(retired.status, StatusCode::CREATED, "{retired:?}");
    assert_eq!(at_org.status, StatusCode::FORBIDDEN, "{at_org:?}");
    assert_eq!(at_org.code(), "permission.denied");
}

// Removing (B14).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_the_last_active_org_admin_stays_and_a_second_one_can_go(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_admin = f.builtin("org_admin").await;
    let (cookie, admin) = f.user().await;
    let own = f.assign(admin, org_admin, At::Org).await;

    let last = f.delete(&cookie, own).await;
    let (_, second) = f.user().await;
    let theirs = f.assign(second, org_admin, At::Org).await;
    let one_of_two = f.delete(&cookie, theirs).await;

    assert_eq!(last.status, StatusCode::CONFLICT, "{last:?}");
    assert_eq!(last.code(), "role_assignment.last_org_admin");
    assert!(f.exists(own).await);
    assert_eq!(one_of_two.status, StatusCode::NO_CONTENT, "{one_of_two:?}");
    assert!(!f.exists(theirs).await);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_an_assignment_from_an_outside_source_is_read_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&[]).await;
    let synced = new_id();
    sqlx::query(
        "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id,
                                       source, external_id)
         VALUES ($1, $2, $3, $4, 'organization', $2, 'sync', 'ext-1')",
    )
    .bind(synced)
    .bind(f.a.id)
    .bind(pat)
    .bind(role)
    .execute(&f.pool)
    .await
    .unwrap();

    let answer = f.delete(&admin, synced).await;

    assert_eq!(answer.status, StatusCode::CONFLICT, "{answer:?}");
    assert_eq!(answer.code(), "role_assignment.externally_managed");
    assert!(f.exists(synced).await);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn removing_needs_role_assign_and_the_roles_permissions_at_the_scope(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (_, pat) = f.user().await;
    let org_admin = f.builtin("org_admin").await;
    let account_admin = f.builtin("account_admin").await;
    let at_ground = f.assign(pat, account_admin, At::Account(f.a.ground)).await;
    let at_enroute = f.assign(pat, account_admin, At::Account(f.a.enroute)).await;
    let at_org = f.assign(pat, account_admin, At::Org).await;
    let admin_at_tower = f.assign(pat, org_admin, At::Account(f.a.tower)).await;
    let stronger = f
        .custom_role(&["service_account.manage", "account.view"])
        .await;
    let stronger_at_ground = f.assign(pat, stronger, At::Account(f.a.ground)).await;

    let hidden = f.delete(&tower_admin, at_enroute).await;
    let visible_org = f.delete(&tower_admin, at_org).await;
    // Both hold organization-wide permissions Tower's admin lacks.
    let admin_removed = f.delete(&tower_admin, admin_at_tower).await;
    let stronger_removed = f.delete(&tower_admin, stronger_at_ground).await;
    let removed = f.delete(&tower_admin, at_ground).await;

    assert_eq!(hidden.status, StatusCode::NOT_FOUND, "{hidden:?}");
    assert_eq!(hidden.code(), "role_assignment.not_found");
    assert_eq!(visible_org.status, StatusCode::FORBIDDEN, "{visible_org:?}");
    assert_eq!(visible_org.code(), "permission.denied");
    assert_eq!(
        admin_removed.status,
        StatusCode::FORBIDDEN,
        "{admin_removed:?}"
    );
    assert_eq!(admin_removed.code(), "permission.escalation");
    assert_eq!(
        stronger_removed.status,
        StatusCode::FORBIDDEN,
        "{stronger_removed:?}"
    );
    assert_eq!(stronger_removed.code(), "permission.escalation");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    assert!(f.exists(at_enroute).await && f.exists(at_org).await);
    assert!(f.exists(admin_at_tower).await && f.exists(stronger_at_ground).await);
    assert!(!f.exists(at_ground).await);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn removing_an_unknown_assignment_answers_404(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;

    let answer = f.delete(&admin, new_id()).await;

    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{answer:?}");
    assert_eq!(answer.code(), "role_assignment.not_found");
}

// Listing (T14).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_an_account_admin_lists_its_subtree_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, admin) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let own: Uuid = sqlx::query_scalar("SELECT id FROM role_assignments WHERE user_id = $1")
        .bind(admin)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let (_, pat) = f.user().await;
    let role = f.custom_role(&["account.view"]).await;
    let at_ground = f.assign(pat, role, At::Account(f.a.ground)).await;
    f.assign(pat, role, At::Account(f.a.enroute)).await;
    f.assign(pat, role, At::Account(f.a.root)).await;
    f.assign(pat, role, At::Org).await;

    let answer = f.list(&tower_admin, "").await;

    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.ids(), [own, at_ground]);
    assert_eq!(answer.body["next_cursor"], Value::Null);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_filters_by_user_and_scope_and_pages(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&["account.view"]).await;
    let at_tower = f.assign(pat, role, At::Account(f.a.tower)).await;
    let at_org = f.assign(pat, role, At::Org).await;
    let all = f.list(&admin, "").await.ids();

    let by_user = f.list(&admin, &format!("?user_id={pat}")).await;
    let tower = format!("?scope_type=account&scope_id={}", f.a.tower);
    let by_scope = f.list(&admin, &tower).await;
    let org = format!("?scope_type=organization&scope_id={}", f.a.id);
    let by_org = f.list(&admin, &org).await;
    let first = f.list(&admin, "?limit=2").await;
    let cursor = first.body["next_cursor"].as_str().expect("a next page");
    let second = f.list(&admin, &format!("?limit=2&cursor={cursor}")).await;

    assert_eq!(all.len(), 3, "the admin's own and pat's two: {all:?}");
    assert_eq!(by_user.ids(), [at_tower, at_org]);
    assert_eq!(by_scope.ids(), [at_tower]);
    assert_eq!(by_org.ids().len(), 2, "admin's and pat's");
    assert!(!by_org.ids().contains(&at_tower));
    assert_eq!(first.ids(), all[..2]);
    assert_eq!(second.ids(), all[2..]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_scope_filter_is_checked_like_a_target(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let query = |scope_type: &str, id: Uuid| format!("?scope_type={scope_type}&scope_id={id}");

    let ground = f.list(&tower_admin, &query("account", f.a.ground)).await;
    let enroute = f.list(&tower_admin, &query("account", f.a.enroute)).await;
    let other_org = f.list(&tower_admin, &query("account", f.b.tower)).await;
    let org = f.list(&tower_admin, &query("organization", f.a.id)).await;
    let half = f.list(&tower_admin, "?scope_type=account").await;

    assert_eq!(ground.status, StatusCode::OK, "{ground:?}");
    for hidden in [&enroute, &other_org] {
        assert_eq!(hidden.status, StatusCode::NOT_FOUND, "{hidden:?}");
        assert_eq!(hidden.code(), "account.not_found");
    }
    assert_eq!(org.status, StatusCode::FORBIDDEN, "{org:?}");
    assert_eq!(org.code(), "permission.denied");
    assert_eq!(half.status, StatusCode::BAD_REQUEST, "{half:?}");
    assert_eq!(half.code(), "request.malformed");
}

/// Waits until a session of this test's database waits on a row or transaction lock.
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

// Built-in roles this release does not define.

/// A built-in row a newer release wrote, which this binary does not define.
async fn future_builtin(f: &Fixture) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO roles (id, org_id, key, name, builtin, permissions)
         VALUES ($1, NULL, 'future_role', 'Future role', true, '{organization.manage}')",
    )
    .bind(id)
    .execute(&f.pool)
    .await
    .expect("insert a future built-in row");
    id
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_builtin_role_cannot_be_granted_or_removed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (_, pat) = f.user().await;
    let future = future_builtin(&f).await;
    let existing = f.assign(pat, future, At::Account(f.a.ground)).await;

    let granted = f
        .create(
            &tower_admin,
            pat,
            future,
            At::Account(f.a.tower).json(f.a.id),
        )
        .await;
    let removed = f.delete(&tower_admin, existing).await;

    for answer in [&granted, &removed] {
        assert_eq!(answer.status, StatusCode::CONFLICT, "{answer:?}");
        assert_eq!(answer.code(), "role.unknown_builtin");
    }
    assert!(f.exists(existing).await);
}

// The role's permissions cannot change between the escalation check and the insert.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_role_strengthened_during_a_grant_is_checked_as_strengthened(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&["account.view"]).await;
    let mut changing = f.pool.begin().await.unwrap();
    roles::lock_custom(&mut changing, scope(f.a.id), role)
        .await
        .unwrap();
    let granting = tokio::spawn({
        let app = f.app.clone();
        let body = json!({
            "user_id": pat, "role_id": role,
            "scope": { "type": "account", "id": f.a.tower },
        });
        let request = Request::post(ASSIGNMENTS)
            .header(TEST_ORG_HEADER, f.a.id.to_string())
            .header(header::COOKIE, tower_admin)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        async move { app.oneshot(request).await.unwrap().status() }
    });
    until_a_call_waits_on_a_lock(&f.pool).await;
    let patch = roles::RolePatch {
        name: None,
        permissions: Some(vec!["account.view".into(), "service_account.manage".into()]),
    };
    roles::update_custom(&mut *changing, scope(f.a.id), role, &patch, now())
        .await
        .unwrap();
    changing.commit().await.unwrap();

    let status = granting.await.expect("the grant ran");

    assert_eq!(status, StatusCode::FORBIDDEN);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM role_assignments WHERE role_id = $1")
        .bind(role)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

// Course-scope assignments, which only SQL can write in M0.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_scope_assignment_is_reached_only_from_the_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (tower_admin, _) = f.holder_of("account_admin", At::Account(f.a.tower)).await;
    let (admin, _) = f.holder_of("org_admin", At::Org).await;
    let (_, pat) = f.user().await;
    let role = f.custom_role(&[]).await;
    let course = new_id();
    sqlx::query(
        "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
         VALUES ($1, $2, $3, $4, 'course', $5)",
    )
    .bind(course)
    .bind(f.a.id)
    .bind(pat)
    .bind(role)
    .bind(course)
    .execute(&f.pool)
    .await
    .unwrap();

    let listed = f.list(&tower_admin, "").await;
    let by_tower_admin = f.delete(&tower_admin, course).await;
    let admin_list = f.list(&admin, &format!("?user_id={pat}")).await;
    let by_admin = f.delete(&admin, course).await;

    assert!(!listed.ids().contains(&course), "{listed:?}");
    assert_eq!(
        by_tower_admin.status,
        StatusCode::FORBIDDEN,
        "{by_tower_admin:?}"
    );
    assert_eq!(by_tower_admin.code(), "permission.denied");
    assert_eq!(admin_list.ids(), [course]);
    assert_eq!(
        admin_list.body["items"][0]["scope"],
        json!({ "type": "course", "id": course })
    );
    assert_eq!(by_admin.status, StatusCode::NO_CONTENT, "{by_admin:?}");
    assert!(!f.exists(course).await);
}
