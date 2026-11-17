// SPDX-License-Identifier: AGPL-3.0-or-later
//! The enrollment routes and course grants over the router
//! (docs/specs/m1/02-courses-and-enrollments.md B4 to B6, B15 to B22, B24a, B27; T5, T6, T11 to
//! T17, T19, T19b, T20, T21 for these routes). The permission suite's generated cases for each
//! route, the cross-organization case included, are in `tests/permissions.rs`; T18a is in
//! `tests/role_assignments.rs`.
//!
//! Every request names its organization with the `test-support` header, since some tests hold
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
    audit::{self, Actor},
    clock::ManualClock,
    courses, enrollments,
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

fn course_uri(id: Uuid) -> String {
    format!("/api/v1/courses/{id}")
}

fn enrollments_uri(course: Uuid) -> String {
    format!("/api/v1/courses/{course}/enrollments")
}

fn enrollment_uri(id: Uuid) -> String {
    format!("/api/v1/enrollments/{id}")
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

    fn ids(&self, member: &str) -> Vec<Uuid> {
        self.body["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|item| {
                let value = if member.is_empty() {
                    &item["id"]
                } else {
                    &item[member]["id"]
                };
                value
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

/// An organization with Tower and Enroute under the root.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    tower: Uuid,
    enroute: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    a: Tree,
    b: Tree,
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
        let b = tree(&pool, "org-b").await;
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            pool,
            a,
            b,
        }
    }

    /// A new user of `org` named `name`, signed in: their cookie and id.
    async fn user(&self, org: Uuid, name: &str) -> (String, Uuid) {
        self.user_with_email(org, name, None).await
    }

    async fn user_with_email(
        &self,
        org: Uuid,
        name: &str,
        email: Option<String>,
    ) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email,
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

    async fn builtin(&self, key: &str) -> Uuid {
        roles::builtin_id(&self.pool, key)
            .await
            .expect("read role")
            .expect("a built-in role")
    }

    async fn custom_role(&self, org: Uuid, keys: &[&str]) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    /// A signed-in user holding `role` at `at`.
    async fn holder(&self, org: Uuid, role: Uuid, at: Scope) -> (String, Uuid) {
        let (cookie, user) = self.user(org, "Holder").await;
        self.assign(org, user, role, at).await;
        (cookie, user)
    }

    /// `role` assigned to `user` at `at` through the repository.
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

    async fn org_admin(&self, org: Uuid) -> String {
        let role = self.builtin("org_admin").await;
        self.holder(org, role, Scope::Organization).await.0
    }

    async fn account_admin(&self, org: Uuid, account: Uuid) -> String {
        let role = self.builtin("account_admin").await;
        self.holder(org, role, Scope::Account(account)).await.0
    }

    /// A course of `org` in `account`, moved through `statuses`.
    async fn course(&self, org: Uuid, account: Uuid, statuses: &[courses::Status]) -> Uuid {
        let new = courses::NewCourse {
            account_id: account,
            title: "Course".into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        let id = courses::create(&self.pool, scope(org), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        for status in statuses {
            courses::set_status(&self.pool, scope(org), id, *status, now(), Actor::Instance)
                .await
                .expect("change status");
        }
        id
    }

    /// A new user active in a new published Tower course with a role of no permissions, so a
    /// `user.view` holder at Tower covers them: their id.
    async fn in_tower(&self, t: Tree) -> Uuid {
        let (_, user) = self.user(t.org, "In Tower").await;
        let course = self
            .course(t.org, t.tower, &[courses::Status::Published])
            .await;
        let role = self.custom_role(t.org, &[]).await;
        self.enroll(t.org, course, user, role).await;
        user
    }

    /// A template copied from a new draft in `account`: its id.
    async fn template(&self, org: Uuid, account: Uuid) -> Uuid {
        let source = self.course(org, account, &[]).await;
        let copy = courses::CourseCopy {
            account_id: account,
            title: None,
            code: format!("T{}", &new_id().simple().to_string()[20..]),
            template: true,
        };
        courses::copy(
            &self.pool,
            scope(org),
            source,
            &copy,
            now(),
            Actor::Instance,
        )
        .await
        .expect("copy as a template")
        .id
    }

    /// An enrollment written by SQL, as the sync API (M4) or an older row could: its id.
    async fn insert_enrollment(
        &self,
        org: Uuid,
        course: Uuid,
        user: Uuid,
        role: Uuid,
        status: &str,
    ) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id, status)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(org)
        .bind(course)
        .bind(user)
        .bind(role)
        .bind(status)
        .execute(&self.pool)
        .await
        .expect("insert enrollment");
        id
    }

    /// `user` enrolled in `course` with `role` through the repository: the enrollment's id.
    async fn enroll(&self, org: Uuid, course: Uuid, user: Uuid, role: Uuid) -> Uuid {
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: role,
            source: Source::Local,
            external_id: None,
        };
        enrollments::create(&self.pool, scope(org), course, &new, now(), Actor::Instance)
            .await
            .expect("enroll")
            .id
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: &str,
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
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        answer(&self.app, request.body(body).expect("a request")).await
    }

    async fn post_enrollment(
        &self,
        org: Uuid,
        cookie: &str,
        course: Uuid,
        user: Uuid,
        role: Uuid,
    ) -> Answer {
        let body = json!({ "user_id": user, "role_id": role });
        self.send(
            Method::POST,
            &enrollments_uri(course),
            org,
            cookie,
            Some(body),
        )
        .await
    }

    async fn set_enrollment(&self, org: Uuid, cookie: &str, id: Uuid, status: &str) -> Answer {
        let body = json!({ "status": status });
        self.send(Method::PATCH, &enrollment_uri(id), org, cookie, Some(body))
            .await
    }

    /// Every enrollment, role assignment and audit entry of `org`, as text.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(e)::text FROM enrollments e WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(r)::text FROM role_assignments r WHERE org_id = $1
             UNION ALL
             SELECT to_jsonb(a)::text FROM audit_events a WHERE org_id = $1
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
    let create = |name: &'static str| async move {
        accounts::create(pool, scope(org), root, name, now(), Actor::Instance)
            .await
            .expect("create account")
            .id
    };
    Tree {
        org,
        tower: create("Tower").await,
        enroute: create("Enroute").await,
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

use courses::Status::{Archived, Published};

// Creating (B15, B17).

/// T11, B17's order and B27's entry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_enrolling_checks_the_user_the_role_and_repeats(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let course = f.course(t.org, t.tower, &[]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let student = f.builtin("student").await;

    let created = f.post_enrollment(t.org, &admin, course, pat, student).await;
    let body = created.ok(StatusCode::CREATED).clone();
    assert_schema("Enrollment", &body);
    let id = created.id();
    assert_eq!(
        created.headers.get(header::LOCATION).map(|v| v.as_bytes()),
        Some(enrollment_uri(id).as_bytes())
    );
    assert_eq!(
        body,
        json!({
            "id": id, "course_id": course, "user": { "id": pat, "display_name": "Pat" },
            "role_id": student, "status": "active", "source": "local", "external_id": null,
            "created_at": "2025-11-13T02:13:20.000000Z",
            "updated_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    assert_eq!(
        f.entries(t.org, "enrollment.create").await,
        [json!({ "after": {
            "course_id": course, "user_id": pat, "role_id": student, "status": "active",
            "source": "local",
        } })]
    );
    let before = f.rows(t.org).await;
    f.post_enrollment(t.org, &admin, course, pat, student)
        .await
        .expect(StatusCode::CONFLICT, "enrollment.exists");
    for key in ["manager", "observer"] {
        let refused = f
            .post_enrollment(t.org, &admin, course, pat, f.builtin(key).await)
            .await;
        refused.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        assert_eq!(
            refused.errors(),
            [("/role_id".into(), "enrollment.role_unavailable".into())],
            "{key}"
        );
    }
    let (_, b_user) = f.user(f.b.org, "Bo").await;
    f.post_enrollment(t.org, &admin, course, b_user, student)
        .await
        .expect(StatusCode::NOT_FOUND, "user.not_found");
    let b_role = f.custom_role(f.b.org, &[]).await;
    f.post_enrollment(t.org, &admin, course, pat, b_role)
        .await
        .expect(StatusCode::NOT_FOUND, "role.not_found");
    f.post_enrollment(t.org, &admin, new_id(), pat, student)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    assert_eq!(
        f.rows(t.org).await,
        before,
        "a refused change writes nothing"
    );

    let instructor = f.builtin("instructor").await;
    f.post_enrollment(t.org, &admin, course, pat, instructor)
        .await
        .ok(StatusCode::CREATED);
    let roster = f
        .send(Method::GET, &enrollments_uri(course), t.org, &admin, None)
        .await;
    assert_schema("Page_Enrollment", roster.ok(StatusCode::OK));
    assert_eq!(roster.ids("user"), [pat, pat], "two roles in one course");
}

/// T12a, T19b (B17, B24a): an account admin enrolls a Student although no assignment can give
/// it `course.participate`; a course-only permission excuses nothing else.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12a_course_only_permissions_are_left_out_of_the_escalation_check(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let tower_admin = f.account_admin(t.org, t.tower).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let pat = f.in_tower(t).await;

    f.post_enrollment(t.org, &tower_admin, course, pat, f.builtin("student").await)
        .await
        .ok(StatusCode::CREATED);
    let stronger = f
        .custom_role(t.org, &["course.participate", "organization.manage"])
        .await;
    let before = f.rows(t.org).await;
    f.post_enrollment(t.org, &tower_admin, course, pat, stronger)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.rows(t.org).await, before);
}

/// T12 (B17): an enrolled Instructor holds `enrollment.manage` at the course through the
/// enrollment, enrolls another Instructor, and cannot hand out `course.manage`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_an_instructor_enrolls_within_their_own_permissions(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let instructor = f.builtin("instructor").await;
    let (cookie, kim) = f.user(t.org, "Kim").await;
    f.enroll(t.org, course, kim, instructor).await;
    // Enrolling someone needs `user.view` covering them (owner decision of 2026-11-17).
    let viewer = f.custom_role(t.org, &["user.view"]).await;
    f.assign(t.org, kim, viewer, Scope::Organization).await;
    let (_, pat) = f.user(t.org, "Pat").await;

    let managing = f
        .custom_role(t.org, &["course.view", "course.manage"])
        .await;
    f.post_enrollment(t.org, &cookie, course, pat, managing)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    f.post_enrollment(t.org, &cookie, course, pat, instructor)
        .await
        .ok(StatusCode::CREATED);
    // The grant is the course's own: in another course of Tower, which Kim sees through the
    // organization's `user.view`, Kim does not hold `enrollment.manage`.
    let other = f.course(t.org, t.tower, &[Published]).await;
    f.post_enrollment(t.org, &cookie, other, pat, instructor)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
}

// Grants (B16, B23, B24).

/// T13, T14, T19 (B16, B23): an active Student enrollment gives `course.view` at its course and
/// not at another course of the account; a permission not grantable at course scope gives
/// nothing; concluding it answers 404 and keeps the row.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_an_active_enrollment_grants_its_course_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let other = f.course(t.org, t.tower, &[Published]).await;
    let (cookie, pat) = f.user(t.org, "Pat").await;
    let id = f
        .enroll(t.org, course, pat, f.builtin("student").await)
        .await;

    let read = f
        .send(Method::GET, &course_uri(course), t.org, &cookie, None)
        .await;
    assert_eq!(
        read.ok(StatusCode::OK)["permissions"],
        json!(["course.participate", "course.view"])
    );
    f.send(Method::GET, &course_uri(other), t.org, &cookie, None)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let listed = f
        .send(Method::GET, "/api/v1/courses", t.org, &cookie, None)
        .await;
    assert_eq!(listed.ids(""), [course]);
    // T19: the Student sees the course, so a write it may not make is 403, not 404.
    let patch = json!({ "title": "Mine" });
    f.send(
        Method::PATCH,
        &course_uri(course),
        t.org,
        &cookie,
        Some(patch),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.send(Method::GET, &enrollments_uri(course), t.org, &cookie, None)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");

    f.set_enrollment(t.org, &admin, id, "concluded")
        .await
        .ok(StatusCode::OK);
    f.send(Method::GET, &course_uri(course), t.org, &cookie, None)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let roster = f
        .send(Method::GET, &enrollments_uri(course), t.org, &admin, None)
        .await;
    assert_eq!(roster.ids(""), [id], "the row remains as history");
    assert_eq!(roster.body["items"][0]["status"], "concluded");

    // T14: `organization.manage` is not grantable at course scope.
    let (cookie, kim) = f.user(t.org, "Kim").await;
    let mixed = f
        .custom_role(t.org, &["organization.manage", "course.view"])
        .await;
    f.enroll(t.org, course, kim, mixed).await;
    let read = f
        .send(Method::GET, &course_uri(course), t.org, &cookie, None)
        .await;
    assert_eq!(
        read.ok(StatusCode::OK)["permissions"],
        json!(["course.view"])
    );
    f.send(
        Method::PATCH,
        "/api/v1/organization",
        t.org,
        &cookie,
        Some(json!({ "name": "Taken" })),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");
}

// Changing and removing (B18, B27).

/// B18, B27, T21: concluding and reactivating each write one entry, the current status none;
/// removing answers 204 with one entry. The same escalation rule applies.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn concluding_reactivating_and_removing_are_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let id = f
        .enroll(t.org, course, pat, f.builtin("student").await)
        .await;

    let concluded = f.set_enrollment(t.org, &admin, id, "concluded").await;
    assert_schema("Enrollment", concluded.ok(StatusCode::OK));
    let before = f.rows(t.org).await;
    f.set_enrollment(t.org, &admin, id, "concluded")
        .await
        .ok(StatusCode::OK);
    assert_eq!(
        f.rows(t.org).await,
        before,
        "the current status writes nothing"
    );
    f.set_enrollment(t.org, &admin, id, "active")
        .await
        .ok(StatusCode::OK);
    assert_eq!(f.entries(t.org, "enrollment.update").await.len(), 2);

    // A Tower admin cannot remove an enrollment whose role holds what it lacks.
    let strong = f.custom_role(t.org, &["course.view", "role.manage"]).await;
    let (_, kim) = f.user(t.org, "Kim").await;
    let strong_id = f.enroll(t.org, course, kim, strong).await;
    let tower_admin = f.account_admin(t.org, t.tower).await;
    f.send(
        Method::DELETE,
        &enrollment_uri(strong_id),
        t.org,
        &tower_admin,
        None,
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.escalation");
    f.set_enrollment(t.org, &tower_admin, strong_id, "concluded")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");

    let removed = f
        .send(
            Method::DELETE,
            &enrollment_uri(id),
            t.org,
            &tower_admin,
            None,
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    assert_eq!(f.entries(t.org, "enrollment.delete").await.len(), 1);
    f.send(
        Method::DELETE,
        &enrollment_uri(id),
        t.org,
        &tower_admin,
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
}

/// T15 (B18): an enrollment an outside source owns is read-only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_an_externally_managed_enrollment_is_read_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let id = new_id();
    sqlx::query(
        "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id, source, external_id)
         VALUES ($1, $2, $3, $4, $5, 'sync', 'ext-1')",
    )
    .bind(id)
    .bind(t.org)
    .bind(course)
    .bind(pat)
    .bind(f.builtin("student").await)
    .execute(&f.pool)
    .await
    .unwrap();
    let before = f.rows(t.org).await;
    f.set_enrollment(t.org, &admin, id, "concluded")
        .await
        .expect(StatusCode::CONFLICT, "enrollment.externally_managed");
    f.send(Method::DELETE, &enrollment_uri(id), t.org, &admin, None)
        .await
        .expect(StatusCode::CONFLICT, "enrollment.externally_managed");
    assert_eq!(f.rows(t.org).await, before);
}

// The course's own rules (B4, B5, Q5).

/// T5, T6: a published course with a Student cannot go back to draft until the enrollment is
/// concluded; an archived course still takes a `PATCH` and refuses a new enrollment.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_t6_learners_keep_a_course_published_and_archived_takes_none(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let student = f.builtin("student").await;
    let id = f.enroll(t.org, course, pat, student).await;
    let status = |to: &'static str| {
        let (f, admin) = (&f, admin.clone());
        async move {
            let uri = format!("{}/status", course_uri(course));
            f.send(
                Method::POST,
                &uri,
                t.org,
                &admin,
                Some(json!({ "status": to })),
            )
            .await
        }
    };
    status("draft")
        .await
        .expect(StatusCode::CONFLICT, "course.has_learners");
    f.set_enrollment(t.org, &admin, id, "concluded")
        .await
        .ok(StatusCode::OK);
    assert_eq!(status("draft").await.ok(StatusCode::OK)["status"], "draft");

    let archived = f.course(t.org, t.tower, &[Published, Archived]).await;
    let patch = json!({ "title": "Kept" });
    f.send(
        Method::PATCH,
        &course_uri(archived),
        t.org,
        &admin,
        Some(patch),
    )
    .await
    .ok(StatusCode::OK);
    f.post_enrollment(t.org, &admin, archived, pat, student)
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
}

/// B17, Q5: a template takes no enrollment whose role holds `course.participate`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_template_takes_no_learner(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let source = f.course(t.org, t.tower, &[]).await;
    let copy = courses::CourseCopy {
        account_id: t.tower,
        title: None,
        code: "TPL".into(),
        template: true,
    };
    let template = courses::copy(&f.pool, scope(t.org), source, &copy, now(), Actor::Instance)
        .await
        .unwrap()
        .id;
    let (_, pat) = f.user(t.org, "Pat").await;
    f.post_enrollment(t.org, &admin, template, pat, f.builtin("student").await)
        .await
        .expect(StatusCode::CONFLICT, "course.template");
    f.post_enrollment(
        t.org,
        &admin,
        template,
        pat,
        f.builtin("course_designer").await,
    )
    .await
    .ok(StatusCode::CREATED);
    // Nor does reactivating a learner enrollment that got into one: 409, and it stays concluded.
    let student = f.builtin("student").await;
    let (_, kim) = f.user(t.org, "Kim").await;
    let concluded = f
        .insert_enrollment(t.org, template, kim, student, "concluded")
        .await;
    f.set_enrollment(t.org, &admin, concluded, "active")
        .await
        .expect(StatusCode::CONFLICT, "course.template");
    let roster = f
        .send(Method::GET, &enrollments_uri(template), t.org, &admin, None)
        .await;
    let statuses: Vec<_> = roster.body["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["id"] == json!(concluded))
        .map(|e| e["status"].clone())
        .collect();
    assert_eq!(statuses, [json!("concluded")]);
}

// Users (B21).

/// T17: an account admin of Tower finds a user whose only link is a Student enrollment in a
/// Tower course, and not one enrolled only in an Enroute course.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_an_enrollment_puts_a_user_in_the_courses_account(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let tower_admin = f.account_admin(t.org, t.tower).await;
    let student = f.builtin("student").await;
    let in_tower = f.course(t.org, t.tower, &[Published]).await;
    let in_enroute = f.course(t.org, t.enroute, &[Published]).await;
    let (_, sam) = f.user(t.org, "Sam").await;
    let (_, lee) = f.user(t.org, "Lee").await;
    let (_, kim) = f.user(t.org, "Kim").await;
    f.enroll(t.org, in_tower, sam, student).await;
    f.enroll(t.org, in_enroute, lee, student).await;
    // Only an active enrollment puts a user in the account (owner decision of 2026-11-17).
    f.insert_enrollment(t.org, in_tower, kim, student, "concluded")
        .await;

    let listed = f
        .send(Method::GET, "/api/v1/users", t.org, &tower_admin, None)
        .await;
    let ids = listed.ids("");
    assert!(ids.contains(&sam), "{listed:?}");
    assert!(!ids.contains(&lee), "{listed:?}");
    assert!(!ids.contains(&kim), "{listed:?}");
}

/// B17 (owner decision of 2026-11-17): enrolling needs `user.view` covering the user, the users
/// list's reach. An account admin of Tower enrolls a user active in a Tower course, and gets 404
/// `user.not_found` for a user in no account, one only in Enroute, one whose only Tower
/// enrollment is concluded, and one that does not exist; `enrollment.manage` without `user.view`
/// gets the same 404 for a user in Tower. Nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn enrolling_needs_user_view_covering_the_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let tower_admin = f.account_admin(t.org, t.tower).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let student = f.builtin("student").await;
    let in_tower = f.in_tower(t).await;
    let (_, nowhere) = f.user(t.org, "Nowhere").await;
    let elsewhere_course = f.course(t.org, t.enroute, &[Published]).await;
    let (_, elsewhere) = f.user(t.org, "Elsewhere").await;
    f.enroll(t.org, elsewhere_course, elsewhere, student).await;
    let other_course = f.course(t.org, t.tower, &[Published]).await;
    let (_, concluded) = f.user(t.org, "Concluded").await;
    f.insert_enrollment(t.org, other_course, concluded, student, "concluded")
        .await;
    let manage_only = f
        .custom_role(t.org, &["enrollment.manage", "course.view"])
        .await;
    let (manager, _) = f.holder(t.org, manage_only, Scope::Account(t.tower)).await;
    let before = f.rows(t.org).await;

    for user in [nowhere, elsewhere, concluded, new_id()] {
        f.post_enrollment(t.org, &tower_admin, course, user, student)
            .await
            .expect(StatusCode::NOT_FOUND, "user.not_found");
    }
    f.post_enrollment(t.org, &manager, course, in_tower, student)
        .await
        .expect(StatusCode::NOT_FOUND, "user.not_found");
    assert_eq!(f.rows(t.org).await, before);
    f.post_enrollment(t.org, &tower_admin, course, in_tower, student)
        .await
        .ok(StatusCode::CREATED);
}

// B24a across the M0 checks (T19b).

/// T19b: with `course.participate` in the catalog, an Organization admin still defines a custom
/// role holding it, enrolls a Student, makes an enrollment link for a user holding a Student
/// enrollment and changes the local sign-in settings; a role holding `course.participate` and a
/// permission the caller lacks at organization scope is still refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19b_an_organization_admin_passes_every_check_that_asks_for_everything(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let admin = f.org_admin(t.org).await;
    let new_role = json!({
        "key": "participant", "name": "Participant", "permissions": ["course.participate"],
    });
    let role = f
        .send(
            Method::POST,
            "/api/v1/admin/roles",
            t.org,
            &admin,
            Some(new_role),
        )
        .await;
    role.ok(StatusCode::CREATED);

    let course = f.course(t.org, t.tower, &[Published]).await;
    let email = Some("pat@example.test".to_owned());
    let (_, pat) = f.user_with_email(t.org, "Pat", email).await;
    f.post_enrollment(t.org, &admin, course, pat, f.builtin("student").await)
        .await
        .ok(StatusCode::CREATED);
    let link = format!("/api/v1/admin/users/{pat}/enrollment-links");
    f.send(Method::POST, &link, t.org, &admin, None)
        .await
        .ok(StatusCode::CREATED);
    let settings = json!({
        "enabled": true, "passkey": true, "password": true, "totp": true,
        "magic_link": false, "totp_required": false,
    });
    f.send(
        Method::PUT,
        "/api/v1/admin/local-sign-in",
        t.org,
        &admin,
        Some(settings),
    )
    .await
    .ok(StatusCode::OK);

    let role_manager = f.custom_role(t.org, &["role.manage"]).await;
    let (manager, _) = f.holder(t.org, role_manager, Scope::Organization).await;
    let stronger = json!({
        "key": "stronger", "name": "Stronger",
        "permissions": ["course.participate", "user.manage"],
    });
    f.send(
        Method::POST,
        "/api/v1/admin/roles",
        t.org,
        &manager,
        Some(stronger),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.escalation");
    let participant = json!({
        "key": "participant_two", "name": "Participant", "permissions": ["course.participate"],
    });
    f.send(
        Method::POST,
        "/api/v1/admin/roles",
        t.org,
        &manager,
        Some(participant),
    )
    .await
    .ok(StatusCode::CREATED);
}

// Drafts and templates (B24, Q13).

/// T19a for the enrollment routes: a caller holding `enrollment.view` and `enrollment.manage`
/// but not `course.view_all` gets 404 on a draft course's enrollments and on an enrollment in
/// it, and the rows are unchanged; once the course is published the same caller lists them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19a_a_draft_hides_its_enrollments_without_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let draft = f.course(t.org, t.tower, &[]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let student = f.builtin("student").await;
    let id = f.enroll(t.org, draft, pat, student).await;
    let role = f
        .custom_role(
            t.org,
            &["enrollment.view", "enrollment.manage", "course.view"],
        )
        .await;
    let (cookie, _) = f.holder(t.org, role, Scope::Account(t.tower)).await;
    let before = f.rows(t.org).await;

    f.send(Method::GET, &enrollments_uri(draft), t.org, &cookie, None)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.post_enrollment(t.org, &cookie, draft, pat, f.builtin("instructor").await)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.set_enrollment(t.org, &cookie, id, "concluded")
        .await
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    f.send(Method::DELETE, &enrollment_uri(id), t.org, &cookie, None)
        .await
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    assert_eq!(f.rows(t.org).await, before);

    courses::set_status(
        &f.pool,
        scope(t.org),
        draft,
        Published,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let roster = f
        .send(Method::GET, &enrollments_uri(draft), t.org, &cookie, None)
        .await;
    assert_eq!(roster.ids(""), [id]);
}

/// T19a for the enrollment routes on a template, which is never published: a caller without
/// `course.view_all` gets the same 404s as on a draft, and the rows are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19a_a_template_hides_its_enrollments_without_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let template = f.template(t.org, t.tower).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let designer = f.builtin("course_designer").await;
    let id = f.enroll(t.org, template, pat, designer).await;
    let role = f
        .custom_role(
            t.org,
            &["enrollment.view", "enrollment.manage", "course.view"],
        )
        .await;
    let (cookie, _) = f.holder(t.org, role, Scope::Account(t.tower)).await;
    let before = f.rows(t.org).await;

    f.send(
        Method::GET,
        &enrollments_uri(template),
        t.org,
        &cookie,
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.post_enrollment(t.org, &cookie, template, pat, f.builtin("instructor").await)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.set_enrollment(t.org, &cookie, id, "concluded")
        .await
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    f.send(Method::DELETE, &enrollment_uri(id), t.org, &cookie, None)
        .await
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    assert_eq!(f.rows(t.org).await, before);
}

/// B24, Q13: a course made a draft after the extractor's check and before the write's
/// transaction locks it is hidden by the write's own check: 404, and nothing changes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_made_a_draft_during_a_write_hides_it(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let id = f
        .enroll(t.org, course, pat, f.builtin("student").await)
        .await;
    let role = f
        .custom_role(t.org, &["enrollment.manage", "course.view"])
        .await;
    let (cookie, _) = f.holder(t.org, role, Scope::Account(t.tower)).await;
    let mut drafting = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM courses WHERE id = $1 FOR NO KEY UPDATE")
        .bind(course)
        .execute(&mut *drafting)
        .await
        .unwrap();
    let concluding = tokio::spawn({
        let app = f.app.clone();
        let request = Request::patch(enrollment_uri(id))
            .header(TEST_ORG_HEADER, t.org.to_string())
            .header(header::COOKIE, cookie)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "status": "concluded" }).to_string()))
            .unwrap();
        async move { answer(&app, request).await }
    });
    until_a_call_waits_on_a_lock(&f.pool).await;
    sqlx::query("UPDATE courses SET status = 'draft' WHERE id = $1")
        .bind(course)
        .execute(&mut *drafting)
        .await
        .unwrap();
    drafting.commit().await.unwrap();

    concluding
        .await
        .expect("the change ran")
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    let status: String = sqlx::query_scalar("SELECT status FROM enrollments WHERE id = $1")
        .bind(id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "active");
}

/// `m0/03` B6 with enrollments (B24a): a `user.manage` holder without course permissions at
/// organization scope cannot change the email of a user enrolled as Instructor, whose
/// enrollment gives permissions the holder lacks; a user enrolled only with `course.participate`
/// is no escalation.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrolled_instructors_email_needs_their_course_permissions(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let manager_role = f.custom_role(t.org, &["user.manage", "user.view"]).await;
    let (manager, _) = f.holder(t.org, manager_role, Scope::Organization).await;
    let (_, kim) = f.user(t.org, "Kim").await;
    f.enroll(t.org, course, kim, f.builtin("instructor").await)
        .await;
    let (_, pat) = f.user(t.org, "Pat").await;
    let participant = f.custom_role(t.org, &["course.participate"]).await;
    f.enroll(t.org, course, pat, participant).await;
    let email = |user: Uuid, address: &str| {
        let (f, manager) = (&f, manager.clone());
        let body = json!({ "email": address });
        async move {
            f.send(
                Method::PATCH,
                &format!("/api/v1/users/{user}"),
                t.org,
                &manager,
                Some(body),
            )
            .await
        }
    };

    email(kim, "kim@example.test")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    email(pat, "pat@example.test").await.ok(StatusCode::OK);
}

// Organizations (T20).

/// T20: org A's admin naming org B's course, enrollment, user or role gets the route's 404, and
/// org B's rows are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_the_enrollment_routes_refuse_another_organizations_ids(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.a, f.b);
    let admin = f.org_admin(a.org).await;
    let student = f.builtin("student").await;
    let b_course = f.course(b.org, b.tower, &[Published]).await;
    let (_, b_user) = f.user(b.org, "Bo").await;
    let b_enrollment = f.enroll(b.org, b_course, b_user, student).await;
    let a_course = f.course(a.org, a.tower, &[Published]).await;
    let (_, pat) = f.user(a.org, "Pat").await;
    let b_role = f.custom_role(b.org, &["course.view"]).await;
    let before = f.rows(b.org).await;

    f.send(Method::GET, &enrollments_uri(b_course), a.org, &admin, None)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.post_enrollment(a.org, &admin, b_course, pat, student)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.post_enrollment(a.org, &admin, a_course, b_user, student)
        .await
        .expect(StatusCode::NOT_FOUND, "user.not_found");
    f.post_enrollment(a.org, &admin, a_course, pat, b_role)
        .await
        .expect(StatusCode::NOT_FOUND, "role.not_found");
    f.set_enrollment(a.org, &admin, b_enrollment, "concluded")
        .await
        .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    f.send(
        Method::DELETE,
        &enrollment_uri(b_enrollment),
        a.org,
        &admin,
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "enrollment.not_found");
    assert_eq!(f.rows(b.org).await, before);
}

// Concurrency (B19, T16).

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

/// T16: a role `PATCH` that adds a permission, holding the role's row while an enrollment
/// create checks escalation, makes the create wait, and the create checks the role as changed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_role_strengthened_during_an_enrollment_is_checked_as_strengthened(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let t = f.a;
    let tower_admin = f.account_admin(t.org, t.tower).await;
    let course = f.course(t.org, t.tower, &[Published]).await;
    let pat = f.in_tower(t).await;
    let role = f.custom_role(t.org, &["course.view"]).await;
    let mut changing = f.pool.begin().await.unwrap();
    roles::lock_custom(&mut changing, scope(t.org), role)
        .await
        .unwrap();
    let enrolling = tokio::spawn({
        let app = f.app.clone();
        let body = json!({ "user_id": pat, "role_id": role });
        let request = Request::post(enrollments_uri(course))
            .header(TEST_ORG_HEADER, t.org.to_string())
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
        permissions: Some(vec!["course.view".into(), "role.manage".into()]),
    };
    roles::update_custom(
        &mut *changing,
        scope(t.org),
        role,
        &patch,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();
    changing.commit().await.unwrap();

    assert_eq!(
        enrolling.await.expect("the enrollment ran"),
        StatusCode::FORBIDDEN
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM enrollments WHERE role_id = $1")
        .bind(role)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}
