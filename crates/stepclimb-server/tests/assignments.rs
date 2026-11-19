// SPDX-License-Identifier: AGPL-3.0-or-later
//! The assignment routes and the completion subscribers over the router
//! (docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B1 to B5, B7 to B9; T1,
//! T3, T8's due changes and T11b). The permission suite's generated cases for each route (T13),
//! the cross-organization case included (T14), are in `tests/permissions.rs`; the repository's
//! own cases are in stepclimb-core's `tests/assignments.rs`.
//!
//! Every request names its organization with the `test-support` header, since each test holds
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
    id::{Uuid, new_id},
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, ModulePatch},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    rules::Requirement,
    users::{self, NewUser},
};
use stepclimb_jobs::{Worker, backoff::Random, outbox::dispatch};
use stepclimb_server::{
    AppState, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// 2026-03-01T10:00:00Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_772_359_200)
}

const DAY: Duration = Duration::from_secs(86_400);

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn assignments_uri(course: Uuid) -> String {
    format!("/api/v1/courses/{course}/assignments")
}

fn assignment_uri(id: Uuid) -> String {
    format!("/api/v1/assignments/{id}")
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

    /// The pointer and code of the first field error.
    fn field_error(&self) -> (String, String) {
        let error = &self.body["errors"][0];
        (
            error["field"].as_str().unwrap_or("").to_owned(),
            error["code"].as_str().unwrap_or("").to_owned(),
        )
    }
}

/// An organization with Tower and Enroute under the root.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    tower: Uuid,
    enroute: Uuid,
}

async fn tree(pool: &PgPool, slug: &str) -> Tree {
    let org = organizations::create(pool, slug, slug, t0())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let create = |name: &'static str| async move {
        accounts::create(pool, scope(org), root, name, t0(), Actor::Instance)
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

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    a: Tree,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, t0()).await.expect("sync roles");
        let clock = ManualClock::new(t0());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
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
        tree(&pool, "org-b").await;
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            clock,
            pool,
            a,
        }
    }

    /// A new user of org A named `name`, signed in: their cookie and id.
    async fn user(&self, name: &str) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", new_id().simple())),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let id = users::create(&self.pool, scope(self.a.org), &new_user, t0())
            .await
            .expect("create user")
            .id;
        (self.sign_in(id).await, id)
    }

    /// A new session of `user` started at the clock's time: its `Cookie` header value.
    async fn sign_in(&self, user: Uuid) -> String {
        let now = self.clock.clock().now();
        let started = session::start(&self.pool, scope(self.a.org), user, &HeaderMap::new(), now)
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn builtin(&self, key: &str) -> Uuid {
        roles::builtin_id(&self.pool, key)
            .await
            .expect("read role")
            .expect("a built-in role")
    }

    async fn custom_role(&self, keys: &[&str]) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(self.a.org), &role, t0(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    /// A signed-in user holding `role` at `at`.
    async fn holder(&self, role: Uuid, at: Scope) -> (String, Uuid) {
        let (cookie, user) = self.user("Holder").await;
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.a.org),
            &assignment,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
        (cookie, user)
    }

    /// A published course of org A in `account`: its id.
    async fn course(&self, account: Uuid) -> Uuid {
        let new = courses::NewCourse {
            account_id: account,
            title: "Course".into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        let id = courses::create(&self.pool, scope(self.a.org), &new, t0(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        let published = courses::Status::Published;
        courses::set_status(
            &self.pool,
            scope(self.a.org),
            id,
            published,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("publish");
        id
    }

    /// `user` enrolled in `course` with the built-in role `role`.
    async fn enroll(&self, course: Uuid, user: Uuid, role: &str) {
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: self.builtin(role).await,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(
            &self.pool,
            scope(self.a.org),
            course,
            &new,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("enroll");
    }

    /// A new user active in a published Tower course with a role of no permissions, so a
    /// `user.view` holder at Tower covers them, and enrolled in no other course: their id.
    async fn in_tower(&self) -> Uuid {
        let (_, user) = self.user("In Tower").await;
        let course = self.course(self.a.tower).await;
        let role = self.custom_role(&[]).await;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: role,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(
            &self.pool,
            scope(self.a.org),
            course,
            &new,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("enroll");
        user
    }

    /// A published module of `course` holding one published link completed by a view: the
    /// module and the item.
    async fn module_with_link(&self, course: Uuid) -> (Uuid, Uuid) {
        let org = scope(self.a.org);
        let module = modules::create(&self.pool, org, course, "Module", t0())
            .await
            .expect("create module")
            .id;
        let publish = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(&self.pool, org, module, &publish, t0())
            .await
            .expect("publish module");
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Brief".into(),
            ref_id: None,
            url: Some("https://example.com/brief".into()),
            position: None,
        };
        let item = items::create(&self.pool, org, module, &new, t0())
            .await
            .expect("create item")
            .id;
        let publish = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(&self.pool, org, item, &publish, t0())
            .await
            .expect("publish item");
        items::set_requirement(&self.pool, org, item, Some(Requirement::View), t0())
            .await
            .expect("set requirement");
        (module, item)
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, self.a.org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a request"))
            .await
            .expect("the router never fails");
        let status = response.status();
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
        Answer { status, body }
    }

    async fn assign(&self, cookie: &str, course: Uuid, target: Value, user: Uuid) -> Answer {
        let body = json!({
            "target": target,
            "assignee": { "type": "user", "id": user },
            "due": { "relative_days": 14 },
        });
        self.send(Method::POST, &assignments_uri(course), cookie, Some(body))
            .await
    }

    async fn assign_course(&self, cookie: &str, course: Uuid, user: Uuid) -> Answer {
        self.assign(
            cookie,
            course,
            json!({ "type": "course", "id": course }),
            user,
        )
        .await
    }

    /// Whether `user` holds an active Student enrollment in `course`.
    async fn is_student(&self, course: Uuid, user: Uuid) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM enrollments e JOIN roles r ON r.id = e.role_id
                            WHERE e.course_id = $1 AND e.user_id = $2 AND r.key = 'student'
                              AND e.status = 'active')",
        )
        .bind(course)
        .bind(user)
        .fetch_one(&self.pool)
        .await
        .expect("read enrollments")
    }

    async fn assignment_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM assignments")
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    /// Dispatch the outbox and run the jobs it enqueued, at the clock's time.
    async fn run_jobs(&self) {
        let registry = stepclimb_server::jobs::registry(None, None).expect("the registry builds");
        let now = self.clock.clock().now();
        dispatch(&self.pool, &registry, now)
            .await
            .expect("dispatch");
        let queues = registry.queues();
        let worker = Worker::new(
            self.pool.clone(),
            registry,
            self.clock.clock(),
            Random::from_fn(|| 0),
            queues,
        );
        worker.run_once(20).await.expect("run jobs");
    }
}

/// T1: a holder of `assignment.manage`, `enrollment.manage`, `user.view` and `course.view` at
/// Tower assigns a course to a user who is not a learner of it: a Student enrollment and one
/// row appear. Without `enrollment.manage` the same request answers 403 and creates nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_assigning_an_unenrolled_user_enrolls_them_with_enrollment_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower = Scope::Account(f.a.tower);
    let course = f.course(f.a.tower).await;
    let pat = f.in_tower().await;
    let lacking = f
        .custom_role(&["assignment.manage", "user.view", "course.view"])
        .await;
    let (lacking, _) = f.holder(lacking, tower).await;
    f.assign_course(&lacking, course, pat)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    assert_eq!(f.assignment_count().await, 0);
    assert!(!f.is_student(course, pat).await);

    let holding = f
        .custom_role(&[
            "assignment.manage",
            "enrollment.manage",
            "user.view",
            "course.view",
        ])
        .await;
    let (holding, holder) = f.holder(holding, tower).await;
    let created = f.assign_course(&holding, course, pat).await;
    let body = created.ok(StatusCode::CREATED).clone();
    assert_schema("Assignment", &body);
    assert_eq!(
        body["assignee"],
        json!({ "type": "user", "id": pat, "name": "In Tower" })
    );
    assert_eq!(body["target"]["type"], "course");
    assert_eq!(body["due"], json!({ "relative_days": 14 }));
    assert_eq!(
        body["counts"],
        json!({ "not_started": 1, "in_progress": 0, "completed": 0, "overdue": 0 })
    );
    assert!(f.is_student(course, pat).await);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'enrollment.create' AND actor_id = $1",
    )
    .bind(holder)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(audited, 1, "the enrollment is the caller's");
}

/// B3 with the enrollment route's rules: an unenrolled user outside the caller's `user.view`
/// answers 404 `user.not_found`, and a caller without `course.view` (a Student permission)
/// cannot enroll anyone as Student (403 `permission.escalation`).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn enrolling_through_an_assignment_follows_the_enrollment_rules(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower = Scope::Account(f.a.tower);
    let course = f.course(f.a.tower).await;
    let all = [
        "assignment.manage",
        "enrollment.manage",
        "user.view",
        "course.view",
    ];
    let (holder, _) = f.holder(f.custom_role(&all).await, tower).await;
    let (_, stranger) = f.user("Nowhere").await;
    f.assign_course(&holder, course, stranger)
        .await
        .expect(StatusCode::NOT_FOUND, "user.not_found");
    f.assign_course(&holder, course, new_id())
        .await
        .expect(StatusCode::NOT_FOUND, "user.not_found");

    let no_view = ["assignment.manage", "enrollment.manage", "user.view"];
    let (no_view, _) = f.holder(f.custom_role(&no_view).await, tower).await;
    let pat = f.in_tower().await;
    f.assign_course(&no_view, course, pat)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.assignment_count().await, 0);
}

/// T11b: the built-in lists. An Account admin of Tower assigns a Tower course to an unenrolled
/// user and gets 404 for an Enroute course; an Instructor assigns a student of their course and
/// gets 403 where they hold only a Student enrollment; a Course designer gets 403.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11b_account_admin_and_instructor_assign_and_course_designer_does_not(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower_course = f.course(f.a.tower).await;
    let enroute_course = f.course(f.a.enroute).await;
    let account_admin = f.builtin("account_admin").await;
    let (admin, _) = f.holder(account_admin, Scope::Account(f.a.tower)).await;
    let pat = f.in_tower().await;
    f.assign_course(&admin, tower_course, pat)
        .await
        .ok(StatusCode::CREATED);
    f.assign_course(&admin, enroute_course, pat)
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");

    let (instructor, instructor_id) = f.user("Instructor").await;
    f.enroll(tower_course, instructor_id, "instructor").await;
    f.enroll(enroute_course, instructor_id, "student").await;
    let (_, student) = f.user("Student").await;
    f.enroll(tower_course, student, "student").await;
    f.enroll(enroute_course, student, "student").await;
    f.assign_course(&instructor, tower_course, student)
        .await
        .ok(StatusCode::CREATED);
    f.assign_course(&instructor, enroute_course, student)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");

    let (designer, designer_id) = f.user("Designer").await;
    f.enroll(tower_course, designer_id, "course_designer").await;
    f.assign_course(&designer, tower_course, student)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
}

/// B3 and the body's rules: an archived course and a template answer 409, a group assignee and
/// a course target other than the path's 422, another course's module 404.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_course_and_the_body_are_checked(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_admin = f.builtin("org_admin").await;
    let (admin, _) = f.holder(org_admin, Scope::Organization).await;
    let course = f.course(f.a.tower).await;
    let other = f.course(f.a.tower).await;
    let (other_module, _) = f.module_with_link(other).await;
    let (_, student) = f.user("Student").await;
    f.enroll(course, student, "student").await;

    let group = json!({
        "target": { "type": "course", "id": course },
        "assignee": { "type": "group", "id": new_id() },
    });
    let refused = f
        .send(Method::POST, &assignments_uri(course), &admin, Some(group))
        .await;
    refused.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        refused.field_error(),
        (
            "/assignee/type".into(),
            "assignment.assignee_unavailable".into()
        )
    );
    let mismatch = f
        .assign(
            &admin,
            course,
            json!({ "type": "course", "id": other }),
            student,
        )
        .await;
    mismatch.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        mismatch.field_error(),
        ("/target/id".into(), "assignment.target_mismatch".into())
    );
    f.assign(
        &admin,
        course,
        json!({ "type": "module", "id": other_module }),
        student,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "module.not_found");
    let bad_due = json!({
        "target": { "type": "course", "id": course },
        "assignee": { "type": "user", "id": student },
        "due": { "relative_days": 0 },
    });
    let refused = f
        .send(
            Method::POST,
            &assignments_uri(course),
            &admin,
            Some(bad_due),
        )
        .await;
    assert_eq!(
        refused.field_error(),
        ("/due/relative_days".into(), "assignment.due_invalid".into())
    );

    let archived = courses::Status::Archived;
    courses::set_status(
        &f.pool,
        scope(f.a.org),
        course,
        archived,
        t0(),
        Actor::Instance,
    )
    .await
    .unwrap();
    f.assign_course(&admin, course, student)
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    sqlx::query("UPDATE courses SET status = 'draft', template = true WHERE id = $1")
        .bind(course)
        .execute(&f.pool)
        .await
        .unwrap();
    f.assign_course(&admin, course, student)
        .await
        .expect(StatusCode::CONFLICT, "course.template");
    assert_eq!(f.assignment_count().await, 0);
}

/// T3 over the router and the jobs: the row is `not_started`, `overdue` once the due date
/// passes, and `completed` when the learner's view of the module's item completes the module
/// and its `module.completed` event reaches the subscriber; the subscriber jobs finish. The
/// `in_progress` step is in stepclimb-core's `tests/assignments.rs`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_completion_event_completes_the_row(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f
        .holder(f.builtin("org_admin").await, Scope::Organization)
        .await;
    let course = f.course(f.a.tower).await;
    let (module, item) = f.module_with_link(course).await;
    let (_, learner) = f.user("Learner").await;
    f.enroll(course, learner, "student").await;
    let body = json!({
        "target": { "type": "module", "id": module },
        "assignee": { "type": "user", "id": learner },
        "due": { "fixed": "2026-03-03T10:00:00Z" },
    });
    let created = f
        .send(Method::POST, &assignments_uri(course), &admin, Some(body))
        .await;
    let id = created.id();
    assert_eq!(created.ok(StatusCode::CREATED)["counts"]["not_started"], 1);

    f.clock.advance(3 * DAY);
    let admin = f.sign_in(admin_id).await;
    let learner_cookie = f.sign_in(learner).await;
    let read = f.send(Method::GET, &assignment_uri(id), &admin, None).await;
    assert_eq!(read.ok(StatusCode::OK)["counts"]["overdue"], 1);

    let step = f
        .send(
            Method::POST,
            &format!("/api/v1/items/{item}/progress"),
            &learner_cookie,
            Some(json!({ "action": "view" })),
        )
        .await;
    step.ok(StatusCode::OK);
    f.run_jobs().await;
    let read = f.send(Method::GET, &assignment_uri(id), &admin, None).await;
    let body = read.ok(StatusCode::OK);
    assert_schema("Assignment", body);
    assert_eq!(
        body["counts"],
        json!({ "not_started": 0, "in_progress": 0, "completed": 1, "overdue": 0 }),
        "completed late is completed (B9)"
    );
    let completed_at: Option<i64> = sqlx::query_scalar(
        "SELECT (extract(epoch FROM completed_at))::bigint FROM assignment_users
         WHERE assignment_id = $1",
    )
    .bind(id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        completed_at,
        Some(
            i64::try_from(
                (t0() + 3 * DAY)
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            )
            .unwrap()
        )
    );
    f.run_jobs().await;
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(jobs, 0, "every subscriber job finished");
}

/// B5 over the router: `PATCH` moves the due date of an open row, `end` ends the assignment
/// once, an ended assignment refuses a change, and the list leaves it out unless asked.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_due_changes_and_ending(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f
        .holder(f.builtin("org_admin").await, Scope::Organization)
        .await;
    let course = f.course(f.a.tower).await;
    let (_, student) = f.user("Student").await;
    f.enroll(course, student, "student").await;
    let id = f.assign_course(&admin, course, student).await.id();

    let moved = f
        .send(
            Method::PATCH,
            &assignment_uri(id),
            &admin,
            Some(json!({ "due": { "fixed": "2026-04-01T09:00:00-04:00" } })),
        )
        .await;
    assert_eq!(
        moved.ok(StatusCode::OK)["due"],
        json!({ "fixed": "2026-04-01T13:00:00.000000Z" })
    );
    let cleared = f
        .send(
            Method::PATCH,
            &assignment_uri(id),
            &admin,
            Some(json!({ "due": null })),
        )
        .await;
    assert_eq!(cleared.ok(StatusCode::OK)["due"], Value::Null);
    let unchanged = f
        .send(Method::PATCH, &assignment_uri(id), &admin, Some(json!({})))
        .await;
    assert_eq!(unchanged.ok(StatusCode::OK)["due"], Value::Null);

    let list = f
        .send(Method::GET, &assignments_uri(course), &admin, None)
        .await;
    assert_schema("Page_Assignment", list.ok(StatusCode::OK));
    assert_eq!(list.body["items"].as_array().map(Vec::len), Some(1));

    let end_uri = format!("{}/end", assignment_uri(id));
    f.clock.advance(DAY);
    let admin = f.sign_in(admin_id).await;
    let ended = f.send(Method::POST, &end_uri, &admin, None).await;
    let ended_at = ended.ok(StatusCode::OK)["ended_at"].clone();
    assert_eq!(ended_at, "2026-03-02T10:00:00.000000Z");
    f.clock.advance(DAY);
    let admin = f.sign_in(admin_id).await;
    let again = f.send(Method::POST, &end_uri, &admin, None).await;
    assert_eq!(again.ok(StatusCode::OK)["ended_at"], ended_at);
    f.send(
        Method::PATCH,
        &assignment_uri(id),
        &admin,
        Some(json!({ "due": { "relative_days": 3 } })),
    )
    .await
    .expect(StatusCode::CONFLICT, "assignment.ended");

    let open = f
        .send(Method::GET, &assignments_uri(course), &admin, None)
        .await;
    assert_eq!(open.ok(StatusCode::OK)["items"], json!([]));
    let all = f
        .send(
            Method::GET,
            &format!("{}?include_ended=true", assignments_uri(course)),
            &admin,
            None,
        )
        .await;
    assert_eq!(all.ok(StatusCode::OK)["items"][0]["id"], id.to_string());
}

/// Data: deleting an assigned item answers 409 `item.assigned`, and an assigned module 409
/// `module.assigned`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_assigned_item_or_module_cannot_be_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f
        .holder(f.builtin("org_admin").await, Scope::Organization)
        .await;
    let course = f.course(f.a.tower).await;
    let (_, item) = f.module_with_link(course).await;
    let (_, student) = f.user("Student").await;
    f.enroll(course, student, "student").await;
    f.assign(
        &admin,
        course,
        json!({ "type": "item", "id": item }),
        student,
    )
    .await
    .ok(StatusCode::CREATED);
    f.send(
        Method::DELETE,
        &format!("/api/v1/items/{item}"),
        &admin,
        None,
    )
    .await
    .expect(StatusCode::CONFLICT, "item.assigned");

    let empty = modules::create(&f.pool, scope(f.a.org), course, "Empty", t0())
        .await
        .unwrap()
        .id;
    f.assign(
        &admin,
        course,
        json!({ "type": "module", "id": empty }),
        student,
    )
    .await
    .ok(StatusCode::CREATED);
    f.send(
        Method::DELETE,
        &format!("/api/v1/modules/{empty}"),
        &admin,
        None,
    )
    .await
    .expect(StatusCode::CONFLICT, "module.assigned");
}
