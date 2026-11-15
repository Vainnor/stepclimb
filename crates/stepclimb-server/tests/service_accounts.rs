// SPDX-License-Identifier: AGPL-3.0-or-later
//! Service accounts and API keys over the whole router (docs/specs/m0/08-sessions-and-csrf.md
//! B4, B16, B24 to B29; T3's API-key part, T15 and T16 with a valid key, T21 to T25; spec 03
//! B6's key case and T17's owned-key case; spec 09 T12). The four-case and cross-organization
//! cases of the three routes are in `tests/permissions.rs`.
//!
//! One organization, so the production request-organization resolver serves it, except where a
//! test builds a second. `session::start` stands in for the sign-in specs. Needs `DATABASE_URL`
//! (`just up`).

use std::{
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use stepclimb_auth::{encryption::KeyRing, token::ApiKey};
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole, RolePatch},
    users::{self, NewUser, Status},
};
use stepclimb_server::{AppState, openapi, router, session, web_origin::WebOrigin};
use tower::ServiceExt;

mod common;

const COLLECTION: &str = "/api/v1/admin/service-accounts";
const ME: &str = "/api/v1/me";

/// The client address every request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

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

fn state(pool: PgPool, clock: &ManualClock) -> AppState {
    AppState {
        db: pool,
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
        passwords: Default::default(),
    }
}

fn app(state: AppState) -> Router {
    router(state).layer(Extension(ConnectInfo(CLIENT)))
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

    #[track_caller]
    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "{self:?}");
        assert_eq!(self.code(), code, "{self:?}");
    }
}

/// How a request is authenticated.
#[derive(Clone, Copy)]
enum Auth<'a> {
    Cookie(&'a str),
    Key(&'a str),
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    auth: &[Auth<'_>],
    body: Option<Value>,
) -> Answer {
    let mut request = Request::builder().method(method.clone()).uri(uri);
    for credential in auth {
        request = match credential {
            Auth::Cookie(cookie) => request.header(header::COOKIE, *cookie),
            Auth::Key(key) => request.header(header::AUTHORIZATION, format!("Bearer {key}")),
        };
    }
    // A browser's same-origin request; a bearer request passes the CSRF check anyway.
    if method != Method::GET {
        request = request.header("sec-fetch-site", "same-origin");
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("a request"))
        .await
        .expect("a response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Answer {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    }
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
    tower: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "example", "Example Org", now())
            .await
            .expect("create organization")
            .id;
        let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        let tower = accounts::create(&pool, scope(org), root, "Tower", now(), Actor::Instance)
            .await
            .expect("create Tower")
            .id;
        let clock = ManualClock::new(now());
        let app = app(state(pool.clone(), &clock));
        Self {
            pool,
            clock,
            app,
            org,
            tower,
        }
    }

    async fn user(&self, name: &str) -> Uuid {
        let new_user = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", new_id().simple())),
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

    async fn sign_in(&self, user: Uuid) -> String {
        let started = session::start(
            &self.pool,
            scope(self.org),
            user,
            &HeaderMap::new(),
            self.clock.clock().now(),
        )
        .await
        .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn assign(&self, user: Uuid, role: Uuid, at: Scope) -> Uuid {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
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
        .expect("create assignment")
        .id
    }

    async fn builtin(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    async fn custom_role(&self, permissions: &[&str]) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    /// A signed-in user holding a new custom role with `permissions` plus
    /// `service_account.manage` at the organization: their id, cookie and role.
    async fn manager(&self, permissions: &[&str]) -> (Uuid, String, Uuid) {
        let user = self.user("Kim Manager").await;
        let mut all = permissions.to_vec();
        if !all.contains(&"service_account.manage") {
            all.push("service_account.manage");
        }
        let role = self.custom_role(&all).await;
        self.assign(user, role, Scope::Organization).await;
        (user, self.sign_in(user).await, role)
    }

    /// Create a service account through the route as `auth`; the 201 body.
    async fn create(&self, auth: Auth<'_>, scopes: &[&str]) -> Value {
        let answer = self.create_answer(auth, scopes).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
        answer.body
    }

    async fn create_answer(&self, auth: Auth<'_>, scopes: &[&str]) -> Answer {
        let body = json!({ "name": "Nightly export", "scopes": scopes });
        send(&self.app, Method::POST, COLLECTION, &[auth], Some(body)).await
    }

    async fn get(&self, uri: &str, auth: &[Auth<'_>]) -> Answer {
        send(&self.app, Method::GET, uri, auth, None).await
    }

    async fn entries(&self) -> Vec<audit::Entry> {
        audit::list(&self.pool, scope(self.org), 1000)
            .await
            .expect("read audit entries")
    }
}

fn key_of(created: &Value) -> String {
    created["key"].as_str().expect("a key").to_owned()
}

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document as JSON")
}

/// T21 (B24, B26), spec 03 T17's owned-key case: the key is returned once, never listed, and
/// authenticates `GET /api/v1/me` as its owner with the key's narrowed grants.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t21_the_key_is_shown_once_and_authenticates_me_as_its_owner(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&["user.view", "account.view"]).await;
    let created = f.create(Auth::Cookie(&cookie), &["account.view"]).await;
    let doc = document();
    let errors = common::schema_errors(&doc, "CreatedServiceAccount", &created);
    assert!(errors.is_empty(), "{errors:#?}");
    let key = key_of(&created);
    assert!(ApiKey::parse(&key).is_ok(), "a well-formed key");
    assert_eq!(created["owner_user_id"], json!(kim));
    assert_eq!(created["kind"], "api_key");
    assert_eq!(created["last_used_at"], Value::Null);

    let me = f.get(ME, &[Auth::Key(&key)]).await;
    assert_eq!(me.status, StatusCode::OK, "{me:?}");
    let errors = common::schema_errors(&doc, "Me", &me.body);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(me.body["user"]["id"], json!(kim));
    assert_eq!(
        me.body["grants"],
        json!([{
            "scope": { "type": "organization", "id": f.org },
            "permissions": ["account.view"],
        }]),
        "the owner's grants narrowed to the key's scopes"
    );

    // Listed with its last use, never with the key, by session or by key.
    f.clock.advance(Duration::from_secs(60));
    for auth in [Auth::Cookie(&cookie)] {
        let list = f.get(COLLECTION, &[auth]).await;
        assert_eq!(list.status, StatusCode::OK, "{list:?}");
        assert!(!list.text.contains(&key), "the list shows the key");
        assert!(!list.text.contains(&key[3..46]), "the list shows the key");
        assert_eq!(list.body["items"][0]["id"], created["id"]);
        assert_eq!(
            list.body["items"][0]["last_used_at"],
            "2025-11-13T02:13:20.000000Z"
        );
        assert!(list.body["items"][0].get("key").is_none());
    }
    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT secret_hash FROM service_accounts")
        .fetch_all(&f.pool)
        .await
        .expect("read hashes");
    assert_eq!(
        stored,
        [ApiKey::parse(&key).unwrap().hash().as_bytes().to_vec()]
    );
}

/// T22 (B25): a key whose checksum fails is refused before any database query. The router's
/// pool points at nothing, so a query would answer 500.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t22_a_wrong_checksum_is_refused_without_a_query(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, _) = f.manager(&[]).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    let last = key.chars().last().expect("a character");
    let wrong = format!(
        "{}{}",
        &key[..key.len() - 1],
        if last == 'A' { 'B' } else { 'A' }
    );
    assert!(ApiKey::parse(&wrong).is_err());

    let unreachable = PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(500))
        .connect_lazy("postgres://stepclimb@127.0.0.1:1/none")
        .expect("a lazy pool parses its URL");
    let offline = app(state(unreachable, &f.clock));
    for presented in [wrong.as_str(), "sc_short_000000"] {
        send(&offline, Method::GET, ME, &[Auth::Key(presented)], None)
            .await
            .expect(StatusCode::UNAUTHORIZED, "auth.api_key_invalid");
    }
    // The control: a well-formed key does query, so the offline router fails it.
    let control = send(&offline, Method::GET, ME, &[Auth::Key(&key)], None).await;
    assert_eq!(
        control.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{control:?}"
    );

    // A well-formed key no service account has: the same 401 after the lookup.
    let unknown = ApiKey::generate().expect("a key");
    f.get(ME, &[Auth::Key(unknown.expose_secret())])
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.api_key_invalid");
    // A bearer credential that is not an API key is no credential this server accepts.
    f.get(ME, &[Auth::Key("not-an-api-key")])
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
}

/// T23 (B27): a key scoped to `user.view` whose owner holds `user.view` and `account.view` can
/// list users and gets 403 listing accounts; once the owner's role loses `user.view`, the same
/// key gets 403 listing users on its next request.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t23_a_key_holds_its_scopes_where_its_owner_holds_them_now(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, role) = f.manager(&["user.view", "account.view"]).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &["user.view"]).await);
    let users = "/api/v1/users";
    let accounts = "/api/v1/accounts";
    assert_eq!(
        f.get(users, &[Auth::Key(&key)]).await.status,
        StatusCode::OK
    );
    f.get(accounts, &[Auth::Key(&key)])
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    // The owner's own session still lists accounts.
    assert_eq!(
        f.get(accounts, &[Auth::Cookie(&cookie)]).await.status,
        StatusCode::OK
    );

    let patch = RolePatch {
        name: None,
        permissions: Some(vec!["account.view".into(), "service_account.manage".into()]),
    };
    roles::update_custom(&f.pool, scope(f.org), role, &patch, now(), Actor::Instance)
        .await
        .expect("drop user.view from the role");
    f.get(users, &[Auth::Key(&key)])
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
}

/// Spec 09 T12 (B16): an owner with `account.manage` at Tower; a key whose scopes do not cover
/// it gets 403 renaming Tower, and a key whose scopes do gets 404 once the owner's assignment
/// is removed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn spec_09_t12_a_key_is_narrowed_to_its_scopes_and_its_owners_assignments(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&["account.view", "account.manage"]).await;
    let narrow = key_of(&f.create(Auth::Cookie(&cookie), &["account.view"]).await);
    let covering = key_of(&f.create(Auth::Cookie(&cookie), &["account.manage"]).await);
    // From here Kim holds account.manage at Tower only: the organization role keeps
    // service_account.manage alone.
    let admin_role: Uuid =
        sqlx::query_scalar("SELECT role_id FROM role_assignments WHERE user_id = $1")
            .bind(kim)
            .fetch_one(&f.pool)
            .await
            .expect("Kim's role");
    let patch = RolePatch {
        name: None,
        permissions: Some(vec!["service_account.manage".into()]),
    };
    roles::update_custom(
        &f.pool,
        scope(f.org),
        admin_role,
        &patch,
        now(),
        Actor::Instance,
    )
    .await
    .expect("narrow Kim's organization role");
    let account_admin = f.builtin("account_admin").await;
    let at_tower = f.assign(kim, account_admin, Scope::Account(f.tower)).await;

    let tower = format!("/api/v1/accounts/{}", f.tower);
    let rename = || json!({ "name": "Tower Cab" });
    send(
        &f.app,
        Method::PATCH,
        &tower,
        &[Auth::Key(&narrow)],
        Some(rename()),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");
    let renamed = send(
        &f.app,
        Method::PATCH,
        &tower,
        &[Auth::Key(&covering)],
        Some(rename()),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::OK, "{renamed:?}");

    role_assignments::delete(&f.pool, scope(f.org), at_tower, now(), Actor::Instance)
        .await
        .expect("remove the assignment");
    send(
        &f.app,
        Method::PATCH,
        &tower,
        &[Auth::Key(&covering)],
        Some(rename()),
    )
    .await
    .expect(StatusCode::NOT_FOUND, "account.not_found");
}

/// T24 (B27): a suspended owner's key answers 401 `auth.suspended`, and works again once the
/// owner is active.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t24_a_suspended_owners_key_answers_suspended(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&[]).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    users::set_status(
        &f.pool,
        scope(f.org),
        kim,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    f.get(ME, &[Auth::Key(&key)])
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.suspended");
    users::set_status(
        &f.pool,
        scope(f.org),
        kim,
        Status::Active,
        now(),
        Actor::Instance,
    )
    .await
    .expect("unsuspend");
    assert_eq!(f.get(ME, &[Auth::Key(&key)]).await.status, StatusCode::OK);
}

/// T25 (B29) and Q14: deleting the service account makes its key answer 401 on the next
/// request; creating is audited with the session's user, the key's own deletion with the key.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t25_a_deleted_accounts_key_answers_401_and_both_changes_are_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&[]).await;
    let created = f
        .create(Auth::Cookie(&cookie), &["service_account.manage"])
        .await;
    let key = key_of(&created);
    let id: Uuid = serde_json::from_value(created["id"].clone()).expect("an id");
    let own = format!("{COLLECTION}/{id}");
    let deleted = send(&f.app, Method::DELETE, &own, &[Auth::Key(&key)], None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    f.get(ME, &[Auth::Key(&key)])
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.api_key_invalid");
    send(&f.app, Method::DELETE, &own, &[Auth::Cookie(&cookie)], None)
        .await
        .expect(StatusCode::NOT_FOUND, "service_account.not_found");

    let entries: Vec<(String, Actor, Uuid)> = f
        .entries()
        .await
        .into_iter()
        .filter(|e| e.target_type == "service_account")
        .map(|e| (e.action, e.actor, e.target_id))
        .collect();
    let by_kim = Actor::User {
        id: kim,
        ip: CLIENT.ip(),
    };
    let by_key = Actor::ApiKey {
        id,
        ip: CLIENT.ip(),
    };
    assert_eq!(
        entries,
        [
            ("service_account.create".to_owned(), by_kim, id),
            ("service_account.delete".to_owned(), by_key, id),
        ]
    );
}

/// B44: a key cannot create a key, whatever its scopes (403 `auth.session_required`), and
/// writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_key_cannot_create_a_service_account(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, _) = f.manager(&[]).await;
    let key = key_of(
        &f.create(Auth::Cookie(&cookie), &["service_account.manage"])
            .await,
    );
    let before = f.entries().await.len();
    for scopes in [&[][..], &["service_account.manage"][..]] {
        f.create_answer(Auth::Key(&key), scopes)
            .await
            .expect(StatusCode::FORBIDDEN, "auth.session_required");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM service_accounts")
        .fetch_one(&f.pool)
        .await
        .expect("count");
    assert_eq!(count, 1);
    assert_eq!(f.entries().await.len(), before);
}

/// B24, Q9: scopes are catalog keys listed once (422), and each must be held by the caller
/// (403 `service_account.scope_exceeds_owner`).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn scopes_are_catalog_keys_the_caller_holds(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, _) = f.manager(&["user.view"]).await;
    let session = Auth::Cookie(&cookie);
    let unknown = f
        .create_answer(session, &["user.view", "reports:read"])
        .await;
    unknown.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        unknown.body["errors"],
        json!([{ "field": "/scopes/1", "code": "service_account.scope_unknown" }])
    );
    let twice = f.create_answer(session, &["user.view", "user.view"]).await;
    assert_eq!(
        twice.body["errors"],
        json!([{ "field": "/scopes/1", "code": "service_account.scope_duplicate" }])
    );
    f.create_answer(session, &["user.manage"])
        .await
        .expect(StatusCode::FORBIDDEN, "service_account.scope_exceeds_owner");

    f.create(session, &["service_account.manage"]).await;
    let blank = send(
        &f.app,
        Method::POST,
        COLLECTION,
        &[session],
        Some(json!({ "name": " ", "scopes": [] })),
    )
    .await;
    assert_eq!(
        blank.body["errors"],
        json!([{ "field": "/name", "code": "name.empty" }])
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM service_accounts")
        .fetch_one(&f.pool)
        .await
        .expect("count");
    assert_eq!(count, 1, "only the valid request stored an account");
}

/// T15 (B16) with a valid key: a cross-site DELETE carrying it passes the CSRF layer, and the
/// same request with only a cookie does not. T16: a valid key beside another user's session
/// cookie is answered for the key's owner.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_t16_a_valid_key_skips_csrf_and_outranks_a_cookie(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&[]).await;
    let key = key_of(
        &f.create(Auth::Cookie(&cookie), &["service_account.manage"])
            .await,
    );
    let cross_site = |auth: Option<String>, session: Option<&str>, id: &Value| {
        let mut request = Request::builder()
            .method(Method::DELETE)
            .uri(format!("{COLLECTION}/{}", id.as_str().expect("an id")))
            .header("sec-fetch-site", "cross-site");
        if let Some(key) = auth {
            request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        if let Some(cookie) = session {
            request = request.header(header::COOKIE, cookie);
        }
        f.app
            .clone()
            .oneshot(request.body(Body::empty()).expect("a request"))
    };
    let first = f.create(Auth::Cookie(&cookie), &[]).await;
    let by_cookie = cross_site(None, Some(&cookie), &first["id"])
        .await
        .expect("a response");
    assert_eq!(by_cookie.status(), StatusCode::FORBIDDEN);
    let by_key = cross_site(Some(key.clone()), None, &first["id"])
        .await
        .expect("a response");
    assert_eq!(by_key.status(), StatusCode::NO_CONTENT);

    let pat = f.user("Pat Other").await;
    let pats_cookie = f.sign_in(pat).await;
    let me = f
        .get(ME, &[Auth::Cookie(&pats_cookie), Auth::Key(&key)])
        .await;
    assert_eq!(me.status, StatusCode::OK, "{me:?}");
    assert_eq!(me.body["user"]["id"], json!(kim));
}

/// Spec 08 B44, spec 03 B6: an API key, whatever its scopes, cannot change its owner's email
/// or status, link or remove its owner's identities, remove its owner's TOTP, or create an
/// enrollment link for anyone (403 `auth.session_required`); nothing changes. The same key may still change another
/// user's email.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_key_cannot_take_over_its_owner_through_the_admin_routes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&["user.manage", "user.view"]).await;
    let full = key_of(
        &f.create(
            Auth::Cookie(&cookie),
            &["user.manage", "user.view", "service_account.manage"],
        )
        .await,
    );
    let pat = f.user("Pat Example").await;
    let user = |id: Uuid| format!("/api/v1/users/{id}");
    let refused = [
        (
            Method::PATCH,
            user(kim),
            Some(json!({ "email": "kim.new@example.test" })),
        ),
        (
            Method::PATCH,
            user(kim),
            Some(json!({ "status": "active" })),
        ),
        (
            Method::POST,
            format!("{}/identities", user(kim)),
            Some(json!({ "provider_key": "corp", "subject": "1000001" })),
        ),
        (
            Method::DELETE,
            format!("{}/identities/{}", user(kim), new_id()),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/admin/users/{kim}/enrollment-links"),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/admin/users/{pat}/enrollment-links"),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/v1/admin/users/{kim}/totp"),
            None,
        ),
    ];
    let before = f.entries().await.len();
    for (method, uri, body) in refused {
        send(&f.app, method.clone(), &uri, &[Auth::Key(&full)], body)
            .await
            .expect(StatusCode::FORBIDDEN, "auth.session_required");
    }
    assert_eq!(f.entries().await.len(), before);
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM local_tokens")
        .fetch_one(&f.pool)
        .await
        .expect("count");
    assert_eq!(links, 0);

    let changed = send(
        &f.app,
        Method::PATCH,
        &user(pat),
        &[Auth::Key(&full)],
        Some(json!({ "email": "pat.new@example.test" })),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{changed:?}");
    // The owner's own session still may.
    let own = send(
        &f.app,
        Method::PATCH,
        &user(kim),
        &[Auth::Cookie(&cookie)],
        Some(json!({ "email": "kim.new@example.test" })),
    )
    .await;
    assert_eq!(own.status, StatusCode::OK, "{own:?}");
}

/// Spec 04 Permissions (from #174), the `/me` identity route and the password routes: a valid
/// key, whose owner has a session-capable account, gets 403 `auth.session_required` on every
/// route that changes how its owner signs in or uses an enrollment link, and nothing changes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_valid_key_cannot_manage_its_owners_sign_in_methods(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (kim, cookie, _) = f.manager(&[]).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    let credential = json!({ "placeholder": true });
    let credential_id = new_id().as_bytes().to_vec();
    let passkey = stepclimb_core::passkeys::create(
        &f.pool,
        scope(f.org),
        stepclimb_core::passkeys::NewPasskey {
            user_id: kim,
            credential_id: &credential_id,
            credential: &credential,
            name: "Phone",
        },
        now(),
        Actor::Instance,
    )
    .await
    .expect("store a passkey")
    .id;
    let one = format!("/api/v1/me/passkeys/{passkey}");
    let routes = [
        (Method::GET, "/api/v1/me/passkeys".to_owned(), None),
        (Method::POST, "/api/v1/me/passkeys/start".to_owned(), None),
        (
            Method::POST,
            "/api/v1/me/passkeys".to_owned(),
            Some(json!({ "credential": {} })),
        ),
        (
            Method::PATCH,
            one.clone(),
            Some(json!({ "name": "Renamed" })),
        ),
        (Method::DELETE, one, None),
        (
            Method::DELETE,
            format!("/api/v1/me/identities/{}", new_id()),
            None,
        ),
        (
            Method::PUT,
            "/api/v1/me/password".to_owned(),
            Some(json!({ "current_password": "x", "new_password": "y" })),
        ),
        (
            Method::POST,
            "/auth/local/enrollment/password".to_owned(),
            Some(json!({ "token": "x", "password": "y" })),
        ),
        (Method::POST, "/api/v1/me/totp".to_owned(), None),
        (
            Method::POST,
            "/api/v1/me/totp/confirm".to_owned(),
            Some(json!({ "code": "000000" })),
        ),
        (
            Method::POST,
            "/api/v1/me/totp/disable".to_owned(),
            Some(json!({ "code": "000000" })),
        ),
        (
            Method::POST,
            "/auth/local/enrollment/start".to_owned(),
            Some(json!({ "token": "x" })),
        ),
        (
            Method::POST,
            "/auth/local/enrollment/passkey".to_owned(),
            Some(json!({ "credential": {} })),
        ),
    ];
    let before = f.entries().await.len();
    for (method, uri, body) in routes {
        send(&f.app, method, &uri, &[Auth::Key(&key)], body)
            .await
            .expect(StatusCode::FORBIDDEN, "auth.session_required");
    }
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM passkeys")
        .fetch_all(&f.pool)
        .await
        .expect("read passkeys");
    assert_eq!(names, ["Phone"]);
    assert_eq!(f.entries().await.len(), before);
}

/// T3's API-key part (B4): creating a key and using it, logged by the server's subscriber at
/// `trace`, leaves neither the key nor its random part in the log.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_an_api_key_request_logs_no_key(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, _) = f.manager(&[]).await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    assert_eq!(f.get(ME, &[Auth::Key(&key)]).await.status, StatusCode::OK);
    let wrong = format!("{}0", &key[..key.len() - 1]);
    f.get(ME, &[Auth::Key(&wrong)]).await;
    drop(guard);

    let text = log.text();
    assert!(
        text.contains("\"level\":\"TRACE\"") || text.contains("\"level\":\"DEBUG\""),
        "the capture missed the log"
    );
    assert!(text.contains(COLLECTION), "the capture missed the requests");
    for secret in [key.as_str(), &key[3..46], wrong.as_str()] {
        assert!(!text.contains(secret), "the log holds a key");
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

/// The `/api/v1` operations a key with no scopes may call: those that need a principal and no
/// permission and change nothing about how its owner signs in (spec 08 B44). Every other
/// operation outside `/api/v1` is public and not walked.
const EMPTY_SCOPE_ALLOWED: &[&str] = &[
    "getMe",
    "getOrganization",
    "updateMyAppearance",
    "listMyIdentities",
];

/// B27, B44: a key with no scopes, whose owner holds every catalog permission at the
/// organization, is refused on every `/api/v1` operation of the document but
/// [`EMPTY_SCOPE_ALLOWED`]: 403, or for a route on an account the 404 B17 answers for an account
/// the principal cannot see. The walk reads the document, which lists every route the router
/// mounts (`tests/csrf.rs` T17), so a new route is walked without being named here.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_empty_scope_key_is_refused_on_every_operation_but_the_allowed_ones(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let every: Vec<&str> = stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect();
    let (_, cookie, _) = f.manager(&every).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    let doc = document();
    let mut walked = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        if !path.starts_with("/api/v1/") {
            continue;
        }
        for (method, operation) in item.as_object().expect("a path item") {
            let id = operation["operationId"].as_str().expect("an operationId");
            walked.push(id.to_owned());
            let mut uri = path.replace("{account_id}", &f.tower.to_string());
            if uri.contains("{user_id}") {
                let target = f.user("Walk target").await;
                uri = uri.replace("{user_id}", &target.to_string());
            }
            let uri = fill_other_params(&uri);
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method");
            let body = (method != Method::GET && method != Method::DELETE).then(|| {
                if id == "updateMyAppearance" {
                    json!({ "theme": "dark", "density": "compact" })
                } else {
                    json!({})
                }
            });
            let answer = send(&f.app, method, &uri, &[Auth::Key(&key)], body).await;
            if EMPTY_SCOPE_ALLOWED.contains(&id) {
                assert!(answer.status.is_success(), "{id}: {answer:?}");
                continue;
            }
            let hidden_account =
                path.contains("{account_id}") && answer.code() == "account.not_found";
            assert!(
                answer.status == StatusCode::FORBIDDEN
                    || (answer.status == StatusCode::NOT_FOUND && hidden_account),
                "{id} answered an empty-scope key: {answer:?}"
            );
        }
    }
    for id in EMPTY_SCOPE_ALLOWED {
        assert!(walked.iter().any(|w| w == id), "{id} is no operation");
    }
}

/// Every `{name}` path parameter left in `uri`, as a new id.
fn fill_other_params(uri: &str) -> String {
    let mut out = String::new();
    let mut rest = uri;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').expect("a closing brace") + start;
        out.push_str(&rest[..start]);
        out.push_str(&new_id().to_string());
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// B44: a page load carrying an API key draws the page without looking the key up, so it
/// records no use of it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_page_load_with_a_key_does_not_look_it_up(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (_, cookie, _) = f.manager(&[]).await;
    let key = key_of(&f.create(Auth::Cookie(&cookie), &[]).await);
    let page = f.get("/", &[Auth::Key(&key)]).await;
    assert_eq!(page.status, StatusCode::OK, "{page:?}");
    let last_used = || async {
        let used: Option<i64> = sqlx::query_scalar(
            "SELECT (extract(epoch FROM last_used_at))::bigint FROM service_accounts",
        )
        .fetch_one(&f.pool)
        .await
        .expect("read last use");
        used
    };
    assert_eq!(last_used().await, None, "the page load looked the key up");
    assert_eq!(f.get(ME, &[Auth::Key(&key)]).await.status, StatusCode::OK);
    assert!(
        last_used().await.is_some(),
        "the control: a request records it"
    );
}

/// Spec 07 and 04 (the "every permission" rule of `PUT /api/v1/admin/admission` lists and of
/// local sign-in changes) apply to a key's narrowed grants: a key scoped to the route's
/// permission alone is refused 403 `permission.escalation` even though its owner holds every
/// permission, and a key scoped to every permission passes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_every_permission_rule_reads_a_keys_narrowed_grants(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let every: Vec<&str> = stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect();
    let (_, cookie, _) = f.manager(&every).await;
    let admission = key_of(&f.create(Auth::Cookie(&cookie), &["admission.manage"]).await);
    let provider = key_of(&f.create(Auth::Cookie(&cookie), &["provider.manage"]).await);
    let full = key_of(&f.create(Auth::Cookie(&cookie), &every).await);
    let rules = || json!({ "rules": { "organization": [] } });
    let local = || {
        json!({
            "enabled": true, "passkey": true, "password": true, "totp": false, "magic_link": false,
            "totp_required": false,
        })
    };
    let admission_path = "/api/v1/admin/admission";
    let local_path = "/api/v1/admin/local-sign-in";
    send(
        &f.app,
        Method::PUT,
        admission_path,
        &[Auth::Key(&admission)],
        Some(rules()),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.escalation");
    send(
        &f.app,
        Method::PUT,
        local_path,
        &[Auth::Key(&provider)],
        Some(local()),
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.escalation");
    for (path, body) in [(admission_path, rules()), (local_path, local())] {
        let answer = send(&f.app, Method::PUT, path, &[Auth::Key(&full)], Some(body)).await;
        assert_eq!(answer.status, StatusCode::OK, "{path}: {answer:?}");
    }
}

/// Spec 07 I4, spec 03 B17: turning `email_matching` on reads a key's narrowed grants too. A
/// key scoped to `organization.manage` alone is refused 403 `permission.denied` (no
/// `provider.manage`), one scoped to both 403 `permission.escalation`, though their owner holds
/// every permission; a key scoped to every permission turns it on.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn turning_email_matching_on_reads_a_keys_narrowed_grants(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let every: Vec<&str> = stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect();
    let (_, cookie, _) = f.manager(&every).await;
    let settings = key_of(
        &f.create(Auth::Cookie(&cookie), &["organization.manage"])
            .await,
    );
    let both = ["organization.manage", "provider.manage"];
    let both = key_of(&f.create(Auth::Cookie(&cookie), &both).await);
    let full = key_of(&f.create(Auth::Cookie(&cookie), &every).await);
    let path = "/api/v1/organization/settings";
    let on = || Some(json!({ "email_matching": true }));
    for (key, code) in [
        (&settings, "permission.denied"),
        (&both, "permission.escalation"),
    ] {
        send(&f.app, Method::PATCH, path, &[Auth::Key(key)], on())
            .await
            .expect(StatusCode::FORBIDDEN, code);
    }
    let answer = send(&f.app, Method::PATCH, path, &[Auth::Key(&full)], on()).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["email_matching"], true);
}
