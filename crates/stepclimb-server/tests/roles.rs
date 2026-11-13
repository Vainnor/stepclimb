// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission catalog and roles routes over the whole router
//! (docs/specs/m0/09-permissions-and-roles.md B7 to B10, T5 to T8, T14's roles half). The four
//! permission cases and the cross-organization case of each route are in `tests/permissions.rs`.
//!
//! One organization, so the production request-organization resolver serves it; Tower sits
//! under the root. `session::start` stands in for the sign-in specs. Needs `DATABASE_URL`
//! (`just up`).

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
    organizations, perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{AppState, router, session, web_origin::WebOrigin};
use tower::ServiceExt;

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

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
    tower: Uuid,
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
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        let tower = accounts::create(&pool, scope(org), root, "Tower", now())
            .await
            .expect("create Tower")
            .id;
        let clock = ManualClock::new(now());
        let app = router(AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
        });
        Self {
            pool,
            clock,
            app,
            org,
            tower,
        }
    }

    /// A new signed-in user with no role: their `Cookie` value and id.
    async fn user(&self) -> (String, Uuid) {
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
        let id = users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(self.org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    async fn custom_role(&self, key: &str, permissions: &[&str]) -> Uuid {
        let role = NewRole {
            key: key.into(),
            name: format!("Role {key}"),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(self.org), &role, now())
            .await
            .expect("create role")
            .id
    }

    async fn builtin(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    /// Assign `role` to `user` at `at`; the assignment's id.
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
        role_assignments::create(&self.pool, scope(self.org), &assignment, now())
            .await
            .expect("create assignment")
            .id
    }

    /// A signed-in user holding a new custom role with exactly `permissions` at `at`.
    async fn holder(&self, permissions: &[&str], at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = self
            .custom_role(&format!("r{}", new_id().simple()), permissions)
            .await;
        self.assign(user, role, at).await;
        cookie
    }

    async fn admin(&self) -> String {
        let (cookie, user) = self.user().await;
        self.assign(user, self.builtin("org_admin").await, At::Org)
            .await;
        cookie
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
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
}

fn role_uri(id: impl std::fmt::Display) -> String {
    format!("/api/v1/admin/roles/{id}")
}

const ROLES: &str = "/api/v1/admin/roles";

// The catalog.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_catalog_lists_every_permission_with_its_scope_types(pool: PgPool) {
    let f = Fixture::new(pool).await;
    // Held at an account only: `AnyScope` lets it read the catalog.
    let viewer = f.holder(&["role.view"], At::Account(f.tower)).await;
    let answer = f
        .send(Method::GET, "/api/v1/admin/permissions", &viewer, None)
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let listed = answer.body.as_array().expect("an array");
    assert_eq!(listed.len(), perm::ALL.len());
    for (item, p) in listed.iter().zip(perm::ALL) {
        assert_eq!(item["key"], p.key());
    }
    let find = |key: &str| {
        listed
            .iter()
            .find(|item| item["key"] == key)
            .expect("listed")["grantable"]
            .clone()
    };
    assert_eq!(find("role.manage"), json!(["organization"]));
    assert_eq!(find("role.assign"), json!(["organization", "account"]));
}

// Reading roles.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_puts_builtin_roles_first_then_custom_ones_by_key_across_pages(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let viewer = f.holder(&["role.view"], At::Account(f.tower)).await;
    f.custom_role("zulu_role", &[]).await;
    f.custom_role("alpha_role", &["account.view"]).await;
    let mut keys = Vec::new();
    let mut builtin = Vec::new();
    let mut uri = format!("{ROLES}?limit=3");
    loop {
        let answer = f.send(Method::GET, &uri, &viewer, None).await;
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
        let items = answer.body["items"].as_array().expect("items");
        assert!(items.len() <= 3);
        for item in items {
            keys.push(item["key"].as_str().unwrap().to_owned());
            builtin.push(item["builtin"].as_bool().unwrap());
        }
        match answer.body["next_cursor"].as_str() {
            Some(cursor) => uri = format!("{ROLES}?limit=3&cursor={cursor}"),
            None => break,
        }
    }
    let mut builtin_keys: Vec<_> = roles::BUILTIN.iter().map(|r| r.key.to_owned()).collect();
    builtin_keys.sort();
    // The viewer's own role, `r<uuid>`, sorts between alpha_role and zulu_role.
    assert_eq!(keys[..8], builtin_keys[..]);
    assert!(builtin[..8].iter().all(|b| *b));
    assert_eq!(keys[8], "alpha_role");
    assert!(keys[9].starts_with('r'));
    assert_eq!(keys[10], "zulu_role");
    assert_eq!(keys.len(), 11);
    assert!(!builtin[8..].iter().any(|b| *b));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_cursor_that_does_not_decode_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let viewer = f.holder(&["role.view"], At::Org).await;
    let answer = f
        .send(Method::GET, &format!("{ROLES}?cursor=!!"), &viewer, None)
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.code(), "pagination.invalid_cursor");
    // A cursor whose key holds NUL decodes but cannot bound the query.
    let nul = stepclimb_server::pagination::encode_cursor(&(true, "a\0")).unwrap();
    let answer = f
        .send(Method::GET, &format!("{ROLES}?cursor={nul}"), &viewer, None)
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.code(), "pagination.invalid_cursor");
}

/// T14's roles half: no `role.view` anywhere is 403.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_without_role_view_anywhere_the_list_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let other = f.holder(&["account.view", "role.assign"], At::Org).await;
    let answer = f.send(Method::GET, ROLES, &other, None).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(answer.code(), "permission.denied");
}

/// B6: a built-in role reports this binary's list, whatever its row holds.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_builtin_role_reads_its_permissions_from_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let viewer = f.holder(&["role.view"], At::Org).await;
    let id = f.builtin("account_admin").await;
    sqlx::query("UPDATE roles SET permissions = '{branding.manage}' WHERE id = $1")
        .bind(id)
        .execute(&f.pool)
        .await
        .expect("edit the row");
    let answer = f.send(Method::GET, &role_uri(id), &viewer, None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["key"], "account_admin");
    assert_eq!(answer.body["builtin"], true);
    assert_eq!(
        answer.body["permissions"],
        json!([
            "account.view",
            "account.manage",
            "user.view",
            "role.view",
            "role.assign"
        ])
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_or_malformed_role_id_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    for method in [Method::GET, Method::PATCH, Method::DELETE] {
        let body = (method == Method::PATCH).then(|| json!({ "name": "X" }));
        let answer = f
            .send(method.clone(), &role_uri(new_id()), &admin, body.clone())
            .await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{method}");
        assert_eq!(answer.code(), "role.not_found", "{method}");
        let answer = f
            .send(method.clone(), &role_uri("tower"), &admin, body)
            .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{method}");
        assert_eq!(answer.code(), "request.malformed", "{method}");
    }
}

// Creating a role (B8, T6).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_answers_201_with_the_role_and_its_location(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let body = json!({
        "key": "tower_mentor",
        "name": "  Tower mentor ",
        "permissions": ["user.view", "account.view"],
    });
    let answer = f.send(Method::POST, ROLES, &admin, Some(body)).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    let id = answer.body["id"].as_str().expect("an id").to_owned();
    assert_eq!(
        answer.headers[header::LOCATION].to_str().unwrap(),
        role_uri(&id)
    );
    assert_eq!(
        answer.body,
        json!({
            "id": id,
            "key": "tower_mentor",
            "name": "Tower mentor",
            "builtin": false,
            "permissions": ["user.view", "account.view"],
            "created_at": "2025-11-13T02:13:20.000000Z",
            "updated_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    let read = f.send(Method::GET, &role_uri(&id), &admin, None).await;
    assert_eq!(read.body, answer.body);
}

/// T6: a built-in key is taken; so is another custom role's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_builtin_or_existing_key_is_taken(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.custom_role("tower_mentor", &[]).await;
    for key in ["org_admin", "tower_mentor"] {
        let body = json!({ "key": key, "name": "Copy", "permissions": [] });
        let answer = f.send(Method::POST, ROLES, &admin, Some(body)).await;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{key}");
        assert_eq!(answer.code(), "role.key_taken", "{key}");
    }
}

/// T6: an unknown permission is a 422 at its index; each other rule names its member.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_body_that_breaks_a_rule_answers_422_at_the_member(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let too_long = "a".repeat(201);
    let cases = [
        (
            json!({ "key": "r", "name": "R", "permissions": ["nonexistent.perm"] }),
            "/permissions/0",
            "role.unknown_permission",
        ),
        (
            json!({ "key": "r", "name": "R", "permissions": ["user.view", "Role.View"] }),
            "/permissions/1",
            "role.unknown_permission",
        ),
        (
            json!({ "key": "r", "name": "R", "permissions": ["user.view", "role.view", "user.view"] }),
            "/permissions/2",
            "role.duplicate_permission",
        ),
        (
            json!({ "key": "Tower-Mentor", "name": "R", "permissions": [] }),
            "/key",
            "role.key_invalid",
        ),
        (
            json!({ "key": "r", "name": " \t", "permissions": [] }),
            "/name",
            "name.empty",
        ),
        (
            json!({ "key": "r", "name": too_long, "permissions": [] }),
            "/name",
            "name.too_long",
        ),
        (
            json!({ "key": "r", "name": "R\u{0}", "permissions": [] }),
            "/name",
            "text.contains_nul",
        ),
    ];
    for (body, field, code) in cases {
        let answer = f.send(Method::POST, ROLES, &admin, Some(body)).await;
        assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{code}");
        assert_eq!(answer.code(), "request.invalid", "{code}");
        assert_eq!(answer.errors(), [(field.to_owned(), code.to_owned())]);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM roles WHERE org_id = $1")
        .bind(f.org)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "no role was stored");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_with_an_unknown_or_missing_member_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let role = f.custom_role("tower_mentor", &[]).await;
    let cases = [
        (
            Method::POST,
            ROLES.to_owned(),
            json!({ "key": "r", "name": "R" }),
        ),
        (
            Method::POST,
            ROLES.to_owned(),
            json!({ "key": "r", "name": "R", "permissions": [], "builtin": true }),
        ),
        (Method::PATCH, role_uri(role), json!({ "key": "renamed" })),
    ];
    for (method, uri, body) in cases {
        let answer = f.send(method, &uri, &admin, Some(body.clone())).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(answer.code(), "request.malformed", "{body}");
    }
}

/// T6: `role.manage` alone cannot define a role with a permission its holder lacks.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_creating_a_role_stronger_than_the_caller_is_an_escalation(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let manager = f.holder(&["role.manage"], At::Org).await;
    let body = json!({ "key": "r", "name": "R", "permissions": ["user.manage"] });
    let answer = f.send(Method::POST, ROLES, &manager, Some(body)).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{answer:?}");
    assert_eq!(answer.code(), "permission.escalation");

    let body = json!({ "key": "r", "name": "R", "permissions": ["role.manage"] });
    let answer = f.send(Method::POST, ROLES, &manager, Some(body)).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
}

/// The rule is checked at organization scope: holding a permission at an account does not let
/// the caller put it in an organization-wide role.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_permission_held_only_at_an_account_cannot_be_put_in_a_role(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, user) = f.user().await;
    let org_role = f.custom_role("org_part", &["role.manage"]).await;
    let tower_role = f.custom_role("tower_part", &["account.manage"]).await;
    f.assign(user, org_role, At::Org).await;
    f.assign(user, tower_role, At::Account(f.tower)).await;
    let body = json!({ "key": "r", "name": "R", "permissions": ["account.manage"] });
    let answer = f.send(Method::POST, ROLES, &manager, Some(body)).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{answer:?}");
    assert_eq!(answer.code(), "permission.escalation");
}

// Changing a role (B7, B9).

/// T5.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_builtin_roles_cannot_be_changed_or_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    for key in ["org_admin", "student"] {
        let id = f.builtin(key).await;
        let patch = json!({ "name": "Renamed" });
        let answer = f
            .send(Method::PATCH, &role_uri(id), &admin, Some(patch))
            .await;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{key}");
        assert_eq!(answer.code(), "role.builtin_immutable", "{key}");
        let answer = f.send(Method::DELETE, &role_uri(id), &admin, None).await;
        assert_eq!(answer.status, StatusCode::CONFLICT, "{key}");
        assert_eq!(answer.code(), "role.builtin_immutable", "{key}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn patch_changes_the_named_members_and_sets_updated_at(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let role = f.custom_role("tower_mentor", &["account.view"]).await;
    f.clock.advance(Duration::from_secs(60));
    let answer = f
        .send(
            Method::PATCH,
            &role_uri(role),
            &admin,
            Some(json!({ "name": "Mentor" })),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["name"], "Mentor");
    assert_eq!(answer.body["permissions"], json!(["account.view"]));
    assert_eq!(answer.body["created_at"], "2025-11-13T02:13:20.000000Z");
    assert_eq!(answer.body["updated_at"], "2025-11-13T02:14:20.000000Z");
    let answer = f
        .send(
            Method::PATCH,
            &role_uri(role),
            &admin,
            Some(json!({ "permissions": ["user.view"] })),
        )
        .await;
    assert_eq!(answer.body["name"], "Mentor");
    assert_eq!(answer.body["permissions"], json!(["user.view"]));
    let answer = f
        .send(
            Method::PATCH,
            &role_uri(role),
            &admin,
            Some(json!({ "permissions": ["user.view", "nope"] })),
        )
        .await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer.errors(),
        [(
            "/permissions/1".to_owned(),
            "role.unknown_permission".to_owned()
        )]
    );
    // A repeated key, however long the list: refused at the first repeat, nothing stored.
    let mut repeated = vec!["user.view"; 100_000];
    repeated[0] = "account.view";
    let answer = f
        .send(
            Method::PATCH,
            &role_uri(role),
            &admin,
            Some(json!({ "permissions": repeated })),
        )
        .await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer.errors(),
        [(
            "/permissions/2".to_owned(),
            "role.duplicate_permission".to_owned()
        )]
    );
    let stored: Vec<String> = sqlx::query_scalar("SELECT permissions FROM roles WHERE id = $1")
        .bind(role)
        .fetch_one(&f.pool)
        .await
        .expect("the role");
    assert_eq!(stored, ["user.view"]);
}

/// B9 with the escalation rule: a change may not add or drop a permission the caller does not
/// hold at organization scope; a rename, or a list that keeps such a permission, may.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn patch_refuses_adding_or_dropping_a_permission_the_caller_lacks(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let manager = f.holder(&["role.manage", "account.view"], At::Org).await;
    let strong = f
        .custom_role("strong", &["user.manage", "account.view"])
        .await;
    let weak = f.custom_role("weak", &[]).await;
    let refused = [
        (strong, json!({ "permissions": ["account.view"] })),
        (strong, json!({ "permissions": [] })),
        (weak, json!({ "permissions": ["user.manage"] })),
        (
            weak,
            json!({ "name": "W", "permissions": ["account.view", "user.manage"] }),
        ),
    ];
    for (role, body) in refused {
        let answer = f
            .send(Method::PATCH, &role_uri(role), &manager, Some(body.clone()))
            .await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{body}: {answer:?}");
        assert_eq!(answer.code(), "permission.escalation", "{body}");
    }
    let unchanged: Vec<String> = sqlx::query_scalar("SELECT permissions FROM roles WHERE id = $1")
        .bind(strong)
        .fetch_one(&f.pool)
        .await
        .map(|p: Vec<String>| p)
        .unwrap();
    assert_eq!(unchanged, ["user.manage", "account.view"]);

    let allowed = [
        (strong, json!({ "name": "Renamed" })),
        (
            strong,
            json!({ "permissions": ["user.manage", "account.view"] }),
        ),
        (weak, json!({ "permissions": ["account.view"] })),
        (weak, json!({ "permissions": [] })),
    ];
    for (role, body) in allowed {
        let answer = f
            .send(Method::PATCH, &role_uri(role), &manager, Some(body.clone()))
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{body}: {answer:?}");
    }
}

/// T7: a holder's next request sees the change; nothing is cached.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_holder_loses_a_permission_on_their_next_request(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let (cookie, user) = f.user().await;
    let role = f.custom_role("viewer", &["role.view"]).await;
    f.assign(user, role, At::Org).await;
    assert_eq!(
        f.send(Method::GET, ROLES, &cookie, None).await.status,
        StatusCode::OK
    );
    let answer = f
        .send(
            Method::PATCH,
            &role_uri(role),
            &admin,
            Some(json!({ "permissions": [] })),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let answer = f.send(Method::GET, ROLES, &cookie, None).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(answer.code(), "permission.denied");
}

// Deleting a role (B10).

/// T8.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_a_role_with_an_assignment_is_in_use_until_it_goes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let (_, user) = f.user().await;
    let role = f.custom_role("tower_mentor", &["account.view"]).await;
    let assignment = f.assign(user, role, At::Account(f.tower)).await;
    let answer = f.send(Method::DELETE, &role_uri(role), &admin, None).await;
    assert_eq!(answer.status, StatusCode::CONFLICT, "{answer:?}");
    assert_eq!(answer.code(), "role.in_use");
    role_assignments::delete(&f.pool, scope(f.org), assignment)
        .await
        .expect("remove the assignment");
    let answer = f.send(Method::DELETE, &role_uri(role), &admin, None).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    assert_eq!(answer.body, Value::Null);
    let answer = f.send(Method::GET, &role_uri(role), &admin, None).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(answer.code(), "role.not_found");
}
