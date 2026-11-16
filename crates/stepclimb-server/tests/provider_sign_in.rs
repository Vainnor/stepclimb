// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /auth/{provider}/start` over the router, against the mock IdP on 127.0.0.1
//! (docs/specs/m0/05-oidc.md B3 to B6, B16, T2 to T4; spec 08 B17 to B20).
//!
//! The requests name their organization with the `test-support` header of `request_org`, so a
//! test can hold two. Needs `DATABASE_URL` (`just up`).

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
    oidc::{DISCOVERY_TTL, FlowSecrets},
    oidc_config::{self, ProviderRow},
    outbound::{OutboundClient, UrlPolicy},
};
use stepclimb_core::{
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
};
use stepclimb_mock_idp::{Endpoint, Fault, MockIdp, StatusCode as MockStatus};
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
const REDIRECT_URI: &str = "https://lms.example.org/auth/mock/callback";

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

    /// `name=value` of the flow cookie this answer sets.
    fn flow_cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("__Host-sc_flow="))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
    }

    fn param(&self, name: &str) -> Option<String> {
        Url::parse(self.location())
            .ok()?
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }

    fn assert_refused(&self, code: &str) {
        assert_eq!(self.status, StatusCode::SEE_OTHER, "{self:?}");
        assert_eq!(
            self.location(),
            format!("/sign-in?error={code}"),
            "{self:?}"
        );
        assert_eq!(self.flow_cookie(), None, "a refusal sets no flow cookie");
    }
}

fn state(pool: &PgPool, clock: &ManualClock, limits: Limits, insecure: bool) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: keys(),
        public_url: WebOrigin::parse(PUBLIC_URL).expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(limits, TrustedProxies::none()),
        passwords: Default::default(),
        storage: None,
        providers: Providers::new(insecure).expect("provider clients"),
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        Self::with(pool, Limits::DEFAULT, true).await
    }

    async fn with(pool: PgPool, limits: Limits, insecure: bool) -> Self {
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app =
            router(state(&pool, &clock, limits, insecure)).layer(Extension(ConnectInfo(CLIENT)));
        let mock = MockIdp::start().await.expect("the mock starts");
        Self {
            pool,
            clock,
            app,
            mock,
            org,
        }
    }

    async fn second_org(&self) -> Uuid {
        organizations::create(&self.pool, "org-b", "Org B", now())
            .await
            .expect("create organization")
            .id
    }

    /// An `oidc` provider of `org` with `key`, pointing at the mock under `issuer`.
    async fn provider(&self, org: Uuid, key: &str, issuer: &str, enabled: bool) -> Uuid {
        self.provider_of_kind(org, key, issuer, enabled, "oidc")
            .await
    }

    /// A provider of `kind` holding a valid `oidc` config that points at the mock.
    async fn provider_of_kind(
        &self,
        org: Uuid,
        key: &str,
        issuer: &str,
        enabled: bool,
        kind: &str,
    ) -> Uuid {
        let id = new_id();
        let config = json!({
            "issuer": issuer,
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
        insert_provider(
            &self.pool,
            org,
            id,
            key,
            kind,
            enabled,
            Value::Object(stored),
        )
        .await;
        id
    }

    async fn start(&self, org: Uuid, path: &str) -> Answer {
        let request = Request::get(path)
            .header(TEST_ORG_HEADER, org.to_string())
            // The redirect URI comes from the public URL, whatever the request's host.
            .header(header::HOST, "evil.example")
            .body(Body::empty())
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        Answer {
            status: response.status(),
            headers: response.headers().clone(),
        }
    }

    async fn flows(&self) -> Vec<(Uuid, String, Option<Uuid>, Vec<u8>, String)> {
        sqlx::query_as("SELECT id, kind, provider_id, payload, return_to FROM auth_flows")
            .fetch_all(&self.pool)
            .await
            .expect("read flows")
    }

    fn discoveries(&self) -> usize {
        self.mock.requests_to(Endpoint::Discovery).len()
    }
}

async fn insert_provider(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    key: &str,
    kind: &str,
    enabled: bool,
    config: Value,
) {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .bind(kind)
    .bind(config)
    .bind(enabled)
    .execute(pool)
    .await
    .expect("insert provider");
}

/// The flow's secrets, opened as the callback will open them.
fn open(org: Uuid, flow: Uuid, sealed: &[u8]) -> FlowSecrets {
    let context: [&[u8]; 4] = [org.as_bytes(), b"auth_flows", flow.as_bytes(), b"payload"];
    let payload = keys().decrypt(sealed, &context).expect("the payload opens");
    FlowSecrets::from_payload(&payload).expect("an OIDC payload")
}

fn mock_client() -> OutboundClient {
    OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client")
}

/// Follow the authorization URL at the mock, which approves at once: the code it sends back.
async fn authorize_at_mock(location: &str) -> String {
    let request = axum::http::Request::get(location)
        .body(Vec::new())
        .expect("a request");
    let response = mock_client().send(request).await.expect("the mock answers");
    assert_eq!(response.status(), StatusCode::FOUND, "the mock approves");
    let back = response.headers()[header::LOCATION]
        .to_str()
        .expect("an ASCII Location");
    assert!(back.starts_with(REDIRECT_URI), "{back}");
    Url::parse(back)
        .expect("an absolute redirect")
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, code)| code.into_owned())
        .expect("a code")
}

/// Redeem `code` at the mock's token endpoint with `verifier`: the status.
async fn redeem(mock: &MockIdp, code: &str, verifier: &str) -> StatusCode {
    let secret = mock.client_secret();
    let client_id = mock.client_id();
    let body: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
            ("client_id", client_id.as_str()),
            ("client_secret", secret.as_str()),
        ])
        .finish();
    let request = axum::http::Request::post(mock.url(Endpoint::Token))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.into_bytes())
        .expect("a request");
    mock_client()
        .send(request)
        .await
        .expect("the mock answers")
        .status()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_start_sends_the_browser_to_the_provider_with_s256_state_and_nonce(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    let answer = fixture
        .start(fixture.org, "/auth/mock/start?return_to=/c/s1/m/2")
        .await;

    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    let location = answer.location();
    assert!(
        location.starts_with(&format!("{}?", fixture.mock.url(Endpoint::Authorize))),
        "{location}"
    );
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(answer.param("response_type").as_deref(), Some("code"));
    assert_eq!(answer.param("client_id"), Some(fixture.mock.client_id()));
    assert_eq!(
        answer.param("redirect_uri").as_deref(),
        Some(REDIRECT_URI),
        "B6: from the public URL, not the Host header"
    );
    assert_eq!(
        answer.param("scope").as_deref(),
        Some("openid email profile")
    );
    assert_eq!(
        answer.param("code_challenge_method").as_deref(),
        Some("S256")
    );
    let cookie = answer.flow_cookie().expect("the flow cookie");
    assert!(cookie.len() > "__Host-sc_flow=".len());

    let flows = fixture.flows().await;
    let [(flow_id, kind, provider_id, sealed, return_to)] = flows.as_slice() else {
        panic!("one flow, got {}", flows.len());
    };
    assert_eq!(kind, "oidc");
    assert_eq!(*provider_id, Some(provider));
    assert_eq!(return_to, "/c/s1/m/2");
    let secrets = open(fixture.org, *flow_id, sealed);
    assert_eq!(answer.param("state").as_deref(), Some(secrets.state()));
    assert_eq!(answer.param("nonce").as_deref(), Some(secrets.nonce()));
    assert_eq!(
        answer.param("code_challenge"),
        Some(secrets.code_challenge()),
        "the challenge is S256 of the stored verifier"
    );
    assert!(!location.contains(secrets.code_verifier()));

    // The mock refuses the stored verifier's neighbor and accepts the stored verifier, so the
    // challenge sent is the stored verifier's.
    let code = authorize_at_mock(location).await;
    let wrong = format!("{}x", secrets.code_verifier());
    assert_eq!(
        redeem(&fixture.mock, &code, &wrong).await,
        StatusCode::BAD_REQUEST
    );
    let code = authorize_at_mock(location).await;
    assert_eq!(
        redeem(&fixture.mock, &code, secrets.code_verifier()).await,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_start_has_its_own_flow_and_secrets(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    let first = fixture.start(fixture.org, "/auth/mock/start").await;
    let second = fixture.start(fixture.org, "/auth/mock/start").await;

    assert_ne!(first.flow_cookie(), second.flow_cookie());
    assert_ne!(first.param("state"), second.param("state"));
    assert_ne!(first.param("nonce"), second.param("nonce"));
    assert_ne!(
        first.param("code_challenge"),
        second.param("code_challenge")
    );
    assert_eq!(fixture.flows().await.len(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_return_to_that_could_leave_the_site_is_stored_as_root(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    let answer = fixture
        .start(fixture.org, "/auth/mock/start?return_to=%2F%2Fevil.example")
        .await;

    assert_eq!(answer.status, StatusCode::SEE_OTHER);
    let flows = fixture.flows().await;
    assert_eq!(flows[0].4, "/");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_an_unknown_disabled_local_or_other_organizations_key_is_not_found(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let issuer = fixture.mock.issuer();
    fixture.provider(fixture.org, "off", &issuer, false).await;
    let other = fixture.second_org().await;
    fixture.provider(other, "theirs", &issuer, true).await;
    organizations::create(&fixture.pool, "org-c", "Org C", now())
        .await
        .unwrap();
    // `organizations::create` gave organization A its local row (spec 04 B1), key `local`.
    for path in [
        "/auth/nobody/start",
        "/auth/off/start",
        "/auth/theirs/start",
        "/auth/local/start",
        "/auth/a%00b/start",
    ] {
        let answer = fixture.start(fixture.org, path).await;
        answer.assert_refused("auth.provider_not_found");
    }
    assert!(fixture.flows().await.is_empty(), "no flow was created");
    assert_eq!(fixture.discoveries(), 0, "nothing was fetched");

    // Organization B's own key works for B.
    let theirs = fixture.start(other, "/auth/theirs/start").await;
    assert_eq!(theirs.status, StatusCode::SEE_OTHER);
    assert!(theirs.flow_cookie().is_some());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_an_issuer_differing_by_a_trailing_slash_is_unavailable(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let configured = format!("{}/", fixture.mock.issuer());
    fixture
        .provider(fixture.org, "mock", &configured, true)
        .await;

    let answer = fixture.start(fixture.org, "/auth/mock/start").await;

    answer.assert_refused("auth.provider_unavailable");
    assert_eq!(fixture.discoveries(), 1);
    assert!(fixture.flows().await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_discovery_endpoint_answering_500_is_unavailable(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;
    fixture.mock.set_fault(
        Endpoint::Discovery,
        Fault::status(MockStatus::INTERNAL_SERVER_ERROR),
    );

    let answer = fixture.start(fixture.org, "/auth/mock/start").await;

    answer.assert_refused("auth.provider_unavailable");
    assert!(fixture.flows().await.is_empty());

    // A failure is not cached: the provider works once it recovers.
    fixture.mock.clear_fault(Endpoint::Discovery);
    let answer = fixture.start(fixture.org, "/auth/mock/start").await;
    assert!(answer.flow_cookie().is_some(), "{answer:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn without_the_insecure_switch_a_loopback_provider_is_never_contacted(pool: PgPool) {
    let fixture = Fixture::with(pool, Limits::DEFAULT, false).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    let answer = fixture.start(fixture.org, "/auth/mock/start").await;

    answer.assert_refused("auth.provider_unavailable");
    assert!(
        fixture.mock.requests().is_empty(),
        "nothing reached the mock"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_config_that_does_not_read_for_its_kind_is_unavailable(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    // An `oidc` config on an `oauth2` row, and an `oidc` row missing its client id.
    fixture
        .provider_of_kind(fixture.org, "plain", &fixture.mock.issuer(), true, "oauth2")
        .await;
    insert_provider(
        &fixture.pool,
        fixture.org,
        new_id(),
        "broken",
        "oidc",
        true,
        json!({ "issuer": fixture.mock.issuer() }),
    )
    .await;

    for path in ["/auth/plain/start", "/auth/broken/start"] {
        fixture
            .start(fixture.org, path)
            .await
            .assert_refused("auth.provider_unavailable");
    }
    assert!(fixture.mock.requests().is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q5_the_discovery_document_is_fetched_once_an_hour(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    fixture.start(fixture.org, "/auth/mock/start").await;
    fixture
        .clock
        .advance(DISCOVERY_TTL - Duration::from_secs(1));
    let cached = fixture.start(fixture.org, "/auth/mock/start").await;
    assert!(cached.flow_cookie().is_some());
    assert_eq!(fixture.discoveries(), 1);

    fixture.clock.advance(Duration::from_secs(1));
    fixture.start(fixture.org, "/auth/mock/start").await;
    assert_eq!(fixture.discoveries(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b16_the_start_route_is_rate_limited_before_anything_runs(pool: PgPool) {
    let limits = Limits {
        sign_in: NonZeroU32::new(2).unwrap(),
        ..Limits::DEFAULT
    };
    let fixture = Fixture::with(pool, limits, true).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    for _ in 0..2 {
        let answer = fixture.start(fixture.org, "/auth/mock/start").await;
        assert!(answer.flow_cookie().is_some(), "{answer:?}");
    }
    let limited = fixture.start(fixture.org, "/auth/mock/start").await;

    limited.assert_refused("auth.rate_limited");
    assert_eq!(limited.headers[header::RETRY_AFTER], "60");
    assert_eq!(
        fixture.flows().await.len(),
        2,
        "the refused request made no flow"
    );
    assert_eq!(fixture.discoveries(), 1);

    fixture.clock.advance(Duration::from_secs(60));
    let again = fixture.start(fixture.org, "/auth/mock/start").await;
    assert!(again.flow_cookie().is_some(), "the window closed");
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

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn no_log_line_carries_the_flows_secrets_or_cookie(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let answer = fixture.start(fixture.org, "/auth/mock/start").await;
    fixture
        .mock
        .set_discovery_field("issuer", Some(json!("https://other.example")));
    fixture.clock.advance(DISCOVERY_TTL);
    let refused = fixture.start(fixture.org, "/auth/mock/start").await;
    drop(guard);

    refused.assert_refused("auth.provider_unavailable");
    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("discovery document was not accepted"),
        "the refusal is logged: {log}"
    );
    let flows = fixture.flows().await;
    let secrets = open(fixture.org, flows[0].0, &flows[0].3);
    let cookie = answer.flow_cookie().unwrap();
    let token = cookie.trim_start_matches("__Host-sc_flow=");
    for secret in [
        secrets.state(),
        secrets.nonce(),
        secrets.code_verifier(),
        token,
        fixture.mock.client_secret().as_str(),
    ] {
        assert!(!log.contains(secret), "a secret reached the log");
    }
}

/// The handler picks the provider client by the organization count (Q4): the cache keeps a
/// document per URL policy, so a second organization makes the next start fetch again under the
/// multi-organization policy, and only then.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q4_the_client_follows_the_organization_count(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture
        .provider(fixture.org, "mock", &fixture.mock.issuer(), true)
        .await;

    fixture.start(fixture.org, "/auth/mock/start").await;
    fixture.start(fixture.org, "/auth/mock/start").await;
    assert_eq!(fixture.discoveries(), 1, "one organization, one policy");

    fixture.second_org().await;
    let multi = fixture.start(fixture.org, "/auth/mock/start").await;
    assert!(multi.flow_cookie().is_some(), "{multi:?}");
    assert_eq!(
        fixture.discoveries(),
        2,
        "two organizations: the multi-organization client"
    );
    fixture.start(fixture.org, "/auth/mock/start").await;
    assert_eq!(fixture.discoveries(), 2);
}
