// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization routes and the `RequestOrg` extractor over the router
//! (docs/specs/m0/02-organizations-and-accounts.md B11 to B14, T6 to T9). The permission suite's
//! cases for each route are also in `tests/permissions.rs`.
//!
//! The production request-organization resolver serves these tests: no request carries the
//! `test-support` organization header, except the one cross-organization read. `session::start`
//! stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::get,
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
    organizations, perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    request_org::{RequestOrg, TEST_ORG_HEADER},
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

mod common;

fn document() -> Value {
    serde_json::to_value(stepclimb_server::openapi::document()).expect("the document as JSON")
}

/// `value` conforms to the component schema `name`, with no undeclared member.
fn assert_schema(name: &str, value: &Value) {
    let errors = common::schema_errors(&document(), name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

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

const ORGANIZATION: &str = "/api/v1/organization";
const SETTINGS: &str = "/api/v1/organization/settings";

fn state(pool: &PgPool, clock: &ManualClock) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
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

    fn is_problem(&self) -> bool {
        self.headers.get(header::CONTENT_TYPE).map(|v| v.as_bytes())
            == Some(b"application/problem+json")
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

/// One organization, `org-a`, and the router over it.
struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
    root: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        let clock = ManualClock::new(now());
        let app = router(state(&pool, &clock)).layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            clock,
            app,
            org,
            root,
        }
    }

    /// A new signed-in user of `org` with no role: their `Cookie` value and id.
    async fn user_of(&self, org: Uuid) -> (String, Uuid) {
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

    async fn assign(&self, user: Uuid, role: Uuid, scope_: Scope) {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: scope_,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.org),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
    }

    /// A signed-in Organization admin.
    async fn admin(&self) -> String {
        let (cookie, user) = self.user_of(self.org).await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind("org_admin")
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(user, role, Scope::Organization).await;
        cookie
    }

    /// A signed-in user holding every permission but `organization.manage`, at the root account.
    async fn account_manager(&self) -> String {
        let (cookie, user) = self.user_of(self.org).await;
        let keys = perm::ALL
            .iter()
            .map(|p| p.key())
            .filter(|key| *key != <perm::OrgManage as perm::Permission>::KEY)
            .map(str::to_owned)
            .collect();
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Account manager".into(),
            permissions: keys,
        };
        let role = roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, Scope::Account(self.root)).await;
        cookie
    }

    async fn send(&self, method: Method, uri: &str, cookie: Option<&str>, body: &str) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if !body.is_empty() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        answer(
            &self.app,
            request
                .body(Body::from(body.to_owned()))
                .expect("a request"),
        )
        .await
    }

    async fn updated_at(&self) -> SystemTime {
        let micros: i64 = sqlx::query_scalar(
            "SELECT (extract(epoch FROM updated_at) * 1000000)::bigint FROM organizations
             WHERE id = $1",
        )
        .bind(self.org)
        .fetch_one(&self.pool)
        .await
        .expect("the organization");
        UNIX_EPOCH + Duration::from_micros(micros.try_into().expect("after the epoch"))
    }
}

// T6: RequestOrg.

/// A route that answers the id of the organization `RequestOrg` resolves.
fn request_org_app(pool: &PgPool) -> Router {
    async fn org(org: RequestOrg) -> String {
        org.scope().org_id().to_string()
    }
    Router::new()
        .route("/t/org", get(org))
        .with_state(state(pool, &ManualClock::new(now())))
}

async fn request_org_answer(app: &Router, host: &str) -> Answer {
    let request = Request::get("/t/org")
        .header(header::HOST, host)
        .body(Body::empty())
        .expect("a request");
    answer(app, request).await
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_with_one_organization_every_host_resolves_to_it(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", now())
        .await
        .expect("create organization");
    let app = request_org_app(&pool);
    for host in ["lms.example.org", "other.example.net", "127.0.0.1:8080"] {
        let answer = request_org_answer(&app, host).await;
        assert_eq!(answer.status, StatusCode::OK, "{host}: {answer:?}");
        assert_eq!(answer.body, Value::String(org.id.to_string()), "{host}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_with_no_organization_the_extractor_answers_org_none(pool: PgPool) {
    let answer = request_org_answer(&request_org_app(&pool), "lms.example.org").await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer.code(), "org.none");
    assert!(answer.is_problem(), "{answer:?}");
    let detail = answer.body["detail"].as_str().expect("a detail");
    assert!(detail.contains("stepclimb admin create"), "{detail}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_with_two_organizations_the_extractor_answers_org_not_found(pool: PgPool) {
    for slug in ["org-a", "org-b"] {
        organizations::create(&pool, slug, slug, now())
            .await
            .expect("create organization");
    }
    let app = request_org_app(&pool);
    for host in ["org-a.example.org", "lms.example.org"] {
        let answer = request_org_answer(&app, host).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{host}");
        assert_eq!(answer.code(), "org.not_found", "{host}");
        assert!(answer.is_problem(), "{answer:?}");
    }
}

// T7: reading the organization.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_any_principal_reads_its_organization_and_anonymous_gets_401(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let anonymous = f.send(Method::GET, ORGANIZATION, None, "").await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.code(), "auth.unauthenticated");

    let (cookie, _) = f.user_of(f.org).await;
    let answer = f.send(Method::GET, ORGANIZATION, Some(&cookie), "").await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body,
        json!({ "id": f.org.to_string(), "slug": "org-a", "name": "Org A" })
    );
    assert_schema("Organization", &answer.body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_principal_of_org_b_sees_org_b(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization");
    let (cookie, _) = f.user_of(b.id).await;
    let request = Request::get(ORGANIZATION)
        .header(header::COOKIE, cookie)
        .header(TEST_ORG_HEADER, b.id.to_string())
        .body(Body::empty())
        .expect("a request");
    let answer = answer(&f.app, request).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body,
        json!({ "id": b.id.to_string(), "slug": "org-b", "name": "Org B" })
    );
}

// T8 and T1: renaming the organization.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_renaming_needs_organization_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let body = r#"{"name": "Renamed"}"#;
    let anonymous = f.send(Method::PATCH, ORGANIZATION, None, body).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let manager = f.account_manager().await;
    let denied = f
        .send(Method::PATCH, ORGANIZATION, Some(&manager), body)
        .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{denied:?}");
    assert_eq!(denied.code(), "permission.denied");
    let (plain, _) = f.user_of(f.org).await;
    let unchanged = f.send(Method::GET, ORGANIZATION, Some(&plain), "").await;
    assert_eq!(unchanged.body["name"], "Org A");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_an_admin_renames_the_organization_and_a_following_get_sees_it(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.clock.advance(Duration::from_secs(60));
    let answer = f
        .send(
            Method::PATCH,
            ORGANIZATION,
            Some(&admin),
            r#"{"name": "  Renamed Org  "}"#,
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body,
        json!({ "id": f.org.to_string(), "slug": "org-a", "name": "Renamed Org" })
    );
    assert_schema("Organization", &answer.body);
    assert_eq!(f.updated_at().await, now() + Duration::from_secs(60));
    let (plain, _) = f.user_of(f.org).await;
    let read = f.send(Method::GET, ORGANIZATION, Some(&plain), "").await;
    assert_eq!(read.body["name"], "Renamed Org");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_patch_without_a_name_changes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.clock.advance(Duration::from_secs(60));
    let answer = f
        .send(Method::PATCH, ORGANIZATION, Some(&admin), "{}")
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["name"], "Org A");
    assert_eq!(f.updated_at().await, now(), "updated_at does not move");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_bad_rename_bodies_answer_problem_details(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let too_long = format!(r#"{{"name": "{}"}}"#, "a".repeat(201));
    let cases: [(&str, StatusCode, &str, Option<&str>); 7] = [
        (
            r#"{"name": "   "}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("name.empty"),
        ),
        (
            &too_long,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("name.too_long"),
        ),
        (
            r#"{"name": "a\u0000b"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request.invalid",
            Some("text.contains_nul"),
        ),
        (
            r#"{"name": "#,
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            r#"{"name": null}"#,
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            r#"{"name": 7}"#,
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
        (
            r#"{"slug": "new-slug"}"#,
            StatusCode::BAD_REQUEST,
            "request.malformed",
            None,
        ),
    ];
    for (body, status, code, field_code) in cases {
        let answer = f
            .send(Method::PATCH, ORGANIZATION, Some(&admin), body)
            .await;
        assert_eq!((answer.status, answer.code()), (status, code), "{body}");
        assert!(answer.is_problem(), "{body}: {answer:?}");
        let expected: Vec<(String, String)> = field_code
            .map(|c| ("/name".to_owned(), c.to_owned()))
            .into_iter()
            .collect();
        assert_eq!(answer.errors(), expected, "{body}");
    }
    let request = Request::patch(ORGANIZATION)
        .header(header::COOKIE, &admin)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(r#"{"name": "Renamed"}"#))
        .expect("a request");
    let wrong_type = answer(&f.app, request).await;
    assert_eq!(wrong_type.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(wrong_type.code(), "request.unsupported_media_type");
    let (plain, _) = f.user_of(f.org).await;
    let read = f.send(Method::GET, ORGANIZATION, Some(&plain), "").await;
    assert_eq!(read.body["name"], "Org A");
    assert_eq!(f.updated_at().await, now());
}

// T9: settings.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_settings_need_organization_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let manager = f.account_manager().await;
    for (method, body) in [(Method::GET, ""), (Method::PATCH, "{}")] {
        let anonymous = f.send(method.clone(), SETTINGS, None, body).await;
        assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED, "{method}");
        assert_eq!(anonymous.code(), "auth.unauthenticated", "{method}");
        let denied = f.send(method.clone(), SETTINGS, Some(&manager), body).await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN, "{method}");
        assert_eq!(denied.code(), "permission.denied", "{method}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_new_organization_reads_every_setting_at_its_default(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let answer = f.send(Method::GET, SETTINGS, Some(&admin), "").await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let defaults = serde_json::to_value(stepclimb_core::OrgSettings::default()).unwrap();
    assert_eq!(answer.body, defaults);
    assert_schema("OrgSettings", &answer.body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_an_unknown_key_is_refused_and_nothing_is_written(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.clock.advance(Duration::from_secs(60));
    let answer = f
        .send(
            Method::PATCH,
            SETTINGS,
            Some(&admin),
            r#"{"no_such_key": true, "other": 1}"#,
        )
        .await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{answer:?}"
    );
    assert_eq!(answer.code(), "request.invalid");
    assert!(answer.is_problem());
    assert_eq!(
        answer.errors(),
        [
            ("/no_such_key".to_owned(), "settings.unknown_key".to_owned()),
            ("/other".to_owned(), "settings.unknown_key".to_owned()),
        ]
    );
    assert_eq!(f.updated_at().await, now());
    let stored: String =
        sqlx::query_scalar("SELECT settings::text FROM organizations WHERE id = $1")
            .bind(f.org)
            .fetch_one(&f.pool)
            .await
            .expect("the organization");
    assert_eq!(stored, "{}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_an_empty_patch_writes_nothing_and_returns_every_setting(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    // A key a newer release wrote: kept in the row, not answered.
    sqlx::query("UPDATE organizations SET settings = '{\"newer_key\": 1}' WHERE id = $1")
        .bind(f.org)
        .execute(&f.pool)
        .await
        .expect("seed a stored key");
    f.clock.advance(Duration::from_secs(60));
    let answer = f.send(Method::PATCH, SETTINGS, Some(&admin), "{}").await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let defaults = serde_json::to_value(stepclimb_core::OrgSettings::default()).unwrap();
    assert_eq!(answer.body, defaults);
    assert_schema("OrgSettings", &answer.body);
    assert_eq!(f.updated_at().await, now(), "updated_at does not move");
    let stored: Value = sqlx::query_scalar("SELECT settings FROM organizations WHERE id = $1")
        .bind(f.org)
        .fetch_one(&f.pool)
        .await
        .expect("the organization");
    assert_eq!(stored, json!({ "newer_key": 1 }));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_settings_body_that_is_not_an_object_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    for body in [
        "[]",
        "null",
        "\"x\"",
        "{",
        r#"{"no_such_key": null}"#,
        r#"{"no_such_key": 1, "other": null}"#,
    ] {
        let answer = f.send(Method::PATCH, SETTINGS, Some(&admin), body).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(answer.code(), "request.malformed", "{body}");
    }
}

// The settings schemas mirror the core types.

#[test]
fn the_documented_settings_describe_what_the_routes_serialize() {
    let served = serde_json::to_value(stepclimb_core::OrgSettings::default()).unwrap();
    assert_schema("OrgSettings", &served);
    let doc = document();
    let schemas = &doc["components"]["schemas"];
    let members = |name: &str| -> Vec<String> {
        schemas[name]["properties"]
            .as_object()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default()
    };
    assert_eq!(members("OrgSettingsPatch"), members("OrgSettings"));
    assert!(schemas["OrgSettingsPatch"].get("required").is_none());
    assert_eq!(schemas["OrgSettingsPatch"]["additionalProperties"], false);
}

#[test]
fn the_rename_patch_documents_a_name_that_is_not_nullable() {
    let doc = document();
    let name = &doc["components"]["schemas"]["OrganizationPatch"]["properties"]["name"];
    assert_eq!(name["type"], "string", "{name}");
    let required = &doc["components"]["schemas"]["OrganizationPatch"]["required"];
    assert!(required.is_null(), "{required}");
}
