// SPDX-License-Identifier: AGPL-3.0-or-later
//! Completion history and validity over the router
//! (docs/specs/m1/14-recurrence-and-expiring-completions.md B1 to B3, B13; T1 and T9 through the
//! routes, T10, T10a's explicit cases, T12's routes and `05` B15's kept completions). The
//! repository's own cases are in `stepclimb-core`'s `tests/completions.rs`; the generated
//! permission and cross-organization cases in `tests/permissions.rs`.
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
    modules::{self, ModulePatch},
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

/// 2026-03-10T00:00:00Z: the manual clock's start.
fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_773_100_800)
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

fn history_uri(course: Uuid) -> String {
    format!("/api/v1/courses/{course}/completions")
}

const MINE: &str = "/api/v1/me/completions";

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

    /// The `(user, target type, target, cycle)` of each item of a page, in order.
    #[track_caller]
    fn rows(&self) -> Vec<(String, String, String, i64)> {
        self.ok()["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|c| {
                (
                    c["user"]["id"].as_str().expect("a user id").to_owned(),
                    c["target"]["type"].as_str().expect("a type").to_owned(),
                    c["target"]["id"].as_str().expect("a target id").to_owned(),
                    c["cycle"].as_i64().expect("a cycle"),
                )
            })
            .collect()
    }
}

/// An organization with Tower under the root and a published course in Tower holding one
/// published module with one published link a view completes.
#[derive(Clone, Copy, Debug)]
struct Org {
    id: Uuid,
    tower: Uuid,
    course: Uuid,
    module: Uuid,
    item: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, start())
            .await
            .expect("sync roles");
        let clock = ManualClock::new(start());
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
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            pool,
            clock,
        }
    }

    async fn org(&self, slug: &str) -> Org {
        let pool = &self.pool;
        let id = organizations::create(pool, slug, slug, start())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let tower = accounts::create(pool, scope(id), root, "Tower", start(), Actor::Instance)
            .await
            .expect("create account")
            .id;
        let hex = new_id().simple().to_string();
        let new = courses::NewCourse {
            account_id: tower,
            title: "Course title".into(),
            code: format!("C{}", &hex[hex.len() - 12..]),
        };
        let course = courses::create(pool, scope(id), &new, start(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        courses::set_status(
            pool,
            scope(id),
            course,
            courses::Status::Published,
            start(),
            Actor::Instance,
        )
        .await
        .expect("publish course");
        let module = self.module(id, course, "Module title").await;
        let item = self.link(id, module).await;
        Org {
            id,
            tower,
            course,
            module,
            item,
        }
    }

    async fn module(&self, org: Uuid, course: Uuid, title: &str) -> Uuid {
        let id = modules::create(&self.pool, scope(org), course, title, start())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(&self.pool, scope(org), id, &patch, start())
            .await
            .expect("publish module");
        id
    }

    async fn link(&self, org: Uuid, module: Uuid) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Link title".into(),
            ref_id: None,
            url: Some("https://example.com/brief".into()),
            position: None,
        };
        let id = items::create(&self.pool, scope(org), module, &new, start())
            .await
            .expect("create item")
            .id;
        let patch = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(&self.pool, scope(org), id, &patch, start())
            .await
            .expect("publish item");
        let view = Some(Requirement::View);
        items::set_requirement(&self.pool, scope(org), id, view, start())
            .await
            .expect("set requirement");
        id
    }

    /// A new user of `org`, signed in: their cookie and id.
    async fn user(&self, org: Uuid) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: "Learner Name".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", new_id().simple())),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let id = users::create(&self.pool, scope(org), &new_user, start())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), start())
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

    async fn enroll(&self, org: Uuid, course: Uuid, user: Uuid, role: Uuid) -> Uuid {
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: role,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(
            &self.pool,
            scope(org),
            course,
            &new,
            start(),
            Actor::Instance,
        )
        .await
        .expect("enroll")
        .id
    }

    /// A new user enrolled in `course` with the built-in role `role`: cookie, user, enrollment.
    async fn enrolled(&self, org: Uuid, course: Uuid, role: &str) -> (String, Uuid, Uuid) {
        let (cookie, user) = self.user(org).await;
        let role = self.builtin(role).await;
        let enrollment = self.enroll(org, course, user, role).await;
        (cookie, user, enrollment)
    }

    /// A new user holding a custom role of exactly `keys`, enrolled in `course`.
    async fn enrolled_with(&self, org: Uuid, course: Uuid, keys: &[&str]) -> String {
        let (cookie, user) = self.user(org).await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Course role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, start(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.enroll(org, course, user, role).await;
        cookie
    }

    /// A new user holding the built-in role `role` at `at`.
    async fn assigned(&self, org: Uuid, role: &str, at: Scope) -> String {
        let (cookie, user) = self.user(org).await;
        let assignment = NewAssignment {
            user_id: user,
            role_id: self.builtin(role).await,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(org),
            &assignment,
            start(),
            Actor::Instance,
        )
        .await
        .expect("assign");
        cookie
    }

    /// A new user holding a custom role of exactly `keys` at `at`.
    async fn holder_at(&self, org: Uuid, keys: &[&str], at: Scope) -> String {
        let (cookie, user) = self.user(org).await;
        self.grant(org, user, keys, at).await;
        cookie
    }

    /// Give `user` a custom role of exactly `keys` at `at`.
    async fn grant(&self, org: Uuid, user: Uuid, keys: &[&str], at: Scope) {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Custom role".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, start(), Actor::Instance)
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
        role_assignments::create(
            &self.pool,
            scope(org),
            &assignment,
            start(),
            Actor::Instance,
        )
        .await
        .expect("assign");
    }

    /// The `(target type, title)` of each row of the caller's own history, sorted by type.
    async fn my_titles(&self, org: Uuid, cookie: &str) -> Vec<(String, Value)> {
        let page = self.get(org, cookie, MINE).await;
        let mut titles: Vec<(String, Value)> = page.ok()["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|c| {
                let kind = c["target"]["type"].as_str().expect("a type").to_owned();
                (kind, c["target"]["title"].clone())
            })
            .collect();
        titles.sort_by(|a, b| a.0.cmp(&b.0));
        titles
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

    async fn get(&self, org: Uuid, cookie: &str, uri: &str) -> Answer {
        self.send(Method::GET, uri, org, cookie, None).await
    }

    async fn view(&self, org: Uuid, cookie: &str, item: Uuid) {
        let uri = format!("/api/v1/items/{item}/progress");
        let body = json!({ "action": "view" });
        let answer = self.send(Method::POST, &uri, org, cookie, Some(body)).await;
        assert_eq!(answer.ok()["state"], "completed");
    }

    /// Delete `user`'s live progress in `course`, as spec 14 I2's reset will: their completions
    /// stay.
    async fn reset(&self, user: Uuid, course: Uuid) {
        for table in ["item_progress", "module_progress", "course_progress"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE user_id = $1 AND course_id = $2"
            )))
            .bind(user)
            .bind(course)
            .execute(&self.pool)
            .await
            .expect("reset");
        }
    }

    /// Every `completions` row of `org`, as text.
    async fn completion_rows(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT to_jsonb(c)::text FROM completions c WHERE org_id = $1 ORDER BY 1",
        )
        .bind(org)
        .fetch_all(&self.pool)
        .await
        .expect("read completions")
    }
}

/// T1 and T9 through the routes: `PATCH` sets a course's and a module's validity, which the
/// course and module reads show; a completion takes its expiry from the validity at that time,
/// in the course progress the learner reads and in their module progress; a later change leaves
/// it as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_t9_validity_is_set_by_patch_and_gives_the_completion_its_expiry(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.assigned(org.id, "org_admin", Scope::Organization).await;
    let course_uri = format!("/api/v1/courses/{}", org.course);
    let module_uri = format!("/api/v1/modules/{}", org.module);
    let course = f
        .send(
            Method::PATCH,
            &course_uri,
            org.id,
            &admin,
            Some(json!({ "completion_valid_months": 12 })),
        )
        .await;
    assert_eq!(course.ok()["completion_valid_months"], 12);
    assert_schema("Course", course.ok());
    let module = f
        .send(
            Method::PATCH,
            &module_uri,
            org.id,
            &admin,
            Some(json!({ "completion_valid_months": 6 })),
        )
        .await;
    assert_eq!(module.ok()["completion_valid_months"], 6);
    assert_schema("Module", module.ok());

    let (student, _, _) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &student, org.item).await;
    let read = f.get(org.id, &student, &course_uri).await;
    assert_eq!(read.ok()["completion_valid_months"], 12);
    assert_eq!(
        read.ok()["progress"]["expires_at"],
        "2027-03-10T00:00:00.000000Z"
    );
    let outline = f.get(org.id, &student, &module_uri).await;
    assert_eq!(
        outline.ok()["progress"]["expires_at"],
        "2026-09-10T00:00:00.000000Z"
    );
    assert_eq!(outline.ok()["completion_valid_months"], 6);
    assert_schema("Module", outline.ok());

    let cleared = f
        .send(
            Method::PATCH,
            &course_uri,
            org.id,
            &admin,
            Some(json!({ "completion_valid_months": null })),
        )
        .await;
    assert_eq!(cleared.ok()["completion_valid_months"], Value::Null);
    let read = f.get(org.id, &student, &course_uri).await;
    assert_eq!(
        read.ok()["progress"]["expires_at"],
        "2027-03-10T00:00:00.000000Z",
        "T9: the recorded expiry stays"
    );
    let audit: Vec<Value> = sqlx::query_scalar(
        "SELECT data FROM audit_events WHERE org_id = $1 AND action = 'course.update'
         ORDER BY id",
    )
    .bind(org.id)
    .fetch_all(&f.pool)
    .await
    .expect("read audit");
    assert_eq!(
        audit.last().expect("an entry"),
        &json!({
            "before": { "completion_valid_months": 12 },
            "after": { "completion_valid_months": null },
        })
    );
}

/// B3: a validity outside 1 to 120 months is a 422 naming the field, on courses and modules,
/// and changes nothing; a value that is not a whole number is malformed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b3_a_validity_outside_1_to_120_months_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.assigned(org.id, "org_admin", Scope::Organization).await;
    for uri in [
        format!("/api/v1/courses/{}", org.course),
        format!("/api/v1/modules/{}", org.module),
    ] {
        for months in [
            json!(0),
            json!(121),
            json!(-1),
            json!(40_000),
            json!(i64::MAX),
        ] {
            let answer = f
                .send(
                    Method::PATCH,
                    &uri,
                    org.id,
                    &admin,
                    Some(json!({ "completion_valid_months": months })),
                )
                .await;
            answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
            assert_eq!(
                answer.body["errors"],
                json!([{
                    "field": "/completion_valid_months",
                    "code": "completion.validity_out_of_range",
                }]),
                "{uri} {months}"
            );
        }
        let answer = f
            .send(
                Method::PATCH,
                &uri,
                org.id,
                &admin,
                Some(json!({ "completion_valid_months": 1.5 })),
            )
            .await;
        answer.expect(StatusCode::BAD_REQUEST, "request.malformed");
    }
    let stored: (Option<i16>, Option<i16>) = sqlx::query_as(
        "SELECT c.completion_valid_months, m.completion_valid_months
         FROM courses c JOIN modules m ON m.course_id = c.id WHERE m.id = $1",
    )
    .bind(org.module)
    .fetch_one(&f.pool)
    .await
    .expect("read");
    assert_eq!(stored, (None, None));
}

/// T10 (B13): an Instructor lists a learner's history across cycles, newest first; the learner
/// reads their own; another learner reads none of it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_history_across_cycles_for_staff_and_the_learner(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (instructor, _, _) = f.enrolled(org.id, org.course, "instructor").await;
    let (learner, learner_id, _) = f.enrolled(org.id, org.course, "student").await;
    let (other, _, _) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &learner, org.item).await;
    f.reset(learner_id, org.course).await;
    f.clock.advance(Duration::from_secs(3_600));
    f.view(org.id, &learner, org.item).await;

    let (me, module, course) = (
        learner_id.to_string(),
        org.module.to_string(),
        org.course.to_string(),
    );
    let staff = f.get(org.id, &instructor, &history_uri(org.course)).await;
    let mut rows = staff.rows();
    rows.sort_by_key(|row| (std::cmp::Reverse(row.3), row.1.clone()));
    assert_eq!(
        rows,
        [
            (me.clone(), "course".into(), course.clone(), 2),
            (me.clone(), "module".into(), module.clone(), 2),
            (me.clone(), "course".into(), course.clone(), 1),
            (me.clone(), "module".into(), module.clone(), 1),
        ]
    );
    let page = staff.ok();
    assert_schema("Completion", &page["items"][0]);
    assert_eq!(page["items"][0]["cycle"], 2, "newest first");
    assert_eq!(
        page["items"][0]["completed_at"],
        "2026-03-10T01:00:00.000000Z"
    );
    assert_eq!(page["items"][0]["expires_at"], Value::Null);
    assert_eq!(page["items"][0]["expired_at"], Value::Null);
    assert_eq!(page["items"][0]["user"]["display_name"], "Learner Name");
    let module_row = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["target"]["type"] == "module")
        .unwrap();
    assert_eq!(module_row["target"]["title"], "Module title");

    let filtered = format!(
        "{}?user_id={learner_id}&target_type=module",
        history_uri(org.course)
    );
    let rows = f.get(org.id, &instructor, &filtered).await.rows();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.1 == "module"));
    let paged = format!("{}?limit=3", history_uri(org.course));
    let first = f.get(org.id, &instructor, &paged).await;
    let cursor = first.ok()["next_cursor"].as_str().expect("a next page");
    let rest = format!("{paged}&cursor={cursor}");
    let rest = f.get(org.id, &instructor, &rest).await;
    assert_eq!(rest.rows().len(), 1);
    assert_eq!(rest.ok()["next_cursor"], Value::Null);

    let own = f.get(org.id, &learner, MINE).await;
    assert_eq!(own.rows().len(), 4);
    assert_schema("Completion", &own.ok()["items"][0]);
    let own = f
        .get(
            org.id,
            &learner,
            &format!("{MINE}?course_id={}", org.course),
        )
        .await;
    assert_eq!(own.rows().len(), 4);
    let elsewhere = f
        .get(org.id, &learner, &format!("{MINE}?course_id={}", new_id()))
        .await;
    assert!(elsewhere.rows().is_empty());
    assert!(f.get(org.id, &other, MINE).await.rows().is_empty());
    f.get(org.id, &other, &history_uri(org.course))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.get(org.id, &learner, &format!("{MINE}?cursor=bm90LWEta2V5"))
        .await
        .expect(StatusCode::BAD_REQUEST, "pagination.invalid_cursor");
}

/// T10a (B13): a Student and a holder of only `course.view` get 403 `permission.denied`; an
/// Observer sees only observed learners' rows; an Account admin of the course's account reads
/// every learner's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10a_observers_read_only_observed_learners_and_viewers_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (one_cookie, one, _) = f.enrolled(org.id, org.course, "student").await;
    let (two_cookie, two, _) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &one_cookie, org.item).await;
    f.view(org.id, &two_cookie, org.item).await;
    let (observer, observer_id) = f.user(org.id).await;
    let observing = f.builtin("observer").await;
    let new = enrollments::NewEnrollment {
        user_id: observer_id,
        role_id: observing,
        source: Source::Local,
        external_id: None,
        observed_user_ids: vec![one],
    };
    enrollments::create(
        &f.pool,
        scope(org.id),
        org.course,
        &new,
        start(),
        Actor::Instance,
    )
    .await
    .expect("enroll the observer");

    let rows = f
        .get(org.id, &observer, &history_uri(org.course))
        .await
        .rows();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.0 == one.to_string()), "{rows:?}");
    let for_two = format!("{}?user_id={two}", history_uri(org.course));
    assert!(f.get(org.id, &observer, &for_two).await.rows().is_empty());

    let viewer = f
        .enrolled_with(org.id, org.course, &[perm::CourseView::KEY])
        .await;
    for cookie in [&one_cookie, &viewer] {
        f.get(org.id, cookie, &history_uri(org.course))
            .await
            .expect(StatusCode::FORBIDDEN, "permission.denied");
    }
    let account_admin = f
        .assigned(org.id, "account_admin", Scope::Account(org.tower))
        .await;
    let rows = f
        .get(org.id, &account_admin, &history_uri(org.course))
        .await
        .rows();
    assert_eq!(rows.len(), 4);
}

/// B13 with spec 05 B14: without `course.view_all`, a completion of a module that is now
/// unpublished is left out; with it, it is listed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unpublished_modules_completions_show_only_to_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (learner, _, _) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &learner, org.item).await;
    let unpublish = ModulePatch {
        published: Some(false),
        ..ModulePatch::default()
    };
    modules::update(&f.pool, scope(org.id), org.module, &unpublish, start())
        .await
        .expect("unpublish");
    let reader = f
        .enrolled_with(
            org.id,
            org.course,
            &[perm::CourseView::KEY, perm::ProgressView::KEY],
        )
        .await;
    let rows = f
        .get(org.id, &reader, &history_uri(org.course))
        .await
        .rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "course");
    let (instructor, _, _) = f.enrolled(org.id, org.course, "instructor").await;
    let rows = f
        .get(org.id, &instructor, &history_uri(org.course))
        .await
        .rows();
    assert_eq!(rows.len(), 2);
    let own = f.get(org.id, &learner, MINE).await.rows();
    assert_eq!(own.len(), 2, "the learner's own history keeps it");
}

/// Spec 05 B15 and its T12 case for `completions`: deleting an enrollment never deletes them.
/// With live progress the delete is refused and nothing changes; without live progress (a
/// learner whose progress a reset cleared), the delete goes ahead and the completions stay,
/// listed to staff and to the learner.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_deleting_an_enrollment_keeps_completions(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.assigned(org.id, "org_admin", Scope::Organization).await;
    let (learner, learner_id, enrollment) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &learner, org.item).await;
    let before = f.completion_rows(org.id).await;
    assert_eq!(before.len(), 2);
    let delete = format!("/api/v1/enrollments/{enrollment}");
    f.send(Method::DELETE, &delete, org.id, &admin, None)
        .await
        .expect(StatusCode::CONFLICT, "enrollment.has_progress");
    assert_eq!(f.completion_rows(org.id).await, before);

    f.reset(learner_id, org.course).await;
    let answer = f.send(Method::DELETE, &delete, org.id, &admin, None).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    assert_eq!(
        f.completion_rows(org.id).await,
        before,
        "the completions stay"
    );
    let staff = f.get(org.id, &admin, &history_uri(org.course)).await.rows();
    assert_eq!(staff.len(), 2);
    assert_eq!(f.get(org.id, &learner, MINE).await.rows().len(), 2);
}

/// T12 (routes): org A's admin with org B's course id gets 404 and org B's rows are unchanged;
/// org A's members never read org B's completions.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_another_organizations_course_and_completions_are_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (a, b) = (f.org("a").await, f.org("b").await);
    let (learner_b, learner_b_id, _) = f.enrolled(b.id, b.course, "student").await;
    f.view(b.id, &learner_b, b.item).await;
    let rows_b = f.completion_rows(b.id).await;
    assert_eq!(rows_b.len(), 2);
    let admin_a = f.assigned(a.id, "org_admin", Scope::Organization).await;
    f.get(a.id, &admin_a, &history_uri(b.course))
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    let foreign = format!("{}?user_id={learner_b_id}", history_uri(a.course));
    assert!(f.get(a.id, &admin_a, &foreign).await.rows().is_empty());
    let foreign = format!("{MINE}?course_id={}", b.course);
    assert!(f.get(a.id, &admin_a, &foreign).await.rows().is_empty());
    assert!(f.get(a.id, &admin_a, MINE).await.rows().is_empty());
    for (method, uri, body) in [
        (
            Method::PATCH,
            format!("/api/v1/courses/{}", b.course),
            json!({ "completion_valid_months": 12 }),
        ),
        (
            Method::PATCH,
            format!("/api/v1/modules/{}", b.module),
            json!({ "completion_valid_months": 12 }),
        ),
    ] {
        let answer = f.send(method, &uri, a.id, &admin_a, Some(body)).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{uri}: {answer:?}");
    }
    assert_eq!(f.completion_rows(b.id).await, rows_b);
    let stored: (Option<i16>, Option<i16>) = sqlx::query_as(
        "SELECT c.completion_valid_months, m.completion_valid_months
         FROM courses c JOIN modules m ON m.course_id = c.id WHERE m.id = $1",
    )
    .bind(b.module)
    .fetch_one(&f.pool)
    .await
    .expect("read");
    assert_eq!(stored, (None, None));
}

/// T11 for the changed `PATCH /api/v1/modules/{module_id}` member (the permission suite's
/// module patch already sends six members): anonymous 401; every permission but
/// `course.content_edit` at Tower 403; every permission at a sibling account 404;
/// `course.content_edit` alone at Tower 200. `PATCH /api/v1/courses/{course_id}`'s cases run in
/// the permission suite with the validity in its body.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_the_module_validity_takes_course_content_edit(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let root: Uuid = sqlx::query_scalar("SELECT parent_id FROM accounts WHERE id = $1")
        .bind(org.tower)
        .fetch_one(&f.pool)
        .await
        .expect("the root");
    let enroute = accounts::create(
        &f.pool,
        scope(org.id),
        root,
        "Enroute",
        start(),
        Actor::Instance,
    )
    .await
    .expect("create account")
    .id;
    let uri = format!("/api/v1/modules/{}", org.module);
    let body = || Some(json!({ "completion_valid_months": 6 }));
    let every: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
    let but_edit: Vec<&str> = every
        .iter()
        .copied()
        .filter(|&k| k != perm::CourseContentEdit::KEY)
        .collect();

    f.send(Method::PATCH, &uri, org.id, "", body())
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    let denied = f
        .holder_at(org.id, &but_edit, Scope::Account(org.tower))
        .await;
    f.send(Method::PATCH, &uri, org.id, &denied, body())
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let sibling = f.holder_at(org.id, &every, Scope::Account(enroute)).await;
    f.send(Method::PATCH, &uri, org.id, &sibling, body())
        .await
        .expect(StatusCode::NOT_FOUND, "module.not_found");
    let stored = || async {
        sqlx::query_scalar::<_, Option<i16>>(
            "SELECT completion_valid_months FROM modules WHERE id = $1",
        )
        .bind(org.module)
        .fetch_one(&f.pool)
        .await
        .expect("read")
    };
    assert_eq!(stored().await, None, "refusals change nothing");
    let editor = f
        .holder_at(
            org.id,
            &[perm::CourseContentEdit::KEY],
            Scope::Account(org.tower),
        )
        .await;
    let answer = f.send(Method::PATCH, &uri, org.id, &editor, body()).await;
    assert_eq!(answer.ok()["completion_valid_months"], 6);
    assert_eq!(stored().await, Some(6));
}

/// Decision 3 (2026-11-19): the caller's own history answers a null title where they cannot
/// read it now. A module unpublished and renamed after the learner completed it has a null
/// title, the published course keeps its title, and with `course.view_all` at the course the
/// learner reads the module's new title.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn my_history_hides_an_unpublished_modules_title_without_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (learner, learner_id, _) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &learner, org.item).await;
    let hide = ModulePatch {
        title: Some("Renamed secret module".into()),
        published: Some(false),
        ..ModulePatch::default()
    };
    modules::update(&f.pool, scope(org.id), org.module, &hide, start())
        .await
        .expect("rename and unpublish");
    assert_eq!(
        f.my_titles(org.id, &learner).await,
        [
            ("course".to_owned(), json!("Course title")),
            ("module".to_owned(), Value::Null),
        ]
    );
    let page = f.get(org.id, &learner, MINE).await;
    for item in page.ok()["items"].as_array().expect("a page") {
        assert_schema("Completion", item);
    }
    let view_all = [perm::CourseViewAll::KEY];
    f.grant(org.id, learner_id, &view_all, Scope::Account(org.tower))
        .await;
    assert_eq!(
        f.my_titles(org.id, &learner).await,
        [
            ("course".to_owned(), json!("Course title")),
            ("module".to_owned(), json!("Renamed secret module")),
        ]
    );
}

/// Decision 3 (2026-11-19): a course put back to draft after the learner's enrollment was
/// deleted hides its title and its module's title from the learner's own history; with
/// `course.view_all` the learner reads both.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn my_history_hides_a_draft_courses_titles_without_view_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let admin = f.assigned(org.id, "org_admin", Scope::Organization).await;
    let (learner, learner_id, enrollment) = f.enrolled(org.id, org.course, "student").await;
    f.view(org.id, &learner, org.item).await;
    f.reset(learner_id, org.course).await;
    let delete = format!("/api/v1/enrollments/{enrollment}");
    let answer = f.send(Method::DELETE, &delete, org.id, &admin, None).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    courses::set_status(
        &f.pool,
        scope(org.id),
        org.course,
        courses::Status::Draft,
        start(),
        Actor::Instance,
    )
    .await
    .expect("back to draft");
    assert_eq!(
        f.my_titles(org.id, &learner).await,
        [
            ("course".to_owned(), Value::Null),
            ("module".to_owned(), Value::Null),
        ]
    );
    let view_all = [perm::CourseViewAll::KEY];
    f.grant(org.id, learner_id, &view_all, Scope::Organization)
        .await;
    assert_eq!(
        f.my_titles(org.id, &learner).await,
        [
            ("course".to_owned(), json!("Course title")),
            ("module".to_owned(), json!("Module title")),
        ]
    );
}
