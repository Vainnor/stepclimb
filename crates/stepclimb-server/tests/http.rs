// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP application end to end, through the router (no socket). Tests marked
//! `#[sqlx::test]` get a fresh database each and need `DATABASE_URL` (`just up`). The SPA tests
//! need the web build in `web/dist` (`just web-build`, which `just test-rust` runs first).

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use sqlx::{PgPool, postgres::PgPoolOptions};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::clock::ManualClock;
use stepclimb_server::{AppState, router, startup, web_origin::WebOrigin};
use tower::ServiceExt;

/// Send a same-origin request, as the web app does; tests/csrf.rs covers the others.
async fn send(
    app: Router,
    method: Method,
    path: &str,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .expect("a valid request"),
        )
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
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

async fn get(app: Router, path: &str) -> (StatusCode, Option<String>, String) {
    let response = app
        .oneshot(
            Request::get(path)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router never fails");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().expect("an ASCII content type").to_owned());
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    (
        status,
        content_type,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// Application state over `db`, with a stopped clock and a test-only key.
fn state(db: PgPool) -> AppState {
    AppState {
        db,
        clock: ManualClock::new(std::time::SystemTime::UNIX_EPOCH).clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("http://localhost:8080").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        passwords: Default::default(),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// A router whose pool points at a port nothing listens on, for routes that must not need the
/// database and for the readiness failure.
fn app_without_database() -> Router {
    let pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(500))
        .connect_lazy("postgres://stepclimb@127.0.0.1:1/none")
        .expect("a lazy pool parses its URL");
    router(state(pool))
}

#[tokio::test]
async fn healthz_answers_without_a_database() {
    let (status, content_type, body) = get(app_without_database(), "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(body, r#"{"status":"ok"}"#);
}

#[tokio::test]
async fn readyz_is_503_when_the_database_is_unreachable() {
    let (status, _, body) = get(app_without_database(), "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, r#"{"status":"unavailable"}"#);
}

#[sqlx::test(migrations = false)]
async fn readyz_is_200_when_the_database_answers(pool: PgPool) {
    let (status, _, body) = get(router(state(pool)), "/readyz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"ok"}"#);
}

#[sqlx::test(migrations = false)]
async fn startup_migrations_create_the_schema_on_an_empty_database(pool: PgPool) {
    startup::migrate(&pool)
        .await
        .expect("serve's migration step applies");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        [
            "accounts",
            "admission_webhooks",
            "audit_events",
            "auth_flows",
            "identities",
            "identity_providers",
            "local_logins",
            "local_tokens",
            "oauth_authorization_codes",
            "org_plans",
            "organizations",
            "passkeys",
            "plans",
            "role_assignments",
            "roles",
            "service_accounts",
            "sessions",
            "totp_recovery_codes",
            "users"
        ]
    );
}

#[tokio::test]
async fn the_openapi_document_is_3_1_and_lists_every_route() {
    let (status, content_type, body) = get(app_without_database(), "/api/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    let doc: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(doc["openapi"], "3.1.0");
    let mut paths: Vec<&str> = doc["paths"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        [
            "/.well-known/stepclimb",
            "/api/v1/accounts",
            "/api/v1/accounts/{account_id}",
            "/api/v1/accounts/{account_id}/move",
            "/api/v1/admin/admission",
            "/api/v1/admin/admission/webhook",
            "/api/v1/admin/admission/webhook/rotate-secret",
            "/api/v1/admin/branding",
            "/api/v1/admin/local-sign-in",
            "/api/v1/admin/permissions",
            "/api/v1/admin/providers",
            "/api/v1/admin/providers/{provider_id}",
            "/api/v1/admin/role-assignments",
            "/api/v1/admin/role-assignments/{assignment_id}",
            "/api/v1/admin/roles",
            "/api/v1/admin/roles/{role_id}",
            "/api/v1/admin/service-accounts",
            "/api/v1/admin/service-accounts/{service_account_id}",
            "/api/v1/admin/users/{user_id}/enrollment-links",
            "/api/v1/me",
            "/api/v1/me/appearance",
            "/api/v1/me/identities",
            "/api/v1/me/identities/{identity_id}",
            "/api/v1/me/passkeys",
            "/api/v1/me/passkeys/start",
            "/api/v1/me/passkeys/{passkey_id}",
            "/api/v1/me/password",
            "/api/v1/me/totp",
            "/api/v1/me/totp/confirm",
            "/api/v1/me/totp/disable",
            "/api/v1/organization",
            "/api/v1/organization/entitlements",
            "/api/v1/organization/settings",
            "/api/v1/users",
            "/api/v1/users/{user_id}",
            "/api/v1/users/{user_id}/identities",
            "/api/v1/users/{user_id}/identities/{identity_id}",
            "/auth/local/enrollment/passkey",
            "/auth/local/enrollment/password",
            "/auth/local/enrollment/start",
            "/auth/local/passkey/finish",
            "/auth/local/passkey/start",
            "/auth/local/password",
            "/auth/local/totp",
            "/auth/sign-out",
            "/auth/{provider}/callback",
            "/auth/{provider}/start",
            "/healthz",
            "/oauth/authorize",
            "/readyz",
            "/theme.css"
        ]
    );
}

#[tokio::test]
async fn docs_serves_the_scalar_ui() {
    let (status, content_type, body) = get(app_without_database(), "/docs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.unwrap().starts_with("text/html"));
    assert!(
        body.contains("api-reference"),
        "the Scalar page mounts its element"
    );
}

#[tokio::test]
async fn the_committed_openapi_document_matches_the_code() {
    let committed = include_str!("../../../openapi/stepclimb.json");
    assert_eq!(
        committed,
        stepclimb_server::openapi::document_json(),
        "openapi/stepclimb.json is stale: run `just openapi` and commit the result"
    );
}

#[tokio::test]
async fn the_root_serves_the_built_spa() {
    let (status, content_type, body) = get(app_without_database(), "/").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "is web/dist built? `just web-build`. Body: {body}"
    );
    assert_eq!(content_type.as_deref(), Some("text/html"));
    assert!(body.contains(r#"<div id="root">"#));
}

#[tokio::test]
async fn a_client_route_falls_back_to_index_html() {
    let (status, content_type, body) = get(app_without_database(), "/c/s1/m/2/i/7").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/html"));
    assert!(body.contains(r#"<div id="root">"#));
}

#[tokio::test]
async fn a_missing_file_is_404_not_the_spa() {
    let (status, _, _) = get(app_without_database(), "/assets/missing-0000.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unknown_api_path_is_a_404_problem_not_the_spa() {
    for method in [Method::GET, Method::POST, Method::DELETE] {
        for path in ["/api", "/api/v1/nothing-here", "/api/openapi.json/x"] {
            let (status, headers, body) = send(app_without_database(), method.clone(), path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
            assert_eq!(
                headers.get(header::CONTENT_TYPE).unwrap(),
                "application/problem+json",
                "{method} {path}"
            );
            let problem: serde_json::Value = serde_json::from_str(&body).expect("JSON");
            assert_eq!(problem["code"], "request.not_found", "{method} {path}");
            assert_eq!(problem["status"], 404);
        }
    }
}

#[tokio::test]
async fn a_known_path_with_another_method_is_a_405_problem() {
    for (method, path, allow) in [
        (Method::POST, "/healthz", "GET,HEAD"),
        (Method::DELETE, "/api/openapi.json", "GET,HEAD"),
    ] {
        let (status, headers, body) = send(app_without_database(), method.clone(), path).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        assert_eq!(
            headers.get(header::ALLOW).unwrap(),
            allow,
            "{method} {path}"
        );
        let problem: serde_json::Value = serde_json::from_str(&body).expect("JSON");
        assert_eq!(problem["code"], "request.method_not_allowed");
        assert_eq!(problem["status"], 405);
    }
}

/// Spec 08 B43: answers, errors and refusals under `/api/v1` are never stored by a cache.
#[tokio::test]
async fn every_api_v1_response_is_no_store_and_nothing_else_is_changed() {
    let api = [
        (Method::GET, "/api/v1/nothing-here", StatusCode::NOT_FOUND),
        (Method::POST, "/api/v1/me", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/api/v1/me", StatusCode::UNAUTHORIZED),
    ];
    for (method, path, expected) in api {
        let (status, headers, _) = send(app_without_database(), method.clone(), path).await;
        assert_eq!(status, expected, "{method} {path}");
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .map(|v| v.to_str().unwrap()),
            Some("no-store"),
            "{method} {path}"
        );
    }
    let cross_site = app_without_database()
        .oneshot(
            Request::post("/api/v1/admin/roles")
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router never fails");
    assert_eq!(cross_site.status(), StatusCode::FORBIDDEN);
    assert_eq!(cross_site.headers()[header::CACHE_CONTROL], "no-store");
    for path in ["/healthz", "/api/openapi.json", "/api/v1x"] {
        let (_, headers, _) = send(app_without_database(), Method::GET, path).await;
        assert_ne!(
            headers
                .get(header::CACHE_CONTROL)
                .map(|v| v.to_str().unwrap()),
            Some("no-store"),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_client_route_takes_any_method_and_stays_the_spa() {
    let (status, headers, body) = send(app_without_database(), Method::POST, "/c/s1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "text/html");
    assert!(body.contains(r#"<div id="root">"#));
}

#[tokio::test]
async fn the_openapi_document_has_the_shared_problem_responses() {
    let (_, _, body) = get(app_without_database(), "/api/openapi.json").await;
    let doc: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");

    let problem = &doc["components"]["schemas"]["Problem"];
    let mut required: Vec<&str> = problem["required"]
        .as_array()
        .expect("Problem lists its required members")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    required.sort_unstable();
    assert_eq!(required, ["code", "detail", "status", "title", "type"]);
    assert!(problem["properties"]["errors"].is_object());
    assert!(doc["components"]["schemas"]["FieldError"].is_object());

    for name in [
        "BadRequest",
        "RouteNotFound",
        "MethodNotAllowed",
        "PayloadTooLarge",
        "UnsupportedMediaType",
        "UnprocessableEntity",
        "InternalError",
    ] {
        let schema = &doc["components"]["responses"][name]["content"]["application/problem+json"]["schema"]
            ["$ref"];
        assert_eq!(schema, "#/components/schemas/Problem", "response {name}");
    }
    let bad_request = doc["components"]["responses"]["BadRequest"]["description"]
        .as_str()
        .unwrap();
    let max = stepclimb_server::pagination::MAX_LIMIT;
    assert!(
        bad_request.contains(&format!("1 to {max}")),
        "{bad_request}"
    );
}

/// Serve `app` on a free loopback port and run `stepclimb healthcheck`'s check against it.
async fn healthcheck_against(
    app: Router,
) -> Result<(), stepclimb_server::healthcheck::HealthcheckError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a free port");
    let addr = listener.local_addr().expect("local address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let result = tokio::task::spawn_blocking(move || stepclimb_server::healthcheck::check(addr))
        .await
        .expect("the check runs");
    server.abort();
    result
}

#[sqlx::test(migrations = false)]
async fn the_healthcheck_passes_when_readyz_answers_200(pool: PgPool) {
    healthcheck_against(router(state(pool)))
        .await
        .expect("the server is ready");
}

#[tokio::test]
async fn the_healthcheck_fails_when_the_database_is_unreachable() {
    let error = healthcheck_against(app_without_database())
        .await
        .expect_err("the server is not ready");
    assert!(
        matches!(
            error,
            stepclimb_server::healthcheck::HealthcheckError::NotReady { status: 503, .. }
        ),
        "{error:?}"
    );
}
