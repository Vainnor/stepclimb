// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generic OAuth 2 sign-in over the router, `GET /auth/{provider}/start` and
//! `/auth/{provider}/callback`, against the mock IdP's `/oauth2` provider on 127.0.0.1
//! (docs/specs/m0/06-oauth2-and-vatsim-preset.md B5, B7 to B10, T3, T5 to T8; `05-oidc.md` B7,
//! B8, B16).
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
    oauth2_config::{self, ProviderRow},
    oauth2_flow::FlowSecrets,
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
const REDIRECT_URI: &str = "https://lms.example.org/auth/plain/callback";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn keys() -> KeyRing {
    KeyRing::parse(KEY_RING).expect("a valid test key")
}

/// A VATSIM-shaped synthetic userinfo body.
fn userinfo_body() -> Value {
    json!({
        "data": {
            "cid": 1000001,
            "personal": { "name_first": "Test", "name_last": "Student", "email": "test.student@example.test" },
            "vatsim": { "rating": { "short": "S1" } }
        }
    })
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
        self.set_cookies()
            .into_iter()
            .find(|value| value.starts_with("__Host-sc_flow=") && !value.contains("Max-Age=0"))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
    }

    fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect()
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
        assert_eq!(self.headers[header::CACHE_CONTROL], "no-store");
        assert!(
            !self
                .set_cookies()
                .iter()
                .any(|c| c.starts_with("__Host-sc_session=")),
            "no session"
        );
    }

    /// `name=value` of the session cookie this answer sets.
    fn session_cookie(&self) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|value| value.starts_with("__Host-sc_session="))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
    }

    /// A completed sign-in: 303 to `return_to`, a session cookie, the flow cookie cleared.
    fn assert_signed_in(&self, return_to: &str) {
        assert_eq!(self.status, StatusCode::SEE_OTHER, "{self:?}");
        assert_eq!(self.location(), return_to, "{self:?}");
        assert!(self.session_cookie().is_some(), "a session: {self:?}");
        assert!(
            self.set_cookies()
                .iter()
                .any(|c| c.starts_with("__Host-sc_flow=;") && c.contains("Max-Age=0")),
            "the flow cookie is cleared: {self:?}"
        );
        assert_eq!(self.headers[header::CACHE_CONTROL], "no-store");
    }

    /// The callback's refusal, which also clears the flow cookie.
    fn assert_callback_refused(&self, code: &str) {
        self.assert_refused(code);
        assert!(
            self.set_cookies()
                .iter()
                .any(|c| c.starts_with("__Host-sc_flow=;") && c.contains("Max-Age=0")),
            "the flow cookie is cleared: {self:?}"
        );
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
        providers: Providers::new(insecure).expect("provider clients"),
    }
}

/// A started sign-in: the start's answer, its flow cookie and the code the mock sent back.
struct Started {
    start: Answer,
    cookie: String,
    state: String,
    code: String,
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
        mock.set_oauth2_userinfo(Some(userinfo_body()));
        let fixture = Self {
            pool,
            clock,
            app,
            mock,
            org,
        };
        fixture.allow_signup(true).await;
        fixture
    }

    fn config(&self) -> Value {
        json!({
            "authorize": format!("{}?prompt=login", self.mock.url(Endpoint::OAuth2Authorize)),
            "token": self.mock.url(Endpoint::OAuth2Token),
            "userinfo": self.mock.url(Endpoint::OAuth2Userinfo),
            "client_id": self.mock.client_id(),
            "client_secret": self.mock.client_secret(),
            "scopes": ["full_name", "email"],
            "claims": {
                "subject": "/data/cid",
                "given_name": "/data/personal/name_first",
                "attributes": { "rating": "/data/vatsim/rating/short" }
            }
        })
    }

    /// An `oauth2` provider of `org` with `key` and `config`.
    async fn provider_with(&self, org: Uuid, key: &str, enabled: bool, config: &Value) -> Uuid {
        let id = new_id();
        let row = ProviderRow {
            org_id: org,
            provider_id: id,
        };
        let stored = oauth2_config::validate(config, UrlPolicy::new(true, true), None)
            .expect("a valid config")
            .seal(&keys(), row)
            .expect("seal")
            .to_json();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled) \
             VALUES ($1, $2, $3, 'oauth2', $4, $5)",
        )
        .bind(id)
        .bind(org)
        .bind(key)
        .bind(Value::Object(stored))
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    async fn provider(&self, org: Uuid, key: &str) -> Uuid {
        self.provider_with(org, key, true, &self.config()).await
    }

    async fn get(&self, org: Uuid, path: &str, cookie: Option<&str>) -> Answer {
        let mut request = Request::get(path)
            .header(TEST_ORG_HEADER, org.to_string())
            .header(header::HOST, "evil.example");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::empty()).expect("a request"))
            .await
            .expect("an answer");
        Answer {
            status: response.status(),
            headers: response.headers().clone(),
        }
    }

    async fn start(&self, org: Uuid, path: &str) -> Answer {
        self.get(org, path, None).await
    }

    /// Start at `/auth/plain/start` and follow the authorization URL at the mock.
    async fn started(&self) -> Started {
        let start = self
            .start(self.org, "/auth/plain/start?return_to=/c/s1")
            .await;
        assert_eq!(start.status, StatusCode::SEE_OTHER, "{start:?}");
        let cookie = start.flow_cookie().expect("the flow cookie");
        let back = follow_at_mock(start.location()).await;
        let param = |name: &str| {
            back.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .expect("the mock sends it back")
        };
        Started {
            state: param("state"),
            code: param("code"),
            start,
            cookie,
        }
    }

    async fn callback(&self, cookie: Option<&str>, query: &str) -> Answer {
        self.get(self.org, &format!("/auth/plain/callback?{query}"), cookie)
            .await
    }

    async fn flows(&self) -> Vec<(Uuid, String, Option<Uuid>, Vec<u8>)> {
        sqlx::query_as("SELECT id, kind, provider_id, payload FROM auth_flows")
            .fetch_all(&self.pool)
            .await
            .expect("read flows")
    }

    /// The organization's `admission.signup_allowed` (`07-admission.md` B21).
    async fn allow_signup(&self, allowed: bool) {
        sqlx::query(
            "UPDATE organizations
             SET settings = settings || jsonb_build_object('admission',
                                jsonb_build_object('signup_allowed', $2::boolean))
             WHERE id = $1",
        )
        .bind(self.org)
        .bind(allowed)
        .execute(&self.pool)
        .await
        .expect("set sign-up");
    }

    async fn sessions(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sessions")
            .fetch_one(&self.pool)
            .await
            .expect("count sessions")
    }
}

fn query(code: &str, state: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("code", code)
        .append_pair("state", state)
        .finish()
}

/// GET `location` at the mock, which approves at once: the redirect it sends back.
async fn follow_at_mock(location: &str) -> Url {
    let client = OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client");
    let request = axum::http::Request::get(location)
        .body(Vec::new())
        .expect("a request");
    let response = client.send(request).await.expect("the mock answers");
    assert_eq!(response.status(), StatusCode::FOUND, "the mock approves");
    let back = response.headers()[header::LOCATION]
        .to_str()
        .expect("an ASCII Location");
    assert!(back.starts_with(REDIRECT_URI), "{back}");
    Url::parse(back).expect("an absolute redirect")
}

fn open(org: Uuid, flow: Uuid, sealed: &[u8]) -> FlowSecrets {
    let context: [&[u8]; 4] = [org.as_bytes(), b"auth_flows", flow.as_bytes(), b"payload"];
    let payload = keys().decrypt(sealed, &context).expect("the payload opens");
    FlowSecrets::from_payload(&payload).expect("an OAuth 2 payload")
}

/// Spec 06 T3: the authorize redirect carries `response_type=code`, the space-joined scopes,
/// the redirect URI, a state and, PKCE on by default (Q3), the S256 challenge of the stored
/// verifier; the provider's own query parameter is kept.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_start_sends_the_browser_to_the_authorize_url(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture.provider(fixture.org, "plain").await;

    let answer = fixture
        .start(fixture.org, "/auth/plain/start?return_to=/c/s1")
        .await;

    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    let authorize = fixture.mock.url(Endpoint::OAuth2Authorize);
    assert!(
        answer
            .location()
            .starts_with(&format!("{authorize}?prompt=login&response_type=code&")),
        "{}",
        answer.location()
    );
    assert_eq!(answer.param("client_id"), Some(fixture.mock.client_id()));
    assert_eq!(answer.param("redirect_uri").as_deref(), Some(REDIRECT_URI));
    assert_eq!(answer.param("scope").as_deref(), Some("full_name email"));
    assert_eq!(answer.param("nonce"), None);
    assert_eq!(
        answer.param("code_challenge_method").as_deref(),
        Some("S256")
    );
    let flows = fixture.flows().await;
    let [(flow_id, kind, provider_id, sealed)] = flows.as_slice() else {
        panic!("one flow, got {}", flows.len());
    };
    assert_eq!(kind, "oauth2");
    assert_eq!(*provider_id, Some(provider));
    let secrets = open(fixture.org, *flow_id, sealed);
    assert_eq!(answer.param("state").as_deref(), Some(secrets.state()));
    assert_eq!(answer.param("code_challenge"), secrets.code_challenge());
    assert!(!answer.location().contains(secrets.code_verifier().unwrap()));
}

/// Q3: a provider with `pkce = false` gets no challenge, and its flow keeps no verifier.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_with_pkce_off_no_challenge_is_sent(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let mut config = fixture.config();
    config["pkce"] = json!(false);
    fixture
        .provider_with(fixture.org, "plain", true, &config)
        .await;
    fixture.mock.set_require_pkce(false);

    let started = fixture.started().await;

    assert_eq!(started.start.param("code_challenge"), None);
    assert_eq!(started.start.param("code_challenge_method"), None);
    let answer = fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;
    answer.assert_signed_in("/c/s1");
    let token = fixture.mock.requests_to(Endpoint::OAuth2Token);
    assert_eq!(token[0].form_param("code_verifier"), None);
}

/// The stored `authorize` URL is checked against the outbound URL policy at start: without the
/// insecure switch, the mock's loopback `http` URL is refused and no flow is made.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_authorize_url_the_policy_refuses_is_unavailable(pool: PgPool) {
    let fixture = Fixture::with(pool, Limits::DEFAULT, false).await;
    fixture.provider(fixture.org, "plain").await;

    let answer = fixture.start(fixture.org, "/auth/plain/start").await;

    answer.assert_refused("auth.provider_unavailable");
    assert!(fixture.flows().await.is_empty());
    assert!(fixture.mock.requests().is_empty());
}

/// Spec 05 T2 for an `oauth2` provider, at start and callback: an unknown key, a disabled
/// provider and organization B's key on organization A are not found, and create no flow.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_disabled_or_other_organizations_provider_is_not_found(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let config = fixture.config();
    fixture
        .provider_with(fixture.org, "off", false, &config)
        .await;
    let other = fixture.second_org().await;
    fixture.provider_with(other, "theirs", true, &config).await;
    for key in ["nobody", "off", "theirs"] {
        fixture
            .start(fixture.org, &format!("/auth/{key}/start"))
            .await
            .assert_refused("auth.provider_not_found");
        fixture
            .get(
                fixture.org,
                &format!("/auth/{key}/callback?code=c&state=s"),
                None,
            )
            .await
            .assert_refused("auth.provider_not_found");
    }
    assert!(fixture.flows().await.is_empty());
    assert!(fixture.mock.requests().is_empty());
}

impl Fixture {
    async fn second_org(&self) -> Uuid {
        organizations::create(&self.pool, "org-b", "Org B", now())
            .await
            .expect("create organization")
            .id
    }
}

/// Spec 06 T8: a full sign-in redeems the code (token request with the redirect URI and the
/// verifier, userinfo with the issued access token), creates the identity with the mapped
/// subject and attributes, sets the session cookie and lands on `return_to`; the flow is used,
/// and `GET /api/v1/me` with the cookie is the new user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_a_full_sign_in_creates_the_identity_and_the_session(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture.provider(fixture.org, "plain").await;
    let started = fixture.started().await;
    let flows = fixture.flows().await;
    let secrets = open(fixture.org, flows[0].0, &flows[0].3);

    let answer = fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;

    answer.assert_signed_in("/c/s1");
    let token = fixture.mock.requests_to(Endpoint::OAuth2Token);
    let [token] = token.as_slice() else {
        panic!("one token request");
    };
    assert_eq!(
        token.form_param("code").as_deref(),
        Some(started.code.as_str())
    );
    assert_eq!(
        token.form_param("redirect_uri").as_deref(),
        Some(REDIRECT_URI)
    );
    assert_eq!(
        token.form_param("code_verifier").as_deref(),
        secrets.code_verifier()
    );
    let issued = fixture.mock.issued_tokens();
    let userinfo = fixture.mock.requests_to(Endpoint::OAuth2Userinfo);
    assert_eq!(
        userinfo[0].header("authorization"),
        Some(format!("Bearer {}", issued[0].access_token).as_str())
    );
    let identities: Vec<(Uuid, String, Value)> =
        sqlx::query_as("SELECT provider_id, subject, attributes FROM identities WHERE org_id = $1")
            .bind(fixture.org)
            .fetch_all(&fixture.pool)
            .await
            .expect("read identities");
    assert_eq!(
        identities,
        [(provider, "1000001".to_owned(), json!({ "rating": "S1" }))]
    );
    assert!(fixture.flows().await.is_empty(), "the flow is used");
    assert_eq!(fixture.sessions().await, 1);
    let me = fixture
        .get(
            fixture.org,
            "/api/v1/me",
            answer.session_cookie().as_deref(),
        )
        .await;
    assert_eq!(me.status, StatusCode::OK, "{me:?}");

    // Replaying the callback finds no flow (spec 05 T6).
    let replay = fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;
    replay.assert_callback_refused("auth.flow_invalid");
    assert_eq!(fixture.mock.requests_to(Endpoint::OAuth2Token).len(), 1);
}

/// B10 (spec 05 T14): an admission refusal (sign-up off, a new person) redirects with
/// `auth.signup_closed`, sets no session and creates no identity.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admission_refusal_redirects_with_its_code(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    fixture.allow_signup(false).await;
    let started = fixture.started().await;

    fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await
        .assert_callback_refused("auth.signup_closed");

    assert_eq!(fixture.sessions().await, 0);
    let identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE subject = '1000001'")
            .fetch_one(&fixture.pool)
            .await
            .expect("count identities");
    assert_eq!(identities, 0);
    assert!(fixture.flows().await.is_empty(), "the flow is used");
}

/// A second sign-in of the same subject is the same user: the identity is found, not made
/// again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn signing_in_again_is_the_same_user(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    for _ in 0..2 {
        let started = fixture.started().await;
        fixture
            .callback(Some(&started.cookie), &query(&started.code, &started.state))
            .await
            .assert_signed_in("/c/s1");
    }
    let (users, identities): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users WHERE source = 'idp'), (SELECT count(*) FROM identities WHERE subject = '1000001')",
    )
    .fetch_one(&fixture.pool)
    .await
    .expect("count");
    assert_eq!((users, identities), (1, 1));
}

/// Spec 06 T5 (spec 05 T5): no flow cookie, another flow's cookie, a state differing by one
/// character, or no state is `auth.flow_invalid`, and the provider is never called.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_a_missing_flow_or_a_wrong_state_is_flow_invalid(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    let started = fixture.started().await;

    fixture
        .callback(None, &query(&started.code, &started.state))
        .await
        .assert_callback_refused("auth.flow_invalid");
    let mut wrong = started.state.clone();
    let last = if wrong.pop() == Some('A') { 'B' } else { 'A' };
    wrong.push(last);
    let other = fixture.started().await;
    fixture
        .callback(Some(&other.cookie), &query(&started.code, &wrong))
        .await
        .assert_callback_refused("auth.flow_invalid");
    let third = fixture.started().await;
    fixture
        .callback(Some(&third.cookie), &format!("code={}", third.code))
        .await
        .assert_callback_refused("auth.flow_invalid");

    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
    // The first flow was never presented and is still live; the others were used up.
    assert_eq!(fixture.flows().await.len(), 1);
}

/// A flow of another provider, or of another kind (an OIDC flow), is `auth.flow_invalid`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_for_another_provider_is_flow_invalid(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    fixture.provider(fixture.org, "second").await;
    let start = fixture.start(fixture.org, "/auth/second/start").await;
    let cookie = start.flow_cookie().expect("a flow");
    let state = start.param("state").expect("a state");

    fixture
        .callback(Some(&cookie), &query("c", &state))
        .await
        .assert_callback_refused("auth.flow_invalid");
    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
}

/// Spec 06 T5 (spec 05 T6): a callback after the flow's lifetime is `auth.flow_expired`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_an_expired_flow_is_flow_expired(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    let started = fixture.started().await;

    fixture.clock.advance(LIFETIME + Duration::from_secs(1));
    fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await
        .assert_callback_refused("auth.flow_expired");
    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
}

/// Spec 05 B8, T7 for an `oauth2` provider: `access_denied` is `auth.provider_denied`, another
/// error `auth.provider_error`, and the description never reaches the redirect.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_provider_error_is_denied_or_provider_error(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    for (error, code) in [
        ("access_denied", "auth.provider_denied"),
        ("server_error", "auth.provider_error"),
    ] {
        fixture.mock.set_authorize_error(Some(AuthorizeError {
            error: error.to_owned(),
            description: Some("a description-zq the user never sees".to_owned()),
        }));
        let start = fixture.start(fixture.org, "/auth/plain/start").await;
        let cookie = start.flow_cookie().expect("a flow");
        let back = follow_at_mock(start.location()).await;
        let answer = fixture
            .callback(Some(&cookie), back.query().unwrap_or(""))
            .await;
        answer.assert_callback_refused(code);
        assert!(!answer.location().contains("zq"));
    }
    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
}

/// A fault a test sets on the mock.
type MockFault = dyn Fn(&MockIdp);

/// Spec 06 T6, T7 through the route: a failed token or userinfo request is
/// `auth.provider_error`, a body without a subject `auth.subject_missing`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_t7_a_failed_exchange_or_userinfo_is_refused_with_its_code(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    let cases: [(&MockFault, &str); 5] = [
        (
            &|m| m.set_token_response_field("token_type", Some(json!("mac"))),
            "auth.provider_error",
        ),
        (
            &|m| m.set_token_response_field("access_token", None),
            "auth.provider_error",
        ),
        (
            &|m| {
                m.set_fault(
                    Endpoint::OAuth2Token,
                    Fault::status(MockStatus::BAD_REQUEST),
                )
            },
            "auth.provider_error",
        ),
        (
            &|m| {
                m.set_fault(
                    Endpoint::OAuth2Userinfo,
                    Fault::status(MockStatus::UNAUTHORIZED),
                )
            },
            "auth.provider_error",
        ),
        (
            &|m| m.set_oauth2_userinfo(Some(json!({ "data": { "cid": "" } }))),
            "auth.subject_missing",
        ),
    ];
    for (fault, code) in cases {
        fault(&fixture.mock);
        let started = fixture.started().await;
        fixture
            .callback(Some(&started.cookie), &query(&started.code, &started.state))
            .await
            .assert_callback_refused(code);
        fixture.mock.clear_overrides();
        fixture.mock.clear_fault(Endpoint::OAuth2Token);
        fixture.mock.clear_fault(Endpoint::OAuth2Userinfo);
        fixture.mock.set_oauth2_userinfo(Some(userinfo_body()));
    }
    assert_eq!(fixture.sessions().await, 0);
}

/// Spec 05 B16: the callback counts against the sign-in budget before anything runs.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_callback_is_rate_limited(pool: PgPool) {
    let limits = Limits {
        sign_in: NonZeroU32::new(2).unwrap(),
        ..Limits::DEFAULT
    };
    let fixture = Fixture::with(pool, limits, true).await;
    fixture.provider(fixture.org, "plain").await;
    let started = fixture.started().await;
    fixture.callback(None, "code=c&state=s").await;

    let limited = fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;

    limited.assert_refused("auth.rate_limited");
    assert_eq!(limited.headers[header::RETRY_AFTER], "60");
    assert_eq!(fixture.flows().await.len(), 1, "the flow is untouched");
    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
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

/// Spec 05 B15, T15 for an `oauth2` provider: after a callback, no log line at any level holds
/// the code, the state, the verifier, the client secret or a token the mock issued, and no
/// column of the flows, sessions, identities or users tables holds a token.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn no_log_line_or_row_carries_a_token_code_or_secret(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "plain").await;
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let started = fixture.started().await;
    let flows = fixture.flows().await;
    let secrets = open(fixture.org, flows[0].0, &flows[0].3);
    let answer = fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;
    // A refused exchange logs its reason too.
    fixture.mock.set_fault(
        Endpoint::OAuth2Token,
        Fault::status(MockStatus::BAD_REQUEST),
    );
    let refused = fixture.started().await;
    fixture
        .callback(Some(&refused.cookie), &query(&refused.code, &refused.state))
        .await
        .assert_callback_refused("auth.provider_error");
    drop(guard);

    answer.assert_signed_in("/c/s1");
    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("the provider sign-in was refused"), "{log}");
    let issued = fixture.mock.issued_tokens();
    let token = started.cookie.trim_start_matches("__Host-sc_flow=");
    let secret = fixture.mock.client_secret();
    let session = answer.session_cookie().expect("a session");
    let session = session.trim_start_matches("__Host-sc_session=");
    let mut needles = vec![
        session,
        started.code.as_str(),
        started.state.as_str(),
        refused.code.as_str(),
        secrets.code_verifier().unwrap(),
        token,
        secret.as_str(),
    ];
    needles.extend(
        issued
            .iter()
            .flat_map(|t| [t.access_token.as_str(), t.refresh_token.as_str()]),
    );
    for needle in &needles {
        assert!(!log.contains(needle), "a secret reached the log");
    }
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT row_to_json(t)::text FROM ( \
           SELECT * FROM auth_flows) t \
         UNION ALL SELECT row_to_json(s)::text FROM sessions s \
         UNION ALL SELECT row_to_json(i)::text FROM identities i \
         UNION ALL SELECT row_to_json(u)::text FROM users u",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read rows");
    for token in issued
        .iter()
        .flat_map(|t| [&t.access_token, &t.refresh_token])
    {
        assert!(!rows.iter().any(|row| row.contains(token.as_str())));
    }
}

/// After the flow is taken, a server fault (here the client secret does not open) is 500
/// problem details that still clears the flow cookie, and nothing reaches the provider.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_server_fault_after_the_flow_is_taken_still_clears_its_cookie(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture.provider(fixture.org, "plain").await;
    let started = fixture.started().await;
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
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await;

    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{answer:?}"
    );
    assert_eq!(
        answer.set_cookies(),
        ["__Host-sc_flow=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax"],
        "{answer:?}"
    );
    assert!(fixture.flows().await.is_empty(), "the flow was taken");
    assert!(fixture.mock.requests_to(Endpoint::OAuth2Token).is_empty());
}

/// Spec 06 B10: when the config maps `email_verified` and the provider does not say `true`,
/// admission gets no email, so the new user has none; with `true` the user gets it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b10_an_unverified_email_never_reaches_admission(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let mut config = fixture.config();
    config["claims"]["email"] = json!("/data/personal/email");
    config["claims"]["email_verified"] = json!("/data/personal/verified");
    fixture
        .provider_with(fixture.org, "plain", true, &config)
        .await;
    let email_of = |subject: &'static str| {
        let pool = fixture.pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT u.email FROM users u JOIN identities i ON i.user_id = u.id \
                 WHERE i.subject = $1",
            )
            .bind(subject)
            .fetch_one(&pool)
            .await
            .expect("the new user")
        }
    };

    let mut body = userinfo_body();
    body["data"]["personal"]["verified"] = json!("true");
    fixture.mock.set_oauth2_userinfo(Some(body.clone()));
    let started = fixture.started().await;
    fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await
        .assert_signed_in("/c/s1");
    assert_eq!(email_of("1000001").await, None);

    body["data"]["cid"] = json!(1000002);
    body["data"]["personal"]["email"] = json!("second.student@example.test");
    body["data"]["personal"]["verified"] = json!(true);
    fixture.mock.set_oauth2_userinfo(Some(body));
    let started = fixture.started().await;
    fixture
        .callback(Some(&started.cookie), &query(&started.code, &started.state))
        .await
        .assert_signed_in("/c/s1");
    assert_eq!(
        email_of("1000002").await.as_deref(),
        Some("second.student@example.test")
    );
}
