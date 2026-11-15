// SPDX-License-Identifier: AGPL-3.0-or-later
//! Passkey sign-in over the router (docs/specs/m0/04-local-accounts.md B2, B5, B11, B12, Q9,
//! Q12, T1's sign-in part, T6, T7, T8's passkey part, T17's passkey sign-in part;
//! docs/specs/m0/08-sessions-and-csrf.md B7, B18, T5). The browser is `webauthn-authenticator-rs`'s
//! software passkey, registered for the test through `stepclimb_auth::passkey` and kept so it
//! can sign later; its sign counter goes up by one on every assertion.
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
use stepclimb_auth::{
    encryption::KeyRing,
    passkey::{Registrant, RelyingParty},
    token::TokenHash,
};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    auth_flows::LIFETIME,
    clock::ManualClock,
    id::Uuid,
    local_sign_in::{self, LocalMethods, LocalSignIn},
    organizations,
    passkeys::{self, NewPasskey},
    roles,
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
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const ORIGIN: &str = "https://lms.example.org";
const START: &str = "/auth/local/passkey/start";
const FINISH: &str = "/auth/local/passkey/finish";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the routes that build these rows"
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

    /// The `__Host-sc_flow=<token>` pair this answer sets.
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

    fn options(&self) -> &Value {
        &self.body["options"]
    }
}

/// A user with a registered software passkey.
struct Member {
    id: Uuid,
    email: String,
    authenticator: SoftPasskey,
    credential_id: Vec<u8>,
}

impl Member {
    /// What the passkey answers to the request options of `started`, after `edit` changed them.
    fn assert_to(&mut self, started: &Answer, edit: impl FnOnce(&mut Value)) -> Value {
        let mut options = started.options().clone();
        edit(&mut options);
        let options: RequestChallengeResponse =
            serde_json::from_value(options).expect("request options");
        let response = self
            .authenticator
            .do_authentication(Url::parse(ORIGIN).expect("a URL"), options)
            .expect("the soft passkey signs");
        serde_json::to_value(response).expect("JSON")
    }

    fn assert(&mut self, started: &Answer) -> Value {
        self.assert_to(started, |_| {})
    }

    /// The request options' `allowCredentials` entry naming this passkey, to edit into options
    /// that do not list it.
    fn allowed(&self) -> Value {
        json!([{ "type": "public-key", "id": URL_SAFE_NO_PAD.encode(&self.credential_id) }])
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

    async fn org_b(&self) -> Uuid {
        organizations::create(&self.pool, "org-b", "Org B", now())
            .await
            .expect("create organization B")
            .id
    }

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

    /// A user of `org` with `email` and a software passkey registered for them.
    async fn member_in(&self, org: Uuid, email: &str) -> Member {
        let id = self.user_in(org, email).await;
        let party = RelyingParty::new(ORIGIN, "Org").expect("a relying party");
        let registrant = Registrant {
            user_id: id,
            name: email,
            display_name: "Pat Example",
        };
        let started = party
            .start_registration(registrant, Vec::new())
            .expect("start a registration");
        let options: CreationChallengeResponse =
            serde_json::from_value(started.options).expect("creation options");
        let mut authenticator = SoftPasskey::new(true);
        let response = authenticator
            .do_registration(Url::parse(ORIGIN).expect("a URL"), options)
            .expect("the soft passkey registers");
        let response = serde_json::to_value(response).expect("JSON");
        let registered = party
            .finish_registration(&response, &started.state)
            .expect("the registration verifies");
        let new = NewPasskey {
            user_id: id,
            credential_id: &registered.credential_id,
            credential: &registered.credential,
            name: "Phone",
        };
        passkeys::create(&self.pool, scope(org), new, now(), Actor::Instance)
            .await
            .expect("store the passkey");
        Member {
            id,
            email: email.to_owned(),
            authenticator,
            credential_id: registered.credential_id,
        }
    }

    async fn member(&self, email: &str) -> Member {
        self.member_in(self.org, email).await
    }

    async fn send(&self, org: Uuid, uri: &str, cookie: Option<&str>, body: Value) -> Answer {
        post(self.app.clone(), org, uri, cookie, body.to_string()).await
    }

    async fn start_in(&self, org: Uuid, email: &str) -> Answer {
        self.send(org, START, None, json!({ "email": email })).await
    }

    /// Start signing in as `email` in a new browser.
    async fn start(&self, email: &str) -> Answer {
        let answer = self.start_in(self.org, email).await;
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
        answer
    }

    async fn finish_in(&self, org: Uuid, cookie: &str, credential: Value) -> Answer {
        self.send(
            org,
            FINISH,
            Some(cookie),
            json!({ "credential": credential }),
        )
        .await
    }

    /// Post `credential` with the flow cookie `started` set.
    async fn finish(&self, started: &Answer, credential: Value) -> Answer {
        self.finish_in(self.org, &started.flow_cookie(), credential)
            .await
    }

    /// The stored row of the passkey with `credential_id`.
    async fn passkey(&self, credential_id: &[u8]) -> Value {
        sqlx::query_scalar("SELECT to_jsonb(p) FROM passkeys p WHERE credential_id = $1")
            .bind(credential_id)
            .fetch_one(&self.pool)
            .await
            .expect("read the passkey")
    }

    async fn counter(&self, member: &Member) -> u64 {
        self.passkey(&member.credential_id).await["credential"]["cred"]["counter"]
            .as_u64()
            .expect("a counter")
    }

    async fn set_counter(&self, member: &Member, counter: u32) {
        sqlx::query(
            "UPDATE passkeys SET credential = jsonb_set(credential, '{cred,counter}', to_jsonb($2))
             WHERE credential_id = $1",
        )
        .bind(member.credential_id.as_slice())
        .bind(i64::from(counter))
        .execute(&self.pool)
        .await
        .expect("set the counter");
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

/// POST `body` as JSON to `uri` of `org` from this site, with `cookie`.
async fn post(app: Router, org: Uuid, uri: &str, cookie: Option<&str>, body: String) -> Answer {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(TEST_ORG_HEADER, org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .oneshot(request.body(Body::from(body)).expect("a request"))
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

fn state(pool: &PgPool, clock: &ManualClock) -> AppState {
    AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse(ORIGIN).expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(Limits::DEFAULT, TrustedProxies::none()),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// `response` with one bit of its signature flipped: an assertion signed by another key.
fn with_bad_signature(mut response: Value) -> Value {
    let signature = response["response"]["signature"]
        .as_str()
        .expect("a signature");
    let mut bytes = URL_SAFE_NO_PAD.decode(signature).expect("base64url");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    response["response"]["signature"] = URL_SAFE_NO_PAD.encode(bytes).into();
    response
}

/// T6 (B11, B12, B5): the start route finds the user by email in any letter case and allows
/// their passkey with user verification required; the assertion signs in at the flow's
/// `return_to`, sets the session cookie, clears the flow cookie, and saves the new sign counter
/// and the time of use.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_registered_passkey_signs_in_and_its_counter_is_saved(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let started = f
        .send(
            f.org,
            START,
            None,
            json!({ "email": "Sam@Example.test", "return_to": "/c/1" }),
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{started:?}");
    assert_schema("PasskeySignInStarted", &started.body);
    assert_eq!(started.headers[header::CACHE_CONTROL], "no-store");
    let public_key = &started.options()["publicKey"];
    assert_eq!(public_key["rpId"], "lms.example.org");
    assert_eq!(public_key["userVerification"], "required");
    assert_eq!(
        public_key["allowCredentials"][0]["id"],
        sam.allowed()[0]["id"]
    );
    assert_eq!(f.counter(&sam).await, 0);

    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_schema("SignedIn", &answer.body);
    assert_eq!(answer.body, json!({ "return_to": "/c/1" }));
    assert!(answer.session_cookie().is_some(), "{answer:?}");
    assert!(answer.clears_the_flow_cookie(), "{answer:?}");
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(f.sessions_of(sam.id).await, 1);
    assert_eq!(f.counter(&sam).await, 1);
    let row = f.passkey(&sam.credential_id).await;
    assert!(row["last_used_at"].is_string(), "{row}");

    let again = f.start(&sam.email).await;
    let answer = f.finish(&again, sam.assert(&again)).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body, json!({ "return_to": "/" }));
    assert_eq!(f.counter(&sam).await, 2);
}

/// Spec 04 B5, spec 07 B4: a sign-in goes through admission's local entry, keyed by the
/// passkey's user. A user with a passkey but no local identity (one who signs in through an
/// outside provider and added a passkey while signed in) signs in, and still has no local
/// identity afterwards: only an enrollment link creates one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_creates_no_local_identity(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    for _ in 0..2 {
        let started = f.start(&sam.email).await;
        let answer = f.finish(&started, sam.assert(&started)).await;
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    }
    assert_eq!(f.sessions_of(sam.id).await, 2);
    let identities: i64 = sqlx::query_scalar("SELECT count(*) FROM identities WHERE user_id = $1")
        .bind(sam.id)
        .fetch_one(&f.pool)
        .await
        .expect("count identities");
    assert_eq!(identities, 0);
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'identity.link'")
            .fetch_one(&f.pool)
            .await
            .expect("count entries");
    assert_eq!(links, 0);
}

/// T6 (B12, B18): an assertion signed by another key, one by a passkey the organization does
/// not have, and one from another ceremony all answer the same 401 `auth.passkey_invalid`, sign
/// nobody in and save nothing; a finished flow cannot be used again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bad_assertion_is_refused_alike_and_a_flow_is_used_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let org_b = f.org_b().await;
    let mut stranger = f.member_in(org_b, "kim@example.test").await;

    let bad_key = f.start(&sam.email).await;
    let bad_signature = with_bad_signature(sam.assert(&bad_key));
    let unknown = f.start(&sam.email).await;
    let allowed = stranger.allowed();
    let unknown_credential = stranger.assert_to(&unknown, |options| {
        options["publicKey"]["allowCredentials"] = allowed;
    });
    let first = f.start(&sam.email).await;
    let second = f.start(&sam.email).await;
    let first_assertion = sam.assert(&first);

    let mut bodies = Vec::new();
    for (case, started, credential) in [
        ("signature", &bad_key, bad_signature),
        ("unknown credential", &unknown, unknown_credential),
        ("challenge", &second, first_assertion),
        ("not an assertion", &first, json!({ "id": 1 })),
    ] {
        let answer = f.finish(started, credential.clone()).await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
            "{case}: {answer:?}"
        );
        assert!(answer.session_cookie().is_none(), "{case}");
        assert!(answer.clears_the_flow_cookie(), "{case}");
        bodies.push(answer.body.clone());
        let replay = f.finish(started, credential).await;
        assert_eq!(
            (replay.status, replay.code()),
            (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
            "{case}: {replay:?}"
        );
    }
    assert!(
        bodies.windows(2).all(|pair| pair[0] == pair[1]),
        "{bodies:#?}"
    );
    assert_eq!(f.sessions_of(sam.id).await, 0);

    let started = f.start(&sam.email).await;
    let assertion = sam.assert(&started);
    let answer = f.finish(&started, assertion.clone()).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let replay = f.finish(&started, assertion).await;
    assert_eq!(
        (replay.status, replay.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{replay:?}"
    );
    assert!(replay.session_cookie().is_none());
    assert_eq!(f.sessions_of(sam.id).await, 1);
}

/// T7 (B11, Q9 (a)): an address with no account, an account without a passkey and another
/// organization's account get the same answer as each other: request options of the same
/// shape with no credential allowed, and a flow cookie. The flow then signs no one in, not even
/// with a real passkey's assertion.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_address_without_a_passkey_gets_options_that_allow_none(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    f.user_in(f.org, "pat@example.test").await;
    let org_b = f.org_b().await;
    f.member_in(org_b, "kim@example.test").await;
    let known = f.start(&sam.email).await;

    let mut answers = Vec::new();
    for email in [
        "nobody@example.test",
        "pat@example.test",
        "kim@example.test",
        "nul\u{0}@example.test",
    ] {
        let answer = f.start(email).await;
        assert_schema("PasskeySignInStarted", &answer.body);
        assert_eq!(
            answer.options()["publicKey"]["allowCredentials"],
            json!([]),
            "{email}"
        );
        assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
        answer.flow_cookie();
        answers.push(answer);
    }
    let shape = |answer: &Answer| {
        let mut options = answer.options().clone();
        options["publicKey"]["challenge"] = Value::Null;
        options["publicKey"]["allowCredentials"] = json!([]);
        options
    };
    for answer in &answers {
        assert_eq!(shape(answer), shape(&known), "{answer:?}");
    }
    let names: Vec<Vec<String>> = answers
        .iter()
        .chain([&known])
        .map(|answer| {
            answer
                .headers
                .keys()
                .map(|name| name.as_str().to_owned())
                .collect()
        })
        .collect();
    assert!(names.windows(2).all(|pair| pair[0] == pair[1]), "{names:?}");

    let allowed = sam.allowed();
    let assertion = sam.assert_to(&answers[0], |options| {
        options["publicKey"]["allowCredentials"] = allowed;
    });
    let answer = f.finish(&answers[0], assertion).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );
    assert_eq!(f.sessions_of(sam.id).await, 0);
}

/// T19's passkey part: organization B's user cannot sign in on organization A, and A's flow
/// does not finish on B.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_passkey_and_flow_are_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_b = f.org_b().await;
    let mut kim = f.member_in(org_b, "kim@example.test").await;
    let started_on_a = f.start(&kim.email).await;
    let allowed = kim.allowed();
    let assertion = kim.assert_to(&started_on_a, |options| {
        options["publicKey"]["allowCredentials"] = allowed;
    });
    let answer = f.finish(&started_on_a, assertion).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );

    let started_on_b = f.start_in(org_b, &kim.email).await;
    assert_eq!(started_on_b.status, StatusCode::OK, "{started_on_b:?}");
    let assertion = kim.assert(&started_on_b);
    let on_a = f
        .finish_in(f.org, &started_on_b.flow_cookie(), assertion.clone())
        .await;
    assert_eq!(
        (on_a.status, on_a.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{on_a:?}"
    );
    let on_b = f
        .finish_in(org_b, &started_on_b.flow_cookie(), assertion)
        .await;
    assert_eq!(on_b.status, StatusCode::OK, "{on_b:?}");
    assert_eq!(f.sessions_of(kim.id).await, 1);
}

/// A passkey that is another user's by the time the assertion arrives does not sign in the
/// flow's user, though the ceremony allowed its credential when it started.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_passkey_now_of_another_user_does_not_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let pat = f.user_in(f.org, "pat@example.test").await;
    let started = f.start(&sam.email).await;
    sqlx::query("UPDATE passkeys SET user_id = $2 WHERE credential_id = $1")
        .bind(sam.credential_id.as_slice())
        .bind(pat)
        .execute(&f.pool)
        .await
        .expect("move the passkey");
    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );
    assert_eq!(f.sessions_of(sam.id).await, 0);
    assert_eq!(f.sessions_of(pat).await, 0);
}

/// Q12: a sign counter that does not go up against the stored one is refused, whether the
/// stored counter moved before the ceremony started or while it ran (a copy of the passkey
/// signing in meanwhile), and nothing is saved.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_counter_that_does_not_go_up_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    f.set_counter(&sam, 50).await;
    let before = f.start(&sam.email).await;
    let answer = f.finish(&before, sam.assert(&before)).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );

    f.set_counter(&sam, 0).await;
    let during = f.start(&sam.email).await;
    f.set_counter(&sam, 50).await;
    let answer = f.finish(&during, sam.assert(&during)).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );
    assert_eq!(f.counter(&sam).await, 50);
    assert_eq!(f.sessions_of(sam.id).await, 0);
    assert!(f.passkey(&sam.credential_id).await["last_used_at"].is_null());
}

/// Q12's "refuse and flag": a refused counter sets `counter_regressed_at`, committed although
/// the sign-in is refused and its savepoint rolled back. The first time stays. The flag blocks
/// nothing: once the counter goes up again the passkey signs in, still flagged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_counter_flags_the_passkey_without_blocking_it(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let flag = |row: Value| row["counter_regressed_at"].clone();
    assert_eq!(flag(f.passkey(&sam.credential_id).await), Value::Null);
    f.set_counter(&sam, 50).await;
    let started = f.start(&sam.email).await;
    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(answer.code(), "auth.passkey_invalid", "{answer:?}");
    let first = flag(f.passkey(&sam.credential_id).await);
    assert!(first.is_string(), "not flagged: {first}");

    f.clock.advance(Duration::from_secs(60));
    let started = f.start(&sam.email).await;
    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(answer.code(), "auth.passkey_invalid", "{answer:?}");
    assert_eq!(flag(f.passkey(&sam.credential_id).await), first);

    f.set_counter(&sam, 0).await;
    let started = f.start(&sam.email).await;
    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(flag(f.passkey(&sam.credential_id).await), first);
}

/// The finish route locks the passkey row before it reads the counter: while another
/// transaction holds the row, the sign-in waits, then checks the counter that transaction
/// saved (50, above the authenticator's 1) and refuses. Without the lock it would read the old
/// counter, pass, and overwrite 50.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_waits_for_the_passkey_row_and_reads_its_saved_counter(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let started = f.start(&sam.email).await;
    let assertion = sam.assert(&started);

    let mut holder = f.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM passkeys WHERE credential_id = $1 FOR UPDATE")
        .bind(sam.credential_id.as_slice())
        .execute(&mut *holder)
        .await
        .expect("lock the passkey");
    let app = f.app.clone();
    let (org, cookie) = (f.org, started.flow_cookie());
    let body = json!({ "credential": assertion }).to_string();
    let finish = tokio::spawn(async move { post(app, org, FINISH, Some(&cookie), body).await });
    wait_for_a_lock_wait(&f.pool).await;
    sqlx::query(
        "UPDATE passkeys SET credential = jsonb_set(credential, '{cred,counter}', '50')
         WHERE credential_id = $1",
    )
    .bind(sam.credential_id.as_slice())
    .execute(&mut *holder)
    .await
    .expect("save a higher counter");
    holder.commit().await.expect("commit");

    let answer = finish.await.expect("the request runs");
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::UNAUTHORIZED, "auth.passkey_invalid"),
        "{answer:?}"
    );
    assert_eq!(f.counter(&sam).await, 50);
    assert_eq!(f.sessions_of(sam.id).await, 0);
}

/// Wait until a backend of this database waits for a lock, at most 10 seconds.
async fn wait_for_a_lock_wait(pool: &PgPool) {
    for _ in 0..1000 {
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
    panic!("no request waited for the passkey row");
}

/// B18: a sign-in older than the flow's lifetime has expired.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_older_than_the_flow_lifetime_has_expired(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let late = f.start(&sam.email).await;
    let in_time = f.start(&sam.email).await;
    f.clock.advance(LIFETIME - Duration::from_micros(1));
    let answer = f.finish(&in_time, sam.assert(&in_time)).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    f.clock.advance(LIFETIME);
    let answer = f.finish(&late, sam.assert(&late)).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "auth.flow_expired"),
        "{answer:?}"
    );
    assert!(answer.clears_the_flow_cookie());
}

/// T1's sign-in part (B2): with passkeys switched off both routes answer 404
/// `auth.method_disabled`, whatever the body, and the passkey signs nobody in; switched back
/// on, it works unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_switched_off_passkey_does_not_sign_in_until_switched_on(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let started = f.start(&sam.email).await;
    let assertion = sam.assert(&started);
    f.set_passkey(false).await;
    let start = f.start_in(f.org, &sam.email).await;
    let finish = f.finish(&started, assertion.clone()).await;
    for answer in [start, finish] {
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::NOT_FOUND, "auth.method_disabled"),
            "{answer:?}"
        );
    }
    for (uri, cookie) in [(START, None), (FINISH, Some(started.flow_cookie()))] {
        let answer = post(
            f.app.clone(),
            f.org,
            uri,
            cookie.as_deref(),
            "{not json".into(),
        )
        .await;
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::NOT_FOUND, "auth.method_disabled"),
            "{uri} with a malformed body: {answer:?}"
        );
    }
    f.set_passkey(true).await;
    let malformed = post(f.app.clone(), f.org, FINISH, None, "{not json".into()).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST, "{malformed:?}");
    let answer = f.finish(&started, assertion).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
}

/// T8's passkey part (B5): a suspended user's correct passkey is refused with 403
/// `auth.suspended`, after the assertion verified; no session is made and nothing is saved.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_users_passkey_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    users::set_status(
        &f.pool,
        scope(f.org),
        sam.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    let started = f.start(&sam.email).await;
    let bad = f
        .finish(&started, with_bad_signature(sam.assert(&started)))
        .await;
    assert_eq!(bad.code(), "auth.passkey_invalid", "{bad:?}");
    let started = f.start(&sam.email).await;
    let answer = f.finish(&started, sam.assert(&started)).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::FORBIDDEN, "auth.suspended"),
        "{answer:?}"
    );
    assert!(answer.session_cookie().is_none());
    assert!(answer.clears_the_flow_cookie());
    assert_eq!(f.sessions_of(sam.id).await, 0);
    assert_eq!(f.counter(&sam).await, 0);
}

/// Spec 08 B7, T5: a sign-in makes a new session token and ends the session the browser's old
/// cookie names, so a planted or earlier cookie is never promoted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_replaces_the_browsers_old_session(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let started = f.start(&sam.email).await;
    let first = f.finish(&started, sam.assert(&started)).await;
    let old = first.session_cookie().expect("a session cookie");
    let started = f.start(&sam.email).await;
    let cookies = format!("{}; {old}", started.flow_cookie());
    let second = f.finish_in(f.org, &cookies, sam.assert(&started)).await;
    assert_eq!(second.status, StatusCode::OK, "{second:?}");
    let new = second.session_cookie().expect("a session cookie");
    assert_ne!(new, old);
    assert_eq!(f.sessions_of(sam.id).await, 1);
    let (_, old_token) = old.split_once('=').expect("a pair");
    let old_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE token_hash = $1")
        .bind(TokenHash::of(old_token).as_bytes().as_slice())
        .fetch_one(&f.pool)
        .await
        .expect("count");
    assert_eq!(old_rows, 0);
}

/// T17's passkey sign-in part (B24, spec 08 B4): a refused and a successful sign-in logged by
/// the server's subscriber at `trace` leave no flow token, session token or challenge in the
/// log, and nothing below `info` from `webauthn-rs`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_logs_no_token(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let mut sam = f.member("sam@example.test").await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let refused = f.start(&sam.email).await;
    let bad = with_bad_signature(sam.assert(&refused));
    assert_eq!(f.finish(&refused, bad).await.code(), "auth.passkey_invalid");
    let started = f.start(&sam.email).await;
    let answer = f.finish(&started, sam.assert(&started)).await;
    drop(guard);
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");

    let text = log.text();
    assert!(
        text.contains("passkey sign-in refused"),
        "the capture missed the log"
    );
    assert!(text.contains("\"level\":\"TRACE\""), "no trace event kept");
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
    for secret in [refused.flow_cookie(), started.flow_cookie(), session] {
        let (_, value) = secret.split_once('=').expect("a pair");
        assert!(!text.contains(value), "the log holds a token");
    }
    assert!(
        !text.contains("sam@example.test"),
        "the log holds the email"
    );
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
