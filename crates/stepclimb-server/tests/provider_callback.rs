// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /auth/{provider}/callback` over the router, against the mock IdP on 127.0.0.1
//! (docs/specs/m0/05-oidc.md B7 to B16, T4 to T15; spec 08 B18).
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
use http_body_util::BodyExt;
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
use stepclimb_mock_idp::{
    AuthorizeError, Endpoint, Fault, MockIdp, MockUser, StatusCode as MockStatus,
};
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

    /// `name=value` of the session cookie this answer sets.
    fn session_cookie(&self) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|value| value.starts_with("__Host-sc_session="))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
    }

    /// A completed sign-in (B14): 303 to `return_to`, `no-store`, the flow cookie cleared and a
    /// session cookie set.
    fn assert_signed_in(&self, return_to: &str) {
        assert_eq!(self.status, StatusCode::SEE_OTHER, "{self:?}");
        assert_eq!(self.location(), return_to, "{self:?}");
        assert_eq!(self.headers[header::CACHE_CONTROL], "no-store");
        let cookies = self.set_cookies();
        assert!(cookies.contains(&CLEARED.to_owned()), "{self:?}");
        assert!(self.session_cookie().is_some(), "a session: {self:?}");
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
        passwords: Default::default(),
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

    /// An enabled provider of `kind` in `org` with `key`, pointing at the mock.
    async fn provider(&self, org: Uuid, key: &str, kind: &str) -> Uuid {
        self.provider_with(org, key, kind, json!({})).await
    }

    /// [`Fixture::provider`] with `extra` members laid over the config.
    async fn provider_with(&self, org: Uuid, key: &str, kind: &str, extra: Value) -> Uuid {
        let id = new_id();
        let mut config = json!({
            "issuer": self.mock.issuer(),
            "client_id": self.mock.client_id(),
            "client_secret": self.mock.client_secret(),
        });
        for (name, value) in extra.as_object().expect("an object") {
            config[name] = value.clone();
        }
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
        self.start_at(&format!("/auth/{key}/start"), key).await
    }

    /// Start at `path`, a start route of `key`, and follow the authorization URL at the mock.
    async fn start_at(&self, path: &str, key: &str) -> Started {
        self.start_in(self.org, path, key).await
    }

    /// [`Fixture::start_at`] in `org`.
    async fn start_in(&self, org: Uuid, path: &str, key: &str) -> Started {
        let answer = self.get(org, path, None).await;
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
            "users" => "SELECT count(*) FROM users",
            "identities" => "SELECT count(*) FROM identities",
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

    /// The organization's `admission.signup_allowed` (`07-admission.md` B21).
    async fn allow_signup(&self, allowed: bool) {
        self.allow_signup_in(self.org, allowed).await;
    }

    /// [`Fixture::allow_signup`] for `org`.
    async fn allow_signup_in(&self, org: Uuid, allowed: bool) {
        sqlx::query(
            "UPDATE organizations
             SET settings = settings || jsonb_build_object('admission',
                                jsonb_build_object('signup_allowed', $2::boolean))
             WHERE id = $1",
        )
        .bind(org)
        .bind(allowed)
        .execute(&self.pool)
        .await
        .expect("set sign-up");
    }

    /// `GET /api/v1/me` in `org` with `cookie`: the status and the body.
    async fn me(&self, org: Uuid, cookie: &str) -> (StatusCode, Value) {
        let request = Request::get("/api/v1/me")
            .header(TEST_ORG_HEADER, org.to_string())
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let body = response.into_body().collect().await.expect("a body");
        let body = serde_json::from_slice(&body.to_bytes()).unwrap_or(Value::Null);
        (status, body)
    }

    /// The email of the user whose identity has `subject`, in `org`.
    async fn email_of(&self, org: Uuid, subject: &str) -> Option<String> {
        sqlx::query_scalar(
            "SELECT u.email FROM users u JOIN identities i ON i.user_id = u.id \
             WHERE i.org_id = $1 AND i.subject = $2",
        )
        .bind(org)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .expect("the user")
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

/// T13, T4: a full sign-in ends with a session cookie and a 303 to `return_to`; the flow is
/// used, the stored PKCE verifier was sent, and `GET /api/v1/me` with the cookie is the new
/// user, named and mailed from the ID token's claims (T12's defaults).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_a_full_sign_in_creates_the_user_and_the_session(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let provider = fixture.provider(fixture.org, "mock", "oidc").await;
    let started = fixture
        .start_at("/auth/mock/start?return_to=/c/s1", "mock")
        .await;
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

    answer.assert_signed_in("/c/s1");
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
    assert_eq!(fixture.count("sessions").await, 1);
    let identities: Vec<(Uuid, String, Value)> =
        sqlx::query_as("SELECT provider_id, subject, attributes FROM identities WHERE org_id = $1")
            .bind(fixture.org)
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(
        identities,
        [(provider, "mock-user-1".to_owned(), json!({}))]
    );
    let cookie = answer.session_cookie().expect("a session");
    let (status, me) = fixture.me(fixture.org, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["user"]["given_name"], "Test");
    assert_eq!(me["user"]["family_name"], "Student");
    assert_eq!(me["user"]["email"], "test.student@example.test");
    assert_eq!(me["user"]["source"], "idp");

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

    // A second sign-in is the same user, through the same identity.
    fixture.sign_in().await.assert_signed_in("/");
    assert_eq!(fixture.count("users").await, 1);
    assert_eq!(fixture.count("identities").await, 1);
}

/// T12: a custom `attributes.department` stores the claim, a pointer to a missing claim leaves
/// its attribute out, a numeric `given_name` is ignored, and the subject is `sub`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_claims_are_mapped_by_the_providers_pointers(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let claims = json!({
        "claims": { "attributes": { "department": "/department", "team": "/team/name" } },
    });
    fixture
        .provider_with(fixture.org, "mock", "oidc", claims)
        .await;
    fixture
        .mock
        .set_id_token_claim("department", Some(json!("Operations")));
    fixture
        .mock
        .set_id_token_claim("given_name", Some(json!(7)));
    fixture
        .mock
        .set_userinfo_claim("given_name", Some(json!(7)));

    fixture.sign_in().await.assert_signed_in("/");

    let (subject, attributes, given_name, family_name): (String, Value, String, String) =
        sqlx::query_as(
            "SELECT i.subject, i.attributes, u.given_name, u.family_name \
             FROM identities i JOIN users u ON u.id = i.user_id WHERE i.org_id = $1",
        )
        .bind(fixture.org)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(subject, "mock-user-1");
    assert_eq!(attributes, json!({ "department": "Operations" }));
    assert_eq!(given_name, "", "a numeric given_name is absent");
    assert_eq!(family_name, "Student");
}

/// B13, B14: when `email_verified` is not exactly `true`, or the email comes from a source
/// that does not itself vouch for it, the user is created without the email.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_email_the_provider_does_not_vouch_for_never_reaches_admission(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let sign_in_as = |subject: &'static str| {
        fixture.mock.set_id_token_claim("sub", Some(json!(subject)));
        fixture.mock.set_userinfo_claim("sub", Some(json!(subject)));
        fixture.sign_in()
    };

    // The ID token and userinfo both say `false`.
    fixture
        .mock
        .set_id_token_claim("email_verified", Some(json!(false)));
    fixture
        .mock
        .set_userinfo_claim("email_verified", Some(json!(false)));
    sign_in_as("unverified").await.assert_signed_in("/");
    assert_eq!(fixture.email_of(fixture.org, "unverified").await, None);

    // Userinfo's `true` does not overrule the ID token's `false` for the same address.
    fixture
        .mock
        .set_userinfo_claim("email_verified", Some(json!(true)));
    sign_in_as("overruled").await.assert_signed_in("/");
    assert_eq!(fixture.email_of(fixture.org, "overruled").await, None);

    // Userinfo's `false` wins the merge over the ID token's `true`, even without an address.
    fixture.mock.clear_overrides();
    fixture.mock.set_userinfo_claim("email", None);
    fixture
        .mock
        .set_userinfo_claim("email_verified", Some(json!(false)));
    sign_in_as("withdrawn").await.assert_signed_in("/");
    assert_eq!(fixture.email_of(fixture.org, "withdrawn").await, None);

    // The ID token's `true` does not vouch for a userinfo address that says nothing.
    fixture.mock.clear_overrides();
    fixture
        .mock
        .set_userinfo_claim("email", Some(json!("other.student@example.test")));
    fixture.mock.set_userinfo_claim("email_verified", None);
    sign_in_as("borrowed").await.assert_signed_in("/");
    assert_eq!(fixture.email_of(fixture.org, "borrowed").await, None);

    // T11: a userinfo address userinfo vouches for overrides the ID token's.
    fixture
        .mock
        .set_userinfo_claim("email_verified", Some(json!(true)));
    sign_in_as("overridden").await.assert_signed_in("/");
    assert_eq!(
        fixture.email_of(fixture.org, "overridden").await.as_deref(),
        Some("other.student@example.test")
    );
}

/// T14: an admission refusal (sign-up off, a new person) redirects with `auth.signup_closed`,
/// sets no session and creates no user or identity.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_sign_up_off_refuses_a_new_person(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    fixture.allow_signup(false).await;

    fixture.sign_in().await.assert_refused("auth.signup_closed");

    assert_eq!(fixture.count("sessions").await, 0);
    assert_eq!(fixture.count("users").await, 0);
    assert_eq!(fixture.count("identities").await, 0);
    assert_eq!(fixture.count("auth_flows").await, 0, "the flow is used");
}

/// `07-admission.md` B4: a known identity whose user is suspended is refused with
/// `auth.suspended` and gets no session.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_user_is_refused(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    fixture.sign_in().await.assert_signed_in("/");
    sqlx::query("UPDATE users SET status = 'suspended' WHERE org_id = $1")
        .bind(fixture.org)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let sessions = fixture.count("sessions").await;

    fixture.sign_in().await.assert_refused("auth.suspended");

    assert_eq!(fixture.count("sessions").await, sessions, "no new session");
}

/// `07-admission.md` T7 over the whole callback: a rule on a mapped claim admits the person
/// whose claim matches and refuses one whose claim differs with `auth.admission_refused`,
/// creating no user, identity or session; a refused known user keeps the refusal marker.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admission_rule_on_a_mapped_claim_refuses_at_the_callback(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let claims = json!({ "claims": { "attributes": { "subdivision": "/division" } } });
    fixture
        .provider_with(fixture.org, "mock", "oidc", claims)
        .await;
    let rules = json!([{ "attribute": "subdivision", "op": "equals", "value": "USA" }]);
    sqlx::query(
        "UPDATE identity_providers
         SET config = config || jsonb_build_object('admission_rules', $2::jsonb)
         WHERE org_id = $1 AND key = 'mock'",
    )
    .bind(fixture.org)
    .bind(rules)
    .execute(&fixture.pool)
    .await
    .unwrap();

    fixture
        .mock
        .set_id_token_claim("division", Some(json!("CAN")));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.admission_refused");
    assert_eq!(fixture.count("users").await, 0);
    assert_eq!(fixture.count("identities").await, 0);
    assert_eq!(fixture.count("sessions").await, 0);

    fixture
        .mock
        .set_id_token_claim("division", Some(json!("USA")));
    fixture.sign_in().await.assert_signed_in("/");
    assert_eq!(fixture.count("users").await, 1);

    // The refusal of a known user keeps the refusal marker the callback commits, and the
    // next sign-in that passes the rules clears it.
    fixture
        .mock
        .set_id_token_claim("division", Some(json!("CAN")));
    fixture
        .sign_in()
        .await
        .assert_refused("auth.admission_refused");
    assert_eq!(marker(&fixture).await.as_deref(), Some("mock"));
    fixture
        .mock
        .set_id_token_claim("division", Some(json!("USA")));
    fixture.sign_in().await.assert_signed_in("/");
    assert_eq!(marker(&fixture).await, None);
}

/// The provider key of the refusal marker of the organization's one user.
async fn marker(fixture: &Fixture) -> Option<String> {
    sqlx::query_scalar("SELECT admission_refused_provider FROM users WHERE org_id = $1")
        .bind(fixture.org)
        .fetch_one(&fixture.pool)
        .await
        .expect("read the marker")
}

/// Cross-organization (T2's callback half, `07-admission.md` B4): organization B's provider of
/// the same key cannot finish A's flow, A's session is not B's, and the same `sub` signing in
/// at B's provider is another person in B, never A's user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organization_neither_finishes_the_flow_nor_shares_the_user(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let other = organizations::create(&fixture.pool, "org-b", "Org B", now())
        .await
        .unwrap()
        .id;
    fixture.provider(other, "mock", "oidc").await;
    let started = fixture.start("mock").await;

    let crossed = fixture
        .get(
            other,
            &format!("/auth/mock/callback?{}", started.back),
            Some(&started.cookie),
        )
        .await;
    crossed.assert_refused("auth.flow_invalid");
    assert_eq!(fixture.token_requests(), 0, "nothing reached the provider");
    assert_eq!(fixture.count("sessions").await, 0);

    let answer = fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await;
    answer.assert_signed_in("/");
    let cookie = answer.session_cookie().expect("a session");
    let (status, _) = fixture.me(other, &cookie).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "A's session is not B's");
    let users: Vec<Uuid> = sqlx::query_scalar("SELECT org_id FROM users")
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(users, [fixture.org]);

    // The same mock user, the same `sub`, signs in at B.
    fixture.allow_signup_in(other, true).await;
    let at_b = fixture.start_in(other, "/auth/mock/start", "mock").await;
    let answer = fixture
        .get(
            other,
            &format!("/auth/mock/callback?{}", at_b.back),
            Some(&at_b.cookie),
        )
        .await;
    answer.assert_signed_in("/");
    let users: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT id, org_id FROM users ORDER BY org_id")
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
    let mut orgs: Vec<Uuid> = users.iter().map(|(_, org)| *org).collect();
    orgs.sort();
    let mut expected = vec![fixture.org, other];
    expected.sort();
    assert_eq!(orgs, expected, "one user in each organization");
    let identities: Vec<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT i.org_id, i.user_id, u.org_id, i.subject FROM identities i \
         JOIN users u ON u.id = i.user_id ORDER BY i.org_id",
    )
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(identities.len(), 2, "{identities:?}");
    for (identity_org, user_id, user_org, subject) in &identities {
        assert_eq!(subject, "mock-user-1");
        assert_eq!(
            identity_org, user_org,
            "an identity belongs to its own organization's user"
        );
        let owner = users
            .iter()
            .find(|(id, _)| id == user_id)
            .expect("the user");
        assert_eq!(owner.1, *identity_org);
    }
    let b_user = users.iter().find(|(_, org)| *org == other).unwrap().0;
    let b_identity = identities.iter().find(|i| i.0 == other).unwrap();
    assert_eq!(b_identity.1, b_user, "B's identity is B's user");
    let cookie = answer.session_cookie().expect("a session");
    let (status, me) = fixture.me(other, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["user"]["id"], json!(b_user.to_string()));
}

/// B14, spec 08 B19: the flow's `return_to` is sanitized again when the flow is read, so a
/// stored one the column's check lets through but a browser would resolve to another host
/// (`/a/..//evil.example`) lands on `/`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_return_to_is_sanitized_again_when_the_flow_is_read(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.provider(fixture.org, "mock", "oidc").await;
    let started = fixture
        .start_at("/auth/mock/start?return_to=/c/s1", "mock")
        .await;
    sqlx::query("UPDATE auth_flows SET return_to = '/a/..//evil.example/x'")
        .execute(&fixture.pool)
        .await
        .unwrap();

    fixture
        .callback("mock", &started.back, Some(&started.cookie))
        .await
        .assert_signed_in("/");
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
    fixture.sign_in().await.assert_signed_in("/");

    fixture.mock.rotate_signing_key().unwrap();
    fixture
        .sign_in()
        .await
        .assert_refused("auth.id_token_invalid");
    fixture
        .clock
        .advance(stepclimb_auth::oidc::JWKS_REFETCH_INTERVAL);
    fixture.sign_in().await.assert_signed_in("/");
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

/// T15 (B15): after a sign-in and a refused one, no log line at any level holds a token the
/// mock issued, the code, the state, the flow cookie, the session cookie or the client secret,
/// and no column of the flows, sessions, identities or users tables holds a token.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_no_log_line_or_row_carries_a_token_the_code_or_a_secret(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let attributes = json!({ "claims": { "attributes": { "department": "/department" } } });
    fixture
        .provider_with(fixture.org, "mock", "oidc", attributes)
        .await;
    // Values distinct enough that a hit in the log can only be the claim.
    let profile = [
        "zqmail.student@example.test",
        "Zqgivenname",
        "Zqfamilyname",
        "Zqdepartment",
    ];
    fixture.mock.set_user(
        MockUser::new("mock-user-1")
            .with_claim("email", json!(profile[0]))
            .with_claim("email_verified", json!(true))
            .with_claim("given_name", json!(profile[1]))
            .with_claim("family_name", json!(profile[2]))
            .with_claim("department", json!(profile[3])),
    );
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

    answer.assert_signed_in("/");
    refused.assert_refused("auth.provider_error");
    let log = log.text();
    assert!(log.contains("admission admitted the sign-in"), "{log}");
    assert!(log.contains("userinfo sub"), "{log}");
    let session = answer.session_cookie().expect("a session");
    let mut secrets = vec![
        fixture.mock.client_secret(),
        session.trim_start_matches("__Host-sc_session=").to_owned(),
    ];
    let mut tokens = Vec::new();
    for issued in fixture.mock.issued_tokens() {
        tokens.push(issued.access_token);
        tokens.push(issued.refresh_token);
        tokens.push(issued.id_token.expect("an ID token"));
    }
    assert_eq!(tokens.len(), 6, "two exchanges");
    secrets.extend(tokens.iter().cloned());
    for run in [&started, &second] {
        secrets.push(param(&run.back, "code"));
        secrets.push(param(&run.back, "state"));
        secrets.push(run.cookie.trim_start_matches("__Host-sc_flow=").to_owned());
    }
    for secret in secrets {
        assert!(!log.contains(&secret), "a secret reached the log");
    }
    // The claims did reach the user and identity, and still none of them is logged (B4 of
    // spec 08, `07-admission.md` B2).
    let (email, department): (Option<String>, Value) = sqlx::query_as(
        "SELECT u.email, i.attributes -> 'department' FROM users u \
         JOIN identities i ON i.user_id = u.id WHERE u.org_id = $1",
    )
    .bind(fixture.org)
    .fetch_one(&fixture.pool)
    .await
    .expect("the user");
    assert_eq!(email.as_deref(), Some(profile[0]));
    assert_eq!(department, json!(profile[3]));
    for value in profile {
        assert!(
            !log.contains(value),
            "a profile value reached the log: {value}"
        );
    }
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT row_to_json(f)::text FROM auth_flows f \
         UNION ALL SELECT row_to_json(s)::text FROM sessions s \
         UNION ALL SELECT row_to_json(i)::text FROM identities i \
         UNION ALL SELECT row_to_json(u)::text FROM users u",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read rows");
    assert!(
        rows.len() >= 3,
        "a session, an identity and a user: {rows:?}"
    );
    for token in &tokens {
        // A bytea column shows as hex in `row_to_json`.
        let hex: String = token.bytes().map(|byte| format!("{byte:02x}")).collect();
        assert!(
            !rows
                .iter()
                .any(|row| row.contains(token.as_str()) || row.contains(&hex)),
            "a token reached a row"
        );
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

/// `07-admission.md` T14, B22: a provider whose mapping names a groups claim gives the user its
/// groups through the whole callback, and the next sign-in's claim replaces them; a claim the
/// provider stops sending is no groups.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn claimed_groups_reach_the_user_through_the_callback(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let claims = json!({ "claims": { "groups": "/groups" } });
    fixture
        .provider_with(fixture.org, "mock", "oidc", claims)
        .await;
    let memberships = || async {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT g.key, m.source FROM group_members m
             JOIN groups g ON g.org_id = m.org_id AND g.id = m.group_id
             WHERE m.org_id = $1 ORDER BY g.key",
        )
        .bind(fixture.org)
        .fetch_all(&fixture.pool)
        .await
        .expect("read memberships");
        rows
    };
    let idp = |keys: &[&str]| -> Vec<(String, String)> {
        keys.iter()
            .map(|&key| (key.to_owned(), "idp".to_owned()))
            .collect()
    };

    fixture
        .mock
        .set_id_token_claim("groups", Some(json!(["home", "staff"])));
    fixture.sign_in().await.assert_signed_in("/");
    assert_eq!(memberships().await, idp(&["home", "staff"]));

    fixture
        .mock
        .set_id_token_claim("groups", Some(json!(["home"])));
    fixture.sign_in().await.assert_signed_in("/");
    assert_eq!(memberships().await, idp(&["home"]));

    fixture.mock.set_id_token_claim("groups", None);
    fixture.sign_in().await.assert_signed_in("/");
    assert!(memberships().await.is_empty());
}
