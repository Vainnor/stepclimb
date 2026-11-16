// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /manifest.webmanifest` over the router
//! (docs/specs/m1/15-installable-web-app-and-offline-reading.md B1, B2, T1).
//!
//! The manifest is public and names the request's organization: these tests check its name, its
//! colors before and after the organization saves branding, that one organization never gets
//! another's, and that the page the server writes carries the same `theme-color`. The requests
//! name their organization with the `test-support` header of `request_org`. Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    appearance::Theme,
    audit::Actor,
    branding,
    clock::ManualClock,
    id::{Uuid, new_id},
    organizations,
    theme::{self, Brand, BrandInputs},
};
use stepclimb_server::{
    AppState,
    manifest::{LightColors, light_colors},
    providers::Providers,
    request_org::TEST_ORG_HEADER,
    router,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const PATH: &str = "/manifest.webmanifest";

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test saves branding as the branding route does, in the organization's scope"
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
        let state = AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            providers: Providers::new(false).expect("provider clients"),
            passwords: Default::default(),
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

    async fn save_branding(&self, org: Uuid, primary: &str) -> Brand {
        let brand = Brand::parse(&BrandInputs {
            primary: primary.into(),
            ..BrandInputs::default()
        })
        .expect("valid inputs");
        let rendered = theme::render(&brand).expect("the brand renders");
        branding::save(
            &self.pool,
            scope(org),
            &rendered,
            Theme::System,
            now(),
            Actor::Instance,
        )
        .await
        .expect("save branding");
        brand
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
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        Answer {
            status,
            headers,
            text: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }
}

fn document() -> Value {
    serde_json::from_str(&stepclimb_server::openapi::document_json()).expect("the document")
}

fn default_colors() -> LightColors {
    light_colors(&Brand::stepclimb_default()).expect("the default brand renders")
}

/// T1: the manifest names the organization, uses its light theme's colors and lists the
/// default icons, as its schema documents.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_the_manifest_names_the_organization_in_its_colors(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("example", "Example Training Group").await;

    let before = fx.get(Some(org), PATH).await;
    assert_eq!(before.status, StatusCode::OK, "{before:?}");
    assert_eq!(
        before.header(header::CONTENT_TYPE),
        "application/manifest+json"
    );
    assert_eq!(before.header(header::CACHE_CONTROL), "no-cache");
    let body = before.json();
    let errors = common::schema_errors(&document(), "WebManifest", &body);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(body["name"], "Example Training Group");
    assert_eq!(body["short_name"], "Example Trai");
    assert_eq!(body["start_url"], "/");
    assert_eq!(body["scope"], "/");
    assert_eq!(body["display"], "standalone");
    let default = default_colors();
    assert_eq!(body["theme_color"], default.brand.as_str());
    assert_eq!(body["background_color"], default.canvas.as_str());
    let sizes: Vec<(&str, &str)> = body["icons"]
        .as_array()
        .expect("icons")
        .iter()
        .map(|icon| {
            (
                icon["sizes"].as_str().unwrap(),
                icon["purpose"].as_str().unwrap_or("any"),
            )
        })
        .collect();
    assert_eq!(
        sizes,
        [
            ("192x192", "any"),
            ("512x512", "any"),
            ("512x512", "maskable")
        ]
    );

    let brand = fx.save_branding(org, "#500E0E").await;
    let saved = light_colors(&brand).expect("the brand renders");
    assert_ne!(saved, default);
    let after = fx.get(Some(org), PATH).await.json();
    assert_eq!(after["theme_color"], saved.brand.as_str());
    assert_eq!(after["background_color"], saved.canvas.as_str());
}

/// B2: the page the server writes carries the organization's `theme-color`, as its manifest
/// does.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_page_carries_the_manifests_theme_color(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("example", "Example Training").await;
    let meta = |color: &str| format!(r#"<meta name="theme-color" content="{color}" />"#);

    let page = fx.get(Some(org), "/").await;
    assert_eq!(page.status, StatusCode::OK, "is web/dist built? {page:?}");
    assert!(
        page.text.contains(&meta(&default_colors().brand)),
        "{}",
        page.text
    );
    assert!(
        page.text
            .contains(r#"<link rel="manifest" href="/manifest.webmanifest" />"#)
    );

    fx.save_branding(org, "#500E0E").await;
    let manifest = fx.get(Some(org), PATH).await.json();
    let color = manifest["theme_color"].as_str().expect("a color");
    let page = fx.get(Some(org), "/").await;
    assert!(page.text.contains(&meta(color)), "{}", page.text);
}

/// One organization's manifest never carries another's name or colors.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_organization_gets_its_own_manifest(pool: PgPool) {
    let fx = Fixture::new(pool);
    let a = fx.org("org-a", "Org A").await;
    let b = fx.org("org-b", "Org B").await;
    let brand = fx.save_branding(a, "#500E0E").await;

    let a_body = fx.get(Some(a), PATH).await.json();
    let b_body = fx.get(Some(b), PATH).await.json();
    assert_eq!(a_body["name"], "Org A");
    assert_eq!(b_body["name"], "Org B");
    let a_colors = light_colors(&brand).expect("the brand renders");
    assert_eq!(a_body["theme_color"], a_colors.brand.as_str());
    assert_eq!(b_body["theme_color"], default_colors().brand.as_str());
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

/// Saved inputs this generator refuses are served the default brand's stylesheet (spec 12 B28),
/// and the manifest takes the default brand's colors with it, not colors of a theme no page
/// draws.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn refused_saved_inputs_give_the_default_brands_colors(pool: PgPool) {
    let fx = Fixture::new(pool);
    let org = fx.org("example", "Example Training").await;
    fx.save_branding(org, "#500E0E").await;
    // As a release before the chroma cap stored it: another generator, inputs this one refuses.
    let mut record: Value = serde_json::from_str(
        &sqlx::query_scalar::<_, String>("SELECT branding::text FROM organizations WHERE id = $1")
            .bind(org)
            .fetch_one(&fx.pool)
            .await
            .expect("the record"),
    )
    .expect("JSON");
    record["generator"] = 0.into();
    record["inputs"]["primary"] = "oklch(0.5 0.9 30)".into();
    sqlx::query("UPDATE organizations SET branding = $2::text::jsonb WHERE id = $1")
        .bind(org)
        .bind(record.to_string())
        .execute(&fx.pool)
        .await
        .expect("age the record");

    let body = fx.get(Some(org), PATH).await.json();
    let default = default_colors();
    assert_eq!(body["theme_color"], default.brand.as_str());
    assert_eq!(body["background_color"], default.canvas.as_str());
}
