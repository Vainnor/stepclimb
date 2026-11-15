// SPDX-License-Identifier: AGPL-3.0-or-later
//! The account routes over the router (docs/specs/m0/02-organizations-and-accounts.md B18 to
//! B25; T1 to T3 for the account routes, T12 to T19). The permission suite's generated cases for
//! each route are also in `tests/permissions.rs`.
//!
//! Every request names its organization with the `test-support` header, since most tests hold
//! two organizations. `session::start` stands in for the sign-in specs. Needs `DATABASE_URL`
//! (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
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

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

mod common;

const ACCOUNTS: &str = "/api/v1/accounts";

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

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
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

    #[track_caller]
    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!((self.status, self.code()), (status, code), "{self:?}");
        assert_eq!(
            self.headers.get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
            Some(&b"application/problem+json"[..]),
            "{self:?}"
        );
    }

    #[track_caller]
    fn ok(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }

    /// The ids of a page's items, in order.
    fn ids(&self) -> Vec<Uuid> {
        self.body["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|item| {
                item["id"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .expect("an id")
            })
            .collect()
    }

    /// The `(field, code)` pairs of a 422's `errors`.
    fn errors(&self) -> Vec<(String, String)> {
        self.body["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                (
                    e["field"].as_str().expect("a field").into(),
                    e["code"].as_str().expect("a code").into(),
                )
            })
            .collect()
    }
}

/// An organization with the tree of the spec's tests: Tower and Enroute under the root, Ground
/// under Tower.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    enroute: Uuid,
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    a: Tree,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let a = tree(&pool, "org-a").await;
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            pool,
            clock,
            a,
        }
    }

    async fn tree(&self, slug: &str) -> Tree {
        tree(&self.pool, slug).await
    }

    async fn account(&self, org: Uuid, parent: Uuid, name: &str) -> Uuid {
        accounts::create(&self.pool, scope(org), parent, name, now(), Actor::Instance)
            .await
            .expect("create account")
            .id
    }

    async fn read(&self, org: Uuid, id: Uuid) -> Option<accounts::Account> {
        accounts::get(&self.pool, scope(org), id)
            .await
            .expect("read account")
    }

    /// A new signed-in user of `org`: their `Cookie` value and id.
    async fn user(&self, org: Uuid) -> (String, Uuid) {
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

    async fn assign(&self, org: Uuid, user: Uuid, role: Uuid, at: Scope) {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
            .await
            .expect("create assignment");
    }

    async fn builtin_role(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    /// A signed-in user holding the built-in role `key` at each scope of `at`.
    async fn holder(&self, org: Uuid, key: &str, at: &[Scope]) -> String {
        let (cookie, user) = self.user(org).await;
        let role = self.builtin_role(key).await;
        for scope in at {
            self.assign(org, user, role, *scope).await;
        }
        cookie
    }

    /// A signed-in user holding a custom role with exactly `keys` at `at`.
    async fn custom_holder(&self, org: Uuid, keys: &[&str], at: Scope) -> String {
        let (cookie, user) = self.user(org).await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(org, user, role, at).await;
        cookie
    }

    async fn org_admin(&self, org: Uuid) -> String {
        self.holder(org, "org_admin", &[Scope::Organization]).await
    }

    fn request(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: &str,
        body: Option<&str>,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        request
            .body(Body::from(body.unwrap_or_default().to_owned()))
            .expect("a request")
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: &str,
        body: Option<Value>,
    ) -> Answer {
        let body = body.map(|b| b.to_string());
        answer(
            &self.app,
            self.request(method, uri, org, cookie, body.as_deref()),
        )
        .await
    }

    async fn get(&self, uri: &str, org: Uuid, cookie: &str) -> Answer {
        self.send(Method::GET, uri, org, cookie, None).await
    }

    async fn create(&self, org: Uuid, cookie: &str, parent: Uuid, name: &str) -> Answer {
        let body = json!({ "parent_id": parent, "name": name });
        self.send(Method::POST, ACCOUNTS, org, cookie, Some(body))
            .await
    }

    async fn rename(&self, org: Uuid, cookie: &str, id: Uuid, name: &str) -> Answer {
        let body = json!({ "name": name });
        self.send(Method::PATCH, &one(id), org, cookie, Some(body))
            .await
    }

    async fn move_to(&self, org: Uuid, cookie: &str, id: Uuid, parent: Uuid) -> Answer {
        let body = json!({ "parent_id": parent });
        self.send(
            Method::POST,
            &format!("{}/move", one(id)),
            org,
            cookie,
            Some(body),
        )
        .await
    }

    async fn delete(&self, org: Uuid, cookie: &str, id: Uuid) -> Answer {
        self.send(Method::DELETE, &one(id), org, cookie, None).await
    }

    /// Every row of `org`'s accounts and role assignments, as text: a snapshot that shows
    /// whether a call changed anything.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(a)::text FROM accounts a WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(r)::text FROM role_assignments r WHERE org_id = $1
             ORDER BY 1",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read rows")
    }
}

fn one(id: Uuid) -> String {
    format!("{ACCOUNTS}/{id}")
}

async fn tree(pool: &PgPool, slug: &str) -> Tree {
    let org = organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let create = |parent, name: &'static str| async move {
        accounts::create(pool, scope(org), parent, name, now(), Actor::Instance)
            .await
            .expect("create account")
            .id
    };
    let tower = create(root, "Tower").await;
    let ground = create(tower, "Ground").await;
    let enroute = create(root, "Enroute").await;
    Tree {
        org,
        root,
        tower,
        ground,
        enroute,
    }
}

async fn answer(app: &Router, request: Request<Body>) -> Answer {
    let response = app
        .clone()
        .oneshot(request)
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
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    Answer {
        status,
        headers,
        body,
    }
}

// T15, T17: an account admin of Tower.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_an_account_admin_of_tower_works_inside_its_subtree_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;

    let under_tower = f.create(t.org, &admin, t.tower, "Approach").await;
    let approach = under_tower.ok(StatusCode::CREATED)["id"].clone();
    let approach: Uuid = approach.as_str().unwrap().parse().unwrap();
    f.create(t.org, &admin, t.ground, "Ramp")
        .await
        .ok(StatusCode::CREATED);
    let renamed = f.rename(t.org, &admin, t.tower, "Tower Cab").await;
    assert_eq!(renamed.ok(StatusCode::OK)["name"], "Tower Cab");
    let moved = f.move_to(t.org, &admin, t.ground, approach).await;
    assert_eq!(moved.ok(StatusCode::OK)["parent_id"], approach.to_string());
    assert_eq!(moved.body["depth"], 3);
    assert_schema("Account", &moved.body);

    // The checks for moving and deleting Tower are at the root, which it cannot see.
    f.move_to(t.org, &admin, t.tower, t.enroute)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    f.delete(t.org, &admin, t.tower)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    // Ground's parent is in reach; the destination Enroute is not.
    f.move_to(t.org, &admin, t.ground, t.enroute)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    for hidden in [t.enroute, t.root] {
        f.get(&one(hidden), t.org, &admin)
            .await
            .expect(StatusCode::NOT_FOUND, "account.not_found");
    }
    assert_eq!(
        f.read(t.org, t.ground).await.unwrap().parent_id,
        Some(approach)
    );
    assert_eq!(
        f.read(t.org, t.tower).await.unwrap().parent_id,
        Some(t.root)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_an_account_admin_reads_its_account_with_the_hidden_parents_id(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let answer = f.get(&one(t.tower), t.org, &admin).await;
    let body = answer.ok(StatusCode::OK);
    assert_eq!(
        *body,
        json!({
            "id": t.tower, "parent_id": t.root, "name": "Tower", "depth": 1,
            "has_children": true, "created_at": "2025-11-13T02:13:20.000000Z",
            "updated_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    assert_schema("Account", body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_visible_destination_without_account_manage_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let (cookie, user) = f.user(t.org).await;
    let admin = f.builtin_role("account_admin").await;
    f.assign(t.org, user, admin, Scope::Account(t.tower)).await;
    let viewer = NewRole {
        key: "viewer".into(),
        name: "Viewer".into(),
        permissions: vec!["account.view".into()],
    };
    let viewer = roles::create_custom(&f.pool, scope(t.org), &viewer, now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    f.assign(t.org, user, viewer, Scope::Account(t.enroute))
        .await;
    // Enroute is visible to the caller, but not a subtree it manages (B23).
    f.get(&one(t.enroute), t.org, &cookie)
        .await
        .ok(StatusCode::OK);
    f.move_to(t.org, &cookie, t.ground, t.enroute)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(
        f.read(t.org, t.ground).await.unwrap().parent_id,
        Some(t.tower)
    );
}

// T16: the topmost accounts.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_without_a_parent_the_list_holds_the_topmost_visible_accounts(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let cases = [
        (vec![Scope::Organization], "org_admin", vec![t.root]),
        (
            vec![Scope::Account(t.tower), Scope::Account(t.enroute)],
            "account_admin",
            vec![t.enroute, t.tower],
        ),
        (
            vec![Scope::Account(t.tower), Scope::Account(t.ground)],
            "account_admin",
            vec![t.tower],
        ),
    ];
    for (at, role, expected) in cases {
        let cookie = f.holder(t.org, role, &at).await;
        let answer = f.get(ACCOUNTS, t.org, &cookie).await;
        answer.ok(StatusCode::OK);
        assert_eq!(answer.ids(), expected, "{at:?}");
        assert_eq!(answer.body["next_cursor"], Value::Null);
        for item in answer.body["items"].as_array().unwrap() {
            assert_schema("Account", item);
        }
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn listing_children_needs_account_view_over_the_parent(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let children = f
        .get(&format!("{ACCOUNTS}?parent_id={}", t.tower), t.org, &admin)
        .await;
    children.ok(StatusCode::OK);
    assert_eq!(children.ids(), [t.ground]);
    for hidden in [t.root, t.enroute, new_id()] {
        f.get(&format!("{ACCOUNTS}?parent_id={hidden}"), t.org, &admin)
            .await
            .expect(StatusCode::NOT_FOUND, "account.not_found");
    }
    // Visible through another permission, without account.view: 403.
    let roles_only = f
        .custom_holder(t.org, &["role.view"], Scope::Account(t.tower))
        .await;
    f.get(
        &format!("{ACCOUNTS}?parent_id={}", t.tower),
        t.org,
        &roles_only,
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.get(&format!("{ACCOUNTS}?parent_id=not-a-uuid"), t.org, &admin)
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

// T2, T3: pages.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_seven_children_page_three_at_a_time_in_name_order(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let parent = f.account(t.org, t.root, "Approach").await;
    let mut created = Vec::new();
    for name in ["G", "C", "E", "A", "F", "B", "C"] {
        created.push((name, f.account(t.org, parent, name).await));
    }
    created.sort();
    let expected: Vec<Uuid> = created.iter().map(|(_, id)| *id).collect();

    let mut seen = Vec::new();
    let mut sizes = Vec::new();
    let mut uri = format!("{ACCOUNTS}?parent_id={parent}&limit=3");
    loop {
        let page = f.get(&uri, t.org, &admin).await;
        page.ok(StatusCode::OK);
        sizes.push(page.ids().len());
        seen.extend(page.ids());
        match page.body["next_cursor"].as_str() {
            Some(cursor) => uri = format!("{ACCOUNTS}?parent_id={parent}&limit=3&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(sizes, [3, 3, 1]);
    assert_eq!(seen, expected);

    f.get(
        &format!("{ACCOUNTS}?parent_id={parent}&cursor=!!"),
        t.org,
        &admin,
    )
    .await
    .expect(StatusCode::BAD_REQUEST, "pagination.invalid_cursor");
    f.get(&format!("{ACCOUNTS}?limit=0"), t.org, &admin)
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_cursor_from_org_a_replayed_in_org_b_lists_only_org_b(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, f.tree("org-b").await);
    let admin_a = f.org_admin(a.org).await;
    let admin_b = f.org_admin(b.org).await;
    let first = f
        .get(
            &format!("{ACCOUNTS}?parent_id={}&limit=1", a.root),
            a.org,
            &admin_a,
        )
        .await;
    let cursor = first.body["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_owned();
    let replayed = f
        .get(
            &format!("{ACCOUNTS}?parent_id={}&cursor={cursor}", b.root),
            b.org,
            &admin_b,
        )
        .await;
    replayed.ok(StatusCode::OK);
    // A's first page ended at its Enroute; only B's accounts come after that key.
    let ids = replayed.ids();
    assert!(ids.contains(&b.tower), "{replayed:?}");
    assert!(
        ids.iter().all(|id| [b.enroute, b.tower].contains(id)),
        "{replayed:?}"
    );
    let top = f
        .get(&format!("{ACCOUNTS}?cursor={cursor}"), b.org, &admin_b)
        .await;
    top.ok(StatusCode::OK);
    assert!(top.ids().iter().all(|id| *id == b.root), "{top:?}");
}

// T1: bad bodies.

/// A method, URI and body, and the status, code and `errors[].code` at `/name` it answers.
type BodyCase = (
    Method,
    String,
    String,
    StatusCode,
    &'static str,
    Option<&'static str>,
);

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_bad_account_bodies_answer_problem_details(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let before = f.rows(t.org).await;
    let parent = t.root;
    let cases: [BodyCase; 9] = [
        (
            Method::POST,
            ACCOUNTS.into(),
            r#"{"parent_id": "#.into(),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            Method::POST,
            ACCOUNTS.into(),
            format!(r#"{{"parent_id": "{parent}", "name": "  "}}"#),
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("name.empty"),
        ),
        (
            Method::POST,
            ACCOUNTS.into(),
            format!(
                r#"{{"parent_id": "{parent}", "name": "{}"}}"#,
                "a".repeat(201)
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("name.too_long"),
        ),
        (
            Method::POST,
            ACCOUNTS.into(),
            r#"{"parent_id": "x", "name": "A"}"#.into(),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            Method::POST,
            ACCOUNTS.into(),
            format!(r#"{{"parent_id": "{parent}", "name": "A", "path": "x"}}"#),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            Method::PATCH,
            one(t.tower),
            r#"{"name": null}"#.into(),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            Method::PATCH,
            one(t.tower),
            r#"{"name": "a\u0000b"}"#.into(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("text.contains_nul"),
        ),
        (
            Method::POST,
            format!("{}/move", one(t.tower)),
            r#"{}"#.into(),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            Method::PATCH,
            format!("{ACCOUNTS}/not-a-uuid"),
            r#"{"name": "A"}"#.into(),
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
    ];
    for (method, uri, body, status, code, field) in cases {
        let request = f.request(method.clone(), &uri, t.org, &admin, Some(&body));
        let answer = answer(&f.app, request).await;
        answer.expect(status, code);
        let expected: Vec<(String, String)> = field
            .map(|c| ("/name".to_owned(), c.to_owned()))
            .into_iter()
            .collect();
        assert_eq!(answer.errors(), expected, "{method} {uri} {body}");
    }
    let wrong_type = Request::post(ACCOUNTS)
        .header(TEST_ORG_HEADER, t.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, &admin)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(
            json!({ "parent_id": parent, "name": "A" }).to_string(),
        ))
        .unwrap();
    answer(&f.app, wrong_type).await.expect(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "request.unsupported_media_type",
    );
    assert_eq!(f.rows(t.org).await, before, "nothing was written");
}

// Create, rename.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_create_answers_201_with_its_location_and_the_account(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    f.clock.advance(Duration::from_secs(60));
    let created = f.create(t.org, &admin, t.ground, "  Ramp  ").await;
    let body = created.ok(StatusCode::CREATED).clone();
    assert_schema("Account", &body);
    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        created.headers.get(header::LOCATION).unwrap(),
        &format!("/api/v1/accounts/{id}")
    );
    assert_eq!(body["name"], "Ramp");
    assert_eq!(body["parent_id"], t.ground.to_string());
    assert_eq!(body["depth"], 3);
    assert_eq!(body["has_children"], false);
    assert_eq!(body["created_at"], "2025-11-13T02:14:20.000000Z");
    let read = f.get(&one(id), t.org, &admin).await;
    assert_eq!(read.ok(StatusCode::OK), &body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_create_more_than_16_levels_below_the_root_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let mut parent = t.root;
    for level in 1..=accounts::MAX_DEPTH {
        parent = f.account(t.org, parent, &format!("Level {level}")).await;
    }
    let before = f.rows(t.org).await;
    f.create(t.org, &admin, parent, "Too deep")
        .await
        .expect(StatusCode::CONFLICT, "account.too_deep");
    assert_eq!(f.rows(t.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rename_without_a_name_changes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    f.clock.advance(Duration::from_secs(60));
    let before = f.rows(t.org).await;
    let answer = f
        .send(Method::PATCH, &one(t.tower), t.org, &admin, Some(json!({})))
        .await;
    assert_eq!(answer.ok(StatusCode::OK)["name"], "Tower");
    assert_eq!(f.rows(t.org).await, before);
    let renamed = f.rename(t.org, &admin, t.tower, "Cab").await;
    assert_eq!(
        renamed.ok(StatusCode::OK)["updated_at"],
        "2025-11-13T02:14:20.000000Z"
    );
}

// T12, T13: moves.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_a_move_rewrites_the_subtree_and_a_move_to_the_same_parent_changes_nothing(
    pool: PgPool,
) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    f.clock.advance(Duration::from_secs(60));
    let before = f.rows(t.org).await;
    let same = f.move_to(t.org, &admin, t.ground, t.tower).await;
    assert_eq!(
        same.ok(StatusCode::OK)["updated_at"],
        "2025-11-13T02:13:20.000000Z"
    );
    assert_eq!(
        f.rows(t.org).await,
        before,
        "a move to the current parent writes nothing"
    );

    let moved = f.move_to(t.org, &admin, t.tower, t.enroute).await;
    let body = moved.ok(StatusCode::OK);
    assert_eq!(body["parent_id"], t.enroute.to_string());
    assert_eq!(body["depth"], 2);
    let ground = f.read(t.org, t.ground).await.unwrap();
    assert_eq!(ground.depth, 3);
    assert_eq!(ground.updated_at, now() + Duration::from_secs(60));
    let path = accounts::ancestors_path(&f.pool, scope(t.org), t.ground)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(path, [t.root, t.enroute, t.tower, t.ground]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_the_root_and_moves_into_the_own_subtree_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    // An account admin of the root holds no scope above it: the root is out of its reach.
    let root_admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.root)])
        .await;
    let before = f.rows(t.org).await;
    f.move_to(t.org, &admin, t.root, t.tower)
        .await
        .expect(StatusCode::CONFLICT, "account.root_immutable");
    f.delete(t.org, &admin, t.root)
        .await
        .expect(StatusCode::CONFLICT, "account.root_immutable");
    for under in [t.ground, t.tower] {
        f.move_to(t.org, &admin, t.tower, under)
            .await
            .expect(StatusCode::CONFLICT, "account.move_into_descendant");
    }
    f.move_to(t.org, &root_admin, t.root, t.tower)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    f.delete(t.org, &root_admin, t.root)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(f.rows(t.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_deeper_than_the_limit_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let mut parent = t.root;
    for level in 1..accounts::MAX_DEPTH {
        parent = f.account(t.org, parent, &format!("Level {level}")).await;
    }
    // Tower lands at 16 and Ground at 17.
    let before = f.rows(t.org).await;
    f.move_to(t.org, &admin, t.tower, parent)
        .await
        .expect(StatusCode::CONFLICT, "account.too_deep");
    assert_eq!(f.rows(t.org).await, before);
}

// T14: delete.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_delete_refuses_children_and_assignments_and_removes_a_leaf(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let (_, user) = f.user(t.org).await;
    let role = f.builtin_role("account_admin").await;
    f.assign(t.org, user, role, Scope::Account(t.ground)).await;
    f.assign(t.org, user, role, Scope::Account(t.enroute)).await;
    let before = f.rows(t.org).await;

    f.delete(t.org, &admin, t.tower)
        .await
        .expect(StatusCode::CONFLICT, "account.has_children");
    f.delete(t.org, &admin, t.ground)
        .await
        .expect(StatusCode::CONFLICT, "account.has_assignments");
    assert_eq!(
        f.rows(t.org).await,
        before,
        "a refused delete changes nothing"
    );

    sqlx::query("DELETE FROM role_assignments WHERE org_id = $1 AND scope_id = $2")
        .bind(t.org)
        .bind(t.ground)
        .execute(&f.pool)
        .await
        .unwrap();
    let deleted = f.delete(t.org, &admin, t.ground).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    assert_eq!(deleted.body, Value::Null);

    assert!(f.read(t.org, t.ground).await.is_none());
    let scopes: Vec<Uuid> = sqlx::query_scalar(
        "SELECT scope_id FROM role_assignments WHERE org_id = $1 AND user_id = $2",
    )
    .bind(t.org)
    .bind(user)
    .fetch_all(&f.pool)
    .await
    .unwrap();
    assert_eq!(scopes, [t.enroute], "assignments elsewhere are kept");
    f.get(&one(t.ground), t.org, &admin)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert!(!f.read(t.org, t.tower).await.unwrap().has_children);
}

// B23: a move cannot widen the caller's own permissions.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_that_would_widen_the_callers_permissions_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    // account.manage at organization scope, and Account admin (with role.assign) at Enroute.
    let (cookie, user) = f.user(t.org).await;
    let manage = NewRole {
        key: "org_account_manager".into(),
        name: "Account manager".into(),
        permissions: vec!["account.manage".into()],
    };
    let manage = roles::create_custom(&f.pool, scope(t.org), &manage, now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    f.assign(t.org, user, manage, Scope::Organization).await;
    let admin = f.builtin_role("account_admin").await;
    f.assign(t.org, user, admin, Scope::Account(t.enroute))
        .await;
    let before = f.rows(t.org).await;

    // Under Enroute, Tower would fall under the caller's Account admin grant.
    f.move_to(t.org, &cookie, t.tower, t.enroute)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.rows(t.org).await, before);

    // A destination that adds nothing over Tower is allowed: the root holds only the
    // organization grant, which already contains Tower.
    let side = f.account(t.org, t.root, "Side").await;
    let moved = f.move_to(t.org, &cookie, t.ground, side).await;
    assert_eq!(moved.ok(StatusCode::OK)["parent_id"], side.to_string());
    // Out of Enroute, the caller loses rights over the account: no escalation.
    let inside = f.account(t.org, t.enroute, "Inside").await;
    f.move_to(t.org, &cookie, inside, t.root)
        .await
        .ok(StatusCode::OK);
}

// T18: the four cases, by hand.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_the_four_cases_on_every_account_route(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    // Anonymous.
    for (method, uri) in [
        (Method::GET, ACCOUNTS.to_owned()),
        (Method::POST, ACCOUNTS.to_owned()),
        (Method::GET, one(t.ground)),
        (Method::PATCH, one(t.ground)),
        (Method::POST, format!("{}/move", one(t.ground))),
        (Method::DELETE, one(t.ground)),
    ] {
        let request = f.request(method.clone(), &uri, t.org, "", Some("{}"));
        answer(&f.app, request)
            .await
            .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    }
    // account.view only, at Tower: 403 on every write to Ground.
    let viewer = f
        .custom_holder(t.org, &["account.view"], Scope::Account(t.tower))
        .await;
    f.rename(t.org, &viewer, t.ground, "X")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.move_to(t.org, &viewer, t.ground, t.tower)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.delete(t.org, &viewer, t.ground)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.create(t.org, &viewer, t.ground, "X")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    // An account admin of Enroute: 404 on every Tower route.
    let enroute = f
        .holder(t.org, "account_admin", &[Scope::Account(t.enroute)])
        .await;
    let before = f.rows(t.org).await;
    for answer in [
        f.get(&one(t.tower), t.org, &enroute).await,
        f.get(
            &format!("{ACCOUNTS}?parent_id={}", t.tower),
            t.org,
            &enroute,
        )
        .await,
        f.create(t.org, &enroute, t.tower, "X").await,
        f.rename(t.org, &enroute, t.tower, "X").await,
        f.move_to(t.org, &enroute, t.tower, t.enroute).await,
        f.move_to(t.org, &enroute, t.ground, t.enroute).await,
        f.delete(t.org, &enroute, t.tower).await,
        f.delete(t.org, &enroute, t.ground).await,
    ] {
        answer.expect(StatusCode::NOT_FOUND, "account.not_found");
    }
    assert_eq!(f.rows(t.org).await, before);
    // An account admin of Tower: 2xx.
    let tower = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    f.get(&one(t.tower), t.org, &tower).await.ok(StatusCode::OK);
    f.rename(t.org, &tower, t.ground, "Ground 2")
        .await
        .ok(StatusCode::OK);
    f.move_to(t.org, &tower, t.ground, t.tower)
        .await
        .ok(StatusCode::OK);
    let ground = f.delete(t.org, &tower, t.ground).await;
    assert_eq!(ground.status, StatusCode::NO_CONTENT, "{ground:?}");
}

// T19: cross-organization.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19_org_bs_ids_are_not_found_for_an_admin_of_org_a(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, f.tree("org-b").await);
    let admin = f.org_admin(a.org).await;
    let before_b = f.rows(b.org).await;
    let before_a = f.rows(a.org).await;
    for answer in [
        f.get(&one(b.tower), a.org, &admin).await,
        f.get(&format!("{ACCOUNTS}?parent_id={}", b.root), a.org, &admin)
            .await,
        f.create(a.org, &admin, b.tower, "X").await,
        f.rename(a.org, &admin, b.tower, "X").await,
        f.move_to(a.org, &admin, b.ground, b.enroute).await,
        f.move_to(a.org, &admin, a.ground, b.enroute).await,
        f.move_to(a.org, &admin, b.ground, a.enroute).await,
        f.delete(a.org, &admin, b.ground).await,
    ] {
        answer.expect(StatusCode::NOT_FOUND, "account.not_found");
        let text = answer.body.to_string();
        assert!(!text.contains(&b.root.to_string()), "{text}");
    }
    assert_eq!(f.rows(b.org).await, before_b, "org B's tree is unchanged");
    assert_eq!(f.rows(a.org).await, before_a, "org A's tree is unchanged");
}

// The checks under the tree lock.

/// Waits until a session of this test's database waits on a lock (the tree lock or a row
/// lock), so the test knows the request has read past its `Require` check and is blocked.
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

/// Sends `request` while another transaction holds the tree lock and has moved `moved` under
/// `to` without committing, so the request's `Require` check reads the tree before the move and
/// its handler waits for the tree lock or the moved rows; the move commits once the request
/// waits. The request's answer.
async fn racing_a_move(f: &Fixture, moved: Uuid, to: Uuid, request: Request<Body>) -> Answer {
    let org = f.a.org;
    let mut tx = f.pool.begin().await.expect("begin");
    accounts::lock_tree(&mut tx, scope(org))
        .await
        .expect("take the tree lock");
    accounts::move_to(&mut *tx, scope(org), moved, to, now(), Actor::Instance)
        .await
        .expect("the competing move");
    let app = f.app.clone();
    let pending = tokio::spawn(async move { answer(&app, request).await });
    until_a_call_waits_on_a_lock(&f.pool).await;
    tx.commit().await.expect("commit the competing move");
    pending.await.expect("the request ran")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_of_an_account_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let destination = f.account(t.org, t.tower, "Destination").await;
    let body = json!({ "parent_id": destination }).to_string();
    let uri = format!("{}/move", one(t.ground));
    let request = f.request(Method::POST, &uri, t.org, &admin, Some(&body));
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(
        f.read(t.org, t.ground).await.unwrap().parent_id,
        Some(t.enroute)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_to_a_destination_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let destination = f.account(t.org, t.tower, "Destination").await;
    let body = json!({ "parent_id": destination }).to_string();
    let uri = format!("{}/move", one(t.ground));
    let request = f.request(Method::POST, &uri, t.org, &admin, Some(&body));
    racing_a_move(&f, destination, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(
        f.read(t.org, t.ground).await.unwrap().parent_id,
        Some(t.tower)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_delete_of_an_account_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let request = f.request(Method::DELETE, &one(t.ground), t.org, &admin, None);
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert!(f.read(t.org, t.ground).await.is_some());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_create_under_a_parent_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let body = json!({ "parent_id": t.ground, "name": "Ramp" }).to_string();
    let request = f.request(Method::POST, ACCOUNTS, t.org, &admin, Some(&body));
    let before = f.rows(t.org).await.len();
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(f.rows(t.org).await.len(), before, "no account was created");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rename_of_an_account_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", &[Scope::Account(t.tower)])
        .await;
    let body = json!({ "name": "Renamed" }).to_string();
    let request = f.request(Method::PATCH, &one(t.ground), t.org, &admin, Some(&body));
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    assert_eq!(f.read(t.org, t.ground).await.unwrap().name, "Ground");
}
