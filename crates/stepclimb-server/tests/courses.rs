// SPDX-License-Identifier: AGPL-3.0-or-later
//! The course routes over the router (docs/specs/m1/02-courses-and-enrollments.md B1 to B11,
//! B24, B27; T1 to T9, T7a, T19 and T20 for the course routes). The permission suite's
//! generated cases for each route are in `tests/permissions.rs`.
//!
//! Enrollments arrive with the spec's issue I2, so a learner here is a Student assigned at an
//! account, which gives `course.view` over the account's courses as an enrollment will over one
//! course. Every request names its organization with the `test-support` header, since most
//! tests hold two organizations. Needs `DATABASE_URL` (`just up`).

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
    audit::{self, Actor},
    clock::ManualClock,
    courses,
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

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const COURSES: &str = "/api/v1/courses";

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

fn one(id: Uuid) -> String {
    format!("{COURSES}/{id}")
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

    fn id(&self) -> Uuid {
        self.body["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a UUID")
    }

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

/// An organization with Tower and Enroute under the root and Ground under Tower.
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

    /// A signed-in user holding the built-in role `key` at `at`.
    async fn holder(&self, org: Uuid, key: &str, at: Scope) -> String {
        let (cookie, user) = self.user(org).await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(org, user, role, at).await;
        cookie
    }

    /// A signed-in user holding a custom role with exactly `keys` at each scope of `at`.
    async fn custom_holder(&self, org: Uuid, keys: &[&str], at: &[Scope]) -> String {
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
        for at in at {
            self.assign(org, user, role, *at).await;
        }
        cookie
    }

    async fn org_admin(&self, org: Uuid) -> String {
        self.holder(org, "org_admin", Scope::Organization).await
    }

    /// A course of `org` in `account` with the code `code`, created through the repository.
    async fn course(&self, org: Uuid, account: Uuid, code: &str) -> Uuid {
        let new = courses::NewCourse {
            account_id: account,
            title: format!("Course {code}"),
            code: code.into(),
        };
        courses::create(&self.pool, scope(org), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id
    }

    async fn set_status(&self, org: Uuid, id: Uuid, statuses: &[courses::Status]) {
        for status in statuses {
            courses::set_status(&self.pool, scope(org), id, *status, now(), Actor::Instance)
                .await
                .expect("change status");
        }
    }

    async fn published(&self, org: Uuid, account: Uuid, code: &str) -> Uuid {
        let id = self.course(org, account, code).await;
        self.set_status(org, id, &[courses::Status::Published])
            .await;
        id
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

    async fn create(&self, org: Uuid, cookie: &str, body: Value) -> Answer {
        self.send(Method::POST, COURSES, org, cookie, Some(body))
            .await
    }

    async fn patch(&self, org: Uuid, cookie: &str, id: Uuid, body: Value) -> Answer {
        self.send(Method::PATCH, &one(id), org, cookie, Some(body))
            .await
    }

    async fn status(&self, org: Uuid, cookie: &str, id: Uuid, status: &str) -> Answer {
        let uri = format!("{}/status", one(id));
        let body = json!({ "status": status });
        self.send(Method::POST, &uri, org, cookie, Some(body)).await
    }

    async fn move_to(&self, org: Uuid, cookie: &str, id: Uuid, account: Uuid) -> Answer {
        let uri = format!("{}/move", one(id));
        let body = json!({ "account_id": account });
        self.send(Method::POST, &uri, org, cookie, Some(body)).await
    }

    /// Every course and audit entry of `org`, as text.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(c)::text FROM courses c WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(e)::text FROM audit_events e WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(a)::text FROM accounts a WHERE org_id = $1
             ORDER BY 1",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read rows")
    }

    /// The `data` of each audit entry of `org` with `action`.
    async fn entries(&self, org: Uuid, action: &str) -> Vec<Value> {
        audit::list(&self.pool, scope(org), 1000)
            .await
            .expect("entries")
            .into_iter()
            .filter(|e| e.action == action)
            .map(|e| e.data)
            .collect()
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

// Creating (B1, B2).

/// T1.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_an_account_admin_creates_a_draft_in_its_subtree_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let created = f
        .create(
            t.org,
            &admin,
            json!({ "account_id": t.tower, "title": " S1 ", "code": "S1" }),
        )
        .await;
    let body = created.ok(StatusCode::CREATED);
    assert_schema("Course", body);
    assert_eq!(body["status"], "draft");
    assert_eq!(body["title"], "S1");
    assert_eq!(body["template"], false);
    assert_eq!(body["settings"], json!({ "home": { "kind": "modules" } }));
    assert_eq!(
        (&body["start_at"], &body["end_at"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(
        created
            .headers
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap()),
        Some(one(created.id()).as_str())
    );
    assert_eq!(
        f.entries(t.org, "course.create").await,
        [json!({ "after": {
            "account_id": t.tower, "title": "S1", "code": "S1", "template": false,
        } })]
    );

    f.create(
        t.org,
        &admin,
        json!({ "account_id": t.enroute, "title": "E", "code": "E1" }),
    )
    .await
    .expect(StatusCode::NOT_FOUND, "account.not_found");
    let blank = f
        .create(
            t.org,
            &admin,
            json!({ "account_id": t.tower, "title": "  ", "code": "B" }),
        )
        .await;
    blank.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(blank.errors(), [("/title".into(), "name.empty".into())]);
    let long = "a".repeat(201);
    let too_long = f
        .create(
            t.org,
            &admin,
            json!({ "account_id": t.tower, "title": long, "code": "L" }),
        )
        .await;
    assert_eq!(
        too_long.errors(),
        [("/title".into(), "name.too_long".into())]
    );
    let code = "c".repeat(51);
    let long_code = f
        .create(
            t.org,
            &admin,
            json!({ "account_id": t.tower, "title": "C", "code": code }),
        )
        .await;
    assert_eq!(
        long_code.errors(),
        [("/code".into(), "text.too_long".into())]
    );
    assert_eq!(
        f.entries(t.org, "course.create").await.len(),
        1,
        "refusals write none"
    );
}

/// T3.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_codes_collide_ignoring_case_within_an_organization_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, tree(&f.pool, "org-b").await);
    let admin_a = f.org_admin(a.org).await;
    let admin_b = f.org_admin(b.org).await;
    let course = |account, code| json!({ "account_id": account, "title": "S", "code": code });
    f.create(a.org, &admin_a, course(a.tower, "S1"))
        .await
        .ok(StatusCode::CREATED);
    f.create(a.org, &admin_a, course(a.enroute, "s1"))
        .await
        .expect(StatusCode::CONFLICT, "course.code_taken");
    f.create(b.org, &admin_b, course(b.tower, "S1"))
        .await
        .ok(StatusCode::CREATED);
}

/// T2: a create that waits on a move of its account out of the caller's subtree.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_create_in_an_account_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let body = json!({ "account_id": t.ground, "title": "Ramp", "code": "R1" }).to_string();
    let request = f.request(Method::POST, COURSES, t.org, &admin, Some(&body));
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM courses")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "no course was created");
}

// Changing (B3 to B5).

/// T4.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_patch_rules(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let id = f.course(t.org, t.tower, "S1").await;
    let before = f.get(&one(id), t.org, &admin).await.body;

    let same = json!({ "title": "Course S1", "code": "S1", "start_at": null,
                       "settings": { "home": { "kind": "modules" } } });
    let unchanged = f.patch(t.org, &admin, id, same).await;
    assert_eq!(unchanged.ok(StatusCode::OK), &before);
    assert!(f.entries(t.org, "course.update").await.is_empty());

    let dates =
        json!({ "start_at": "2026-03-01T00:00:00+01:00", "end_at": "2026-04-01T00:00:00Z" });
    let dated = f.patch(t.org, &admin, id, dates).await;
    let body = dated.ok(StatusCode::OK);
    assert_schema("Course", body);
    assert_eq!(body["start_at"], "2026-02-28T23:00:00.000000Z");
    assert_eq!(
        f.entries(t.org, "course.update").await,
        [json!({
            "before": { "start_at": null, "end_at": null },
            "after": { "start_at": "2026-02-28T23:00:00Z", "end_at": "2026-04-01T00:00:00Z" },
        })]
    );

    let inverted = f
        .patch(
            t.org,
            &admin,
            id,
            json!({ "end_at": "2026-01-01T00:00:00Z" }),
        )
        .await;
    inverted.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        inverted.errors(),
        [("/end_at".into(), "course.dates_inverted".into())]
    );
    let early = f
        .patch(
            t.org,
            &admin,
            id,
            json!({ "start_at": "1969-12-31T00:00:00Z" }),
        )
        .await;
    assert_eq!(
        early.errors(),
        [("/start_at".into(), "course.date_out_of_range".into())]
    );
    for malformed in [
        json!({ "title": null }),
        json!({ "settings": null }),
        json!({ "start_at": "next week" }),
        json!({ "status": "published" }),
    ] {
        f.patch(t.org, &admin, id, malformed)
            .await
            .expect(StatusCode::BAD_REQUEST, "request.malformed");
    }
    let cleared = f.patch(t.org, &admin, id, json!({ "end_at": null })).await;
    assert_eq!(cleared.ok(StatusCode::OK)["end_at"], Value::Null);
}

/// T5, without B4's learners rule, which needs enrollments (issue I2).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_status_changes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let id = f.course(t.org, t.tower, "S1").await;
    f.status(t.org, &admin, id, "archived")
        .await
        .expect(StatusCode::CONFLICT, "course.status_invalid");
    for status in ["published", "archived", "published", "draft"] {
        let changed = f.status(t.org, &admin, id, status).await;
        assert_eq!(changed.ok(StatusCode::OK)["status"], status);
    }
    let same = f.status(t.org, &admin, id, "draft").await;
    assert_eq!(same.ok(StatusCode::OK)["status"], "draft");
    assert_eq!(f.entries(t.org, "course.set_status").await.len(), 4);
    f.status(t.org, &admin, id, "deleted")
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");

    let template = f.course(t.org, t.tower, "T1").await;
    sqlx::query("UPDATE courses SET template = true WHERE id = $1")
        .bind(template)
        .execute(&f.pool)
        .await
        .unwrap();
    f.status(t.org, &admin, template, "published")
        .await
        .expect(StatusCode::CONFLICT, "course.template");
}

/// T6, without the enrollment half (issue I2): an archived course takes a patch.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_an_archived_course_takes_a_patch(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let id = f.course(t.org, t.tower, "S1").await;
    use courses::Status::{Archived, Published};
    f.set_status(t.org, id, &[Published, Archived]).await;
    let patched = f
        .patch(t.org, &admin, id, json!({ "title": "Old S1" }))
        .await;
    assert_eq!(patched.ok(StatusCode::OK)["title"], "Old S1");
}

/// A patch that waits on a move of the course's account out of the caller's subtree.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_patch_of_a_course_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let id = f.course(t.org, t.ground, "S1").await;
    let body = json!({ "title": "Renamed" }).to_string();
    let request = f.request(Method::PATCH, &one(id), t.org, &admin, Some(&body));
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let course = courses::get(&f.pool, scope(t.org), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(course.title, "Course S1");
}

// Accounts that hold courses (B8, B9).

/// T7.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_an_account_holding_a_course_refuses_deletion(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    f.course(t.org, t.enroute, "S1").await;
    let before = f.rows(t.org).await;
    f.send(
        Method::DELETE,
        &format!("/api/v1/accounts/{}", t.enroute),
        t.org,
        &admin,
        None,
    )
    .await
    .expect(StatusCode::CONFLICT, "account.has_courses");
    assert_eq!(f.rows(t.org).await, before);
}

/// T7a.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7a_an_archived_course_moves_out_and_the_account_is_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let id = f.course(t.org, t.ground, "S1").await;
    use courses::Status::{Archived, Published};
    f.set_status(t.org, id, &[Published, Archived]).await;
    let moved = f.move_to(t.org, &admin, id, t.enroute).await;
    let body = moved.ok(StatusCode::OK);
    assert_schema("Course", body);
    assert_eq!(body["account_id"], t.enroute.to_string());
    assert_eq!(
        f.entries(t.org, "course.move").await,
        [json!({ "before": { "account_id": t.ground }, "after": { "account_id": t.enroute } })]
    );
    let in_place = f.move_to(t.org, &admin, id, t.enroute).await;
    in_place.ok(StatusCode::OK);
    assert_eq!(f.entries(t.org, "course.move").await.len(), 1);
    f.send(
        Method::DELETE,
        &format!("/api/v1/accounts/{}", t.ground),
        t.org,
        &admin,
        None,
    )
    .await
    .ok(StatusCode::NO_CONTENT);

    let tower_admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let in_tower = f.course(t.org, t.tower, "S2").await;
    f.move_to(t.org, &tower_admin, in_tower, t.enroute)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    // Visible but without course.create there: the same 404.
    let viewer = f
        .custom_holder(
            t.org,
            &["course.manage", "account.view"],
            &[Scope::Account(t.tower), Scope::Account(t.enroute)],
        )
        .await;
    f.move_to(t.org, &viewer, in_tower, t.enroute)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    f.move_to(t.org, &admin, in_tower, new_id())
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
}

/// T7a's race: a course move to a destination carried out of the caller's subtree meanwhile.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7a_a_move_to_a_destination_carried_out_of_reach_meanwhile_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    let id = f.course(t.org, t.tower, "S1").await;
    let body = json!({ "account_id": t.ground }).to_string();
    let uri = format!("{}/move", one(id));
    let request = f.request(Method::POST, &uri, t.org, &admin, Some(&body));
    racing_a_move(&f, t.ground, t.enroute, request)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");
    let course = courses::get(&f.pool, scope(t.org), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(course.account_id, t.tower);
}

// Reading (B10, B11, B24).

/// T8, with account-scope assignments standing in for enrollments.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_a_draft_is_hidden_from_a_student_and_shown_to_a_designer(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let draft = f.course(t.org, t.tower, "S1").await;
    let student = f.holder(t.org, "student", Scope::Account(t.tower)).await;
    f.get(&one(draft), t.org, &student)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;
    let read = f.get(&one(draft), t.org, &designer).await;
    let body = read.ok(StatusCode::OK);
    assert_schema("Course", body);
    assert_eq!(
        body["permissions"],
        json!(["course.content_edit", "course.view", "course.view_all"])
    );
    let published = f.published(t.org, t.tower, "S2").await;
    let read = f.get(&one(published), t.org, &student).await;
    assert_eq!(
        read.ok(StatusCode::OK)["permissions"],
        json!(["course.view"])
    );
    f.get(&one(new_id()), t.org, &designer)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.get(&format!("{COURSES}/not-a-uuid"), t.org, &designer)
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

/// B10, B11 for a template that is not a draft: an archived template is hidden from a holder
/// of `course.view` alone on `GET` and in the list, shown to a holder of `course.view_all`, and
/// the `template` filter takes it in and out for each.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_template_needs_view_all_on_get_and_in_the_list(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let course = f.published(t.org, t.tower, "S1").await;
    let template = f.course(t.org, t.tower, "T1").await;
    sqlx::query("UPDATE courses SET status = 'archived', template = true WHERE id = $1")
        .bind(template)
        .execute(&f.pool)
        .await
        .unwrap();
    let student = f.holder(t.org, "student", Scope::Account(t.tower)).await;
    let designer = f
        .holder(t.org, "course_designer", Scope::Account(t.tower))
        .await;

    f.get(&one(template), t.org, &student)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let read = f.get(&one(template), t.org, &designer).await;
    let body = read.ok(StatusCode::OK);
    assert_eq!(
        (&body["status"], &body["template"]),
        (&json!("archived"), &json!(true))
    );

    let list = |cookie: String, query: &'static str| {
        let f = &f;
        async move {
            f.get(&format!("{COURSES}{query}"), t.org, &cookie)
                .await
                .ids()
        }
    };
    assert_eq!(list(student.clone(), "").await, [course]);
    assert!(list(student.clone(), "?template=true").await.is_empty());
    assert_eq!(list(student.clone(), "?template=false").await, [course]);
    assert_eq!(list(designer.clone(), "").await, [course, template]);
    assert_eq!(list(designer.clone(), "?template=true").await, [template]);
    assert_eq!(list(designer.clone(), "?template=false").await, [course]);
}

/// B9 with the account move's escalation rule: a move may not give the caller a permission over
/// the course it does not hold there now.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_that_would_widen_the_callers_reach_over_the_course_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let id = f.course(t.org, t.tower, "S1").await;
    let (cookie, user) = f.user(t.org).await;
    let manage = NewRole {
        key: "tower_course_manager".into(),
        name: "Tower course manager".into(),
        permissions: vec!["course.manage".into()],
    };
    let manage = roles::create_custom(&f.pool, scope(t.org), &manage, now(), Actor::Instance)
        .await
        .expect("create role")
        .id;
    f.assign(t.org, user, manage, Scope::Account(t.tower)).await;
    let account_admin =
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = 'account_admin'")
            .fetch_one(&f.pool)
            .await
            .expect("the built-in role");
    f.assign(t.org, user, account_admin, Scope::Account(t.enroute))
        .await;
    let before = f.rows(t.org).await;
    f.move_to(t.org, &cookie, id, t.enroute)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(
        f.rows(t.org).await,
        before,
        "nothing moved and no entry was written"
    );

    // A destination that gives nothing new is allowed.
    let admin = f
        .holder(t.org, "account_admin", Scope::Account(t.root))
        .await;
    f.move_to(t.org, &admin, id, t.enroute)
        .await
        .ok(StatusCode::OK);
}

/// T9, with account-scope assignments standing in for enrollments.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_each_caller_lists_what_it_may_see(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let b = tree(&f.pool, "org-b").await;
    f.published(b.org, b.tower, "B1").await;
    let in_tower = f.published(t.org, t.tower, "A").await;
    let in_enroute = f.published(t.org, t.enroute, "B").await;
    let in_root = f.published(t.org, t.root, "C").await;
    let draft_in_enroute = f.course(t.org, t.enroute, "D").await;

    let admin = f.org_admin(t.org).await;
    let all = f.get(COURSES, t.org, &admin).await;
    assert_schema("Page_Course", all.ok(StatusCode::OK));
    assert_eq!(all.ids(), [in_tower, in_enroute, in_root, draft_in_enroute]);
    let tower_admin = f
        .holder(t.org, "account_admin", Scope::Account(t.tower))
        .await;
    assert_eq!(f.get(COURSES, t.org, &tower_admin).await.ids(), [in_tower]);
    let student = f.holder(t.org, "student", Scope::Account(t.enroute)).await;
    assert_eq!(f.get(COURSES, t.org, &student).await.ids(), [in_enroute]);

    let filtered = |query: String| format!("{COURSES}?{query}");
    let in_enroute_only = f
        .get(
            &filtered(format!("account_id={}", t.enroute)),
            t.org,
            &admin,
        )
        .await;
    assert_eq!(in_enroute_only.ids(), [in_enroute, draft_in_enroute]);
    let drafts = f.get(&filtered("status=draft".into()), t.org, &admin).await;
    assert_eq!(drafts.ids(), [draft_in_enroute]);
    let templates = f
        .get(&filtered("template=true".into()), t.org, &admin)
        .await;
    assert!(templates.ids().is_empty());
    f.get(
        &filtered(format!("account_id={}", t.enroute)),
        t.org,
        &tower_admin,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "account.not_found");
    f.get(&filtered(format!("account_id={}", b.tower)), t.org, &admin)
        .await
        .expect(StatusCode::NOT_FOUND, "account.not_found");

    let first = f.get(&filtered("limit=2".into()), t.org, &admin).await;
    assert_eq!(first.ids(), [in_tower, in_enroute]);
    let cursor = first.body["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();
    let rest = f
        .get(&filtered(format!("limit=2&cursor={cursor}")), t.org, &admin)
        .await;
    assert_eq!(rest.ids(), [in_root, draft_in_enroute]);
    assert_eq!(rest.body["next_cursor"], Value::Null);

    let nobody = f.user(t.org).await.0;
    f.get(COURSES, t.org, &nobody)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
}

/// T19: a learner is refused an edit with 403, not 404.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19_a_student_gets_403_on_patch_of_their_own_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let id = f.published(t.org, t.tower, "S1").await;
    let student = f.holder(t.org, "student", Scope::Account(t.tower)).await;
    let before = f.rows(t.org).await;
    f.patch(t.org, &student, id, json!({ "title": "Mine" }))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.status(t.org, &student, id, "archived")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.move_to(t.org, &student, id, t.tower)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    assert_eq!(f.rows(t.org).await, before);
}

/// T20: an organization admin of org A naming org B's course or account gets the route's 404,
/// and org B's rows are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_every_course_route_refuses_another_organizations_ids(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, tree(&f.pool, "org-b").await);
    let admin = f.org_admin(a.org).await;
    let theirs = f.published(b.org, b.tower, "S1").await;
    let mine = f.course(a.org, a.tower, "S1").await;
    let before_a = f.rows(a.org).await;
    let before_b = f.rows(b.org).await;
    let not_found = |answer: Answer, code: &str| {
        answer.expect(StatusCode::NOT_FOUND, code);
        let text = answer.body.to_string();
        assert!(!text.contains(&theirs.to_string()), "{text}");
    };
    not_found(f.get(&one(theirs), a.org, &admin).await, "course.not_found");
    not_found(
        f.patch(a.org, &admin, theirs, json!({ "title": "X" }))
            .await,
        "course.not_found",
    );
    not_found(
        f.status(a.org, &admin, theirs, "archived").await,
        "course.not_found",
    );
    not_found(
        f.move_to(a.org, &admin, theirs, a.tower).await,
        "course.not_found",
    );
    not_found(
        f.move_to(a.org, &admin, mine, b.tower).await,
        "account.not_found",
    );
    not_found(
        f.create(
            a.org,
            &admin,
            json!({ "account_id": b.tower, "title": "X", "code": "X" }),
        )
        .await,
        "account.not_found",
    );
    let listed = f.get(COURSES, a.org, &admin).await;
    assert_eq!(listed.ids(), [mine]);
    assert_eq!(f.rows(b.org).await, before_b, "org B is unchanged");
    assert_eq!(f.rows(a.org).await, before_a, "org A is unchanged");
}

// The checks under the tree lock.

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

/// Sends `request` while another transaction holds the tree lock and has moved `moved` under
/// `to` without committing; the move commits once the request waits. The request's answer.
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
