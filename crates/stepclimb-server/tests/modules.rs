// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module and item authoring routes over the router
//! (docs/specs/m1/03-modules-and-items.md B1 to B12; T1 to T7 and T13 for the write routes).
//! The permission suite's generated cases for each route (T12) are in `tests/permissions.rs`;
//! the positions under concurrent writes (T8) are in `stepclimb-core`'s `tests/modules.rs`.
//!
//! Every request names its organization with the `test-support` header, since most tests hold
//! two organizations. Needs `DATABASE_URL` (`just up`).

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
    courses,
    id::Uuid,
    items, modules, organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles,
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

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

fn course_modules(course: Uuid) -> String {
    format!("/api/v1/courses/{course}/modules")
}

fn module_path(module: Uuid) -> String {
    format!("/api/v1/modules/{module}")
}

fn item_path(item: Uuid) -> String {
    format!("/api/v1/items/{item}")
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
    fn invalid_at(&self, field: &str, code: &str) {
        self.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        let errors: Vec<(&str, &str)> = self.body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .map(|e| {
                (
                    e["field"].as_str().expect("a field"),
                    e["code"].as_str().expect("a code"),
                )
            })
            .collect();
        assert_eq!(errors, [(field, code)], "{self:?}");
    }

    #[track_caller]
    fn ok(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }

    fn id(&self) -> Uuid {
        self.body["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a UUID")
    }
}

/// An organization with Tower and Enroute under the root, and a draft course in Tower.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    tower: Uuid,
    enroute: Uuid,
    course: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    a: Tree,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let state = AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
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
            a,
        }
    }

    /// A signed-in user holding the built-in role `key` at `at`.
    async fn holder(&self, org: Uuid, key: &str, at: Scope) -> String {
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
        let user = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
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
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn org_admin(&self, org: Uuid) -> String {
        self.holder(org, "org_admin", Scope::Organization).await
    }

    async fn module(&self, t: Tree) -> Uuid {
        modules::create(&self.pool, scope(t.org), t.course, "Module", now())
            .await
            .expect("create module")
            .id
    }

    async fn header(&self, t: Tree, module: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::Header,
            title: "Header".into(),
            ref_id: None,
            url: None,
            position: None,
        };
        items::create(&self.pool, scope(t.org), module, &new, now())
            .await
            .expect("create item")
            .id
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

    /// Every module and item row of `org`, as text.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(m)::text FROM modules m WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(i)::text FROM module_items i WHERE org_id = $1
             ORDER BY 1",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read rows")
    }
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
    let enroute = create(root, "Enroute").await;
    let new = courses::NewCourse {
        account_id: tower,
        title: "Course S1".into(),
        code: "S1".into(),
    };
    let course = courses::create(pool, scope(org), &new, now(), Actor::Instance)
        .await
        .expect("create course")
        .id;
    Tree {
        org,
        tower,
        enroute,
        course,
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

/// T1 over the router, and the schemas of every success body.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_course_designer_builds_an_outline(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    let mut ids = Vec::new();
    for (n, title) in ["Week 1", "Week 2", "Week 3"].into_iter().enumerate() {
        let created = f
            .send(
                Method::POST,
                &course_modules(t.course),
                t.org,
                &designer,
                Some(json!({ "title": title })),
            )
            .await;
        let body = created.ok(StatusCode::CREATED);
        assert_schema("Module", body);
        assert_eq!(body["position"], n);
        assert_eq!(body["published"], false);
        assert_eq!(body["items"], json!([]));
        assert_eq!(
            created.headers.get(header::LOCATION).unwrap(),
            &module_path(created.id())
        );
        ids.push(created.id());
    }
    let link = f
        .send(
            Method::POST,
            &format!("{}/items", module_path(ids[0])),
            t.org,
            &designer,
            Some(json!({ "kind": "external_link", "title": "Charts", "url": "https://example.com/a?b=1" })),
        )
        .await;
    let body = link.ok(StatusCode::CREATED);
    assert_schema("Item", body);
    assert_eq!(body["url"], "https://example.com/a?b=1");
    assert!(body.get("ref_id").is_none(), "{body}");
    assert_eq!(
        link.headers.get(header::LOCATION).unwrap(),
        &item_path(link.id())
    );
    let header_item = f
        .send(
            Method::POST,
            &format!("{}/items", module_path(ids[0])),
            t.org,
            &designer,
            Some(json!({ "kind": "header", "title": "Section", "position": 0 })),
        )
        .await;
    let header_body = header_item.ok(StatusCode::CREATED);
    assert!(header_body.get("url").is_none(), "{header_body}");
    assert_eq!(header_body["position"], 0);

    let patched = f
        .send(
            Method::PATCH,
            &module_path(ids[0]),
            t.org,
            &designer,
            Some(json!({ "published": true })),
        )
        .await;
    let body = patched.ok(StatusCode::OK);
    assert_schema("Module", body);
    let titles: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Section", "Charts"]);

    let reordered = f
        .send(
            Method::PUT,
            &format!("{}/order", course_modules(t.course)),
            t.org,
            &designer,
            Some(json!({ "module_ids": [ids[2], ids[0], ids[1]] })),
        )
        .await;
    let body = reordered.ok(StatusCode::OK);
    assert_schema("ModuleList", body);
    let order: Vec<&str> = body["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["Week 3", "Week 1", "Week 2"]);
    assert_eq!(body["modules"][1]["items"].as_array().unwrap().len(), 2);

    let deleted = f
        .send(
            Method::DELETE,
            &item_path(link.id()),
            t.org,
            &designer,
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    let deleted = f
        .send(Method::DELETE, &module_path(ids[1]), t.org, &designer, None)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
}

/// T2.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_every_write_route_answers_409_in_an_archived_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let (m1, m2) = (f.module(t).await, f.module(t).await);
    let item = f.header(t, m1).await;
    for status in [courses::Status::Published, courses::Status::Archived] {
        courses::set_status(
            &f.pool,
            scope(t.org),
            t.course,
            status,
            now(),
            Actor::Instance,
        )
        .await
        .expect("change status");
    }
    let before = f.rows(t.org).await;
    let calls = [
        (
            Method::POST,
            course_modules(t.course),
            Some(json!({ "title": "X" })),
        ),
        (
            Method::PUT,
            format!("{}/order", course_modules(t.course)),
            Some(json!({ "module_ids": [m2, m1] })),
        ),
        (
            Method::PATCH,
            module_path(m1),
            Some(json!({ "title": "X" })),
        ),
        (Method::DELETE, module_path(m2), None),
        (
            Method::POST,
            format!("{}/items", module_path(m1)),
            Some(json!({ "kind": "header", "title": "X" })),
        ),
        (
            Method::PUT,
            format!("{}/items/order", module_path(m1)),
            Some(json!({ "item_ids": [item] })),
        ),
        (
            Method::PATCH,
            item_path(item),
            Some(json!({ "title": "X" })),
        ),
        (
            Method::POST,
            format!("{}/move", item_path(item)),
            Some(json!({ "module_id": m2, "position": 0 })),
        ),
        (Method::DELETE, item_path(item), None),
    ];
    for (method, uri, body) in calls {
        f.send(method, &uri, t.org, &admin, body)
            .await
            .expect(StatusCode::CONFLICT, "course.archived");
    }
    assert_eq!(f.rows(t.org).await, before);
}

/// T4 and T5 over the router.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_t5_kinds_and_urls_are_checked(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let module = f.module(t).await;
    let items_uri = format!("{}/items", module_path(module));
    for kind in ["quiz", "activity", "page", "file", "video"] {
        f.send(
            Method::POST,
            &items_uri,
            t.org,
            &admin,
            Some(json!({ "kind": kind, "title": "X", "ref_id": Uuid::nil() })),
        )
        .await
        .invalid_at("/kind", "item.kind_unavailable");
    }
    f.send(
        Method::POST,
        &items_uri,
        t.org,
        &admin,
        Some(json!({ "kind": "lti", "title": "X" })),
    )
    .await
    .expect(StatusCode::BAD_REQUEST, "request.malformed");
    let long = format!("https://example.com/{}", "a".repeat(2029));
    for url in [
        "javascript:alert(1)",
        "//evil.example",
        "https://user:pw@host/",
        "https://exa\tmple.com/",
        long.as_str(),
    ] {
        f.send(
            Method::POST,
            &items_uri,
            t.org,
            &admin,
            Some(json!({ "kind": "external_link", "title": "X", "url": url })),
        )
        .await
        .invalid_at("/url", "item.url_invalid");
    }
    let cases = [
        (
            json!({ "kind": "external_link", "title": "X" }),
            "/url",
            "item.url_required",
        ),
        (
            json!({ "kind": "header", "title": "X", "url": "https://example.com/" }),
            "/url",
            "item.url_unavailable",
        ),
        (
            json!({ "kind": "header", "title": "X", "ref_id": Uuid::nil() }),
            "/ref_id",
            "item.ref_id_unavailable",
        ),
        (
            json!({ "kind": "header", "title": "X", "position": 1 }),
            "/position",
            "item.position_out_of_range",
        ),
        (
            json!({ "kind": "header", "title": " " }),
            "/title",
            "name.empty",
        ),
    ];
    for (body, field, code) in cases {
        f.send(Method::POST, &items_uri, t.org, &admin, Some(body))
            .await
            .invalid_at(field, code);
    }
    assert!(f.rows(t.org).await.len() == 1, "only the module");
    let header_item = f.header(t, module).await;
    f.send(
        Method::PATCH,
        &item_path(header_item),
        t.org,
        &admin,
        Some(json!({ "url": "https://example.com/" })),
    )
    .await
    .invalid_at("/url", "item.url_unavailable");
    f.send(
        Method::PATCH,
        &item_path(header_item),
        t.org,
        &admin,
        Some(json!({ "title": null })),
    )
    .await
    .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

/// T3, T6 and T7 over the router.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_t6_t7_delete_reorder_and_move_rules(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let (ma, mb) = (f.module(t).await, f.module(t).await);
    let (a0, a1) = (f.header(t, ma).await, f.header(t, ma).await);
    f.send(Method::DELETE, &module_path(ma), t.org, &admin, None)
        .await
        .expect(StatusCode::CONFLICT, "module.not_empty");
    f.send(
        Method::PUT,
        &format!("{}/items/order", module_path(ma)),
        t.org,
        &admin,
        Some(json!({ "item_ids": [a1, a0, a0] })),
    )
    .await
    .expect(StatusCode::CONFLICT, "order.stale");
    f.send(
        Method::PUT,
        &format!("{}/order", course_modules(t.course)),
        t.org,
        &admin,
        Some(json!({ "module_ids": [mb] })),
    )
    .await
    .expect(StatusCode::CONFLICT, "order.stale");
    let moved = f
        .send(
            Method::POST,
            &format!("{}/move", item_path(a1)),
            t.org,
            &admin,
            Some(json!({ "module_id": mb, "position": 0 })),
        )
        .await;
    let body = moved.ok(StatusCode::OK);
    assert_schema("Item", body);
    assert_eq!(
        (body["module_id"].as_str(), &body["position"]),
        (Some(mb.to_string().as_str()), &json!(0))
    );
    f.send(
        Method::POST,
        &format!("{}/move", item_path(a0)),
        t.org,
        &admin,
        Some(json!({ "module_id": mb, "position": 5 })),
    )
    .await
    .invalid_at("/position", "item.position_out_of_range");
    let other = courses::create(
        &f.pool,
        scope(t.org),
        &courses::NewCourse {
            account_id: t.tower,
            title: "Other".into(),
            code: "S2".into(),
        },
        now(),
        Actor::Instance,
    )
    .await
    .unwrap()
    .id;
    let elsewhere = modules::create(&f.pool, scope(t.org), other, "Elsewhere", now())
        .await
        .unwrap()
        .id;
    f.send(
        Method::POST,
        &format!("{}/move", item_path(a0)),
        t.org,
        &admin,
        Some(json!({ "module_id": elsewhere, "position": 0 })),
    )
    .await
    .expect(StatusCode::NOT_FOUND, "module.not_found");
}

/// T13: org B's ids in the path and in the body.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_every_write_route_refuses_another_organizations_ids(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, tree(&f.pool, "org-b").await);
    let admin = f.org_admin(a.org).await;
    let (theirs_module, theirs_other) = (f.module(b).await, f.module(b).await);
    let theirs_item = f.header(b, theirs_module).await;
    let mine_module = f.module(a).await;
    let mine_item = f.header(a, mine_module).await;
    let before_a = f.rows(a.org).await;
    let before_b = f.rows(b.org).await;
    let calls = [
        (
            Method::POST,
            course_modules(b.course),
            Some(json!({ "title": "X" })),
            StatusCode::NOT_FOUND,
            "course.not_found",
        ),
        (
            Method::PUT,
            format!("{}/order", course_modules(b.course)),
            Some(json!({ "module_ids": [theirs_other, theirs_module] })),
            StatusCode::NOT_FOUND,
            "course.not_found",
        ),
        (
            Method::PUT,
            format!("{}/order", course_modules(a.course)),
            Some(json!({ "module_ids": [theirs_module] })),
            StatusCode::CONFLICT,
            "order.stale",
        ),
        (
            Method::PATCH,
            module_path(theirs_module),
            Some(json!({ "title": "X" })),
            StatusCode::NOT_FOUND,
            "module.not_found",
        ),
        (
            Method::DELETE,
            module_path(theirs_other),
            None,
            StatusCode::NOT_FOUND,
            "module.not_found",
        ),
        (
            Method::POST,
            format!("{}/items", module_path(theirs_module)),
            Some(json!({ "kind": "header", "title": "X" })),
            StatusCode::NOT_FOUND,
            "module.not_found",
        ),
        (
            Method::PUT,
            format!("{}/items/order", module_path(theirs_module)),
            Some(json!({ "item_ids": [theirs_item] })),
            StatusCode::NOT_FOUND,
            "module.not_found",
        ),
        (
            Method::PUT,
            format!("{}/items/order", module_path(mine_module)),
            Some(json!({ "item_ids": [theirs_item] })),
            StatusCode::CONFLICT,
            "order.stale",
        ),
        (
            Method::PATCH,
            item_path(theirs_item),
            Some(json!({ "title": "X" })),
            StatusCode::NOT_FOUND,
            "item.not_found",
        ),
        (
            Method::POST,
            format!("{}/move", item_path(theirs_item)),
            Some(json!({ "module_id": mine_module, "position": 0 })),
            StatusCode::NOT_FOUND,
            "item.not_found",
        ),
        (
            Method::POST,
            format!("{}/move", item_path(mine_item)),
            Some(json!({ "module_id": theirs_module, "position": 0 })),
            StatusCode::NOT_FOUND,
            "module.not_found",
        ),
        (
            Method::DELETE,
            item_path(theirs_item),
            None,
            StatusCode::NOT_FOUND,
            "item.not_found",
        ),
    ];
    let theirs = [theirs_module, theirs_other, theirs_item].map(|id| id.to_string());
    for (method, uri, body, status, code) in calls {
        let answer = f.send(method, &uri, a.org, &admin, body).await;
        answer.expect(status, code);
        let text = answer.body.to_string();
        assert!(!theirs.iter().any(|id| text.contains(id)), "{text}");
    }
    assert_eq!(f.rows(b.org).await, before_b, "org B is unchanged");
    assert_eq!(f.rows(a.org).await, before_a, "org A is unchanged");
}

/// The extractor's check is repeated on the path the write's transaction reads: an item create
/// that waits on an account move carrying the course out of the caller's reach answers 404.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_write_to_a_course_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    // A sub-account of Tower holds the course; the move carries it under Enroute.
    let ground = accounts::create(
        &f.pool,
        scope(t.org),
        t.tower,
        "Ground",
        now(),
        Actor::Instance,
    )
    .await
    .unwrap()
    .id;
    courses::move_to(
        &f.pool,
        scope(t.org),
        t.course,
        ground,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let module = f.module(t).await;
    let body = json!({ "kind": "header", "title": "Late" }).to_string();
    let uri = format!("{}/items", module_path(module));
    let request = f.request(Method::POST, &uri, t.org, &designer, Some(&body));

    let mut tx = f.pool.begin().await.expect("begin");
    accounts::lock_tree(&mut tx, scope(t.org))
        .await
        .expect("take the tree lock");
    accounts::move_to(
        &mut *tx,
        scope(t.org),
        ground,
        t.enroute,
        now(),
        Actor::Instance,
    )
    .await
    .expect("the competing move");
    let app = f.app.clone();
    let pending = tokio::spawn(async move { answer(&app, request).await });
    until_a_call_waits_on_a_lock(&f.pool).await;
    tx.commit().await.expect("commit the competing move");
    pending
        .await
        .expect("the request ran")
        .expect(StatusCode::NOT_FOUND, "module.not_found");
    assert_eq!(f.rows(t.org).await.len(), 1, "no item was created");
}

/// Waits until a session of this test's database waits on a lock.
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
