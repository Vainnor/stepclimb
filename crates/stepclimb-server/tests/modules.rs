// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module and item routes over the router: authoring
//! (docs/specs/m1/03-modules-and-items.md B1 to B12; T1 to T7 and T13 for the write routes),
//! reading (B13 to B15; T9 and T13 for the read routes, and the references and URLs a read
//! withholds from a learner until spec 04 I3), and
//! the rule fields those routes write (docs/specs/m1/04-unlock-and-completion-rules.md B1 to B3;
//! T9, T10 and T15's cross-organization ids).
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
    courses, enrollments,
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
            storage: None,
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
        let (user, cookie) = self.user(org).await;
        let assignment = NewAssignment {
            user_id: user,
            role_id: self.builtin_role(key).await,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
            .await
            .expect("create assignment");
        cookie
    }

    /// A signed-in user enrolled in `t`'s course with the built-in role Student.
    async fn student(&self, t: Tree) -> String {
        let (user, cookie) = self.user(t.org).await;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: self.builtin_role("student").await,
            source: Source::Local,
            external_id: None,
        };
        enrollments::create(
            &self.pool,
            scope(t.org),
            t.course,
            &new,
            now(),
            Actor::Instance,
        )
        .await
        .expect("enroll");
        cookie
    }

    async fn builtin_role(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    /// A signed-in user of `org` with no role: their id and session cookie.
    async fn user(&self, org: Uuid) -> (Uuid, String) {
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
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (user, cookie.split(';').next().expect("a pair").to_owned())
    }

    async fn set_course_status(&self, t: Tree, to: courses::Status) {
        courses::set_status(
            &self.pool,
            scope(t.org),
            t.course,
            to,
            now(),
            Actor::Instance,
        )
        .await
        .expect("set the course status");
    }

    async fn publish_module(&self, t: Tree, module: Uuid) {
        let patch = modules::ModulePatch {
            published: Some(true),
            ..Default::default()
        };
        modules::update(&self.pool, scope(t.org), module, &patch, now())
            .await
            .expect("publish the module");
    }

    async fn publish_item(&self, t: Tree, item: Uuid) {
        let patch = items::ItemPatch {
            published: Some(true),
            ..Default::default()
        };
        items::update(&self.pool, scope(t.org), item, &patch, now())
            .await
            .expect("publish the item");
    }

    /// A published module of `t`'s course.
    async fn published_module(&self, t: Tree) -> Uuid {
        let module = self.module(t).await;
        self.publish_module(t, module).await;
        module
    }

    /// A published external link to `url`, last in `module`.
    async fn published_link(&self, t: Tree, module: Uuid, url: &str) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::ExternalLink,
            title: "Link".into(),
            ref_id: None,
            url: Some(url.into()),
            position: None,
        };
        let item = items::create(&self.pool, scope(t.org), module, &new, now())
            .await
            .expect("create item")
            .id;
        self.publish_item(t, item).await;
        item
    }

    async fn get(&self, uri: &str, org: Uuid, cookie: &str) -> Answer {
        self.send(Method::GET, uri, org, cookie, None).await
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

    /// Every module, item and prerequisite row of `org`, as text.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(m)::text FROM modules m WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(i)::text FROM module_items i WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(p)::text FROM module_prerequisites p WHERE org_id = $1
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
        "https://example.com/\u{202E}gpj.exe",
        "https://exa\u{200B}mple.com/",
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

// Rules (spec 04 B1 to B3).

/// Spec 04 T9 over the router, and the rule fields in the `Module` schema.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn spec04_t9_module_rules_over_the_router(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    let (ma, mb) = (f.module(t).await, f.module(t).await);
    let patch = |module: Uuid, body: Value| {
        let (f, designer) = (&f, designer.clone());
        async move {
            f.send(
                Method::PATCH,
                &module_path(module),
                t.org,
                &designer,
                Some(body),
            )
            .await
        }
    };
    let fresh = patch(mb, json!({})).await;
    let body = fresh.ok(StatusCode::OK);
    assert_schema("Module", body);
    assert_eq!(body["unlock_at"], Value::Null);
    assert_eq!(body["prerequisite_ids"], json!([]));
    assert_eq!(body["require_sequential"], false);
    assert_eq!(body["completion_rule"], "all");

    let set = patch(
        mb,
        json!({
            "unlock_at": "2030-01-02T05:04:05+02:00",
            "prerequisite_ids": [ma],
            "require_sequential": true,
            "completion_rule": "one",
        }),
    )
    .await;
    let body = set.ok(StatusCode::OK);
    assert_schema("Module", body);
    assert_eq!(body["unlock_at"], "2030-01-02T03:04:05.000000Z");
    assert_eq!(body["prerequisite_ids"], json!([ma]));
    assert_eq!(
        (&body["require_sequential"], &body["completion_rule"]),
        (&json!(true), &json!("one"))
    );

    patch(ma, json!({ "prerequisite_ids": [mb] }))
        .await
        .expect(StatusCode::CONFLICT, "module.prerequisite_cycle");
    patch(ma, json!({ "prerequisite_ids": [ma] }))
        .await
        .invalid_at("/prerequisite_ids", "module.prerequisite_self");
    patch(ma, json!({ "prerequisite_ids": [mb, mb] }))
        .await
        .invalid_at("/prerequisite_ids", "module.prerequisite_duplicate");
    let many: Vec<Uuid> = (0..21).map(|_| Uuid::now_v7()).collect();
    patch(ma, json!({ "prerequisite_ids": many }))
        .await
        .invalid_at("/prerequisite_ids", "module.prerequisites_too_many");
    patch(ma, json!({ "unlock_at": "1969-12-31T23:59:59Z" }))
        .await
        .invalid_at("/unlock_at", "module.date_out_of_range");
    patch(ma, json!({ "completion_rule": null }))
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
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
    patch(ma, json!({ "prerequisite_ids": [elsewhere] }))
        .await
        .expect(StatusCode::NOT_FOUND, "module.not_found");
    let cleared = patch(mb, json!({ "unlock_at": null, "prerequisite_ids": [] })).await;
    let body = cleared.ok(StatusCode::OK);
    assert_eq!(
        (&body["unlock_at"], &body["prerequisite_ids"]),
        (&Value::Null, &json!([]))
    );
}

/// Spec 04 T10 over the router, and `requirement` in the `Item` schema.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn spec04_t10_item_requirements_over_the_router(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let module = f.module(t).await;
    let header = f.header(t, module).await;
    let new = items::NewItem {
        kind: items::Kind::ExternalLink,
        title: "Charts".into(),
        ref_id: None,
        url: Some("https://example.com/charts".into()),
        position: None,
    };
    let link = items::create(&f.pool, scope(t.org), module, &new, now())
        .await
        .unwrap()
        .id;
    let patch = |item: Uuid, body: Value| {
        let (f, admin) = (&f, admin.clone());
        async move {
            f.send(Method::PATCH, &item_path(item), t.org, &admin, Some(body))
                .await
        }
    };
    let plain = patch(link, json!({})).await;
    let body = plain.ok(StatusCode::OK);
    assert_schema("Item", body);
    assert_eq!(body["requirement"], Value::Null);
    for requirement in [json!({ "kind": "view" }), json!({ "kind": "mark_done" })] {
        let set = patch(link, json!({ "requirement": requirement })).await;
        let body = set.ok(StatusCode::OK);
        assert_schema("Item", body);
        assert_eq!(body["requirement"], requirement);
    }
    let invalid = [
        (header, json!({ "kind": "view" })),
        (link, json!({ "kind": "watch", "min_percent": 50 })),
        (link, json!({ "kind": "watch", "min_percent": 0 })),
        (link, json!({ "kind": "watch", "min_percent": 101 })),
        (link, json!({ "kind": "view", "after": 3 })),
        (link, json!({ "kind": "complete" })),
    ];
    for (item, requirement) in invalid {
        patch(item, json!({ "requirement": requirement }))
            .await
            .invalid_at("/requirement", "requirement.invalid");
    }
    for kind in ["submit", "score", "mastery"] {
        patch(link, json!({ "requirement": { "kind": kind } }))
            .await
            .invalid_at("/requirement", "requirement.unavailable");
    }
    for not_an_object in [json!("view"), json!(["view"]), json!(1)] {
        patch(link, json!({ "requirement": not_an_object }))
            .await
            .expect(StatusCode::BAD_REQUEST, "request.malformed");
    }
    let removed = patch(link, json!({ "requirement": null })).await;
    assert_eq!(removed.ok(StatusCode::OK)["requirement"], Value::Null);
}

/// Spec 04 T15: org B's ids in a rule write, in the path and as a prerequisite, are not found
/// and change nothing; an archived course takes no rule write.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn spec04_t15_rule_writes_refuse_another_organizations_ids(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, tree(&f.pool, "org-b").await);
    let admin = f.org_admin(a.org).await;
    let mine = f.module(a).await;
    let theirs = f.module(b).await;
    let their_item = f.header(b, theirs).await;
    let before_a = f.rows(a.org).await;
    let before_b = f.rows(b.org).await;
    let calls = [
        (
            module_path(mine),
            json!({ "prerequisite_ids": [theirs] }),
            "module.not_found",
        ),
        (
            module_path(theirs),
            json!({ "require_sequential": true }),
            "module.not_found",
        ),
        (
            item_path(their_item),
            json!({ "requirement": { "kind": "view" } }),
            "item.not_found",
        ),
    ];
    for (uri, body, code) in calls {
        let answer = f.send(Method::PATCH, &uri, a.org, &admin, Some(body)).await;
        answer.expect(StatusCode::NOT_FOUND, code);
        let text = answer.body.to_string();
        assert!(!text.contains(&theirs.to_string()), "{text}");
    }
    assert_eq!(f.rows(b.org).await, before_b, "org B is unchanged");
    assert_eq!(f.rows(a.org).await, before_a, "org A is unchanged");

    for status in [courses::Status::Published, courses::Status::Archived] {
        courses::set_status(
            &f.pool,
            scope(a.org),
            a.course,
            status,
            now(),
            Actor::Instance,
        )
        .await
        .expect("change status");
    }
    f.send(
        Method::PATCH,
        &module_path(mine),
        a.org,
        &admin,
        Some(json!({ "completion_rule": "one" })),
    )
    .await
    .expect(StatusCode::CONFLICT, "course.archived");
}

/// The ids of a list of modules or items in a response.
fn ids(list: &Value) -> Vec<Uuid> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|v| v["id"].as_str().expect("an id").parse().expect("a UUID"))
        .collect()
}

/// T9 (B13, B14): an enrolled Student sees only published modules, their published items and
/// published prerequisites; an unpublished module or item, or an item of an unpublished module,
/// answers 404 to the Student and 200 to a Course designer.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_student_reads_only_published_content(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    f.set_course_status(t, courses::Status::Published).await;
    let open = f.published_module(t).await;
    let hidden = f.module(t).await;
    let later = f.published_module(t).await;
    let shown = f.published_link(t, open, "https://example.com/a").await;
    let unpublished = f.header(t, open).await;
    let inner = f.published_link(t, hidden, "https://example.com/b").await;
    let rules = modules::RulesPatch {
        prerequisite_ids: Some(vec![open, hidden]),
        ..Default::default()
    };
    modules::set_rules(&f.pool, scope(t.org), later, &rules, now())
        .await
        .unwrap();
    let student = f.student(t).await;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;

    let list = f.get(&course_modules(t.course), t.org, &student).await;
    let body = list.ok(StatusCode::OK);
    assert_schema("ModuleList", body);
    assert_eq!(ids(&body["modules"]), [open, later]);
    assert_eq!(ids(&body["modules"][0]["items"]), [shown]);
    assert_eq!(body["modules"][1]["prerequisite_ids"], json!([open]));
    let list = f.get(&course_modules(t.course), t.org, &designer).await;
    let body = list.ok(StatusCode::OK);
    assert_eq!(ids(&body["modules"]), [open, hidden, later]);
    assert_eq!(ids(&body["modules"][0]["items"]), [shown, unpublished]);
    assert_eq!(
        body["modules"][2]["prerequisite_ids"],
        json!([open, hidden])
    );

    let module = f.get(&module_path(open), t.org, &student).await;
    let body = module.ok(StatusCode::OK);
    assert_schema("Module", body);
    assert_eq!(ids(&body["items"]), [shown]);
    let module = f.get(&module_path(later), t.org, &student).await;
    assert_eq!(module.ok(StatusCode::OK)["prerequisite_ids"], json!([open]));
    f.get(&module_path(hidden), t.org, &student)
        .await
        .expect(StatusCode::NOT_FOUND, "module.not_found");
    f.get(&module_path(hidden), t.org, &designer)
        .await
        .ok(StatusCode::OK);

    let item = f.get(&item_path(shown), t.org, &student).await;
    let body = item.ok(StatusCode::OK);
    assert_schema("Item", body);
    assert_eq!(body["url"], "https://example.com/a");
    assert_eq!(
        (body["module_id"].as_str(), body["course_id"].as_str()),
        (
            Some(open.to_string().as_str()),
            Some(t.course.to_string().as_str())
        )
    );
    for hidden_item in [unpublished, inner] {
        f.get(&item_path(hidden_item), t.org, &student)
            .await
            .expect(StatusCode::NOT_FOUND, "item.not_found");
        f.get(&item_path(hidden_item), t.org, &designer)
            .await
            .ok(StatusCode::OK);
    }
}

/// B13, B14 and spec 02 B24: a draft course's outline, modules and items answer 404 to a
/// Student enrolled in it and 200 to a Course designer; an archived course's answer 200 to both
/// (T2: reads still answer in an archived course).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_course_status_decides_whether_a_student_reads_the_outline(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let module = f.published_module(t).await;
    let item = f.published_link(t, module, "https://example.com/a").await;
    let student = f.student(t).await;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    let reads = [
        (course_modules(t.course), "course.not_found"),
        (module_path(module), "module.not_found"),
        (item_path(item), "item.not_found"),
    ];
    for (uri, code) in &reads {
        let answer = f.get(uri, t.org, &student).await;
        answer.expect(StatusCode::NOT_FOUND, code);
        assert!(
            !answer.body.to_string().contains("example.com"),
            "{answer:?}"
        );
        f.get(uri, t.org, &designer).await.ok(StatusCode::OK);
    }
    f.set_course_status(t, courses::Status::Published).await;
    f.set_course_status(t, courses::Status::Archived).await;
    for (uri, _) in &reads {
        f.get(uri, t.org, &student).await.ok(StatusCode::OK);
        f.get(uri, t.org, &designer).await.ok(StatusCode::OK);
    }
}

/// The stand-in for spec 04 I3: while a module has an unlock date (future or past),
/// prerequisites or sequential order, every read withholds its items' URLs from a Student (the
/// raw body does not hold them); a module without rules shows them; a Course designer sees them
/// all.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn reads_withhold_links_of_a_module_with_unlock_rules_from_a_student(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    f.set_course_status(t, courses::Status::Published).await;
    let (ruled, plain) = (f.published_module(t).await, f.published_module(t).await);
    let gated_url = "https://gated.example/secret";
    let gated = f.published_link(t, ruled, gated_url).await;
    let open = f.published_link(t, plain, "https://open.example/a").await;
    let student = f.student(t).await;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    let day = Duration::from_secs(86_400);
    let rules = [
        modules::RulesPatch {
            unlock_at: Some(Some(now() + day)),
            ..Default::default()
        },
        modules::RulesPatch {
            unlock_at: Some(Some(now() - day)),
            ..Default::default()
        },
        modules::RulesPatch {
            prerequisite_ids: Some(vec![plain]),
            ..Default::default()
        },
        modules::RulesPatch {
            require_sequential: Some(true),
            ..Default::default()
        },
    ];
    let cleared = modules::RulesPatch {
        unlock_at: Some(None),
        prerequisite_ids: Some(Vec::new()),
        require_sequential: Some(false),
        completion_rule: None,
    };
    for rule in rules {
        modules::set_rules(&f.pool, scope(t.org), ruled, &cleared, now())
            .await
            .unwrap();
        modules::set_rules(&f.pool, scope(t.org), ruled, &rule, now())
            .await
            .unwrap();
        let list = f.get(&course_modules(t.course), t.org, &student).await;
        let body = list.ok(StatusCode::OK);
        assert!(!body.to_string().contains(gated_url), "{rule:?}: {body}");
        assert_eq!(ids(&body["modules"][0]["items"]), [gated], "{rule:?}");
        assert!(
            body["modules"][0]["items"][0].get("url").is_none(),
            "{rule:?}"
        );
        assert_eq!(
            body["modules"][1]["items"][0]["url"],
            "https://open.example/a"
        );
        let module = f.get(&module_path(ruled), t.org, &student).await;
        let body = module.ok(StatusCode::OK);
        assert!(!body.to_string().contains(gated_url), "{rule:?}: {body}");
        assert_eq!(ids(&body["items"]), [gated], "{rule:?}");
        let item = f.get(&item_path(gated), t.org, &student).await;
        let body = item.ok(StatusCode::OK);
        assert_schema("Item", body);
        assert!(!body.to_string().contains(gated_url), "{rule:?}: {body}");
        assert_eq!(body["title"], "Link");
        let item = f.get(&item_path(open), t.org, &student).await;
        assert_eq!(item.ok(StatusCode::OK)["url"], "https://open.example/a");

        for uri in [
            course_modules(t.course),
            module_path(ruled),
            item_path(gated),
        ] {
            let answer = f.get(&uri, t.org, &designer).await;
            assert!(
                answer.ok(StatusCode::OK).to_string().contains(gated_url),
                "{rule:?}: {uri}"
            );
        }
    }
    modules::set_rules(&f.pool, scope(t.org), ruled, &cleared, now())
        .await
        .unwrap();
    let item = f.get(&item_path(gated), t.org, &student).await;
    assert_eq!(
        item.ok(StatusCode::OK)["url"],
        gated_url,
        "no rule, no withholding"
    );
}

/// T13 for the read routes: org A's admin naming org B's published course, module and item gets
/// 404, and nothing of org B's in the body.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_every_read_route_refuses_another_organizations_ids(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, tree(&f.pool, "org-b").await);
    let admin = f.org_admin(a.org).await;
    f.set_course_status(b, courses::Status::Published).await;
    let module = f.published_module(b).await;
    let item = f.published_link(b, module, "https://theirs.example/").await;
    let before = f.rows(b.org).await;
    let calls = [
        (course_modules(b.course), "course.not_found"),
        (module_path(module), "module.not_found"),
        (item_path(item), "item.not_found"),
    ];
    let theirs = [b.course, module, item].map(|id| id.to_string());
    for (uri, code) in calls {
        let answer = f.get(&uri, a.org, &admin).await;
        answer.expect(StatusCode::NOT_FOUND, code);
        let text = answer.body.to_string();
        assert!(!theirs.iter().any(|id| text.contains(id)), "{text}");
        assert!(!text.contains("theirs.example"), "{text}");
    }
    assert_eq!(f.rows(b.org).await, before, "org B is unchanged");
}
