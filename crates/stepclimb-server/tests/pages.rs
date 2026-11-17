// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pages and their versions over the router (docs/specs/m1/07-pages-and-the-editor.md B1 to B13;
//! T1 to T5, T7a, T10, T11 and T15 for these routes). The permission suite's generated cases for
//! each route (T14, T15) are in `tests/permissions.rs`.
//!
//! Course files are uploaded through spec 06's route into a local store under the system's
//! temporary directory, so no case needs the gate's MinIO. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

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
    id::{Uuid, new_id},
    modules, organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, config::StorageConfig, request_org::TEST_ORG_HEADER, router, session,
    storage::Storage, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";
const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01";
const GIF: &[u8] = b"GIF89a\x01\0\x01\0\0\0\0;";
const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 \x18\0\0\0";
const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << >> endobj\n%%EOF\n";
const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
const TEXT: &[u8] = b"Plain words, not a picture.\n";
/// Bytes no signature matches and that are not UTF-8: `application/octet-stream`.
const UNKNOWN: &[u8] = b"\x00\x9f\x92\x96\xfe\xfe";

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// A directory of its own under the system's temporary directory, removed on drop.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

    fn uuid(&self, pointer: &str) -> Uuid {
        self.body
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{pointer} in {self:?}"))
            .parse()
            .expect("a UUID")
    }
}

/// An organization with Tower under the root and a draft course in Tower.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    tower: Uuid,
    course: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
    _dir: TempDir,
    a: Tree,
    admin: String,
    admin_id: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, SystemTime::now())
            .await
            .expect("sync roles");
        let clock = ManualClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000));
        let dir = std::env::temp_dir().join(format!("stepclimb-pages-{}", new_id()));
        std::fs::create_dir_all(&dir).expect("create the directory");
        let storage = Storage::open(&StorageConfig::Local { root: dir.clone() }, 64 * 1024)
            .expect("open the store");
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage,
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        let now = clock.clock().now();
        let a = tree(
            &pool,
            &format!("org-a-{}", &new_id().simple().to_string()[24..]),
            now,
        )
        .await;
        let mut fixture = Self {
            pool,
            app,
            clock,
            _dir: TempDir(dir),
            a,
            admin: String::new(),
            admin_id: Uuid::nil(),
        };
        let (admin, admin_id) = fixture.builtin(a.org, "org_admin").await;
        fixture.admin = admin;
        fixture.admin_id = admin_id;
        fixture
    }

    fn now(&self) -> SystemTime {
        self.clock.clock().now()
    }

    /// A signed-in user of `org`: their cookie and id.
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
        let user = users::create(&self.pool, scope(org), &new_user, self.now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), self.now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), user)
    }

    async fn assign(&self, org: Uuid, user: Uuid, role: Uuid, at: Scope) {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(org),
            &assignment,
            self.now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
    }

    /// A user of `org` holding the built-in role `key` at the organization.
    async fn builtin(&self, org: Uuid, key: &str) -> (String, Uuid) {
        let (cookie, user) = self.user(org).await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(org, user, role, Scope::Organization).await;
        (cookie, user)
    }

    /// A user of `org` holding a custom role with `permissions` at the organization.
    async fn holder(&self, org: Uuid, permissions: &[&str]) -> String {
        let (cookie, user) = self.user(org).await;
        let new = NewRole {
            key: format!("r{}", &new_id().simple().to_string()[20..]),
            name: "Custom".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &new, self.now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(org, user, role, Scope::Organization).await;
        cookie
    }

    async fn course(&self, org: Uuid, account: Uuid) -> Uuid {
        let new = courses::NewCourse {
            account_id: account,
            title: "Another course".into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        courses::create(&self.pool, scope(org), &new, self.now(), Actor::Instance)
            .await
            .expect("create course")
            .id
    }

    async fn set_status(&self, org: Uuid, course: Uuid, status: courses::Status) {
        courses::set_status(
            &self.pool,
            scope(org),
            course,
            status,
            self.now(),
            Actor::Instance,
        )
        .await
        .expect("set the status");
    }

    async fn send_as(
        &self,
        org: Uuid,
        cookie: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        answer(
            &self.app,
            request.body(Body::from(body)).expect("a request"),
        )
        .await
    }

    /// A request as org A's admin.
    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Answer {
        self.send_as(self.a.org, &self.admin, method, uri, body)
            .await
    }

    /// Org A's admin uploads `bytes` as `name` into `course`, sent with `content_type`: the
    /// course file's id.
    async fn upload(&self, course: Uuid, name: &str, content_type: &str, bytes: &[u8]) -> Uuid {
        self.upload_in(self.a.org, &self.admin, course, (name, content_type), bytes)
            .await
    }

    async fn upload_in(
        &self,
        org: Uuid,
        cookie: &str,
        course: Uuid,
        (name, content_type): (&str, &str),
        bytes: &[u8],
    ) -> Uuid {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/courses/{course}/files?name={name}"))
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_LENGTH, bytes.len())
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(bytes.to_vec()))
            .expect("a request");
        let uploaded = answer(&self.app, request).await;
        assert_eq!(uploaded.status, StatusCode::CREATED, "{uploaded:?}");
        uploaded.uuid("/id")
    }

    /// Org A's admin creates a page of `course`: the answer.
    async fn create(&self, course: Uuid, body: Value) -> Answer {
        let uri = format!("/api/v1/courses/{course}/pages");
        self.send(Method::POST, &uri, Some(body)).await
    }

    /// A new page of org A's course titled `title`: its id and version 1's id.
    async fn page(&self, title: &str) -> (Uuid, Uuid) {
        let created = self.create(self.a.course, json!({ "title": title })).await;
        created.ok(StatusCode::CREATED);
        (created.uuid("/id"), created.uuid("/current_version/id"))
    }

    async fn save(&self, page: Uuid, body: &Value, base: Uuid) -> Answer {
        let uri = format!("/api/v1/pages/{page}/body");
        let request = json!({ "body": body, "base_version_id": base });
        self.send(Method::PUT, &uri, Some(request)).await
    }

    async fn restore(&self, page: Uuid, version: Uuid, base: Uuid) -> Answer {
        let uri = format!("/api/v1/pages/{page}/versions/{version}/restore");
        let request = json!({ "base_version_id": base });
        self.send(Method::POST, &uri, Some(request)).await
    }

    async fn versions(&self, page: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM page_versions WHERE page_id = $1")
            .bind(page)
            .fetch_one(&self.pool)
            .await
            .expect("count versions")
    }
}

async fn tree(pool: &PgPool, slug: &str, now: SystemTime) -> Tree {
    let org = organizations::create(pool, slug, slug, now)
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let tower = accounts::create(pool, scope(org), root, "Tower", now, Actor::Instance)
        .await
        .expect("create account")
        .id;
    let new = courses::NewCourse {
        account_id: tower,
        title: "Course P1".into(),
        code: "P1".into(),
    };
    let course = courses::create(pool, scope(org), &new, now, Actor::Instance)
        .await
        .expect("create course")
        .id;
    Tree { org, tower, course }
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
        .expect("the body")
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

#[track_caller]
fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

fn paragraph(text: &str) -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": text }] },
    ] })
}

fn image(course_file_id: Uuid) -> Value {
    json!({ "type": "doc", "content": [
        { "type": "image", "attrs": {
            "course_file_id": course_file_id, "alt": "A diagram", "decorative": false,
        } },
    ] })
}

/// T1 (B1, B2): creating a page writes version 1 and makes it current; the slug is derived
/// from the title; a taken slug answers 409.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_new_page_has_version_1_and_a_derived_slug(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let created = f
        .create(f.a.course, json!({ "title": "Ground Control 101" }))
        .await;
    let body = created.ok(StatusCode::CREATED).clone();
    assert_schema("PageSummary", &body);
    let id = created.uuid("/id");
    assert_eq!(
        created.headers[header::LOCATION],
        format!("/api/v1/pages/{id}").as_str()
    );
    assert_eq!(body["slug"], "ground-control-101");
    assert_eq!(body["title"], "Ground Control 101");
    assert_eq!(body["course_id"], f.a.course.to_string());
    assert_eq!(body["current_version"]["number"], 1);
    assert_eq!(body["current_version"]["author_id"], f.admin_id.to_string());
    assert_eq!(f.versions(id).await, 1);
    // Created without a body: one empty paragraph.
    let version = created.uuid("/current_version/id");
    let read = f
        .send(
            Method::GET,
            &format!("/api/v1/pages/{id}/versions/{version}"),
            None,
        )
        .await;
    assert_schema("PageVersion", read.ok(StatusCode::OK));
    assert_eq!(
        read.body["body"],
        json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
    );

    let same = f
        .create(f.a.course, json!({ "title": "Ground control 101!" }))
        .await;
    same.expect(StatusCode::CONFLICT, "page.slug_taken");
    let given = f
        .create(
            f.a.course,
            json!({ "title": "Ground Control 101", "slug": "gc-101", "body": paragraph("Hi") }),
        )
        .await;
    assert_eq!(given.ok(StatusCode::CREATED)["slug"], "gc-101");
    let other_course = f.course(f.a.org, f.a.tower).await;
    let elsewhere = f
        .create(other_course, json!({ "title": "Ground Control 101" }))
        .await;
    elsewhere.ok(StatusCode::CREATED);

    f.create(f.a.course, json!({ "title": "X", "slug": "Not-A-Slug" }))
        .await
        .invalid_at("/slug", "page.slug_invalid");
    f.create(f.a.course, json!({ "title": "日本語" }))
        .await
        .invalid_at("/slug", "page.slug_invalid");
    f.create(f.a.course, json!({ "title": "  " }))
        .await
        .invalid_at("/title", "name.empty");
    f.create(f.a.course, json!({ "title": "x".repeat(201) }))
        .await
        .invalid_at("/title", "name.too_long");
}

/// T2 (B3): two editors save from the same base; the second is refused naming the first's
/// version, and an unchanged body writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_save_from_a_stale_base_conflicts(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Taxi").await;
    let first = f.save(page, &paragraph("First editor"), v1).await;
    let first = first.ok(StatusCode::OK).clone();
    assert_schema("PageSummary", &first);
    assert_eq!(first["current_version"]["number"], 2);
    let v2 = first["current_version"]["id"].as_str().unwrap().to_owned();

    let second = f.save(page, &paragraph("Second editor"), v1).await;
    second.expect(StatusCode::CONFLICT, "page.version_conflict");
    assert_schema("Problem", &second.body);
    assert_eq!(second.body["current_version"]["id"], v2.as_str());
    assert_eq!(second.body["current_version"]["number"], 2);
    assert_eq!(
        second.body["current_version"]["author_id"],
        f.admin_id.to_string()
    );
    assert_eq!(f.versions(page).await, 2);

    let v2: Uuid = v2.parse().unwrap();
    let unchanged = f.save(page, &paragraph("First editor"), v2).await;
    assert_eq!(
        unchanged.ok(StatusCode::OK)["current_version"]["id"],
        v2.to_string()
    );
    assert_eq!(f.versions(page).await, 2);
}

/// T3 (B2): a version's body, number, page and author cannot be updated in SQL.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_versions_are_immutable(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Immutable").await;
    for change in [
        "body = '{\"type\":\"doc\"}'::jsonb",
        "number = 7",
        "author_id = author_id",
    ] {
        let sql = format!("UPDATE page_versions SET {change} WHERE id = $1");
        let result = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(v1)
            .execute(&f.pool)
            .await;
        if change == "author_id = author_id" {
            // An update that changes nothing the trigger guards goes through.
            result.expect("an unchanged author is not a change");
            continue;
        }
        let error = result.expect_err(change);
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        assert_eq!(constraint, Some("page_versions_immutable"), "{change}");
    }
    let (other_page, _) = f.page("Other").await;
    let moved = sqlx::query("UPDATE page_versions SET page_id = $2 WHERE id = $1")
        .bind(v1)
        .bind(other_page)
        .execute(&f.pool)
        .await
        .expect_err("a version cannot move to another page");
    assert_eq!(
        moved.as_database_error().and_then(|e| e.constraint()),
        Some("page_versions_immutable")
    );
    assert_eq!(f.versions(page).await, 1);
}

/// T4, T5 (B7 to B9, B13): bodies the schema does not allow answer 422 at the first offense
/// and write nothing; the allowed links save.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_t5_a_body_outside_the_schema_is_refused_at_its_first_offense(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Schema").await;
    let link = |href: &str| {
        json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "a", "marks": [{ "type": "link", "attrs": { "href": href } }] },
        ] }] })
    };
    let mut over = "x".repeat(1_048_576);
    over.push('x');
    let cases = [
        (
            json!({ "type": "doc", "content": [{ "type": "script" }] }),
            "/body/content/0/type",
        ),
        (
            json!({ "type": "doc", "content": [{ "type": "paragraph", "attrs": { "onclick": "x()" } }] }),
            "/body/content/0/attrs/onclick",
        ),
        (
            json!({ "type": "doc", "content": [{ "type": "heading", "attrs": { "level": 1 } }] }),
            "/body/content/0/attrs/level",
        ),
        (
            json!({ "type": "doc", "content": [{ "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableHeader", "attrs": { "colspan": 21 }, "content": [{ "type": "paragraph" }] },
            ] }] }] }),
            "/body/content/0/content/0/content/0/attrs/colspan",
        ),
        (paragraph(&over), "/body"),
        (paragraph("nul\u{0}"), "/body/content/0/content/0/text"),
        (
            json!({ "type": "doc", "content": [{ "type": "question" }] }),
            "/body/content/0/type",
        ),
        (
            link("javascript:alert(1)"),
            "/body/content/0/content/0/marks/0/attrs/href",
        ),
        (
            link("data:text/html,<script>alert(1)</script>"),
            "/body/content/0/content/0/marks/0/attrs/href",
        ),
        (
            link("//evil.example"),
            "/body/content/0/content/0/marks/0/attrs/href",
        ),
        (
            link("/\\evil.example"),
            "/body/content/0/content/0/marks/0/attrs/href",
        ),
        (json!(null), "/body"),
    ];
    for (body, pointer) in cases {
        f.save(page, &body, v1)
            .await
            .invalid_at(pointer, "page.body_invalid");
    }
    assert_eq!(f.versions(page).await, 1);
    let mut base = v1;
    for href in ["https://example.com", "mailto:a@example.com", "/c/x"] {
        let saved = f.save(page, &link(href), base).await;
        saved.ok(StatusCode::OK);
        base = saved.uuid("/current_version/id");
    }
    assert_eq!(f.versions(page).await, 4);
    f.create(
        f.a.course,
        json!({ "title": "Bad", "body": { "type": "doc", "content": [] } }),
    )
    .await
    .invalid_at("/body/content", "page.body_invalid");
}

/// The published `PageDoc` schema describes every node, mark and attribute the server takes, as
/// the server writes them back.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_allowed_node_conforms_to_the_published_schema(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let png = f.upload(f.a.course, "map.png", "image/png", PNG).await;
    let text = |t: &str| json!({ "type": "text", "text": t });
    let para = |t: &str| json!({ "type": "paragraph", "content": [text(t)] });
    let marks = [
        "bold",
        "italic",
        "underline",
        "strike",
        "code",
        "subscript",
        "superscript",
    ]
    .map(|m| json!({ "type": "text", "text": m, "marks": [{ "type": m }] }));
    let mut inline = marks.to_vec();
    inline.push(json!({ "type": "hardBreak" }));
    inline.push(json!({ "type": "text", "text": "l", "marks": [
        { "type": "link", "attrs": { "href": "/c/x" } },
    ] }));
    let body = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": inline },
        { "type": "heading", "attrs": { "level": 2 }, "content": [text("H")] },
        { "type": "bulletList", "content": [{ "type": "listItem", "content": [
            para("a"),
            { "type": "orderedList", "content": [{ "type": "listItem", "content": [para("b")] }] },
        ] }] },
        { "type": "blockquote", "content": [para("q")] },
        { "type": "horizontalRule" },
        { "type": "codeBlock", "attrs": { "language": "sql" }, "content": [text("SELECT 1;")] },
        { "type": "table", "content": [{ "type": "tableRow", "content": [
            { "type": "tableHeader", "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null }, "content": [para("h")] },
            { "type": "tableCell", "content": [para("c")] },
        ] }] },
        { "type": "image", "attrs": { "course_file_id": png, "alt": "Map" } },
        { "type": "callout", "attrs": { "kind": "warning" }, "content": [para("w")] },
        { "type": "videoEmbed", "attrs": { "provider": "youtube", "video_id": "dQw4w9WgXcQ", "title": "Intro" } },
        { "type": "transcript", "content": [
            { "type": "transcriptLine", "attrs": { "speaker": "Speaker A" }, "content": [text("Hello")] },
        ] },
    ] });
    assert_schema("PageDoc", &body);
    let created = f
        .create(f.a.course, json!({ "title": "All", "body": body }))
        .await;
    created.ok(StatusCode::CREATED);
    let (page, version) = (created.uuid("/id"), created.uuid("/current_version/id"));
    let stored = f
        .send(
            Method::GET,
            &format!("/api/v1/pages/{page}/versions/{version}"),
            None,
        )
        .await;
    assert_schema("PageVersion", stored.ok(StatusCode::OK));
    assert_schema("PageDoc", &stored.body["body"]);
}

/// T7a (B10, B10a): only PNG, JPEG, GIF and WebP course files of the page's course save as
/// images, by the type read from their bytes; anything else answers 422 `page.image_type` and
/// writes no version, on a save and on a restore.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7a_an_image_must_be_an_image_file_of_the_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let course = f.a.course;
    let (page, mut base) = f.page("Images").await;
    for (name, bytes) in [
        ("a.png", PNG),
        ("b.jpg", JPEG),
        ("c.gif", GIF),
        ("d.webp", WEBP),
    ] {
        let file = f
            .upload(course, name, "application/octet-stream", bytes)
            .await;
        let saved = f.save(page, &image(file), base).await;
        saved.ok(StatusCode::OK);
        base = saved.uuid("/current_version/id");
    }
    let saved = f.versions(page).await;
    assert_eq!(saved, 5);
    let pdf = f.upload(course, "doc.pdf", "application/pdf", PDF).await;
    let refused = [
        pdf,
        f.upload(course, "logo.svg", "image/svg+xml", SVG).await,
        f.upload(course, "notes.txt", "text/plain", TEXT).await,
        f.upload(course, "blob.bin", "application/octet-stream", UNKNOWN)
            .await,
        f.upload(course, "photo.png", "image/png", PDF).await,
    ];
    for file in refused {
        f.save(page, &image(file), base)
            .await
            .invalid_at("/body/content/0/attrs/course_file_id", "page.image_type");
        let created = f
            .create(
                course,
                json!({ "title": format!("I {file}"), "body": image(file) }),
            )
            .await;
        created.invalid_at("/body/content/0/attrs/course_file_id", "page.image_type");
    }
    assert_eq!(f.versions(page).await, saved);

    // A version naming the PDF, written past the route, cannot be restored as current.
    let planted = new_id();
    sqlx::query(
        "INSERT INTO page_versions (id, org_id, course_id, page_id, number, body, author_id)
         VALUES ($1, $2, $6, $3, 99, $4::jsonb, $5)",
    )
    .bind(planted)
    .bind(f.a.org)
    .bind(page)
    .bind(image(pdf).to_string())
    .bind(f.admin_id)
    .bind(course)
    .execute(&f.pool)
    .await
    .expect("plant a version");
    f.restore(page, planted, base)
        .await
        .invalid_at("/body/content/0/attrs/course_file_id", "page.image_type");
    assert_eq!(f.versions(page).await, saved + 1);

    // An image of another course, or without alt text, is refused too.
    let other = f.course(f.a.org, f.a.tower).await;
    let elsewhere = f.upload(other, "e.png", "image/png", PNG).await;
    f.save(page, &image(elsewhere), base)
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    let no_alt = json!({ "type": "doc", "content": [
        { "type": "image", "attrs": { "course_file_id": elsewhere, "alt": "" } },
    ] });
    f.save(page, &no_alt, base)
        .await
        .invalid_at("/body/content/0/attrs/alt", "page.body_invalid");
    assert_eq!(f.versions(page).await, saved + 1);
}

/// Spec 06 B11 with spec 07 Data: a course file any version of a page names as an image cannot
/// be deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_file_a_version_uses_is_in_use(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let file = f.upload(f.a.course, "a.png", "image/png", PNG).await;
    let (page, v1) = f.page("Uses").await;
    let v2 = f
        .save(page, &image(file), v1)
        .await
        .uuid("/current_version/id");
    // No longer current, still named by version 2.
    f.save(page, &paragraph("Text only"), v2)
        .await
        .ok(StatusCode::OK);
    let named: Vec<Uuid> =
        sqlx::query_scalar("SELECT course_file_id FROM page_version_files WHERE version_id = $1")
            .bind(v2)
            .fetch_all(&f.pool)
            .await
            .expect("read the list");
    assert_eq!(named, [file]);
    // The database refuses a version naming another course's file, whatever the code does.
    let other = f.course(f.a.org, f.a.tower).await;
    let elsewhere = f.upload(other, "b.png", "image/png", PNG).await;
    let refused = sqlx::query(
        "INSERT INTO page_version_files (org_id, course_id, version_id, course_file_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(f.a.org)
    .bind(f.a.course)
    .bind(v2)
    .bind(elsewhere)
    .execute(&f.pool)
    .await
    .expect_err("another course's file");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("23503"),
        "{refused}"
    );
    let uri = format!("/api/v1/course-files/{file}");
    f.send(Method::DELETE, &uri, None)
        .await
        .expect(StatusCode::CONFLICT, "file.in_use");
    f.send(Method::DELETE, &format!("/api/v1/pages/{page}"), None)
        .await
        .ok(StatusCode::NO_CONTENT);
    f.send(Method::DELETE, &uri, None)
        .await
        .ok(StatusCode::NO_CONTENT);
}

/// T10 (B6): a page an item references cannot be deleted; otherwise it goes with its versions.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_a_page_in_use_is_not_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Used").await;
    f.save(page, &paragraph("Two"), v1).await.ok(StatusCode::OK);
    let module = modules::create(&f.pool, scope(f.a.org), f.a.course, "M", f.now())
        .await
        .expect("create module")
        .id;
    // `page` items become creatable with spec 07 I2; the row stands in for one.
    let item = new_id();
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, title)
         VALUES ($1, $2, $3, $4, 0, 'page', $5, 'Read this')",
    )
    .bind(item)
    .bind(f.a.org)
    .bind(f.a.course)
    .bind(module)
    .bind(page)
    .execute(&f.pool)
    .await
    .expect("insert a page item");
    let uri = format!("/api/v1/pages/{page}");
    f.send(Method::DELETE, &uri, None)
        .await
        .expect(StatusCode::CONFLICT, "page.in_use");
    assert_eq!(f.versions(page).await, 2);
    sqlx::query("DELETE FROM module_items WHERE id = $1")
        .bind(item)
        .execute(&f.pool)
        .await
        .expect("delete the item");
    f.send(Method::DELETE, &uri, None)
        .await
        .ok(StatusCode::NO_CONTENT);
    assert_eq!(f.versions(page).await, 0);
    f.send(Method::PATCH, &uri, Some(json!({ "title": "Gone" })))
        .await
        .expect(StatusCode::NOT_FOUND, "page.not_found");
}

/// T11 (B5): restoring version 1 makes version 3, equal to version 1; a stale base conflicts,
/// and a version of another page is not found.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_restore_saves_the_old_body_as_a_new_version(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let created = f
        .create(
            f.a.course,
            json!({ "title": "History", "body": paragraph("One") }),
        )
        .await;
    let (page, v1) = (created.uuid("/id"), created.uuid("/current_version/id"));
    let v2 = f
        .save(page, &paragraph("Two"), v1)
        .await
        .uuid("/current_version/id");
    f.restore(page, v1, v1)
        .await
        .expect(StatusCode::CONFLICT, "page.version_conflict");
    let restored = f.restore(page, v1, v2).await;
    assert_schema("PageSummary", restored.ok(StatusCode::OK));
    assert_eq!(restored.body["current_version"]["number"], 3);
    let v3 = restored.uuid("/current_version/id");
    let (one_uri, three_uri) = (
        format!("/api/v1/pages/{page}/versions/{v1}"),
        format!("/api/v1/pages/{page}/versions/{v3}"),
    );
    let one = f.send(Method::GET, &one_uri, None).await;
    let three = f.send(Method::GET, &three_uri, None).await;
    assert_eq!(
        one.ok(StatusCode::OK)["body"],
        three.ok(StatusCode::OK)["body"]
    );
    assert_eq!(three.body["body"], paragraph("One"));

    let (other, other_v1) = f.page("Other").await;
    f.restore(page, other_v1, v3)
        .await
        .expect(StatusCode::NOT_FOUND, "page_version.not_found");
    f.send(
        Method::GET,
        &format!("/api/v1/pages/{page}/versions/{other_v1}"),
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "page_version.not_found");
    assert_eq!(f.versions(other).await, 1);
}

/// B5: versions list newest first, a page at a time; B6: a page's title and slug change.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn versions_list_newest_first_and_pages_list_by_slug(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, mut base) = f.page("Zulu").await;
    for n in 2..=3 {
        let saved = f.save(page, &paragraph(&format!("v{n}")), base).await;
        base = saved.uuid("/current_version/id");
    }
    let first = f
        .send(
            Method::GET,
            &format!("/api/v1/pages/{page}/versions?limit=2"),
            None,
        )
        .await;
    let first = first.ok(StatusCode::OK).clone();
    let numbers = |page: &Value| -> Vec<i64> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["number"].as_i64().unwrap())
            .collect()
    };
    assert_eq!(numbers(&first), [3, 2]);
    for item in first["items"].as_array().unwrap() {
        assert_schema("PageVersionSummary", item);
        assert!(item.get("body").is_none());
    }
    let cursor = first["next_cursor"].as_str().expect("a next page");
    let rest = f
        .send(
            Method::GET,
            &format!("/api/v1/pages/{page}/versions?limit=2&cursor={cursor}"),
            None,
        )
        .await;
    assert_eq!(numbers(rest.ok(StatusCode::OK)), [1]);
    assert!(rest.body["next_cursor"].is_null());

    f.page("Alpha").await;
    let uri = format!("/api/v1/pages/{page}");
    let renamed = f
        .send(
            Method::PATCH,
            &uri,
            Some(json!({ "title": "Mike", "slug": "mike" })),
        )
        .await;
    assert_eq!(renamed.ok(StatusCode::OK)["slug"], "mike");
    assert_eq!(renamed.body["title"], "Mike");
    assert_eq!(renamed.body["current_version"]["id"], base.to_string());
    f.send(Method::PATCH, &uri, Some(json!({ "slug": "alpha" })))
        .await
        .expect(StatusCode::CONFLICT, "page.slug_taken");
    f.send(Method::PATCH, &uri, Some(json!({ "slug": "Bad slug" })))
        .await
        .invalid_at("/slug", "page.slug_invalid");
    let listed = f
        .send(
            Method::GET,
            &format!("/api/v1/courses/{}/pages", f.a.course),
            None,
        )
        .await;
    let slugs: Vec<&str> = listed.ok(StatusCode::OK)["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, ["alpha", "mike"]);
}

/// Spec 02 B5: an archived course takes no page write; its pages still read.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_takes_no_page_write(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Archived").await;
    for status in [courses::Status::Published, courses::Status::Archived] {
        f.set_status(f.a.org, f.a.course, status).await;
    }
    let archived = |answer: Answer| answer.expect(StatusCode::CONFLICT, "course.archived");
    archived(f.create(f.a.course, json!({ "title": "New" })).await);
    archived(f.save(page, &paragraph("x"), v1).await);
    archived(f.restore(page, v1, v1).await);
    let uri = format!("/api/v1/pages/{page}");
    archived(
        f.send(Method::PATCH, &uri, Some(json!({ "title": "y" })))
            .await,
    );
    archived(f.send(Method::DELETE, &uri, None).await);
    f.send(Method::GET, &format!("{uri}/versions"), None)
        .await
        .ok(StatusCode::OK);
}

/// Spec 02 B24, Q13 through `PageTarget`: a page of a draft course answers 404 to a holder of
/// `course.content_edit` without `course.view_all`, and is reached once the course is published.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_draft_courses_pages_need_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Draft").await;
    let editor = f
        .holder(f.a.org, &["course.view", "course.content_edit"])
        .await;
    let uri = format!("/api/v1/pages/{page}");
    let body_uri = format!("{uri}/body");
    let patch = || Some(json!({ "title": "Edited" }));
    f.send_as(f.a.org, &editor, Method::PATCH, &uri, patch())
        .await
        .expect(StatusCode::NOT_FOUND, "page.not_found");
    let save = json!({ "body": paragraph("x"), "base_version_id": v1 });
    f.send_as(f.a.org, &editor, Method::PUT, &body_uri, Some(save.clone()))
        .await
        .expect(StatusCode::NOT_FOUND, "page.not_found");
    let create = format!("/api/v1/courses/{}/pages", f.a.course);
    f.send_as(
        f.a.org,
        &editor,
        Method::POST,
        &create,
        Some(json!({ "title": "N" })),
    )
    .await
    .expect(StatusCode::NOT_FOUND, "course.not_found");
    assert_eq!(f.versions(page).await, 1);

    f.set_status(f.a.org, f.a.course, courses::Status::Published)
        .await;
    f.send_as(f.a.org, &editor, Method::PATCH, &uri, patch())
        .await
        .ok(StatusCode::OK);
    f.send_as(f.a.org, &editor, Method::PUT, &body_uri, Some(save))
        .await
        .ok(StatusCode::OK);
    // Reading versions needs `course.view_all` itself.
    f.send_as(
        f.a.org,
        &editor,
        Method::GET,
        &format!("{uri}/versions"),
        None,
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");
}

/// T15 (B10): org A's admin cannot name org B's course file inside a body, nor reach org B's
/// page or versions; org B's rows are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_another_organizations_ids_are_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let b = tree(
        &f.pool,
        &format!("org-b-{}", &new_id().simple().to_string()[24..]),
        f.now(),
    )
    .await;
    let (b_admin, b_admin_id) = f.builtin(b.org, "org_admin").await;
    let b_file = f
        .upload_in(b.org, &b_admin, b.course, ("b.png", "image/png"), PNG)
        .await;
    let b_page = f
        .send_as(
            b.org,
            &b_admin,
            Method::POST,
            &format!("/api/v1/courses/{}/pages", b.course),
            Some(json!({ "title": "B" })),
        )
        .await;
    let (b_page, b_v1) = (b_page.uuid("/id"), b_page.uuid("/current_version/id"));
    assert_ne!(b_admin_id, f.admin_id);

    let (page, v1) = f.page("A").await;
    f.save(page, &image(b_file), v1)
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    f.create(f.a.course, json!({ "title": "A2", "body": image(b_file) }))
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    f.save(b_page, &paragraph("x"), b_v1)
        .await
        .expect(StatusCode::NOT_FOUND, "page.not_found");
    f.restore(b_page, b_v1, b_v1)
        .await
        .expect(StatusCode::NOT_FOUND, "page.not_found");
    f.send(
        Method::GET,
        &format!("/api/v1/pages/{b_page}/versions/{b_v1}"),
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "page.not_found");
    // Org B's version through org A's page.
    f.restore(page, b_v1, v1)
        .await
        .expect(StatusCode::NOT_FOUND, "page_version.not_found");
    f.send(
        Method::GET,
        &format!("/api/v1/courses/{}/pages", b.course),
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "course.not_found");
    assert_eq!(f.versions(b_page).await, 1);
    assert_eq!(f.versions(page).await, 1);
}

/// Review case: the 1 MiB limit is on the stored form. 9,900 table cells without attributes,
/// under 1 MiB as submitted, are over it once every cell's attributes are written out, so the
/// save is refused rather than stored as a version that could not be saved again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_size_limit_is_on_the_stored_form(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Cells").await;
    let cell = json!({ "type": "tableCell", "content": [{ "type": "paragraph" }] });
    let row = json!({ "type": "tableRow", "content": vec![cell; 9_900] });
    let table = json!({ "type": "table", "content": [row] });
    let text =
        |t: String| json!({ "type": "paragraph", "content": [{ "type": "text", "text": t }] });
    let bare = json!({ "type": "doc", "content": [table.clone(), text("x".into())] });
    let pad = 1_048_576 - bare.to_string().len() - 16;
    let body = json!({ "type": "doc", "content": [table, text("x".repeat(pad + 1))] });
    assert!(body.to_string().len() < 1_048_576);
    f.save(page, &body, v1)
        .await
        .invalid_at("/body", "page.body_invalid");
    assert_eq!(f.versions(page).await, 1);
}

/// Review cases: absolute link targets are stored as the URL parser writes them, and a target
/// that could close an attribute after that (a same-origin path, a `mailto:` address, a host,
/// a `'` in a path) is refused. I2's renderer still escapes
/// every attribute it writes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn link_targets_are_stored_normalized(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Links").await;
    let link = |href: &str| {
        json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "a", "marks": [{ "type": "link", "attrs": { "href": href } }] },
        ] }] })
    };
    for refused in [
        "/\"><svg/onload=alert(1)>",
        "mailto:a@example.com\"><svg/onload=alert(1)>",
        "https://a\"onmouseover=alert(1).example/",
        "https://a.example/it's",
    ] {
        f.save(page, &link(refused), v1).await.invalid_at(
            "/body/content/0/content/0/marks/0/attrs/href",
            "page.body_invalid",
        );
    }
    let saved = f
        .save(
            page,
            &link("https://a.example/\"onmouseover=alert(1)//"),
            v1,
        )
        .await;
    let v2 = saved.uuid("/current_version/id");
    let read = f
        .send(
            Method::GET,
            &format!("/api/v1/pages/{page}/versions/{v2}"),
            None,
        )
        .await;
    let href =
        &read.ok(StatusCode::OK)["body"]["content"][0]["content"][0]["marks"][0]["attrs"]["href"];
    assert_eq!(href, "https://a.example/%22onmouseover=alert(1)//");
}

/// Review case: a body nested deeper than the JSON parser takes (200 bullet lists) answers a
/// client error, and the server keeps answering.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_deeply_nested_body_is_refused_without_a_crash(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Deep").await;
    let mut list = String::from(
        r#"{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph"}]}]}"#,
    );
    for _ in 0..199 {
        list = format!(
            r#"{{"type":"bulletList","content":[{{"type":"listItem","content":[{{"type":"paragraph"}},{list}]}}]}}"#
        );
    }
    let raw = format!(r#"{{"body":{{"type":"doc","content":[{list}]}},"base_version_id":"{v1}"}}"#);
    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/v1/pages/{page}/body"))
        .header(TEST_ORG_HEADER, f.a.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, &f.admin)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(raw))
        .expect("a request");
    let deep = answer(&f.app, request).await;
    assert!(
        matches!(
            deep.status,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ),
        "{deep:?}"
    );
    assert_eq!(f.versions(page).await, 1);
    f.save(page, &paragraph("Still here"), v1)
        .await
        .ok(StatusCode::OK);
}

/// T2 (B3) at the same moment: two saves from one base, sent together, give one new version
/// and one conflict. A transaction holding `page_versions` in `EXCLUSIVE` mode (reads pass,
/// writes wait) holds both saves until it ends. The course lock alone serializes them: the
/// first holds it until it commits, so the second reads the page only after the first's
/// version is current. The page-row lock under it adds nothing for two saves of one page;
/// removing it alone leaves this test green, removing both turns it red.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_simultaneous_saves_from_one_base_give_one_version(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (page, v1) = f.page("Race").await;
    let mut blocker = f.pool.begin().await.expect("a transaction");
    sqlx::query("LOCK TABLE page_versions IN EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .expect("lock the versions");
    let (first, second) = (paragraph("First"), paragraph("Second"));
    let release = async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        blocker.commit().await.expect("release the lock");
    };
    let (a, b, ()) = tokio::join!(f.save(page, &first, v1), f.save(page, &second, v1), release);
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "{a:?} {b:?}"
    );
    let conflict = if a.status == StatusCode::CONFLICT {
        &a
    } else {
        &b
    };
    conflict.expect(StatusCode::CONFLICT, "page.version_conflict");
    assert_eq!(f.versions(page).await, 2);
}

/// Spec 02 B24 with #318's enrollments: an enrolled Student (`course.view` and
/// `course.participate` at the course) gets 403 on every authoring route of a published
/// course's page, and 404 when the course is a draft.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrolled_student_cannot_author_pages(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let draft = f.a.course;
    let published = f.course(f.a.org, f.a.tower).await;
    f.set_status(f.a.org, published, courses::Status::Published)
        .await;
    let (student, user) = f.user(f.a.org).await;
    let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = 'student'")
        .fetch_one(&f.pool)
        .await
        .expect("the student role");
    for course in [draft, published] {
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: role,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(
            &f.pool,
            scope(f.a.org),
            course,
            &new,
            f.now(),
            Actor::Instance,
        )
        .await
        .expect("enroll the student");
    }
    for (course, status, code) in [
        (published, StatusCode::FORBIDDEN, "permission.denied"),
        (draft, StatusCode::NOT_FOUND, "page.not_found"),
    ] {
        let created = f.create(course, json!({ "title": "Read me" })).await;
        let (page, v1) = (created.uuid("/id"), created.uuid("/current_version/id"));
        let uri = format!("/api/v1/pages/{page}");
        let calls = [
            (Method::PATCH, uri.clone(), Some(json!({ "title": "Mine" }))),
            (
                Method::PUT,
                format!("{uri}/body"),
                Some(json!({ "body": paragraph("x"), "base_version_id": v1 })),
            ),
            (Method::DELETE, uri.clone(), None),
            (Method::GET, format!("{uri}/versions"), None),
            (Method::GET, format!("{uri}/versions/{v1}"), None),
            (
                Method::POST,
                format!("{uri}/versions/{v1}/restore"),
                Some(json!({ "base_version_id": v1 })),
            ),
        ];
        for (method, path, body) in calls {
            f.send_as(f.a.org, &student, method.clone(), &path, body)
                .await
                .expect(status, code);
        }
        let course_code = if course == draft {
            "course.not_found"
        } else {
            code
        };
        let list = format!("/api/v1/courses/{course}/pages");
        f.send_as(f.a.org, &student, Method::GET, &list, None)
            .await
            .expect(status, course_code);
        f.send_as(
            f.a.org,
            &student,
            Method::POST,
            &list,
            Some(json!({ "title": "Mine" })),
        )
        .await
        .expect(status, course_code);
        assert_eq!(f.versions(page).await, 1);
    }
}
