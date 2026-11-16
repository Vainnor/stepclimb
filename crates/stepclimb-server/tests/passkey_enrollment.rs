// SPDX-License-Identifier: AGPL-3.0-or-later
//! Passkey registration through an enrollment link over the router
//! (docs/specs/m0/04-local-accounts.md B3, B5, B7, B8, T4, T8's passkey part, T19's token part;
//! docs/specs/m0/08-sessions-and-csrf.md B17, B18). The browser is `webauthn-authenticator-rs`'s
//! software passkey, which answers the server's creation options as a browser at a given origin
//! would; no test needs a real authenticator.
//!
//! The requests name their organization with the `test-support` header of `request_org`. Needs
//! `DATABASE_URL` (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, token::TokenHash};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    auth_flows::LIFETIME,
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    local::has_credential,
    local_sign_in::{self, LocalMethods, LocalSignIn},
    local_tokens::{self, Origin},
    organizations, perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
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
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::{CreationChallengeResponse, Url};

mod app_tokens;
mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const ORIGIN: &str = "https://lms.example.org";
const START: &str = "/auth/local/enrollment/start";
const ENROLL: &str = "/auth/local/enrollment/passkey";

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

    /// The `name=value` pairs of every `Set-Cookie` this answer sets.
    fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| {
                let text = value.to_str().expect("ASCII");
                text.split(';').next().expect("a pair").to_owned()
            })
            .collect()
    }

    /// The `__Host-sc_flow=<token>` pair this answer sets, for the next request's `Cookie`.
    fn flow_cookie(&self) -> String {
        self.set_cookies()
            .into_iter()
            .find(|pair| pair.starts_with("__Host-sc_flow=") && pair.len() > 15)
            .unwrap_or_else(|| panic!("no flow cookie: {self:?}"))
    }

    fn clears_the_flow_cookie(&self) -> bool {
        self.set_cookies()
            .iter()
            .any(|pair| pair == "__Host-sc_flow=")
    }

    fn session_cookie(&self) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|pair| pair.starts_with("__Host-sc_session=") && pair.len() > 18)
    }
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
}

/// An enrollment link and the start route's answer to it.
struct Started {
    token: String,
    answer: Answer,
}

impl Started {
    fn options(&self) -> &Value {
        &self.answer.body["passkey_options"]
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app = router(state(&pool, &clock)).layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            clock,
            app,
            org,
        }
    }

    /// A user of `org` with `email` and no role.
    async fn user_in(&self, org: Uuid, email: &str) -> Uuid {
        let new_user = NewUser {
            display_name: "Pat Example".into(),
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

    /// An enrollment link for `user` of `org`, made by an admin who holds every permission (as
    /// the admin route would): its token.
    async fn link_in(&self, org: Uuid, user: Uuid) -> String {
        self.link_and_creator(org, user).await.0
    }

    /// [`Fixture::link_in`], with the id of the admin who made the link.
    async fn link_and_creator(&self, org: Uuid, user: Uuid) -> (String, Uuid) {
        let admin = self
            .user_in(org, &format!("admin-{}@example.test", new_id().simple()))
            .await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "All".into(),
            permissions: perm::ALL.iter().map(|p| p.key().to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        let assignment = NewAssignment {
            user_id: admin,
            role_id: role,
            scope: Scope::Organization,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
            .await
            .expect("create assignment");
        let token = stepclimb_auth::token::Token::generate().expect("a token");
        let hash = token.hash();
        let origin = Origin::Admin(admin);
        local_tokens::create_enrollment(
            &self.pool,
            scope(org),
            user,
            hash.as_bytes(),
            origin,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create the link");
        (token.expose_secret().to_owned(), admin)
    }

    async fn link(&self, user: Uuid) -> String {
        self.link_in(self.org, user).await
    }

    async fn send(&self, org: Uuid, uri: &str, cookie: Option<&str>, body: Value) -> Answer {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .app
            .clone()
            .oneshot(
                request
                    .body(Body::from(body.to_string()))
                    .expect("a request"),
            )
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

    async fn start_with(&self, org: Uuid, token: &str) -> Started {
        let answer = self.send(org, START, None, json!({ "token": token })).await;
        Started {
            token: token.to_owned(),
            answer,
        }
    }

    /// Open the link `token` in a new browser.
    async fn start(&self, token: &str) -> Started {
        let started = self.start_with(self.org, token).await;
        assert_eq!(
            started.answer.status,
            StatusCode::OK,
            "{:?}",
            started.answer
        );
        started
    }

    /// Post `credential` for `started`'s link with its flow cookie.
    async fn enroll(&self, started: &Started, credential: Value, name: Option<&str>) -> Answer {
        self.enroll_in(
            self.org,
            started,
            &started.answer.flow_cookie(),
            credential,
            name,
        )
        .await
    }

    async fn enroll_in(
        &self,
        org: Uuid,
        started: &Started,
        cookie: &str,
        credential: Value,
        name: Option<&str>,
    ) -> Answer {
        let mut body = json!({ "token": started.token, "credential": credential });
        if let Some(name) = name {
            body["name"] = name.into();
        }
        self.send(org, ENROLL, Some(cookie), body).await
    }

    async fn passkeys_of(&self, user: Uuid) -> Vec<Value> {
        sqlx::query_scalar("SELECT to_jsonb(p) FROM passkeys p WHERE user_id = $1")
            .bind(user)
            .fetch_all(&self.pool)
            .await
            .expect("read passkeys")
    }

    async fn link_used(&self, token: &str) -> bool {
        sqlx::query_scalar("SELECT used_at IS NOT NULL FROM local_tokens WHERE token_hash = $1")
            .bind(TokenHash::of(token).as_bytes().as_slice())
            .fetch_one(&self.pool)
            .await
            .expect("the link")
    }

    async fn sessions_of(&self, user: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
            .bind(user)
            .fetch_one(&self.pool)
            .await
            .expect("count sessions")
    }

    async fn set_passkey(&self, on: bool) {
        let methods = LocalMethods {
            passkey: on,
            ..LocalMethods::default()
        };
        let new = LocalSignIn {
            enabled: true,
            methods,
        };
        local_sign_in::update(&self.pool, scope(self.org), new, now(), Actor::Instance)
            .await
            .expect("set the switches");
    }
}

fn state(pool: &PgPool, clock: &ManualClock) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse(ORIGIN).expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(Limits::DEFAULT, TrustedProxies::none()),
        passwords: Default::default(),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// What a browser at `origin` answers to `options` after `edit` changed them, with a software
/// passkey that verifies its user.
fn respond_to(options: &Value, origin: &str, edit: impl FnOnce(&mut Value)) -> Value {
    let mut options = options.clone();
    edit(&mut options);
    let options: CreationChallengeResponse =
        serde_json::from_value(options).expect("creation options");
    let response = SoftPasskey::new(true)
        .do_registration(Url::parse(origin).expect("a URL"), options)
        .expect("the soft passkey registers");
    serde_json::to_value(response).expect("JSON")
}

fn respond(options: &Value) -> Value {
    respond_to(options, ORIGIN, |_| {})
}

/// T4 (B7, B8, B3): the start route answers creation options for the user and sets the flow
/// cookie; the registration stores the passkey, uses the link, links the local identity, signs
/// in and writes both audit entries. The passkey is a local credential: the user's last outside
/// identity can now go.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn registering_through_a_link_stores_the_passkey_and_signs_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let started = f.start(&f.link(user).await).await;
    assert_schema("EnrollmentStarted", &started.answer.body);
    let public_key = &started.options()["publicKey"];
    assert_eq!(public_key["rp"]["id"], "lms.example.org");
    assert_eq!(public_key["rp"]["name"], "Org A");
    assert_eq!(public_key["user"]["name"], "sam@example.test");
    assert_eq!(
        public_key["authenticatorSelection"]["userVerification"],
        "required"
    );
    assert_eq!(public_key["attestation"], "none");
    assert!(
        public_key.get("excludeCredentials").is_none(),
        "{public_key}"
    );

    let answer = f
        .enroll(&started, respond(started.options()), Some(" Work laptop "))
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_schema("SignedIn", &answer.body);
    assert_eq!(answer.body, json!({ "return_to": "/" }));
    assert!(answer.session_cookie().is_some(), "{answer:?}");
    assert!(answer.clears_the_flow_cookie(), "{answer:?}");
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");

    let stored = f.passkeys_of(user).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["name"], "Work laptop");
    assert_eq!(stored[0]["org_id"], json!(f.org));
    assert!(
        stored[0]["credential"]["cred"]["counter"].is_number(),
        "{}",
        stored[0]
    );
    assert!(f.link_used(&started.token).await);
    assert_eq!(f.sessions_of(user).await, 1);

    let local = stepclimb_core::identity_providers::get_by_key(&f.pool, scope(f.org), "local")
        .await
        .expect("read the local row")
        .expect("the local row");
    let linked = identities::find_user(&f.pool, scope(f.org), local.id, &user.to_string())
        .await
        .expect("find the identity")
        .expect("the local identity");
    assert_eq!(linked.user.id, user);

    let entries = audit::list(&f.pool, scope(f.org), 100)
        .await
        .expect("audit entries");
    let actor = Actor::User {
        id: user,
        ip: CLIENT.ip(),
    };
    let ours: Vec<(&str, Value)> = entries
        .iter()
        .filter(|entry| entry.actor == actor)
        .map(|entry| (entry.action.as_str(), entry.data.clone()))
        .collect();
    assert_eq!(
        ours,
        [
            (
                "passkey.create",
                json!({ "after": { "user_id": user, "name": "Work laptop" } })
            ),
            (
                "identity.link",
                json!({ "after": { "user_id": user, "provider_key": "local", "subject": user.to_string() } })
            ),
        ]
    );

    let mut conn = f.pool.acquire().await.expect("a connection");
    assert!(
        has_credential(&mut conn, scope(f.org), user)
            .await
            .expect("read")
    );
    let provider = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'example-oidc', 'oidc', '{}', true)",
    )
    .bind(provider)
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("insert provider");
    let identity = NewIdentity {
        user_id: user,
        provider_id: provider,
        subject: "1000001".into(),
        attributes: serde_json::Map::new(),
    };
    let outside = identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link");
    identities::unlink_unless_last(
        &f.pool,
        scope(f.org),
        user,
        outside.id,
        now(),
        Actor::Instance,
    )
    .await
    .expect("the passkey lets the last outside identity go");
}

/// T4 (B8): a response for another origin, another relying party ID or another challenge
/// answers `auth.passkey_invalid`, stores nothing and leaves the link usable. Each failure uses
/// its flow up: the same cookie again is `auth.flow_invalid`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_response_for_another_ceremony_is_refused_and_the_link_still_works(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let token = f.link(user).await;

    let other_origin = f.start(&token).await;
    let response = respond_to(
        other_origin.options(),
        "https://sub.lms.example.org",
        |_| {},
    );
    let other_rp = f.start(&token).await;
    let response_rp = respond_to(other_rp.options(), ORIGIN, |options| {
        options["publicKey"]["rp"]["id"] = "example.org".into();
    });
    let first = f.start(&token).await;
    let second = f.start(&token).await;
    let response_first = respond(first.options());

    for (case, started, credential) in [
        ("origin", &other_origin, response),
        ("rp id", &other_rp, response_rp),
        // The first ceremony's response, posted with the second's flow.
        ("challenge", &second, response_first),
    ] {
        let answer = f.enroll(started, credential.clone(), None).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::BAD_REQUEST, "auth.passkey_invalid"),
            "{case}: {answer:?}"
        );
        assert!(answer.clears_the_flow_cookie(), "{case}");
        assert!(answer.session_cookie().is_none(), "{case}");
        let again = f.enroll(started, credential, None).await;
        assert_eq!(again.code(), "auth.flow_invalid", "{case}: {again:?}");
    }
    assert!(f.passkeys_of(user).await.is_empty());
    assert!(!f.link_used(&token).await);
    assert_eq!(f.sessions_of(user).await, 0);

    let retry = f.start(&token).await;
    let answer = f.enroll(&retry, respond(retry.options()), None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(f.passkeys_of(user).await[0]["name"], "Passkey");
}

/// B18, B7: a finished registration cannot be replayed, and the used link starts nothing more.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_finished_registration_and_its_link_are_used_up(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let started = f.start(&f.link(user).await).await;
    let credential = respond(started.options());
    assert_eq!(
        f.enroll(&started, credential.clone(), None).await.status,
        StatusCode::OK
    );

    let replay = f.enroll(&started, credential, None).await;
    assert_eq!(
        (replay.status, replay.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid")
    );
    let reopened = f.start_with(f.org, &started.token).await;
    assert_eq!(reopened.answer.code(), "auth.enrollment_invalid");
    assert_eq!(f.passkeys_of(user).await.len(), 1);
}

/// Two browsers holding the same link: the first registration uses it, so the second is
/// `auth.enrollment_invalid` and stores nothing, even when both arrive at once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_registers_one_passkey_even_when_two_finish_at_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let token = f.link(user).await;
    let one = f.start(&token).await;
    let two = f.start(&token).await;
    let (a, b) = tokio::join!(
        f.enroll(&one, respond(one.options()), None),
        f.enroll(&two, respond(two.options()), None),
    );
    let mut outcomes = [
        (a.status, a.code().to_owned()),
        (b.status, b.code().to_owned()),
    ];
    outcomes.sort();
    assert_eq!(
        outcomes,
        [
            (StatusCode::OK, String::new()),
            (
                StatusCode::BAD_REQUEST,
                "auth.enrollment_invalid".to_owned()
            ),
        ]
    );
    assert_eq!(f.passkeys_of(user).await.len(), 1);
    assert_eq!(f.sessions_of(user).await, 1);
}

/// B18, B21 on the controllable clock: the flow works until its lifetime ends, then answers
/// `auth.flow_expired`, and the link still works.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_registration_older_than_the_flow_lifetime_has_expired(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let token = f.link(user).await;
    let late = f.start(&token).await;
    let in_time = f.start(&token).await;
    f.clock.advance(LIFETIME - Duration::from_micros(1));
    let answer = f.enroll(&in_time, respond(in_time.options()), None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");

    let token = f.link(f.user("kim@example.test").await).await;
    let late_too = f.start(&token).await;
    f.clock.advance(LIFETIME);
    for started in [&late, &late_too] {
        let answer = f.enroll(started, respond(started.options()), None).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::BAD_REQUEST, "auth.flow_expired"),
            "{answer:?}"
        );
        assert!(answer.clears_the_flow_cookie());
    }
    assert!(!f.link_used(&token).await);
}

/// The flow binds the link it was started for: another link's token with this flow is
/// `auth.flow_invalid`, and both links still work.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_finishes_only_the_link_it_was_started_for(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let kim = f.user("kim@example.test").await;
    let sams = f.start(&f.link(sam).await).await;
    let kims = f.start(&f.link(kim).await).await;
    let answer = f
        .enroll_in(
            f.org,
            &kims,
            &sams.answer.flow_cookie(),
            respond(sams.options()),
            None,
        )
        .await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{answer:?}"
    );
    assert!(!f.link_used(&kims.token).await);
    assert!(!f.link_used(&sams.token).await);
    assert!(f.passkeys_of(kim).await.is_empty());
    let kims = f.start(&kims.token).await;
    let answer = f.enroll(&kims, respond(kims.options()), None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
}

/// T8's passkey part (B5): a suspended user's correct passkey is refused by admission with 403
/// `auth.suspended`; nothing is stored and the link is not used.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_users_registration_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let started = f.start(&f.link(user).await).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        user,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    let answer = f.enroll(&started, respond(started.options()), None).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::FORBIDDEN, "auth.suspended"),
        "{answer:?}"
    );
    assert!(answer.session_cookie().is_none());
    assert!(f.passkeys_of(user).await.is_empty());
    assert!(!f.link_used(&started.token).await);
    assert_eq!(f.sessions_of(user).await, 0);
}

/// B6's re-check at use, over the route: the link's creator is suspended, or loses
/// `user.manage`, between the start and the registration. The registration answers
/// `auth.enrollment_invalid`, rolls its savepoint back (the link stays unused) and stores no
/// passkey and no session.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_whose_creator_lost_the_right_registers_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    for change in ["suspend", "remove user.manage"] {
        let user = f.user(&format!("{}@example.test", new_id().simple())).await;
        let (token, creator) = f.link_and_creator(f.org, user).await;
        let started = f.start(&token).await;
        match change {
            "suspend" => {
                users::set_status(
                    &f.pool,
                    scope(f.org),
                    creator,
                    Status::Suspended,
                    now(),
                    Actor::Instance,
                )
                .await
                .expect("suspend the creator");
            }
            _ => {
                sqlx::query("DELETE FROM role_assignments WHERE user_id = $1")
                    .bind(creator)
                    .execute(&f.pool)
                    .await
                    .expect("remove the creator's role");
            }
        }
        let answer = f.enroll(&started, respond(started.options()), None).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::BAD_REQUEST, "auth.enrollment_invalid"),
            "{change}: {answer:?}"
        );
        assert!(answer.session_cookie().is_none(), "{change}");
        assert!(!f.link_used(&token).await, "{change}");
        assert!(f.passkeys_of(user).await.is_empty(), "{change}");
        assert_eq!(f.sessions_of(user).await, 0, "{change}");
    }
}

/// B2: with passkeys off the start route sets up no ceremony and the passkey route answers 404
/// `auth.method_disabled` before it reads the flow, which still works once passkeys are back on.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_switched_off_passkey_sets_nothing_up(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let token = f.link(user).await;
    let started = f.start(&token).await;
    f.set_passkey(false).await;

    let password_only = f.start(&token).await;
    assert_eq!(password_only.answer.body["methods"], json!(["password"]));
    assert!(password_only.answer.body.get("passkey_options").is_none());
    assert!(password_only.answer.set_cookies().is_empty());

    let credential = respond(started.options());
    let off = f.enroll(&started, credential.clone(), None).await;
    assert_eq!(
        (off.status, off.code()),
        (StatusCode::NOT_FOUND, "auth.method_disabled"),
        "{off:?}"
    );
    f.set_passkey(true).await;
    let on = f.enroll(&started, credential, None).await;
    assert_eq!(on.status, StatusCode::OK, "{on:?}");
}

/// A bad name is refused before the flow is taken, so the same ceremony can be retried.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bad_name_is_refused_without_using_the_flow(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let started = f.start(&f.link(user).await).await;
    let credential = respond(started.options());
    let blank = f.enroll(&started, credential.clone(), Some("   ")).await;
    assert_eq!(
        (blank.status, blank.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "request.invalid"),
        "{blank:?}"
    );
    assert_eq!(
        blank.body["errors"][0],
        json!({ "field": "/name", "code": "name.empty" })
    );
    let answer = f.enroll(&started, credential, Some("Phone")).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
}

/// A user's second registration excludes the first passkey, so the authenticator that holds it
/// is not registered twice.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_second_registration_excludes_the_users_passkeys(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let first = f.start(&f.link(user).await).await;
    let credential = respond(first.options());
    assert_eq!(
        f.enroll(&first, credential.clone(), None).await.status,
        StatusCode::OK
    );

    let second = f.start(&f.link(user).await).await;
    let excluded = &second.options()["publicKey"]["excludeCredentials"];
    assert_eq!(excluded.as_array().map(Vec::len), Some(1), "{excluded}");
    assert_eq!(excluded[0]["id"], credential["rawId"]);
}

/// T19's token part: organization B's link and flow used on organization A answer
/// `auth.flow_invalid` (A has no such flow) and leave B's link usable.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_link_and_flow_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization B")
        .id;
    let user_b = f.user_in(org_b, "sam@example.test").await;
    let token_b = f.link_in(org_b, user_b).await;
    let started_b = f.start_with(org_b, &token_b).await;
    assert_eq!(
        started_b.answer.status,
        StatusCode::OK,
        "{:?}",
        started_b.answer
    );
    let credential = respond(started_b.options());
    let cookie = started_b.answer.flow_cookie();

    let on_a = f
        .enroll_in(f.org, &started_b, &cookie, credential.clone(), None)
        .await;
    assert_eq!(
        (on_a.status, on_a.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{on_a:?}"
    );
    assert!(!f.link_used(&token_b).await);
    let on_b = f
        .enroll_in(org_b, &started_b, &cookie, credential, None)
        .await;
    assert_eq!(on_b.status, StatusCode::OK, "{on_b:?}");
}

/// B24, spec 08 B4: a registration logged by the server's subscriber at `trace` for every
/// target leaves no enrollment token, flow token, session token or ceremony challenge in the
/// log: `logging::subscriber` holds `webauthn-rs`, which logs its state at `trace`, at `info`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_registration_logs_no_token(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let token = f.link(user).await;
    let log = Captured::default();
    let writer = log.clone();
    // The server's own subscriber with every target at `trace`: its floor must still hold.
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let refused = f.start(&token).await;
    let bad = respond_to(refused.options(), "https://sub.lms.example.org", |_| {});
    assert_eq!(
        f.enroll(&refused, bad, None).await.code(),
        "auth.passkey_invalid"
    );
    let started = f.start(&token).await;
    let answer = f.enroll(&started, respond(started.options()), None).await;
    drop(guard);
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");

    let text = log.text();
    assert!(
        text.contains("passkey enrollment refused"),
        "the capture missed the log"
    );
    assert!(
        text.contains("\"level\":\"TRACE\""),
        "the capture kept no trace event"
    );
    for line in text.lines() {
        let event: Value = serde_json::from_str(line).expect("a JSON log line");
        let target = event["target"].as_str().unwrap_or("");
        let level = event["level"].as_str().unwrap_or("");
        assert!(
            !target.starts_with("webauthn_rs") || matches!(level, "INFO" | "WARN" | "ERROR"),
            "webauthn-rs logged below info: {target} {level}"
        );
    }
    let session = answer.session_cookie().expect("a session cookie");
    let secrets = [
        token.clone(),
        refused.answer.flow_cookie(),
        started.answer.flow_cookie(),
        session,
    ];
    for secret in secrets {
        let value = secret.split_once('=').map_or(secret.as_str(), |(_, v)| v);
        assert!(!text.contains(value), "the log holds a secret");
    }
    // The challenge as the API sends it (base64url) and as `webauthn-rs` would log it (`Debug`
    // of its bytes).
    for started in [&refused, &started] {
        let challenge = started.options()["publicKey"]["challenge"]
            .as_str()
            .expect("a challenge");
        let bytes = URL_SAFE_NO_PAD.decode(challenge).expect("base64url");
        assert!(!text.contains(challenge), "the log holds a challenge");
        assert!(
            !text.contains(&format!("{bytes:?}")),
            "the log holds a challenge's bytes"
        );
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

/// Spec 08 B46: registering a passkey through an enrollment link signs out every app signed in
/// as the user and refuses a code issued before it; a refused registration signs out nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn registering_through_a_link_signs_out_every_app(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let user = f.user("sam@example.test").await;
    let app = app_tokens::sign_in(&f.app, &f.pool, scope(f.org), f.org, user, now()).await;
    let started = f.start(&f.link(user).await).await;
    let refused = f
        .enroll(
            &started,
            respond_to(started.options(), "https://sub.lms.example.org", |_| {}),
            None,
        )
        .await;
    assert_ne!(refused.status, StatusCode::OK, "{refused:?}");
    assert!(
        app_tokens::signed_in(&f.app, f.org, &app, user).await,
        "a refused one keeps it"
    );
    let ready = app_tokens::issue_code(&f.pool, scope(f.org), user, now()).await;
    let started = f.start(&f.link(user).await).await;
    let answer = f.enroll(&started, respond(started.options()), None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert!(app_tokens::signed_out(&f.app, f.org, &app).await);
    assert!(app_tokens::code_refused(&f.app, f.org, &ready).await);
}
