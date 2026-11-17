// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recording progress over the router, its outbox events and the learner's own progress in
//! the course read (docs/specs/m1/05-progress-and-events.md B1 to B3, B10 to B13, B16; T1 to T3,
//! T5, T9, T10 for `GET /api/v1/courses/{course_id}`, T13, and T14's explicit cases for
//! `POST /api/v1/items/{item_id}/progress`), and the staff and Observer reads, Observer
//! enrollments and enrollment deletion (B14, B15, B17 to B20; T11, T12, T14a, T15 to T18). The
//! repository's own cases (T4, T6, T7) are in `stepclimb-core`'s `tests/progress.rs`; the
//! generated permission and cross-organization cases in `tests/permissions.rs`.
//!
//! Every request names its organization with the `test-support` header, since tests hold two
//! organizations. Needs `DATABASE_URL` (`just up`).

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
    modules::{self, ModulePatch, RulesPatch},
    organizations,
    perm::{self, Permission},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    rules::Requirement,
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// A display name, an email and a title no event may carry (T9).
const NAME: &str = "Quentin Progress-Learner";
const EMAIL_DOMAIN: &str = "progress-learner.example.test";
const TITLE: &str = "Approach Briefing Title";

/// 2025-11-13T02:13:20Z: the manual clock's time.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

const NOW_RFC3339: &str = "2025-11-13T02:13:20.000000Z";

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn progress_uri(item: Uuid) -> String {
    format!("/api/v1/items/{item}/progress")
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
    fn ok(&self) -> &Value {
        assert_eq!(self.status, StatusCode::OK, "{self:?}");
        &self.body
    }

    #[track_caller]
    fn ok_with(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }

    #[track_caller]
    fn expect_status(&self, status: StatusCode) {
        assert_eq!(self.status, status, "{self:?}");
    }
}

/// An organization with Tower and Enroute under the root, and a published course in Tower
/// holding one published module.
#[derive(Clone, Copy, Debug)]
struct Org {
    id: Uuid,
    tower: Uuid,
    enroute: Uuid,
    course: Uuid,
    module: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
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
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            pool,
        }
    }

    async fn org(&self, slug: &str) -> Org {
        let pool = &self.pool;
        let id = organizations::create(pool, slug, slug, now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let create = |name: &'static str| async move {
            accounts::create(pool, scope(id), root, name, now(), Actor::Instance)
                .await
                .expect("create account")
                .id
        };
        let (tower, enroute) = (create("Tower").await, create("Enroute").await);
        let course = self.course(id, tower, &[courses::Status::Published]).await;
        let module = self.module(id, course).await;
        Org {
            id,
            tower,
            enroute,
            course,
            module,
        }
    }

    async fn course(&self, org: Uuid, account: Uuid, statuses: &[courses::Status]) -> Uuid {
        let new = courses::NewCourse {
            account_id: account,
            title: TITLE.into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        let id = courses::create(&self.pool, scope(org), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        for status in statuses {
            self.set_course_status(org, id, *status).await;
        }
        id
    }

    async fn set_course_status(&self, org: Uuid, course: Uuid, status: courses::Status) {
        courses::set_status(
            &self.pool,
            scope(org),
            course,
            status,
            now(),
            Actor::Instance,
        )
        .await
        .expect("change status");
    }

    async fn module(&self, org: Uuid, course: Uuid) -> Uuid {
        let id = modules::create(&self.pool, scope(org), course, TITLE, now())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(&self.pool, scope(org), id, &patch, now())
            .await
            .expect("publish module");
        id
    }

    /// A published external link, last in `module`, with `requirement`.
    async fn link(&self, org: Uuid, module: Uuid, requirement: Option<Requirement>) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: TITLE.into(),
            ref_id: None,
            url: Some("https://example.com/brief".into()),
            position: None,
        };
        let id = items::create(&self.pool, scope(org), module, &new, now())
            .await
            .expect("create item")
            .id;
        let patch = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(&self.pool, scope(org), id, &patch, now())
            .await
            .expect("publish item");
        items::set_requirement(&self.pool, scope(org), id, requirement, now())
            .await
            .expect("set requirement");
        id
    }

    /// A new user of `org`, signed in: their cookie and id.
    async fn user(&self, org: Uuid) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: NAME.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@{EMAIL_DOMAIN}", new_id().simple())),
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

    /// A new user enrolled in `course` with the built-in role `role`: cookie, user, enrollment.
    async fn enrolled(&self, org: Uuid, course: Uuid, role: &str) -> (String, Uuid, Uuid) {
        let (cookie, user) = self.user(org).await;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: self.builtin(role).await,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        let enrollment =
            enrollments::create(&self.pool, scope(org), course, &new, now(), Actor::Instance)
                .await
                .expect("enroll")
                .id;
        (cookie, user, enrollment)
    }

    async fn student(&self, org: Uuid, course: Uuid) -> (String, Uuid) {
        let (cookie, user, _) = self.enrolled(org, course, "student").await;
        (cookie, user)
    }

    /// A new user holding every catalog permission through a custom role assigned at `at`.
    async fn holder_of_everything(&self, org: Uuid, at: Scope) -> String {
        let (cookie, user) = self.user(org).await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Everything".into(),
            permissions: perm::ALL.iter().map(|p| p.key().to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
            .await
            .expect("assign");
        cookie
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

    async fn step(&self, org: Uuid, cookie: &str, item: Uuid, action: &str) -> Answer {
        let body = json!({ "action": action });
        self.send(Method::POST, &progress_uri(item), org, cookie, Some(body))
            .await
    }

    /// The outbox payloads of `org`, oldest first.
    async fn events(&self, org: Uuid) -> Vec<(String, i32, Value)> {
        sqlx::query_as("SELECT topic, version, payload FROM outbox WHERE org_id = $1 ORDER BY id")
            .bind(org)
            .fetch_all(&self.pool)
            .await
            .expect("read the outbox")
    }

    /// Every progress row and outbox row of `org`, as text.
    async fn rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(p)::text FROM item_progress p WHERE org_id = $1
             UNION ALL SELECT to_jsonb(m)::text FROM module_progress m WHERE org_id = $1
             UNION ALL SELECT to_jsonb(c)::text FROM course_progress c WHERE org_id = $1
             UNION ALL SELECT to_jsonb(o)::text FROM outbox o WHERE org_id = $1
             ORDER BY 1",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read rows")
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_only_a_learner_records_progress(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f
        .link(org.id, org.module, Some(Requirement::MarkDone))
        .await;
    let (designer, _, _) = f.enrolled(org.id, org.course, "course_designer").await;
    f.step(org.id, &designer, item, "view")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let (student, _) = f.student(org.id, org.course).await;
    let answer = f.step(org.id, &student, item, "view").await;
    let body = answer.ok();
    assert_eq!(body["state"], "started");
    assert_eq!(body["item_id"], item.to_string());
    assert_eq!(body["started_at"], NOW_RFC3339);
    assert_eq!(body["completed_at"], Value::Null);
    assert_schema("ItemProgress", body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_steps_complete_their_requirement_and_others_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let view = f.link(org.id, org.module, Some(Requirement::View)).await;
    let done = f
        .link(org.id, org.module, Some(Requirement::MarkDone))
        .await;
    let (student, _) = f.student(org.id, org.course).await;
    let answer = f.step(org.id, &student, view, "view").await;
    assert_eq!(answer.ok()["state"], "completed");
    assert_eq!(answer.ok()["completed_at"], NOW_RFC3339);
    let refused = f.step(org.id, &student, view, "mark_done").await;
    refused.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        refused.body["errors"],
        json!([{ "field": "/action", "code": "progress.action_invalid" }])
    );
    assert_eq!(
        f.step(org.id, &student, done, "view").await.ok()["state"],
        "started"
    );
    assert_eq!(
        f.step(org.id, &student, done, "mark_done").await.ok()["state"],
        "completed"
    );
    f.step(org.id, &student, done, "unmark")
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_locked_item_and_an_archived_course_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let later = f.module(org.id, org.course).await;
    f.link(org.id, org.module, Some(Requirement::MarkDone))
        .await;
    let waiting = f.link(org.id, later, Some(Requirement::View)).await;
    let rules = RulesPatch {
        prerequisite_ids: Some(vec![org.module]),
        ..RulesPatch::default()
    };
    modules::set_rules(&f.pool, scope(org.id), later, &rules, now())
        .await
        .expect("set rules");
    let (student, _) = f.student(org.id, org.course).await;
    let before = f.rows(org.id).await;
    let locked = f.step(org.id, &student, waiting, "view").await;
    locked.expect(StatusCode::FORBIDDEN, "item.locked");
    assert_eq!(locked.body["reasons"], json!([{ "kind": "module_locked" }]));
    assert_eq!(f.rows(org.id).await, before, "no row is written");
    f.set_course_status(org.id, org.course, courses::Status::Archived)
        .await;
    f.step(org.id, &student, waiting, "view")
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_draft_course_hides_its_items_from_a_learner(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let draft = f.course(org.id, org.tower, &[]).await;
    let module = f.module(org.id, draft).await;
    let item = f.link(org.id, module, Some(Requirement::View)).await;
    let (student, _) = f.student(org.id, draft).await;
    f.step(org.id, &student, item, "view")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_t9_completions_write_one_event_each_with_ids_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let first = f
        .link(org.id, org.module, Some(Requirement::MarkDone))
        .await;
    let last = f.link(org.id, org.module, Some(Requirement::View)).await;
    let (student, user) = f.student(org.id, org.course).await;
    f.step(org.id, &student, first, "view").await.ok();
    assert!(
        f.events(org.id).await.is_empty(),
        "T9: a start writes no event"
    );
    f.step(org.id, &student, first, "mark_done").await.ok();
    let after_first = f.events(org.id).await;
    assert_eq!(after_first.len(), 1, "{after_first:?}");
    f.step(org.id, &student, last, "view").await.ok();
    let events = f.events(org.id).await;
    let topics: Vec<(&str, i32)> = events.iter().map(|(t, v, _)| (t.as_str(), *v)).collect();
    assert_eq!(
        topics,
        [
            ("item.completed", 1),
            ("item.completed", 1),
            ("module.completed", 1),
            ("course.completed", 1),
        ]
    );
    let (u, c, m) = (
        user.to_string(),
        org.course.to_string(),
        org.module.to_string(),
    );
    assert_eq!(
        events[1].2,
        json!({
            "type": "item.completed",
            "timestamp": NOW_RFC3339,
            "data": {
                "user_id": u, "course_id": c, "module_id": m, "item_id": last.to_string(),
                "item_kind": "external_link", "requirement_kind": "view",
                "completed_at": NOW_RFC3339,
            },
        })
    );
    assert_eq!(
        events[2].2["data"],
        json!({ "user_id": u, "course_id": c, "module_id": m, "completed_at": NOW_RFC3339 })
    );
    assert_eq!(
        events[3].2["data"],
        json!({ "user_id": u, "course_id": c, "completed_at": NOW_RFC3339 })
    );
    for ((_, _, payload), schema) in events.iter().zip([
        "ItemCompletedEvent",
        "ItemCompletedEvent",
        "ModuleCompletedEvent",
        "CourseCompletedEvent",
    ]) {
        assert_schema(schema, payload);
        let text = payload.to_string();
        for secret in [NAME, EMAIL_DOMAIN, TITLE] {
            assert!(!text.contains(secret), "T9: {secret} in {text}");
        }
    }
    f.step(org.id, &student, last, "view").await.ok();
    assert_eq!(f.events(org.id).await.len(), 4, "repeating writes none");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_the_course_read_carries_only_the_learners_own_progress(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let course_uri = format!("/api/v1/courses/{}", org.course);
    let (one, _) = f.student(org.id, org.course).await;
    let (two, _) = f.student(org.id, org.course).await;
    let fresh = f.send(Method::GET, &course_uri, org.id, &one, None).await;
    assert_eq!(fresh.ok()["progress"], Value::Null);
    assert!(fresh.ok().as_object().unwrap().contains_key("progress"));
    f.step(org.id, &one, item, "view").await.ok();
    let read = f.send(Method::GET, &course_uri, org.id, &one, None).await;
    assert_eq!(
        read.ok()["progress"],
        json!({ "required_count": 1, "completed_count": 1, "completed_at": NOW_RFC3339 })
    );
    assert_schema("Course", read.ok());
    let other = f.send(Method::GET, &course_uri, org.id, &two, None).await;
    assert_eq!(other.ok()["progress"], Value::Null, "another learner's");
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let staff = f.send(Method::GET, &course_uri, org.id, &admin, None).await;
    assert!(
        !staff.ok().as_object().unwrap().contains_key("progress"),
        "absent for one who is not a learner: {:?}",
        staff.body
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_an_item_with_a_started_row_is_not_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f
        .link(org.id, org.module, Some(Requirement::MarkDone))
        .await;
    let (student, _) = f.student(org.id, org.course).await;
    f.step(org.id, &student, item, "view").await.ok();
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    f.send(
        Method::DELETE,
        &format!("/api/v1/items/{item}"),
        org.id,
        &admin,
        None,
    )
    .await
    .expect(StatusCode::CONFLICT, "item.has_progress");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_holders_without_an_active_participating_enrollment_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let before = f.rows(org.id).await;
    // Every grantable permission through an account assignment, which cannot hold
    // `course.participate`.
    let account = f
        .holder_of_everything(org.id, Scope::Account(org.tower))
        .await;
    f.step(org.id, &account, item, "view")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let (concluded, _, enrollment) = f.enrolled(org.id, org.course, "student").await;
    enrollments::set_status(
        &f.pool,
        scope(org.id),
        enrollment,
        enrollments::Status::Concluded,
        now(),
        Actor::Instance,
    )
    .await
    .expect("conclude");
    f.step(org.id, &concluded, item, "view")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
    let sibling = f
        .holder_of_everything(org.id, Scope::Account(org.enroute))
        .await;
    f.step(org.id, &sibling, item, "view")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
    assert_eq!(f.rows(org.id).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_another_organizations_item_is_not_found_and_untouched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.org("a").await, f.org("b").await);
    let item = f.link(b.id, b.module, Some(Requirement::View)).await;
    let (b_student, _) = f.student(b.id, b.course).await;
    f.step(b.id, &b_student, item, "view").await.ok();
    let before = f.rows(b.id).await;
    let admin = f.holder_of_everything(a.id, Scope::Organization).await;
    f.step(a.id, &admin, item, "view")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
    // A learner of org A naming org B's item.
    let (a_student, _) = f.student(a.id, a.course).await;
    f.step(a.id, &a_student, item, "view")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
    assert_eq!(f.rows(b.id).await, before);
}

/// The course check runs again on the course row the step's transaction holds `FOR SHARE`: a
/// course made a draft after the extractor passed answers 404, and nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_made_a_draft_before_the_transaction_reads_it_answers_404(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let (student, _) = f.student(org.id, org.course).await;
    let before = f.rows(org.id).await;
    // Hold the course row as a content write does, so the step's extractor passes on the
    // published course and its transaction then waits for the row.
    let mut writer = f.pool.begin().await.expect("begin");
    sqlx::query("SELECT 1 FROM courses WHERE id = $1 FOR NO KEY UPDATE")
        .bind(org.course)
        .execute(&mut *writer)
        .await
        .expect("lock the course");
    let request = Request::builder()
        .method(Method::POST)
        .uri(progress_uri(item))
        .header(TEST_ORG_HEADER, org.id.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, student)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "action": "view" }).to_string()))
        .expect("a request");
    let step = tokio::spawn(f.app.clone().oneshot(request));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!step.is_finished(), "the step waits for the course row");
    // A status change the API refuses while the course has learners, made by SQL to stand in
    // for any change that hides the course between the extractor and the transaction.
    sqlx::query("UPDATE courses SET status = 'draft' WHERE id = $1")
        .bind(org.course)
        .execute(&mut *writer)
        .await
        .expect("make the course a draft");
    writer.commit().await.expect("commit");
    let response = step
        .await
        .expect("the task")
        .expect("the router never fails");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("a JSON body");
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("item.not_found")),
        "{body}"
    );
    assert_eq!(f.rows(org.id).await, before, "nothing is written");
}

// Staff and Observer reads, Observer enrollments and enrollment deletion (I3: B14, B15, B17 to
// B20; T11, T12, T14a, T15 to T18).

fn list_uri(course: Uuid) -> String {
    format!("/api/v1/courses/{course}/progress")
}

fn detail_uri(course: Uuid, user: Uuid) -> String {
    format!("/api/v1/courses/{course}/progress/{user}")
}

/// The user ids of a progress page, in order.
fn learners(page: &Value) -> Vec<Uuid> {
    page["items"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|l| {
            l["user"]["id"]
                .as_str()
                .expect("an id")
                .parse()
                .expect("a UUID")
        })
        .collect()
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

impl Fixture {
    async fn get(&self, org: Uuid, cookie: &str, uri: &str) -> Answer {
        self.send(Method::GET, uri, org, cookie, None).await
    }

    /// A new user holding a custom role of exactly `keys`, enrolled in `course`: their cookie.
    async fn enrolled_with(&self, org: Uuid, course: Uuid, keys: &[&str]) -> String {
        let (cookie, user) = self.user(org).await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Course role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: role,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(&self.pool, scope(org), course, &new, now(), Actor::Instance)
            .await
            .expect("enroll");
        cookie
    }

    /// `POST` an Observer enrollment of a new user naming `observed`, as `admin`: the answer
    /// and the Observer's cookie.
    async fn enroll_observer(
        &self,
        org: Uuid,
        admin: &str,
        course: Uuid,
        observed: Value,
    ) -> (Answer, String) {
        let (cookie, user) = self.user(org).await;
        let body = json!({
            "user_id": user,
            "role_id": self.builtin("observer").await,
            "observed_user_ids": observed,
        });
        let uri = format!("/api/v1/courses/{course}/enrollments");
        let answer = self.send(Method::POST, &uri, org, admin, Some(body)).await;
        (answer, cookie)
    }

    /// The learner progress rows of `user` in `course`, as text.
    async fn progress_of(&self, user: Uuid, course: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(p)::text FROM item_progress p WHERE user_id = $1 AND course_id = $2
             UNION ALL SELECT to_jsonb(m)::text FROM module_progress m
                       WHERE user_id = $1 AND course_id = $2
             UNION ALL SELECT to_jsonb(c)::text FROM course_progress c
                       WHERE user_id = $1 AND course_id = $2
             ORDER BY 1",
        )
        .bind(user)
        .bind(course)
        .fetch_all(&self.pool)
        .await
        .expect("read rows")
    }

    async fn observee_links(&self, org: Uuid) -> Vec<(Uuid, Uuid)> {
        sqlx::query_as(
            "SELECT enrollment_id, user_id FROM enrollment_observees WHERE org_id = $1
             ORDER BY 1, 2",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read links")
    }

    async fn audit_data(&self, org: Uuid, action: &str) -> Vec<Value> {
        sqlx::query_scalar(
            "SELECT data FROM audit_events WHERE org_id = $1 AND action = $2 ORDER BY id",
        )
        .bind(org)
        .bind(action)
        .fetch_all(&self.pool)
        .await
        .expect("read audit")
    }
}

/// T11 (B14): an Instructor lists every learner with their progress, `?status` filters over
/// their enrollment, the detail carries item rows, and a user who is not a learner answers 404
/// `progress.not_found`. An Account admin of the course's account reads the same.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_staff_read_every_learner(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let (instructor, instructor_id, _) = f.enrolled(org.id, org.course, "instructor").await;
    let (one_cookie, one) = f.student(org.id, org.course).await;
    let (_, two, two_enrollment) = f.enrolled(org.id, org.course, "student").await;
    f.step(org.id, &one_cookie, item, "view").await.ok();
    enrollments::set_status(
        &f.pool,
        scope(org.id),
        two_enrollment,
        enrollments::Status::Concluded,
        now(),
        Actor::Instance,
    )
    .await
    .expect("conclude");

    let page = f.get(org.id, &instructor, &list_uri(org.course)).await;
    let page = page.ok();
    assert_eq!(learners(page), sorted(vec![one, two]));
    assert_schema("LearnerProgress", &page["items"][0]);
    let first = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["user"]["id"] == one.to_string())
        .unwrap();
    assert_eq!(first["enrollment_status"], "active");
    assert_eq!(first["user"]["display_name"], NAME);
    assert_eq!(first["course"]["completed_count"], 1);
    assert_eq!(first["modules"][0]["module_id"], org.module.to_string());
    let uri = format!("{}?status=concluded", list_uri(org.course));
    assert_eq!(learners(f.get(org.id, &instructor, &uri).await.ok()), [two]);
    let uri = format!("{}?status=active", list_uri(org.course));
    assert_eq!(learners(f.get(org.id, &instructor, &uri).await.ok()), [one]);

    let detail = f
        .get(org.id, &instructor, &detail_uri(org.course, one))
        .await;
    let detail = detail.ok();
    assert_schema("LearnerProgressDetail", detail);
    assert_eq!(detail["items"][0]["item_id"], item.to_string());
    assert_eq!(detail["items"][0]["state"], "completed");
    let detail = f
        .get(org.id, &instructor, &detail_uri(org.course, two))
        .await;
    assert_eq!(detail.ok()["course"], Value::Null);
    let (_, stranger) = f.user(org.id).await;
    for user in [instructor_id, stranger, new_id()] {
        f.get(org.id, &instructor, &detail_uri(org.course, user))
            .await
            .expect(StatusCode::NOT_FOUND, "progress.not_found");
    }

    let (admin, admin_id) = f.user(org.id).await;
    let assignment = NewAssignment {
        user_id: admin_id,
        role_id: f.builtin("account_admin").await,
        scope: Scope::Account(org.tower),
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(&f.pool, scope(org.id), &assignment, now(), Actor::Instance)
        .await
        .expect("assign");
    let page = f.get(org.id, &admin, &list_uri(org.course)).await;
    assert_eq!(learners(page.ok()), sorted(vec![one, two]));
}

/// T14a (B20): a Student (holding `course.view` and `course.participate`), a holder of only
/// `course.view`, and a Course designer get 403 `permission.denied` on both staff routes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14a_learners_and_viewers_are_refused_staff_reads(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (student, student_id) = f.student(org.id, org.course).await;
    let viewer = f
        .enrolled_with(org.id, org.course, &[perm::CourseView::KEY])
        .await;
    let (designer, _, _) = f.enrolled(org.id, org.course, "course_designer").await;
    for cookie in [&student, &viewer, &designer] {
        for uri in [list_uri(org.course), detail_uri(org.course, student_id)] {
            f.get(org.id, cookie, &uri)
                .await
                .expect(StatusCode::FORBIDDEN, "permission.denied");
        }
    }
}

/// T16 (B17, B19): enrolling an Observer names learners of the course; the list's rules, the
/// `progress.view` check on naming and the Student refusal.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_observer_enrollments_name_learners(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let (_, one) = f.student(org.id, org.course).await;
    let (_, two) = f.student(org.id, org.course).await;
    let (_, outsider) = f.user(org.id).await;

    let (created, _) = f
        .enroll_observer(org.id, &admin, org.course, json!([two, one]))
        .await;
    let body = created.ok_with(StatusCode::CREATED);
    assert_eq!(body["observed_user_ids"], json!(sorted(vec![one, two])));
    assert_schema("Enrollment", body);
    let created_audit = f.audit_data(org.id, "enrollment.create").await;
    assert_eq!(
        created_audit.last().unwrap()["after"]["observed_user_ids"],
        json!(sorted(vec![one, two]))
    );

    let invalid = |answer: Answer, field: &str| {
        answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        assert_eq!(answer.body["errors"][0]["field"], field, "{answer:?}");
        assert_eq!(
            answer.body["errors"][0]["code"], "enrollment.observed_invalid",
            "{answer:?}"
        );
    };
    let (answer, _) = f
        .enroll_observer(org.id, &admin, org.course, json!([one, outsider]))
        .await;
    invalid(answer, "/observed_user_ids/1");
    let (answer, _) = f
        .enroll_observer(org.id, &admin, org.course, json!([one, one]))
        .await;
    invalid(answer, "/observed_user_ids/1");
    let (answer, _) = f
        .enroll_observer(org.id, &admin, org.course, json!([]))
        .await;
    invalid(answer, "/observed_user_ids");
    let many: Vec<Uuid> = (0..101).map(|_| one).collect();
    let (answer, _) = f
        .enroll_observer(org.id, &admin, org.course, json!(many))
        .await;
    invalid(answer, "/observed_user_ids");
    // An Observer without the list, and a Student with one.
    let (_, user) = f.user(org.id).await;
    let uri = format!("/api/v1/courses/{}/enrollments", org.course);
    let observer = json!({ "user_id": user, "role_id": f.builtin("observer").await });
    invalid(
        f.send(Method::POST, &uri, org.id, &admin, Some(observer))
            .await,
        "/observed_user_ids",
    );
    let student = json!({
        "user_id": user,
        "role_id": f.builtin("student").await,
        "observed_user_ids": [one],
    });
    invalid(
        f.send(Method::POST, &uri, org.id, &admin, Some(student))
            .await,
        "/observed_user_ids",
    );

    // `enrollment.manage` without `progress.view`: naming learners is an escalation (B19).
    let (manager, manager_id) = f.user(org.id).await;
    let role = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Enroller".into(),
        permissions: vec![
            perm::EnrollmentManage::KEY.to_owned(),
            perm::UserView::KEY.to_owned(),
            perm::CourseView::KEY.to_owned(),
        ],
    };
    let role = roles::create_custom(&f.pool, scope(org.id), &role, now(), Actor::Instance)
        .await
        .expect("create role")
        .id;
    // At the organization, so its `user.view` covers the new Observer (spec 02 B17).
    let assignment = NewAssignment {
        user_id: manager_id,
        role_id: role,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(&f.pool, scope(org.id), &assignment, now(), Actor::Instance)
        .await
        .expect("assign");
    let before = f.observee_links(org.id).await;
    let (answer, _) = f
        .enroll_observer(org.id, &manager, org.course, json!([one]))
        .await;
    answer.expect(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.observee_links(org.id).await, before);
    // The same caller still enrolls a Student.
    let (_, three) = f.user(org.id).await;
    let student = json!({ "user_id": three, "role_id": f.builtin("student").await });
    f.send(Method::POST, &uri, org.id, &manager, Some(student))
        .await
        .ok_with(StatusCode::CREATED);
}

/// B17, B19 on `PATCH`: the list is replaced whole; naming learners or reactivating an Observer
/// needs `progress.view`; a Student enrollment takes no list.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn observed_learners_are_replaced_by_patch(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let (_, one) = f.student(org.id, org.course).await;
    let (_, three, three_enrollment) = f.enrolled(org.id, org.course, "student").await;
    let (created, observer) = f
        .enroll_observer(org.id, &admin, org.course, json!([one]))
        .await;
    let observer_enrollment = created.body["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/v1/enrollments/{observer_enrollment}");

    let patched = f
        .send(
            Method::PATCH,
            &uri,
            org.id,
            &admin,
            Some(json!({ "observed_user_ids": [three] })),
        )
        .await;
    assert_eq!(patched.ok()["observed_user_ids"], json!([three]));
    let audit = f.audit_data(org.id, "enrollment.update").await;
    assert_eq!(
        audit.last().unwrap(),
        &json!({
            "before": { "observed_user_ids": [one] },
            "after": { "observed_user_ids": [three] },
        })
    );
    let page = f.get(org.id, &observer, &list_uri(org.course)).await;
    assert_eq!(learners(page.ok()), [three]);
    // The same list again writes nothing.
    let count = audit.len();
    f.send(
        Method::PATCH,
        &uri,
        org.id,
        &admin,
        Some(json!({ "observed_user_ids": [three] })),
    )
    .await
    .ok();
    assert_eq!(f.audit_data(org.id, "enrollment.update").await.len(), count);

    let student_uri = format!("/api/v1/enrollments/{three_enrollment}");
    let answer = f
        .send(
            Method::PATCH,
            &student_uri,
            org.id,
            &admin,
            Some(json!({ "observed_user_ids": [one] })),
        )
        .await;
    answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(answer.body["errors"][0]["field"], "/observed_user_ids");

    // A holder of `enrollment.manage` without `progress.view` at the course concludes the
    // Observer, but neither reactivates it nor names learners.
    let manager = f
        .enrolled_with(
            org.id,
            org.course,
            &[perm::CourseView::KEY, perm::EnrollmentManage::KEY],
        )
        .await;
    for body in [json!({ "observed_user_ids": [one] })] {
        f.send(Method::PATCH, &uri, org.id, &manager, Some(body))
            .await
            .expect(StatusCode::FORBIDDEN, "permission.escalation");
    }
    let concluded = f
        .send(
            Method::PATCH,
            &uri,
            org.id,
            &manager,
            Some(json!({ "status": "concluded" })),
        )
        .await;
    assert_eq!(concluded.ok()["status"], "concluded");
    f.send(
        Method::PATCH,
        &uri,
        org.id,
        &manager,
        Some(json!({ "status": "active" })),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.escalation");
    let reactivated = f
        .send(
            Method::PATCH,
            &uri,
            org.id,
            &admin,
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(reactivated.ok()["status"], "active");
    f.send(
        Method::PATCH,
        &uri,
        org.id,
        &admin,
        Some(json!({ "status": null })),
    )
    .await
    .expect(StatusCode::BAD_REQUEST, "request.malformed");
}

/// T17 (B18, B20): the Observer lists exactly the learners they observe and reads only their
/// detail, records no progress, and reads nothing once the enrollment is concluded.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_an_observer_reads_only_the_learners_they_observe(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let (_, one) = f.student(org.id, org.course).await;
    let (_, two) = f.student(org.id, org.course).await;
    let (third_cookie, third) = f.student(org.id, org.course).await;
    f.step(org.id, &third_cookie, item, "view").await.ok();
    let (created, observer) = f
        .enroll_observer(org.id, &admin, org.course, json!([one, two]))
        .await;
    let enrollment = created.ok_with(StatusCode::CREATED)["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let page = f.get(org.id, &observer, &list_uri(org.course)).await;
    assert_eq!(learners(page.ok()), sorted(vec![one, two]));
    f.get(org.id, &observer, &detail_uri(org.course, one))
        .await
        .ok();
    f.get(org.id, &observer, &detail_uri(org.course, third))
        .await
        .expect(StatusCode::NOT_FOUND, "progress.not_found");
    f.step(org.id, &observer, item, "view")
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");

    let uri = format!("/api/v1/enrollments/{enrollment}");
    f.send(
        Method::PATCH,
        &uri,
        org.id,
        &admin,
        Some(json!({ "status": "concluded" })),
    )
    .await
    .ok();
    // A concluded enrollment grants nothing: the published course is out of the Observer's
    // sight altogether.
    for uri in [list_uri(org.course), detail_uri(org.course, one)] {
        f.get(org.id, &observer, &uri)
            .await
            .expect(StatusCode::NOT_FOUND, "course.not_found");
    }
    // Still holding `course.view` through another enrollment, they read no one's progress.
    let observer_id: Uuid = created.body["user"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let viewer_role = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Viewer".into(),
        permissions: vec![perm::CourseView::KEY.to_owned()],
    };
    let viewer_role =
        roles::create_custom(&f.pool, scope(org.id), &viewer_role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
    let new = enrollments::NewEnrollment {
        user_id: observer_id,
        role_id: viewer_role,
        source: Source::Local,
        external_id: None,
        observed_user_ids: Vec::new(),
    };
    enrollments::create(
        &f.pool,
        scope(org.id),
        org.course,
        &new,
        now(),
        Actor::Instance,
    )
    .await
    .expect("enroll");
    f.get(org.id, &observer, &list_uri(org.course))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
}

/// Without `course.view_all` a staff read leaves out unpublished modules and items; with it,
/// they are read.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn staff_reads_show_unpublished_content_only_to_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let (student, one) = f.student(org.id, org.course).await;
    f.step(org.id, &student, item, "view").await.ok();
    let (_, observer) = f
        .enroll_observer(org.id, &admin, org.course, json!([one]))
        .await;
    let reader = f
        .enrolled_with(
            org.id,
            org.course,
            &[perm::CourseView::KEY, perm::ProgressView::KEY],
        )
        .await;
    let (instructor, _, _) = f.enrolled(org.id, org.course, "instructor").await;
    let unpublish = ItemPatch {
        published: Some(false),
        ..ItemPatch::default()
    };
    items::update(&f.pool, scope(org.id), item, &unpublish, now())
        .await
        .expect("unpublish item");
    for cookie in [&observer, &reader] {
        let detail = f.get(org.id, cookie, &detail_uri(org.course, one)).await;
        assert_eq!(detail.ok()["items"], json!([]), "{detail:?}");
    }
    let detail = f
        .get(org.id, &instructor, &detail_uri(org.course, one))
        .await;
    assert_eq!(detail.ok()["items"][0]["item_id"], item.to_string());

    let unpublish = ModulePatch {
        published: Some(false),
        ..ModulePatch::default()
    };
    modules::update(&f.pool, scope(org.id), org.module, &unpublish, now())
        .await
        .expect("unpublish module");
    for cookie in [&observer, &reader] {
        let page = f.get(org.id, cookie, &list_uri(org.course)).await;
        assert_eq!(page.ok()["items"][0]["modules"], json!([]), "{page:?}");
        let detail = f.get(org.id, cookie, &detail_uri(org.course, one)).await;
        assert_eq!(detail.ok()["modules"], json!([]), "{detail:?}");
    }
    let page = f.get(org.id, &instructor, &list_uri(org.course)).await;
    assert_eq!(
        page.ok()["items"][0]["modules"][0]["module_id"],
        org.module.to_string()
    );
}

/// T12, T18 (B15, B20): deleting a Student enrollment removes that learner's progress in the
/// course and the Observer links naming them, keeps their progress in another course, records
/// the counts, and removes them from the Observer's list. With a second participating
/// enrollment in the course nothing is removed. Deleting an Observer enrollment removes its
/// links only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_t18_deleting_an_enrollment_removes_live_progress(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let item = f.link(org.id, org.module, Some(Requirement::View)).await;
    let other_course = f
        .course(org.id, org.tower, &[courses::Status::Published])
        .await;
    let other_module = f.module(org.id, other_course).await;
    let other_item = f.link(org.id, other_module, Some(Requirement::View)).await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;

    let (one_cookie, one, one_enrollment) = f.enrolled(org.id, org.course, "student").await;
    let new = enrollments::NewEnrollment {
        user_id: one,
        role_id: f.builtin("student").await,
        source: Source::Local,
        external_id: None,
        observed_user_ids: Vec::new(),
    };
    enrollments::create(
        &f.pool,
        scope(org.id),
        other_course,
        &new,
        now(),
        Actor::Instance,
    )
    .await
    .expect("enroll in the other course");
    f.step(org.id, &one_cookie, item, "view").await.ok();
    f.step(org.id, &one_cookie, other_item, "view").await.ok();
    let (two_cookie, two, two_enrollment) = f.enrolled(org.id, org.course, "student").await;
    f.step(org.id, &two_cookie, item, "view").await.ok();
    let (created, observer) = f
        .enroll_observer(org.id, &admin, org.course, json!([one, two]))
        .await;
    let observer_enrollment: Uuid = created.body["id"].as_str().unwrap().parse().unwrap();
    let other_progress = f.progress_of(one, other_course).await;
    assert_eq!(other_progress.len(), 3);
    assert_eq!(f.progress_of(one, org.course).await.len(), 3);

    f.send(
        Method::DELETE,
        &format!("/api/v1/enrollments/{one_enrollment}"),
        org.id,
        &admin,
        None,
    )
    .await
    .expect_status(StatusCode::NO_CONTENT);
    assert_eq!(f.progress_of(one, org.course).await, Vec::<String>::new());
    assert_eq!(f.progress_of(one, other_course).await, other_progress);
    assert_eq!(f.progress_of(two, org.course).await.len(), 3);
    assert_eq!(f.observee_links(org.id).await, [(observer_enrollment, two)]);
    let audit = f.audit_data(org.id, "enrollment.delete").await;
    assert_eq!(
        audit.last().unwrap()["removed"],
        json!({
            "enrollment_observees": 1,
            "item_progress": 1,
            "module_progress": 1,
            "course_progress": 1,
        })
    );
    let page = f.get(org.id, &observer, &list_uri(org.course)).await;
    assert_eq!(learners(page.ok()), [two]);

    // A second participating enrollment keeps everything.
    let participant = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Participant".into(),
        permissions: vec![perm::CourseParticipate::KEY.to_owned()],
    };
    let participant =
        roles::create_custom(&f.pool, scope(org.id), &participant, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
    let new = enrollments::NewEnrollment {
        user_id: two,
        role_id: participant,
        source: Source::Local,
        external_id: None,
        observed_user_ids: Vec::new(),
    };
    let second = enrollments::create(
        &f.pool,
        scope(org.id),
        org.course,
        &new,
        now(),
        Actor::Instance,
    )
    .await
    .expect("enroll again")
    .id;
    enrollments::set_status(
        &f.pool,
        scope(org.id),
        second,
        enrollments::Status::Concluded,
        now(),
        Actor::Instance,
    )
    .await
    .expect("conclude");
    f.send(
        Method::DELETE,
        &format!("/api/v1/enrollments/{two_enrollment}"),
        org.id,
        &admin,
        None,
    )
    .await
    .expect_status(StatusCode::NO_CONTENT);
    assert_eq!(f.progress_of(two, org.course).await.len(), 3);
    assert_eq!(f.observee_links(org.id).await, [(observer_enrollment, two)]);

    // An Observer enrollment takes its links and nothing else.
    f.send(
        Method::DELETE,
        &format!("/api/v1/enrollments/{observer_enrollment}"),
        org.id,
        &admin,
        None,
    )
    .await
    .expect_status(StatusCode::NO_CONTENT);
    assert_eq!(f.observee_links(org.id).await, []);
    assert_eq!(f.progress_of(two, org.course).await.len(), 3);
    let audit = f.audit_data(org.id, "enrollment.delete").await;
    assert_eq!(
        audit.last().unwrap()["removed"],
        json!({
            "enrollment_observees": 1,
            "item_progress": 0,
            "module_progress": 0,
            "course_progress": 0,
        })
    );
}

/// T15 (B14): org A's admin naming org B's course, or org A's course with org B's learner, gets
/// 404, and org B's rows are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_another_organizations_course_and_learner_are_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.org("a").await;
    let b = f.org("b").await;
    let item = f.link(b.id, b.module, Some(Requirement::View)).await;
    let (student, learner) = f.student(b.id, b.course).await;
    f.step(b.id, &student, item, "view").await.ok();
    let admin = f.holder_of_everything(a.id, Scope::Organization).await;
    let before = f.rows(b.id).await;
    for uri in [list_uri(b.course), detail_uri(b.course, learner)] {
        f.get(a.id, &admin, &uri)
            .await
            .expect(StatusCode::NOT_FOUND, "course.not_found");
    }
    f.get(a.id, &admin, &detail_uri(a.course, learner))
        .await
        .expect(StatusCode::NOT_FOUND, "progress.not_found");
    // Org A's Observer cannot be pointed at org B's learner.
    let (answer, _) = f
        .enroll_observer(a.id, &admin, a.course, json!([learner]))
        .await;
    answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(f.rows(b.id).await, before);
}

/// T14a's 404 for `progress.view_observed`, which the permission suite cannot build since it is
/// grantable at course scope only: an Observer of another course gets 404 `course.not_found` on
/// a course they hold nothing at.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14a_an_observer_of_another_course_does_not_find_this_one(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let other = f
        .course(org.id, org.tower, &[courses::Status::Published])
        .await;
    let (_, learner) = f.student(org.id, other).await;
    let (_, here) = f.student(org.id, org.course).await;
    let (created, observer) = f
        .enroll_observer(org.id, &admin, other, json!([learner]))
        .await;
    created.ok_with(StatusCode::CREATED);
    for uri in [list_uri(org.course), detail_uri(org.course, here)] {
        f.get(org.id, &observer, &uri)
            .await
            .expect(StatusCode::NOT_FOUND, "course.not_found");
    }
}

/// B20: an Observer reads only the links of their active enrollments whose role holds
/// `progress.view_observed`. A link on a concluded Observer enrollment, or on an active one
/// whose role lacks the permission (written by SQL, as a role edit would leave it), names no
/// one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn only_active_observer_enrollments_name_learners(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.holder_of_everything(org.id, Scope::Organization).await;
    let (_, one) = f.student(org.id, org.course).await;
    let (_, two) = f.student(org.id, org.course).await;
    let (_, three) = f.student(org.id, org.course).await;
    let (created, observer) = f
        .enroll_observer(org.id, &admin, org.course, json!([one]))
        .await;
    let observer_id: Uuid = created.body["user"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // A second Observer role of the organization's own, concluded, naming `two`.
    let custom = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Mentor".into(),
        permissions: vec![
            perm::CourseView::KEY.to_owned(),
            perm::ProgressViewObserved::KEY.to_owned(),
        ],
    };
    let custom = roles::create_custom(&f.pool, scope(org.id), &custom, now(), Actor::Instance)
        .await
        .expect("create role")
        .id;
    let new = enrollments::NewEnrollment {
        user_id: observer_id,
        role_id: custom,
        source: Source::Local,
        external_id: None,
        observed_user_ids: vec![two],
    };
    let mentor = enrollments::create(
        &f.pool,
        scope(org.id),
        org.course,
        &new,
        now(),
        Actor::Instance,
    )
    .await
    .expect("enroll as a mentor")
    .id;
    enrollments::set_status(
        &f.pool,
        scope(org.id),
        mentor,
        enrollments::Status::Concluded,
        now(),
        Actor::Instance,
    )
    .await
    .expect("conclude");
    // An active enrollment whose role lacks the permission, with a link to `three`.
    let viewer = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Viewer".into(),
        permissions: vec![perm::CourseView::KEY.to_owned()],
    };
    let viewer = roles::create_custom(&f.pool, scope(org.id), &viewer, now(), Actor::Instance)
        .await
        .expect("create role")
        .id;
    let new = enrollments::NewEnrollment {
        user_id: observer_id,
        role_id: viewer,
        source: Source::Local,
        external_id: None,
        observed_user_ids: Vec::new(),
    };
    let plain = enrollments::create(
        &f.pool,
        scope(org.id),
        org.course,
        &new,
        now(),
        Actor::Instance,
    )
    .await
    .expect("enroll as a viewer")
    .id;
    sqlx::query(
        "INSERT INTO enrollment_observees (org_id, enrollment_id, course_id, user_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(org.id)
    .bind(plain)
    .bind(org.course)
    .bind(three)
    .execute(&f.pool)
    .await
    .expect("a stale link");

    let page = f.get(org.id, &observer, &list_uri(org.course)).await;
    assert_eq!(learners(page.ok()), [one]);
    for user in [two, three] {
        f.get(org.id, &observer, &detail_uri(org.course, user))
            .await
            .expect(StatusCode::NOT_FOUND, "progress.not_found");
    }
}
