// SPDX-License-Identifier: AGPL-3.0-or-later
//! The branding routes and `/theme.css` over the router
//! (docs/specs/m0/12-tokens-and-theme-generator.md B21, B24, B25; T14 to T17). The permission
//! suite's four cases and cross-organization case for each route are in `tests/permissions.rs`;
//! these tests cover what the suite cannot: the stored stylesheet, `/theme.css` before and after
//! a save, validation, the feature check and one organization's save against another's theme.
//!
//! Two organizations share the database, so every request names its own with the
//! `test-support` organization header. `session::start` stands in for the sign-in specs. Needs
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
    audit::Actor,
    branding::css_hash,
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles,
    theme::{self, Brand},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, branding::THEME_CSS_CACHE_CONTROL, request_org::TEST_ORG_HEADER, router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const BRANDING: &str = "/api/v1/admin/branding";
const THEME_CSS: &str = "/theme.css";

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

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    text: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or("").to_owned()
    }
}

struct World {
    pool: PgPool,
    app: Router,
}

impl World {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let state = AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app }
    }

    async fn org(&self, slug: &str) -> Uuid {
        organizations::create(&self.pool, slug, slug, now())
            .await
            .expect("create organization")
            .id
    }

    /// A signed-in user of `org` holding the built-in role `role` at organization scope, or no
    /// role: their `Cookie` value.
    async fn user(&self, org: Uuid, role: Option<&str>) -> String {
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
        let id = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        if let Some(role) = role {
            let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
                .bind(role)
                .fetch_one(&self.pool)
                .await
                .expect("a built-in role");
            let assignment = NewAssignment {
                user_id: id,
                role_id: role,
                scope: Scope::Organization,
                source: Source::Local,
                external_id: None,
            };
            role_assignments::create(&self.pool, scope(org), &assignment, now(), Actor::Instance)
                .await
                .expect("assign the role");
        }
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn send(
        &self,
        org: Uuid,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
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
        Answer {
            status,
            headers,
            text: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
        }
    }

    async fn theme_css(&self, org: Uuid) -> Answer {
        self.send(org, Method::GET, THEME_CSS, None, None).await
    }

    async fn stored(&self, org: Uuid) -> (String, i64) {
        sqlx::query_as(
            "SELECT branding::text, (extract(epoch FROM updated_at) * 1000000)::bigint
             FROM organizations WHERE id = $1",
        )
        .bind(org)
        .fetch_one(&self.pool)
        .await
        .expect("the organization")
    }
}

fn default_css() -> String {
    theme::render(&Brand::stepclimb_default())
        .expect("the default brand renders")
        .css()
        .to_owned()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_theme_css_serves_the_default_then_the_saved_stylesheet(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let admin = w.user(a, Some("org_admin")).await;

    let before = w.theme_css(a).await;
    assert_eq!(before.status, StatusCode::OK, "{before:?}");
    assert_eq!(
        before.headers[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    assert_eq!(
        before.headers[header::CACHE_CONTROL],
        THEME_CSS_CACHE_CONTROL
    );
    assert_eq!(before.text, default_css());

    let read = w.send(a, Method::GET, BRANDING, Some(&admin), None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_schema("Branding", &read.json());
    assert_eq!(read.json()["css_hash"], css_hash(&before.text));
    assert_eq!(read.json()["inputs"]["primary"], "oklch(0.5 0.09 195)");

    let inputs = json!({ "primary": "#500E0E", "ui_font": "Roboto", "corner_style": "round" });
    let saved = w
        .send(a, Method::PUT, BRANDING, Some(&admin), Some(inputs))
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{saved:?}");
    let body = saved.json();
    assert_schema("Branding", &body);
    assert_eq!(
        body["inputs"],
        json!({
            "primary": "#500E0E",
            "ui_font": "Roboto",
            "reading_font": "serif",
            "corner_style": "round",
        })
    );
    assert_eq!(body["generator"], theme::GENERATOR_VERSION);
    assert_eq!(body["report"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        body["report"][0]["warnings"][0]["code"],
        "brand-near-danger"
    );

    let after = w.theme_css(a).await;
    assert_eq!(after.status, StatusCode::OK);
    assert_ne!(after.text, before.text);
    assert_eq!(body["css_hash"], css_hash(&after.text));
    assert_ne!(body["css_hash"], read.json()["css_hash"]);
    assert!(
        after.text.contains("--sc-font-ui: \"Roboto\""),
        "{}",
        after.text
    );
    assert!(after.text.contains("--sc-radius-md: 16px;"));
    let reread = w.send(a, Method::GET, BRANDING, Some(&admin), None).await;
    assert_eq!(reread.json(), body);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_an_invalid_input_is_422_naming_it_and_changes_nothing(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let admin = w.user(a, Some("org_admin")).await;
    let first = json!({ "primary": "#0a1030" });
    let saved = w
        .send(a, Method::PUT, BRANDING, Some(&admin), Some(first))
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{saved:?}");
    let stored = w.stored(a).await;
    let css = w.theme_css(a).await.text;

    for (body, field, rule) in [
        (
            json!({ "primary": "dark red" }),
            "/primary",
            "branding.invalid_color",
        ),
        (
            json!({ "primary": "oklch(0.5 1e400 10)" }),
            "/primary",
            "branding.invalid_color",
        ),
        // A chroma above 0.5: 1e150 once made the generator find no step and answer 500.
        (
            json!({ "primary": "oklch(0.5 1e150 10)" }),
            "/primary",
            "branding.invalid_color",
        ),
        (
            json!({ "primary": "oklch(0.5 0.5001 10)" }),
            "/primary",
            "branding.invalid_color",
        ),
        (
            json!({ "primary": format!("oklch(0.5 0.1 {})", "1".repeat(60)) }),
            "/primary",
            "branding.too_long",
        ),
        (
            json!({ "primary": "#500E0E", "ui_font": "Comic Sans" }),
            "/ui_font",
            "branding.not_a_choice",
        ),
        (
            json!({ "primary": "#500E0E", "reading_font": "mono" }),
            "/reading_font",
            "branding.not_a_choice",
        ),
        (
            json!({ "primary": "#500E0E", "corner_style": "pointy" }),
            "/corner_style",
            "branding.not_a_choice",
        ),
    ] {
        let answer = w
            .send(a, Method::PUT, BRANDING, Some(&admin), Some(body.clone()))
            .await;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {answer:?}"
        );
        assert_eq!(answer.code(), "branding.invalid_input", "{body}");
        assert_eq!(
            answer.json()["errors"],
            json!([{ "field": field, "code": rule }]),
            "{body}"
        );
    }
    for body in [
        json!({}),
        json!({ "primary": 5 }),
        json!({ "primary": "#fff", "accent": "#000" }),
    ] {
        let answer = w
            .send(a, Method::PUT, BRANDING, Some(&admin), Some(body.clone()))
            .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}: {answer:?}");
    }
    assert_eq!(w.stored(a).await, stored);
    assert_eq!(w.theme_css(a).await.text, css);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_one_organizations_save_never_changes_anothers_theme(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let b = w.org("org-b").await;
    let admin_a = w.user(a, Some("org_admin")).await;
    let b_before = w.stored(b).await;

    let saved = w
        .send(
            a,
            Method::PUT,
            BRANDING,
            Some(&admin_a),
            Some(json!({ "primary": "#f8f000" })),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{saved:?}");
    assert_eq!(w.stored(b).await, b_before);
    assert_eq!(w.theme_css(b).await.text, default_css());
    assert_ne!(w.theme_css(a).await.text, default_css());

    // A's session on a request for B's organization is no principal of B.
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(json!({ "primary": "#000" }))),
    ] {
        let answer = w
            .send(b, method.clone(), BRANDING, Some(&admin_a), body)
            .await;
        assert_eq!(
            answer.status,
            StatusCode::UNAUTHORIZED,
            "{method}: {answer:?}"
        );
    }
    assert_eq!(w.stored(b).await, b_before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_plan_without_custom_branding_refuses_a_change_but_not_a_read(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let admin = w.user(a, Some("org_admin")).await;
    let plan = new_id();
    sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, '{}', '{}')")
        .bind(plan)
        .bind(format!("test_{}", plan.simple()))
        .execute(&w.pool)
        .await
        .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(a)
        .bind(plan)
        .execute(&w.pool)
        .await
        .expect("move to the plan");
    let stored = w.stored(a).await;

    let refused = w
        .send(
            a,
            Method::PUT,
            BRANDING,
            Some(&admin),
            Some(json!({ "primary": "#000080" })),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{refused:?}");
    assert_eq!(refused.code(), "entitlement.feature_unavailable");
    assert_eq!(w.stored(a).await, stored);
    let read = w.send(a, Method::GET, BRANDING, Some(&admin), None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_only_the_organization_admin_role_holds_branding_manage(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let anonymous = w.send(a, Method::GET, BRANDING, None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    let holders: Vec<String> = sqlx::query_scalar(
        "SELECT key FROM roles WHERE org_id IS NULL AND permissions @> ARRAY['branding.manage']
         ORDER BY key",
    )
    .fetch_all(&w.pool)
    .await
    .expect("the built-in roles");
    assert_eq!(holders, ["org_admin"]);
    let admin = w.user(a, Some("org_admin")).await;
    let allowed = w.send(a, Method::GET, BRANDING, Some(&admin), None).await;
    assert_eq!(allowed.status, StatusCode::OK);
    let member = w.user(a, None).await;
    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(json!({ "primary": "#000" }))),
    ] {
        let denied = w
            .send(a, method.clone(), BRANDING, Some(&member), body)
            .await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN, "{method}: {denied:?}");
        assert_eq!(denied.code(), "permission.denied");
    }
}

async fn audit_entries(pool: &PgPool, org: Uuid) -> Vec<stepclimb_core::audit::Entry> {
    stepclimb_core::audit::list(pool, scope(org), 50)
        .await
        .expect("read audit entries")
        .into_iter()
        .filter(|e| e.action == "organization.update_branding")
        .collect()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_writes_one_audit_entry_and_a_refused_or_unchanged_one_none(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let admin = w.user(a, Some("org_admin")).await;
    let member = w.user(a, None).await;
    let put = |cookie: &str, primary: &str| {
        let body = json!({ "primary": primary });
        let cookie = cookie.to_owned();
        let w = &w;
        async move {
            w.send(a, Method::PUT, BRANDING, Some(&cookie), Some(body))
                .await
        }
    };

    assert_eq!(put(&member, "#500E0E").await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        put(&admin, "dark red").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(audit_entries(&w.pool, a).await.is_empty());

    let saved = put(&admin, "#500E0E").await;
    assert_eq!(saved.status, StatusCode::OK, "{saved:?}");
    assert_eq!(put(&admin, "  #500E0E ").await.status, StatusCode::OK);
    let entries = audit_entries(&w.pool, a).await;
    assert_eq!(entries.len(), 1, "{entries:#?}");
    let entry = &entries[0];
    assert_eq!(entry.target_type, "organization");
    assert_eq!(entry.target_id, a);
    assert!(
        matches!(entry.actor, Actor::User { ip, .. } if ip == CLIENT.ip()),
        "{entry:?}"
    );
    assert_eq!(entry.data["before"], Value::Null);
    assert_eq!(entry.data["after"]["inputs"], saved.json()["inputs"]);
    assert_eq!(entry.data["after"]["css_hash"], saved.json()["css_hash"]);
    assert!(!entry.data.to_string().contains("--sc-"));

    assert_eq!(put(&admin, "#0a1030").await.status, StatusCode::OK);
    let entries = audit_entries(&w.pool, a).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].data["before"], entries[0].data["after"]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_answers_the_stored_report_without_generating_it(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let admin = w.user(a, Some("org_admin")).await;
    let saved = w
        .send(
            a,
            Method::PUT,
            BRANDING,
            Some(&admin),
            Some(json!({ "primary": "#f8f000" })),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{saved:?}");
    // Rewrite the stored report: GET must answer it as stored, not a fresh one.
    sqlx::query(
        "UPDATE organizations
         SET branding = jsonb_set(branding, '{report,0,warnings}',
                                  '[{\"code\": \"stored\", \"message\": \"as stored\"}]')
         WHERE id = $1",
    )
    .bind(a)
    .execute(&w.pool)
    .await
    .expect("rewrite the stored report");
    let read = w.send(a, Method::GET, BRANDING, Some(&admin), None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_eq!(read.json()["report"][0]["warnings"][0]["code"], "stored");
    assert_eq!(read.json()["css_hash"], saved.json()["css_hash"]);
}
