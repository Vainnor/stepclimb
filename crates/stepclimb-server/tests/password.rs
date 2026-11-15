// SPDX-License-Identifier: AGPL-3.0-or-later
//! Passwords over the router (docs/specs/m0/04-local-accounts.md B2, B5, B9, B14, B16 to B18,
//! B24, T5, T8, T10, T12 to T14, T19; docs/specs/m0/08-sessions-and-csrf.md B40 to B42). The
//! permission suite's cases for `PUT /api/v1/me/password` are in `tests/permissions.rs`, and the
//! rate limit walk over every `/auth/` route in `tests/enrollment.rs`.
//!
//! The requests name their organization with the `test-support` header of `request_org`, so a
//! test can hold two. Needs `DATABASE_URL` (`just up`).

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
use stepclimb_auth::{
    encryption::KeyRing,
    password::{self, StoredHash},
    token::{Token, TokenHash},
};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::Uuid,
    identity_providers, local_logins,
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
const ENROLL: &str = "/auth/local/enrollment/password";
const MY_PASSWORD: &str = "/api/v1/me/password";

const PASSWORD: &str = "correct horse battery staple";
const OTHER_PASSWORD: &str = "tremendous violet umbrella";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn client(last: u8) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, last)), 50_000)
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

    fn session(&self) -> Option<String> {
        self.cookie("__Host-sc_session")
    }

    fn device(&self) -> Option<String> {
        self.cookie("__Host-sc_device")
    }
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    state: AppState,
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
        let state = state(&pool, &clock, limits);
        let app = router(state.clone());
        Self {
            pool,
            clock,
            state,
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

    async fn user(&self, email: &str) -> Uuid {
        self.user_in(self.org, email).await
    }

    /// An enrollment link for `user` of `org`, made by the instance: its token.
    async fn link_in(&self, org: Uuid, user: Uuid) -> String {
        let token = Token::generate().expect("a token");
        local_tokens::create_enrollment(
            &self.pool,
            scope(org),
            user,
            token.hash().as_bytes(),
            Origin::Bootstrap,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create a link");
        token.expose_secret().to_owned()
    }

    async fn link(&self, user: Uuid) -> String {
        self.link_in(self.org, user).await
    }

    /// `user` of `org` with `password` stored, hashed as the server hashes.
    async fn with_password_in(&self, org: Uuid, user: Uuid, password: &str) {
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

    async fn with_password(&self, user: Uuid, password: &str) {
        self.with_password_in(self.org, user, password).await;
    }

    async fn send_from(
        &self,
        from: SocketAddr,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: Option<&str>,
        body: Value,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let mut request = request
            .body(Body::from(body.to_string()))
            .expect("a request");
        request.extensions_mut().insert(ConnectInfo(from));
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

    async fn sign_in_from(
        &self,
        from: SocketAddr,
        cookie: Option<&str>,
        email: &str,
        password: &str,
    ) -> Answer {
        let body = json!({ "email": email, "password": password });
        self.send_from(from, Method::POST, SIGN_IN, self.org, cookie, body)
            .await
    }

    async fn sign_in(&self, email: &str, password: &str) -> Answer {
        self.sign_in_from(client(10), None, email, password).await
    }

    async fn enroll_in(&self, org: Uuid, token: &str, password: &str) -> Answer {
        let body = json!({ "token": token, "password": password });
        self.send_from(client(10), Method::POST, ENROLL, org, None, body)
            .await
    }

    async fn enroll(&self, token: &str, password: &str) -> Answer {
        self.enroll_in(self.org, token, password).await
    }

    async fn change(&self, cookie: &str, current: &str, new: &str) -> Answer {
        let body = json!({ "current_password": current, "new_password": new });
        self.send_from(
            client(10),
            Method::PUT,
            MY_PASSWORD,
            self.org,
            Some(cookie),
            body,
        )
        .await
    }

    async fn link_used(&self, token: &str) -> bool {
        sqlx::query_scalar("SELECT used_at IS NOT NULL FROM local_tokens WHERE token_hash = $1")
            .bind(TokenHash::of(token).as_bytes().as_slice())
            .fetch_one(&self.pool)
            .await
            .expect("the link")
    }

    async fn stored_hash(&self, user: Uuid) -> Option<String> {
        local_logins::get(&self.pool, scope(self.org), user)
            .await
            .expect("read the login")
            .and_then(|login| login.password_hash)
    }

    async fn sessions_of(&self, user: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
            .bind(user)
            .fetch_one(&self.pool)
            .await
            .expect("count sessions")
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

    async fn suspend(&self, user: Uuid) {
        users::set_status(
            &self.pool,
            scope(self.org),
            user,
            Status::Suspended,
            now(),
            Actor::Instance,
        )
        .await
        .expect("suspend");
    }

    fn computations(&self) -> u64 {
        self.state.passwords.computations()
    }
}

/// T5 (B9, B5, B3): setting a password through a link signs in, uses the link, links the local
/// identity, stores an argon2id hash, writes `password.set` and sets the device cookie; the
/// password then signs in.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_set_through_a_link_signs_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let token = f.link(sam).await;

    let answer = f.enroll(&token, PASSWORD).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_schema("SignedIn", &answer.body);
    assert_eq!(answer.body, json!({ "return_to": "/" }));
    assert!(answer.session().is_some(), "{answer:?}");
    assert!(answer.device().is_some(), "{answer:?}");
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    assert!(f.link_used(&token).await);
    let stored = f.stored_hash(sam).await.expect("a hash");
    assert!(
        stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{stored}"
    );

    let local = identity_providers::get_by_key(&f.pool, scope(f.org), "local")
        .await
        .unwrap()
        .expect("the local row");
    let subject = sam.to_string();
    let identity = stepclimb_core::identities::find_user(&f.pool, scope(f.org), local.id, &subject)
        .await
        .unwrap();
    assert_eq!(identity.map(|linked| linked.user.id), Some(sam));
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert!(actions.contains(&"password.set"), "{actions:?}");
    assert!(actions.contains(&"identity.link"), "{actions:?}");

    let again = f.enroll(&token, OTHER_PASSWORD).await;
    assert_eq!(
        (again.status, again.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );
    let signed_in = f.sign_in("SAM@example.test", PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
}

/// T5 (Q8): a password failing the policy answers 422 `local.password_weak` at `/password`,
/// and the link still works.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_weak_password_is_refused_and_the_link_stays_usable(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let token = f.link(sam).await;
    for weak in ["short", "QWERTY123456", &"x".repeat(257)] {
        let answer = f.enroll(&token, weak).await;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{answer:?}"
        );
        assert_eq!(answer.code(), "request.invalid");
        assert_eq!(answer.body["errors"][0]["field"], "/password");
        assert_eq!(answer.body["errors"][0]["code"], "local.password_weak");
    }
    assert_eq!(f.computations(), 0, "no hash for a refused password");
    assert!(!f.link_used(&token).await);
    assert_eq!(f.stored_hash(sam).await, None);
    assert_eq!(f.enroll(&token, PASSWORD).await.status, StatusCode::OK);
}

/// B7, T19: an unknown, expired or other organization's link sets nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bad_link_sets_no_password(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let unknown = f.enroll("not-a-token", PASSWORD).await;
    assert_eq!(
        (unknown.status, unknown.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );

    let b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .unwrap()
        .id;
    let pat = f.user_in(b, "pat@example.test").await;
    let token_b = f.link_in(b, pat).await;
    let crossed = f.enroll(&token_b, PASSWORD).await;
    assert_eq!(
        (crossed.status, crossed.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );
    assert!(!f.link_used(&token_b).await);

    let token = f.link(sam).await;
    f.clock.advance(local_tokens::ENROLLMENT_LIFETIME);
    let expired = f.enroll(&token, PASSWORD).await;
    assert_eq!(
        (expired.status, expired.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_expired")
    );
    assert_eq!(f.stored_hash(sam).await, None);
}

/// T8, B5: a suspended user's link and right password are refused with 403 `auth.suspended`;
/// the link stays usable and nothing is stored.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_user_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let token = f.link(sam).await;
    f.suspend(sam).await;
    let enrolled = f.enroll(&token, PASSWORD).await;
    assert_eq!(
        (enrolled.status, enrolled.code()),
        (StatusCode::FORBIDDEN, "auth.suspended")
    );
    assert!(enrolled.session().is_none());
    assert!(!f.link_used(&token).await);
    assert_eq!(f.stored_hash(sam).await, None);

    let pat = f.user("pat@example.test").await;
    f.with_password(pat, PASSWORD).await;
    f.suspend(pat).await;
    let signed_in = f.sign_in("pat@example.test", PASSWORD).await;
    assert_eq!(
        (signed_in.status, signed_in.code()),
        (StatusCode::FORBIDDEN, "auth.suspended")
    );
    assert!(signed_in.session().is_none());
    assert_eq!(f.sessions_of(pat).await, 0);
}

/// B14, B5: the right password signs in with a session, the device cookie and the sanitized
/// `return_to`, in any letter case of the email.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_right_password_signs_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("Sam@Example.test").await;
    f.with_password(sam, PASSWORD).await;
    let body =
        json!({ "email": " sam@EXAMPLE.test ", "password": PASSWORD, "return_to": "/courses/7" });
    let answer = f
        .send_from(client(10), Method::POST, SIGN_IN, f.org, None, body)
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_schema("SignedIn", &answer.body);
    assert_eq!(answer.body, json!({ "return_to": "/courses/7" }));
    assert!(answer.session().is_some());
    assert!(answer.device().is_some());
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(f.sessions_of(sam).await, 1);

    let body = json!({ "email": "sam@example.test", "password": PASSWORD, "return_to": "https://evil.example/" });
    let answer = f
        .send_from(client(10), Method::POST, SIGN_IN, f.org, None, body)
        .await;
    assert_eq!(answer.body, json!({ "return_to": "/" }));
}

/// T10, T19 (B14): a wrong password, an unknown email, a user without a password and another
/// organization's user all answer the same 401 after exactly one argon2id verification.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_failure_is_the_same_401_after_one_verification(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    f.user("nopassword@example.test").await;
    let b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .unwrap()
        .id;
    let pat = f.user_in(b, "pat@example.test").await;
    f.with_password_in(b, pat, PASSWORD).await;

    let cases = [
        ("sam@example.test", OTHER_PASSWORD),
        ("nobody@example.test", PASSWORD),
        ("nopassword@example.test", PASSWORD),
        ("pat@example.test", PASSWORD),
        ("sam\u{0}@example.test", PASSWORD),
    ];
    let mut bodies = Vec::new();
    for (email, password) in cases {
        let before = f.computations();
        let answer = f.sign_in(email, password).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::UNAUTHORIZED, "auth.credentials_invalid"),
            "{email:?}"
        );
        assert!(
            answer.session().is_none() && answer.device().is_none(),
            "{email:?}"
        );
        assert_eq!(
            f.computations() - before,
            1,
            "one verification for {email:?}"
        );
        bodies.push(answer.body);
    }
    assert!(
        bodies.windows(2).all(|pair| pair[0] == pair[1]),
        "{bodies:#?}"
    );
    assert_eq!(f.sessions_of(pat).await, 0);
}

/// B2: with passwords switched off every password route answers 404 `auth.method_disabled`,
/// and the stored password works again once they are back on.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn switched_off_passwords_answer_404(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let token = f.link(f.user("pat@example.test").await).await;
    f.set_methods(LocalMethods {
        password: false,
        ..LocalMethods::default()
    })
    .await;

    let signed_in = f.sign_in("sam@example.test", PASSWORD).await;
    assert_eq!(
        (signed_in.status, signed_in.code()),
        (StatusCode::NOT_FOUND, "auth.method_disabled")
    );
    let enrolled = f.enroll(&token, PASSWORD).await;
    assert_eq!(
        (enrolled.status, enrolled.code()),
        (StatusCode::NOT_FOUND, "auth.method_disabled")
    );
    assert!(!f.link_used(&token).await);
    assert_eq!(f.computations(), 0);

    f.set_methods(LocalMethods::default()).await;
    assert_eq!(
        f.sign_in("sam@example.test", PASSWORD).await.status,
        StatusCode::OK
    );
}

/// T12 (B16): a hash stored with weaker parameters is replaced after a successful sign-in by
/// one at the current parameters, and the password still signs in.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_older_hash_is_replaced_after_a_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let weak = StoredHash::new(WEAK_HASH.to_owned());
    local_logins::set_password(
        &f.pool,
        scope(f.org),
        sam,
        weak.as_str(),
        now(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let failed = f.sign_in("sam@example.test", OTHER_PASSWORD).await;
    assert_eq!(failed.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.stored_hash(sam).await.as_deref(),
        Some(weak.as_str()),
        "a failure rehashes nothing"
    );

    assert_eq!(
        f.sign_in("sam@example.test", PASSWORD).await.status,
        StatusCode::OK
    );
    let stored = f.stored_hash(sam).await.expect("a hash");
    assert_ne!(stored, weak.as_str());
    assert!(
        stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{stored}"
    );
    assert_eq!(
        f.sign_in("sam@example.test", PASSWORD).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.stored_hash(sam).await.as_deref(),
        Some(stored.as_str()),
        "a current hash stays"
    );
}

/// `PASSWORD` hashed with argon2id at 8 MiB and one pass, as an older release might have
/// stored it.
const WEAK_HASH: &str = "$argon2id$v=19$m=8192,t=1,p=1$PLOmvCyqPjO0r5ZEN68Mxw$94vG4dYRzz8jAKq7/rZP7e39BOQPA6ANFHM5UNeHPdI";

/// T13 (B17, Q8): a change needs the current password; afterwards the old one stops working,
/// the new one signs in, and every other session of the user has ended but the caller's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_the_password_needs_the_current_one_and_ends_other_sessions(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let mine = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    let other = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    assert_eq!(f.sessions_of(sam).await, 2);

    let wrong = f
        .change(&mine, OTHER_PASSWORD, "another long passphrase")
        .await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (StatusCode::UNAUTHORIZED, "auth.credentials_invalid")
    );
    let weak = f.change(&mine, PASSWORD, "too short").await;
    assert_eq!(weak.status, StatusCode::UNPROCESSABLE_ENTITY, "{weak:?}");
    assert_eq!(weak.body["errors"][0]["field"], "/new_password");
    assert_eq!(f.sessions_of(sam).await, 2, "a refused change ends nothing");

    let changed = f.change(&mine, PASSWORD, OTHER_PASSWORD).await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{changed:?}");
    assert_eq!(f.sessions_of(sam).await, 1);
    let me = f
        .send_from(
            client(10),
            Method::GET,
            "/api/v1/me",
            f.org,
            Some(&mine),
            Value::Null,
        )
        .await;
    assert_eq!(me.status, StatusCode::OK, "the caller's session stays");
    let gone = f
        .send_from(
            client(10),
            Method::GET,
            "/api/v1/me",
            f.org,
            Some(&other),
            Value::Null,
        )
        .await;
    assert_eq!(
        gone.status,
        StatusCode::UNAUTHORIZED,
        "the other session ended"
    );

    assert_eq!(
        f.sign_in("sam@example.test", PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.sign_in("sam@example.test", OTHER_PASSWORD).await.status,
        StatusCode::OK
    );
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    let sets: Vec<_> = entries
        .iter()
        .filter(|e| e.action == "password.set")
        .collect();
    assert_eq!(sets.len(), 2);
    assert_eq!(
        sets[1].actor,
        Actor::User {
            id: sam,
            ip: client(10).ip()
        }
    );
}

/// Permissions: the change takes a browser session only; a bearer request is 403
/// `auth.session_required` before anything else.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bearer_request_cannot_change_a_password(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let mine = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    let mut request = Request::builder()
        .method(Method::PUT)
        .uri(MY_PASSWORD)
        .header(TEST_ORG_HEADER, f.org.to_string())
        .header(header::AUTHORIZATION, "Bearer sc_not_a_key")
        .header(header::COOKIE, &mine)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "current_password": PASSWORD, "new_password": OTHER_PASSWORD }).to_string(),
        ))
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(client(10)));
    let response = f.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "auth.session_required");
    assert_eq!(
        f.sign_in("sam@example.test", PASSWORD).await.status,
        StatusCode::OK
    );
}

fn failed_checks(n: u32) -> Limits {
    Limits {
        failed_checks: NonZeroU32::new(n).expect("a nonzero limit"),
        ..Limits::DEFAULT
    }
}

/// T14 (B18, spec 08 B40, B41): the eleventh failed check in the window answers 429 for the
/// account from a new address, in any letter case of the email, without a verification, and
/// for the right password too; another account is untouched; the window closing restores it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_eleventh_failure_is_refused_for_the_account_from_any_address(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let grace = f.user("grace@example.test").await;
    f.with_password(grace, PASSWORD).await;
    for n in 0..10u8 {
        let email = if n % 2 == 0 {
            "ada@example.test"
        } else {
            "ADA@Example.test"
        };
        let answer = f
            .sign_in_from(client(20 + n), None, email, OTHER_PASSWORD)
            .await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "failure {n}");
    }
    let before = f.computations();
    let refused = f
        .sign_in_from(client(99), None, "Ada@example.test", PASSWORD)
        .await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::TOO_MANY_REQUESTS, "auth.rate_limited")
    );
    assert_eq!(refused.headers[header::RETRY_AFTER], "900");
    assert_eq!(f.computations(), before, "a spent budget runs no check");
    assert_eq!(
        f.sign_in_from(client(99), None, "grace@example.test", PASSWORD)
            .await
            .status,
        StatusCode::OK
    );

    f.clock.advance(Duration::from_secs(15 * 60));
    assert_eq!(
        f.sign_in_from(client(99), None, "ada@example.test", PASSWORD)
            .await
            .status,
        StatusCode::OK
    );
}

/// Spec 08 B42 (the owner's device-cookie decision): a browser that signed in to the account
/// before is neither refused by nor counted against its spent budget, a device cookie of
/// another account exempts nothing, and the per-address budget still applies.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_device_cookie_for_the_account_skips_its_budget(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(2)).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let grace = f.user("grace@example.test").await;
    f.with_password(grace, PASSWORD).await;
    let ada_device = f
        .sign_in("ada@example.test", PASSWORD)
        .await
        .device()
        .unwrap();
    let grace_device = f
        .sign_in("grace@example.test", PASSWORD)
        .await
        .device()
        .unwrap();

    for _ in 0..2 {
        f.sign_in_from(client(30), None, "ada@example.test", OTHER_PASSWORD)
            .await;
    }
    let attacker = f
        .sign_in_from(client(31), None, "ada@example.test", PASSWORD)
        .await;
    assert_eq!(attacker.status, StatusCode::TOO_MANY_REQUESTS);
    let wrong_device = f
        .sign_in_from(
            client(31),
            Some(&grace_device),
            "ada@example.test",
            PASSWORD,
        )
        .await;
    assert_eq!(
        wrong_device.status,
        StatusCode::TOO_MANY_REQUESTS,
        "another account's cookie"
    );

    // The owner's browser: a failure counts against the cookie's own budget, not the spent
    // account's, and the right password signs in.
    let typo = f
        .sign_in_from(
            client(32),
            Some(&ada_device),
            "ada@example.test",
            OTHER_PASSWORD,
        )
        .await;
    assert_eq!(
        (typo.status, typo.code()),
        (StatusCode::UNAUTHORIZED, "auth.credentials_invalid")
    );
    let owner = f
        .sign_in_from(client(32), Some(&ada_device), "ada@example.test", PASSWORD)
        .await;
    assert_eq!(owner.status, StatusCode::OK, "{owner:?}");
}

/// Spec 08 B42: with the device cookie a client still hits its per-address budget.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_device_cookie_does_not_lift_the_address_budget(pool: PgPool) {
    let limits = Limits {
        sign_in: NonZeroU32::new(2).unwrap(),
        ..Limits::DEFAULT
    };
    let f = Fixture::with_limits(pool, limits).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let device = f
        .sign_in_from(client(40), None, "ada@example.test", PASSWORD)
        .await
        .device()
        .unwrap();
    assert_eq!(
        f.sign_in_from(client(40), Some(&device), "ada@example.test", PASSWORD)
            .await
            .status,
        StatusCode::OK
    );
    let third = f
        .sign_in_from(client(40), Some(&device), "ada@example.test", PASSWORD)
        .await;
    assert_eq!(third.status, StatusCode::TOO_MANY_REQUESTS);
}

/// A device cookie older than its lifetime is not an exemption.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_expired_device_cookie_exempts_nothing(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(1)).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let device = f
        .sign_in("ada@example.test", PASSWORD)
        .await
        .device()
        .unwrap();
    f.clock.advance(stepclimb_server::device::LIFETIME);
    f.sign_in_from(client(50), None, "ada@example.test", OTHER_PASSWORD)
        .await;
    let refused = f
        .sign_in_from(client(51), Some(&device), "ada@example.test", PASSWORD)
        .await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
}

/// B18: a wrong current password counts against the account's budget too.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_wrong_current_password_counts_against_the_account(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(1)).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let body = json!({ "email": "ada@example.test", "password": PASSWORD });
    let signed_in = f
        .send_from(client(60), Method::POST, SIGN_IN, f.org, None, body)
        .await;
    let session = signed_in.session().unwrap();
    let wrong = f
        .change(&session, OTHER_PASSWORD, "another long passphrase")
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let refused = f
        .sign_in_from(client(61), None, "ada@example.test", PASSWORD)
        .await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
}

/// B24, spec 08 B4: password sign-in, enrollment and change log no password, hash, email or
/// token, even with every target at `trace`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_password_routes_log_no_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let token = f.link(sam).await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let enrolled = f.enroll(&token, PASSWORD).await;
    let failed = f.sign_in("sam@example.test", "wrong but long enough").await;
    let session = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    let changed = f.change(&session, PASSWORD, OTHER_PASSWORD).await;
    drop(guard);
    assert_eq!(enrolled.status, StatusCode::OK);
    assert_eq!(failed.status, StatusCode::UNAUTHORIZED);
    assert_eq!(changed.status, StatusCode::NO_CONTENT);

    let text = log.text();
    assert!(
        text.contains("password check failed"),
        "the capture missed the log"
    );
    let hash = f.stored_hash(sam).await.unwrap();
    let secrets = [
        PASSWORD.to_owned(),
        OTHER_PASSWORD.to_owned(),
        "wrong but long enough".to_owned(),
        "sam@example.test".to_owned(),
        token,
        hash,
        session.split_once('=').unwrap().1.to_owned(),
    ];
    for secret in secrets {
        assert!(!text.contains(&secret), "the log holds {secret:?}");
    }
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

/// Waits until a session of this test's database waits on a row or transaction lock.
async fn until_a_call_waits_on_a_lock(pool: &PgPool) {
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no session waited on a lock within 5 s");
}

/// B14, B17: a sign-in whose password changes after it was verified, before the sign-in's
/// transaction takes the user's lock, is refused, so no session outlives the change that ended
/// the others. The test holds the user's row lock, lets the sign-in verify and wait on it,
/// stores another password and commits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_changed_during_the_check_does_not_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let mut tx = f.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR NO KEY UPDATE")
        .bind(sam)
        .execute(&mut *tx)
        .await
        .expect("lock the user");
    let signing_in = f.sign_in("sam@example.test", PASSWORD);
    let change = async {
        until_a_call_waits_on_a_lock(&f.pool).await;
        let hash = password::hash(OTHER_PASSWORD).expect("a hash");
        local_logins::set_password(
            &mut *tx,
            scope(f.org),
            sam,
            hash.as_str(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("change the password");
        tx.commit().await.expect("commit");
    };
    let (answer, ()) = tokio::join!(signing_in, change);
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.credentials_invalid"),
        "{answer:?}"
    );
    assert_eq!(f.sessions_of(sam).await, 0);
}

/// Spec 08 B42 (coordinator decision): a device cookie has a failed-check budget of its own, so
/// a stolen cookie gives no more guesses than the account had; the account's own budget is not
/// touched by them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_device_cookie_has_a_budget_of_its_own(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(2)).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let device = f
        .sign_in("ada@example.test", PASSWORD)
        .await
        .device()
        .unwrap();
    for n in 0..2 {
        let wrong = f
            .sign_in_from(
                client(70 + n),
                Some(&device),
                "ada@example.test",
                OTHER_PASSWORD,
            )
            .await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    }
    let stolen = f
        .sign_in_from(client(79), Some(&device), "ada@example.test", PASSWORD)
        .await;
    assert_eq!(
        (stolen.status, stolen.code()),
        (StatusCode::TOO_MANY_REQUESTS, "auth.rate_limited")
    );
    // The account's budget was not spent by the cookie's failures.
    let without = f
        .sign_in_from(client(80), None, "ada@example.test", PASSWORD)
        .await;
    assert_eq!(without.status, StatusCode::OK, "{without:?}");
}

/// Spec 08 B42 (coordinator decision): a device cookie is good for the password it was issued
/// with; a change or a reset through an enrollment link revokes it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_password_revokes_older_device_cookies(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(1)).await;
    let ada = f.user("ada@example.test").await;
    f.with_password(ada, PASSWORD).await;
    let signed_in = f
        .sign_in_from(client(81), None, "ada@example.test", PASSWORD)
        .await;
    let (session, old_device) = (signed_in.session().unwrap(), signed_in.device().unwrap());
    assert_eq!(
        f.change(&session, PASSWORD, OTHER_PASSWORD).await.status,
        StatusCode::NO_CONTENT
    );
    // Spend the account's budget; the cookie from before the change no longer exempts.
    f.sign_in_from(client(82), None, "ada@example.test", PASSWORD)
        .await;
    let old = f
        .sign_in_from(
            client(83),
            Some(&old_device),
            "ada@example.test",
            OTHER_PASSWORD,
        )
        .await;
    assert_eq!(old.status, StatusCode::TOO_MANY_REQUESTS, "{old:?}");

    // A reset through a link moves the generation too.
    f.clock.advance(Duration::from_secs(15 * 60));
    let fresh = f
        .sign_in_from(client(84), None, "ada@example.test", OTHER_PASSWORD)
        .await
        .device()
        .unwrap();
    let token = f.link(ada).await;
    assert_eq!(
        f.enroll(&token, "a third long passphrase").await.status,
        StatusCode::OK
    );
    f.sign_in_from(client(85), None, "ada@example.test", OTHER_PASSWORD)
        .await;
    let reset = f
        .sign_in_from(
            client(86),
            Some(&fresh),
            "ada@example.test",
            "a third long passphrase",
        )
        .await;
    assert_eq!(reset.status, StatusCode::TOO_MANY_REQUESTS, "{reset:?}");
}

/// B9 (coordinator decision): a password set through an enrollment link is a reset, so every
/// other session of the user ends in the enrollment's transaction; the one it creates stays.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_reset_through_a_link_ends_every_other_session(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let before = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    f.sign_in("sam@example.test", PASSWORD).await;
    let pat = f.user("pat@example.test").await;
    f.with_password(pat, PASSWORD).await;
    f.sign_in("pat@example.test", PASSWORD).await;
    assert_eq!(f.sessions_of(sam).await, 2);

    let token = f.link(sam).await;
    let reset = f.enroll(&token, OTHER_PASSWORD).await;
    assert_eq!(reset.status, StatusCode::OK, "{reset:?}");
    assert_eq!(f.sessions_of(sam).await, 1);
    assert_eq!(f.sessions_of(pat).await, 1, "another user's session stays");
    let new = reset.session().unwrap();
    let me = |cookie: String| {
        let f = &f;
        async move {
            f.send_from(
                client(10),
                Method::GET,
                "/api/v1/me",
                f.org,
                Some(&cookie),
                Value::Null,
            )
            .await
            .status
        }
    };
    assert_eq!(me(new).await, StatusCode::OK);
    assert_eq!(me(before).await, StatusCode::UNAUTHORIZED);
}

/// B17, B18 (coordinator decision): a change of password has a budget per user that holds with
/// no email and with the user's own device cookie, so neither means no limit.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_is_limited_per_user_without_an_email_and_with_a_device_cookie(pool: PgPool) {
    let f = Fixture::with_limits(pool, failed_checks(2)).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let signed_in = f.sign_in("sam@example.test", PASSWORD).await;
    let (session, device) = (signed_in.session().unwrap(), signed_in.device().unwrap());
    let both = format!("{session}; {device}");
    // The user's email is cleared: the account has no address budget left to count against,
    // and the failures below, made without the device cookie, fill the user's budget alone.
    let patch = stepclimb_core::users::UserPatch {
        email: Some(None),
        ..Default::default()
    };
    users::update(&f.pool, scope(f.org), sam, &patch, now())
        .await
        .unwrap();
    for _ in 0..2 {
        let wrong = f
            .change(&session, OTHER_PASSWORD, "another long passphrase")
            .await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "{wrong:?}");
    }
    // The device cookie's own budget is untouched, and the user's still refuses.
    let before = f.computations();
    let refused = f.change(&both, PASSWORD, "another long passphrase").await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::TOO_MANY_REQUESTS, "auth.rate_limited"),
        "{refused:?}"
    );
    assert!(refused.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(f.computations(), before, "a spent budget runs no check");
    f.clock.advance(Duration::from_secs(15 * 60));
    let changed = f.change(&both, PASSWORD, "another long passphrase").await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{changed:?}");
}

/// Spec 08 B22: the change takes the per-address budget first, before the session is read.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_takes_the_address_budget(pool: PgPool) {
    let limits = Limits {
        sign_in: NonZeroU32::new(1).expect("a nonzero limit"),
        ..Limits::DEFAULT
    };
    let f = Fixture::with_limits(pool, limits).await;
    let body = json!({ "current_password": PASSWORD, "new_password": OTHER_PASSWORD });
    let first = f
        .send_from(
            client(90),
            Method::PUT,
            MY_PASSWORD,
            f.org,
            None,
            body.clone(),
        )
        .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);
    let second = f
        .send_from(client(90), Method::PUT, MY_PASSWORD, f.org, None, body)
        .await;
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
}

/// B17 (MINOR 1): a change whose current password is replaced after it was verified, before
/// the change takes the user's lock, is refused with the same 401 and stores nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_password_changed_during_a_change_is_not_overwritten(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.with_password(sam, PASSWORD).await;
    let session = f
        .sign_in("sam@example.test", PASSWORD)
        .await
        .session()
        .unwrap();
    let mut tx = f.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR NO KEY UPDATE")
        .bind(sam)
        .execute(&mut *tx)
        .await
        .expect("lock the user");
    let changing = f.change(&session, PASSWORD, "another long passphrase");
    let reset = async {
        until_a_call_waits_on_a_lock(&f.pool).await;
        let hash = password::hash(OTHER_PASSWORD).expect("a hash");
        local_logins::set_password(
            &mut *tx,
            scope(f.org),
            sam,
            hash.as_str(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("reset the password");
        tx.commit().await.expect("commit");
    };
    let (answer, ()) = tokio::join!(changing, reset);
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.credentials_invalid"),
        "{answer:?}"
    );
    assert_eq!(
        f.sign_in("sam@example.test", OTHER_PASSWORD).await.status,
        StatusCode::OK
    );
}
