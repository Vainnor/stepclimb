// SPDX-License-Identifier: AGPL-3.0-or-later
//! The appearance choice, the page the server writes and the fingerprinted `/theme.css`
//! (docs/specs/m0/11-app-shell.md B20, B21, Q4, Q6; T20). The permission suite's 401, 2xx and
//! cross-organization cases for `PUT /api/v1/me/appearance` are in `tests/permissions.rs`; these
//! tests cover what it cannot: the stored choice, its cookie, `index.html` for a signed-in and a
//! signed-out browser, and one organization's page and stylesheet against another's.
//!
//! Requests name their organization with the `test-support` header, except the one that checks
//! an instance without an organization. The page tests read the built `web/dist/index.html`
//! (`just web-build`, which `just test-rust` runs first). Needs `DATABASE_URL` (`just up`).

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
    branding::{self, css_hash},
    clock::ManualClock,
    id::Uuid,
    organizations, roles,
    theme::{self, Brand, BrandInputs},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    branding::{THEME_CSS_CACHE_CONTROL, THEME_CSS_IMMUTABLE},
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const APPEARANCE: &str = "/api/v1/me/appearance";

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

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .map_or("", |value| value.to_str().expect("ASCII"))
    }

    /// The `<html ...>` start tag of a page.
    fn html_tag(&self) -> &str {
        let start = self.text.find("<html").expect("a page");
        let end = start + self.text[start..].find('>').expect("a start tag");
        &self.text[start..=end]
    }
}

struct World {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
}

/// A signed-in user: their id and `Cookie` value.
struct User {
    id: Uuid,
    cookie: String,
}

impl World {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
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
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app, clock }
    }

    async fn org(&self, slug: &str) -> Uuid {
        organizations::create(&self.pool, slug, slug, now())
            .await
            .expect("create organization")
            .id
    }

    async fn user(&self, org: Uuid) -> User {
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
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        let cookie = cookie.split(';').next().expect("a pair").to_owned();
        User { id, cookie }
    }

    /// Saves `primary` as the organization's brand: the stylesheet's hash.
    async fn brand(&self, org: Uuid, primary: &str) -> String {
        let brand = Brand::parse(&BrandInputs {
            primary: primary.into(),
            ..BrandInputs::default()
        })
        .expect("a valid brand");
        let rendered = theme::render(&brand).expect("the brand renders");
        branding::save(&self.pool, scope(org), &rendered, now(), Actor::Instance)
            .await
            .expect("save the branding")
            .css_hash
    }

    async fn send(
        &self,
        org: Option<Uuid>,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin");
        if let Some(org) = org {
            request = request.header(TEST_ORG_HEADER, org.to_string());
        }
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

    async fn page(&self, org: Option<Uuid>, cookie: Option<&str>) -> Answer {
        let page = self.send(org, Method::GET, "/courses", cookie, None).await;
        assert_eq!(page.status, StatusCode::OK, "is web/dist built? {page:?}");
        page
    }

    async fn put(&self, org: Uuid, user: &User, body: Value) -> Answer {
        self.send(
            Some(org),
            Method::PUT,
            APPEARANCE,
            Some(&user.cookie),
            Some(body),
        )
        .await
    }

    async fn me(&self, org: Uuid, user: &User) -> Value {
        let me = self
            .send(
                Some(org),
                Method::GET,
                "/api/v1/me",
                Some(&user.cookie),
                None,
            )
            .await;
        assert_eq!(me.status, StatusCode::OK, "{me:?}");
        me.json()
    }

    async fn stored(&self, user: Uuid) -> (String, String, i64) {
        sqlx::query_as(
            "SELECT theme, density, (extract(epoch FROM updated_at) * 1000000)::bigint
             FROM users WHERE id = $1",
        )
        .bind(user)
        .fetch_one(&self.pool)
        .await
        .expect("the user")
    }
}

/// The hash of the default brand's stylesheet.
fn default_hash() -> String {
    let rendered = theme::render(&Brand::stepclimb_default()).expect("the default brand renders");
    css_hash(rendered.css())
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_user_follows_the_system_and_me_says_so(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let pat = w.user(a).await;
    let me = w.me(a, &pat).await;
    assert_eq!(
        me["appearance"],
        json!({ "theme": "system", "density": "comfortable" })
    );
    let errors = common::schema_errors(&document(), "Me", &me);
    assert!(errors.is_empty(), "{errors:#?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_choice_is_stored_returned_in_me_and_set_as_the_cookie(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let pat = w.user(a).await;
    let choice = json!({ "theme": "high-contrast", "density": "compact" });
    let answer = w.put(a, &pat, choice.clone()).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.json(), choice);
    let errors = common::schema_errors(&document(), "UserAppearance", &answer.json());
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        answer.header(header::SET_COOKIE),
        "__Host-sc_appearance=high-contrast.compact; Max-Age=31536000; Path=/; Secure; HttpOnly; SameSite=Lax"
    );
    assert_eq!(answer.header(header::CACHE_CONTROL), "no-store");
    assert_eq!(w.me(a, &pat).await["appearance"], choice);
    let (theme, density, _) = w.stored(pat.id).await;
    assert_eq!(
        (theme.as_str(), density.as_str()),
        ("high-contrast", "compact")
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_same_choice_again_leaves_updated_at(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let pat = w.user(a).await;
    let before = w.stored(pat.id).await;
    w.clock.advance(Duration::from_secs(60));
    let same = w
        .put(
            a,
            &pat,
            json!({ "theme": "system", "density": "comfortable" }),
        )
        .await;
    assert_eq!(same.status, StatusCode::OK, "{same:?}");
    assert_eq!(w.stored(pat.id).await, before);
    let changed = w
        .put(
            a,
            &pat,
            json!({ "theme": "light", "density": "comfortable" }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{changed:?}");
    let (_, _, updated_at) = w.stored(pat.id).await;
    assert_eq!(updated_at, before.2 + 60_000_000);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_value_that_is_not_a_choice_is_400_and_changes_nothing(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let pat = w.user(a).await;
    let before = w.stored(pat.id).await;
    for body in [
        json!({ "theme": "sepia", "density": "compact" }),
        json!({ "theme": "dark", "density": "roomy" }),
        json!({ "theme": "dark" }),
        json!({ "theme": "dark", "density": "compact", "brand": "vzdc" }),
        json!({ "theme": null, "density": "compact" }),
    ] {
        let answer = w.put(a, &pat, body.clone()).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{body}: {answer:?}");
        assert_eq!(answer.json()["code"], "request.malformed", "{body}");
        assert!(answer.headers.get(header::SET_COOKIE).is_none(), "{body}");
    }
    assert_eq!(w.stored(pat.id).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_choice_changes_only_the_callers_row(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let b = w.org("org-b").await;
    let pat = w.user(a).await;
    let colleague = w.user(a).await;
    let other = w.user(b).await;
    let answer = w
        .put(a, &pat, json!({ "theme": "dark", "density": "compact" }))
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    for user in [colleague.id, other.id] {
        let (theme, density, _) = w.stored(user).await;
        assert_eq!(
            (theme.as_str(), density.as_str()),
            ("system", "comfortable")
        );
    }
    // A's session on a request for B's organization is nobody there.
    let cross = w
        .put(b, &pat, json!({ "theme": "light", "density": "compact" }))
        .await;
    assert_eq!(cross.status, StatusCode::UNAUTHORIZED, "{cross:?}");
    let (theme, _, _) = w.stored(pat.id).await;
    assert_eq!(theme, "dark");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_page_carries_the_signed_in_users_choice_over_the_cookie(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let pat = w.user(a).await;
    let page = w.page(Some(a), Some(&pat.cookie)).await;
    assert_eq!(page.html_tag(), r#"<html lang="en">"#);
    assert_eq!(page.header(header::CONTENT_TYPE), "text/html");
    assert_eq!(page.header(header::CACHE_CONTROL), "private, no-cache");
    assert_eq!(page.header(header::VARY), "Cookie");

    let answer = w
        .put(a, &pat, json!({ "theme": "dark", "density": "compact" }))
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let page = w.page(Some(a), Some(&pat.cookie)).await;
    assert_eq!(
        page.html_tag(),
        r#"<html data-sc-theme="dark" data-sc-density="compact" lang="en">"#
    );
    // The profile wins over a cookie another choice left in this browser.
    let both = format!("{}; __Host-sc_appearance=light.comfortable", pat.cookie);
    let page = w.page(Some(a), Some(&both)).await;
    assert_eq!(
        page.html_tag(),
        r#"<html data-sc-theme="dark" data-sc-density="compact" lang="en">"#
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_signed_out_page_carries_the_cookies_choice(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    for (cookie, tag) in [
        (None, r#"<html lang="en">"#),
        (
            Some("__Host-sc_appearance=high-contrast.comfortable"),
            r#"<html data-sc-theme="high-contrast" lang="en">"#,
        ),
        (
            Some("__Host-sc_appearance=system.compact"),
            r#"<html data-sc-density="compact" lang="en">"#,
        ),
        (
            Some(r#"__Host-sc_appearance=dark" onload="x.compact"#),
            r#"<html lang="en">"#,
        ),
        // An expired or unknown session is no user: the cookie's choice applies.
        (
            Some("__Host-sc_session=unknown; __Host-sc_appearance=dark.comfortable"),
            r#"<html data-sc-theme="dark" lang="en">"#,
        ),
    ] {
        let page = w.page(Some(a), cookie).await;
        assert_eq!(page.html_tag(), tag, "{cookie:?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_the_page_links_the_stylesheet_it_is_served_with_and_that_url_is_immutable(
    pool: PgPool,
) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let link = |hash: &str| format!(r#"href="/theme.css?v={hash}""#);

    let page = w.page(Some(a), None).await;
    assert!(page.text.contains(&link(&default_hash())), "{}", page.text);

    let hash = w.brand(a, "#500E0E").await;
    assert_ne!(hash, default_hash());
    let page = w.page(Some(a), None).await;
    assert!(page.text.contains(&link(&hash)), "{}", page.text);

    let css = w
        .send(
            Some(a),
            Method::GET,
            &format!("/theme.css?v={hash}"),
            None,
            None,
        )
        .await;
    assert_eq!(css.status, StatusCode::OK, "{css:?}");
    assert_eq!(css_hash(&css.text), hash);
    assert_eq!(css.header(header::CACHE_CONTROL), THEME_CSS_IMMUTABLE);
    assert_eq!(css.header(header::CONTENT_TYPE), "text/css; charset=utf-8");

    // A page from before the change links the old hash: it gets the new stylesheet, which no
    // cache may keep under the old URL.
    for uri in [
        format!("/theme.css?v={}", default_hash()),
        "/theme.css".to_owned(),
        "/theme.css?v=".to_owned(),
    ] {
        let css = w.send(Some(a), Method::GET, &uri, None, None).await;
        assert_eq!(css.status, StatusCode::OK, "{uri}");
        assert_eq!(css_hash(&css.text), hash, "{uri}");
        assert_eq!(
            css.header(header::CACHE_CONTROL),
            THEME_CSS_CACHE_CONTROL,
            "{uri}"
        );
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn one_organizations_page_never_links_anothers_stylesheet(pool: PgPool) {
    let w = World::new(pool).await;
    let a = w.org("org-a").await;
    let b = w.org("org-b").await;
    let hash_a = w.brand(a, "#500E0E").await;
    let hash_b = w.brand(b, "#0a1030").await;
    let page_b = w.page(Some(b), None).await;
    assert!(page_b.text.contains(&format!("?v={hash_b}\"")));
    assert!(!page_b.text.contains(&hash_a));
    // B's host asking for A's hash gets B's stylesheet, revalidated.
    let css = w
        .send(
            Some(b),
            Method::GET,
            &format!("/theme.css?v={hash_a}"),
            None,
            None,
        )
        .await;
    assert_eq!(css_hash(&css.text), hash_b);
    assert_eq!(css.header(header::CACHE_CONTROL), THEME_CSS_CACHE_CONTROL);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_instance_without_an_organization_serves_the_default_theme(pool: PgPool) {
    let w = World::new(pool).await;
    let page = w.page(None, None).await;
    assert!(
        page.text.contains(&format!("?v={}\"", default_hash())),
        "{}",
        page.text
    );
    let css = w
        .send(
            None,
            Method::GET,
            &format!("/theme.css?v={}", default_hash()),
            None,
            None,
        )
        .await;
    assert_eq!(css.status, StatusCode::OK, "{css:?}");
    assert_eq!(css_hash(&css.text), default_hash());
    assert_eq!(css.header(header::CACHE_CONTROL), THEME_CSS_IMMUTABLE);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn with_two_organizations_and_no_header_the_page_is_unfingerprinted(pool: PgPool) {
    let w = World::new(pool).await;
    w.org("org-a").await;
    w.org("org-b").await;
    let page = w.page(None, None).await;
    assert!(page.text.contains(r#"href="/theme.css""#), "{}", page.text);
    let css = w.send(None, Method::GET, "/theme.css", None, None).await;
    assert_eq!(css.status, StatusCode::NOT_FOUND);
    assert_eq!(css.json()["code"], "org.not_found");
}
