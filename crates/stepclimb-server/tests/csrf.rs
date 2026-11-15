// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CSRF layer, sign-out and HSTS through the whole router (docs/specs/m0/08-sessions-and-csrf.md
//! B12 to B16, B39; T11 to T15, T17, T37). A refused request is shown not to reach its handler by
//! the session row sign-out would have deleted. Needs `DATABASE_URL` (`just up`); the SPA cases
//! need the web build (`just web-build`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use sqlx::{PgPool, postgres::PgPoolOptions};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    clock::ManualClock,
    id::{Uuid, new_id},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    app::HSTS,
    error::Problem,
    openapi::{self, MUTATES},
    router,
    session::{self, Started},
    web_origin::WebOrigin,
};
use tower::ServiceExt;

const PUBLIC: &str = "https://lms.example.org";
const ALLOWED: &str = "https://app.example.org";
const CLEARED: &str = "__Host-sc_session=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the request-organization resolver of the sign-in specs"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn state(db: PgPool, public_url: &str) -> AppState {
    AppState {
        db,
        clock: ManualClock::new(now()).clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse(public_url).expect("a valid origin"),
        allowed_origins: vec![WebOrigin::parse(ALLOWED).expect("a valid origin")],
        rate_limits: Default::default(),
        passwords: Default::default(),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// A pool on a port nothing listens on, for requests that must not reach the database.
fn unreachable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(500))
        .connect_lazy("postgres://stepclimb@127.0.0.1:1/none")
        .expect("a lazy pool parses its URL")
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Answer {
    fn code(&self) -> String {
        serde_json::from_str::<Problem>(&self.body)
            .unwrap_or_else(|_| panic!("not a problem: {}", self.body))
            .code
    }

    fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("an ASCII Set-Cookie"))
            .collect()
    }

    fn assert_refused(&self, case: &str) {
        assert_eq!(self.status, StatusCode::FORBIDDEN, "{case}: {}", self.body);
        assert_eq!(self.code(), "auth.csrf_refused", "{case}");
        assert!(self.set_cookies().is_empty(), "{case}: nothing is cleared");
    }
}

async fn send(app: Router, method: Method, path: &str, headers: &[(&str, &str)]) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("a valid request"))
        .await
        .expect("the router never fails");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    user: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = insert_org(&pool, "org-a").await;
        let user = create_user(&pool, org).await;
        Self { pool, org, user }
    }

    fn app(&self) -> Router {
        router(state(self.pool.clone(), PUBLIC))
    }

    async fn sign_in(&self) -> String {
        sign_in(&self.pool, self.org, self.user).await
    }

    /// `POST /auth/sign-out` with the session cookie `cookie` and the extra `headers`.
    async fn sign_out(&self, cookie: &str, headers: &[(&str, &str)]) -> Answer {
        let mut all = vec![("cookie", cookie)];
        all.extend_from_slice(headers);
        send(self.app(), Method::POST, "/auth/sign-out", &all).await
    }

    async fn session_count(&self) -> i64 {
        session_count(&self.pool).await
    }
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

async fn create_user(pool: &PgPool, org: Uuid) -> Uuid {
    let user = NewUser {
        display_name: "Sam Ortiz".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    users::create(pool, scope(org), &user, now())
        .await
        .expect("create user")
        .id
}

/// Start a session as a sign-in does and return the `Cookie` pair for it.
async fn sign_in(pool: &PgPool, org: Uuid, user: Uuid) -> String {
    let Started { set_cookie, .. } =
        session::start(pool, scope(org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
    let value = set_cookie.to_str().expect("an ASCII Set-Cookie");
    value.split(';').next().expect("a cookie pair").to_owned()
}

async fn session_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(pool)
        .await
        .expect("count sessions")
}

// T11 (B13).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_sign_out_deletes_the_session_clears_the_cookie_and_repeats(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;
    let other_browser = fixture.sign_in().await;
    let same_origin = [("sec-fetch-site", "same-origin")];

    let first = fixture.sign_out(&cookie, &same_origin).await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.body);
    assert_eq!(first.set_cookies(), [CLEARED]);
    assert_eq!(fixture.session_count().await, 1, "only this browser's row");

    let again = fixture.sign_out(&cookie, &same_origin).await;
    assert_eq!(again.status, StatusCode::NO_CONTENT);
    assert_eq!(again.set_cookies(), [CLEARED]);

    let without_cookie = send(fixture.app(), Method::POST, "/auth/sign-out", &same_origin).await;
    assert_eq!(without_cookie.status, StatusCode::NO_CONTENT);
    assert_eq!(without_cookie.set_cookies(), [CLEARED]);
    assert_eq!(fixture.session_count().await, 1);

    let still_signed_in = fixture.sign_out(&other_browser, &same_origin).await;
    assert_eq!(still_signed_in.status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.session_count().await, 0);
}

#[tokio::test]
async fn sign_out_without_a_cookie_does_not_touch_the_database() {
    let app = router(state(unreachable_pool(), PUBLIC));
    let answer = send(
        app,
        Method::POST,
        "/auth/sign-out",
        &[("sec-fetch-site", "same-origin")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{}", answer.body);
    assert_eq!(answer.set_cookies(), [CLEARED]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn sign_out_with_two_session_cookies_ends_both(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let first = fixture.sign_in().await;
    let second = fixture.sign_in().await;

    let answer = fixture
        .sign_out(
            &format!("{first}; theme=dark; {second}"),
            &[("sec-fetch-site", "same-origin")],
        )
        .await;

    assert_eq!(answer.status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.session_count().await, 0);
}

/// Cross-organization: a session of organization B is never ended through a request that is not
/// B's. With two organizations, M0 cannot tell which one a request belongs to (spec 08 B5, Q1),
/// so sign-out answers `org.not_found` and deletes neither session. That a sign-out scoped to A
/// cannot delete B's row is `delete_by_token_hash_ends_the_named_session_of_the_scope_only` in
/// stepclimb-core.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn sign_out_never_deletes_another_organizations_session(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let org_b = insert_org(&fixture.pool, "org-b").await;
    let user_b = create_user(&fixture.pool, org_b).await;
    let cookie_b = sign_in(&fixture.pool, org_b, user_b).await;
    let cookie_a = fixture.sign_in().await;

    for cookie in [&cookie_a, &cookie_b] {
        let answer = fixture
            .sign_out(cookie, &[("sec-fetch-site", "same-origin")])
            .await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        assert_eq!(answer.code(), "org.not_found");
        assert!(answer.set_cookies().is_empty(), "the cookie is left (B13)");
    }
    assert_eq!(fixture.session_count().await, 2);
}

// T12 (B14): Fetch Metadata.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_only_a_same_origin_fetch_reaches_the_handler(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;

    for site in ["same-site", "cross-site", "none"] {
        let answer = fixture
            .sign_out(&cookie, &[("sec-fetch-site", site), ("origin", PUBLIC)])
            .await;
        answer.assert_refused(site);
        assert_eq!(
            fixture.session_count().await,
            1,
            "{site}: the handler did not run"
        );
    }

    let answer = fixture
        .sign_out(&cookie, &[("sec-fetch-site", "same-origin")])
        .await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.session_count().await, 0);
}

// T13 (B14), T14 (B15, Q6): Origin when Fetch Metadata is absent.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_without_fetch_metadata_only_an_allowed_origin_passes(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;

    for origin in [
        "https://evil.example",
        "http://lms.example.org",
        "https://lms.example.org.evil.example",
        "null",
    ] {
        fixture
            .sign_out(&cookie, &[("origin", origin)])
            .await
            .assert_refused(origin);
        assert_eq!(fixture.session_count().await, 1, "{origin}");
    }

    for origin in [PUBLIC, ALLOWED] {
        let cookie = fixture.sign_in().await;
        let answer = fixture.sign_out(&cookie, &[("origin", origin)]).await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{origin}");
    }
    assert_eq!(fixture.session_count().await, 1, "only the refused one's");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_a_request_with_neither_header_is_refused(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;

    fixture.sign_out(&cookie, &[]).await.assert_refused("none");

    assert_eq!(fixture.session_count().await, 1);
}

#[tokio::test]
async fn every_unsafe_method_on_any_path_is_checked_before_routing() {
    for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
        for path in [
            "/auth/sign-out",
            "/healthz",
            "/api/v1/nothing-here",
            "/c/s1",
        ] {
            let app = router(state(unreachable_pool(), PUBLIC));
            let answer = send(
                app,
                method.clone(),
                path,
                &[("sec-fetch-site", "cross-site")],
            )
            .await;
            answer.assert_refused(&format!("{method} {path}"));
        }
    }
}

#[tokio::test]
async fn safe_methods_are_never_checked() {
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        let app = router(state(unreachable_pool(), PUBLIC));
        let answer = send(
            app,
            method.clone(),
            "/healthz",
            &[("sec-fetch-site", "cross-site")],
        )
        .await;
        assert_ne!(answer.status, StatusCode::FORBIDDEN, "{method}");
    }
}

// B14 (coordinator decision, 13 Nov 2026): an `Origin` that is sent must be allowed, even on a
// same-origin fetch.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_dns_rebound_page_is_refused_though_its_fetch_is_same_origin(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;

    let answer = fixture
        .sign_out(
            &cookie,
            &[
                ("host", "rebound.evil.example"),
                ("sec-fetch-site", "same-origin"),
                ("origin", "http://rebound.evil.example"),
            ],
        )
        .await;

    answer.assert_refused("rebound");
    assert_eq!(fixture.session_count().await, 1, "the handler did not run");
}

/// `just dev`: the browser loads the web app from Vite on :5173, which proxies API requests to
/// the server, so the browser's `Origin` is Vite's. The development configuration lists it in
/// `allowed_origins`; a configuration without it refuses it.
#[tokio::test]
async fn the_vite_dev_origin_passes_only_where_it_is_configured() {
    let vite = [
        ("sec-fetch-site", "same-origin"),
        ("origin", "http://localhost:5173"),
    ];
    let mut development = state(unreachable_pool(), "http://localhost:8080");
    development.allowed_origins =
        vec![WebOrigin::parse("http://localhost:5173").expect("an origin")];
    let answer = send(router(development), Method::POST, "/auth/sign-out", &vite).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{}", answer.body);

    let production = router(state(unreachable_pool(), PUBLIC));
    send(production, Method::POST, "/auth/sign-out", &vite)
        .await
        .assert_refused("vite origin without the development configuration");
}

// T15 (B16): a bearer request skips the check; the cookie beside it is not acted on.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_bearer_request_passes_the_layer_and_its_cookie_is_ignored(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in().await;

    let only_cookie = fixture
        .sign_out(&cookie, &[("sec-fetch-site", "cross-site")])
        .await;
    only_cookie.assert_refused("cookie only");

    let bearer = fixture
        .sign_out(
            &cookie,
            &[
                ("sec-fetch-site", "cross-site"),
                ("authorization", "Bearer sc_not-a-real-key_000000"),
            ],
        )
        .await;
    assert_eq!(
        bearer.status,
        StatusCode::NO_CONTENT,
        "the layer let it through"
    );
    assert_eq!(
        fixture.session_count().await,
        1,
        "a bearer request does not act on the session cookie"
    );

    let basic = fixture
        .sign_out(
            &cookie,
            &[
                ("sec-fetch-site", "cross-site"),
                ("authorization", "Basic dTpw"),
            ],
        )
        .await;
    basic.assert_refused("Basic is not a bearer credential");
}

// T17 (B12): the router walk.

const SAFE: [&str; 3] = ["get", "head", "options"];
const METHODS: [&str; 7] = ["get", "head", "options", "post", "put", "patch", "delete"];

/// Every operation in the API document, as (path, method, operation).
fn operations(doc: &serde_json::Value) -> Vec<(String, String, serde_json::Value)> {
    let mut all = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            if METHODS.contains(&method.as_str()) {
                all.push((path.clone(), method.clone(), operation.clone()));
            }
        }
    }
    all
}

fn document() -> serde_json::Value {
    serde_json::to_value(openapi::document()).expect("the document serializes")
}

#[test]
fn t17_no_safe_operation_is_marked_as_mutating() {
    let offenders: Vec<String> = operations(&document())
        .into_iter()
        .filter(|(_, method, operation)| {
            SAFE.contains(&method.as_str()) && operation.get(MUTATES).is_some()
        })
        .map(|(path, method, operation)| format!("{method} {path} ({})", operation["operationId"]))
        .collect();
    assert!(
        offenders.is_empty(),
        "these safe operations change state (B12); make them POST, PUT, PATCH or DELETE: \
         {offenders:?}"
    );
}

#[test]
fn t17_the_walk_sees_the_marker_on_sign_out() {
    let doc = document();
    assert_eq!(
        doc["paths"]["/auth/sign-out"]["post"][MUTATES],
        serde_json::Value::Bool(true),
        "the marker's name in auth.rs matches openapi::MUTATES"
    );
}

/// The walk reads the document, so the router must register each documented path with exactly
/// the documented methods: a mutating handler mounted with `get(...)` in `app.rs` while its
/// `#[utoipa::path]` says `post` answers a GET here instead of 405.
#[tokio::test]
async fn t17_the_router_takes_exactly_the_documented_methods_on_each_path() {
    let doc = document();
    let mut mismatches = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        let uri = path.replace(['{', '}'], "");
        for method in ["get", "post", "put", "patch", "delete"] {
            let documented = item.get(method).is_some();
            let app = router(state(unreachable_pool(), PUBLIC));
            let answer = send(
                app,
                Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method"),
                &uri,
                &[("sec-fetch-site", "same-origin")],
            )
            .await;
            let routed = answer.status != StatusCode::METHOD_NOT_ALLOWED;
            if routed != documented {
                mismatches.push(format!("{method} {path}: {}", answer.status));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "router and document disagree: {mismatches:?}"
    );
}

// T37 (B39): HSTS.

/// A request: its method, path and headers.
type Case = (
    Method,
    &'static str,
    &'static [(&'static str, &'static str)],
);

#[tokio::test]
async fn t37_an_https_public_url_sends_hsts_on_every_response() {
    let cases: [Case; 3] = [
        (Method::GET, "/healthz", &[]),
        (
            Method::POST,
            "/auth/sign-out",
            &[("sec-fetch-site", "cross-site")],
        ),
        (Method::GET, "/api/v1/nothing-here", &[]),
    ];
    for (method, path, headers) in cases {
        let app = router(state(unreachable_pool(), PUBLIC));
        let answer = send(app, method.clone(), path, headers).await;
        assert_eq!(
            answer
                .headers
                .get(header::STRICT_TRANSPORT_SECURITY)
                .map(|v| v.to_str().unwrap()),
            Some(HSTS),
            "{method} {path} ({})",
            answer.status
        );
    }
    assert_eq!(HSTS, "max-age=31536000");
}

#[tokio::test]
async fn t37_an_http_public_url_sends_no_hsts() {
    let app = router(state(unreachable_pool(), "http://localhost:8080"));
    let answer = send(app, Method::GET, "/healthz", &[]).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(
        answer
            .headers
            .get(header::STRICT_TRANSPORT_SECURITY)
            .is_none()
    );
}

/// Every route `router` mounts besides the document and its UI is an operation of the document,
/// so the method walk above reaches it.
#[test]
fn every_route_is_in_the_openapi_document() {
    let doc = document();
    let documented = doc["paths"].as_object().expect("paths");
    let missing: Vec<&str> = stepclimb_server::app::routes()
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| !documented.contains_key(*path))
        .collect();
    assert!(
        missing.is_empty(),
        "routes without an operation: {missing:?}"
    );
}

/// `router` mounts routes only from `app::routes()` and the two document routes, so the test
/// above sees every route: a `.route(` call written anywhere else in app.rs fails here.
#[test]
fn the_router_mounts_routes_only_from_the_route_list() {
    let source = include_str!("../src/app.rs");
    let calls: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.contains(".route(") || line.contains(".merge(") || line.contains(".nest(")
        })
        .collect();
    assert_eq!(
        calls,
        [
            "app = app.route(path, handler);",
            ".route(OPENAPI_JSON, get(openapi_json))",
            ".merge(Scalar::with_url(DOCS, openapi::document()))",
        ],
        "add the route to app::routes() instead"
    );
}
