// SPDX-License-Identifier: AGPL-3.0-or-later
//! Enrollment links over the router (docs/specs/m0/04-local-accounts.md B2, B6, B7, T1's
//! route-level half, T2, T3, T19; docs/specs/m0/08-sessions-and-csrf.md B41). The permission
//! suite's four cases and cross-organization case for the admin route are in
//! `tests/permissions.rs`; these cover what the link does, the escalation rule, the audit entry,
//! the method switches, the rate limit and the other organization's token.
//!
//! The requests name their organization with the `test-support` header of `request_org`, so a
//! test can hold two. `session::start` stands in for the sign-in specs. Needs `DATABASE_URL`
//! (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::post,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, token::TokenHash};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    local_sign_in::{self, LocalMethods, LocalSignIn},
    local_tokens::{self, ENROLLMENT_LIFETIME},
    organizations, perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    client_ip::TrustedProxies,
    rate_limit::{Limits, RateLimiter},
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const START: &str = "/auth/local/enrollment/start";
const LINK_PREFIX: &str = "https://lms.example.org/enroll#token=";

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

fn links_path(user: Uuid) -> String {
    format!("/api/v1/admin/users/{user}/enrollment-links")
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

    /// The token of the link a 201 answered.
    fn token(&self) -> String {
        let url = self.body["url"].as_str().expect("a url");
        url.strip_prefix(LINK_PREFIX)
            .unwrap_or_else(|| panic!("not an enrollment link: {url}"))
            .to_owned()
    }
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        Self::with_limits(pool, Limits::DEFAULT).await
    }

    async fn with_limits(pool: PgPool, limits: Limits) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app = router(state(&pool, &clock, limits)).layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            clock,
            app,
            org,
        }
    }

    /// A user of `org`, with `email`, holding a custom role with `keys` at organization scope
    /// (none when empty): their id.
    async fn user_in(&self, org: Uuid, email: Option<&str>, keys: &[&str]) -> Uuid {
        let new_user = NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: email.map(str::to_owned),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        if keys.is_empty() {
            return user;
        }
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Holder".into(),
            permissions: keys.iter().map(|k| (*k).to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: Scope::Organization,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
            .await
            .expect("create assignment");
        user
    }

    async fn user(&self, email: &str) -> Uuid {
        self.user_in(self.org, Some(email), &[]).await
    }

    /// A session cookie for `user` of `org`.
    async fn cookie(&self, org: Uuid, user: Uuid) -> String {
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    /// A signed-in holder of every catalog permission, as an Organization admin is.
    async fn admin(&self) -> (String, Uuid) {
        let all: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
        let email = format!("admin-{}@example.test", new_id().simple());
        let admin = self.user_in(self.org, Some(&email), &all).await;
        (self.cookie(self.org, admin).await, admin)
    }

    async fn send(
        &self,
        org: Uuid,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a request"))
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

    async fn create_link(&self, cookie: &str, user: Uuid) -> Answer {
        self.send(self.org, &links_path(user), Some(cookie), None)
            .await
    }

    async fn start(&self, token: &str) -> Answer {
        self.start_in(self.org, token).await
    }

    async fn start_in(&self, org: Uuid, token: &str) -> Answer {
        self.send(org, START, None, Some(json!({ "token": token })))
            .await
    }

    async fn links_of(&self, user: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM local_tokens WHERE user_id = $1")
            .bind(user)
            .fetch_one(&self.pool)
            .await
            .expect("count links")
    }

    async fn set_switches(&self, enabled: bool, methods: LocalMethods) {
        let new = LocalSignIn { enabled, methods };
        local_sign_in::update(&self.pool, scope(self.org), new, now(), Actor::Instance)
            .await
            .expect("set the switches");
    }

    /// An enabled OpenID Connect provider, so the switches may turn every local method off.
    async fn outside_provider(&self) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
             VALUES ($1, $2, 'example-oidc', 'oidc', '{}', true)",
        )
        .bind(id)
        .bind(self.org)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }
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
    }
}

fn limits(sign_in: u32) -> Limits {
    Limits {
        sign_in: NonZeroU32::new(sign_in).expect("not zero"),
        ..Limits::DEFAULT
    }
}

/// T2 (B6, B7): the link is answered once with its expiry, the database holds only the token's
/// hash, and the link opens to the user's email and the methods the switches allow.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_is_returned_once_and_only_its_hash_is_stored(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.admin().await;
    let user = f.user("Sam@Example.test").await;

    let created = f.create_link(&cookie, user).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_schema("EnrollmentLink", &created.body);
    assert_eq!(created.body["expires_at"], "2025-11-20T02:13:20.000000Z");

    let token = created.token();
    assert_eq!(token.len(), 43);

    let rows: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(t) FROM local_tokens t")
        .fetch_all(&f.pool)
        .await
        .expect("read the links");
    assert_eq!(rows.len(), 1);
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM local_tokens")
        .fetch_one(&f.pool)
        .await
        .expect("the hash");
    assert_eq!(stored, TokenHash::of(&token).as_bytes());
    assert!(!rows[0].to_string().contains(&token), "{}", rows[0]);
    assert_eq!(rows[0]["kind"], "enrollment");
    assert_eq!(rows[0]["used_at"], Value::Null);

    let started = f.start(&token).await;
    assert_eq!(started.status, StatusCode::OK, "{started:?}");
    assert_schema("EnrollmentStarted", &started.body);
    assert_eq!(started.body["email"], "Sam@Example.test");
    assert_eq!(started.body["methods"], json!(["passkey", "password"]));
    // Passkeys are on, so the answer carries their creation options (tests/passkey_enrollment.rs).
    assert!(
        started.body["passkey_options"]["publicKey"].is_object(),
        "{:?}",
        started.body
    );
    assert_eq!(started.headers[header::CACHE_CONTROL], "no-store");
    // Starting uses nothing up.
    assert_eq!(f.start(&token).await.status, StatusCode::OK);
}

/// T2 (B6): a second link replaces the first, which then answers `auth.enrollment_invalid`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_second_link_replaces_the_first(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.admin().await;
    let user = f.user("sam@example.test").await;
    let other = f.user("kim@example.test").await;
    let first = f.create_link(&cookie, user).await.token();
    let others = f.create_link(&cookie, other).await.token();

    let second = f.create_link(&cookie, user).await.token();
    assert_ne!(first, second);
    let replaced = f.start(&first).await;
    assert_eq!(
        (replaced.status, replaced.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );
    assert_eq!(f.start(&second).await.status, StatusCode::OK);
    assert_eq!(f.links_of(user).await, 1);
    // Another user's link is not touched.
    assert_eq!(f.start(&others).await.status, StatusCode::OK);
}

/// T3 (B7): a used link answers `auth.enrollment_invalid`; one past its lifetime answers
/// `auth.enrollment_expired`, and it works until then.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_used_or_expired_link_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.admin().await;
    let used = f
        .create_link(&cookie, f.user("sam@example.test").await)
        .await
        .token();
    let aging = f
        .create_link(&cookie, f.user("kim@example.test").await)
        .await
        .token();

    let mut conn = f.pool.acquire().await.expect("a connection");
    let hash = TokenHash::of(&used);
    local_tokens::consume_enrollment(&mut conn, scope(f.org), hash.as_bytes(), now())
        .await
        .expect("use the link");
    let again =
        local_tokens::consume_enrollment(&mut conn, scope(f.org), hash.as_bytes(), now()).await;
    assert!(
        matches!(again, Err(local_tokens::Error::Invalid)),
        "{again:?}"
    );
    let answer = f.start(&used).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );

    f.clock
        .advance(ENROLLMENT_LIFETIME - Duration::from_micros(1));
    assert_eq!(f.start(&aging).await.status, StatusCode::OK);
    f.clock.advance(Duration::from_micros(1));
    let expired = f.start(&aging).await;
    assert_eq!(
        (expired.status, expired.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_expired")
    );
    let late = local_tokens::consume_enrollment(
        &mut conn,
        scope(f.org),
        TokenHash::of(&aging).as_bytes(),
        f.clock.clock().now(),
    )
    .await;
    assert!(
        matches!(late, Err(local_tokens::Error::Expired)),
        "{late:?}"
    );
}

/// An unknown token, an empty one and a malformed body are refused without a lookup's answer
/// telling them apart from a used link.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_token_is_invalid_and_a_bad_body_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    for token in ["", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", "x"] {
        let answer = f.start(token).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::BAD_REQUEST, "auth.enrollment_invalid"),
            "{token:?}"
        );
    }
    for body in [
        json!({}),
        json!({ "token": 5 }),
        json!({ "token": "x", "extra": 1 }),
    ] {
        let answer = f.send(f.org, START, None, Some(body.clone())).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::BAD_REQUEST, "request.malformed"),
            "{body}"
        );
    }
}

/// T1's route-level half (B2): with passkey and password both off the start route answers 404
/// `auth.method_disabled`, with one off it lists the other, and switched back on the same link
/// works again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_start_route_follows_the_method_switches(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.admin().await;
    let token = f
        .create_link(&cookie, f.user("sam@example.test").await)
        .await
        .token();
    f.outside_provider().await;
    let on = LocalMethods::default();

    f.set_switches(
        true,
        LocalMethods {
            passkey: false,
            ..on
        },
    )
    .await;
    assert_eq!(f.start(&token).await.body["methods"], json!(["password"]));
    f.set_switches(
        true,
        LocalMethods {
            password: false,
            ..on
        },
    )
    .await;
    assert_eq!(f.start(&token).await.body["methods"], json!(["passkey"]));

    let none = LocalMethods {
        passkey: false,
        password: false,
        ..on
    };
    for (enabled, methods) in [(true, none), (false, on)] {
        f.set_switches(enabled, methods).await;
        let off = f.start(&token).await;
        assert_eq!(
            (off.status, off.code()),
            (StatusCode::NOT_FOUND, "auth.method_disabled"),
            "enabled {enabled}, {methods:?}"
        );
    }
    // The method check runs before the token is read: an unknown token answers the same.
    let unknown = f.start("unknown").await;
    assert_eq!(unknown.code(), "auth.method_disabled");

    f.set_switches(true, on).await;
    let back = f.start(&token).await;
    assert_eq!(back.status, StatusCode::OK, "{back:?}");
    assert_eq!(back.body["methods"], json!(["passkey", "password"]));
}

/// T19 (B7): organization B's token used on organization A answers `auth.enrollment_invalid`
/// and still works on B.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_token_is_invalid(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization B")
        .id;
    let all: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
    let admin_b = f.user_in(org_b, Some("admin@b.example.test"), &all).await;
    let cookie_b = f.cookie(org_b, admin_b).await;
    let user_b = f.user_in(org_b, Some("sam@example.test"), &[]).await;
    let token_b = f
        .send(org_b, &links_path(user_b), Some(&cookie_b), None)
        .await
        .token();

    let on_a = f.start_in(f.org, &token_b).await;
    assert_eq!(
        (on_a.status, on_a.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid")
    );
    assert_eq!(f.start_in(org_b, &token_b).await.status, StatusCode::OK);
}

/// A user without an email cannot get a link (B4: local sign-in finds a user by email), and
/// nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_without_an_email_gets_no_link(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.admin().await;
    let user = f.user_in(f.org, None, &[]).await;
    let answer = f.create_link(&cookie, user).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::CONFLICT, "local.email_required")
    );
    assert_eq!(f.links_of(user).await, 0);
    let unknown = f.create_link(&cookie, new_id()).await;
    assert_eq!(
        (unknown.status, unknown.code()),
        (StatusCode::NOT_FOUND, "user.not_found")
    );
}

/// The owner's escalation rule: `user.manage` alone cannot create a link for a user who holds a
/// permission the caller does not, since the link sets up a way to sign in as them. It can for
/// a user without one, and for itself.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_for_a_user_with_more_permissions_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let manager = f
        .user_in(f.org, Some("manager@example.test"), &["user.manage"])
        .await;
    let cookie = f.cookie(f.org, manager).await;
    let (_, admin) = f.admin().await;

    let refused = f.create_link(&cookie, admin).await;
    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::FORBIDDEN, "permission.escalation")
    );
    assert_eq!(f.links_of(admin).await, 0);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'enrollment_link.create'",
    )
    .fetch_one(&f.pool)
    .await
    .expect("count entries");
    assert_eq!(audited, 0);

    let plain = f.user("sam@example.test").await;
    assert_eq!(
        f.create_link(&cookie, plain).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        f.create_link(&cookie, manager).await.status,
        StatusCode::CREATED
    );
}

/// Spec 09 B22's rule for a new change: one `enrollment_link.create` entry by the admin with the
/// client address, naming the link's row, the user and the expiry, never the token or its hash.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn creating_a_link_is_audited_without_the_token(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, admin) = f.admin().await;
    let user = f.user("sam@example.test").await;
    let token = f.create_link(&cookie, user).await.token();

    let entries = audit::list(&f.pool, scope(f.org), 100)
        .await
        .expect("entries");
    let entry = entries.last().expect("an entry");
    let link_id: Uuid = sqlx::query_scalar("SELECT id FROM local_tokens")
        .fetch_one(&f.pool)
        .await
        .expect("the link");
    assert_eq!(entry.action, "enrollment_link.create");
    assert_eq!(entry.target_type, "enrollment_link");
    assert_eq!(entry.target_id, link_id);
    assert_eq!(
        entry.actor,
        Actor::User {
            id: admin,
            ip: CLIENT.ip()
        }
    );
    assert_eq!(
        entry.data,
        json!({ "after": { "user_id": user, "expires_at": "2025-11-20T02:13:20Z" } })
    );
    let hash_hex: String = TokenHash::of(&token)
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let text = entry.data.to_string();
    assert!(
        !text.contains(&token) && !text.contains(&hash_hex),
        "{text}"
    );
    let creator: (Option<Uuid>, bool) =
        sqlx::query_as("SELECT created_by, created_by_instance FROM local_tokens")
            .fetch_one(&f.pool)
            .await
            .expect("the creator");
    assert_eq!(creator, (Some(admin), false));
}

/// B23, `08` B22: the start route counts against the client address's sign-in budget before it
/// does anything else.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_start_route_is_rate_limited(pool: PgPool) {
    let f = Fixture::with_limits(pool, limits(2)).await;
    assert_eq!(f.start("unknown").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(f.start("unknown").await.status, StatusCode::BAD_REQUEST);
    let limited = f.start("unknown").await;
    assert_eq!(
        (limited.status, limited.code()),
        (StatusCode::TOO_MANY_REQUESTS, "auth.rate_limited")
    );
    assert!(limited.headers.contains_key(header::RETRY_AFTER));
}

/// B23, `08` B14: a cross-site request is refused before the route runs.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_cross_site_start_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let request = Request::builder()
        .method(Method::POST)
        .uri(START)
        .header(TEST_ORG_HEADER, f.org.to_string())
        .header("sec-fetch-site", "cross-site")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"token":"x"}"#))
        .expect("a request");
    let response = f.app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// Spec 03 B19 with spec 04 B3: a user's last identity at an enabled provider stays, since the
/// user has no local credential (`local::has_credential`).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn without_a_local_credential_the_last_identity_stays(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let provider = f.outside_provider().await;
    let user = f.user("sam@example.test").await;
    let identity = NewIdentity {
        user_id: user,
        provider_id: provider,
        subject: "1000001".into(),
        attributes: serde_json::Map::new(),
    };
    let linked = identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link");
    let result = identities::unlink_unless_last(
        &f.pool,
        scope(f.org),
        user,
        linked.id,
        now(),
        Actor::Instance,
    )
    .await;
    assert!(
        matches!(result, Err(identities::Error::LastSignInMethod)),
        "{result:?}"
    );
}

/// B6, coordinator decision (2026-11-14): a link an admin made works only while that admin
/// could make it again. Each case makes a link by a holder of `user.manage` alone for a user
/// with no role, sees it work, changes one thing, and sees both the start route and
/// `consume_enrollment` refuse it with the answer of an unknown token.
async fn manager_link(f: &Fixture) -> (Uuid, Uuid, String) {
    let email = format!("manager-{}@example.test", new_id().simple());
    let manager = f.user_in(f.org, Some(&email), &["user.manage"]).await;
    let cookie = f.cookie(f.org, manager).await;
    let target = f
        .user(&format!("target-{}@example.test", new_id().simple()))
        .await;
    let token = f.create_link(&cookie, target).await.token();
    assert_eq!(f.start(&token).await.status, StatusCode::OK);
    (manager, target, token)
}

async fn assert_unbacked(f: &Fixture, token: &str) {
    let answer = f.start(token).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "auth.enrollment_invalid"),
        "{answer:?}"
    );
    let mut conn = f.pool.acquire().await.expect("a connection");
    let hash = TokenHash::of(token);
    let used =
        local_tokens::consume_enrollment(&mut conn, scope(f.org), hash.as_bytes(), now()).await;
    assert!(
        matches!(used, Err(local_tokens::Error::Invalid)),
        "{used:?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_stops_working_when_its_user_is_promoted_past_its_creator(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, target, token) = manager_link(&f).await;
    let org_admin = roles::builtin_id(&f.pool, "org_admin")
        .await
        .expect("read the role")
        .expect("the built-in role");
    let assignment = NewAssignment {
        user_id: target,
        role_id: org_admin,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(&f.pool, scope(f.org), &assignment, now(), Actor::Instance)
        .await
        .expect("promote the user");
    assert_unbacked(&f, &token).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_stops_working_when_its_creator_loses_user_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _, token) = manager_link(&f).await;
    sqlx::query("DELETE FROM role_assignments WHERE user_id = $1")
        .bind(manager)
        .execute(&f.pool)
        .await
        .expect("remove the creator's role");
    assert_unbacked(&f, &token).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_stops_working_when_its_creator_is_suspended(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _, token) = manager_link(&f).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        manager,
        users::Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend the creator");
    assert_unbacked(&f, &token).await;
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_stops_working_when_its_creator_is_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _, token) = manager_link(&f).await;
    for sql in [
        "DELETE FROM role_assignments WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql)
            .bind(manager)
            .execute(&f.pool)
            .await
            .expect("delete the creator");
    }
    let creator: (Option<Uuid>, bool) =
        sqlx::query_as("SELECT created_by, created_by_instance FROM local_tokens")
            .fetch_one(&f.pool)
            .await
            .expect("the link");
    assert_eq!(creator, (None, false));
    assert_unbacked(&f, &token).await;
}

/// Spec 08 B41: every operation under `/auth/` or `/oauth/` that adds a sign-in step takes
/// `RateLimited` first. With a budget of one request a minute, each method of each such path in
/// the OpenAPI document answers 429 the second time, whatever its body. The requests name no
/// organization while two exist, so the request-organization resolver would refuse every one of
/// them with 404: a 429 shows the limit ran before it. Sign-out ends a session and adds no step,
/// so it is not limited.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_sign_in_route_is_rate_limited(pool: PgPool) {
    let f = Fixture::new(pool.clone()).await;
    organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("a second organization");
    let operations = sign_in_operations();
    assert!(
        operations.contains(&(Method::POST, START.to_owned())),
        "{operations:?}"
    );
    assert_eq!(
        unlimited(&pool, &operations, router).await,
        Vec::<String>::new()
    );
}

/// The walk of [`every_sign_in_route_is_rate_limited`] catches a route without the extractor.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_rate_limit_walk_names_a_route_without_the_extractor(pool: PgPool) {
    Fixture::new(pool.clone()).await;
    async fn open() -> StatusCode {
        StatusCode::NO_CONTENT
    }
    let fixture = |state: AppState| router(state).route("/auth/test/open", post(open));
    let operations = [
        (Method::POST, "/auth/test/open".to_owned()),
        (Method::POST, START.to_owned()),
    ];
    let found = unlimited(&pool, &operations, fixture).await;
    assert_eq!(found, ["POST /auth/test/open"]);
}

/// Every method of every path of the OpenAPI document under `/auth/` or `/oauth/`, sign-out
/// aside.
fn sign_in_operations() -> Vec<(Method, String)> {
    let document = serde_json::to_value(stepclimb_server::openapi::document()).expect("JSON");
    let paths = document["paths"].as_object().expect("paths");
    let mut operations = Vec::new();
    for (path, item) in paths {
        let sign_in = path.starts_with("/auth/") || path.starts_with("/oauth/");
        if !sign_in || path == "/auth/sign-out" {
            continue;
        }
        for method in item.as_object().expect("a path item").keys() {
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method");
            operations.push((method, path.clone()));
        }
    }
    operations
}

/// The operations of `operations` that do not answer 429 to a second request within a minute
/// from one address, naming no organization, on the app `build` makes with a budget of one
/// request.
async fn unlimited(
    pool: &PgPool,
    operations: &[(Method, String)],
    build: impl Fn(AppState) -> Router,
) -> Vec<String> {
    let mut unlimited = Vec::new();
    for (method, path) in operations {
        // A fresh limiter per operation, so each starts with its budget unspent.
        let clock = ManualClock::new(now());
        let mut budget = limits(1);
        budget.token = NonZeroU32::new(1).expect("not zero");
        let app = build(state(pool, &clock, budget)).layer(Extension(ConnectInfo(CLIENT)));
        let mut second = StatusCode::OK;
        for _ in 0..2 {
            let request = Request::builder()
                .method(method.clone())
                .uri(path.as_str())
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .expect("a request");
            second = app
                .clone()
                .oneshot(request)
                .await
                .expect("an answer")
                .status();
        }
        if second != StatusCode::TOO_MANY_REQUESTS {
            unlimited.push(format!("{method} {path}"));
        }
    }
    unlimited
}
