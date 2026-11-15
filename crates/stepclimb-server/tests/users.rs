// SPDX-License-Identifier: AGPL-3.0-or-later
//! The user admin routes over the whole router (docs/specs/m0/03-users-and-identities.md B3, B6,
//! B7, B9, B10, B12; T2 to T5, T7, T9, T10, T19, T20's users part). The four permission cases and
//! the cross-organization case of each route are also in `tests/permissions.rs`.
//!
//! Org A is the organization every request names with the `test-support` header; org B exists
//! to hold another organization's ids. `session::start` stands in for the sign-in specs. Needs
//! `DATABASE_URL` (`just up`).

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

const USERS: &str = "/api/v1/users";

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

    /// The `(field, code)` pairs of a 422's `errors`.
    fn errors(&self) -> Vec<(String, String)> {
        self.body["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .map(|e| {
                        (
                            e["field"].as_str().expect("a field").into(),
                            e["code"].as_str().expect("a code").into(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[track_caller]
    fn refused(&self, status: StatusCode, code: &str) {
        assert_eq!((self.status, self.code()), (status, code), "{self:?}");
    }

    #[track_caller]
    fn invalid(&self, field: &str, code: &str) {
        self.refused(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        assert_eq!(
            self.errors(),
            [(field.to_owned(), code.to_owned())],
            "{self:?}"
        );
    }

    fn id(&self) -> Uuid {
        self.body["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a UUID")
    }

    /// The `display_name`s of a page's items.
    fn names(&self) -> Vec<String> {
        self.body["items"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|u| u["display_name"].as_str().expect("a name").to_owned())
            .collect()
    }
}

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
    a: Uuid,
    b: Uuid,
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id
}

fn new_user(display_name: &str, source: Source) -> NewUser {
    NewUser {
        display_name: display_name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source,
        external_id: None,
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let a = org(&pool, "org-a").await;
        let b = org(&pool, "org-b").await;
        let clock = ManualClock::new(now());
        let app = router(AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        })
        .layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            app,
            clock,
            a,
            b,
        }
    }

    /// A user of `org` made by the repository, as sign-in or a sync would make one.
    async fn stored_user(&self, org: Uuid, user: NewUser) -> Uuid {
        users::create(&self.pool, scope(org), &user, now())
            .await
            .expect("create user")
            .id
    }

    /// A signed-in user of org A with no role: their `Cookie` value and id.
    async fn user(&self) -> (String, Uuid) {
        let id = self
            .stored_user(self.a, new_user("Pat Example", Source::Local))
            .await;
        (self.sign_in(id).await, id)
    }

    async fn sign_in(&self, user: Uuid) -> String {
        let started = session::start(&self.pool, scope(self.a), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn assign(&self, user: Uuid, role: Uuid, at: Scope) {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.a),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
    }

    /// A signed-in Organization admin of org A.
    async fn admin(&self) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind("org_admin")
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(user, role, Scope::Organization).await;
        (cookie, user)
    }

    /// A signed-in user of org A holding a custom role with `permissions` at `at`.
    async fn holder(&self, permissions: &[&str], at: Scope) -> String {
        self.holder_with_id(permissions, at).await.0
    }

    /// [`Self::holder`] with the holder's id.
    async fn holder_with_id(&self, permissions: &[&str], at: Scope) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(self.a), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, at).await;
        (cookie, user)
    }

    async fn root(&self) -> Uuid {
        sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(self.a)
            .fetch_one(&self.pool)
            .await
            .expect("the root")
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        send(&self.app, self.a, method, uri, cookie, body).await
    }

    async fn post(&self, cookie: &str, body: Value) -> Answer {
        self.send(Method::POST, USERS, cookie, Some(body)).await
    }

    async fn patch(&self, cookie: &str, user: Uuid, body: Value) -> Answer {
        let uri = format!("{USERS}/{user}");
        self.send(Method::PATCH, &uri, cookie, Some(body)).await
    }

    async fn count(&self, org: Uuid) -> i64 {
        users::count_for_limit(&self.pool, scope(org))
            .await
            .expect("count users")
    }

    /// Every column of a `users` row as JSON text.
    async fn stored(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT to_jsonb(u)::text FROM users u WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("read user")
    }

    /// Point org A at a new test plan with these `limits`, by SQL.
    async fn limit_users(&self, max: i64) {
        let plan = new_id();
        sqlx::query(
            "INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, '{}', $3::jsonb)",
        )
        .bind(plan)
        .bind(format!("test_{}", plan.simple()))
        .bind(json!({ "users": max }).to_string())
        .execute(&self.pool)
        .await
        .expect("insert plan");
        sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
            .bind(self.a)
            .bind(plan)
            .execute(&self.pool)
            .await
            .expect("move to the plan");
    }

    async fn provider(&self, org: Uuid, key: &str) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config)
             VALUES ($1, $2, $3, 'oidc', '{}'::jsonb)",
        )
        .bind(id)
        .bind(org)
        .bind(key)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    async fn identities_of(&self, user: Uuid) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT p.key, i.subject FROM identities i
             JOIN identity_providers p ON p.id = i.provider_id
             WHERE i.user_id = $1 ORDER BY i.id",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .expect("read identities")
    }

    /// The `user.set_status` audit entries of `user`: `(actor id, ip, data)`.
    async fn status_entries(&self, user: Uuid) -> Vec<(Option<Uuid>, Option<String>, Value)> {
        sqlx::query_as(
            "SELECT actor_id, host(ip), data FROM audit_events
             WHERE action = 'user.set_status' AND target_id = $1 ORDER BY id",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .expect("read entries")
    }
}

async fn send(
    app: &Router,
    org: Uuid,
    method: Method,
    uri: &str,
    cookie: &str,
    body: Option<Value>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(TEST_ORG_HEADER, org.to_string())
        .header(header::COOKIE, cookie)
        .header("sec-fetch-site", "same-origin");
    let body = match body {
        Some(json) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("a valid request"))
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
        serde_json::from_slice(&bytes).expect("a JSON body")
    };
    Answer {
        status,
        headers,
        body,
    }
}

// Creating (B3, B4, B9).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_created_user_is_local_and_active_with_the_defaults(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;

    let created = f
        .post(&admin, json!({ "display_name": "  Sam Ortiz " }))
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_schema("User", &created.body);
    let id = created.id();
    assert_eq!(
        created.headers[header::LOCATION].to_str().unwrap(),
        format!("{USERS}/{id}")
    );
    assert_eq!(
        created.body,
        json!({
            "id": id,
            "display_name": "Sam Ortiz",
            "given_name": "",
            "family_name": "",
            "email": null,
            "locale": "en",
            "tz": "UTC",
            "status": "active",
            "source": "local",
            "external_id": null,
            "created_at": "2025-11-13T02:13:20.000000Z",
            "updated_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    let read = f
        .send(Method::GET, &format!("{USERS}/{id}"), &admin, None)
        .await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body, created.body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn creating_stores_every_field_given(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let body = json!({
        "display_name": "Sam Ortiz",
        "given_name": "Sam",
        "family_name": "Ortiz",
        "email": " Sam@Example.test ",
        "locale": "en",
        "tz": "america/new_york",
        "external_id": "E1",
    });

    let created = f.post(&admin, body).await;

    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_schema("User", &created.body);
    for (member, value) in [
        ("given_name", "Sam"),
        ("family_name", "Ortiz"),
        ("email", "Sam@Example.test"),
        ("tz", "America/New_York"),
        ("external_id", "E1"),
    ] {
        assert_eq!(created.body[member], value, "{member}");
    }
}

/// B3, Q4: a user created without `tz` gets the organization's `default_tz`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_without_a_time_zone_gets_the_organizations_default(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let settings = f
        .send(
            Method::PATCH,
            "/api/v1/organization/settings",
            &admin,
            Some(json!({ "default_tz": "europe/dublin" })),
        )
        .await;
    assert_eq!(settings.status, StatusCode::OK, "{settings:?}");
    assert_eq!(settings.body["default_tz"], "Europe/Dublin");

    let defaulted = f.post(&admin, json!({ "display_name": "Sam" })).await;
    let given = f
        .post(
            &admin,
            json!({ "display_name": "Robin", "tz": "Asia/Tokyo" }),
        )
        .await;

    assert_eq!(defaulted.body["tz"], "Europe/Dublin", "{defaulted:?}");
    assert_eq!(given.body["tz"], "Asia/Tokyo", "{given:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_invalid_fields_answer_422_and_insert_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let before = f.count(f.a).await;
    let long = "a".repeat(201);
    let cases = [
        (
            json!({ "display_name": " " }),
            "/display_name",
            "name.empty",
        ),
        (
            json!({ "display_name": long }),
            "/display_name",
            "text.too_long",
        ),
        (
            json!({ "display_name": "Sam", "family_name": long }),
            "/family_name",
            "text.too_long",
        ),
        (
            json!({ "display_name": "Sam", "locale": "fr" }),
            "/locale",
            "user.locale_unsupported",
        ),
        (
            json!({ "display_name": "Sam", "tz": "Mars/Olympus_Mons" }),
            "/tz",
            "tz.unknown",
        ),
        (
            json!({ "display_name": "Sam", "email": "  " }),
            "/email",
            "email.empty",
        ),
        (
            json!({ "display_name": "Sam", "email": "sam\u{200B}@example.test" }),
            "/email",
            "email.invalid_character",
        ),
        (
            json!({ "display_name": "Sam\u{0}" }),
            "/display_name",
            "text.contains_nul",
        ),
        (
            json!({ "display_name": "Sam", "external_id": "E\u{0}1" }),
            "/external_id",
            "text.contains_nul",
        ),
    ];
    for (body, field, code) in cases {
        f.post(&admin, body).await.invalid(field, code);
    }
    assert_eq!(f.count(f.a).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_omitted_names_are_empty_and_null_members_are_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let created = f
        .post(&admin, json!({ "display_name": "Sam", "email": null }))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(
        (&created.body["given_name"], &created.body["family_name"]),
        (&json!(""), &json!(""))
    );
    let before = f.count(f.a).await;
    for body in [
        json!({ "display_name": "Sam", "given_name": null }),
        json!({ "display_name": "Sam", "tz": null }),
        json!({ "display_name": "Sam", "identities": null }),
        json!({ "display_name": "Sam", "source": "idp" }),
        json!({ "display_name": "Sam", "status": "suspended" }),
        json!({ "given_name": "Sam" }),
    ] {
        let answer = f.post(&admin, body.clone()).await;
        answer.refused(StatusCode::BAD_REQUEST, "request.malformed");
        assert_eq!(f.count(f.a).await, before, "{body}");
    }
}

/// T4 (B5) and B12: an external id or an email another user of the organization has (the email
/// in any letter case) is refused; another organization's user does not collide.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_external_ids_and_emails_are_unique_within_the_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let taken = NewUser {
        external_id: Some("E1".into()),
        email: Some("sam@example.test".into()),
        ..new_user("In B", Source::Local)
    };
    f.stored_user(f.b, taken).await;
    let first = f
        .post(
            &admin,
            json!({ "display_name": "Sam", "external_id": "E1", "email": "sam@example.test" }),
        )
        .await;
    assert_eq!(
        first.status,
        StatusCode::CREATED,
        "org B's user does not collide"
    );
    let before = f.count(f.a).await;

    f.post(
        &admin,
        json!({ "display_name": "Robin", "external_id": "E1" }),
    )
    .await
    .refused(StatusCode::CONFLICT, "user.external_id_taken");
    f.post(
        &admin,
        json!({ "display_name": "Robin", "email": "SAM@example.TEST" }),
    )
    .await
    .refused(StatusCode::CONFLICT, "user.email_taken");
    assert_eq!(f.count(f.a).await, before);
}

// The users limit (B9, spec 10 B10).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_reached_users_limit_refuses_and_inserts_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    f.limit_users(2).await;
    let created = f.post(&admin, json!({ "display_name": "Second" })).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");

    let refused = f.post(&admin, json!({ "display_name": "Third" })).await;

    refused.refused(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(refused.body["limit"], "users");
    assert_eq!(refused.body["max"], 2);
    assert_eq!(f.count(f.a).await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_the_unlimited_plan_creates(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    for name in ["One", "Two", "Three"] {
        let created = f.post(&admin, json!({ "display_name": name })).await;
        assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    }
    assert_eq!(f.count(f.a).await, 4);
}

/// Spec 10 T7 on the real route: two creations at `users - 1` both wait on the plan row, which
/// this test holds locked; once it lets go, one creates and the other counts that user and is
/// refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_two_concurrent_creations_at_the_limit_create_one_user(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    let (admin, _) = f.admin().await;
    f.limit_users(2).await;
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM org_plans WHERE org_id = $1 FOR UPDATE")
        .bind(f.a)
        .execute(&mut *lock)
        .await
        .unwrap();

    let spawn = |name: &'static str| {
        let (app, org, admin) = (f.app.clone(), f.a, admin.clone());
        tokio::spawn(async move {
            let body = json!({ "display_name": name });
            send(&app, org, Method::POST, USERS, &admin, Some(body)).await
        })
    };
    let first = spawn("First");
    let second = spawn("Second");
    wait_for_lock_waiters(&pool, 2).await;
    lock.commit().await.unwrap();

    let mut statuses = Vec::new();
    for task in [first, second] {
        let answer = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("the creation finishes once the lock is released")
            .expect("the task does not panic");
        statuses.push((answer.status, answer.code().to_owned()));
    }
    statuses.sort();
    assert_eq!(
        statuses,
        [
            (StatusCode::CREATED, String::new()),
            (StatusCode::FORBIDDEN, "entitlement.limit_reached".into()),
        ]
    );
    assert_eq!(f.count(f.a).await, 2);
}

/// Waits until `n` backends of this database wait on a lock.
async fn wait_for_lock_waiters(pool: &PgPool, n: i64) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("count waiting backends");
        if waiting >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{n} requests never waited on a lock");
}

// Pre-provisioned identities (B9, B14, B18).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn identities_given_on_create_are_linked_to_the_new_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    f.provider(f.a, "vatsim").await;
    f.provider(f.a, "google").await;
    let body = json!({
        "display_name": "Sam",
        "identities": [
            { "provider_key": "vatsim", "subject": "1000001" },
            { "provider_key": "google", "subject": "1000001" },
        ],
    });

    let created = f.post(&admin, body).await;

    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(
        f.identities_of(created.id()).await,
        [
            ("vatsim".to_owned(), "1000001".to_owned()),
            ("google".to_owned(), "1000001".to_owned()),
        ]
    );
}

/// A refused identity rolls back the whole creation: no user, no identity.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_identity_creates_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    f.provider(f.a, "vatsim").await;
    f.provider(f.b, "org-b-only").await;
    let linked = f
        .post(
            &admin,
            json!({
                "display_name": "Linked",
                "identities": [{ "provider_key": "vatsim", "subject": "1000001" }],
            }),
        )
        .await;
    assert_eq!(linked.status, StatusCode::CREATED, "{linked:?}");
    let before = f.count(f.a).await;
    let with = |identities: Value| json!({ "display_name": "Sam", "identities": identities });
    let ok = json!({ "provider_key": "vatsim", "subject": "2000002" });

    let taken = f
        .post(
            &admin,
            with(json!([ok, { "provider_key": "vatsim", "subject": "1000001" }])),
        )
        .await;
    taken.refused(StatusCode::CONFLICT, "identity.taken");
    let twice = f.post(&admin, with(json!([ok, ok]))).await;
    twice.refused(StatusCode::CONFLICT, "identity.taken");
    f.post(
        &admin,
        with(json!([ok, { "provider_key": "nowhere", "subject": "1" }])),
    )
    .await
    .invalid("/identities/1/provider_key", "identity.unknown_provider");
    f.post(
        &admin,
        with(json!([{ "provider_key": "org-b-only", "subject": "1" }])),
    )
    .await
    .invalid("/identities/0/provider_key", "identity.unknown_provider");
    // The local provider exists but takes no pre-provisioned identity (spec 04 B3, Q4).
    f.post(
        &admin,
        with(json!([{ "provider_key": "local", "subject": "1" }])),
    )
    .await
    .invalid("/identities/0/provider_key", "identity.unknown_provider");
    f.post(
        &admin,
        with(json!([{ "provider_key": "vatsim", "subject": "" }])),
    )
    .await
    .invalid("/identities/0/subject", "identity.subject_empty");
    let too_long = "1".repeat(256);
    f.post(
        &admin,
        with(json!([ok, { "provider_key": "vatsim", "subject": too_long }])),
    )
    .await
    .invalid("/identities/1/subject", "text.too_long");
    let many: Vec<Value> = (0..21)
        .map(|i| json!({ "provider_key": "vatsim", "subject": format!("s{i}") }))
        .collect();
    f.post(&admin, with(json!(many)))
        .await
        .invalid("/identities", "identities.too_many");

    assert_eq!(f.count(f.a).await, before);
    let identities: i64 = sqlx::query_scalar("SELECT count(*) FROM identities")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(identities, 1);
}

// Reading (B10).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_the_list_filters_and_pages_in_name_order(pool: PgPool) {
    let f = Fixture::new(pool).await;
    // The admin is "Pat Example".
    let (admin, _) = f.admin().await;
    let sam = NewUser {
        email: Some("sam@example.test".into()),
        ..new_user("Sam Ortiz", Source::Local)
    };
    let sam = f.stored_user(f.a, sam).await;
    let mail = NewUser {
        email: Some("Sandy@example.test".into()),
        ..new_user("Robin Lee", Source::Idp)
    };
    f.stored_user(f.a, mail).await;
    f.stored_user(f.a, new_user("Alex Kim", Source::Local))
        .await;
    f.stored_user(f.b, new_user("Sam In B", Source::Local))
        .await;
    users::set_status(
        &f.pool,
        scope(f.a),
        sam,
        users::Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();

    let list = |query: &str| {
        let uri = format!("{USERS}{query}");
        let (f, admin) = (&f, &admin);
        async move { f.send(Method::GET, &uri, admin, None).await }
    };
    let all = list("").await;
    assert_eq!(all.status, StatusCode::OK, "{all:?}");
    assert_eq!(
        all.names(),
        ["Alex Kim", "Pat Example", "Robin Lee", "Sam Ortiz"]
    );
    assert_eq!(all.body["next_cursor"], Value::Null);
    for item in all.body["items"].as_array().unwrap() {
        assert_schema("User", item);
    }
    assert_eq!(list("?q=sa").await.names(), ["Robin Lee", "Sam Ortiz"]);
    assert_eq!(list("?q=SAM").await.names(), ["Sam Ortiz"]);
    assert_eq!(list("?status=suspended").await.names(), ["Sam Ortiz"]);
    assert_eq!(list("?source=idp").await.names(), ["Robin Lee"]);
    assert_eq!(list("?status=active&q=sa").await.names(), ["Robin Lee"]);

    let first = list("?limit=3").await;
    assert_eq!(first.names(), ["Alex Kim", "Pat Example", "Robin Lee"]);
    let cursor = first.body["next_cursor"].as_str().expect("a cursor");
    let second = list(&format!("?limit=3&cursor={cursor}")).await;
    assert_eq!(second.names(), ["Sam Ortiz"]);
    assert_eq!(second.body["next_cursor"], Value::Null);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_refuses_bad_parameters(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    for (query, code) in [
        ("?limit=0", "request.malformed"),
        ("?limit=201", "request.malformed"),
        ("?status=deleted", "request.malformed"),
        ("?source=ldap", "request.malformed"),
        ("?q=%00", "request.malformed"),
        ("?cursor=not-base64!", "pagination.invalid_cursor"),
        // ["\u0000", <uuid>]: a cursor whose name holds NUL.
        (
            "?cursor=WyJcdTAwMDAiLCIwMTkwMDAwMC0wMDAwLTcwMDAtODAwMC0wMDAwMDAwMDAwMDAiXQ",
            "pagination.invalid_cursor",
        ),
    ] {
        let answer = f
            .send(Method::GET, &format!("{USERS}{query}"), &admin, None)
            .await;
        answer.refused(StatusCode::BAD_REQUEST, code);
    }
    let at_most = f
        .send(Method::GET, &format!("{USERS}?limit=200"), &admin, None)
        .await;
    assert_eq!(at_most.status, StatusCode::OK);
}

// Changing (B6, B7).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_patch_changes_the_named_fields_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let sam = NewUser {
        email: Some("sam@example.test".into()),
        given_name: "Sam".into(),
        ..new_user("Sam", Source::Local)
    };
    let sam = f.stored_user(f.a, sam).await;
    f.clock.advance(Duration::from_secs(60));

    let changed = f
        .patch(
            &admin,
            sam,
            json!({ "display_name": "Sam Ortiz", "family_name": "Ortiz", "tz": "asia/tokyo" }),
        )
        .await;

    assert_eq!(changed.status, StatusCode::OK, "{changed:?}");
    assert_schema("User", &changed.body);
    assert_eq!(changed.body["display_name"], "Sam Ortiz");
    assert_eq!(changed.body["given_name"], "Sam");
    assert_eq!(changed.body["family_name"], "Ortiz");
    assert_eq!(changed.body["email"], "sam@example.test");
    assert_eq!(changed.body["tz"], "Asia/Tokyo");
    assert_eq!(changed.body["updated_at"], "2025-11-13T02:14:20.000000Z");

    let cleared = f.patch(&admin, sam, json!({ "email": null })).await;
    assert_eq!(cleared.body["email"], Value::Null, "{cleared:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_empty_patch_changes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let sam = f.stored_user(f.a, new_user("Sam", Source::Local)).await;
    let before = f.stored(sam).await;
    f.clock.advance(Duration::from_secs(60));

    let answer = f.patch(&admin, sam, json!({})).await;

    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["updated_at"], "2025-11-13T02:13:20.000000Z");
    assert_eq!(f.stored(sam).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_patch_refuses_null_unknown_and_invalid_members(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let sam = f.stored_user(f.a, new_user("Sam", Source::Local)).await;
    let robin = NewUser {
        email: Some("robin@example.test".into()),
        ..new_user("Robin", Source::Local)
    };
    f.stored_user(f.a, robin).await;
    let before = f.stored(sam).await;
    for body in [
        json!({ "display_name": null }),
        json!({ "status": null }),
        json!({ "status": "deleted" }),
        json!({ "external_id": "E1" }),
        json!({ "source": "idp" }),
    ] {
        let answer = f.patch(&admin, sam, body.clone()).await;
        answer.refused(StatusCode::BAD_REQUEST, "request.malformed");
    }
    f.patch(&admin, sam, json!({ "display_name": "" }))
        .await
        .invalid("/display_name", "name.empty");
    f.patch(&admin, sam, json!({ "locale": "fr" }))
        .await
        .invalid("/locale", "user.locale_unsupported");
    f.patch(&admin, sam, json!({ "tz": "Nowhere" }))
        .await
        .invalid("/tz", "tz.unknown");
    f.patch(&admin, sam, json!({ "email": "ROBIN@example.test" }))
        .await
        .refused(StatusCode::CONFLICT, "user.email_taken");
    assert_eq!(f.stored(sam).await, before);
}

/// T5 (B6) with Q3: an outside source owns the names and email of its user; the organization
/// keeps its locale, time zone and status.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_a_user_an_outside_source_owns_refuses_changes_to_its_fields(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let local = f.stored_user(f.a, new_user("Local", Source::Local)).await;
    for source in [Source::Idp, Source::Sync, Source::Scim, Source::Connector] {
        let owned = f.stored_user(f.a, new_user("Owned", source)).await;
        let before = f.stored(owned).await;
        for body in [
            json!({ "display_name": "Renamed" }),
            json!({ "given_name": "Renamed" }),
            json!({ "family_name": "Renamed" }),
            json!({ "email": "owned@example.test" }),
            json!({ "email": null }),
            json!({ "tz": "Asia/Tokyo", "display_name": "Renamed" }),
            json!({ "status": "suspended", "family_name": "Renamed" }),
        ] {
            let answer = f.patch(&admin, owned, body.clone()).await;
            answer.refused(StatusCode::CONFLICT, "user.externally_managed");
            assert_eq!(f.stored(owned).await, before, "{source:?} {body}");
        }
        let allowed = f
            .patch(
                &admin,
                owned,
                json!({ "tz": "Asia/Tokyo", "locale": "en", "status": "suspended" }),
            )
            .await;
        assert_eq!(allowed.status, StatusCode::OK, "{source:?}: {allowed:?}");
        assert_eq!(allowed.body["status"], "suspended");
        assert_eq!(allowed.body["tz"], "Asia/Tokyo");
    }
    let renamed = f
        .patch(&admin, local, json!({ "display_name": "Renamed" }))
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{renamed:?}");
    assert_eq!(renamed.body["display_name"], "Renamed");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_suspending_yourself_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, me) = f.admin().await;
    f.admin().await;
    let before = f.stored(me).await;

    let answer = f.patch(&admin, me, json!({ "status": "suspended" })).await;

    answer.refused(StatusCode::CONFLICT, "user.cannot_suspend_self");
    assert_eq!(f.stored(me).await, before);
    let renamed = f.patch(&admin, me, json!({ "display_name": "Me" })).await;
    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "editing yourself is allowed"
    );
}

/// B7: a suspension ends the user's sessions on their next request, writes one audit entry with
/// the caller and their address, and unsuspending lets a new session work.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn suspending_a_user_ends_their_sessions_and_is_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f.admin().await;
    let (cookie, sam) = f.user().await;
    let me = f.send(Method::GET, "/api/v1/me", &cookie, None).await;
    assert_eq!(me.status, StatusCode::OK);

    let suspended = f.patch(&admin, sam, json!({ "status": "suspended" })).await;
    assert_eq!(suspended.status, StatusCode::OK, "{suspended:?}");
    assert_eq!(suspended.body["status"], "suspended");
    let again = f.patch(&admin, sam, json!({ "status": "suspended" })).await;
    assert_eq!(again.status, StatusCode::OK);

    let me = f.send(Method::GET, "/api/v1/me", &cookie, None).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED, "{me:?}");
    let entries = f.status_entries(sam).await;
    assert_eq!(
        entries,
        [(
            Some(admin_id),
            Some("192.0.2.10".to_owned()),
            json!({ "before": { "status": "active" }, "after": { "status": "suspended" } }),
        )]
    );

    let restored = f.patch(&admin, sam, json!({ "status": "active" })).await;
    assert_eq!(restored.body["status"], "active", "{restored:?}");
    let fresh = f.sign_in(sam).await;
    let me = f.send(Method::GET, "/api/v1/me", &fresh, None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(f.status_entries(sam).await.len(), 2);
}

/// Spec 09 B14 through the route: the last active Organization admin cannot be suspended, and
/// the fields of the same patch are not changed either.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn suspending_the_last_organization_admin_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, last) = f.admin().await;
    // Every catalog permission, so the escalation rule passes and the last-admin rule decides.
    let every: Vec<&str> = stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect();
    let manager = f.holder(&every, Scope::Organization).await;
    let before = f.stored(last).await;

    let answer = f
        .patch(
            &manager,
            last,
            json!({ "status": "suspended", "tz": "Asia/Tokyo" }),
        )
        .await;

    answer.refused(StatusCode::CONFLICT, "role_assignment.last_org_admin");
    assert_eq!(f.stored(last).await, before);
    assert!(f.status_entries(last).await.is_empty());
}

// Permissions and organizations (T19, T20).

/// T19: `user.view` at an account does not reach the organization's users, and `user.view`
/// alone does not change them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19_the_routes_need_their_permission_at_organization_scope(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let root = f.root().await;
    let sam = f.stored_user(f.a, new_user("Sam", Source::Local)).await;
    let at_account = f
        .holder(&["user.view", "user.manage"], Scope::Account(root))
        .await;
    let account_manager = f.holder(&["account.manage"], Scope::Organization).await;
    let viewer = f.holder(&["user.view"], Scope::Organization).await;
    let user_uri = format!("{USERS}/{sam}");
    let body = json!({ "display_name": "Renamed" });

    for cookie in [&at_account, &account_manager] {
        for (method, uri) in [(Method::GET, USERS), (Method::GET, user_uri.as_str())] {
            let answer = f.send(method, uri, cookie, None).await;
            answer.refused(StatusCode::FORBIDDEN, "permission.denied");
        }
    }
    for cookie in [&at_account, &viewer] {
        f.post(cookie, body.clone())
            .await
            .refused(StatusCode::FORBIDDEN, "permission.denied");
        f.patch(cookie, sam, body.clone())
            .await
            .refused(StatusCode::FORBIDDEN, "permission.denied");
    }
    let read = f.send(Method::GET, &user_uri, &viewer, None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
}

/// T20: org A's Organization admin naming org B's user gets 404, and the row is unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_another_organizations_user_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let theirs = f
        .stored_user(f.b, new_user("Sam In B", Source::Local))
        .await;
    let before = f.stored(theirs).await;
    let uri = format!("{USERS}/{theirs}");

    f.send(Method::GET, &uri, &admin, None)
        .await
        .refused(StatusCode::NOT_FOUND, "user.not_found");
    for body in [
        json!({ "display_name": "Renamed" }),
        json!({ "status": "suspended" }),
        json!({}),
    ] {
        f.patch(&admin, theirs, body)
            .await
            .refused(StatusCode::NOT_FOUND, "user.not_found");
    }
    f.send(Method::GET, &format!("{USERS}/{}", new_id()), &admin, None)
        .await
        .refused(StatusCode::NOT_FOUND, "user.not_found");
    f.send(Method::GET, &format!("{USERS}/not-a-uuid"), &admin, None)
        .await
        .refused(StatusCode::BAD_REQUEST, "request.malformed");
    assert_eq!(f.stored(theirs).await, before);
}

// Escalation (owner decision of 2026-11-14).

/// Changing the email or status of a user who holds a permission the caller lacks at
/// organization scope is refused and changes nothing; a peer's is allowed, and so is your own.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_the_email_or_status_of_a_user_with_more_permissions_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, admin) = f.admin().await;
    let (manager, me) = f
        .holder_with_id(&["user.manage", "user.view"], Scope::Organization)
        .await;
    let root = f.root().await;
    let (_, account_viewer) = f
        .holder_with_id(&["account.view"], Scope::Account(root))
        .await;
    let (_, peer) = f
        .holder_with_id(&["user.manage"], Scope::Organization)
        .await;
    let plain = f.stored_user(f.a, new_user("Plain", Source::Local)).await;

    for target in [admin, account_viewer] {
        let before = f.stored(target).await;
        for body in [
            json!({ "email": "taken@example.test" }),
            json!({ "email": null }),
            json!({ "status": "suspended" }),
            json!({ "status": "active" }),
            json!({ "status": "suspended", "display_name": "Renamed" }),
        ] {
            f.patch(&manager, target, body.clone())
                .await
                .refused(StatusCode::FORBIDDEN, "permission.escalation");
            assert_eq!(f.stored(target).await, before, "{body}");
        }
        let other = f
            .patch(&manager, target, json!({ "tz": "Asia/Tokyo" }))
            .await;
        assert_eq!(
            other.status,
            StatusCode::OK,
            "other fields stay open: {other:?}"
        );
    }
    assert!(f.status_entries(admin).await.is_empty());

    for target in [peer, plain] {
        let email = json!({ "email": format!("{target}@example.test") });
        let changed = f.patch(&manager, target, email).await;
        assert_eq!(changed.status, StatusCode::OK, "{changed:?}");
        let suspended = f
            .patch(&manager, target, json!({ "status": "suspended" }))
            .await;
        assert_eq!(suspended.status, StatusCode::OK, "{suspended:?}");
    }
    let own = f
        .patch(&manager, me, json!({ "email": "me@example.test" }))
        .await;
    assert_eq!(own.status, StatusCode::OK, "your own email: {own:?}");
    assert_eq!(own.body["email"], "me@example.test");
}

/// A field change and a suspension in one PATCH, and a suspension alone, of the same user, both
/// waiting on the organization row this test holds: they take the organization row and the user
/// row in the same order, so both finish.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn concurrent_suspensions_with_and_without_fields_do_not_deadlock(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    let (admin, _) = f.admin().await;
    f.admin().await;
    let sam = f.stored_user(f.a, new_user("Sam", Source::Local)).await;
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(f.a)
        .execute(&mut *lock)
        .await
        .unwrap();

    let spawn = |body: Value| {
        let (app, org, admin) = (f.app.clone(), f.a, admin.clone());
        let uri = format!("{USERS}/{sam}");
        tokio::spawn(async move { send(&app, org, Method::PATCH, &uri, &admin, Some(body)).await })
    };
    // The suspension alone queues first, so it gets the organization row first: a PATCH that
    // held the user row while it waited for the organization row would deadlock with it.
    let status_only = spawn(json!({ "status": "suspended" }));
    wait_for_lock_waiters(&pool, 1).await;
    let with_fields = spawn(json!({ "status": "suspended", "display_name": "Sam Ortiz" }));
    wait_for_lock_waiters(&pool, 2).await;
    lock.commit().await.unwrap();

    for task in [with_fields, status_only] {
        let answer = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("the PATCH finishes once the organization row is free")
            .expect("the task does not panic");
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    }
    let read = users::get(&f.pool, scope(f.a), sam).await.unwrap().unwrap();
    assert_eq!(
        (read.display_name.as_str(), read.status),
        ("Sam Ortiz", users::Status::Suspended)
    );
}

/// The body is checked before the users limit: an organization at its limit still gets the 422.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_that_breaks_a_rule_answers_422_at_the_limit(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    f.limit_users(1).await;
    f.provider(f.a, "vatsim").await;
    let long_subject = "1".repeat(256);
    let identities = json!([{ "provider_key": "vatsim", "subject": long_subject }]);

    f.post(&admin, json!({ "display_name": " " }))
        .await
        .invalid("/display_name", "name.empty");
    f.post(&admin, json!({ "display_name": "Sam", "tz": "Nowhere" }))
        .await
        .invalid("/tz", "tz.unknown");
    f.post(
        &admin,
        json!({ "display_name": "Sam", "identities": identities }),
    )
    .await
    .invalid("/identities/0/subject", "text.too_long");
    f.post(&admin, json!({ "display_name": "Sam" }))
        .await
        .refused(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(f.count(f.a).await, 1);
}

/// The guarded PATCH reads the target's grants only after the account tree lock: an
/// Organization-admin assignment whose transaction holds that lock when the PATCH arrives is
/// visible to the check once it commits, so the email change is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_assignment_committed_while_the_patch_waits_is_checked(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    let manager = f.holder(&["user.manage"], Scope::Organization).await;
    let plain = f.stored_user(f.a, new_user("Plain", Source::Local)).await;
    let before = f.stored(plain).await;
    let org_admin: Uuid =
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = 'org_admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut assigning = pool.begin().await.unwrap();
    stepclimb_core::accounts::lock_tree(&mut assigning, scope(f.a))
        .await
        .unwrap();
    let assignment = NewAssignment {
        user_id: plain,
        role_id: org_admin,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(
        &mut *assigning,
        scope(f.a),
        &assignment,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();

    let patch = {
        let (app, org, manager) = (f.app.clone(), f.a, manager.clone());
        let uri = format!("{USERS}/{plain}");
        let body = json!({ "email": "plain@example.test" });
        tokio::spawn(
            async move { send(&app, org, Method::PATCH, &uri, &manager, Some(body)).await },
        )
    };
    wait_for_lock_waiters(&pool, 1).await;
    assigning.commit().await.unwrap();

    let answer = tokio::time::timeout(Duration::from_secs(10), patch)
        .await
        .expect("the PATCH finishes once the assignment commits")
        .expect("the task does not panic");
    answer.refused(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.stored(plain).await, before);
}

/// The guarded PATCH locks the target's custom roles `FOR KEY SHARE` before it reads their
/// permissions: an edit that gives one of them a permission the caller lacks, holding the role
/// row when the PATCH arrives, is visible to the check once it commits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_role_edit_committed_while_the_patch_waits_is_checked(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    let manager = f.holder(&["user.manage"], Scope::Organization).await;
    let (_, peer) = f
        .holder_with_id(&["user.manage"], Scope::Organization)
        .await;
    let role: Uuid = sqlx::query_scalar("SELECT role_id FROM role_assignments WHERE user_id = $1")
        .bind(peer)
        .fetch_one(&pool)
        .await
        .unwrap();
    let before = f.stored(peer).await;
    let mut editing = pool.begin().await.unwrap();
    let patch = roles::RolePatch {
        name: None,
        permissions: Some(vec!["user.manage".into(), "organization.manage".into()]),
    };
    roles::update_custom(
        &mut *editing,
        scope(f.a),
        role,
        &patch,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();

    let request = {
        let (app, org, manager) = (f.app.clone(), f.a, manager.clone());
        let uri = format!("{USERS}/{peer}");
        let body = json!({ "status": "suspended" });
        tokio::spawn(
            async move { send(&app, org, Method::PATCH, &uri, &manager, Some(body)).await },
        )
    };
    wait_for_lock_waiters(&pool, 1).await;
    editing.commit().await.unwrap();

    let answer = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("the PATCH finishes once the edit commits")
        .expect("the task does not panic");
    answer.refused(StatusCode::FORBIDDEN, "permission.escalation");
    assert_eq!(f.stored(peer).await, before);
}

/// A PATCH that suspends and renames, queued on the organization row, holds no lock on the user
/// row: another transaction gets that row at once. A PATCH that updated the fields first would
/// hold it while it waits, and this would time out (or deadlock).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspension_waiting_on_the_organization_row_holds_no_user_row(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    let (admin, _) = f.admin().await;
    let sam = f.stored_user(f.a, new_user("Sam", Source::Local)).await;
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(f.a)
        .execute(&mut *holder)
        .await
        .unwrap();

    let request = {
        let (app, org, admin) = (f.app.clone(), f.a, admin.clone());
        let uri = format!("{USERS}/{sam}");
        let body = json!({ "status": "suspended", "display_name": "X" });
        tokio::spawn(async move { send(&app, org, Method::PATCH, &uri, &admin, Some(body)).await })
    };
    wait_for_lock_waiters(&pool, 1).await;
    sqlx::raw_sql("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *holder)
        .await
        .unwrap();
    let user_row = sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(sam)
        .execute(&mut *holder)
        .await;
    assert!(
        user_row.is_ok(),
        "the PATCH held the user row: {user_row:?}"
    );
    holder.commit().await.unwrap();

    let answer = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("the PATCH finishes once the rows are free")
        .expect("the task does not panic");
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["display_name"], "X");
}
