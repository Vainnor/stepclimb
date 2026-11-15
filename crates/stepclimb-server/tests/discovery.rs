// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /.well-known/stepclimb` over the router (docs/specs/m0/08-sessions-and-csrf.md B38, T35;
//! `11-app-shell.md` Q4's `theme` field).
//!
//! The document is public, so these tests check what it leaves out as closely as what it lists:
//! every listed method is one the server offers (its start route or `require_method` accepts
//! it), a switched-off method and a disabled provider are absent, no provider config member but
//! `label` and no row id reaches the body, and another organization's providers never show.
//! The requests name their organization with the `test-support` header of `request_org`, so a
//! test can hold two. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    id::{Uuid, new_id},
    local_sign_in::Method,
    organizations,
};
use stepclimb_server::{
    AppState, local_sign_in::require_method, providers::Providers, request_org::TEST_ORG_HEADER,
    router, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const PATH: &str = "/.well-known/stepclimb";
const PUBLIC_URL: &str = "https://lms.example.org";

/// A value that stands for a provider's client secret: it must never reach the document.
const SECRET: &str = "client-secret-marker-7f3c";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the switches the way the method routes do"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    text: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).expect("a JSON body")
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }
}

struct Fixture {
    pool: PgPool,
    app: Router,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse(PUBLIC_URL).expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            providers: Providers::new(false).expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app }
    }

    async fn org(&self, slug: &str, name: &str) -> Uuid {
        organizations::create(&self.pool, slug, name, now())
            .await
            .expect("create organization")
            .id
    }

    /// A provider of `org` whose config holds a label, an issuer, a client id and a sealed-looking
    /// client secret, as a written provider's does.
    async fn provider(&self, org: Uuid, key: &str, kind: &str, enabled: bool) -> Uuid {
        let id = new_id();
        let config = json!({
            "label": format!("Sign in with {key}"),
            "issuer": "https://idp.example.org/realm",
            "client_id": "client-id-marker",
            "client_secret": { "kid": "test", "sealed": SECRET },
        });
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(org)
        .bind(key)
        .bind(kind)
        .bind(config)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("insert provider");
        id
    }

    /// Set `org`'s method switches to `switches`, bypassing the route's last-method refusal.
    async fn switches(&self, org: Uuid, switches: Value) {
        sqlx::query(
            "UPDATE organizations \
             SET settings = settings || jsonb_build_object('local_sign_in', $2::jsonb) \
             WHERE id = $1",
        )
        .bind(org)
        .bind(switches)
        .execute(&self.pool)
        .await
        .expect("set the switches");
    }

    async fn local_row(&self, org: Uuid, enabled: bool) {
        sqlx::query(
            "UPDATE identity_providers SET enabled = $2 WHERE org_id = $1 AND kind = 'local'",
        )
        .bind(org)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("switch the local row");
    }

    async fn get(&self, org: Option<Uuid>, path: &str) -> Answer {
        let mut request = Request::get(path);
        if let Some(org) = org {
            request = request.header(TEST_ORG_HEADER, org.to_string());
        }
        let request = request.body(Body::empty()).expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.expect("a body");
        Answer {
            status,
            headers,
            text: String::from_utf8_lossy(&body.to_bytes()).into_owned(),
        }
    }

    async fn discovery(&self, org: Uuid) -> Answer {
        let answer = self.get(Some(org), PATH).await;
        assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
        answer
    }
}

fn document() -> Value {
    serde_json::to_value(stepclimb_server::openapi::document()).expect("the document as JSON")
}

fn local(methods: &[&str]) -> Value {
    json!({ "key": "local", "kind": "local", "label": null, "methods": methods })
}

fn provider_entry(key: &str, kind: &str) -> Value {
    json!({ "key": key, "kind": kind, "label": format!("Sign in with {key}") })
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_organization_offers_its_default_local_methods(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    let answer = fx.discovery(org).await;
    assert_eq!(answer.header(header::CACHE_CONTROL), "no-cache");
    assert!(
        answer
            .header(header::CONTENT_TYPE)
            .starts_with("application/json"),
        "{answer:?}"
    );
    let body = answer.json();
    assert_eq!(body["name"], "Org A");
    assert_eq!(
        body["sign_in"],
        json!([local(&["passkey", "password", "totp"])])
    );
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["name", "sign_in", "theme"]);
    let errors = common::schema_errors(&document(), "Discovery", &body);
    assert!(errors.is_empty(), "{errors:#?}");
}

/// T35: exactly the enabled providers, each with `key`, `kind` and `label` only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn it_lists_exactly_the_enabled_providers_and_leaks_nothing_else(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    let oidc = fx.provider(org, "acme", "oidc", true).await;
    let oauth2 = fx.provider(org, "beta", "oauth2", true).await;
    let disabled = fx.provider(org, "gamma", "oidc", false).await;
    let answer = fx.discovery(org).await;
    let body = answer.json();
    assert_eq!(
        body["sign_in"],
        json!([
            local(&["passkey", "password", "totp"]),
            provider_entry("acme", "oidc"),
            provider_entry("beta", "oauth2"),
        ])
    );
    for entry in body["sign_in"].as_array().unwrap().iter().skip(1) {
        let keys: Vec<&String> = entry.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["key", "kind", "label"], "{entry}");
    }
    for leaked in [
        SECRET,
        "client_secret",
        "client-id-marker",
        "idp.example.org",
        "gamma",
        &oidc.to_string(),
        &oauth2.to_string(),
        &disabled.to_string(),
        &org.to_string(),
    ] {
        assert!(!answer.text.contains(leaked), "{leaked} in {}", answer.text);
    }
    let errors = common::schema_errors(&document(), "Discovery", &body);
    assert!(errors.is_empty(), "{errors:#?}");
}

/// Every provider the document lists gets past its start route's provider check, and the
/// disabled one does not: the document never offers a provider the server refuses.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_listed_provider_is_one_its_start_route_accepts(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    fx.provider(org, "acme", "oidc", true).await;
    fx.provider(org, "beta", "oauth2", true).await;
    fx.provider(org, "gamma", "oidc", false).await;
    let body = fx.discovery(org).await.json();
    let listed: Vec<String> = body["sign_in"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] != "local")
        .map(|entry| entry["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(listed, ["acme", "beta"]);
    let not_found = "/sign-in?error=auth.provider_not_found";
    for key in &listed {
        let start = fx.get(Some(org), &format!("/auth/{key}/start")).await;
        assert_eq!(start.status, StatusCode::SEE_OTHER, "{start:?}");
        assert_ne!(start.header(header::LOCATION), not_found, "{key}");
    }
    let start = fx.get(Some(org), "/auth/gamma/start").await;
    assert_eq!(start.header(header::LOCATION), not_found);
}

/// Every local method the document lists passes `require_method`, the check each method's route
/// makes first, and every method it leaves out fails it.
async fn assert_local_methods_are_offered(fx: &Fixture, org: Uuid, expected: &[&str]) {
    let body = fx.discovery(org).await.json();
    let listed: Vec<String> = body["sign_in"]
        .as_array()
        .expect("sign_in is a list")
        .iter()
        .filter(|entry| entry["kind"] == "local")
        .flat_map(|entry| entry["methods"].as_array().expect("a method list").clone())
        .map(|method| method.as_str().expect("a method name").to_owned())
        .collect();
    assert_eq!(listed, expected, "{body}");
    for method in Method::ALL {
        let offered = require_method(&fx.pool, scope(org), method).await.is_ok();
        let is_listed = listed.iter().any(|name| name == method.as_str());
        assert_eq!(offered, is_listed, "{}: offered {offered}", method.as_str());
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_switched_off_local_method_is_absent(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    assert_local_methods_are_offered(&fx, org, &["passkey", "password", "totp"]).await;
    fx.switches(
        org,
        json!({ "passkey": true, "password": false, "totp": true, "magic_link": false }),
    )
    .await;
    assert_local_methods_are_offered(&fx, org, &["passkey", "totp"]).await;
    fx.switches(
        org,
        json!({ "passkey": false, "password": true, "totp": false, "magic_link": false }),
    )
    .await;
    assert_local_methods_are_offered(&fx, org, &["password"]).await;
}

/// TOTP alone is not a way in, and a disabled local row offers nothing: no local entry, while
/// an enabled provider is still listed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn local_sign_in_is_absent_with_totp_alone_or_the_row_disabled(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    fx.provider(org, "acme", "oidc", true).await;
    fx.switches(
        org,
        json!({ "passkey": false, "password": false, "totp": true, "magic_link": false }),
    )
    .await;
    let body = fx.discovery(org).await.json();
    assert_eq!(body["sign_in"], json!([provider_entry("acme", "oidc")]));

    fx.switches(
        org,
        json!({ "passkey": true, "password": true, "totp": true, "magic_link": false }),
    )
    .await;
    fx.local_row(org, false).await;
    let body = fx.discovery(org).await.json();
    assert_eq!(body["sign_in"], json!([provider_entry("acme", "oidc")]));
    assert!(
        require_method(&fx.pool, scope(org), Method::Passkey)
            .await
            .is_err()
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_with_no_enabled_method_lists_none(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    fx.provider(org, "gamma", "oidc", false).await;
    fx.local_row(org, false).await;
    let body = fx.discovery(org).await.json();
    assert_eq!(body["sign_in"], json!([]));
    assert_eq!(body["name"], "Org A");
}

/// Each organization's document lists its own name and providers only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_providers_never_show(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org_a = fx.org("org-a", "Org A").await;
    let org_b = fx.org("org-b", "Org B").await;
    fx.provider(org_a, "acme", "oidc", true).await;
    let bravo = fx.provider(org_b, "bravo", "oauth2", true).await;
    fx.local_row(org_b, false).await;

    let a = fx.discovery(org_a).await;
    let body = a.json();
    assert_eq!(body["name"], "Org A");
    assert_eq!(
        body["sign_in"],
        json!([
            local(&["passkey", "password", "totp"]),
            provider_entry("acme", "oidc"),
        ])
    );
    for leaked in ["bravo", "Org B", &bravo.to_string(), &org_b.to_string()] {
        assert!(!a.text.contains(leaked), "{leaked} in {}", a.text);
    }

    let b = fx.discovery(org_b).await.json();
    assert_eq!(b["name"], "Org B");
    assert_eq!(b["sign_in"], json!([provider_entry("bravo", "oauth2")]));
}

/// `theme.stylesheet` is the URL the served page links: `/theme.css` answers it as the
/// fingerprinted, year-long stylesheet.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_theme_is_the_fingerprinted_stylesheet(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("org-a", "Org A").await;
    let body = fx.discovery(org).await.json();
    let stylesheet = body["theme"]["stylesheet"].as_str().unwrap();
    let path = stylesheet
        .strip_prefix(PUBLIC_URL)
        .unwrap_or_else(|| panic!("{stylesheet} is not under the public URL"));
    let hash = path.strip_prefix("/theme.css?v=").expect("a fingerprint");
    assert_eq!(hash.len(), 64);
    let css = fx.get(Some(org), path).await;
    assert_eq!(css.status, StatusCode::OK);
    assert_eq!(
        css.header(header::CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
}

/// The organization comes from `RequestOrg`: none answers 503 `org.none`, several without a
/// way to tell them apart 404 `org.not_found`, as every unauthenticated route does.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unresolved_organization_answers_its_problem(pool: PgPool) {
    let fx = Fixture::new(pool);
    let none = fx.get(None, PATH).await;
    assert_eq!(none.status, StatusCode::SERVICE_UNAVAILABLE, "{none:?}");
    assert_eq!(none.json()["code"], "org.none");

    fx.org("org-a", "Org A").await;
    let one = fx.get(None, PATH).await;
    assert_eq!(one.status, StatusCode::OK, "{one:?}");

    fx.org("org-b", "Org B").await;
    let several = fx.get(None, PATH).await;
    assert_eq!(several.status, StatusCode::NOT_FOUND, "{several:?}");
    assert_eq!(several.json()["code"], "org.not_found");
    let unknown = fx.get(Some(new_id()), PATH).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{unknown:?}");
}
