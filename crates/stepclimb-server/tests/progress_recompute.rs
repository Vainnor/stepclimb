// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recomputing learners' progress after a rule change, over the router and the job worker
//! (docs/specs/m1/05-progress-and-events.md B8; T8): which content writes enqueue
//! `progress.recompute_course`, how a burst of them collapses into one job, and what the job
//! leaves in the learners' rows, their course outline and the outbox. The repository's own
//! cases (batches, concluded learners, archived courses, another organization) are in
//! `stepclimb-core`'s `tests/progress.rs`.
//!
//! Every request names its organization with the `test-support` header. Needs `DATABASE_URL`
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
    OrgScope, Source,
    audit::Actor,
    clock::ManualClock,
    courses, enrollments,
    id::{Uuid, new_id},
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, ModulePatch},
    organizations, roles,
    rules::Requirement,
    users::{self, NewUser},
};
use stepclimb_jobs::{NewJob, Worker, backoff::Random};
use stepclimb_server::{
    AppState, jobs, progress_recompute, request_org::TEST_ORG_HEADER, router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// 2025-11-13T02:13:20Z: the manual clock's start.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization with a published course holding two published modules.
#[derive(Clone, Copy, Debug)]
struct Org {
    id: Uuid,
    course: Uuid,
    first: Uuid,
    second: Uuid,
}

/// One `progress.recompute_course` row: organization, payload, dedupe key, `run_at`.
type JobRow = (Option<Uuid>, Value, Option<String>, SystemTime);

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
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
        Self {
            app: router(state).layer(Extension(ConnectInfo(CLIENT))),
            pool,
            clock,
        }
    }

    async fn org(&self, slug: &str) -> Org {
        let pool = &self.pool;
        let id = organizations::create(pool, slug, slug, t0())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let new = courses::NewCourse {
            account_id: root,
            title: "Course".into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        let course = courses::create(pool, scope(id), &new, t0(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        courses::set_status(
            pool,
            scope(id),
            course,
            courses::Status::Published,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("publish course");
        let first = self.module(id, course).await;
        let second = self.module(id, course).await;
        Org {
            id,
            course,
            first,
            second,
        }
    }

    async fn module(&self, org: Uuid, course: Uuid) -> Uuid {
        let id = modules::create(&self.pool, scope(org), course, "Module", t0())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(&self.pool, scope(org), id, &patch, t0())
            .await
            .expect("publish module");
        id
    }

    /// A published external link, last in `module`, with `requirement`.
    async fn link(&self, org: Uuid, module: Uuid, requirement: Option<Requirement>) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Link".into(),
            ref_id: None,
            url: Some("https://example.com/brief".into()),
            position: None,
        };
        let id = items::create(&self.pool, scope(org), module, &new, t0())
            .await
            .expect("create item")
            .id;
        let patch = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(&self.pool, scope(org), id, &patch, t0())
            .await
            .expect("publish item");
        items::set_requirement(&self.pool, scope(org), id, requirement, t0())
            .await
            .expect("set requirement");
        id
    }

    /// A new user of `org` enrolled in `course` with the built-in role `role`, signed in: their
    /// cookie and id.
    async fn enrolled(&self, org: Uuid, course: Uuid, role: &str) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: "Recompute Tester".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", new_id().simple())),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(&self.pool, scope(org), &new_user, t0())
            .await
            .expect("create user")
            .id;
        let role_id = roles::builtin_id(&self.pool, role)
            .await
            .expect("read role")
            .expect("a built-in role");
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(&self.pool, scope(org), course, &new, t0(), Actor::Instance)
            .await
            .expect("enroll");
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), t0())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), user)
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        (status, body)
    }

    /// Send a write and expect it to succeed.
    async fn write(&self, org: Uuid, cookie: &str, method: Method, uri: &str, body: Option<Value>) {
        let (status, answer) = self.send(method.clone(), uri, org, cookie, body).await;
        assert!(status.is_success(), "{method} {uri}: {status} {answer}");
    }

    /// Every waiting `progress.recompute_course` job.
    async fn jobs(&self) -> Vec<JobRow> {
        let rows: Vec<(Option<Uuid>, Value, Option<String>, i64)> = sqlx::query_as(
            "SELECT org_id, payload, dedupe_key,
                    (extract(epoch FROM run_at) * 1000000)::bigint
             FROM jobs WHERE kind = 'progress.recompute_course' ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await
        .expect("read jobs");
        rows.into_iter()
            .map(|(org, payload, key, run_at)| {
                let at =
                    UNIX_EPOCH + Duration::from_micros(u64::try_from(run_at).expect("after 1970"));
                (org, payload, key, at)
            })
            .collect()
    }

    async fn clear_jobs(&self) {
        sqlx::query("DELETE FROM jobs WHERE kind = 'progress.recompute_course'")
            .execute(&self.pool)
            .await
            .expect("clear jobs");
    }

    /// Run every due job once with the server's registry; how many ran.
    async fn run_jobs(&self) -> usize {
        let registry = jobs::registry(None, None).expect("the registry");
        let queues = registry.queues();
        let worker = Worker::new(
            self.pool.clone(),
            registry,
            self.clock.clock(),
            Random::from_fn(|| 0),
            queues,
        );
        worker.run_once(10).await.expect("run jobs")
    }

    /// The outbox topics of `org`, oldest first.
    async fn topics(&self, org: Uuid) -> Vec<String> {
        sqlx::query_scalar("SELECT topic FROM outbox WHERE org_id = $1 ORDER BY id")
            .bind(org)
            .fetch_all(&self.pool)
            .await
            .expect("read the outbox")
    }

    /// The learner's `progress` member of `module` in their course outline.
    async fn outline_progress(&self, org: Org, cookie: &str, module: Uuid) -> Value {
        let uri = format!("/api/v1/courses/{}/modules", org.course);
        let (status, body) = self.send(Method::GET, &uri, org.id, cookie, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["modules"]
            .as_array()
            .expect("modules")
            .iter()
            .find(|m| m["id"] == module.to_string())
            .expect("the module")["progress"]
            .clone()
    }

    async fn step(&self, org: Uuid, cookie: &str, item: Uuid, action: &str) {
        let uri = format!("/api/v1/items/{item}/progress");
        let body = Some(json!({ "action": action }));
        self.write(org, cookie, Method::POST, &uri, body).await;
    }
}

fn item_uri(item: Uuid) -> String {
    format!("/api/v1/items/{item}")
}

fn module_uri(module: Uuid) -> String {
    format!("/api/v1/modules/{module}")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_every_rule_changing_write_enqueues_the_recompute(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (designer, _) = f.enrolled(org.id, org.course, "course_designer").await;
    let a = f.link(org.id, org.first, Some(Requirement::View)).await;
    let spare = f.module(org.id, org.course).await;
    let gone = f.link(org.id, org.second, None).await;
    let empty = f.module(org.id, org.course).await;
    let writes: Vec<(&str, Method, String, Option<Value>)> = vec![
        (
            "unpublish an item",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "published": false })),
        ),
        (
            "publish an item",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "published": true })),
        ),
        (
            "change a requirement",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "requirement": { "kind": "mark_done" } })),
        ),
        (
            "remove a requirement",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "requirement": null })),
        ),
        (
            "add a requirement",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "requirement": { "kind": "view" } })),
        ),
        (
            "move an item to another module",
            Method::POST,
            format!("{}/move", item_uri(a)),
            Some(json!({ "module_id": org.second, "position": 0 })),
        ),
        ("delete an item", Method::DELETE, item_uri(gone), None),
        (
            "unpublish a module",
            Method::PATCH,
            module_uri(spare),
            Some(json!({ "published": false })),
        ),
        (
            "publish a module",
            Method::PATCH,
            module_uri(spare),
            Some(json!({ "published": true })),
        ),
        (
            "change prerequisites",
            Method::PATCH,
            module_uri(org.second),
            Some(json!({ "prerequisite_ids": [org.first] })),
        ),
        (
            "change a completion rule",
            Method::PATCH,
            module_uri(org.second),
            Some(json!({ "completion_rule": "one" })),
        ),
        ("delete a module", Method::DELETE, module_uri(empty), None),
    ];
    for (name, method, uri, body) in writes {
        f.clear_jobs().await;
        f.write(org.id, &designer, method, &uri, body).await;
        let jobs = f.jobs().await;
        let expected: Vec<JobRow> = vec![(
            Some(org.id),
            json!({ "course_id": org.course }),
            Some(org.course.to_string()),
            t0() + progress_recompute::DELAY,
        )];
        assert_eq!(jobs, expected, "{name}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b8_writes_that_change_no_count_enqueue_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (designer, _) = f.enrolled(org.id, org.course, "course_designer").await;
    let a = f.link(org.id, org.first, Some(Requirement::View)).await;
    let b = f.link(org.id, org.first, Some(Requirement::View)).await;
    let writes: Vec<(&str, Method, String, Option<Value>)> = vec![
        (
            "create a module",
            Method::POST,
            format!("/api/v1/courses/{}/modules", org.course),
            Some(json!({ "title": "New" })),
        ),
        (
            "create an item",
            Method::POST,
            format!("{}/items", module_uri(org.first)),
            Some(json!({ "kind": "header", "title": "New" })),
        ),
        (
            "rename an item",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "title": "Renamed" })),
        ),
        (
            "name the current state",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "published": true })),
        ),
        (
            "name the current requirement",
            Method::PATCH,
            item_uri(a),
            Some(json!({ "requirement": { "kind": "view" } })),
        ),
        (
            "move an item within its module",
            Method::POST,
            format!("{}/move", item_uri(b)),
            Some(json!({ "module_id": org.first, "position": 0 })),
        ),
        (
            "rename a module",
            Method::PATCH,
            module_uri(org.first),
            Some(json!({ "title": "R" })),
        ),
        (
            "set an unlock date",
            Method::PATCH,
            module_uri(org.first),
            Some(json!({ "unlock_at": "2030-01-01T00:00:00Z" })),
        ),
        (
            "require sequential order",
            Method::PATCH,
            module_uri(org.first),
            Some(json!({ "require_sequential": true })),
        ),
        (
            "reorder modules",
            Method::PUT,
            format!("/api/v1/courses/{}/modules/order", org.course),
            None,
        ),
    ];
    for (name, method, uri, body) in writes {
        let body = match (name, body) {
            ("reorder modules", _) => {
                let ids = modules_in(&f, org).await;
                Some(json!({ "module_ids": ids }))
            }
            (_, body) => body,
        };
        f.write(org.id, &designer, method, &uri, body).await;
        assert_eq!(f.jobs().await, Vec::<JobRow>::new(), "{name}");
    }
}

/// The course's module ids, last first.
async fn modules_in(f: &Fixture, org: Org) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM modules WHERE course_id = $1 ORDER BY position")
            .bind(org.course)
            .fetch_all(&f.pool)
            .await
            .expect("read modules");
    ids.reverse();
    ids
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_ten_edits_within_30_s_enqueue_one_job(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (designer, _) = f.enrolled(org.id, org.course, "course_designer").await;
    let a = f.link(org.id, org.first, Some(Requirement::View)).await;
    for edit in 0..10 {
        let published = edit % 2 == 1;
        let body = Some(json!({ "published": published }));
        f.write(org.id, &designer, Method::PATCH, &item_uri(a), body)
            .await;
        f.clock.advance(Duration::from_secs(2));
    }
    let jobs = f.jobs().await;
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(
        jobs[0].3,
        t0() + progress_recompute::DELAY,
        "the first edit's time"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_the_job_lowers_the_outline_counts_after_an_unpublish(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let (designer, _) = f.enrolled(org.id, org.course, "course_designer").await;
    let a = f.link(org.id, org.first, Some(Requirement::View)).await;
    f.link(org.id, org.first, Some(Requirement::View)).await;
    let (student, _) = f.enrolled(org.id, org.course, "student").await;
    f.step(org.id, &student, a, "view").await;
    let before = f.outline_progress(org, &student, org.first).await;
    assert_eq!(
        (&before["required_count"], &before["completed_count"]),
        (&json!(2), &json!(1))
    );
    let body = Some(json!({ "published": false }));
    f.write(org.id, &designer, Method::PATCH, &item_uri(a), body)
        .await;
    assert_eq!(f.run_jobs().await, 0, "the job waits 30 s");
    f.clock.advance(progress_recompute::DELAY);
    assert_eq!(f.run_jobs().await, 1);
    assert!(f.jobs().await.is_empty(), "the job is done");
    let after = f.outline_progress(org, &student, org.first).await;
    assert_eq!(
        (&after["required_count"], &after["completed_count"]),
        (&json!(1), &json!(0))
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_the_job_completes_a_module_once_and_a_rerun_emits_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org = f.org("a").await;
    let other = f.org("b").await;
    let (designer, _) = f.enrolled(org.id, org.course, "course_designer").await;
    let a = f.link(org.id, org.first, Some(Requirement::View)).await;
    let b = f.link(org.id, org.first, Some(Requirement::MarkDone)).await;
    let (student, _) = f.enrolled(org.id, org.course, "student").await;
    f.step(org.id, &student, a, "view").await;
    assert_eq!(f.topics(org.id).await, ["item.completed"]);
    let body = Some(json!({ "requirement": null }));
    f.write(org.id, &designer, Method::PATCH, &item_uri(b), body)
        .await;
    f.clock.advance(progress_recompute::DELAY);
    assert_eq!(f.run_jobs().await, 1);
    assert_eq!(
        f.topics(org.id).await,
        ["item.completed", "module.completed", "course.completed"],
        "the second module has no requirement, so the first is the course's only counted one"
    );
    let progress = f.outline_progress(org, &student, org.first).await;
    assert_eq!(progress["completed_at"], "2025-11-13T02:13:50.000000Z");

    let mut tx = f.pool.begin().await.unwrap();
    progress_recompute::enqueue(&mut tx, scope(org.id), org.course, f.clock.clock().now())
        .await
        .expect("enqueue");
    tx.commit().await.unwrap();
    f.clock.advance(progress_recompute::DELAY);
    assert_eq!(f.run_jobs().await, 1);
    assert_eq!(
        f.topics(org.id).await,
        ["item.completed", "module.completed", "course.completed"],
        "a second run emits nothing"
    );
    assert!(f.topics(other.id).await.is_empty(), "another organization");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_recompute_job_without_an_organization_fails(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let payload = progress_recompute::Payload {
        course_id: Uuid::from_u128(1),
    };
    let mut tx = f.pool.begin().await.unwrap();
    let job = NewJob::instance(&progress_recompute::RECOMPUTE, &payload).unwrap();
    stepclimb_jobs::enqueue(&mut tx, job, t0()).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(f.run_jobs().await, 1);
    let dead: (bool, Option<String>) = sqlx::query_as(
        "SELECT dead_at IS NOT NULL, last_error FROM jobs
         WHERE kind = 'progress.recompute_course'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        dead,
        (
            true,
            Some("progress.recompute_course needs an organization".into())
        )
    );
}
