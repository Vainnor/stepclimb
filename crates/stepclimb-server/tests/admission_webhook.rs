// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission webhook routes over the router (docs/specs/m0/07-admission.md B17, B24, Q7;
//! T18, and T19 and T20 for `/api/v1/admin/admission/webhook*`). The permission suite's 401,
//! 403, feature-off, 2xx and cross-organization cases for the four operations are in
//! `tests/permissions.rs`; these tests cover the secret shown once and stored sealed, rotation,
//! the escalation rule, the URL checks under the outbound URL policy, the partial body, the
//! audit entries, the log, an API key's narrowed rights, and that one organization's writes
//! leave another's webhook alone.
//!
//! Requests name their organization with the `test-support` header of `request_org`, since some
//! tests have two organizations. `session::start` stands in for the sign-in specs. All data is
//! synthetic. Needs `DATABASE_URL` (`just up`).

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
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, token::ApiKey};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
    perm::{self, AdmissionManage, Permission},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    service_accounts::{self, NewServiceAccount},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, admission_webhook::secret_context, request_org::TEST_ORG_HEADER, router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const PATH: &str = "/api/v1/admin/admission/webhook";
const ROTATE: &str = "/api/v1/admin/admission/webhook/rotate-secret";
const URL: &str = "https://hooks.example.test/admission";
const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

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

fn every_key() -> Vec<&'static str> {
    perm::ALL.iter().map(|p| p.key()).collect()
}

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: Value,
    text: String,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    fn secret(&self) -> String {
        self.body["secret"].as_str().expect("a secret").to_owned()
    }
}

/// The bytes a `whsec_` secret carries.
fn secret_bytes(secret: &str) -> Vec<u8> {
    let encoded = secret.strip_prefix("whsec_").expect("the whsec_ prefix");
    STANDARD.decode(encoded).expect("base64")
}

enum Auth<'a> {
    Cookie(&'a str),
    Key(&'a str),
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
    keys: KeyRing,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let keys = KeyRing::parse(KEYS).expect("a valid test key");
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: keys.clone(),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage: None,
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            app,
            org,
            keys,
        }
    }

    /// A user of `org` holding a custom role with `keys` at organization scope.
    async fn user_holding(&self, org: Uuid, keys: &[&str]) -> Uuid {
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
        let user = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
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

    /// A session `Cookie` value of a new user of `org` holding `keys`, and the user's id.
    async fn holder_in(&self, org: Uuid, keys: &[&str]) -> (String, Uuid) {
        let user = self.user_holding(org, keys).await;
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), user)
    }

    /// [`Fixture::holder_in`] org A, holding every catalog permission.
    async fn admin(&self) -> String {
        self.holder_in(self.org, &every_key()).await.0
    }

    /// An API key with `scopes`, owned by a new user of org A holding every permission.
    async fn key(&self, scopes: &[&str]) -> String {
        let owner = self.user_holding(self.org, &every_key()).await;
        let key = ApiKey::generate().expect("a key");
        let account = NewServiceAccount {
            name: "Automation".into(),
            owner_user_id: owner,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        };
        service_accounts::create(
            &self.pool,
            scope(self.org),
            &account,
            key.hash().as_bytes(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("create service account");
        key.expose_secret().to_owned()
    }

    async fn send_in(
        &self,
        org: Uuid,
        method: Method,
        uri: &str,
        auth: Auth<'_>,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin")
            .header(TEST_ORG_HEADER, org.to_string());
        request = match auth {
            Auth::Cookie(cookie) => request.header(header::COOKIE, cookie),
            Auth::Key(key) => request.header(header::AUTHORIZATION, format!("Bearer {key}")),
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
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8");
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("a JSON body")
        };
        Answer { status, body, text }
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        self.send_in(self.org, method, uri, Auth::Cookie(cookie), body)
            .await
    }

    async fn get(&self, cookie: &str) -> Answer {
        self.send(Method::GET, PATH, cookie, None).await
    }

    async fn put(&self, cookie: &str, body: Value) -> Answer {
        self.send(Method::PUT, PATH, cookie, Some(body)).await
    }

    async fn rotate(&self, cookie: &str) -> Answer {
        self.send(Method::POST, ROTATE, cookie, None).await
    }

    async fn delete(&self, cookie: &str) -> Answer {
        self.send(Method::DELETE, PATH, cookie, None).await
    }

    /// Org `org`'s stored row (`id`, `url`, `enabled`, `secret`, `updated_at`), if any.
    async fn row(&self, org: Uuid) -> Option<(Uuid, String, bool, Vec<u8>, String)> {
        sqlx::query_as(
            "SELECT id, url, enabled, secret, updated_at::text FROM admission_webhooks
             WHERE org_id = $1",
        )
        .bind(org)
        .fetch_optional(&self.pool)
        .await
        .expect("read the webhook")
    }

    /// Point org A at a new plan with every catalog feature but `without`.
    async fn plan_without(&self, without: &str) {
        let features: serde_json::Map<String, Value> = stepclimb_core::feature::ALL
            .iter()
            .map(|key| ((*key).to_owned(), Value::Bool(*key != without)))
            .collect();
        let limits: serde_json::Map<String, Value> = stepclimb_core::feature::Limit::ALL
            .iter()
            .map(|limit| (limit.key().to_owned(), Value::Null))
            .collect();
        let plan = new_id();
        sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3, $4)")
            .bind(plan)
            .bind(format!("test_{}", plan.simple()))
            .bind(Value::Object(features))
            .bind(Value::Object(limits))
            .execute(&self.pool)
            .await
            .expect("insert plan");
        sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
            .bind(self.org)
            .bind(plan)
            .execute(&self.pool)
            .await
            .expect("move to the plan");
    }

    /// The bytes org A's stored secret opens to with its own context.
    async fn opened(&self) -> Vec<u8> {
        let (id, _, _, sealed, _) = self.row(self.org).await.expect("a webhook");
        self.keys
            .decrypt(&sealed, &secret_context(&self.org, &id))
            .expect("the sealed secret opens for its row")
    }

    async fn entries(&self) -> Vec<audit::Entry> {
        audit::list(&self.pool, scope(self.org), 100)
            .await
            .expect("read audit")
            .into_iter()
            .filter(|e| e.target_type == "admission_webhook")
            .collect()
    }

    /// The whole of `audit_events` as text, every organization's.
    async fn audit_text(&self) -> String {
        sqlx::query_scalar(
            "SELECT coalesce(string_agg(to_jsonb(a)::text, ' '), '') FROM audit_events a",
        )
        .fetch_one(&self.pool)
        .await
        .expect("read audit")
    }
}

/// T18: creating returns the secret once, in Standard Webhooks form; `GET` and a later `PUT`
/// never contain it; the column holds it sealed for its row, not in the clear.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_the_secret_is_shown_once_and_stored_sealed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    assert_eq!(f.get(&admin).await.code(), "admission.webhook_not_found");

    let created = f.put(&admin, json!({ "url": URL })).await;
    assert_eq!(created.status, StatusCode::OK, "{created:?}");
    assert_schema("SavedAdmissionWebhook", &created.body);
    assert_eq!(created.body["url"], URL);
    assert_eq!(
        created.body["enabled"], false,
        "a new webhook is off until enabled"
    );
    assert_eq!(created.body["created_at"], "2025-11-13T02:13:20.000000Z");
    let secret = created.secret();
    let bytes = secret_bytes(&secret);
    assert_eq!(bytes.len(), 32);

    let read = f.get(&admin).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_schema("AdmissionWebhook", &read.body);
    assert_eq!(
        read.body,
        json!({ "url": URL, "enabled": false, "created_at": "2025-11-13T02:13:20.000000Z" })
    );
    let changed = f.put(&admin, json!({ "enabled": true })).await;
    assert_eq!(changed.status, StatusCode::OK);
    assert!(changed.body.get("secret").is_none(), "{changed:?}");
    for answer in [&read, &changed] {
        assert!(!answer.text.contains(&secret[6..]), "{answer:?}");
    }

    let (id, _, _, sealed, _) = f.row(f.org).await.expect("a webhook");
    assert_ne!(sealed, bytes, "the column is not the plaintext secret");
    assert!(
        !sealed.windows(bytes.len()).any(|w| w == bytes.as_slice()),
        "the column does not contain the plaintext secret"
    );
    assert_eq!(
        f.opened().await,
        bytes,
        "the column opens to the shown secret"
    );
    let other_row = new_id();
    assert!(
        f.keys
            .decrypt(&sealed, &secret_context(&f.org, &other_row))
            .is_err(),
        "the sealed value is bound to its row"
    );
    assert!(
        f.keys
            .decrypt(&sealed, &secret_context(&new_id(), &id))
            .is_err()
    );
}

/// T18: rotating returns a new secret, which the column then holds; the old one is gone.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_rotating_replaces_the_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    assert_eq!(f.rotate(&admin).await.code(), "admission.webhook_not_found");
    let first = f.put(&admin, json!({ "url": URL })).await.secret();
    let (id, ..) = f.row(f.org).await.expect("a webhook");

    let rotated = f.rotate(&admin).await;
    assert_eq!(rotated.status, StatusCode::OK, "{rotated:?}");
    assert_schema("AdmissionWebhookSecret", &rotated.body);
    let second = rotated.secret();
    assert_ne!(second, first);
    assert_eq!(f.opened().await, secret_bytes(&second));
    let (same_id, url, enabled, ..) = f.row(f.org).await.expect("a webhook");
    assert_eq!((same_id, url.as_str(), enabled), (id, URL, false));
    assert!(f.get(&admin).await.body.get("secret").is_none());
}

/// B24: a `PUT` names only what it changes; one of stored values writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_put_changes_only_what_it_names(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let created = f.put(&admin, json!({ "url": URL, "enabled": false })).await;
    assert_eq!(created.body["enabled"], false);
    let moved = "https://hooks.example.test/v2?tenant=7";
    let answer = f.put(&admin, json!({ "url": moved })).await;
    assert_eq!(
        (answer.body["url"].as_str(), &answer.body["enabled"]),
        (Some(moved), &json!(false))
    );
    let before = (f.row(f.org).await, f.entries().await.len());
    for body in [json!({}), json!({ "url": moved, "enabled": false })] {
        let answer = f.put(&admin, body).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert!(answer.body.get("secret").is_none());
    }
    assert_eq!((f.row(f.org).await, f.entries().await.len()), before);
}

/// B24: creating needs a URL; `null` and unknown members are malformed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_without_a_url_cannot_create_and_null_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let answer = f.put(&admin, json!({ "enabled": true })).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer.body["errors"],
        json!([{ "field": "/url", "code": "admission.webhook_url_required" }])
    );
    for body in [
        json!({ "url": null }),
        json!({ "url": URL, "enabled": null }),
        json!({ "url": URL, "secret": "whsec_AAAA" }),
        json!([URL, true]),
    ] {
        let answer = f.put(&admin, body.clone()).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}: {answer:?}");
        assert_eq!(answer.code(), "request.malformed");
    }
    assert_eq!(f.row(f.org).await, None);
    assert!(f.entries().await.is_empty());
}

/// B17: the URL is checked on save under the outbound policy for the instance's tenancy.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b17_the_url_is_checked_under_the_outbound_policy(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let refuse = |url: &'static str, code: &'static str| (url, code);
    for (url, code) in [
        refuse(
            "hooks.example.test/admission",
            "admission.webhook_url_invalid",
        ),
        refuse(
            "https://hooks.example.test/#frag",
            "admission.webhook_url_invalid",
        ),
        refuse(
            "https://hooks.example.test/a b",
            "admission.webhook_url_invalid",
        ),
        refuse(
            "http://hooks.example.test/",
            "admission.webhook_url_refused",
        ),
        refuse(
            "https://u:p@hooks.example.test/",
            "admission.webhook_url_refused",
        ),
        refuse("https://127.0.0.1/hook", "admission.webhook_url_refused"),
        refuse("https://169.254.169.254/", "admission.webhook_url_refused"),
    ] {
        let answer = f.put(&admin, json!({ "url": url })).await;
        assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(
            answer.body["errors"],
            json!([{ "field": "/url", "code": code }]),
            "{url}"
        );
    }
    assert_eq!(f.row(f.org).await, None);

    // One organization: a private address is the operator's own network, allowed.
    let private = "https://10.0.0.5/hook";
    let answer = f.put(&admin, json!({ "url": private })).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    // Two organizations: refused, and the stored URL stays.
    organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization");
    let answer = f
        .put(&admin, json!({ "url": "https://10.0.0.6/hook" }))
        .await;
    assert_eq!(
        answer.body["errors"],
        json!([{ "field": "/url", "code": "admission.webhook_url_refused" }])
    );
    assert_eq!(f.row(f.org).await.expect("a webhook").1, private);
}

/// The escalation rule: `admission.manage` alone reads but cannot create, change, rotate or
/// delete, and the refusal comes before the webhook is looked up.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn writes_need_every_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, _) = f.holder_in(f.org, &[AdmissionManage::KEY]).await;
    let escalation = |answer: Answer| {
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{answer:?}");
        assert_eq!(answer.code(), "permission.escalation");
    };
    // No webhook yet: still 403, not 404.
    escalation(f.rotate(&manager).await);
    escalation(f.delete(&manager).await);
    escalation(f.put(&manager, json!({ "url": URL })).await);
    assert_eq!(f.row(f.org).await, None);

    let admin = f.admin().await;
    f.put(&admin, json!({ "url": URL })).await.secret();
    let stored = f.row(f.org).await;
    let entries = f.entries().await.len();
    assert_eq!(f.get(&manager).await.status, StatusCode::OK);
    escalation(f.put(&manager, json!({ "enabled": false })).await);
    escalation(f.put(&manager, json!({})).await);
    escalation(f.rotate(&manager).await);
    escalation(f.delete(&manager).await);
    assert_eq!(f.row(f.org).await, stored);
    assert_eq!(f.entries().await.len(), entries);
}

/// An API key's rights are its scopes over its owner's: a key with only `admission.manage`
/// reads but cannot write, even when its owner holds every permission; a key with every scope
/// writes, and gets the secret like a session does.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_api_key_writes_only_with_every_scope(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let narrow = f.key(&[AdmissionManage::KEY]).await;
    let wide = f.key(&every_key()).await;
    let body = Some(json!({ "url": URL }));
    let answer = f
        .send_in(f.org, Method::PUT, PATH, Auth::Key(&narrow), body.clone())
        .await;
    assert_eq!(answer.code(), "permission.escalation", "{answer:?}");
    let answer = f
        .send_in(f.org, Method::PUT, PATH, Auth::Key(&wide), body)
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert!(answer.secret().starts_with("whsec_"));
    let read = f
        .send_in(f.org, Method::GET, PATH, Auth::Key(&narrow), None)
        .await;
    assert_eq!(read.status, StatusCode::OK);
    let entries = f.entries().await;
    assert!(
        matches!(entries[0].actor, Actor::ApiKey { .. }),
        "{entries:?}"
    );
}

/// Spec 09 B22: each write's entry, by the caller from its address, never with the secret.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_write_is_audited_without_the_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, user) = f.holder_in(f.org, &every_key()).await;
    let first = f.put(&admin, json!({ "url": URL })).await.secret();
    f.put(&admin, json!({ "enabled": true })).await;
    let second = f.rotate(&admin).await.secret();
    let (id, ..) = f.row(f.org).await.expect("a webhook");
    assert_eq!(f.delete(&admin).await.status, StatusCode::NO_CONTENT);
    assert_eq!(f.row(f.org).await, None);
    assert_eq!(f.get(&admin).await.code(), "admission.webhook_not_found");
    assert_eq!(f.delete(&admin).await.code(), "admission.webhook_not_found");

    let entries = f.entries().await;
    let actions: Vec<_> = entries
        .iter()
        .map(|e| (e.action.as_str(), &e.data))
        .collect();
    assert_eq!(
        actions,
        [
            (
                "admission_webhook.create",
                &json!({ "after": { "url": URL, "enabled": false } })
            ),
            (
                "admission_webhook.update",
                &json!({ "before": { "enabled": false }, "after": { "enabled": true } })
            ),
            ("admission_webhook.rotate_secret", &json!({})),
            (
                "admission_webhook.delete",
                &json!({ "before": { "url": URL, "enabled": true } })
            ),
        ]
    );
    let actor = Actor::User {
        id: user,
        ip: CLIENT.ip(),
    };
    assert!(
        entries
            .iter()
            .all(|e| e.actor == actor && e.target_id == id)
    );
    let text = f.audit_text().await;
    for secret in [&first, &second] {
        assert!(
            !text.contains(&secret[6..]),
            "an audit entry holds a secret"
        );
    }
}

/// B2 of spec 08: the server's log at `trace` holds neither secret.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_log_holds_no_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let first = f.put(&admin, json!({ "url": URL })).await.secret();
    let second = f.rotate(&admin).await.secret();
    drop(guard);
    let text = log.text();
    assert!(text.contains(PATH), "the capture missed the requests");
    for secret in [&first, &second] {
        assert!(!text.contains(&secret[6..]), "the log holds a secret");
    }
}

/// T20: org A's admin creates, changes, rotates and deletes A's webhook; B's stays as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_one_organization_leaves_anothers_webhook_alone(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization")
        .id;
    let (b_admin, _) = f.holder_in(b, &every_key()).await;
    let b_url = "https://org-b.example.test/hook";
    let created = f
        .send_in(
            b,
            Method::PUT,
            PATH,
            Auth::Cookie(&b_admin),
            Some(json!({ "url": b_url })),
        )
        .await;
    let b_secret = created.secret();
    let b_row = f.row(b).await;

    let admin = f.admin().await;
    assert_eq!(f.get(&admin).await.code(), "admission.webhook_not_found");
    assert_eq!(f.rotate(&admin).await.code(), "admission.webhook_not_found");
    assert_eq!(f.delete(&admin).await.code(), "admission.webhook_not_found");
    let a_created = f.put(&admin, json!({ "url": URL })).await;
    assert_ne!(a_created.secret(), b_secret);
    f.put(&admin, json!({ "enabled": false })).await;
    f.rotate(&admin).await;
    let read = f.get(&admin).await;
    assert_eq!(read.body["url"], URL);
    assert!(!read.text.contains(b_url));
    f.delete(&admin).await;
    assert_eq!(f.row(b).await, b_row, "org B's webhook is unchanged");
    // B's admin cannot read A's from B either: each request reads its own organization.
    let b_read = f
        .send_in(b, Method::GET, PATH, Auth::Cookie(&b_admin), None)
        .await;
    assert_eq!(b_read.body["url"], b_url);
}

/// Spec 10 B11: an organization whose plan lacks `admission_webhook` keeps a read-only webhook.
/// It can read, disable and delete it, but not enable it, change its URL, create one or rotate
/// its secret.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_downgraded_organization_can_read_disable_and_delete_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.put(&admin, json!({ "url": URL, "enabled": true }))
        .await
        .secret();
    f.plan_without("admission_webhook").await;
    let unavailable = |answer: Answer| {
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{answer:?}");
        assert_eq!(answer.code(), "entitlement.feature_unavailable");
    };

    assert_eq!(f.get(&admin).await.body["enabled"], true);
    unavailable(f.rotate(&admin).await);
    unavailable(
        f.put(&admin, json!({ "url": "https://hooks.example.test/v2" }))
            .await,
    );
    unavailable(f.put(&admin, json!({ "url": URL })).await);
    let stored = f.row(f.org).await;
    let disabled = f.put(&admin, json!({ "enabled": false })).await;
    assert_eq!(disabled.status, StatusCode::OK, "{disabled:?}");
    assert_eq!(disabled.body["enabled"], false);
    assert_eq!(f.put(&admin, json!({})).await.status, StatusCode::OK);
    unavailable(f.put(&admin, json!({ "enabled": true })).await);
    let after = f.row(f.org).await.expect("a webhook");
    let before = stored.expect("a webhook");
    assert_eq!(
        (&after.0, &after.1, &after.3),
        (&before.0, &before.1, &before.3)
    );
    assert_eq!(f.delete(&admin).await.status, StatusCode::NO_CONTENT);
    unavailable(f.put(&admin, json!({ "url": URL })).await);
    assert_eq!(f.row(f.org).await, None);
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
