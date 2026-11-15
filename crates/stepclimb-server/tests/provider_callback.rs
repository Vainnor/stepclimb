// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /auth/{provider}/callback` over the router, against the mock IdP on 127.0.0.1
//! (docs/specs/m0/05-oidc.md B7 to B11, B16, T5 to T11; spec 08 B18).
//!
//! Each sign-in starts at the start route, follows the authorization URL at the mock, which
//! approves at once, and brings the mock's redirect back to the callback with the flow cookie.
//! The mock issues its tokens at the server's manual clock. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{
    encryption::KeyRing,
    oidc::FlowSecrets,
    oidc_config::{self, ProviderRow},
    outbound::{OutboundClient, UrlPolicy},
};
use stepclimb_core::{
    auth_flows::LIFETIME,
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
};
use stepclimb_mock_idp::{AuthorizeError, Endpoint, Fault, MockIdp, StatusCode as MockStatus};
use stepclimb_server::{
    AppState,
    client_ip::TrustedProxies,
    providers::Providers,
    rate_limit::{Limits, RateLimiter},
    request_org::TEST_ORG_HEADER,
    router,
    web_origin::WebOrigin,
};
use tower::ServiceExt;
use url::Url;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const KEY_RING: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const PUBLIC_URL: &str = "https://lms.example.org";
const CLEARED: &str = "__Host-sc_flow=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn keys() -> KeyRing {
    KeyRing::parse(KEY_RING).expect("a valid test key")
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    mock: MockIdp,
    org: Uuid,
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
}

impl Answer {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }

    fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect()
    }

    /// `name=value` of the flow cookie this answer sets.
    fn flow_cookie(&self) -> String {
        self.set_cookies()
            .into_iter()
            .find(|value| value.starts_with("__Host-sc_flow="))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
            .expect("a flow cookie")
    }

    /// A 303 to the sign-in page with `code`, no session cookie, `no-store`.
    fn assert_lands_on_sign_in(&self, code: &str) {
        assert_eq!(self.status, StatusCode::SEE_OTHER, "{self:?}");
        assert_eq!(
            self.location(),
            format!("/sign-in?error={code}"),
            "{self:?}"
        );
        assert_eq!(self.headers[header::CACHE_CONTROL], "no-store");
        assert!(
            !self
                .set_cookies()
                .iter()
                .any(|cookie| cookie.starts_with("__Host-sc_session")),
            "no session cookie: {self:?}"
        );
    }

    /// [`Answer::assert_lands_on_sign_in`], with the flow cookie cleared.
    fn assert_refused(&self, code: &str) {
        self.assert_lands_on_sign_in(code);
        assert_eq!(self.set_cookies(), [CLEARED], "{self:?}");
    }
}

/// A started sign-in: the flow cookie and the query the mock sends the browser back with.
struct Started {
    cookie: String,
    back: String,
}

fn state(pool: &PgPool, clock: &ManualClock, limits: Limits) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: keys(),
        public_url: WebOrigin::parse(PUBLIC_URL).expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(limits, TrustedProxies::none()),
        providers: Providers::new(true).expect("provider clients"),
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        Self::with(pool, Limits::DEFAULT).await
    }

    async fn with(pool: PgPool, limits: Limits) -> Self {
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app = router(state(&pool, &clock, limits)).layer(Extension(ConnectInfo(CLIENT)));
        let mock = MockIdp::start().await.expect("the mock starts");
        let mock_clock = clock.clock();
        mock.set_time_source(move || mock_clock.now());
        Self {
            pool,
            clock,
            app,
            mock,
            org,
        }
    }

    /// An enabled provider of `kind` in `org` with `key`, pointing at the mock.
    async fn provider(&self, org: Uuid, key: &str, kind: &str) -> Uuid {
        let id = new_id();
        let config = json!({
            "issuer": self.mock.issuer(),
            "client_id": self.mock.client_id(),
            "client_secret": self.mock.client_secret(),
        });
        let stored = oidc_config::validate(&config, UrlPolicy::new(true, true), None)
            .expect("a valid config")
            .seal(
                &keys(),
                ProviderRow {
                    org_id: org,
                    provider_id: id,
                },
            )
            .expect("seal")
            .to_json();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled) \
             VALUES ($1, $2, $3, $4, $5, true)",
        )
        .bind(id)
        .bind(org)
        .bind(key)
        .bind(kind)
        .bind(Value::Object(stored))
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    async fn get(&self, org: Uuid, path: &str, cookie: Option<&str>) -> Answer {
        let mut request = Request::get(path).header(TEST_ORG_HEADER, org.to_string());
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let request = request.body(Body::empty()).expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        Answer {
            status: response.status(),
            headers: response.headers().clone(),
        }
    }

    /// Start at `key` and follow the authorization URL at the mock.
    async fn start(&self, key: &str) -> Started {
        let answer = self
            .get(self.org, &format!("/auth/{key}/start"), None)
            .await;
        assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
        let request = axum::http::Request::get(answer.location())
            .body(Vec::new())
            .expect("a request");
        let client = OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client");
        let response = client.send(request).await.expect("the mock answers");
        assert_eq!(response.status(), StatusCode::FOUND);
        let back = Url::parse(
            response.headers()[header::LOCATION]
                .to_str()
                .expect("ASCII"),
        )
        .expect("an absolute redirect");
        assert_eq!(back.path(), format!("/auth/{key}/callback"));
        Started {
            cookie: answer.flow_cookie(),
            back: back.query().unwrap_or("").to_owned(),
        }
    }

    async fn callback(&self, key: &str, query: &str, cookie: Option<&str>) -> Answer {
        self.get(self.org, &format!("/auth/{key}/callback?{query}"), cookie)
            .await
    }

    /// Start at `mock` and bring the mock's answer back with the flow cookie.
    async fn sign_in(&self) -> Answer {
        let started = self.start("mock").await;
        self.callback("mock", &started.back, Some(&started.cookie))
            .await
    }

    async fn count(&self, table: &str) -> i64 {
        let query = match table {
            "auth_flows" => "SELECT count(*) FROM auth_flows",
            "sessions" => "SELECT count(*) FROM sessions",
            other => panic!("no count for {other}"),
        };
        sqlx::query_scalar(query)
            .fetch_one(&self.pool)
            .await
            .expect("count rows")
    }

    fn token_requests(&self) -> usize {
        self.mock.requests_to(Endpoint::Token).len()
    }
}

/// `query` with its `state` replaced by `state`.
fn with_state(query: &str, state: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(url::form_urlencoded::parse(query.as_bytes()).map(|(k, v)| {
            if k == "state" {
                (k, state.into())
            } else {
                (k, v)
            }
        }))
        .finish()
}

fn param(query: &str, name: &str) -> String {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .expect("the parameter")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_verified_sign_in_reaches_completion_which_creates_no_session_yet(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let started = fixture.start("mock").await;
    let flows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as("SELECT id, payload FROM auth_flows")
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
    let context: [&[u8]; 4] = [
        fixture.org.as_bytes(),
        b"auth_flows",
        flows[0].0.as_bytes(),
        b"payload",
    ];
    let secrets =
        FlowSecrets::from_payload(&keys().decrypt(&flows[0].1, &context).unwrap()).unwrap();

    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;

    answer.assert_refused("auth.provider_unavailable");
    let token = &fixture.mock.requests_to(Endpoint::Token)[0];
    assert_eq!(
        token.form_param("code_verifier").as_deref(),
        Some(secrets.code_verifier()),
        "T4: the stored verifier is sent"
    );
    assert_eq!(
        token.form_param("redirect_uri").as_deref(),
        Some("https://lms.example.org/auth/mock/callback")
    );
    assert_eq!(fixture.mock.requests_to(Endpoint::Userinfo).len(), 1);
    assert_eq!(fixture.count("auth_flows").await, 0, "the flow is used");
    assert_eq!(fixture.count("sessions").await, 0, "no session yet");

    // T6: the same callback again finds no flow.
    let replay = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;
    replay.assert_refused("auth.flow_invalid");
    assert_eq!(
        fixture.token_requests(),
        1,
        "the replay reached no provider"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_no_cookie_another_flows_cookie_or_another_state_is_flow_invalid(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;

    let started = fixture.start("mock").await;
    let no_cookie = fixture.callback("mock", &started.back, None).await;
    no_cookie.assert_lands_on_sign_in("auth.flow_invalid");
    assert_eq!(fixture.count("auth_flows").await, 1, "no flow was named");

    // Two flows in this browser's reach: the second's cookie with the first's state.
    let other = fixture.start("mock").await;
    let crossed = fixture
        .callback("mock", &started.back, Some(&other.cookie))
        .await;
    crossed.assert_refused("auth.flow_invalid");

    let state = param(&started.back, "state");
    let mut last = state.clone().into_bytes();
    let end = last.len() - 1;
    last[end] = if last[end] == b'A' { b'B' } else { b'A' };
    let one_off = with_state(&started.back, &String::from_utf8(last).unwrap());
    let answer = fixture
        .callback("mock", &one_off, Some(&started.cookie))
        .await;
    answer.assert_refused("auth.flow_invalid");

    let third = fixture.start("mock").await;
    let code = param(&third.back, "code");
    let answer = fixture
        .callback("mock", &format!("code={code}"), Some(&third.cookie))
        .await;
    answer.assert_refused("auth.flow_invalid");

    assert_eq!(fixture.token_requests(), 0, "nothing reached the provider");
    assert_eq!(fixture.count("sessions").await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b7_a_flow_started_for_another_provider_is_flow_invalid(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    fixture.provider(fixture.org, "other", "oidc").await;
    let started = fixture.start("other").await;

    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;

    answer.assert_refused("auth.flow_invalid");
    assert_eq!(fixture.token_requests(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_callback_after_the_flow_lifetime_is_flow_expired(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let started = fixture.start("mock").await;
    fixture.clock.advance(LIFETIME);

    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;

    answer.assert_refused("auth.flow_expired");
    assert_eq!(
        fixture.count("auth_flows").await,
        0,
        "the expired flow is gone"
    );
    assert_eq!(fixture.token_requests(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_another_organizations_or_an_unknown_key_is_not_found_and_leaves_the_flow(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let other = organizations::create(&fixture.pool, "org-b", "Org B", now())
        .await
        .unwrap()
        .id;
    fixture.provider(other, "theirs", "oidc").await;
    let started = fixture.start("mock").await;

    for key in ["theirs", "nobody", "local"] {
        let answer = fixture
            .callback(key, &started.back, Some(&started.cookie))
            .await;
        answer.assert_lands_on_sign_in("auth.provider_not_found");
        assert!(answer.set_cookies().is_empty(), "{answer:?}");
    }
    assert_eq!(
        fixture.count("auth_flows").await,
        1,
        "the flow is untouched"
    );
}

/// Collects every log line the server writes while it is the thread's subscriber.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("the log buffer")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("the log buffer").clone()).expect("UTF-8")
    }
}

fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_provider_errors_are_answered_without_their_description(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let description = format!("zq{}", "d".repeat(300));
    let (log, guard) = capture();

    fixture.mock.set_authorize_error(Some(AuthorizeError {
        error: "access_denied".into(),
        description: Some(description.clone()),
    }));
    let denied = fixture.sign_in().await;
    fixture.mock.set_authorize_error(Some(AuthorizeError {
        error: "server_error".into(),
        description: Some(description.clone()),
    }));
    let failed = fixture.sign_in().await;
    drop(guard);

    denied.assert_refused("auth.provider_denied");
    failed.assert_refused("auth.provider_error");
    assert!(!denied.location().contains("zq") && !failed.location().contains("zq"));
    let log = log.text();
    let kept = format!("zq{}", "d".repeat(198));
    assert!(log.contains(&kept), "the description is logged: {log}");
    assert!(!log.contains(&format!("{kept}d")), "cut to 200 characters");
    assert_eq!(fixture.token_requests(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_a_failed_token_request_is_provider_error(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;

    fixture.mock.set_fault(
        Endpoint::Token,
        Fault::status(MockStatus::INTERNAL_SERVER_ERROR),
    );
    fixture
        .sign_in()
        .await
        .assert_refused("auth.provider_error");
    fixture.mock.clear_fault(Endpoint::Token);

    fixture.mock.set_token_response_field("id_token", None);
    fixture
        .sign_in()
        .await
        .assert_refused("auth.provider_error");
    assert_eq!(fixture.count("sessions").await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_refused_id_token_is_id_token_invalid_and_a_bad_sub_subject_missing(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;

    fixture
        .mock
        .set_id_token_claim("nonce", Some(json!("another")));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.id_token_invalid");
    fixture.mock.clear_overrides();

    fixture
        .mock
        .set_id_token_signing(stepclimb_mock_idp::IdTokenSigning::AlgNone)
        .unwrap();
    fixture
        .sign_in()
        .await
        .assert_refused("auth.id_token_invalid");
    fixture
        .mock
        .set_id_token_signing(stepclimb_mock_idp::IdTokenSigning::Current)
        .unwrap();

    // The token expired by more than the skew at the server's clock.
    let issued_at = fixture.clock.clock().now();
    let exp = issued_at.duration_since(UNIX_EPOCH).unwrap().as_secs() - 61;
    fixture.mock.set_id_token_claim("exp", Some(json!(exp)));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.id_token_invalid");
    fixture.mock.clear_overrides();

    fixture.mock.set_id_token_claim("sub", Some(json!("")));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.subject_missing");
    assert_eq!(fixture.count("sessions").await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_a_rotated_key_is_found_after_the_refetch_interval(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    fixture
        .sign_in()
        .await
        .assert_refused("auth.provider_unavailable");

    fixture.mock.rotate_signing_key().unwrap();
    fixture
        .sign_in()
        .await
        .assert_refused("auth.id_token_invalid");
    fixture
        .clock
        .advance(stepclimb_auth::oidc::JWKS_REFETCH_INTERVAL);
    fixture
        .sign_in()
        .await
        .assert_refused("auth.provider_unavailable");
    assert_eq!(fixture.mock.requests_to(Endpoint::Jwks).len(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_userinfo_sub_of_someone_else_is_provider_error(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    fixture
        .mock
        .set_userinfo_claim("sub", Some(json!("someone-else")));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.provider_error");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b16_the_callback_is_rate_limited_before_anything_runs(pool: PgPool) {
    let limits = Limits {
        sign_in: NonZeroU32::new(2).unwrap(),
        ..Limits::DEFAULT
    };
    let fixture = Fixture::with(pool, limits).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    // The start and the first callback spend the budget.
    let started = fixture.start("mock").await;
    fixture.callback("mock", "state=x", None).await;

    let limited = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;

    limited.assert_lands_on_sign_in("auth.rate_limited");
    assert_eq!(limited.headers[header::RETRY_AFTER], "60");
    assert_eq!(
        fixture.count("auth_flows").await,
        1,
        "the flow is untouched"
    );
    assert_eq!(fixture.token_requests(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn no_log_line_carries_a_token_the_code_or_a_secret(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let started = fixture.start("mock").await;
    let (log, guard) = capture();
    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;
    fixture
        .mock
        .set_userinfo_claim("sub", Some(json!("someone-else")));
    let second = fixture.start("mock").await;
    let refused = fixture
        .callback("mock", &second.back, Some(&second.cookie))
        .await;
    drop(guard);

    answer.assert_refused("auth.provider_unavailable");
    refused.assert_refused("auth.provider_error");
    let log = log.text();
    assert!(
        log.contains("does not complete provider sign-in yet"),
        "{log}"
    );
    assert!(log.contains("userinfo sub"), "{log}");
    let mut secrets = vec![fixture.mock.client_secret()];
    for tokens in fixture.mock.issued_tokens() {
        secrets.push(tokens.access_token);
        secrets.push(tokens.refresh_token);
        secrets.push(tokens.id_token.expect("an ID token"));
    }
    for run in [&started, &second] {
        secrets.push(param(&run.back, "code"));
        secrets.push(param(&run.back, "state"));
        secrets.push(run.cookie.trim_start_matches("__Host-sc_flow=").to_owned());
    }
    for secret in secrets {
        assert!(!log.contains(&secret), "a secret reached the log");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_server_fault_after_the_flow_is_taken_still_clears_its_cookie(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture.provider(fixture.org, "mock", "oidc").await;

    // The flow's payload does not open: the flow is taken, then its payload fails.
    let started = fixture.start("mock").await;
    sqlx::query("UPDATE auth_flows SET payload = '\\x00010203'::bytea")
        .execute(&fixture.pool)
        .await
        .expect("corrupt the payload");
    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;
    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{answer:?}"
    );
    assert_eq!(answer.set_cookies(), [CLEARED], "{answer:?}");
    assert_eq!(fixture.count("auth_flows").await, 0, "the flow was taken");

    // The client secret does not open: the failure comes from deeper in the callback.
    let started = fixture.start("mock").await;
    sqlx::query(
        "UPDATE identity_providers \
         SET config = jsonb_set(config, '{client_secret,sealed}', '\"AAECAwQFBgcICQoL\"') \
         WHERE id = $1",
    )
    .bind(provider)
    .execute(&fixture.pool)
    .await
    .expect("corrupt the secret");
    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;
    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{answer:?}"
    );
    assert_eq!(answer.set_cookies(), [CLEARED], "{answer:?}");
    assert_eq!(fixture.token_requests(), 0, "nothing reached the provider");
}
