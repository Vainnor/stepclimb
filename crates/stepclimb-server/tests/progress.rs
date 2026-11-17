// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recording progress over the router, its outbox events and the learner's own progress in
//! the course read (docs/specs/m1/05-progress-and-events.md B1 to B3, B10 to B13, B16; T1 to T3,
//! T5, T9, T10 for `GET /api/v1/courses/{course_id}`, T13, and T14's explicit cases for
//! `POST /api/v1/items/{item_id}/progress`). The repository's own cases (T4, T6, T7) are in
//! `stepclimb-core`'s `tests/progress.rs`; the generated permission and cross-organization cases
//! in `tests/permissions.rs`.
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
    organizations, perm,
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
