// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rate limits through axum (docs/specs/m0/08-sessions-and-csrf.md B22, B23, Q7; T20). No
//! sign-in or token route exists yet, so a router built here mounts test-only routes that declare
//! the limits the way those routes will: `RateLimited<SignIn>` or `RateLimited<Token>` first, and
//! `account_check` around a password check. No database is reached.

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{OrgScope, clock::ManualClock, id::Uuid, local_logins::LoginEmail};
use stepclimb_server::{
    AppState, Config,
    client_ip::TrustedProxies,
    error::Problem,
    rate_limit::{Limits, RateLimited, RateLimiter, SignIn, Token},
    startup,
    web_origin::WebOrigin,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

const CADDY: &str = "10.201.73.2";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test stands in for the request-organization resolver"
)]
fn scope() -> OrgScope {
    OrgScope::instance(Uuid::from_u128(7))
}

fn limits(sign_in: u32, token: u32, failed: u32) -> Limits {
    Limits {
        sign_in: NonZeroU32::new(sign_in).expect("a nonzero limit"),
        token: NonZeroU32::new(token).expect("a nonzero limit"),
        failed_checks: NonZeroU32::new(failed).expect("a nonzero limit"),
    }
}

/// The handlers' state: the app's, plus how many handler bodies ran.
#[derive(Clone)]
struct Fixture {
    app: AppState,
    ran: Arc<AtomicUsize>,
}

impl axum::extract::FromRef<Fixture> for AppState {
    fn from_ref(fixture: &Fixture) -> Self {
        fixture.app.clone()
    }
}

async fn sign_in(State(fixture): State<Fixture>, limited: RateLimited<SignIn>) -> String {
    fixture.ran.fetch_add(1, Ordering::SeqCst);
    limited.client.to_string()
}

async fn token(State(fixture): State<Fixture>, _: RateLimited<Token>) -> StatusCode {
    fixture.ran.fetch_add(1, Ordering::SeqCst);
    StatusCode::OK
}

/// A password check for the account in `x-account`: the password `right` passes.
async fn password(
    State(fixture): State<Fixture>,
    _: RateLimited<SignIn>,
    headers: HeaderMap,
) -> Response {
    let account = headers["x-account"].to_str().expect("an ASCII account");
    let now = fixture.app.clock.now();
    let check =
        match fixture
            .app
            .rate_limits
            .account_check(scope(), &LoginEmail::for_test(account), now)
        {
            Ok(check) => check,
            Err(refused) => return refused.into_response(),
        };
    fixture.ran.fetch_add(1, Ordering::SeqCst);
    if headers["x-password"] == "right" {
        check.passed();
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

struct Setup {
    router: Router,
    clock: ManualClock,
    ran: Arc<AtomicUsize>,
}

fn setup(limits: Limits, trusted: &str) -> Setup {
    let clock = ManualClock::new(now());
    let trusted = if trusted.is_empty() {
        TrustedProxies::none()
    } else {
        TrustedProxies::parse(trusted).expect("a valid proxy list")
    };
    let app = AppState {
        db: PgPoolOptions::new()
            .connect_lazy("postgres://stepclimb@127.0.0.1:1/none")
            .expect("a lazy pool parses its URL"),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(limits, trusted),
        passwords: Default::default(),
        storage: None,
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    };
    let ran = Arc::new(AtomicUsize::new(0));
    let router = Router::new()
        .route("/test/sign-in", post(sign_in))
        .route("/test/token", post(token))
        .route("/test/password", post(password))
        .with_state(Fixture {
            app,
            ran: ran.clone(),
        });
    Setup { router, clock, ran }
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

    fn retry_after(&self) -> &str {
        self.headers[header::RETRY_AFTER]
            .to_str()
            .expect("an ASCII header")
    }
}

async fn send(router: &Router, path: &str, peer: &str, extra: &[(&str, &str)]) -> Answer {
    let mut request = Request::post(path);
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let mut request = request.body(Body::empty()).expect("a valid request");
    let peer: SocketAddr = format!("{peer}:40000").parse().expect("a socket address");
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body reads")
        .to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec()).expect("a UTF-8 body"),
    }
}

// T20: the request after the limit answers 429 with Retry-After, and the handler does not run.
#[tokio::test]
async fn the_request_after_the_limit_answers_429_with_retry_after() {
    let setup = setup(limits(3, 60, 10), "");
    for _ in 0..3 {
        let answer = send(&setup.router, "/test/sign-in", "203.0.113.9", &[]).await;
        assert_eq!(answer.status, StatusCode::OK);
    }
    setup.clock.advance(Duration::from_secs(20));
    let refused = send(&setup.router, "/test/sign-in", "203.0.113.9", &[]).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.code(), "auth.rate_limited");
    assert_eq!(refused.retry_after(), "40");
    assert_eq!(
        refused.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(setup.ran.load(Ordering::SeqCst), 3);
    // Another address has its own budget.
    let other = send(&setup.router, "/test/sign-in", "203.0.113.10", &[]).await;
    assert_eq!(other.status, StatusCode::OK);
}

// T20: the counter resets after the window.
#[tokio::test]
async fn the_counter_resets_after_the_window() {
    let setup = setup(limits(1, 60, 10), "");
    assert_eq!(
        send(&setup.router, "/test/sign-in", "203.0.113.9", &[])
            .await
            .status,
        StatusCode::OK
    );
    setup.clock.advance(Duration::from_secs(59));
    assert_eq!(
        send(&setup.router, "/test/sign-in", "203.0.113.9", &[])
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    setup.clock.advance(Duration::from_secs(1));
    assert_eq!(
        send(&setup.router, "/test/sign-in", "203.0.113.9", &[])
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn token_routes_have_their_own_budget() {
    let setup = setup(limits(1, 2, 10), "");
    assert_eq!(
        send(&setup.router, "/test/sign-in", "203.0.113.9", &[])
            .await
            .status,
        StatusCode::OK
    );
    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        let answer = send(&setup.router, "/test/token", "203.0.113.9", &[]).await;
        assert_eq!(answer.status, expected);
    }
}

// T20 (B23): failures for one account from two addresses share the account's budget.
#[tokio::test]
async fn failures_for_one_account_from_two_addresses_share_its_budget() {
    let setup = setup(limits(20, 60, 3), "");
    let wrong = [("x-account", "ada"), ("x-password", "wrong")];
    for peer in ["203.0.113.9", "198.51.100.7", "203.0.113.9"] {
        let answer = send(&setup.router, "/test/password", peer, &wrong).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{peer}");
    }
    // A third address, and the right password: no check runs, so the right password is refused
    // too and the guess learns nothing.
    let right = [("x-account", "ada"), ("x-password", "right")];
    let refused = send(&setup.router, "/test/password", "192.0.2.1", &right).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.code(), "auth.rate_limited");
    assert_eq!(refused.retry_after(), "900");
    assert_eq!(setup.ran.load(Ordering::SeqCst), 3);
    // Another account is not affected, and the account works again after 15 minutes.
    let grace = [("x-account", "grace"), ("x-password", "right")];
    let other = send(&setup.router, "/test/password", "203.0.113.9", &grace).await;
    assert_eq!(other.status, StatusCode::NO_CONTENT);
    setup.clock.advance(Duration::from_secs(15 * 60));
    let again = send(&setup.router, "/test/password", "192.0.2.1", &right).await;
    assert_eq!(again.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn successful_checks_do_not_use_the_account_budget() {
    let setup = setup(limits(20, 60, 2), "");
    let right = [("x-account", "ada"), ("x-password", "right")];
    for _ in 0..5 {
        let answer = send(&setup.router, "/test/password", "203.0.113.9", &right).await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT);
    }
}

// Behind the trusted proxy each forwarded client has its own budget.
#[tokio::test]
async fn behind_a_trusted_proxy_each_forwarded_client_is_counted_apart() {
    let setup = setup(limits(1, 60, 10), CADDY);
    for client in ["203.0.113.9", "198.51.100.7"] {
        let answer = send(
            &setup.router,
            "/test/sign-in",
            CADDY,
            &[("x-forwarded-for", client)],
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.body, client);
    }
    let again = send(
        &setup.router,
        "/test/sign-in",
        CADDY,
        &[("x-forwarded-for", "203.0.113.9")],
    )
    .await;
    assert_eq!(again.status, StatusCode::TOO_MANY_REQUESTS);
}

// A client that is not the proxy cannot buy a new budget by sending X-Forwarded-For.
#[tokio::test]
async fn a_direct_client_cannot_choose_its_address() {
    let setup = setup(limits(1, 60, 10), CADDY);
    let first = send(
        &setup.router,
        "/test/sign-in",
        "203.0.113.9",
        &[("x-forwarded-for", "1.1.1.1")],
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.body, "203.0.113.9");
    let second = send(
        &setup.router,
        "/test/sign-in",
        "203.0.113.9",
        &[("x-forwarded-for", "2.2.2.2")],
    )
    .await;
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
}

// Through the proxy, a client-sent entry left of the proxy's own is never the key.
#[tokio::test]
async fn a_spoofed_entry_behind_the_proxy_is_ignored() {
    let setup = setup(limits(1, 60, 10), CADDY);
    for spoofed in ["1.1.1.1", "2.2.2.2"] {
        let forwarded = format!("{spoofed}, 203.0.113.9");
        let answer = send(
            &setup.router,
            "/test/sign-in",
            CADDY,
            &[("x-forwarded-for", &forwarded)],
        )
        .await;
        let expected = if spoofed == "1.1.1.1" {
            StatusCode::OK
        } else {
            StatusCode::TOO_MANY_REQUESTS
        };
        assert_eq!(answer.status, expected, "{forwarded}");
    }
}

// A router served without the peer address cannot count, so it refuses rather than letting the
// request through.
#[tokio::test]
async fn without_the_peer_address_the_request_fails_closed() {
    let setup = setup(limits(1, 60, 10), "");
    let request = Request::post("/test/sign-in").body(Body::empty()).unwrap();
    let response = setup.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(setup.ran.load(Ordering::SeqCst), 0);
}

/// One HTTP/1.1 POST over TCP with `X-Forwarded-For: forwarded`; the status code and the body.
async fn post_over_tcp(addr: SocketAddr, path: &str, forwarded: &str) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("the server accepts");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: test\r\nX-Forwarded-For: {forwarded}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("the request is written");
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .await
        .expect("the response is read");
    let status = raw[9..12].parse().expect("a status code");
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
    (status, body)
}

async fn sign_in_route(limited: RateLimited<SignIn>) -> String {
    limited.client.to_string()
}

// The server's serving path builds the limiter from the configuration (a limit of 2, and the
// test's own loopback address as the trusted proxy) and hands each request its TCP peer.
#[tokio::test]
async fn the_server_limits_with_its_configuration_over_tcp() {
    let env = [
        (
            "STEPCLIMB_DATABASE_URL",
            "postgres://stepclimb@127.0.0.1:1/none",
        ),
        (
            "STEPCLIMB_ENCRYPTION_KEYS",
            "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        ),
        ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
        ("STEPCLIMB_SIGN_IN_RATE_LIMIT", "2"),
        ("STEPCLIMB_TRUSTED_PROXIES", "127.0.0.1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let config = Config::load(&env, None).expect("a valid configuration");
    let pool = PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .expect("a lazy pool parses its URL");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    let addr = listener.local_addr().expect("a bound address");
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = |state: AppState| {
        Router::new()
            .route("/test/sign-in", post(sign_in_route))
            .with_state(state)
    };
    let server = tokio::spawn(startup::serve_on(
        listener,
        config,
        startup::Opened::default(),
        pool,
        ManualClock::new(now()).clock(),
        app,
        async {
            let _ = stopped.await;
        },
    ));
    for expected in [200, 200, 429] {
        let (status, body) = post_over_tcp(addr, "/test/sign-in", "203.0.113.9").await;
        assert_eq!(status, expected, "{body}");
        if status == 200 {
            assert_eq!(body, "203.0.113.9");
        }
    }
    let (status, body) = post_over_tcp(addr, "/test/sign-in", "198.51.100.7").await;
    assert_eq!((status, body.as_str()), (200, "198.51.100.7"));
    stop.send(()).expect("the server is running");
    server
        .await
        .expect("the server task")
        .expect("the server stops cleanly");
}

// The 429 the coming routes reference is documented with its header.
#[test]
fn the_document_describes_the_429_and_its_retry_after() {
    let document: serde_json::Value =
        serde_json::from_str(&stepclimb_server::openapi::document_json()).unwrap();
    let response = &document["components"]["responses"]["RateLimited"];
    assert!(
        response["description"]
            .as_str()
            .unwrap()
            .starts_with("`auth.rate_limited`"),
        "{response}"
    );
    assert_eq!(
        response["headers"]["Retry-After"]["schema"]["type"],
        "integer"
    );
    assert_eq!(
        response["content"]["application/problem+json"]["schema"]["$ref"],
        "#/components/schemas/Problem"
    );
}
