// SPDX-License-Identifier: AGPL-3.0-or-later
//! Browser sessions over HTTP (docs/specs/m0/08-sessions-and-csrf.md B5 to B11; T1's database
//! half, T4 to T9, T16). A test-only route takes the `Authenticated` extractor; `session::start`
//! stands in for the sign-in specs that will call it. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
    routing::get,
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    clock::ManualClock,
    id::{Uuid, new_id},
    users::{self, NewUser, Status},
};
use stepclimb_server::{
    AppState,
    error::Problem,
    session::{self, Authenticated, Started},
    web_origin::WebOrigin,
};
use tower::ServiceExt;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// 2025-11-13T02:13:20Z.
fn sign_in_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the request-organization resolver of the sign-in specs"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// The route a signed-in user reaches: it answers the user id and the scope it was given.
async fn whoami(auth: Authenticated) -> String {
    format!(
        "{} {} {:?}",
        auth.user_id,
        auth.scope().org_id(),
        auth.scope().origin()
    )
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    org: Uuid,
    user: Uuid,
}

impl Fixture {
    /// One organization with one active user, the clock at [`sign_in_time`].
    async fn new(pool: PgPool) -> Self {
        let org = insert_org(&pool, "org-a").await;
        let user = NewUser {
            display_name: "Sam Ortiz".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(&pool, scope(org), &user, sign_in_time())
            .await
            .expect("create user")
            .id;
        Self {
            pool,
            clock: ManualClock::new(sign_in_time()),
            org,
            user,
        }
    }

    fn app(&self) -> Router {
        let state = AppState {
            db: self.pool.clone(),
            clock: self.clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
        };
        Router::new()
            .route("/whoami", get(whoami))
            .with_state(state)
    }

    /// What a sign-in does: start a session at the clock's time, from a request without cookies.
    async fn sign_in(&self) -> Started {
        self.sign_in_with(None).await
    }

    /// A sign-in from a request whose `Cookie` header is `cookie`.
    async fn sign_in_with(&self, cookie: Option<&str>) -> Started {
        let mut headers = HeaderMap::new();
        if let Some(cookie) = cookie {
            headers.insert(
                header::COOKIE,
                cookie.parse().expect("a valid Cookie header"),
            );
        }
        let mut tx = self.pool.begin().await.expect("begin");
        let started = session::start(
            &mut tx,
            scope(self.org),
            self.user,
            &headers,
            self.clock.clock().now(),
        )
        .await
        .expect("start a session");
        tx.commit().await.expect("commit");
        started
    }

    async fn whoami(&self, cookie: Option<&str>) -> Answer {
        let mut request = Request::get("/whoami");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .app()
            .oneshot(request.body(Body::empty()).expect("a valid request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        Answer {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    async fn session_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM sessions")
            .fetch_one(&self.pool)
            .await
            .expect("count sessions")
    }
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Answer {
    fn code(&self) -> String {
        serde_json::from_str::<Problem>(&self.body)
            .unwrap_or_else(|_| panic!("not a problem: {}", self.body))
            .code
    }

    fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("an ASCII Set-Cookie"))
            .collect()
    }

    fn assert_cleared(&self) {
        assert_eq!(
            self.set_cookies(),
            ["__Host-sc_session=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax"]
        );
    }
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

/// The `name=value` pair of a `Set-Cookie` value, for the `Cookie` header of the next request.
fn cookie_pair(started: &Started) -> String {
    let value = started.set_cookie.to_str().expect("an ASCII Set-Cookie");
    value.split(';').next().expect("a cookie pair").to_owned()
}

fn token_of(started: &Started) -> String {
    cookie_pair(started)
        .strip_prefix("__Host-sc_session=")
        .expect("a session cookie")
        .to_owned()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_the_cookie_has_the_b7_attributes_and_the_row_lasts_thirty_days(pool: PgPool) {
    let fixture = Fixture::new(pool).await;

    let started = fixture.sign_in().await;

    let value = started.set_cookie.to_str().unwrap();
    let mut parts = value.split("; ");
    let pair = parts.next().unwrap();
    let attributes: Vec<&str> = parts.collect();
    assert!(pair.starts_with("__Host-sc_session="), "{value}");
    assert_eq!(token_of(&started).len(), 43, "a 32-byte base64url token");
    assert_eq!(
        attributes,
        [
            "Expires=Sat, 13 Dec 2025 02:13:20 GMT",
            "Path=/",
            "Secure",
            "HttpOnly",
            "SameSite=Lax"
        ],
        "Expires is sign-in + 30 days (Q5); no Domain"
    );
    assert_eq!(started.session.expires_at, sign_in_time() + 30 * DAY);
    assert_eq!(started.session.last_seen_at, sign_in_time());

    let answer = fixture.whoami(Some(&cookie_pair(&started))).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(
        answer.body,
        format!("{} {} Session", fixture.user, fixture.org)
    );
    assert!(
        answer.set_cookies().is_empty(),
        "a valid session sets nothing"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_the_database_holds_no_plaintext_token_after_a_sign_in(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.sign_in().await;
    let token = token_of(&started);
    let raw = base64_url_decode(&token);

    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text, data_type::text
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'",
    )
    .fetch_all(&fixture.pool)
    .await
    .unwrap();
    assert!(
        columns
            .iter()
            .any(|(t, c, _)| t == "sessions" && c == "token_hash"),
        "the scan reaches the sessions table"
    );
    for (table, column, data_type) in &columns {
        // The identifiers come from the catalog and are double-quoted.
        let sql = if data_type == "bytea" {
            format!(
                "SELECT count(*) FROM \"{table}\" WHERE position($1::bytea IN \"{column}\") > 0
                    OR position($2::bytea IN \"{column}\") > 0"
            )
        } else {
            format!("SELECT count(*) FROM \"{table}\" WHERE position($3 IN \"{column}\"::text) > 0")
        };
        // Audited: the identifiers come from information_schema, never from a request.
        let hits: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(token.as_bytes())
            .bind(raw.as_slice())
            .bind(&token)
            .fetch_one(&fixture.pool)
            .await
            .unwrap_or_else(|e| panic!("{table}.{column}: {e}"));
        assert_eq!(hits, 0, "{table}.{column} holds the token");
    }
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(
        stored,
        stepclimb_auth::token::TokenHash::of(&token).as_bytes()
    );
}

fn base64_url_decode(text: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .expect("a token is base64url")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_a_cookie_planted_before_sign_in_is_never_promoted(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let planted = "__Host-sc_session=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    assert_eq!(
        fixture.whoami(Some(planted)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let started = fixture.sign_in_with(Some(planted)).await;

    assert_ne!(cookie_pair(&started), planted);
    let old = fixture.whoami(Some(planted)).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    assert_eq!(old.code(), "auth.unauthenticated");
    let new = fixture.whoami(Some(&cookie_pair(&started))).await;
    assert_eq!(new.status, StatusCode::OK);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn signing_in_again_ends_the_session_the_browser_presented(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let first = cookie_pair(&fixture.sign_in().await);
    let other_browser = cookie_pair(&fixture.sign_in().await);

    let second = fixture
        .sign_in_with(Some(&format!("theme=dark; {first}")))
        .await;

    let old = fixture.whoami(Some(&first)).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    assert_eq!(old.code(), "auth.unauthenticated", "the old row is gone");
    let new = fixture.whoami(Some(&cookie_pair(&second))).await;
    assert_eq!(new.status, StatusCode::OK);
    let other = fixture.whoami(Some(&other_browser)).await;
    assert_eq!(
        other.status,
        StatusCode::OK,
        "another browser's session stays"
    );
    assert_eq!(fixture.session_count().await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_non_ascii_neighbor_cookie_does_not_sign_the_user_out(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);
    let mut request = Request::get("/whoami");
    request = request.header(
        header::COOKIE,
        axum::http::HeaderValue::from_bytes(format!("theme=café; {cookie}").as_bytes())
            .expect("a header value may hold UTF-8 bytes"),
    );

    let response = fixture
        .app()
        .oneshot(request.body(Body::empty()).expect("a valid request"))
        .await
        .expect("the router never fails");

    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_twelve_idle_hours_answer_session_expired_and_clear_the_cookie(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);

    fixture.clock.advance(12 * HOUR - MINUTE);
    assert_eq!(
        fixture.whoami(Some(&cookie)).await.status,
        StatusCode::OK,
        "11h59m"
    );

    fixture.clock.advance(12 * HOUR);
    let answer = fixture.whoami(Some(&cookie)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.session_expired");
    answer.assert_cleared();
    assert_eq!(fixture.session_count().await, 0, "the row is gone");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_session_used_hourly_ends_at_thirty_days(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);

    for hour in 1..30 * 24 {
        fixture.clock.advance(HOUR);
        let answer = fixture.whoami(Some(&cookie)).await;
        assert_eq!(answer.status, StatusCode::OK, "hour {hour}");
    }
    fixture.clock.advance(HOUR);
    assert_eq!(fixture.clock.clock().now(), sign_in_time() + 30 * DAY);

    let answer = fixture.whoami(Some(&cookie)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.session_expired");
    answer.assert_cleared();
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_an_unknown_token_is_unauthenticated_and_cleared(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    fixture.sign_in().await;
    let unknown = stepclimb_auth::token::Token::generate().unwrap();

    let answer = fixture
        .whoami(Some(&format!(
            "__Host-sc_session={}",
            unknown.expose_secret()
        )))
        .await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.unauthenticated");
    answer.assert_cleared();
    assert!(
        !answer.body.contains(unknown.expose_secret()),
        "the body never repeats the token (B4)"
    );
    assert_eq!(fixture.session_count().await, 1, "the real session stays");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn without_a_cookie_the_route_answers_401_and_sets_nothing(pool: PgPool) {
    let fixture = Fixture::new(pool).await;

    let answer = fixture.whoami(None).await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.unauthenticated");
    assert!(answer.set_cookies().is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_session_cookies_authenticate_nothing(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);

    let answer = fixture.whoami(Some(&format!("{cookie}; {cookie}"))).await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.unauthenticated");
    answer.assert_cleared();
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_suspended_user_gets_auth_suspended_until_unsuspended(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);
    users::set_status(
        &fixture.pool,
        scope(fixture.org),
        fixture.user,
        Status::Suspended,
        sign_in_time(),
    )
    .await
    .unwrap();

    fixture.clock.advance(HOUR);
    let answer = fixture.whoami(Some(&cookie)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "auth.suspended");
    assert!(answer.set_cookies().is_empty(), "the cookie is kept");
    assert_eq!(fixture.session_count().await, 1, "the row is kept");

    users::set_status(
        &fixture.pool,
        scope(fixture.org),
        fixture.user,
        Status::Active,
        sign_in_time(),
    )
    .await
    .unwrap();
    assert_eq!(fixture.whoami(Some(&cookie)).await.status, StatusCode::OK);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b5_with_two_organizations_the_request_has_no_organization(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);
    insert_org(&fixture.pool, "org-b").await;

    let answer = fixture.whoami(Some(&cookie)).await;

    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(answer.code(), "org.not_found");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b5_without_an_organization_the_request_answers_org_none(pool: PgPool) {
    let fixture = Fixture {
        clock: ManualClock::new(sign_in_time()),
        org: new_id(),
        user: new_id(),
        pool,
    };

    let answer = fixture
        .whoami(Some(
            "__Host-sc_session=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        ))
        .await;

    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer.code(), "org.none");
}

/// `/whoami` with the session cookie `cookie` and an `Authorization` header.
async fn whoami_with_authorization(fixture: &Fixture, cookie: &str, authorization: &str) -> Answer {
    let request = Request::get("/whoami")
        .header(header::COOKIE, cookie)
        .header(header::AUTHORIZATION, authorization)
        .body(Body::empty())
        .expect("a valid request");
    let response = fixture
        .app()
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

/// T16 (B16): a bearer request is authenticated by the bearer only. No bearer credential is
/// accepted until API keys and app tokens exist (spec 08 I6, I8), so a valid session cookie
/// beside one authenticates nothing, and is neither cleared nor deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_bearer_header_is_never_answered_with_the_cookie_beside_it(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);

    for authorization in ["Bearer sc_someone-else_000000", "bearer x"] {
        let answer = whoami_with_authorization(&fixture, &cookie, authorization).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{authorization}");
        assert_eq!(answer.code(), "auth.unauthenticated");
        assert!(answer.set_cookies().is_empty(), "the cookie is not cleared");
    }
    assert_eq!(fixture.session_count().await, 1);
}

/// A `Basic` header (a proxy's, which the browser resends on its own) is not a credential of
/// Stepclimb's and does not hide the session cookie.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_basic_authorization_header_leaves_the_session_cookie_in_charge(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = cookie_pair(&fixture.sign_in().await);

    let answer = whoami_with_authorization(&fixture, &cookie, "Basic dTpw").await;

    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
}
