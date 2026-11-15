// SPDX-License-Identifier: AGPL-3.0-or-later
//! The signed-in user's passkeys over the router (docs/specs/m0/04-local-accounts.md B2, B13,
//! Q10, Permissions, T9; the owner's provider rule of 15 Nov 2026). The browser is
//! `webauthn-authenticator-rs`'s software passkey.
//!
//! The requests name their organization with the `test-support` header of `request_org`. Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers,
    local_sign_in::{self, LocalMethods, LocalSignIn, PROVIDER_KEY},
    local_tokens::Origin,
    organizations,
    passkeys::{self, NewPasskey},
    roles,
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    client_ip::TrustedProxies,
    enrollment,
    rate_limit::{Limits, RateLimiter},
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const ORIGIN: &str = "https://lms.example.org";
const LIST: &str = "/api/v1/me/passkeys";
const START: &str = "/api/v1/me/passkeys/start";

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

fn one(id: Uuid) -> String {
    format!("{LIST}/{id}")
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

    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!((self.status, self.code()), (status, code), "{self:?}");
    }

    fn pairs(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| {
                v.to_str()
                    .expect("ASCII")
                    .split(';')
                    .next()
                    .expect("present")
                    .to_owned()
            })
            .collect()
    }

    /// The `__Host-sc_flow=<token>` pair this answer sets.
    fn flow_cookie(&self) -> String {
        self.pairs()
            .into_iter()
            .find(|pair| pair.starts_with("__Host-sc_flow=") && pair.len() > 15)
            .unwrap_or_else(|| panic!("no flow cookie: {self:?}"))
    }

    fn clears_the_flow_cookie(&self) -> bool {
        self.pairs().iter().any(|pair| pair == "__Host-sc_flow=")
    }
}

/// A signed-in user: their session cookie and id.
struct Member {
    id: Uuid,
    cookie: String,
}

struct Fixture {
    pool: PgPool,
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
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse(ORIGIN).expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: RateLimiter::new(Limits::DEFAULT, TrustedProxies::none()),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app, org }
    }

    async fn org_b(&self) -> Uuid {
        organizations::create(&self.pool, "org-b", "Org B", now())
            .await
            .expect("create organization B")
            .id
    }

    /// A signed-in user of `org` with no identity.
    async fn bare_in(&self, org: Uuid, email: &str) -> Member {
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
        let id = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        let cookie = cookie.split(';').next().expect("a pair").to_owned();
        Member { id, cookie }
    }

    /// A signed-in user of `org` with a local account (the identity enrollment creates).
    async fn local_in(&self, org: Uuid, email: &str) -> Member {
        let member = self.bare_in(org, email).await;
        let provider = identity_providers::get_by_key(&self.pool, scope(org), PROVIDER_KEY)
            .await
            .expect("read the local row")
            .expect("the local row");
        self.link(org, member.id, provider.id).await;
        member
    }

    async fn local(&self, email: &str) -> Member {
        self.local_in(self.org, email).await
    }

    async fn link(&self, org: Uuid, user: Uuid, provider: Uuid) {
        let identity = NewIdentity {
            user_id: user,
            provider_id: provider,
            subject: user.to_string(),
            attributes: serde_json::Map::new(),
        };
        identities::link(&self.pool, scope(org), &identity, now(), Actor::Instance)
            .await
            .expect("link identity");
    }

    /// A new OIDC provider of the organization with `config`, inserted directly since its admin
    /// API is not built: its id.
    async fn provider(&self, enabled: bool, config: Value) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
             VALUES ($1, $2, $3, 'oidc', $4, $5)",
        )
        .bind(id)
        .bind(self.org)
        .bind(format!("p{}", id.simple()))
        .bind(config)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    /// A signed-in user whose only identity is at a new OIDC provider with `config`.
    async fn outside(&self, email: &str, enabled: bool, config: Value) -> (Member, Uuid) {
        let member = self.bare_in(self.org, email).await;
        let provider = self.provider(enabled, config).await;
        self.link(self.org, member.id, provider).await;
        (member, provider)
    }

    /// A stored passkey of `user` named `name`, with a placeholder credential: the management
    /// routes never read it.
    async fn passkey_in(&self, org: Uuid, user: Uuid, name: &str) -> Uuid {
        let credential = json!({ "placeholder": true });
        let credential_id = new_id().as_bytes().to_vec();
        let new = NewPasskey {
            user_id: user,
            credential_id: &credential_id,
            credential: &credential,
            name,
        };
        passkeys::create(&self.pool, scope(org), new, now(), Actor::Instance)
            .await
            .expect("store a passkey")
            .id
    }

    async fn passkey(&self, user: Uuid, name: &str) -> Uuid {
        self.passkey_in(self.org, user, name).await
    }

    async fn switch_passkeys(&self, on: bool) {
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

    async fn count(&self, user: Uuid) -> usize {
        passkeys::list_for_user(&self.pool, scope(self.org), user)
            .await
            .expect("list")
            .len()
    }

    async fn actions(&self) -> Vec<String> {
        audit::list(&self.pool, scope(self.org), 1000)
            .await
            .expect("audit entries")
            .into_iter()
            .map(|entry| entry.action)
            .collect()
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        self.send_in(self.org, method, uri, cookie, body, &[]).await
    }

    async fn send_in(
        &self,
        org: Uuid,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        for (name, value) in extra {
            request = request.header(*name, *value);
        }
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
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

    /// Start adding a passkey as `member` and answer it with a new software passkey: the flow
    /// cookie and the registration response.
    async fn started(&self, member: &Member) -> (String, Value) {
        let (flow, credential, _) = self.started_with(member).await;
        (flow, credential)
    }

    /// [`Self::started`], keeping the software passkey so it can sign in later.
    async fn started_with(&self, member: &Member) -> (String, Value, SoftPasskey) {
        let started = self
            .send(Method::POST, START, Some(&member.cookie), None)
            .await;
        assert_eq!(started.status, StatusCode::OK, "{started:?}");
        assert_schema("MyPasskeyStarted", &started.body);
        let options: CreationChallengeResponse =
            serde_json::from_value(started.body["options"].clone()).expect("creation options");
        let mut authenticator = SoftPasskey::new(true);
        let credential = authenticator
            .do_registration(Url::parse(ORIGIN).expect("a URL"), options)
            .expect("the soft passkey registers");
        (
            started.flow_cookie(),
            serde_json::to_value(credential).expect("JSON"),
            authenticator,
        )
    }

    /// Sign in as `email` through `/auth/local/passkey` with `authenticator`: the finish answer.
    async fn sign_in(&self, email: &str, authenticator: &mut SoftPasskey) -> Answer {
        let body = json!({ "email": email });
        let started = self
            .send(Method::POST, "/auth/local/passkey/start", None, Some(body))
            .await;
        assert_eq!(started.status, StatusCode::OK, "{started:?}");
        let options: RequestChallengeResponse =
            serde_json::from_value(started.body["options"].clone()).expect("request options");
        let assertion = authenticator
            .do_authentication(Url::parse(ORIGIN).expect("a URL"), options)
            .expect("the soft passkey signs");
        let body = json!({ "credential": assertion });
        let flow = started.flow_cookie();
        self.send(
            Method::POST,
            "/auth/local/passkey/finish",
            Some(&flow),
            Some(body),
        )
        .await
    }

    /// Set the provider `id`'s `allow_local_passkeys`, by SQL, since its admin API is not built.
    async fn allow_passkeys(&self, id: Uuid, on: bool) {
        sqlx::query(
            "UPDATE identity_providers
             SET config = jsonb_set(config, '{allow_local_passkeys}', to_jsonb($2::boolean))
             WHERE id = $1",
        )
        .bind(id)
        .bind(on)
        .execute(&self.pool)
        .await
        .expect("set allow_local_passkeys");
    }

    async fn finish(&self, member: &Member, flow: &str, credential: &Value) -> Answer {
        let cookie = format!("{}; {flow}", member.cookie);
        let body = json!({ "credential": credential, "name": "Laptop" });
        self.send(Method::POST, LIST, Some(&cookie), Some(body))
            .await
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_local_user_adds_lists_renames_and_deletes_passkeys(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let listed = fx.send(Method::GET, LIST, Some(&pat.cookie), None).await;
    assert_eq!(
        (listed.status, listed.body.clone()),
        (StatusCode::OK, json!([]))
    );

    let (flow, credential) = fx.started(&pat).await;
    let added = fx.finish(&pat, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");
    assert_schema("MyPasskey", &added.body);
    assert_eq!(added.body["name"], "Laptop");
    assert!(added.clears_the_flow_cookie());
    let first: Uuid = added.body["id"]
        .as_str()
        .expect("present")
        .parse()
        .expect("present");

    let second = fx.passkey(pat.id, "Phone").await;
    let listed = fx.send(Method::GET, LIST, Some(&pat.cookie), None).await;
    assert_eq!(listed.status, StatusCode::OK);
    let names: Vec<&str> = listed
        .body
        .as_array()
        .expect("present")
        .iter()
        .map(|p| p["name"].as_str().expect("present"))
        .collect();
    assert_eq!(names, ["Laptop", "Phone"]);
    for item in listed.body.as_array().expect("present") {
        assert_schema("MyPasskey", item);
        assert_eq!(item["last_used_at"], Value::Null);
        assert!(item.get("credential").is_none());
    }

    let body = json!({ "name": "  Work laptop " });
    let renamed = fx
        .send(Method::PATCH, &one(first), Some(&pat.cookie), Some(body))
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{renamed:?}");
    assert_eq!(renamed.body["name"], "Work laptop");
    assert_schema("MyPasskey", &renamed.body);

    let deleted = fx
        .send(Method::DELETE, &one(second), Some(&pat.cookie), None)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    assert_eq!(fx.count(pat.id).await, 1);
    let actions = fx.actions().await;
    for action in ["passkey.create", "passkey.rename", "passkey.delete"] {
        assert!(actions.iter().any(|a| a == action), "{action}: {actions:?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn adding_a_passkey_links_no_local_identity_and_records_the_user(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let (sam, _) = fx
        .outside(
            "sam@example.test",
            true,
            json!({ "allow_local_passkeys": true }),
        )
        .await;
    let (flow, credential) = fx.started(&sam).await;
    let added = fx.finish(&sam, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");
    let identities = identities::list_for_user(&fx.pool, scope(fx.org), sam.id)
        .await
        .expect("identities");
    assert!(
        identities.iter().all(|i| i.provider_key != PROVIDER_KEY),
        "{identities:?}"
    );
    let entries = audit::list(&fx.pool, scope(fx.org), 1000)
        .await
        .expect("entries");
    let create = entries
        .iter()
        .find(|e| e.action == "passkey.create")
        .expect("an entry");
    assert_eq!(
        create.actor,
        Actor::User {
            id: sam.id,
            ip: CLIENT.ip()
        }
    );
    assert_eq!(create.data["after"]["name"], "Laptop");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_outside_provider_user_may_not_add_a_passkey_by_default(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let (sam, _) = fx.outside("sam@example.test", true, json!({})).await;
    let started = fx.send(Method::POST, START, Some(&sam.cookie), None).await;
    started.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
    assert!(
        started
            .pairs()
            .iter()
            .all(|p| !p.starts_with("__Host-sc_flow"))
    );
    let (off, _) = fx
        .outside(
            "off@example.test",
            true,
            json!({ "allow_local_passkeys": false }),
        )
        .await;
    let started = fx.send(Method::POST, START, Some(&off.cookie), None).await;
    started.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
    // A user with no identity at all has no provider that allows it.
    let bare = fx.bare_in(fx.org, "bare@example.test").await;
    let started = fx.send(Method::POST, START, Some(&bare.cookie), None).await;
    started.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_disabled_provider_that_allows_passkeys_does_not_count(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let (sam, _) = fx
        .outside(
            "sam@example.test",
            false,
            json!({ "allow_local_passkeys": true }),
        )
        .await;
    let started = fx.send(Method::POST, START, Some(&sam.cookie), None).await;
    started.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_user_is_unaffected_by_a_provider_that_refuses_passkeys(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let closed = fx.provider(true, json!({})).await;
    fx.link(fx.org, pat.id, closed).await;
    let (flow, credential) = fx.started(&pat).await;
    let added = fx.finish(&pat, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_add_rechecks_the_provider_rule_and_uses_the_flow_up(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let (sam, provider) = fx
        .outside(
            "sam@example.test",
            true,
            json!({ "allow_local_passkeys": true }),
        )
        .await;
    let (flow, credential) = fx.started(&sam).await;
    sqlx::query("UPDATE identity_providers SET config = '{}' WHERE id = $1")
        .bind(provider)
        .execute(&fx.pool)
        .await
        .expect("switch the provider's passkeys off");
    let refused = fx.finish(&sam, &flow, &credential).await;
    refused.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
    assert!(refused.clears_the_flow_cookie());
    assert_eq!(fx.count(sam.id).await, 0);
    let again = fx.finish(&sam, &flow, &credential).await;
    again.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bad_response_another_users_flow_and_a_replay_are_refused(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let kim = fx.local("kim@example.test").await;

    let (flow, mut credential) = fx.started(&pat).await;
    credential["response"]["clientDataJSON"] = json!("e30");
    let bad = fx.finish(&pat, &flow, &credential).await;
    bad.expect(StatusCode::BAD_REQUEST, "auth.passkey_invalid");
    assert!(bad.clears_the_flow_cookie());

    let (pats_flow, credential) = fx.started(&pat).await;
    let stolen = fx.finish(&kim, &pats_flow, &credential).await;
    stolen.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");
    assert_eq!(fx.count(kim.id).await, 0);

    let (flow, credential) = fx.started(&pat).await;
    let added = fx.finish(&pat, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");
    let replay = fx.finish(&pat, &flow, &credential).await;
    replay.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");

    let none = fx
        .send(
            Method::POST,
            LIST,
            Some(&pat.cookie),
            Some(json!({ "credential": {} })),
        )
        .await;
    none.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");
    let bad_name = json!({ "credential": {}, "name": "  " });
    let bad_name = fx
        .send(Method::POST, LIST, Some(&pat.cookie), Some(bad_name))
        .await;
    bad_name.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrollment_flow_and_an_account_flow_do_not_finish_each_other(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let link = enrollment::create_link(
        &fx.pool,
        scope(fx.org),
        pat.id,
        Origin::Bootstrap,
        &WebOrigin::parse(ORIGIN).expect("present"),
        now(),
        Actor::Instance,
    )
    .await
    .expect("an enrollment link");
    let (_, token) = link.expose_url().split_once("#token=").expect("a token");
    let token = token.to_owned();
    let start = json!({ "token": token });
    let enrolling = fx
        .send(
            Method::POST,
            "/auth/local/enrollment/start",
            None,
            Some(start),
        )
        .await;
    assert_eq!(enrolling.status, StatusCode::OK, "{enrolling:?}");
    let options: CreationChallengeResponse =
        serde_json::from_value(enrolling.body["passkey_options"].clone()).expect("options");
    let credential = SoftPasskey::new(true)
        .do_registration(Url::parse(ORIGIN).expect("present"), options)
        .expect("registers");
    let credential = serde_json::to_value(credential).expect("present");
    let wrong_route = fx.finish(&pat, &enrolling.flow_cookie(), &credential).await;
    wrong_route.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");

    let (flow, credential) = fx.started(&pat).await;
    let body = json!({ "token": token, "credential": credential });
    let other = fx
        .send(
            Method::POST,
            "/auth/local/enrollment/passkey",
            Some(&flow),
            Some(body),
        )
        .await;
    other.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");
    assert!(other.clears_the_flow_cookie());
    assert_eq!(fx.count(pat.id).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_bearer_credential_answers_session_required_on_every_route(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let id = fx.passkey(pat.id, "Phone").await;
    let bearer = [("authorization", "Bearer sc_not-a-real-key")];
    let routes = [
        (Method::GET, LIST.to_owned(), None),
        (Method::POST, START.to_owned(), None),
        (
            Method::POST,
            LIST.to_owned(),
            Some(json!({ "credential": {} })),
        ),
        (Method::PATCH, one(id), Some(json!({ "name": "Renamed" }))),
        (Method::DELETE, one(id), None),
    ];
    for (method, uri, body) in routes {
        for cookie in [None, Some(pat.cookie.as_str())] {
            let answer = fx
                .send_in(fx.org, method.clone(), &uri, cookie, body.clone(), &bearer)
                .await;
            answer.expect(StatusCode::FORBIDDEN, "auth.session_required");
        }
        let anonymous = fx.send(method.clone(), &uri, None, body.clone()).await;
        anonymous.expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    }
    assert_eq!(fx.count(pat.id).await, 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_another_users_passkey_answers_not_found_in_and_across_organizations(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let kim = fx.local("kim@example.test").await;
    let kims = fx.passkey(kim.id, "Kim's").await;
    let _second = fx.passkey(kim.id, "Kim's second").await;
    let b = fx.org_b().await;
    let robin = fx.local_in(b, "robin@example.test").await;
    let robins = fx.passkey_in(b, robin.id, "Robin's").await;
    let _robins_second = fx.passkey_in(b, robin.id, "Robin's second").await;
    for id in [kims, robins, new_id()] {
        let renamed = fx
            .send(
                Method::PATCH,
                &one(id),
                Some(&pat.cookie),
                Some(json!({ "name": "Mine" })),
            )
            .await;
        renamed.expect(StatusCode::NOT_FOUND, "passkey.not_found");
        let deleted = fx
            .send(Method::DELETE, &one(id), Some(&pat.cookie), None)
            .await;
        deleted.expect(StatusCode::NOT_FOUND, "passkey.not_found");
    }
    let listed = fx.send(Method::GET, LIST, Some(&pat.cookie), None).await;
    assert_eq!(listed.body, json!([]));
    assert_eq!(fx.count(kim.id).await, 2);
    let robin_left = passkeys::list_for_user(&fx.pool, scope(b), robin.id)
        .await
        .expect("present");
    assert_eq!(robin_left.len(), 2);
    assert!(robin_left.iter().all(|p| p.name.starts_with("Robin's")));
    // Org B's session does not authenticate in org A.
    let crossed = fx.send(Method::GET, LIST, Some(&robin.cookie), None).await;
    assert_eq!(crossed.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q10_the_last_way_to_sign_in_cannot_be_deleted(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let only = fx.passkey(pat.id, "Only").await;
    let refused = fx
        .send(Method::DELETE, &one(only), Some(&pat.cookie), None)
        .await;
    refused.expect(StatusCode::CONFLICT, "local.last_method");
    assert_eq!(fx.count(pat.id).await, 1);

    // An identity at a disabled provider is no way to sign in.
    let disabled = fx.provider(false, json!({})).await;
    fx.link(fx.org, pat.id, disabled).await;
    let refused = fx
        .send(Method::DELETE, &one(only), Some(&pat.cookie), None)
        .await;
    refused.expect(StatusCode::CONFLICT, "local.last_method");

    // A second passkey lets the first go.
    let spare = fx.passkey(pat.id, "Spare").await;
    let deleted = fx
        .send(Method::DELETE, &one(only), Some(&pat.cookie), None)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");

    // So does an identity at an enabled outside provider.
    let enabled = fx.provider(true, json!({})).await;
    fx.link(fx.org, pat.id, enabled).await;
    let deleted = fx
        .send(Method::DELETE, &one(spare), Some(&pat.cookie), None)
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    assert_eq!(fx.count(pat.id).await, 0);
    let entries = fx.actions().await;
    assert_eq!(entries.iter().filter(|a| *a == "passkey.delete").count(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn renaming_to_the_same_name_writes_nothing(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let id = fx.passkey(pat.id, "Phone").await;
    let same = fx
        .send(
            Method::PATCH,
            &one(id),
            Some(&pat.cookie),
            Some(json!({ "name": " Phone " })),
        )
        .await;
    assert_eq!(
        (same.status, same.body["name"].clone()),
        (StatusCode::OK, json!("Phone"))
    );
    assert!(!fx.actions().await.iter().any(|a| a == "passkey.rename"));
    let blank = fx
        .send(
            Method::PATCH,
            &one(id),
            Some(&pat.cookie),
            Some(json!({ "name": "" })),
        )
        .await;
    blank.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    let renamed = fx
        .send(
            Method::PATCH,
            &one(id),
            Some(&pat.cookie),
            Some(json!({ "name": "Tablet" })),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    let entries = audit::list(&fx.pool, scope(fx.org), 1000)
        .await
        .expect("present");
    let rename = entries
        .iter()
        .find(|e| e.action == "passkey.rename")
        .expect("an entry");
    assert_eq!(
        rename.data,
        json!({ "before": { "name": "Phone" }, "after": { "name": "Tablet" } })
    );
    assert_eq!(rename.target_id, id);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_route_answers_method_disabled_while_passkeys_are_off(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let id = fx.passkey(pat.id, "Phone").await;
    let _spare = fx.passkey(pat.id, "Spare").await;
    fx.switch_passkeys(false).await;
    let routes = [
        (Method::GET, LIST.to_owned(), None),
        (Method::POST, START.to_owned(), None),
        (
            Method::POST,
            LIST.to_owned(),
            Some(json!({ "credential": {} })),
        ),
        (Method::PATCH, one(id), Some(json!({ "name": "Renamed" }))),
        (Method::DELETE, one(id), None),
    ];
    for (method, uri, body) in &routes {
        let answer = fx
            .send(method.clone(), uri, Some(&pat.cookie), body.clone())
            .await;
        answer.expect(StatusCode::NOT_FOUND, "auth.method_disabled");
    }
    assert_eq!(fx.count(pat.id).await, 2);
    fx.switch_passkeys(true).await;
    let listed = fx.send(Method::GET, LIST, Some(&pat.cookie), None).await;
    assert_eq!(listed.body.as_array().map(Vec::len), Some(2));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_registration_does_not_finish_here(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let pat = fx.local("pat@example.test").await;
    let b = fx.org_b().await;
    let robin = fx.local_in(b, "robin@example.test").await;
    let started = fx
        .send_in(b, Method::POST, START, Some(&robin.cookie), None, &[])
        .await;
    assert_eq!(started.status, StatusCode::OK, "{started:?}");
    let options: CreationChallengeResponse =
        serde_json::from_value(started.body["options"].clone()).expect("creation options");
    let credential = SoftPasskey::new(true)
        .do_registration(Url::parse(ORIGIN).expect("a URL"), options)
        .expect("the soft passkey registers");
    let credential = serde_json::to_value(credential).expect("JSON");
    let crossed = fx.finish(&pat, &started.flow_cookie(), &credential).await;
    crossed.expect(StatusCode::BAD_REQUEST, "auth.flow_invalid");
    assert_eq!(fx.count(pat.id).await, 0);
    let robins = passkeys::list_for_user(&fx.pool, scope(b), robin.id)
        .await
        .expect("list");
    assert!(robins.is_empty());
}

/// An outside provider's user adds a passkey while the provider allows it and signs in with it,
/// with no local identity made on the way; switched off, the provider stops both the add and the
/// sign-in (spec 04 Q16), the passkey row stays, and switched back on it signs in again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q16_an_outside_users_passkey_follows_the_providers_setting(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let email = "sam@example.test";
    let (sam, provider) = fx
        .outside(email, true, json!({ "allow_local_passkeys": true }))
        .await;
    let (flow, credential, mut authenticator) = fx.started_with(&sam).await;
    let added = fx.finish(&sam, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");

    let signed_in = fx.sign_in(email, &mut authenticator).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
    let local = identities::list_for_user(&fx.pool, scope(fx.org), sam.id)
        .await
        .expect("identities");
    assert!(
        local.iter().all(|i| i.provider_key != PROVIDER_KEY),
        "a local identity appeared: {local:?}"
    );

    fx.allow_passkeys(provider, false).await;
    let start = fx.send(Method::POST, START, Some(&sam.cookie), None).await;
    start.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
    let refused = fx.sign_in(email, &mut authenticator).await;
    refused.expect(StatusCode::FORBIDDEN, "local.passkeys_not_allowed");
    assert!(
        refused
            .pairs()
            .iter()
            .all(|p| !p.starts_with("__Host-sc_session="))
    );
    assert!(refused.clears_the_flow_cookie());
    assert_eq!(fx.count(sam.id).await, 1, "the passkey row stays");

    fx.allow_passkeys(provider, true).await;
    let again = fx.sign_in(email, &mut authenticator).await;
    assert_eq!(again.status, StatusCode::OK, "{again:?}");
}

/// A local account's passkey signs in whatever its other providers say (spec 04 Q16).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q16_a_local_accounts_passkey_signs_in_beside_a_closed_provider(pool: PgPool) {
    let fx = Fixture::new(pool).await;
    let email = "pat@example.test";
    let pat = fx.local(email).await;
    let closed = fx.provider(true, json!({})).await;
    fx.link(fx.org, pat.id, closed).await;
    let (flow, credential, mut authenticator) = fx.started_with(&pat).await;
    let added = fx.finish(&pat, &flow, &credential).await;
    assert_eq!(added.status, StatusCode::CREATED, "{added:?}");
    let signed_in = fx.sign_in(email, &mut authenticator).await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
}
