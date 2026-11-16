// SPDX-License-Identifier: AGPL-3.0-or-later
//! The provider administration API over the router, `/api/v1/admin/providers`
//! (docs/specs/m0/06-oauth2-and-vatsim-preset.md B14 to B17, Q2, Q5; T11 to T13). The four
//! cases and the cross-organization case of each route are in `tests/permissions.rs`; these
//! tests cover what the routes do: the answers and the secret they never hold, the audit
//! entries, the key, kind, config and ownership rules, deletion, the last-method rule, and the
//! rules that need more than `provider.manage` (sign-up on, identities, trust for email
//! matching), for sessions and for API keys.
//!
//! One organization unless a test adds a second; requests name it with the `test-support`
//! header of `request_org`. The OAuth 2 sign-in runs against the mock IdP on 127.0.0.1. All data
//! is synthetic. Needs `DATABASE_URL` (`just up`).

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
use stepclimb_auth::{
    encryption::KeyRing,
    outbound::{OutboundClient, UrlPolicy},
    token::ApiKey,
};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    organizations,
    perm::{AdmissionManage, Permission, ProviderManage},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    service_accounts::{self, NewServiceAccount},
    users::{self, NewUser},
};
use stepclimb_mock_idp::{Endpoint, MockIdp};
use stepclimb_server::{
    AppState, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;
use url::Url;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const PATH: &str = "/api/v1/admin/providers";
const SECRET: &str = "a-client-secret-only-the-test-knows";

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

fn every() -> Vec<&'static str> {
    stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect()
}

fn oidc(key: &str) -> Value {
    json!({
        "key": key,
        "kind": "oidc",
        "label": "Sign in with Example",
        "config": {
            "issuer": "https://idp.example.test/realm",
            "client_id": "stepclimb",
            "client_secret": SECRET,
        },
    })
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    text: String,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    #[track_caller]
    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "{self:?}");
        assert_eq!(self.code(), code, "{self:?}");
    }

    #[track_caller]
    fn ok(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }
}

/// How a request is authenticated.
#[derive(Clone, Copy)]
enum Auth<'a> {
    Cookie(&'a str),
    Key(&'a str),
    Anonymous,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
    mock: MockIdp,
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
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            // The mock listens on http loopback (spec 05 Q4's insecure switch).
            providers: stepclimb_server::providers::Providers::new(true).expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        let mock = MockIdp::start().await.expect("the mock starts");
        Self {
            pool,
            app,
            org,
            mock,
        }
    }

    async fn user(&self) -> Uuid {
        let new_user = NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id
    }

    /// A signed-in user holding a custom role with `keys` at organization scope: the `Cookie`
    /// value and the user's id.
    async fn holder(&self, keys: &[&str]) -> (String, Uuid) {
        let user = self.user().await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Holder".into(),
            permissions: keys.iter().map(|k| (*k).to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
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
        role_assignments::create(
            &self.pool,
            scope(self.org),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
        let started = session::start(&self.pool, scope(self.org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), user)
    }

    async fn admin(&self) -> String {
        self.holder(&every()).await.0
    }

    /// An API key of `owner` with `scopes`.
    async fn key(&self, owner: Uuid, scopes: &[&str]) -> String {
        let key = ApiKey::generate().expect("a key");
        let new = NewServiceAccount {
            name: "Suite key".into(),
            owner_user_id: owner,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        };
        service_accounts::create(
            &self.pool,
            scope(self.org),
            &new,
            key.hash().as_bytes(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("create service account");
        key.expose_secret().to_owned()
    }

    async fn send(&self, method: Method, uri: &str, auth: Auth<'_>, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin")
            .header(TEST_ORG_HEADER, self.org.to_string());
        request = match auth {
            Auth::Cookie(cookie) => request.header(header::COOKIE, cookie),
            Auth::Key(key) => request.header(header::AUTHORIZATION, format!("Bearer {key}")),
            Auth::Anonymous => request,
        };
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
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Answer {
            status,
            headers,
            body,
            text,
        }
    }

    async fn post(&self, cookie: &str, body: Value) -> Answer {
        self.send(Method::POST, PATH, Auth::Cookie(cookie), Some(body))
            .await
    }

    async fn patch(&self, cookie: &str, id: &str, body: Value) -> Answer {
        let uri = format!("{PATH}/{id}");
        self.send(Method::PATCH, &uri, Auth::Cookie(cookie), Some(body))
            .await
    }

    async fn get(&self, cookie: &str, uri: &str) -> Answer {
        self.send(Method::GET, uri, Auth::Cookie(cookie), None)
            .await
    }

    async fn delete(&self, cookie: &str, id: &str) -> Answer {
        let uri = format!("{PATH}/{id}");
        self.send(Method::DELETE, &uri, Auth::Cookie(cookie), None)
            .await
    }

    /// Created through the API by `cookie`: the provider's id.
    async fn created(&self, cookie: &str, body: Value) -> String {
        let answer = self.post(cookie, body).await;
        answer.ok(StatusCode::CREATED)["id"]
            .as_str()
            .expect("an id")
            .to_owned()
    }

    /// The stored config, as text.
    async fn stored(&self, id: &str) -> String {
        sqlx::query_scalar("SELECT config::text FROM identity_providers WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("the row")
    }

    async fn entries(&self) -> Vec<audit::Entry> {
        audit::list(&self.pool, scope(self.org), 100)
            .await
            .expect("read entries")
            .into_iter()
            .filter(|e| e.target_type == "identity_provider")
            .collect()
    }

    async fn signup(&self, allowed: bool) {
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

    async fn link(&self, provider: &str, subject: &str) {
        let identity = NewIdentity {
            user_id: self.user().await,
            provider_id: Uuid::parse_str(provider).expect("an id"),
            subject: subject.into(),
            attributes: serde_json::Map::new(),
        };
        identities::link(
            &self.pool,
            scope(self.org),
            &identity,
            now(),
            Actor::Instance,
        )
        .await
        .expect("link identity");
    }

    fn oauth2(&self, key: &str) -> Value {
        json!({
            "key": key,
            "kind": "oauth2",
            "label": "Sign in with the mock",
            "config": {
                "authorize": self.mock.url(Endpoint::OAuth2Authorize),
                "token": self.mock.url(Endpoint::OAuth2Token),
                "userinfo": self.mock.url(Endpoint::OAuth2Userinfo),
                "client_id": self.mock.client_id(),
                "client_secret": self.mock.client_secret(),
                "scopes": ["profile"],
                "claims": { "subject": "/data/cid" },
            },
        })
    }

    /// A full OAuth 2 sign-in through `key` against the mock: the callback's `Location`.
    async fn sign_in(&self, key: &str) -> String {
        let start = self
            .send_anonymous(&format!("/auth/{key}/start?return_to=/c/s1"), None)
            .await;
        assert_eq!(start.status, StatusCode::SEE_OTHER, "{start:?}");
        let set_cookie = start.headers[header::SET_COOKIE].to_str().expect("ASCII");
        let flow = set_cookie.split(';').next().expect("a pair").to_owned();
        let location = start.headers[header::LOCATION].to_str().expect("ASCII");
        let back = follow_at_mock(location).await;
        let query = back.query().expect("a query");
        let callback = self
            .send_anonymous(&format!("/auth/{key}/callback?{query}"), Some(&flow))
            .await;
        callback.headers[header::LOCATION]
            .to_str()
            .expect("ASCII")
            .to_owned()
    }

    async fn send_anonymous(&self, uri: &str, cookie: Option<&str>) -> Answer {
        let mut request = Request::get(uri).header(TEST_ORG_HEADER, self.org.to_string());
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
            body: Value::Null,
            text: String::new(),
        }
    }
}

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
    Url::parse(back).expect("an absolute redirect")
}

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

/// T11: create, read, list, change and delete a provider. Each answer has the documented shape,
/// the redirect URI and `client_secret_set`, never the secret; each write leaves one audit
/// entry by the caller, none holding the secret.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_provider_is_created_read_listed_changed_and_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f.holder(&every()).await;
    let created = f.post(&admin, oidc("corp-sso")).await;
    let body = created.ok(StatusCode::CREATED).clone();
    let id = body["id"].as_str().expect("an id").to_owned();
    assert_eq!(created.headers[header::LOCATION], format!("{PATH}/{id}"));
    assert_schema("Provider", &body);
    assert_eq!(body["key"], "corp-sso");
    assert_eq!(body["kind"], "oidc");
    assert_eq!(body["label"], "Sign in with Example");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["source"], "api");
    assert_eq!(body["client_secret_set"], true);
    assert_eq!(
        body["redirect_uri"],
        "https://lms.example.org/auth/corp-sso/callback"
    );
    assert_eq!(body["config"]["issuer"], "https://idp.example.test/realm");
    assert_eq!(
        body["config"]["scopes"],
        json!(["openid", "email", "profile"])
    );

    let read = f.get(&admin, &format!("{PATH}/{id}")).await;
    assert_eq!(read.ok(StatusCode::OK), &body);
    let list = f.get(&admin, PATH).await;
    assert_eq!(list.ok(StatusCode::OK)["items"], json!([body]));

    let changed = f
        .patch(
            &admin,
            &id,
            json!({ "label": "Corporate SSO", "enabled": false }),
        )
        .await;
    let changed = changed.ok(StatusCode::OK);
    assert_schema("Provider", changed);
    assert_eq!(
        (changed["label"].clone(), changed["enabled"].clone()),
        (json!("Corporate SSO"), json!(false))
    );

    f.delete(&admin, &id).await.ok(StatusCode::NO_CONTENT);
    f.get(&admin, &format!("{PATH}/{id}"))
        .await
        .expect(StatusCode::NOT_FOUND, "provider.not_found");

    let entries = f.entries().await;
    let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        [
            "identity_provider.create",
            "identity_provider.update",
            "identity_provider.delete"
        ]
    );
    for entry in &entries {
        assert_eq!(entry.target_id.to_string(), id);
        assert_eq!(
            entry.actor,
            Actor::User {
                id: admin_id,
                ip: CLIENT.ip()
            }
        );
        assert!(!entry.data.to_string().contains(SECRET), "{entry:?}");
        assert!(!entry.data.to_string().contains("sealed"), "{entry:?}");
    }
    assert_eq!(entries[0].data["after"]["key"], "corp-sso");
    assert_eq!(entries[0].data["after"]["config"]["client_id"], "stepclimb");
    assert_eq!(
        entries[1].data,
        json!({
            "before": { "label": "Sign in with Example", "enabled": true },
            "after": { "label": "Corporate SSO", "enabled": false },
        })
    );
    assert_eq!(entries[2].data["before"]["label"], "Corporate SSO");
}

/// T11, B14: the secret is in no answer, and the row holds it only sealed. A change naming a
/// new secret replaces it, recorded as `client_secret_replaced` without either value.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_no_answer_and_no_stored_text_holds_the_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let created = f.post(&admin, oidc("corp-sso")).await;
    assert!(!created.text.contains(SECRET), "{}", created.text);
    let id = created.body["id"].as_str().expect("an id").to_owned();
    let stored = f.stored(&id).await;
    assert!(!stored.contains(SECRET), "{stored}");
    assert!(stored.contains("sealed"), "{stored}");
    for uri in [PATH.to_owned(), format!("{PATH}/{id}")] {
        let answer = f.get(&admin, &uri).await;
        assert!(!answer.text.contains(SECRET) && !answer.text.contains("sealed"));
    }
    let new_secret = "another-secret-only-the-test-knows";
    let changed = f
        .patch(
            &admin,
            &id,
            json!({ "config": { "client_secret": new_secret } }),
        )
        .await;
    changed.ok(StatusCode::OK);
    assert!(!changed.text.contains(new_secret));
    assert_ne!(f.stored(&id).await, stored, "a new sealed secret");
    let entry = f.entries().await.pop().expect("the update");
    assert_eq!(entry.data["client_secret_replaced"], true);
    let data = entry.data.to_string();
    assert!(
        !data.contains(new_secret) && !data.contains("sealed"),
        "{data}"
    );
}

/// T11: a change without `client_secret` keeps the stored secret, so a sign-in after it still
/// authenticates at the mock's token endpoint. The control: a change to a wrong secret makes
/// the same sign-in fail there.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_change_without_the_secret_keeps_it_for_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.mock
        .set_oauth2_userinfo(Some(json!({ "data": { "cid": 1000001 } })));
    let admin = f.admin().await;
    let id = f.created(&admin, f.oauth2("mock")).await;
    f.signup(true).await;
    let scopes = json!({ "config": { "scopes": ["profile", "email"] } });
    let changed = f.patch(&admin, &id, scopes).await;
    let changed = changed.ok(StatusCode::OK);
    assert_schema("Provider", changed);
    assert_eq!(changed["config"]["scopes"], json!(["profile", "email"]));
    assert_eq!(f.sign_in("mock").await, "/c/s1");

    let wrong = json!({ "config": { "client_secret": "not-the-mocks-secret" } });
    f.patch(&admin, &id, wrong).await.ok(StatusCode::OK);
    assert_eq!(
        f.sign_in("mock").await,
        "/sign-in?error=auth.provider_error"
    );
}

/// B14: a config breaking B4 or `05-oidc.md` B1 answers 422 `provider.config_invalid` with a
/// pointer under `/config`; the URL policy of the instance as it is now applies (Q4: a private
/// address only with one organization).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b14_an_invalid_config_is_refused_naming_the_member(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let mut body = f.oauth2("plain");
    body["config"]["claims"]["subject"] = json!("data/cid");
    body["config"].as_object_mut().unwrap().remove("token");
    let answer = f.post(&admin, body).await;
    answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "provider.config_invalid");
    let fields: Vec<&str> = answer.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["field"].as_str().unwrap())
        .collect();
    assert!(fields.contains(&"/config/token"), "{fields:?}");
    assert!(fields.contains(&"/config/claims/subject"), "{fields:?}");

    let mut private = oidc("private");
    private["config"]["issuer"] = json!("https://10.1.2.3/realm");
    f.post(&admin, private.clone())
        .await
        .ok(StatusCode::CREATED);
    organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("a second organization");
    private["key"] = json!("private-two");
    let refused = f.post(&admin, private).await;
    refused.expect(StatusCode::UNPROCESSABLE_ENTITY, "provider.config_invalid");
    assert_eq!(
        refused.body["errors"],
        json!([{ "field": "/config/issuer", "code": "provider.url_refused" }])
    );
    assert_eq!(f.entries().await.len(), 1, "only the first provider");
}

/// B15, Q5 (a): a key another provider has answers 409; a key outside the rule, `local` and the
/// dot segments among them, 422 naming `/key`. The table refuses such a key too.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_keys_are_unique_and_follow_q5(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.created(&admin, oidc("corp-sso")).await;
    f.post(&admin, oidc("corp-sso"))
        .await
        .expect(StatusCode::CONFLICT, "provider.key_taken");
    for key in [
        "local",
        ".",
        "..",
        "Corp",
        "corp_sso",
        "-corp",
        &"k".repeat(33),
    ] {
        let answer = f.post(&admin, oidc(key)).await;
        answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        assert_eq!(
            answer.body["errors"],
            json!([{ "field": "/key", "code": "provider.key_invalid" }]),
            "{key:?}"
        );
    }
    for key in [".", "..", "Corp", "a/b", &"k".repeat(33)] {
        let refused = sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config)
             VALUES ($1, $2, $3, 'oidc', '{}')",
        )
        .bind(new_id())
        .bind(f.org)
        .bind(key)
        .execute(&f.pool)
        .await
        .expect_err("the check refuses it");
        let constraint = refused.as_database_error().and_then(|e| e.constraint());
        assert_eq!(constraint, Some("identity_providers_key_format"), "{key:?}");
    }
    let mut label = oidc("labelled");
    label["label"] = json!("line\nbreak");
    let answer = f.post(&admin, label).await;
    assert_eq!(
        answer.body["errors"],
        json!([{ "field": "/label", "code": "provider.label_invalid" }])
    );
}

/// T12, Q5 (a), B16: deleting a provider identities link users to answers 409
/// `provider.in_use` and keeps it; disabling it keeps the identities and makes its start route
/// answer `auth.provider_not_found`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_a_provider_in_use_is_disabled_not_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let id = f.created(&admin, oidc("corp-sso")).await;
    f.link(&id, "1000001").await;
    f.delete(&admin, &id)
        .await
        .expect(StatusCode::CONFLICT, "provider.in_use");
    f.patch(&admin, &id, json!({ "enabled": false }))
        .await
        .ok(StatusCode::OK);
    let identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE provider_id = $1::uuid")
            .bind(&id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(identities, 1);
    let start = f.send_anonymous("/auth/corp-sso/start", None).await;
    assert_eq!(start.status, StatusCode::SEE_OTHER);
    assert_eq!(
        start.headers[header::LOCATION],
        "/sign-in?error=auth.provider_not_found"
    );
}

/// T13, B17: `local` and `saml` are not kinds this API creates, and the local row is not one
/// of its providers: reading, changing or deleting it by id answers 404.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_the_local_kind_and_row_are_not_this_apis(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    for kind in ["local", "saml"] {
        let mut body = oidc("other");
        body["kind"] = json!(kind);
        f.post(&admin, body).await.expect(
            StatusCode::UNPROCESSABLE_ENTITY,
            "provider.kind_unsupported",
        );
    }
    let local: Uuid = sqlx::query_scalar("SELECT id FROM identity_providers WHERE key = 'local'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let local = local.to_string();
    f.get(&admin, &format!("{PATH}/{local}"))
        .await
        .expect(StatusCode::NOT_FOUND, "provider.not_found");
    f.patch(&admin, &local, json!({ "enabled": false }))
        .await
        .expect(StatusCode::NOT_FOUND, "provider.not_found");
    f.delete(&admin, &local)
        .await
        .expect(StatusCode::NOT_FOUND, "provider.not_found");
    let list = f.get(&admin, PATH).await;
    assert_eq!(list.ok(StatusCode::OK)["items"], json!([]));
}

/// Q2 (a), T10: a row `stepclimb.toml` declares shows `source: "config"`, and changing or
/// deleting it answers 409 `provider.config_owned`, writing nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_a_provider_of_the_file_is_read_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, 'file-sso', 'oidc', $3)",
    )
    .bind(id)
    .bind(f.org)
    .bind(json!({
        "issuer": "https://idp.example.test/file",
        "client_id": "file",
        "label": "From the file",
        "source": "config",
    }))
    .execute(&f.pool)
    .await
    .unwrap();
    let read = f.get(&admin, &format!("{PATH}/{id}")).await;
    let read = read.ok(StatusCode::OK);
    assert_eq!(
        (read["source"].clone(), read["client_secret_set"].clone()),
        (json!("config"), json!(true))
    );
    let before = f.stored(&id.to_string()).await;
    f.patch(&admin, &id.to_string(), json!({ "label": "Changed" }))
        .await
        .expect(StatusCode::CONFLICT, "provider.config_owned");
    f.delete(&admin, &id.to_string())
        .await
        .expect(StatusCode::CONFLICT, "provider.config_owned");
    assert_eq!(f.stored(&id.to_string()).await, before);
    assert!(f.entries().await.is_empty());
}

/// Spec 07 I3: a config change keeps the provider's admission rules, which only the admission
/// API writes, and the answer does not show them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_keeps_the_admission_rules(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let id = f.created(&admin, oidc("corp-sso")).await;
    let rules = json!([{ "attribute": "team", "op": "exists" }]);
    sqlx::query(
        "UPDATE identity_providers SET config = config || jsonb_build_object('admission_rules', $2::jsonb)
         WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(&rules)
    .execute(&f.pool)
    .await
    .unwrap();
    let changed = f
        .patch(
            &admin,
            &id,
            json!({ "label": "Relabelled", "config": { "userinfo": false } }),
        )
        .await;
    assert!(
        !changed.text.contains("admission_rules"),
        "{}",
        changed.text
    );
    assert_eq!(changed.ok(StatusCode::OK)["config"]["userinfo"], false);
    let stored: Value = serde_json::from_str(&f.stored(&id).await).unwrap();
    assert_eq!(stored["admission_rules"], rules);
}

/// Spec 09 B22: a change naming only current values writes nothing: no audit entry, the same
/// `updated_at`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_to_the_current_values_writes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let id = f.created(&admin, oidc("corp-sso")).await;
    let updated_at = || async {
        let at: i64 = sqlx::query_scalar(
            "SELECT (extract(epoch FROM updated_at) * 1000000)::bigint FROM identity_providers WHERE id = $1::uuid",
        )
        .bind(&id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        at
    };
    let before = updated_at().await;
    let same = json!({
        "label": "Sign in with Example",
        "enabled": true,
        "config": { "issuer": "https://idp.example.test/realm", "scopes": ["openid", "email", "profile"] },
    });
    f.patch(&admin, &id, same).await.ok(StatusCode::OK);
    f.patch(&admin, &id, json!({})).await.ok(StatusCode::OK);
    assert_eq!(updated_at().await, before);
    assert_eq!(f.entries().await.len(), 1, "the create only");
}

/// The list pages by key.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_pages_by_key(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    for key in ["c-sso", "a-sso", "b-sso"] {
        f.created(&admin, oidc(key)).await;
    }
    let first = f.get(&admin, &format!("{PATH}?limit=2")).await;
    let first = first.ok(StatusCode::OK).clone();
    let keys = |page: &Value| -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["key"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(keys(&first), ["a-sso", "b-sso"]);
    let cursor = first["next_cursor"].as_str().expect("a next page");
    let second = f
        .get(&admin, &format!("{PATH}?limit=2&cursor={cursor}"))
        .await;
    let second = second.ok(StatusCode::OK);
    assert_eq!(keys(second), ["c-sso"]);
    assert_eq!(second["next_cursor"], Value::Null);
}

/// Coordinator decision on #37 (from #178): with sign-up on, creating or turning on a provider
/// also needs `admission.manage`; a disabled one, or sign-up off, needs `provider.manage`
/// alone.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn with_sign_up_on_turning_a_provider_on_needs_admission_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    f.signup(true).await;
    f.post(&manager, oidc("on"))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let mut off = oidc("off");
    off["enabled"] = json!(false);
    let id = f.created(&manager, off).await;
    f.patch(&manager, &id, json!({ "enabled": true }))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.patch(&manager, &id, json!({ "label": "Still off" }))
        .await
        .ok(StatusCode::OK);
    f.patch(&both, &id, json!({ "enabled": true }))
        .await
        .ok(StatusCode::OK);
    let issuer = json!({ "config": { "issuer": "https://other.example.test/realm" } });
    f.patch(&manager, &id, issuer.clone())
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.patch(&manager, &id, json!({ "label": "Relabelled while on" }))
        .await
        .ok(StatusCode::OK);
    f.created(&both, oidc("on")).await;

    f.signup(false).await;
    f.created(&manager, oidc("closed")).await;
    f.patch(&manager, &id, issuer).await.ok(StatusCode::OK);
}

/// Changing what a provider identities link users to is (its issuer, endpoint origins or
/// subject pointer) would sign those users in as whoever the new provider says: it needs every
/// permission. Without identities, or for its label, `provider.manage` is enough.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_what_a_linked_provider_is_needs_every_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let admin = f.admin().await;
    let id = f.created(&manager, f.oauth2("plain")).await;
    let other = json!({ "config": { "userinfo": "https://other.example.test/api/user" } });
    let subject = json!({ "config": { "claims": { "subject": "/data/id" } } });
    f.patch(&manager, &id, subject.clone())
        .await
        .ok(StatusCode::OK);
    f.link(&id, "1000001").await;
    for body in [
        other.clone(),
        json!({ "config": { "claims": { "subject": "/data/cid" } } }),
    ] {
        f.patch(&manager, &id, body)
            .await
            .expect(StatusCode::FORBIDDEN, "permission.escalation");
    }
    let path_only = json!({ "config": { "scopes": ["profile", "email"] } });
    f.patch(&manager, &id, path_only).await.ok(StatusCode::OK);
    f.patch(&admin, &id, other).await.ok(StatusCode::OK);
}

/// Spec 04 B2's rule from the providers' side: disabling or deleting the organization's last
/// way to sign in answers 409 `local.last_method`; with another provider enabled it goes ahead.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_last_way_to_sign_in_cannot_be_disabled_or_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let first = f.created(&admin, oidc("first")).await;
    sqlx::query("UPDATE identity_providers SET enabled = false WHERE key = 'local'")
        .execute(&f.pool)
        .await
        .unwrap();
    f.patch(&admin, &first, json!({ "enabled": false }))
        .await
        .expect(StatusCode::CONFLICT, "local.last_method");
    f.delete(&admin, &first)
        .await
        .expect(StatusCode::CONFLICT, "local.last_method");
    let second = f.created(&admin, oidc("second")).await;
    f.patch(&admin, &first, json!({ "enabled": false }))
        .await
        .ok(StatusCode::OK);
    f.delete(&admin, &first).await.ok(StatusCode::NO_CONTENT);
    f.delete(&admin, &second)
        .await
        .expect(StatusCode::CONFLICT, "local.last_method");
}

/// `05-oidc.md` Q1 (a): a provider's kind needs its feature to be created or changed; reading
/// and deleting need none. With `oauth2_providers` off, `oidc` providers are unaffected.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_kind_needs_its_own_feature_to_be_written(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let plain = f.created(&admin, f.oauth2("plain")).await;
    let sso = f.created(&admin, oidc("sso")).await;
    let plan = new_id();
    let features: serde_json::Map<String, Value> = stepclimb_core::feature::ALL
        .iter()
        .map(|key| ((*key).to_owned(), Value::Bool(*key != "oauth2_providers")))
        .collect();
    let limits: serde_json::Map<String, Value> = stepclimb_core::feature::Limit::ALL
        .iter()
        .map(|limit| (limit.key().to_owned(), Value::Null))
        .collect();
    sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, 'no_oauth2', $2, $3)")
        .bind(plan)
        .bind(Value::Object(features))
        .bind(Value::Object(limits))
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(f.org)
        .bind(plan)
        .execute(&f.pool)
        .await
        .unwrap();
    let unavailable = "entitlement.feature_unavailable";
    f.post(&admin, f.oauth2("again"))
        .await
        .expect(StatusCode::FORBIDDEN, unavailable);
    f.patch(&admin, &plain, json!({ "label": "No" }))
        .await
        .expect(StatusCode::FORBIDDEN, unavailable);
    f.get(&admin, &format!("{PATH}/{plain}"))
        .await
        .ok(StatusCode::OK);
    f.patch(&admin, &sso, json!({ "label": "Yes" }))
        .await
        .ok(StatusCode::OK);
    f.created(&admin, oidc("more")).await;
    f.delete(&admin, &plain).await.ok(StatusCode::NO_CONTENT);
}

/// Spec 08 B27: an API key acts with its owner's grants narrowed to its scopes, so a key
/// scoped to `provider.manage` alone, of an owner holding every permission, is refused what
/// needs every permission, and a key with every scope is not.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_rules_read_a_keys_narrowed_grants(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, owner) = f.holder(&every()).await;
    let narrow = f.key(owner, &[ProviderManage::KEY]).await;
    let full = f.key(owner, &every()).await;
    let id = f
        .send(
            Method::POST,
            PATH,
            Auth::Key(&narrow),
            Some(f.oauth2("plain")),
        )
        .await
        .ok(StatusCode::CREATED)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    f.link(&id, "1000001").await;
    let uri = format!("{PATH}/{id}");
    let other = json!({ "config": { "userinfo": "https://other.example.test/api/user" } });
    f.send(Method::PATCH, &uri, Auth::Key(&narrow), Some(other.clone()))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.escalation");
    f.send(Method::PATCH, &uri, Auth::Key(&full), Some(other))
        .await
        .ok(StatusCode::OK);
    f.signup(true).await;
    f.send(Method::POST, PATH, Auth::Key(&narrow), Some(oidc("on")))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.send(Method::POST, PATH, Auth::Key(&full), Some(oidc("on")))
        .await
        .ok(StatusCode::CREATED);
}

fn trusted(mut body: Value) -> Value {
    body["config"]["trusted_for_email_matching"] = json!(true);
    body
}

const ESCALATION: &str = "permission.escalation";

/// The #37 rule from the review of PR #199: creating a provider trusted for email matching,
/// turning trust on, or changing `claims.email`, `claims.email_verified` or the issuer of a
/// trusted provider needs every permission. Anything else on a trusted provider, and turning
/// trust off, needs `provider.manage` alone. A refused write leaves no row and no entry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn trust_for_email_matching_needs_every_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let admin = f.admin().await;
    f.post(&manager, trusted(oidc("trusted")))
        .await
        .expect(StatusCode::FORBIDDEN, ESCALATION);
    f.post(&manager, trusted(f.oauth2("trusted")))
        .await
        .expect(StatusCode::FORBIDDEN, ESCALATION);
    assert!(f.entries().await.is_empty(), "nothing was created");

    let id = f.created(&manager, oidc("sso")).await;
    let trust = json!({ "config": { "trusted_for_email_matching": true } });
    f.patch(&manager, &id, trust.clone())
        .await
        .expect(StatusCode::FORBIDDEN, ESCALATION);
    let answer = f.patch(&admin, &id, trust).await;
    let answer = answer.ok(StatusCode::OK);
    assert_schema("Provider", answer);
    assert_eq!(answer["config"]["trusted_for_email_matching"], true);

    for change in [
        json!({ "config": { "claims": { "email": "/mail" } } }),
        json!({ "config": { "claims": { "email_verified": "/mail_verified" } } }),
        json!({ "config": { "issuer": "https://other.example.test/realm" } }),
    ] {
        f.patch(&manager, &id, change.clone())
            .await
            .expect(StatusCode::FORBIDDEN, ESCALATION);
        f.patch(&admin, &id, change).await.ok(StatusCode::OK);
        let back = json!({ "config": {
            "issuer": "https://idp.example.test/realm",
            "claims": {},
        } });
        f.patch(&admin, &id, back).await.ok(StatusCode::OK);
    }
    f.patch(&manager, &id, json!({ "label": "Still trusted" }))
        .await
        .ok(StatusCode::OK);
    f.patch(
        &manager,
        &id,
        json!({ "config": { "scopes": ["openid", "email"] } }),
    )
    .await
    .ok(StatusCode::OK);
    let untrust = json!({ "config": { "trusted_for_email_matching": false } });
    f.patch(&manager, &id, untrust).await.ok(StatusCode::OK);
    let moved = json!({ "config": { "issuer": "https://other.example.test/realm" } });
    f.patch(&manager, &id, moved).await.ok(StatusCode::OK);
    f.created(&admin, trusted(f.oauth2("trusted"))).await;
}

/// The trust rule reads an API key's narrowed grants (spec 08 B27): a key scoped to
/// `provider.manage` alone, whose owner holds every permission, can neither create a trusted
/// provider nor change a trusted provider's email claims; a key with every scope can.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_trust_rule_reads_a_keys_narrowed_grants(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, owner) = f.holder(&every()).await;
    let narrow = f.key(owner, &[ProviderManage::KEY]).await;
    let full = f.key(owner, &every()).await;
    let body = trusted(oidc("trusted"));
    f.send(Method::POST, PATH, Auth::Key(&narrow), Some(body.clone()))
        .await
        .expect(StatusCode::FORBIDDEN, ESCALATION);
    let id = f
        .send(Method::POST, PATH, Auth::Key(&full), Some(body))
        .await
        .ok(StatusCode::CREATED)["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let uri = format!("{PATH}/{id}");
    let claims = json!({ "config": { "claims": { "email": "/mail" } } });
    f.send(
        Method::PATCH,
        &uri,
        Auth::Key(&narrow),
        Some(claims.clone()),
    )
    .await
    .expect(StatusCode::FORBIDDEN, ESCALATION);
    f.send(Method::PATCH, &uri, Auth::Key(&full), Some(claims))
        .await
        .ok(StatusCode::OK);
}

/// One host can serve another account's user endpoint under another path: on a provider that
/// identities link users to, a same-origin change of the token or userinfo URL's path or query
/// is a change of what the provider is and needs every permission.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_same_origin_endpoint_change_of_a_linked_provider_needs_every_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let admin = f.admin().await;
    let id = f.created(&manager, f.oauth2("plain")).await;
    f.link(&id, "1000001").await;
    let userinfo = Url::parse(&f.mock.url(Endpoint::OAuth2Userinfo)).expect("a URL");
    let token = Url::parse(&f.mock.url(Endpoint::OAuth2Token)).expect("a URL");
    let elsewhere = |url: &Url, path: &str| url.join(path).expect("a URL").to_string();
    let changes = [
        ("userinfo", elsewhere(&userinfo, "/accounts/another-user")),
        ("userinfo", format!("{userinfo}?account=another")),
        ("token", elsewhere(&token, "/another/token")),
    ];
    let before = f.stored(&id).await;
    for (member, url) in &changes {
        let body = json!({ "config": { *member: url } });
        f.patch(&manager, &id, body)
            .await
            .expect(StatusCode::FORBIDDEN, ESCALATION);
    }
    assert_eq!(f.stored(&id).await, before);
    let (member, url) = &changes[0];
    let changed = f
        .patch(&admin, &id, json!({ "config": { *member: url } }))
        .await;
    assert_eq!(changed.ok(StatusCode::OK)["config"]["userinfo"], json!(url));
}

/// Sets the organization's own admission rules, as `PUT /api/v1/admin/admission` stores them.
async fn organization_rules(pool: &PgPool, org: Uuid, rules: &Value) {
    sqlx::query(
        "UPDATE organizations
         SET settings = settings || jsonb_build_object('admission',
                            COALESCE(settings -> 'admission', '{}'::jsonb)
                            || jsonb_build_object('rules', $2::jsonb))
         WHERE id = $1",
    )
    .bind(org)
    .bind(rules)
    .execute(pool)
    .await
    .expect("set the organization's rules");
}

fn mapping(attributes: Value) -> Value {
    json!({ "config": { "claims": { "attributes": attributes } } })
}

/// An admission rule reads an attribute from where the provider maps it, so mapping a tested
/// attribute differently needs `admission.manage` as well as `provider.manage`, and removing
/// it answers 422. The four cases (anonymous 401, `provider.manage` alone 403, another
/// organization's provider 404, both permissions 200) and a removal; no refused write changes
/// the row.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn mapping_an_attribute_a_rule_tests_needs_admission_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    let mut body = oidc("corp-sso");
    body["config"]["claims"] = json!({ "attributes": { "rating": "/rating", "team": "/team" } });
    body["enabled"] = json!(false);
    let id = f.created(&manager, body).await;
    let rules = json!([{ "attribute": "rating", "op": "in", "value": ["S3", "C1"] }]);
    organization_rules(&f.pool, f.org, &rules).await;
    let before = f.stored(&id).await;

    let remap = mapping(json!({ "rating": "/always-c1", "team": "/team" }));
    let uri = format!("{PATH}/{id}");
    f.send(Method::PATCH, &uri, Auth::Anonymous, Some(remap.clone()))
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    f.patch(&manager, &id, remap.clone())
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    assert_eq!(f.stored(&id).await, before);

    let untested = mapping(json!({ "rating": "/rating", "team": "/squad", "region": "/r" }));
    f.patch(&manager, &id, untested).await.ok(StatusCode::OK);
    let changed = f.patch(&both, &id, remap).await;
    assert_eq!(
        changed.ok(StatusCode::OK)["config"]["claims"]["attributes"]["rating"],
        "/always-c1"
    );

    let after_remap = f.stored(&id).await;
    let removed = f
        .patch(&both, &id, mapping(json!({ "team": "/squad" })))
        .await;
    removed.expect(StatusCode::UNPROCESSABLE_ENTITY, "provider.config_invalid");
    assert_eq!(
        removed.body["errors"],
        json!([{
            "field": "/config/claims/attributes/rating",
            "code": "provider.attribute_in_use",
        }])
    );
    assert_eq!(f.stored(&id).await, after_remap);
}

/// A provider's own admission rules protect its attributes the same way.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_providers_own_rules_protect_its_attributes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let mut body = oidc("corp-sso");
    body["config"]["claims"] = json!({ "attributes": { "team": "/team" } });
    let id = f.created(&manager, body).await;
    sqlx::query(
        "UPDATE identity_providers
         SET config = config || jsonb_build_object('admission_rules', $2::jsonb)
         WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(json!([{ "attribute": "team", "op": "exists" }]))
    .execute(&f.pool)
    .await
    .expect("set the provider's rules");
    f.patch(&manager, &id, mapping(json!({ "team": "/everyone" })))
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    f.patch(&manager, &id, mapping(json!({})))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY, "provider.config_invalid");
}

/// Another organization's provider is not found, whatever its rules, and stays as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_provider_mapping_is_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    let other = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization")
        .id;
    let config = json!({
        "label": "Org B SSO",
        "issuer": "https://idp.example.test/org-b",
        "client_id": "org-b",
        "claims": { "attributes": { "rating": "/rating" } },
    });
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, 'org-b-sso', 'oidc', $3::jsonb) RETURNING id",
    )
    .bind(new_id())
    .bind(other)
    .bind(&config)
    .fetch_one(&f.pool)
    .await
    .expect("insert org B's provider");
    let rules = json!([{ "attribute": "rating", "op": "exists" }]);
    organization_rules(&f.pool, other, &rules).await;
    let before = f.stored(&id.to_string()).await;
    for body in [mapping(json!({ "rating": "/always" })), mapping(json!({}))] {
        f.patch(&both, &id.to_string(), body)
            .await
            .expect(StatusCode::NOT_FOUND, "provider.not_found");
    }
    assert_eq!(f.stored(&id.to_string()).await, before);
}

/// While the provider maps an attribute a rule tests, its client, scopes, user endpoint flag
/// and issuer decide which document the attribute is read from: changing one needs
/// `admission.manage` as well.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_where_a_tested_attribute_is_read_needs_admission_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    let mut body = oidc("corp-sso");
    body["config"]["claims"] = json!({ "attributes": { "rating": "/rating" } });
    let id = f.created(&manager, body).await;
    let rules = json!([{ "attribute": "rating", "op": "exists" }]);
    organization_rules(&f.pool, f.org, &rules).await;
    let before = f.stored(&id).await;
    let changes = [
        json!({ "config": { "client_id": "another-client" } }),
        json!({ "config": { "scopes": ["openid", "profile", "roles"] } }),
        json!({ "config": { "userinfo": false } }),
        json!({ "config": { "issuer": "https://other.example.test/realm" } }),
    ];
    for change in &changes {
        f.patch(&manager, &id, change.clone())
            .await
            .expect(StatusCode::FORBIDDEN, "permission.denied");
    }
    assert_eq!(f.stored(&id).await, before);
    for change in changes {
        f.patch(&both, &id, change).await.ok(StatusCode::OK);
    }
}

/// On an `oauth2` provider no identity links, the token URL decides whose access token the
/// user endpoint answers to: while it maps an attribute a rule tests, moving the token URL,
/// even to another path on the same host, needs `admission.manage` as well. Refused, the row
/// stays as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_a_token_url_under_a_tested_attribute_needs_admission_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    let mut body = f.oauth2("plain");
    body["config"]["claims"]["attributes"] = json!({ "rating": "/data/rating" });
    let id = f.created(&manager, body).await;
    let rules = json!([{ "attribute": "rating", "op": "exists" }]);
    organization_rules(&f.pool, f.org, &rules).await;
    let token = Url::parse(&f.mock.url(Endpoint::OAuth2Token)).expect("a URL");
    let moved = token.join("/another/token").expect("a URL").to_string();
    let change = json!({ "config": { "token": moved } });
    let before = f.stored(&id).await;
    f.patch(&manager, &id, change.clone())
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    assert_eq!(f.stored(&id).await, before);
    let changed = f.patch(&both, &id, change).await;
    assert_eq!(changed.ok(StatusCode::OK)["config"]["token"], json!(moved));
}

/// A new provider mapping an attribute the organization's rules test needs
/// `admission.manage` as well; refused, it leaves no row.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn creating_a_provider_that_maps_a_tested_attribute_needs_admission_manage(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder(&[ProviderManage::KEY]).await;
    let (both, _) = f.holder(&[ProviderManage::KEY, AdmissionManage::KEY]).await;
    let rules = json!([{ "attribute": "rating", "op": "exists" }]);
    organization_rules(&f.pool, f.org, &rules).await;
    let mut body = oidc("corp-sso");
    body["config"]["claims"] = json!({ "attributes": { "rating": "/rating" } });
    f.post(&manager, body.clone())
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity_providers WHERE org_id = $1 AND key = 'corp-sso'",
    )
    .bind(f.org)
    .fetch_one(&f.pool)
    .await
    .expect("count rows");
    assert_eq!(rows, 0);
    assert!(f.entries().await.is_empty());
    f.post(&both, body).await.ok(StatusCode::CREATED);
}
