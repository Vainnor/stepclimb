// SPDX-License-Identifier: AGPL-3.0-or-later
//! TOTP over the router (docs/specs/m0/04-local-accounts.md B2, B15, B18 to B21, B24, T8, T11,
//! T15, T17). The permission suite's cases for the `/api/v1/me/totp` routes and
//! `POST /auth/local/totp` are in `tests/permissions.rs`, and the rate limit walk over every
//! route that documents a 429 in `tests/enrollment.rs`.
//!
//! The tests play the authenticator app with `Secret::code_at_time` at the controllable
//! clock's time. Needs `DATABASE_URL` (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, password, token::Token, totp::Secret};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::Uuid,
    local_logins,
    local_sign_in::{self, LocalMethods, LocalSignIn},
    local_tokens::{self, Origin},
    organizations,
    users::{self, NewUser, Status},
};
use stepclimb_server::{
    AppState,
    client_ip::TrustedProxies,
    rate_limit::{Limits, RateLimiter},
    request_org::TEST_ORG_HEADER,
    router,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const SIGN_IN: &str = "/auth/local/password";
const TOTP: &str = "/auth/local/totp";
const MY_TOTP: &str = "/api/v1/me/totp";
const CONFIRM: &str = "/api/v1/me/totp/confirm";
const DISABLE: &str = "/api/v1/me/totp/disable";
const ENROLL: &str = "/auth/local/enrollment/password";

const EMAIL: &str = "sam@example.test";
const PASSWORD: &str = "correct horse battery staple";
const OTHER_PASSWORD: &str = "tremendous violet umbrella";

/// 2025-11-13T02:13:20Z, 20 seconds into its 30-second step.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

const STEP: Duration = Duration::from_secs(30);

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn client() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000)
}

fn assert_schema(name: &str, value: &Value) {
    let document = serde_json::to_value(stepclimb_server::openapi::document()).expect("JSON");
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

    /// The `name=value` pair of the `Set-Cookie` that sets `name`.
    fn cookie(&self, name: &str) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ASCII").to_owned())
            .filter(|v| v.starts_with(&format!("{name}=")) && !v.contains("Max-Age=0"))
            .map(|v| v.split(';').next().expect("a pair").to_owned())
            .next()
    }

    fn clears(&self, name: &str) -> bool {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.starts_with(&format!("{name}=;")) && v.contains("Max-Age=0"))
    }

    fn session(&self) -> Option<String> {
        self.cookie("__Host-sc_session")
    }

    fn flow(&self) -> Option<String> {
        self.cookie("__Host-sc_flow")
    }
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
}

fn state(pool: &PgPool, clock: &ManualClock, limits: Limits) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(limits, TrustedProxies::none()),
        passwords: Default::default(),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// A user with a password, signed in, who turned TOTP on.
struct Enrolled {
    id: Uuid,
    session: String,
    secret: Secret,
    recovery_codes: Vec<String>,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        Self::with_limits(pool, Limits::DEFAULT).await
    }

    async fn with_limits(pool: PgPool, limits: Limits) -> Self {
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app = router(state(&pool, &clock, limits));
        Self {
            pool,
            clock,
            app,
            org,
        }
    }

    async fn user_in(&self, org: Uuid, email: &str) -> Uuid {
        let new_user = NewUser {
            display_name: "Sam Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(email.to_owned()),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id
    }

    async fn with_password(&self, org: Uuid, user: Uuid, password: &str) {
        let hash = password::hash(password).expect("a hash");
        local_logins::set_password(
            &self.pool,
            scope(org),
            user,
            hash.as_str(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("set the password");
    }

    async fn send_in(
        &self,
        org: Uuid,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let mut request = request.body(body).expect("a request");
        request.extensions_mut().insert(ConnectInfo(client()));
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router never fails");
        let (status, headers) = (response.status(), response.headers().clone());
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

    async fn send(
        &self,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        self.send_in(self.org, method, uri, cookie, body).await
    }

    async fn sign_in(&self, password: &str) -> Answer {
        let body = json!({ "email": EMAIL, "password": password, "return_to": "/c/1" });
        self.send(Method::POST, SIGN_IN, None, Some(body)).await
    }

    /// The password step: its flow cookie, for an account with TOTP on.
    async fn password_step(&self) -> String {
        let answer = self.sign_in(PASSWORD).await;
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
        answer.flow().expect("the second step's flow cookie")
    }

    async fn code_step(&self, flow: &str, code: &str) -> Answer {
        let body = json!({ "code": code });
        self.send(Method::POST, TOTP, Some(flow), Some(body)).await
    }

    async fn start(&self, session: &str) -> Answer {
        self.send(Method::POST, MY_TOTP, Some(session), None).await
    }

    async fn confirm(&self, session: &str, code: &str) -> Answer {
        let body = json!({ "code": code });
        self.send(Method::POST, CONFIRM, Some(session), Some(body))
            .await
    }

    async fn disable(&self, session: &str, code: &str) -> Answer {
        let body = json!({ "code": code });
        self.send(Method::POST, DISABLE, Some(session), Some(body))
            .await
    }

    /// Sam, with a password, signed in.
    async fn signed_in(&self) -> (Uuid, String) {
        let sam = self.user_in(self.org, EMAIL).await;
        self.with_password(self.org, sam, PASSWORD).await;
        let session = self.sign_in(PASSWORD).await.session().expect("a session");
        (sam, session)
    }

    /// Sam with TOTP confirmed at the clock's time; the clock then moves to the next step, so
    /// the confirming code's step is past.
    async fn enrolled(&self) -> Enrolled {
        let (id, session) = self.signed_in().await;
        let started = self.start(&session).await;
        assert_eq!(started.status, StatusCode::OK, "{started:?}");
        let secret = Secret::from_base32(started.body["secret"].as_str().expect("the secret"))
            .expect("a secret");
        let confirmed = self
            .confirm(&session, &secret.code_at_time(self.clock.clock().now()))
            .await;
        assert_eq!(confirmed.status, StatusCode::OK, "{confirmed:?}");
        let recovery_codes = confirmed.body["recovery_codes"]
            .as_array()
            .expect("the recovery codes")
            .iter()
            .map(|code| code.as_str().expect("a code").to_owned())
            .collect();
        self.clock.advance(STEP);
        Enrolled {
            id,
            session,
            secret,
            recovery_codes,
        }
    }

    fn code(&self, enrolled: &Enrolled) -> String {
        enrolled.secret.code_at_time(self.clock.clock().now())
    }

    async fn set_methods(&self, methods: LocalMethods) {
        let new = LocalSignIn {
            enabled: true,
            methods,
        };
        local_sign_in::update(&self.pool, scope(self.org), new, now(), Actor::Instance)
            .await
            .expect("set the switches");
    }

    async fn actions(&self) -> Vec<String> {
        audit::list(&self.pool, scope(self.org), 100)
            .await
            .expect("the audit entries")
            .into_iter()
            .map(|entry| entry.action)
            .collect()
    }
}

/// T15 (B19): enrollment answers the secret once with its URI, stores it sealed (neither the
/// bytes nor the base32 text), and starting again replaces it; an unconfirmed secret asks for
/// no code at sign-in (B15).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn enrollment_shows_the_secret_once_and_stores_it_sealed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (sam, session) = f.signed_in().await;
    let first = f.start(&session).await;
    assert_eq!(first.status, StatusCode::OK, "{first:?}");
    assert_schema("TotpEnrollment", &first.body);
    assert_eq!(first.headers[header::CACHE_CONTROL], "no-store");
    let text = first.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(text.len(), 32, "20 bytes in unpadded base32");
    assert_eq!(
        first.body["otpauth_uri"],
        format!(
            "otpauth://totp/Org%20A:sam%40example.test?secret={text}&issuer=Org%20A&algorithm=SHA1&digits=6&period=30"
        )
    );
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT totp_secret FROM local_logins WHERE user_id = $1")
            .bind(sam)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let raw = Secret::from_base32(&text).unwrap();
    let raw = raw.expose_bytes().as_slice();
    assert!(
        !stored.windows(raw.len()).any(|w| w == raw),
        "the raw secret"
    );
    assert!(
        !stored.windows(text.len()).any(|w| w == text.as_bytes()),
        "the base32 secret"
    );

    let second = f.start(&session).await;
    assert_eq!(second.status, StatusCode::OK);
    assert_ne!(second.body["secret"], first.body["secret"]);
    let stale = Secret::from_base32(&text).unwrap();
    let refused = f
        .confirm(&session, &stale.code_at_time(f.clock.clock().now()))
        .await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid"),
        "the replaced secret's code"
    );

    let signed_in = f.sign_in(PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
    assert!(
        signed_in.session().is_some(),
        "an unconfirmed secret asks for no code"
    );
    assert!(!f.actions().await.contains(&"totp.enable".to_owned()));
}

/// B19, Q11: a wrong code confirms nothing; the right one turns TOTP on, answers ten recovery
/// codes once and writes `totp.enable`; after that, starting and confirming are refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn confirming_turns_totp_on_and_answers_the_recovery_codes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, session) = f.signed_in().await;
    let not_started = f.confirm(&session, "123456").await;
    assert_eq!(
        (not_started.status, not_started.code()),
        (StatusCode::CONFLICT, "local.totp_not_started")
    );
    let started = f.start(&session).await;
    let secret = Secret::from_base32(started.body["secret"].as_str().unwrap()).unwrap();
    let wrong = f.confirm(&session, "000000").await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid")
    );
    assert!(f.sign_in(PASSWORD).await.session().is_some(), "still off");

    let confirmed = f
        .confirm(&session, &secret.code_at_time(f.clock.clock().now()))
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{confirmed:?}");
    assert_schema("TotpConfirmed", &confirmed.body);
    assert_eq!(confirmed.headers[header::CACHE_CONTROL], "no-store");
    let codes = confirmed.body["recovery_codes"].as_array().unwrap();
    assert_eq!(codes.len(), 10);
    assert!(f.actions().await.contains(&"totp.enable".to_owned()));
    let hashes: i64 = sqlx::query_scalar("SELECT count(*) FROM totp_recovery_codes")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(hashes, 10);

    for answer in [
        f.start(&session).await,
        f.confirm(&session, &secret.code_at_time(f.clock.clock().now()))
            .await,
    ] {
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::CONFLICT, "local.totp_enabled"),
            "{answer:?}"
        );
    }
}

/// B19: TOTP follows a password; a signed-in user without one cannot start it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_without_a_password_cannot_start_totp(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user_in(f.org, EMAIL).await;
    let started =
        stepclimb_server::session::start(&f.pool, scope(f.org), sam, &HeaderMap::new(), now())
            .await
            .unwrap();
    let cookie = started
        .set_cookie
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let refused = f.start(&cookie).await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::CONFLICT, "local.password_required")
    );
}

/// T11 (B15, B20): with TOTP confirmed, the right password answers `{next: "totp"}` with a
/// flow and no session; the right code then signs in to the flow's `return_to`; the same code
/// again, after another password step, is refused; so is a code from two steps earlier, while
/// one from the step before passes (Q11's window).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_code_after_the_password_signs_in_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let first = f.sign_in(PASSWORD).await;
    assert_eq!(first.status, StatusCode::OK, "{first:?}");
    assert_eq!(first.body, json!({ "next": "totp" }));
    assert_schema("CodeRequired", &first.body);
    assert_schema("PasswordSignedIn", &first.body);
    assert_eq!(first.headers[header::CACHE_CONTROL], "no-store");
    assert!(first.session().is_none(), "no session before the code");
    assert!(first.cookie("__Host-sc_device").is_none());
    let flow = first.flow().expect("a flow cookie");

    let code = f.code(&sam);
    let signed_in = f.code_step(&flow, &code).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
    assert_eq!(signed_in.body, json!({ "return_to": "/c/1" }));
    assert_schema("SignedIn", &signed_in.body);
    assert!(signed_in.session().is_some());
    assert!(signed_in.cookie("__Host-sc_device").is_some());
    assert!(signed_in.clears("__Host-sc_flow"));
    assert_eq!(signed_in.headers[header::CACHE_CONTROL], "no-store");

    let replayed = f.code_step(&f.password_step().await, &code).await;
    assert_eq!(
        (replayed.status, replayed.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid"),
        "the same code in the same step"
    );

    f.clock.advance(3 * STEP);
    let two_back = sam.secret.code_at_time(f.clock.clock().now() - 2 * STEP);
    let old = f.code_step(&f.password_step().await, &two_back).await;
    assert_eq!(
        (old.status, old.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid"),
        "two steps earlier"
    );
    let one_back = sam.secret.code_at_time(f.clock.clock().now() - STEP);
    let drift = f.code_step(&f.password_step().await, &one_back).await;
    assert_eq!(drift.status, StatusCode::OK, "one step of drift: {drift:?}");
    // The step before is used, and so is every earlier one.
    let current = f.code_step(&f.password_step().await, &f.code(&sam)).await;
    assert_eq!(current.status, StatusCode::OK, "{current:?}");
    let earlier = f.code_step(&f.password_step().await, &one_back).await;
    assert_eq!(earlier.code(), "auth.totp_invalid");
}

/// B20, coordinator decision: a wrong code ends the flow, so every guess needs the password
/// again; the flow cookie is cleared.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_wrong_code_ends_the_flow(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    let wrong = f.code_step(&flow, "000000").await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid")
    );
    assert!(wrong.clears("__Host-sc_flow"));
    assert!(wrong.session().is_none());
    let again = f.code_step(&flow, &f.code(&sam)).await;
    assert_eq!(
        (again.status, again.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid")
    );
    let none = f.code_step("__Host-sc_flow=unknown", &f.code(&sam)).await;
    assert_eq!(none.code(), "auth.flow_invalid");
    let fresh = f.code_step(&f.password_step().await, &f.code(&sam)).await;
    assert_eq!(fresh.status, StatusCode::OK, "{fresh:?}");
}

/// Q11: a recovery code stands in for the code once, typed in any case.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_recovery_code_signs_in_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let typed = sam.recovery_codes[0].to_lowercase();
    let first = f.code_step(&f.password_step().await, &typed).await;
    assert_eq!(first.status, StatusCode::OK, "{first:?}");
    let again = f
        .code_step(&f.password_step().await, &sam.recovery_codes[0])
        .await;
    assert_eq!(
        (again.status, again.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid")
    );
    let other = f
        .code_step(&f.password_step().await, &sam.recovery_codes[1])
        .await;
    assert_eq!(other.status, StatusCode::OK, "{other:?}");
}

/// B14, B20: a password changed between the two steps ends the waiting sign-in.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_changed_between_the_steps_ends_the_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    f.with_password(f.org, sam.id, OTHER_PASSWORD).await;
    let refused = f.code_step(&flow, &f.code(&sam)).await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid")
    );
    assert!(refused.session().is_none());
}

/// T8 (B5): a suspended user's right code is refused by admission.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_user_is_refused_at_the_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    users::set_status(
        &f.pool,
        scope(f.org),
        sam.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let refused = f.code_step(&flow, &f.code(&sam)).await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::FORBIDDEN, "auth.suspended")
    );
}

/// B2: while TOTP is switched off, the password signs in alone and the code route answers 404;
/// switched back on, the same secret is asked for again. With the password off, TOTP is off.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn switched_off_totp_asks_for_no_code_and_comes_back(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    f.set_methods(LocalMethods {
        totp: false,
        ..LocalMethods::default()
    })
    .await;
    let off = f.code_step(&flow, &f.code(&sam)).await;
    assert_eq!(
        (off.status, off.code()),
        (StatusCode::NOT_FOUND, "auth.method_disabled")
    );
    let alone = f.sign_in(PASSWORD).await;
    assert!(alone.session().is_some(), "{alone:?}");
    for answer in [
        f.start(&sam.session).await,
        f.disable(&sam.session, &f.code(&sam)).await,
    ] {
        assert_eq!(answer.code(), "auth.method_disabled", "{answer:?}");
    }

    f.set_methods(LocalMethods::default()).await;
    let back = f.sign_in(PASSWORD).await;
    assert_eq!(back.body, json!({ "next": "totp" }));
    let signed_in = f.code_step(&back.flow().unwrap(), &f.code(&sam)).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
}

fn failed_checks(n: u32) -> Limits {
    Limits {
        failed_checks: NonZeroU32::new(n).expect("a nonzero limit"),
        ..Limits::DEFAULT
    }
}

/// B20, B18: wrong codes count against the account's failed-check budget: once it is spent,
/// a right code answers 429, and so does the password.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn wrong_codes_spend_the_account_budget(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(2)).await;
    let sam = f.enrolled().await;
    let held = f.password_step().await;
    let wrong = f.code_step(&f.password_step().await, "000000").await;
    assert_eq!(wrong.code(), "auth.totp_invalid");
    let wrong = f.code_step(&f.password_step().await, "111111").await;
    assert_eq!(wrong.code(), "auth.totp_invalid");

    let limited = f.code_step(&held, &f.code(&sam)).await;
    assert_eq!(
        (limited.status, limited.code()),
        (StatusCode::TOO_MANY_REQUESTS, "auth.rate_limited")
    );
    assert!(limited.headers.contains_key(header::RETRY_AFTER));
    assert!(limited.session().is_none());
    let password = f.sign_in(PASSWORD).await;
    assert_eq!(password.status, StatusCode::TOO_MANY_REQUESTS);
}

/// Q11 (B21): disabling needs a current code or a recovery code; afterwards the password signs
/// in alone and `totp.disable` is written; there is then nothing to disable.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn disabling_needs_a_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let wrong = f.disable(&sam.session, "000000").await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "auth.totp_invalid")
    );
    let disabled = f.disable(&sam.session, &f.code(&sam)).await;
    assert_eq!(disabled.status, StatusCode::NO_CONTENT, "{disabled:?}");
    assert!(f.actions().await.contains(&"totp.disable".to_owned()));
    assert!(f.sign_in(PASSWORD).await.session().is_some());
    let again = f.disable(&sam.session, &f.code(&sam)).await;
    assert_eq!(
        (again.status, again.code()),
        (StatusCode::CONFLICT, "local.totp_not_enabled")
    );
}

/// Q11: a recovery code disables TOTP too.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_recovery_code_disables_totp(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let disabled = f.disable(&sam.session, &sam.recovery_codes[3]).await;
    assert_eq!(disabled.status, StatusCode::NO_CONTENT, "{disabled:?}");
    assert!(f.sign_in(PASSWORD).await.session().is_some());
}

/// The session-only rule: a bearer request to a `/api/v1/me/totp` route is refused 403
/// `auth.session_required`, with the session cookie beside it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bearer_request_cannot_manage_totp(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, session) = f.signed_in().await;
    for path in [MY_TOTP, CONFIRM, DISABLE] {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(TEST_ORG_HEADER, f.org.to_string())
            .header(header::AUTHORIZATION, "Bearer sc_not_a_key")
            .header(header::COOKIE, &session)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "code": "123456" }).to_string()))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(client()));
        let response = f.app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "auth.session_required", "{path}");
    }
    let secret: Option<Vec<u8>> = sqlx::query_scalar("SELECT totp_secret FROM local_logins")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert!(secret.is_none());
}

/// Cross-organization: organization B's second-step flow does not finish in organization A,
/// whose flows are looked up in A only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_flow_is_unknown(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .unwrap()
        .id;
    let flow = f.password_step().await;
    let body = json!({ "code": f.code(&sam) });
    let elsewhere = f
        .send_in(b, Method::POST, TOTP, Some(&flow), Some(body))
        .await;
    assert_eq!(
        (elsewhere.status, elsewhere.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid")
    );
}

/// T17 (B24): the TOTP routes log no secret, URI, code or recovery code, even with every
/// target at `trace`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_totp_routes_log_no_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    let code = f.code(&sam);
    let signed_in = f.code_step(&flow, &code).await;
    let recovered = f
        .code_step(&f.password_step().await, &sam.recovery_codes[0])
        .await;
    f.clock.advance(STEP);
    let disable_code = f.code(&sam);
    let disabled = f.disable(&sam.session, &disable_code).await;
    drop(guard);
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_eq!(recovered.status, StatusCode::OK);
    assert_eq!(disabled.status, StatusCode::NO_CONTENT);

    let text = log.text();
    assert!(text.contains("TOTP sign-in"), "the capture missed the log");
    // Long secrets cannot appear by chance: the whole log is searched for them.
    let mut secrets = vec![
        sam.secret.expose_base32(),
        PASSWORD.to_owned(),
        flow.split_once('=').expect("a pair").1.to_owned(),
    ];
    secrets.extend(sam.recovery_codes.iter().cloned());
    secrets.extend(sam.recovery_codes.iter().map(|c| c.replace('-', "")));
    for secret in &secrets {
        assert!(!text.contains(secret.as_str()), "the log holds {secret:?}");
    }
    // Six digits could appear by chance inside a timestamp or an id, so the codes are looked
    // for as whole values: every field of every event but its timestamp, as a token with no
    // letter or digit beside it.
    let codes = [sam.secret.code_at_time(now()), code, disable_code];
    for line in text.lines() {
        let event: Value = serde_json::from_str(line).expect("a JSON log line");
        let mut values = Vec::new();
        field_values(&event, &mut values);
        for code in &codes {
            let leaked = values.iter().find(|value| holds_token(value, code));
            assert!(leaked.is_none(), "the log holds the code {code:?}: {line}");
        }
    }
}

/// Every string and number in `value`, but under a `timestamp` key, as text.
fn field_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(members) => members
            .iter()
            .filter(|(key, _)| key.as_str() != "timestamp")
            .for_each(|(_, member)| field_values(member, out)),
        Value::Array(items) => items.iter().for_each(|item| field_values(item, out)),
        Value::String(text) => out.push(text.clone()),
        Value::Number(number) => out.push(number.to_string()),
        Value::Bool(_) | Value::Null => {}
    }
}

/// Whether `token` appears in `text` with no letter or digit right before or after it.
fn holds_token(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + token.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

#[test]
fn a_code_is_found_only_as_a_whole_token() {
    assert!(holds_token("123456", "123456"));
    assert!(holds_token("code=123456 sent", "123456"));
    assert!(!holds_token("2025-11-13T02:13:20.1234567Z", "123456"));
    assert!(!holds_token(
        "01900000-0000-7000-8000-a123456b0000",
        "123456"
    ));
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("the log buffer").clone()).expect("UTF-8")
    }
}

impl Write for Captured {
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

/// Decision 7 of the PR (B9, B19): a password reset through an enrollment link signs in without
/// a code, TOTP stays confirmed, and the next password sign-in asks for the code again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrollment_link_reset_keeps_totp(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let token = Token::generate().expect("a token");
    local_tokens::create_enrollment(
        &f.pool,
        scope(f.org),
        sam.id,
        token.hash().as_bytes(),
        Origin::Bootstrap,
        f.clock.clock().now(),
        Actor::Instance,
    )
    .await
    .expect("create a link");
    let body = json!({ "token": token.expose_secret(), "password": OTHER_PASSWORD });
    let reset = f.send(Method::POST, ENROLL, None, Some(body)).await;
    assert_eq!(reset.status, StatusCode::OK, "{reset:?}");
    assert_eq!(reset.body, json!({ "return_to": "/" }));
    assert!(
        reset.session().is_some(),
        "the link signs in without a code"
    );
    let confirmed: bool = sqlx::query_scalar(
        "SELECT totp_confirmed_at IS NOT NULL FROM local_logins WHERE user_id = $1",
    )
    .bind(sam.id)
    .fetch_one(&f.pool)
    .await
    .expect("the login");
    assert!(confirmed, "TOTP stays confirmed");

    let next = f.sign_in(OTHER_PASSWORD).await;
    assert_eq!(next.body, json!({ "next": "totp" }), "{next:?}");
    assert!(next.session().is_none());
    let signed_in = f
        .code_step(&next.flow().expect("a flow"), &f.code(&sam))
        .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
}

/// B20: TOTP disabled between the two steps ends the waiting sign-in with 400
/// `auth.flow_invalid`, not a code refusal.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn totp_disabled_between_the_steps_ends_the_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let flow = f.password_step().await;
    let disabled = f.disable(&sam.session, &f.code(&sam)).await;
    assert_eq!(disabled.status, StatusCode::NO_CONTENT, "{disabled:?}");
    f.clock.advance(STEP);
    let refused = f.code_step(&flow, &f.code(&sam)).await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{refused:?}"
    );
    assert!(refused.session().is_none());
}

/// Waits until `n` sessions of this test's database wait on a row or transaction lock.
async fn until_waiting(pool: &PgPool, n: i64) {
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{n} sessions did not wait on a lock within 5 s");
}

/// B20, RFC 6238 section 5.2: the same code sent twice at once, from two password steps, signs
/// in once. The test holds the login's row lock until both requests wait, so they reach the
/// check together; the other is refused with 401 `auth.totp_invalid`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_same_code_sent_twice_at_once_signs_in_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.enrolled().await;
    let (first, second) = (f.password_step().await, f.password_step().await);
    let code = f.code(&sam);
    let mut holder = f.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM local_logins WHERE user_id = $1 FOR UPDATE")
        .bind(sam.id)
        .execute(&mut *holder)
        .await
        .expect("lock the login");
    let both = async { tokio::join!(f.code_step(&first, &code), f.code_step(&second, &code)) };
    let release = async {
        until_waiting(&f.pool, 2).await;
        holder.commit().await.expect("release the lock");
    };
    let ((one, two), ()) = tokio::join!(both, release);
    let mut outcomes = [
        (one.status, one.code().to_owned()),
        (two.status, two.code().to_owned()),
    ];
    outcomes.sort();
    assert_eq!(
        outcomes,
        [
            (StatusCode::OK, String::new()),
            (StatusCode::UNAUTHORIZED, "auth.totp_invalid".to_owned()),
        ],
        "{one:?} {two:?}"
    );
}
