// SPDX-License-Identifier: AGPL-3.0-or-later
//! The local sign-in switches over the router and the method check
//! (docs/specs/m0/04-local-accounts.md B1 to B3, T1's switch part, T18 for this route, T21).
//! The permission suite's four cases and cross-organization case for both operations are in
//! `tests/permissions.rs`; these tests cover the escalation rule, the magic link refusal, the
//! audit entries, the settings route's refusal of the switches, `require_method` and startup.
//!
//! The production request-organization resolver serves these tests: each has one organization.
//! `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

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
    local_sign_in::{self, Method as LocalMethod},
    organizations,
    perm::{self, Permission, ProviderManage},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, local_sign_in::require_method, router, session, startup, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const PATH: &str = "/api/v1/admin/local-sign-in";
const SETTINGS: &str = "/api/v1/organization/settings";

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

fn document() -> Value {
    serde_json::to_value(stepclimb_server::openapi::document()).expect("the document as JSON")
}

fn assert_schema(name: &str, value: &Value) {
    let errors = common::schema_errors(&document(), name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

/// The switches a new organization has.
fn defaults() -> Value {
    json!({ "enabled": true, "passkey": true, "password": true, "totp": true, "magic_link": false })
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
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
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app, org }
    }

    /// A signed-in user holding a custom role with `keys` at organization scope: the `Cookie`
    /// value.
    async fn holder(&self, keys: &[&str]) -> String {
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
        let user = users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id;
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
        cookie.split(';').next().expect("a pair").to_owned()
    }

    /// A holder of every catalog permission, as an Organization admin is.
    async fn admin(&self) -> String {
        let all: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
        self.holder(&all).await
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie);
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
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("a JSON body")
        };
        Answer { status, body }
    }

    /// An enabled OpenID Connect provider (`key`), inserted by SQL: the provider write functions
    /// belong to specs 05 and 06. Its id.
    async fn outside_provider(&self, key: &str, enabled: bool) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
             VALUES ($1, $2, $3, 'oidc', '{}', $4)",
        )
        .bind(id)
        .bind(self.org)
        .bind(key)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    async fn put(&self, cookie: &str, body: Value) -> Answer {
        self.send(Method::PUT, PATH, cookie, Some(body)).await
    }

    /// The settings, the local row and the number of audit entries.
    async fn snapshot(&self) -> (String, String, i64) {
        sqlx::query_as(
            "SELECT o.settings::text || o.updated_at::text,
                    (SELECT to_jsonb(p)::text FROM identity_providers p
                     WHERE p.org_id = o.id AND p.key = 'local'),
                    (SELECT count(*) FROM audit_events a WHERE a.org_id = o.id)
             FROM organizations o WHERE o.id = $1",
        )
        .bind(self.org)
        .fetch_one(&self.pool)
        .await
        .expect("read the organization")
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_answers_the_new_organizations_switches(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let cookie = f.holder(&[ProviderManage::KEY]).await;

    let read = f.send(Method::GET, PATH, &cookie, None).await;

    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_eq!(read.body, defaults());
    assert_schema("LocalSignIn", &read.body);
}

/// T1's switch part over the API: an admin switches passkeys off and the row off (with an
/// outside provider left to sign in with), each change is audited with the admin and the client
/// address, and switching back restores them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admin_changes_the_switches_and_each_change_is_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.outside_provider("example-oidc", true).await;
    let mut off = defaults();
    off["passkey"] = false.into();
    off["enabled"] = false.into();

    let changed = f.put(&admin, off.clone()).await;

    assert_eq!(changed.status, StatusCode::OK, "{changed:?}");
    assert_eq!(changed.body, off);
    assert_schema("LocalSignIn", &changed.body);
    assert_eq!(f.send(Method::GET, PATH, &admin, None).await.body, off);
    let stored = local_sign_in::get(&f.pool, scope(f.org)).await.unwrap();
    assert!(stored.offered().is_empty());

    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    let recent: Vec<(&str, Actor)> = entries
        .iter()
        .rev()
        .take(2)
        .map(|e| (e.action.as_str(), e.actor))
        .collect();
    assert_eq!(recent[0].0, "identity_provider.update");
    assert_eq!(recent[1].0, "organization.update_settings");
    for (_, actor) in &recent {
        assert!(
            matches!(actor, Actor::User { ip, .. } if *ip == CLIENT.ip()),
            "{actor:?}"
        );
    }

    let restored = f.put(&admin, defaults()).await;
    assert_eq!(restored.body, defaults());
    let stored = local_sign_in::get(&f.pool, scope(f.org)).await.unwrap();
    assert!(stored.offers(LocalMethod::Passkey));
}

/// The owner's escalation rule: `provider.manage` alone cannot change how every user signs
/// in, nor can every permission but one; a `PUT` that changes nothing passes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_needs_every_catalog_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let all_but_role_manage: Vec<&str> = perm::ALL
        .iter()
        .map(|p| p.key())
        .filter(|k| *k != <perm::RoleManage as Permission>::KEY)
        .collect();
    let mut off = defaults();
    off["password"] = false.into();

    for keys in [vec![ProviderManage::KEY], all_but_role_manage] {
        let cookie = f.holder(&keys).await;
        let before = f.snapshot().await;
        let refused = f.put(&cookie, off.clone()).await;
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::FORBIDDEN, "permission.escalation"),
            "{keys:?}: {refused:?}"
        );
        assert_eq!(f.snapshot().await, before, "{keys:?}");
        let unchanged = f.put(&cookie, defaults()).await;
        assert_eq!(unchanged.status, StatusCode::OK, "{keys:?}: {unchanged:?}");
        assert_eq!(unchanged.body, defaults());
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_magic_link_cannot_be_switched_on_before_email(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let before = f.snapshot().await;
    let mut on = defaults();
    on["magic_link"] = true.into();

    let refused = f.put(&admin, on).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{refused:?}"
    );
    assert_eq!(refused.code(), "request.invalid");
    assert_eq!(refused.body["errors"][0]["field"], "/magic_link");
    assert_eq!(
        refused.body["errors"][0]["code"],
        "local.magic_link_needs_email"
    );
    assert_eq!(f.snapshot().await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_missing_a_switch_or_with_an_extra_member_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let before = f.snapshot().await;
    let mut missing = defaults();
    missing.as_object_mut().unwrap().remove("enabled");
    let mut extra = defaults();
    extra["sso"] = true.into();

    for body in [missing, extra, json!({ "enabled": null })] {
        let refused = f.put(&admin, body.clone()).await;
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::BAD_REQUEST, "request.malformed"),
            "{body}"
        );
    }
    assert_eq!(f.snapshot().await, before);
}

/// Q2: the switches show in the organization settings, but only this route writes them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_settings_route_reads_the_switches_and_refuses_to_write_them(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    let before = f.snapshot().await;

    let read = f.send(Method::GET, SETTINGS, &admin, None).await;
    assert_eq!(
        read.body["local_sign_in"],
        json!({ "passkey": true, "password": true, "totp": true, "magic_link": false })
    );
    assert_schema("OrgSettings", &read.body);

    let patch = json!({ "local_sign_in": { "passkey": false } });
    let refused = f.send(Method::PATCH, SETTINGS, &admin, Some(patch)).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{refused:?}"
    );
    assert_eq!(refused.body["errors"][0]["field"], "/local_sign_in");
    assert_eq!(refused.body["errors"][0]["code"], "settings.read_only");
    assert_eq!(f.snapshot().await, before);
}

/// B2: a method the organization does not offer answers 404 `auth.method_disabled`, whether
/// its switch or the whole row is off, and is offered again once switched back on.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn require_method_follows_the_switches_and_the_row(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.outside_provider("example-oidc", true).await;
    let s = scope(f.org);
    for method in [
        LocalMethod::Passkey,
        LocalMethod::Password,
        LocalMethod::Totp,
    ] {
        assert!(
            require_method(&f.pool, s, method).await.is_ok(),
            "{method:?}"
        );
    }
    let magic = require_method(&f.pool, s, LocalMethod::MagicLink).await;
    assert_eq!(magic.unwrap_err().code(), "auth.method_disabled");

    let mut off = defaults();
    off["passkey"] = false.into();
    f.put(&admin, off).await;
    let disabled = require_method(&f.pool, s, LocalMethod::Passkey)
        .await
        .unwrap_err()
        .problem();
    assert_eq!(
        (disabled.status, disabled.code.as_str()),
        (404, "auth.method_disabled")
    );
    assert!(
        require_method(&f.pool, s, LocalMethod::Password)
            .await
            .is_ok()
    );

    let mut row_off = defaults();
    row_off["enabled"] = false.into();
    let answered = f.put(&admin, row_off).await;
    assert_eq!(answered.status, StatusCode::OK, "{answered:?}");
    let password = require_method(&f.pool, s, LocalMethod::Password).await;
    assert_eq!(password.unwrap_err().code(), "auth.method_disabled");

    f.put(&admin, defaults()).await;
    for method in [LocalMethod::Passkey, LocalMethod::Password] {
        assert!(
            require_method(&f.pool, s, method).await.is_ok(),
            "{method:?}"
        );
    }
}

/// An organization created before the local provider migration gets its row at startup.
#[sqlx::test(migrations = false)]
async fn startup_gives_every_organization_its_local_row(pool: PgPool) {
    let clock = ManualClock::new(now());
    startup::prepare(&pool, &clock.clock())
        .await
        .expect("first start");
    let bare = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'bare', 'Bare')")
        .bind(bare)
        .execute(&pool)
        .await
        .unwrap();
    assert!(local_sign_in::get(&pool, scope(bare)).await.is_err());

    startup::prepare(&pool, &clock.clock())
        .await
        .expect("second start");

    let local = local_sign_in::get(&pool, scope(bare)).await.unwrap();
    assert!(local.enabled);
    assert!(local.offers(LocalMethod::Passkey));
}

/// B2, coordinator decision: a change that would leave the organization no primary way to sign
/// in answers 409 `local.last_method` and writes nothing. TOTP alone is no way in, a disabled
/// outside provider does not count, and an enabled one lifts the refusal.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_leaving_no_way_to_sign_in_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.admin().await;
    f.outside_provider("disabled-oidc", false).await;
    let mut row_off = defaults();
    row_off["enabled"] = false.into();
    let mut totp_only = defaults();
    totp_only["passkey"] = false.into();
    totp_only["password"] = false.into();

    for body in [row_off.clone(), totp_only.clone()] {
        let before = f.snapshot().await;
        let refused = f.put(&admin, body.clone()).await;
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::CONFLICT, "local.last_method"),
            "{body}: {refused:?}"
        );
        assert_eq!(f.snapshot().await, before, "{body}");
    }

    let mut password_only = defaults();
    password_only["passkey"] = false.into();
    let kept = f.put(&admin, password_only).await;
    assert_eq!(
        kept.status,
        StatusCode::OK,
        "one primary method is enough: {kept:?}"
    );

    f.outside_provider("example-oidc", true).await;
    for body in [totp_only, row_off] {
        let allowed = f.put(&admin, body.clone()).await;
        assert_eq!(allowed.status, StatusCode::OK, "{body}: {allowed:?}");
        assert_eq!(allowed.body, body);
    }
    let local = local_sign_in::get(&f.pool, scope(f.org)).await.unwrap();
    assert!(local.offered().is_empty());
}
